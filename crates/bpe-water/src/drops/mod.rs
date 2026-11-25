//! The all-drops engine trial (`docs/superpowers/plans/water-mode/all-drops-trial.md`): a body of
//! liquid with no grid pressure, every drop feeling its neighbours directly. [`DropsGpu`] holds
//! the pipelines (one per device), [`Drops`] one body. The same shape as [`crate::sim::Water`]:
//! drops come and go in the grid core's [`Drop`] layout, so fills, pours, probes and the bridge
//! are shared.
//!
//! A trial: not used by anything but tests.

use std::collections::VecDeque;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

pub mod numbers;

use crate::sim::drop::Drop;
use crate::sim::numbers::{Feel, MOST_LIQUIDS};
use crate::solid::{Layout, SolidField};
use numbers::{DropFeelGpu, DropNumbers, DropStepGpu};

/// The most drops a body holds: its drops' buffer (64 bytes a drop) stays within the 128 MiB a
/// storage buffer may be bound with on every card.
pub const MOST: u32 = 2_097_152;
const IN_FLIGHT: usize = 3;
const WORKGROUP: u32 = 256;
const BLOCK: u32 = 512;
const LEAST_ROOM: u32 = 4096;

/// A drop on the card. Matches `P` in alldrops.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct P {
    x: [f32; 2],
    v: [f32; 2],
    xp: [f32; 2],
    warm: [f32; 2],
    n: [f32; 2],
    lam: f32,
    rho: f32,
    kind: u32,
    serial: u32,
    colour: u32,
    spare: u32,
}

const STORE: wgpu::BufferUsages =
    wgpu::BufferUsages::STORAGE.union(wgpu::BufferUsages::COPY_DST).union(wgpu::BufferUsages::COPY_SRC);

/// A pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pass {
    Zero,
    Predict,
    SumA,
    SumB,
    SumC,
    Scatter,
    Order,
    Lambda,
    Apply,
    Finish,
    Export,
}

/// The entry points in [`Pass`]'s order and the bindings each uses: 0 the step, 1 the drops as
/// they were, 2 the drops sorted, 3 the cells' counts, 4 their starts, 5 the blocks' sums, 6 the
/// sorted order, 7 and 8 the places (in, out), 9 the multipliers, 10 the speeds, 11 the solid
/// field, 12 the drops in the grid core's layout.
const ENTRIES: &[(&str, &[u32])] = &[
    ("cs_zero", &[0, 3]),
    ("cs_predict", &[0, 1, 3, 11]),
    ("cs_sum_a", &[0, 3, 4, 5]),
    ("cs_sum_b", &[0, 5]),
    ("cs_sum_c", &[0, 3, 4, 5]),
    ("cs_scatter", &[0, 1, 3, 4, 6]),
    ("cs_order", &[0, 1, 2, 4, 6, 7, 9]),
    ("cs_lambda", &[0, 4, 7, 9, 11]),
    ("cs_apply", &[0, 2, 4, 7, 8, 9, 10, 11]),
    ("cs_finish", &[0, 2, 4, 7, 9, 10, 11]),
    ("cs_export", &[0, 1, 12]),
];

/// The all-drops engine's pipelines on one device.
pub struct DropsGpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    list: Vec<(wgpu::ComputePipeline, wgpu::BindGroupLayout)>,
}

impl std::fmt::Debug for DropsGpu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DropsGpu")
    }
}

impl DropsGpu {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Arc<DropsGpu> {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("all drops"),
            // (`BPE_DROPS_SHADER=<file>` replaces the shader without building again: the trial's
            // tuning only.)
            source: wgpu::ShaderSource::Wgsl(
                std::env::var("BPE_DROPS_SHADER")
                    .ok()
                    .and_then(|path| std::fs::read_to_string(path).ok())
                    .map_or(include_str!("../shaders/alldrops.wgsl").into(), Into::into),
            ),
        });
        let list = ENTRIES
            .iter()
            .map(|(name, bindings)| {
                let entries: Vec<wgpu::BindGroupLayoutEntry> = bindings
                    .iter()
                    .map(|&b| wgpu::BindGroupLayoutEntry {
                        binding: b,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: if b == 0 {
                                wgpu::BufferBindingType::Uniform
                            } else {
                                wgpu::BufferBindingType::Storage { read_only: false }
                            },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    })
                    .collect();
                let group = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some(name),
                    entries: &entries,
                });
                let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some(name),
                    bind_group_layouts: &[Some(&group)],
                    immediate_size: 0,
                });
                let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(name),
                    layout: Some(&layout),
                    module: &module,
                    entry_point: Some(name),
                    compilation_options: Default::default(),
                    cache: None,
                });
                (pipeline, group)
            })
            .collect();
        Arc::new(DropsGpu { device: device.clone(), queue: queue.clone(), list })
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
}

fn storage(device: &wgpu::Device, label: &'static str, bytes: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: bytes.max(256),
        usage: STORE,
        mapped_at_creation: false,
    })
}

fn room_for(n: u32) -> u32 {
    n.max(LEAST_ROOM).next_power_of_two().min(MOST)
}

/// The buffers sized by how many drops there is room for.
struct ByDrop {
    /// The drops, twice: each step sorts them from one into the other.
    p: [wgpu::Buffer; 2],
    idx: wgpu::Buffer,
    y: [wgpu::Buffer; 2],
    lamb: wgpu::Buffer,
    velb: wgpu::Buffer,
    out: wgpu::Buffer,
}

impl ByDrop {
    fn new(device: &wgpu::Device, room: u32) -> ByDrop {
        let n = u64::from(room);
        let p = std::mem::size_of::<P>() as u64;
        ByDrop {
            p: [storage(device, "all drops a", n * p), storage(device, "all drops b", n * p)],
            idx: storage(device, "all drops order", n * 4),
            y: [storage(device, "all drops places a", n * 16), storage(device, "all drops places b", n * 16)],
            lamb: storage(device, "all drops multipliers", n * 16),
            velb: storage(device, "all drops speeds", n * 16),
            out: storage(device, "all drops out", n * std::mem::size_of::<Drop>() as u64),
        }
    }
}

/// One body of liquid on the card, all drops.
pub struct Drops {
    gpu: Arc<DropsGpu>,
    layout: Layout,
    numbers: DropNumbers,
    feels: Vec<Feel>,
    gravity: glam::Vec2,
    edges_shut: bool,
    count: u32,
    room: u32,
    steps: u32,
    /// The neighbour cells across and down.
    cells: (u32, u32),
    /// The density at rest and a drop's stiffness there, and the solids' share by distance.
    rest: (f32, f32),
    wall: [[f32; 4]; 17],
    uniform: wgpu::Buffer,
    solid: wgpu::Buffer,
    counts: wgpu::Buffer,
    starts: wgpu::Buffer,
    blocks: wgpu::Buffer,
    by: ByDrop,
    /// Which of the two drops' buffers holds the drops now.
    now: usize,
    /// Every pass's bind group, for each of the two drops' buffers and each way round of the
    /// places' buffers ([`Drops::group`]).
    groups: Vec<wgpu::BindGroup>,
    in_flight: VecDeque<wgpu::SubmissionIndex>,
}

impl std::fmt::Debug for Drops {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Drops({} drops, {} x {})", self.count, self.layout.cols, self.layout.rows)
    }
}

impl Drops {
    /// A body with no drops on `field`'s layout, in a course whose gravity is `gravity` (pixels a
    /// second squared). `feels[k]` is liquid `k`.
    pub fn new(
        gpu: Arc<DropsGpu>,
        field: &SolidField,
        gravity: glam::Vec2,
        numbers: DropNumbers,
        feels: &[Feel],
    ) -> Drops {
        let device = &gpu.device;
        let layout = field.layout;
        let reach = numbers.reach.max(0.5);
        let cells =
            (((layout.cols as f32 / reach).ceil() as u32).max(1), ((layout.rows as f32 / reach).ceil() as u32).max(1));
        let bins = u64::from(cells.0 * cells.1) + 1;
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("all drops step"),
            size: std::mem::size_of::<DropStepGpu>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let solid = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("all drops solid field"),
            contents: bytemuck::cast_slice(&field.in_cells()),
            usage: STORE,
        });
        let room = room_for(0);
        let mut feels: Vec<Feel> = feels.iter().copied().take(MOST_LIQUIDS).collect();
        if feels.is_empty() {
            feels.push(Feel::default());
        }
        let mut drops = Drops {
            layout,
            numbers,
            feels,
            gravity,
            edges_shut: true,
            count: 0,
            room,
            steps: 0,
            cells,
            rest: numbers.at_rest(),
            wall: DropNumbers::wall_table(),
            uniform,
            solid,
            counts: storage(device, "all drops counts", bins * 4),
            starts: storage(device, "all drops starts", (bins + 1) * 4),
            blocks: storage(device, "all drops blocks", bins.div_ceil(u64::from(BLOCK)) * 4),
            by: ByDrop::new(device, room),
            now: 0,
            groups: Vec::new(),
            in_flight: VecDeque::new(),
            gpu,
        };
        drops.bind();
        drops
    }

    /// Makes the bind groups (again, after a buffer was replaced).
    fn bind(&mut self) {
        let mut groups = Vec::with_capacity(ENTRIES.len() * 4);
        for k in 0..ENTRIES.len() {
            for now in 0..2 {
                for y_in in 0..2 {
                    groups.push(self.make_group(k, now, y_in));
                }
            }
        }
        self.groups = groups;
    }

    /// The bind group of `pass` when the drops are in buffer `now` and the places come from `y_in`.
    fn group(&self, pass: Pass, now: usize, y_in: usize) -> &wgpu::BindGroup {
        &self.groups[(pass as usize * 2 + now) * 2 + y_in]
    }

    fn make_group(&self, pass: usize, now: usize, y_in: usize) -> wgpu::BindGroup {
        let (name, bindings) = ENTRIES[pass];
        let entries: Vec<wgpu::BindGroupEntry> = bindings
            .iter()
            .map(|&b| {
                let buffer = match b {
                    0 => &self.uniform,
                    1 => &self.by.p[now],
                    2 => &self.by.p[1 - now],
                    3 => &self.counts,
                    4 => &self.starts,
                    5 => &self.blocks,
                    6 => &self.by.idx,
                    7 => &self.by.y[y_in],
                    8 => &self.by.y[1 - y_in],
                    9 => &self.by.lamb,
                    10 => &self.by.velb,
                    11 => &self.solid,
                    _ => &self.by.out,
                };
                wgpu::BindGroupEntry { binding: b, resource: buffer.as_entire_binding() }
            })
            .collect();
        self.gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(name),
            layout: &self.gpu.list[pass].1,
            entries: &entries,
        })
    }

    pub fn layout(&self) -> Layout {
        self.layout
    }

    pub fn numbers(&self) -> &DropNumbers {
        &self.numbers
    }

    pub fn set_numbers(&mut self, numbers: DropNumbers) {
        // (The reach sizes the cells: it stays as the body was made with.)
        self.numbers = DropNumbers { reach: self.numbers.reach, ..numbers };
    }

    pub fn gpu(&self) -> &Arc<DropsGpu> {
        &self.gpu
    }

    pub fn count(&self) -> u32 {
        self.count
    }

    pub fn steps(&self) -> u32 {
        self.steps
    }

    pub fn set_edges_shut(&mut self, shut: bool) {
        self.edges_shut = shut;
    }

    pub fn set_gravity(&mut self, gravity: glam::Vec2) {
        self.gravity = gravity;
    }

    /// Adds `born` to the drops (those past [`MOST`] are let go). Returns how many were added.
    pub fn add(&mut self, born: &[Drop]) -> u32 {
        let n = (born.len() as u32).min(MOST - self.count);
        if n == 0 {
            return 0;
        }
        let size = std::mem::size_of::<P>() as u64;
        if self.count + n > self.room {
            let room = room_for(self.count + n);
            let device = &self.gpu.device;
            let bigger = ByDrop::new(device, room);
            if self.count > 0 {
                let mut encoder =
                    device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("all drops grow") });
                encoder.copy_buffer_to_buffer(
                    &self.by.p[self.now],
                    0,
                    &bigger.p[self.now],
                    0,
                    u64::from(self.count) * size,
                );
                self.gpu.queue.submit([encoder.finish()]);
            }
            self.by = bigger;
            self.room = room;
            self.bind();
        }
        let ps: Vec<P> = born[..n as usize]
            .iter()
            .map(|d| P {
                x: d.x,
                v: d.v,
                xp: d.x,
                rho: 1.0,
                kind: d.kind,
                serial: d.serial,
                colour: d.colour,
                ..P::default()
            })
            .collect();
        self.gpu.queue.write_buffer(&self.by.p[self.now], u64::from(self.count) * size, bytemuck::cast_slice(&ps));
        self.count += n;
        n
    }

    fn bins(&self) -> u32 {
        self.cells.0 * self.cells.1 + 1
    }

    fn step_gpu(&self) -> DropStepGpu {
        let n = &self.numbers;
        let dt = n.dt();
        let cell = self.layout.cell;
        let g = self.gravity / cell;
        let (rho0, stiff) = self.rest;
        let mut feels = [DropFeelGpu::default(); MOST_LIQUIDS];
        for (k, f) in self.feels.iter().enumerate() {
            feels[k] = n.feel(f, cell, g.length());
        }
        let reach_px = n.reach * cell;
        DropStepGpu {
            grid: [self.layout.cols, self.layout.rows, self.count, u32::from(self.edges_shut)],
            cells: [self.cells.0, self.cells.1, self.cells.0 * self.cells.1, self.bins().div_ceil(BLOCK)],
            time: [dt, g.x, g.y, n.most_move / dt],
            k: [n.reach, rho0, n.eps.max(0.0) * stiff, n.relax],
            w: [n.warm.clamp(0.0, 1.0), n.wall_off, n.sub_move, n.wall_share.max(0.0)],
            s: [
                (n.visc.max(0.0) * dt * 20.0 / (reach_px * reach_px)).min(0.5),
                n.drag_reach.max(n.wall_off + 0.01),
                n.most_fix,
                n.warm_most,
            ],
            feels,
            wall: {
                // (The table's last two numbers are spare.)
                let mut wall = self.wall;
                wall[16][2] = n.tens.max(0.0);
                wall[16][3] = n.tens_low;
                wall
            },
        }
    }

    /// One liquid step (`1 / rate` seconds). Never waits for the card unless it is more than a few
    /// steps behind.
    pub fn step(&mut self) {
        let gpu = self.gpu.clone();
        let (device, queue) = (&gpu.device, &gpu.queue);
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&self.step_gpu()));
        let by_drop = self.count.max(1).div_ceil(WORKGROUP);
        let by_bin = self.bins().div_ceil(WORKGROUP);
        let by_block = self.bins().div_ceil(BLOCK).div_ceil(WORKGROUP);
        let now = self.now;
        let iters = self.numbers.iters.clamp(1, 8) as usize;
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("all drops step") });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("all drops step"),
                timestamp_writes: None,
            });
            let mut go = |p: Pass, y_in: usize, n: u32| {
                pass.set_pipeline(&gpu.list[p as usize].0);
                pass.set_bind_group(0, self.group(p, now, y_in), &[]);
                pass.dispatch_workgroups(n.max(1), 1, 1);
            };
            go(Pass::Zero, 0, by_bin);
            go(Pass::Predict, 0, by_drop);
            go(Pass::SumA, 0, by_block);
            go(Pass::SumB, 0, 1);
            go(Pass::SumC, 0, by_bin);
            go(Pass::Scatter, 0, by_drop);
            go(Pass::Order, 0, by_drop);
            for k in 0..iters {
                go(Pass::Lambda, k % 2, by_drop);
                go(Pass::Apply, k % 2, by_drop);
            }
            go(Pass::Finish, iters % 2, by_drop);
        }
        self.in_flight.push_back(queue.submit([encoder.finish()]));
        self.now = 1 - now;
        self.steps += 1;
        let _ = device.poll(wgpu::PollType::Poll);
        if self.in_flight.len() > IN_FLIGHT
            && let Some(oldest) = self.in_flight.pop_front()
        {
            let _ = device.poll(wgpu::PollType::Wait { submission_index: Some(oldest), timeout: None });
        }
    }

    /// A copy that plays on exactly as this body does.
    pub fn copy(&self) -> Drops {
        let field = SolidField { layout: self.layout, distance: vec![0.0; self.layout.nodes()] };
        let mut new = Drops::new(self.gpu.clone(), &field, self.gravity, self.numbers, &self.feels);
        let device = &self.gpu.device;
        if self.room > new.room {
            new.by = ByDrop::new(device, self.room);
            new.room = self.room;
            new.bind();
        }
        new.now = self.now;
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("all drops copy") });
        if self.count > 0 {
            let bytes = u64::from(self.count) * std::mem::size_of::<P>() as u64;
            encoder.copy_buffer_to_buffer(&self.by.p[self.now], 0, &new.by.p[new.now], 0, bytes);
        }
        encoder.copy_buffer_to_buffer(&self.solid, 0, &new.solid, 0, self.solid.size());
        self.gpu.queue.submit([encoder.finish()]);
        new.count = self.count;
        new.steps = self.steps;
        new.edges_shut = self.edges_shut;
        new
    }

    /// Writes the drops in the grid core's layout into the buffer the bridge and [`Drops::read`]
    /// take them from (the drops as they are after every step queued so far).
    pub fn export(&self) -> &wgpu::Buffer {
        let (device, queue) = (&self.gpu.device, &self.gpu.queue);
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&self.step_gpu()));
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("all drops out") });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("all drops out"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.gpu.list[Pass::Export as usize].0);
            pass.set_bind_group(0, self.group(Pass::Export, self.now, 0), &[]);
            pass.dispatch_workgroups(self.count.max(1).div_ceil(WORKGROUP), 1, 1);
        }
        queue.submit([encoder.finish()]);
        &self.by.out
    }

    /// A buffer the bridge may bind where it reads nothing.
    pub(crate) fn spare(&self) -> &wgpu::Buffer {
        &self.by.lamb
    }

    /// Every drop, once the steps sent so far are done (its `c[0]` is how full its neighbourhood
    /// is). Waits for the card: for tests and measuring, never for a step.
    pub fn read(&self) -> Vec<Drop> {
        if self.count == 0 {
            return Vec::new();
        }
        let out = self.export();
        let device = &self.gpu.device;
        let bytes = u64::from(self.count) * std::mem::size_of::<Drop>() as u64;
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("all drops read"),
            size: bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("all drops read") });
        encoder.copy_buffer_to_buffer(out, 0, &staging, 0, bytes);
        self.gpu.queue.submit([encoder.finish()]);
        staging.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        bytemuck::pod_collect_to_vec(&staging.slice(..).get_mapped_range().expect("mapped"))
    }

    /// Waits until the card has done every step sent so far (for timing).
    pub fn wait(&mut self) {
        if let Some(last) = self.in_flight.pop_back() {
            let _ = self.gpu.device.poll(wgpu::PollType::Wait { submission_index: Some(last), timeout: None });
        }
        self.in_flight.clear();
    }
}

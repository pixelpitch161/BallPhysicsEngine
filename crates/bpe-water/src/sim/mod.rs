//! The liquid's physics on the card: [`WaterGpu`] (one per device: the pipelines) and [`Water`]
//! (one body of liquid: its drops, its grid and what the pressure solve keeps from step to step).
//!
//! A step never waits for the card. Births come from the processor ([`Water::add`]), so the host
//! always knows how many drops there are.

use std::collections::VecDeque;
use std::sync::Arc;

use wgpu::util::DeviceExt;

pub mod drop;
pub mod numbers;
mod passes;

use crate::solid::{Layout, SolidField};
use drop::Drop;
use numbers::{Feel, FeelGpu, MOST_LEVELS, MOST_LIQUIDS, Numbers, StepGpu};
use passes::{ENTRIES, P, Pipelines, TICK, TICK_BYTES};

/// The most drops a body holds: its drops' buffer (48 bytes a drop) stays under the 128 MiB a
/// storage buffer may be bound with on every card (wgpu's default limits).
pub const MOST_DROPS: u32 = 2_500_000;
/// Liquid steps the card may be behind before the host waits for it.
const IN_FLIGHT: usize = 3;
const WORKGROUP: u32 = 256;
const TILE: u32 = 16;
/// The pass numbers the table of ticks holds (two a level of the pressure).
const TICKS: u32 = 2 * MOST_LEVELS as u32;
const LEAST_ROOM: u32 = 4096;

const STORE: wgpu::BufferUsages =
    wgpu::BufferUsages::STORAGE.union(wgpu::BufferUsages::COPY_DST).union(wgpu::BufferUsages::COPY_SRC);

/// The water's pipelines on one device.
pub struct WaterGpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipelines: Pipelines,
    /// The numbers 0 to [`TICKS`], `tick_stride` bytes apart: a pass is told which of a step's
    /// passes it is by the offset this is bound at.
    ticks: wgpu::Buffer,
    tick_stride: u32,
}

impl std::fmt::Debug for WaterGpu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WaterGpu")
    }
}

impl WaterGpu {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Arc<WaterGpu> {
        let pipelines = Pipelines::new(device);
        let tick_stride = device.limits().min_uniform_buffer_offset_alignment.max(TICK_BYTES as u32);
        let mut numbers = vec![0u8; tick_stride as usize * (TICKS as usize + 1)];
        for n in 0..=TICKS {
            let at = (n * tick_stride) as usize;
            numbers[at..at + 4].copy_from_slice(&n.to_le_bytes());
        }
        let ticks = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("water pass numbers"),
            contents: &numbers,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        Arc::new(WaterGpu { device: device.clone(), queue: queue.clone(), pipelines, ticks, tick_stride })
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
}

/// The pressure's levels for a grid of `cols` x `rows` nodes: (where it starts, columns, rows, 0),
/// each half the one before each way, down to a handful of cells.
fn levels(cols: u32, rows: u32) -> Vec<[u32; 4]> {
    let mut out = vec![[0, cols, rows, 0]];
    while out.len() < MOST_LEVELS {
        let [at, c, r, _] = out[out.len() - 1];
        if c.min(r) <= 4 {
            break;
        }
        out.push([at + c * r, c.div_ceil(2), r.div_ceil(2), 0]);
    }
    out
}

fn storage(device: &wgpu::Device, label: &'static str, bytes: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: bytes.max(256),
        usage: STORE,
        mapped_at_creation: false,
    })
}

/// One body of liquid on the card.
pub struct Water {
    gpu: Arc<WaterGpu>,
    layout: Layout,
    numbers: Numbers,
    feels: Vec<Feel>,
    /// The course's gravity, pixels a second squared.
    gravity: glam::Vec2,
    /// Whether the course's edge shuts the liquid in.
    edges_shut: bool,
    count: u32,
    room: u32,
    steps: u32,
    walls_put: bool,
    levels: Vec<[u32; 4]>,
    uniform: wgpu::Buffer,
    drops: wgpu::Buffer,
    solid: wgpu::Buffer,
    sums: wgpu::Buffer,
    wallc: wgpu::Buffer,
    vel: wgpu::Buffer,
    /// The pressure: part of the body's state (the sweeps start from it), so a copy copies it.
    cellp: wgpu::Buffer,
    coef: wgpu::Buffer,
    vel_out: wgpu::Buffer,
    hold: wgpu::Buffer,
    hold_out: wgpu::Buffer,
    groups: Vec<wgpu::BindGroup>,
    in_flight: VecDeque<wgpu::SubmissionIndex>,
}

impl std::fmt::Debug for Water {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Water({} drops, {} x {})", self.count, self.layout.cols, self.layout.rows)
    }
}

fn room_for(n: u32) -> u32 {
    n.max(LEAST_ROOM).next_power_of_two().min(MOST_DROPS)
}

impl Water {
    /// A body with no drops on `field`'s grid, in a course whose gravity is `gravity` (pixels a
    /// second squared). `feels[k]` is liquid `k`.
    pub fn new(gpu: Arc<WaterGpu>, field: &SolidField, gravity: glam::Vec2, numbers: Numbers, feels: &[Feel]) -> Water {
        let device = &gpu.device;
        let layout = field.layout;
        let cells = layout.nodes() as u64;
        let levels = levels(layout.cols, layout.rows);
        let every = levels.iter().map(|l| u64::from(l[1] * l[2])).sum::<u64>();
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("water step"),
            size: std::mem::size_of::<StepGpu>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let solid = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("water solid field"),
            contents: bytemuck::cast_slice(&field.in_cells()),
            usage: STORE,
        });
        let room = room_for(0);
        let mut water = Water {
            layout,
            numbers,
            feels: feels.iter().copied().take(MOST_LIQUIDS).collect(),
            gravity,
            edges_shut: true,
            count: 0,
            room,
            steps: 0,
            walls_put: false,
            levels,
            uniform,
            drops: storage(device, "water drops", u64::from(room) * std::mem::size_of::<Drop>() as u64),
            solid,
            sums: storage(device, "water sums", cells * 24),
            wallc: storage(device, "water solids by cell", cells * 16),
            vel: storage(device, "water speeds", cells * 16),
            cellp: storage(device, "water pressure", every * 16),
            coef: storage(device, "water faces", every * 16),
            vel_out: storage(device, "water speeds out", cells * 16),
            hold: storage(device, "water hold sums", cells * 16),
            hold_out: storage(device, "water hold nodes", cells * 16),
            groups: Vec::new(),
            in_flight: VecDeque::new(),
            gpu,
        };
        if water.feels.is_empty() {
            water.feels.push(Feel::default());
        }
        water.bind();
        water
    }

    /// Makes the bind groups (again, after a buffer was replaced).
    fn bind(&mut self) {
        let gpu = &self.gpu;
        fn whole(b: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
            wgpu::BindGroupEntry { binding: b, resource: buffer.as_entire_binding() }
        }
        self.groups = ENTRIES
            .iter()
            .zip(&gpu.pipelines.list)
            .map(|((name, bindings), (_, layout))| {
                let entries: Vec<wgpu::BindGroupEntry> = bindings
                    .iter()
                    .map(|&b| match b {
                        0 => whole(0, &self.uniform),
                        1 => whole(1, &self.drops),
                        2 => whole(2, &self.solid),
                        3 => whole(3, &self.sums),
                        4 => whole(4, &self.wallc),
                        5 => whole(5, &self.vel),
                        6 => whole(6, &self.cellp),
                        7 => whole(7, &self.coef),
                        8 => whole(8, &self.vel_out),
                        TICK => wgpu::BindGroupEntry {
                            binding: TICK,
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: &gpu.ticks,
                                offset: 0,
                                size: std::num::NonZeroU64::new(TICK_BYTES),
                            }),
                        },
                        10 => whole(10, &self.hold),
                        _ => whole(11, &self.hold_out),
                    })
                    .collect();
                gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(name),
                    layout,
                    entries: &entries,
                })
            })
            .collect();
    }

    pub fn layout(&self) -> Layout {
        self.layout
    }

    pub fn numbers(&self) -> &Numbers {
        &self.numbers
    }

    pub fn gpu(&self) -> &Arc<WaterGpu> {
        &self.gpu
    }

    /// How many drops there are.
    pub fn count(&self) -> u32 {
        self.count
    }

    /// Liquid steps taken.
    pub fn steps(&self) -> u32 {
        self.steps
    }

    /// Whether the course's edge shuts the liquid in (it does unless this is turned off).
    pub fn set_edges_shut(&mut self, shut: bool) {
        self.edges_shut = shut;
    }

    pub fn set_gravity(&mut self, gravity: glam::Vec2) {
        self.gravity = gravity;
    }

    /// The buffers the drawing bridge reads.
    pub(crate) fn buffers(&self) -> (&wgpu::Buffer, &wgpu::Buffer, &wgpu::Buffer) {
        (&self.drops, &self.wallc, &self.vel_out)
    }

    /// Adds `born` to the drops (those past [`MOST_DROPS`] are let go). Returns how many were added.
    pub fn add(&mut self, born: &[Drop]) -> u32 {
        let n = (born.len() as u32).min(MOST_DROPS - self.count);
        if n == 0 {
            return 0;
        }
        let size = std::mem::size_of::<Drop>() as u64;
        if self.count + n > self.room {
            let room = room_for(self.count + n);
            let device = &self.gpu.device;
            let bigger = storage(device, "water drops", u64::from(room) * size);
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("water grows") });
            if self.count > 0 {
                encoder.copy_buffer_to_buffer(&self.drops, 0, &bigger, 0, u64::from(self.count) * size);
            }
            self.gpu.queue.submit([encoder.finish()]);
            self.drops = bigger;
            self.room = room;
            self.bind();
        }
        self.gpu.queue.write_buffer(
            &self.drops,
            u64::from(self.count) * size,
            bytemuck::cast_slice(&born[..n as usize]),
        );
        self.count += n;
        n
    }

    fn step_gpu(&self) -> StepGpu {
        let n = &self.numbers;
        let dt = n.dt();
        let g = self.gravity / self.layout.cell;
        let mut lv = [[0u32; 4]; MOST_LEVELS];
        lv[..self.levels.len()].copy_from_slice(&self.levels);
        let mut feels = [FeelGpu::default(); MOST_LIQUIDS];
        for (k, f) in self.feels.iter().enumerate() {
            feels[k] = f.gpu(n, self.layout.cell);
        }
        StepGpu {
            grid: [self.layout.cols, self.layout.rows, self.count, self.levels.len() as u32],
            time: [dt, g.x, g.y, n.most_move / dt],
            flags: [self.steps, u32::from(self.edges_shut), 0, 0],
            a: [n.omega, n.push_out, n.most_out, n.face_floor],
            b: [n.fill, n.sub_move, n.scale, n.most_out_thin.max(0.0)],
            h: [n.hold_damp, n.hold_settle, n.hold_join, if n.hold { 1.0 } else { 0.0 }],
            h2: [n.hold_relax, n.hold_full, n.even.max(0.0), n.even_most.max(0.0)],
            lv,
            feels,
        }
    }

    /// One liquid step (`1 / rate` seconds). Never waits for the card unless it is more than a few
    /// steps behind.
    pub fn step(&mut self) {
        let gpu = self.gpu.clone();
        let (device, queue) = (&gpu.device, &gpu.queue);
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&self.step_gpu()));
        let (cols, rows) = (self.layout.cols, self.layout.rows);
        let cells = (cols * rows).div_ceil(WORKGROUP);
        let tiles = (cols.div_ceil(TILE), rows.div_ceil(TILE));
        let drop_groups = self.count.max(1).div_ceil(WORKGROUP);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("water step") });
        {
            let mut pass = encoder
                .begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("water step"), timestamp_writes: None });
            let groups = &self.groups;
            let mut go = |p: P, n: (u32, u32), number: Option<u32>| {
                pass.set_pipeline(&gpu.pipelines.list[p as usize].0);
                match number {
                    Some(k) => pass.set_bind_group(0, &groups[p as usize], &[k * gpu.tick_stride]),
                    None => pass.set_bind_group(0, &groups[p as usize], &[]),
                }
                pass.dispatch_workgroups(n.0.max(1), n.1.max(1), 1);
            };
            if !self.walls_put {
                go(P::Walls, tiles, None);
            }
            go(P::Clear, (cells, 1), None);
            // (Always: the hold nodes also hold the liquid round each drop, which the spacing reads.)
            go(P::HoldSpread, (drop_groups, 1), None);
            go(P::HoldGrid, tiles, None);
            go(P::Spread, (drop_groups, 1), None);
            go(P::Grid, tiles, None);
            let n = self.numbers;
            let top = self.levels.len() as u32 - 1;
            let size = |level: u32| {
                let [_, c, r, _] = self.levels[level as usize];
                (c.div_ceil(16), r.div_ceil(16))
            };
            let sweep = |go: &mut dyn FnMut(P, (u32, u32), Option<u32>), level: u32, times: u32| {
                for _ in 0..times {
                    go(P::Sor, size(level), Some(2 * level));
                    go(P::Sor, size(level), Some(2 * level + 1));
                }
            };
            for _ in 0..if top > 0 { n.cycles } else { 0 } {
                sweep(&mut go, 0, n.pre);
                for level in 1..=top {
                    go(P::Down, size(level), Some(2 * level));
                    sweep(&mut go, level, if level == top { n.coarse } else { n.pre });
                }
                for level in (0..top).rev() {
                    go(P::Up, size(level), Some(2 * level));
                    sweep(&mut go, level, n.post);
                }
            }
            sweep(&mut go, 0, n.iters);
            go(P::Proj, tiles, None);
            go(P::Slip, tiles, None);
            go(P::Gather, (drop_groups, 1), None);
        }
        self.in_flight.push_back(queue.submit([encoder.finish()]));
        self.walls_put = true;
        self.steps += 1;
        let _ = device.poll(wgpu::PollType::Poll);
        if self.in_flight.len() > IN_FLIGHT
            && let Some(oldest) = self.in_flight.pop_front()
        {
            let _ = device.poll(wgpu::PollType::Wait { submission_index: Some(oldest), timeout: None });
        }
    }

    /// A copy that plays on exactly as this body does: its drops and the pressure it keeps, as
    /// they will be once every step sent so far is done.
    pub fn copy(&self) -> Water {
        let field = SolidField { layout: self.layout, distance: vec![0.0; self.layout.nodes()] };
        let mut new = Water::new(self.gpu.clone(), &field, self.gravity, self.numbers, &self.feels);
        let device = &self.gpu.device;
        let size = std::mem::size_of::<Drop>() as u64;
        if self.room > new.room {
            new.drops = storage(device, "water drops", u64::from(self.room) * size);
            new.room = self.room;
            new.bind();
        }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("water copy") });
        if self.count > 0 {
            encoder.copy_buffer_to_buffer(&self.drops, 0, &new.drops, 0, u64::from(self.count) * size);
        }
        encoder.copy_buffer_to_buffer(&self.solid, 0, &new.solid, 0, self.solid.size());
        encoder.copy_buffer_to_buffer(&self.cellp, 0, &new.cellp, 0, self.cellp.size());
        self.gpu.queue.submit([encoder.finish()]);
        new.count = self.count;
        new.steps = self.steps;
        new.edges_shut = self.edges_shut;
        new
    }

    /// Waits for the card and reads `bytes` of `buffer` (tests and measuring only).
    fn read_buffer(&self, buffer: &wgpu::Buffer, bytes: u64) -> Vec<u8> {
        let device = &self.gpu.device;
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("water read"),
            size: bytes.max(4),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("water read") });
        encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, bytes.max(4));
        self.gpu.queue.submit([encoder.finish()]);
        staging.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        staging.slice(..).get_mapped_range().expect("mapped").to_vec()
    }

    /// Every drop, once the steps sent so far are done. Waits for the card: for tests and
    /// measuring, never for a step.
    pub fn read(&self) -> Vec<Drop> {
        if self.count == 0 {
            return Vec::new();
        }
        let bytes = self.read_buffer(&self.drops, u64::from(self.count) * std::mem::size_of::<Drop>() as u64);
        bytemuck::pod_collect_to_vec(&bytes)
    }

    /// The cells' numbers (tests): `which` 0 the pressure's records, 1 the speeds the drops
    /// gather, 2 the solids by cell, 3 the hold nodes.
    pub fn read_cells(&self, which: u32) -> Vec<[f32; 4]> {
        let from = match which {
            0 => &self.cellp,
            1 => &self.vel_out,
            2 => &self.wallc,
            _ => &self.hold_out,
        };
        bytemuck::pod_collect_to_vec(&self.read_buffer(from, from.size()))
    }
}

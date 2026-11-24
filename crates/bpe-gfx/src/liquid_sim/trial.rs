//! The Water mode's physics trial (throwaway, never merged; plan
//! `docs/superpowers/plans/water-mode/physics-trial.md`): a second core for the drops, which solves
//! the pressure instead of springing (`shaders/liquid_trial.wgsl`). A [`Body`](super::Body) made
//! while [`trial`] is on steps with it; everything else (births, mixing, the dead's removal, the
//! wet grid, drawing) is today's.

use std::sync::RwLock;

use bytemuck::{Pod, Zeroable};

use super::{STORE, entry};

/// The trial core's switch and its numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trial {
    /// Bodies made from now on step with the trial core.
    pub on: bool,
    /// Plain pressure sweeps a liquid step, after the cycles.
    pub iters: u32,
    /// Multigrid cycles a liquid step: each sweeps the cells, hands what they still lack to a
    /// grid half as fine (and that to the next, down to a handful of cells), then adds the
    /// coarser answers back on the way up. A pool's weight then reaches its floor in a cycle or
    /// two, where plain sweeps need about as many as the pool is cells deep.
    pub cycles: u32,
    /// Sweeps on a level on the way down, on the way up, and on the coarsest level.
    pub pre: u32,
    pub post: u32,
    pub coarse: u32,
    /// How much of a coarser level's answer is added.
    pub scale: f32,
    /// The sweeps' over-relaxation.
    pub omega: f32,
    /// The share of a cell's excess density pushed out a step, and the most a step.
    pub push_out: f32,
    pub most_out: f32,
    /// The share of a cell's room that may fill a step before it pushes back.
    pub fill: f32,
    /// The least mass on a face for it to have a speed (a drop's is 0.25).
    pub face_floor: f32,
    /// A sub-move's length, in cells.
    pub sub_move: f32,
    /// The most cells a drop moves in a liquid step.
    pub most_move: f32,
    /// Liquid steps a second (0: the Liquid thing's own).
    pub rate: u32,
}

impl Trial {
    pub const OFF: Trial = Trial {
        on: false,
        iters: 0,
        cycles: 2,
        pre: 2,
        post: 2,
        coarse: 16,
        scale: 1.0,
        omega: 1.0,
        push_out: 0.25,
        most_out: 0.1,
        fill: 0.25,
        face_floor: 0.005,
        sub_move: 0.4,
        most_move: 8.0,
        rate: 0,
    };

    /// The trial core with its usual numbers, each replaced by its `BPE_TRIAL_*` environment
    /// variable when that is set (ITERS, OMEGA, PUSH_OUT, MOST_OUT, FILL, FACE_FLOOR, SUB_MOVE,
    /// MOST_MOVE, RATE): tuning without building again.
    pub fn from_env() -> Trial {
        fn num<T: std::str::FromStr>(name: &str, or: T) -> T {
            std::env::var(format!("BPE_TRIAL_{name}")).ok().and_then(|s| s.parse().ok()).unwrap_or(or)
        }
        let d = Trial::OFF;
        Trial {
            on: true,
            iters: num("ITERS", d.iters),
            cycles: num("CYCLES", d.cycles),
            pre: num("PRE", d.pre),
            post: num("POST", d.post),
            coarse: num("COARSE", d.coarse),
            scale: num("SCALE", d.scale),
            omega: num("OMEGA", d.omega),
            push_out: num("PUSH_OUT", d.push_out),
            most_out: num("MOST_OUT", d.most_out),
            fill: num("FILL", d.fill),
            face_floor: num("FACE_FLOOR", d.face_floor),
            sub_move: num("SUB_MOVE", d.sub_move),
            most_move: num("MOST_MOVE", d.most_move),
            rate: num("RATE", d.rate),
        }
    }
}

static TRIAL: RwLock<Trial> = RwLock::new(Trial::OFF);

/// Sets the trial core's switch and numbers for the bodies made from now on.
pub fn set_trial(t: Trial) {
    if let Ok(mut now) = TRIAL.write() {
        *now = t;
    }
}

pub fn trial() -> Trial {
    TRIAL.read().map(|t| *t).unwrap_or(Trial::OFF)
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct TrialGpu {
    n: [u32; 4],
    a: [f32; 4],
    b: [f32; 4],
    lv: [[u32; 4]; 12],
}

/// The pressure's levels for a grid of `cols` x `rows` nodes: (where it starts, columns, rows, 0),
/// each half the one before each way, down to a handful of cells.
fn levels(cols: u32, rows: u32) -> Vec<[u32; 4]> {
    let mut out = vec![[0, cols, rows, 0]];
    while out.len() < 12 {
        let [at, c, r, _] = out[out.len() - 1];
        if c.min(r) <= 4 {
            break;
        }
        out.push([at + c * r, c.div_ceil(2), r.div_ceil(2), 0]);
    }
    out
}

/// The trial's entry points and the bindings each uses: 0 the step, 1 drops, 3 counts, 5 the walls
/// band, 19 the pass's number, 20 the trial's numbers, 21 its sums, 22 the walls by cell, 23 the
/// faces' speeds, 24 the pressure, 25 the faces' open shares, 26 the nodes for the wet grid, 27 the
/// speeds the drops gather.
const ENTRIES: &[(&str, &[u32])] = &[
    ("cs2_walls", &[0, 5, 22]),
    ("cs2_clear", &[0, 21]),
    ("cs2_p2g", &[0, 1, 3, 5, 21, 22]),
    ("cs2_grid", &[0, 20, 21, 22, 23, 24, 25]),
    ("cs2_sor", &[19, 20, 24, 25]),
    ("cs2_down", &[19, 20, 24, 25]),
    ("cs2_up", &[19, 20, 24]),
    ("cs2_proj", &[0, 21, 22, 23, 24, 26]),
    ("cs2_slip", &[0, 5, 22, 23, 27]),
    ("cs2_g2p", &[0, 1, 3, 5, 20, 22, 27]),
];

#[derive(Clone, Copy)]
pub(super) enum T {
    Walls,
    Clear,
    P2g,
    Grid,
    Sor,
    Down,
    Up,
    Proj,
    Slip,
    G2p,
}

pub(super) struct TrialPipelines {
    pipelines: Vec<(wgpu::ComputePipeline, wgpu::BindGroupLayout)>,
}

impl TrialPipelines {
    pub(super) fn new(device: &wgpu::Device) -> TrialPipelines {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("liquid trial"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/liquid_trial.wgsl").into()),
        });
        let pipelines = ENTRIES
            .iter()
            .map(|(name, bindings)| {
                let entries: Vec<wgpu::BindGroupLayoutEntry> = bindings
                    .iter()
                    .map(|&b| wgpu::BindGroupLayoutEntry {
                        binding: b,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: match b {
                            20 => super::UNIFORM,
                            _ => super::binding_type(b),
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
        TrialPipelines { pipelines }
    }
}

/// The trial core's buffers of one body. `pressure` is part of the body's state (the sweeps start
/// from it), so a copy of the body copies it; the rest is worked out afresh each step.
pub(super) struct TrialScratch {
    pub(super) numbers: Trial,
    levels: Vec<[u32; 4]>,
    uniform: wgpu::Buffer,
    sums: wgpu::Buffer,
    wallc: wgpu::Buffer,
    vel: wgpu::Buffer,
    pub(super) pressure: wgpu::Buffer,
    coef: wgpu::Buffer,
    /// The liquid at each node as today's sums have it, for the wet grid's pass.
    pub(super) nodes: wgpu::Buffer,
    vel_out: wgpu::Buffer,
    /// The bind groups by entry point, and the buffers of the body they were made for.
    bound: Option<(Vec<wgpu::Buffer>, Vec<wgpu::BindGroup>)>,
}

impl TrialScratch {
    pub(super) fn new(device: &wgpu::Device, (cols, rows): (u32, u32), numbers: Trial) -> TrialScratch {
        let levels = levels(cols, rows);
        let every = levels.iter().map(|l| (l[1] * l[2]) as u64).sum::<u64>();
        let cells = cols * rows;
        let fixed = |label, bytes: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes.max(256),
                usage: STORE.union(wgpu::BufferUsages::COPY_SRC),
                mapped_at_creation: false,
            })
        };
        let cells = cells as u64;
        TrialScratch {
            numbers,
            levels,
            uniform: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("liquid trial"),
                size: std::mem::size_of::<TrialGpu>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            sums: fixed("trial sums", cells * 24),
            wallc: fixed("trial walls", cells * 32),
            vel: fixed("trial speeds", cells * 16),
            pressure: fixed("trial pressure", every * 16),
            coef: fixed("trial faces", every * 16),
            nodes: fixed("trial nodes", cells * 16),
            vel_out: fixed("trial speeds out", cells * 16),
            bound: None,
        }
    }

    /// A copy for a copy of the body: the pressure as it will be once every queued step is done.
    pub(super) fn copy(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        grid: (u32, u32),
    ) -> TrialScratch {
        let new = TrialScratch::new(device, grid, self.numbers);
        encoder.copy_buffer_to_buffer(&self.pressure, 0, &new.pressure, 0, self.pressure.size());
        new
    }

    pub(super) fn bytes(&self) -> usize {
        self.pressure.size() as usize
    }

    /// Writes the trial's numbers and makes the bind groups for a body whose step uniform, drops,
    /// counts and walls band are `step`, `drops`, `counts` and `solid`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipes: &TrialPipelines,
        ticks: &wgpu::Buffer,
        step: &wgpu::Buffer,
        drops: &wgpu::Buffer,
        counts: &wgpu::Buffer,
        solid: &wgpu::Buffer,
    ) {
        let t = self.numbers;
        let mut lv = [[0u32; 4]; 12];
        lv[..self.levels.len()].copy_from_slice(&self.levels);
        let numbers = TrialGpu {
            n: [t.iters, self.levels.len() as u32, 0, 0],
            a: [t.omega, t.push_out, t.most_out, t.face_floor],
            b: [t.fill, t.sub_move, t.scale, 0.0],
            lv,
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&numbers));
        let key = vec![step.clone(), drops.clone(), counts.clone(), solid.clone()];
        if self.bound.as_ref().is_some_and(|(k, _)| *k == key) {
            return;
        }
        let tick = wgpu::BindGroupEntry {
            binding: 19,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: ticks,
                offset: 0,
                size: std::num::NonZeroU64::new(super::TICK_BYTES),
            }),
        };
        let groups = ENTRIES
            .iter()
            .zip(&pipes.pipelines)
            .map(|((name, bindings), (_, layout))| {
                let entries: Vec<wgpu::BindGroupEntry> = bindings
                    .iter()
                    .map(|&b| match b {
                        0 => entry(0, step),
                        1 => entry(1, drops),
                        3 => entry(3, counts),
                        5 => entry(5, solid),
                        19 => tick.clone(),
                        20 => entry(20, &self.uniform),
                        21 => entry(21, &self.sums),
                        22 => entry(22, &self.wallc),
                        23 => entry(23, &self.vel),
                        24 => entry(24, &self.pressure),
                        25 => entry(25, &self.coef),
                        26 => entry(26, &self.nodes),
                        _ => entry(27, &self.vel_out),
                    })
                    .collect();
                device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some(name), layout, entries: &entries })
            })
            .collect();
        self.bound = Some((key, groups));
    }

    /// Queues the trial core's passes of one liquid step on `pass`: the walls by cell when the
    /// band was just put, then (when the step has time in it) the sums, the grid, the sweeps, the
    /// correction and the drops' move.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn run(
        &self,
        pass: &mut wgpu::ComputePass<'_>,
        pipes: &TrialPipelines,
        tick_stride: u32,
        (cols, rows): (u32, u32),
        drop_groups: u32,
        put_walls: bool,
        moves: bool,
    ) {
        let Some((_, groups)) = self.bound.as_ref() else { return };
        let cells = (cols * rows).div_ceil(256);
        let tiles = (cols.div_ceil(16), rows.div_ceil(16));
        let mut go = |t: T, n: (u32, u32), number: Option<u32>| {
            pass.set_pipeline(&pipes.pipelines[t as usize].0);
            match number {
                Some(k) => pass.set_bind_group(0, &groups[t as usize], &[k * tick_stride]),
                None => pass.set_bind_group(0, &groups[t as usize], &[]),
            }
            pass.dispatch_workgroups(n.0.max(1), n.1.max(1), 1);
        };
        if put_walls {
            go(T::Walls, (cells, 1), None);
        }
        if !moves {
            return;
        }
        go(T::Clear, (cells, 1), None);
        go(T::P2g, (drop_groups, 1), None);
        go(T::Grid, tiles, None);
        let n = self.numbers;
        let top = self.levels.len() as u32 - 1;
        let size = |level: u32| {
            let [_, c, r, _] = self.levels[level as usize];
            (c.div_ceil(16), r.div_ceil(16))
        };
        let sweep = |go: &mut dyn FnMut(T, (u32, u32), Option<u32>), level: u32, times: u32| {
            for _ in 0..times {
                go(T::Sor, size(level), Some(2 * level));
                go(T::Sor, size(level), Some(2 * level + 1));
            }
        };
        for _ in 0..if top > 0 { n.cycles } else { 0 } {
            sweep(&mut go, 0, n.pre);
            for level in 1..=top {
                go(T::Down, size(level), Some(2 * level));
                sweep(&mut go, level, if level == top { n.coarse } else { n.pre });
            }
            for level in (0..top).rev() {
                go(T::Up, size(level), Some(2 * level));
                sweep(&mut go, level, n.post);
            }
        }
        sweep(&mut go, 0, n.iters);
        go(T::Proj, tiles, None);
        go(T::Slip, tiles, None);
        go(T::G2p, (drop_groups, 1), None);
    }

    /// The pressure (× dt) of every cell, row by row, once the queued steps are done (tests).
    pub(super) fn read_cells(&self, device: &wgpu::Device, queue: &wgpu::Queue, which: u32) -> Vec<[f32; 4]> {
        let from = match which {
            0 => &self.pressure,
            1 => &self.vel_out,
            2 => &self.coef,
            _ => &self.vel,
        };
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("trial read"),
            size: from.size(),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("trial read") });
        encoder.copy_buffer_to_buffer(from, 0, &staging, 0, from.size());
        queue.submit([encoder.finish()]);
        staging.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        bytemuck::cast_slice::<u8, [f32; 4]>(&staging.slice(..).get_mapped_range().expect("mapped")).to_vec()
    }
}

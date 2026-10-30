//! The swarm layer: up to a million points moved by compute shaders and drawn as glowing dots.

use bytemuck::{Pod, Zeroable};

use super::inputs::{Inputs, STEP_WGSL};
use super::sdf::Sdf;
use super::util::{self, groups};
use crate::frame::{Flock, SwarmLayer, SwarmStart};

/// Size of a flocking grid cell in world pixels.
pub const CELL: f32 = 24.0;
/// The swarm's look.follow when every point is the one colour.
const ONE_COLOUR: u32 = 5;

/// Matches `struct Params` in swarm.wgsl and swarm_draw.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
struct SwarmParams {
    counts: [u32; 4],
    motion: [f32; 4],
    steer: [f32; 4],
    more: [f32; 4],
    look: [u32; 4],
    arena: [f32; 4],
    draw: [f32; 4],
    color: [f32; 4],
    /// The paint, as `PaintGpu`'s fields in order.
    paint_a: [f32; 4],
    paint_b: [f32; 4],
    /// Direction in degrees, span, centre.
    place: [f32; 4],
}

impl SwarmParams {
    fn new(layer: &SwarmLayer, cells: (u32, u32), arena_size: f32) -> Self {
        let m = &layer.motion;
        let p = &layer.look.paint;
        // With no row there is nothing to paint with: every point (but those near a ball) takes
        // the one colour.
        let follow = match layer.look.follow.min(ONE_COLOUR) {
            1 => 1,
            _ if p.row < 0.0 => ONE_COLOUR,
            f => f,
        };
        let start = match layer.start {
            SwarmStart::Fill => 0,
            SwarmStart::Centre => 1,
            SwarmStart::Ring => 2,
        };
        Self {
            counts: [layer.count, cells.0, cells.1, layer.seed],
            motion: [m.top_speed.max(1.0), m.drag.max(0.0), m.flow.max(0.0), m.swirl_size.max(1.0)],
            steer: [m.swirl_speed.max(0.0), m.flock.max(0.0), m.separation.max(0.0), m.attraction],
            more: [m.reach.max(0.0), m.avoid_walls.max(0.0), m.gravity_scale, CELL],
            look: [follow, 0, start, 0],
            arena: [layer.center[0], layer.center[1], arena_size.max(1.0), layer.look.size.max(0.0)],
            draw: [layer.look.brightness.max(0.0), layer.release_seconds.max(0.0), 0.0, 0.0],
            color: layer.look.color,
            paint_a: [p.row, p.t0, p.pattern, p.steps],
            paint_b: [p.angle, p.scale, p.scroll, p.repeat],
            place: layer.look.place,
        }
    }
}

/// The part of a swarm that needs a fresh start when it changes; the look does not.
#[derive(Clone, Debug, PartialEq)]
pub struct SwarmSim {
    pub count: u32,
    pub start: SwarmStart,
    pub center: [f32; 2],
    pub seed: u32,
    pub release_seconds: f32,
    pub motion: Flock,
}

impl SwarmSim {
    pub fn of(layer: &SwarmLayer) -> Self {
        Self {
            count: layer.count,
            start: layer.start,
            center: layer.center,
            seed: layer.seed,
            release_seconds: layer.release_seconds,
            motion: layer.motion,
        }
    }
}

/// Flocking grid cells across and down the video.
fn cells(video: (u32, u32)) -> (u32, u32) {
    ((video.0.max(1) as f32 / CELL).ceil() as u32, (video.1.max(1) as f32 / CELL).ceil() as u32)
}

/// The pipelines every swarm shares.
pub struct SwarmPipelines {
    layout: wgpu::BindGroupLayout,
    draw_layout: wgpu::BindGroupLayout,
    init: wgpu::ComputePipeline,
    clear: wgpu::ComputePipeline,
    fill: wgpu::ComputePipeline,
    advance: wgpu::ComputePipeline,
    draw: wgpu::RenderPipeline,
}

impl SwarmPipelines {
    pub fn new(
        device: &wgpu::Device,
        inputs_layout: &wgpu::BindGroupLayout,
        sdf_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        use wgpu::ShaderStages as S;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("swarm layout"),
            entries: &[
                util::uniform_entry(0, S::COMPUTE, false),
                util::storage_entry(1, S::COMPUTE, false),
                util::storage_entry(2, S::COMPUTE, false),
                util::storage_entry(3, S::COMPUTE, false),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("swarm"),
            bind_group_layouts: &[Some(inputs_layout), Some(&layout), Some(sdf_layout)],
            immediate_size: 0,
        });
        let module = util::shader(
            device,
            "swarm",
            &[include_str!("../shaders/flow/common.wgsl"), STEP_WGSL, include_str!("../shaders/flow/swarm.wgsl")],
        );
        let compute = |entry| util::compute_pipeline(device, entry, &pipeline_layout, &module, entry);

        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("swarm draw layout"),
            entries: &[
                util::uniform_entry(0, S::VERTEX, false),
                util::storage_entry(1, S::VERTEX, true),
                util::storage_entry(2, S::VERTEX, true),
            ],
        });
        let globals = util::globals_layout(device);
        let paint = crate::gradients::paint_layout(device);
        let draw_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("swarm draw"),
            bind_group_layouts: &[Some(&globals), Some(&draw_layout), Some(&paint)],
            immediate_size: 0,
        });
        let draw_module = util::shader(
            device,
            "swarm draw",
            &[&crate::gradients::paint_wgsl(2), include_str!("../shaders/flow/swarm_draw.wgsl")],
        );
        let draw = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("swarm draw"),
            layout: Some(&draw_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &draw_module,
                entry_point: Some("vs_point"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &draw_module,
                entry_point: Some("fs_point"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: crate::HDR_FORMAT,
                    blend: Some(util::ADDITIVE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            init: compute("init_points"),
            clear: compute("clear_field"),
            fill: compute("fill_field"),
            advance: compute("move_points"),
            layout,
            draw_layout,
            draw,
        }
    }
}

/// One swarm's points on the GPU.
pub struct SwarmGpu {
    sim: SwarmSim,
    cells: (u32, u32),
    arena_size: f32,
    params: wgpu::Buffer,
    draw_params: wgpu::Buffer,
    points: wgpu::Buffer,
    tints: wgpu::Buffer,
    /// Holds the params, points, tints and the flocking grid (which lives only in the group).
    group: wgpu::BindGroup,
    draw_group: wgpu::BindGroup,
    last: SwarmParams,
    /// Whether steps fill the neighbour grid; motion settings only change with a restart.
    flocking: bool,
}

impl SwarmGpu {
    /// Makes the swarm's buffers and records its start into `encoder`. The inputs' slot 0 must
    /// hold step 0 and the field must be baked first, since the start reads both.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        pipelines: &SwarmPipelines,
        inputs: &Inputs,
        sdf: &Sdf,
        layer: &SwarmLayer,
        video: (u32, u32),
        arena_size: f32,
    ) -> Self {
        let sim = SwarmSim::of(layer);
        let cells = cells(video);
        let count = layer.count;
        let storage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST;
        let make = |label, size: u64, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size.max(16),
                usage,
                mapped_at_creation: false,
            })
        };
        let points = make("swarm points", count as u64 * 16, storage);
        let tints = make("swarm colours", count as u64 * 8, storage);
        let field = make("swarm grid", (cells.0 * cells.1 * 3 * 4) as u64, wgpu::BufferUsages::STORAGE);
        let uniform = wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST;
        let params = make("swarm params", std::mem::size_of::<SwarmParams>() as u64, uniform);
        let draw_params = make("swarm draw params", std::mem::size_of::<SwarmParams>() as u64, uniform);
        let last = SwarmParams::new(layer, cells, arena_size);
        queue.write_buffer(&params, 0, bytemuck::bytes_of(&last));
        queue.write_buffer(&draw_params, 0, bytemuck::bytes_of(&last));
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("swarm"),
            layout: &pipelines.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: params.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: points.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: tints.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: field.as_entire_binding() },
            ],
        });
        let draw_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("swarm draw"),
            layout: &pipelines.draw_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: draw_params.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: points.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: tints.as_entire_binding() },
            ],
        });
        let flocking = last.steer[1] > 0.0 || last.steer[2] > 0.0;
        let mut pass = encoder
            .begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("swarm start"), timestamp_writes: None });
        pass.set_pipeline(&pipelines.init);
        pass.set_bind_group(0, &inputs.group, &[0]);
        pass.set_bind_group(1, &group, &[]);
        pass.set_bind_group(2, sdf.bind_group(), &[]);
        pass.dispatch_workgroups(groups(count, 256), 1, 1);
        drop(pass);
        Self { sim, cells, arena_size, params, draw_params, points, tints, group, draw_group, last, flocking }
    }

    pub fn sim(&self) -> &SwarmSim {
        &self.sim
    }

    /// Takes the layer's look (the rest is the same, or the swarm would have been remade).
    pub fn set_look(&mut self, queue: &wgpu::Queue, layer: &SwarmLayer) {
        let params = SwarmParams::new(layer, self.cells, self.arena_size);
        if params != self.last {
            queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&params));
            queue.write_buffer(&self.draw_params, 0, bytemuck::bytes_of(&params));
            self.last = params;
        }
    }

    /// Records one step, reading the step inputs at `offset`.
    pub fn step(
        &self,
        pipelines: &SwarmPipelines,
        encoder: &mut wgpu::CommandEncoder,
        inputs: &Inputs,
        offset: u32,
        sdf: &Sdf,
    ) {
        let count = self.sim.count;
        let mut pass = encoder
            .begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("swarm step"), timestamp_writes: None });
        pass.set_bind_group(0, &inputs.group, &[offset]);
        pass.set_bind_group(1, &self.group, &[]);
        pass.set_bind_group(2, sdf.bind_group(), &[]);
        if self.flocking {
            pass.set_pipeline(&pipelines.clear);
            pass.dispatch_workgroups(groups(self.cells.0 * self.cells.1 * 3, 256), 1, 1);
            pass.set_pipeline(&pipelines.fill);
            pass.dispatch_workgroups(groups(count, 256), 1, 1);
        }
        pass.set_pipeline(&pipelines.advance);
        pass.dispatch_workgroups(groups(count, 256), 1, 1);
    }

    /// The buffers a checkpoint saves: the grid is rebuilt every step.
    pub fn state(&self) -> Vec<&wgpu::Buffer> {
        vec![&self.points, &self.tints]
    }

    /// Draws the points, reading the frame's gradients from `paint`.
    pub fn draw(
        &self,
        pipelines: &SwarmPipelines,
        pass: &mut wgpu::RenderPass<'_>,
        globals: &wgpu::BindGroup,
        paint: &wgpu::BindGroup,
    ) {
        if self.sim.count == 0 || self.last.draw[0] <= 0.0 {
            return;
        }
        pass.set_pipeline(&pipelines.draw);
        pass.set_bind_group(0, globals, &[]);
        pass.set_bind_group(1, &self.draw_group, &[]);
        pass.set_bind_group(2, paint, &[]);
        pass.draw(0..4, 0..self.sim.count);
    }

    /// Reads the points back (position, velocity), for tests.
    #[cfg(test)]
    pub fn read_points(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<[f32; 4]> {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("swarm readback"),
            size: self.points.size(),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&self.points, 0, &buffer, 0, self.points.size());
        queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let data = buffer.slice(..).get_mapped_range().unwrap();
        let values: &[[f32; 4]] = bytemuck::cast_slice(&data);
        values.iter().take(self.sim.count as usize).copied().collect()
    }
}

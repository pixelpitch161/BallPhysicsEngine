//! Liquid part 3: the wet grid the Wet walls look draws. Each body keeps, on the card, how wet each
//! grid node near a wall is (`shaders/liquid_wet.wgsl`): 1 where the liquid is on it, drying
//! (`exp(−t / DRY_SECONDS)`) and running slowly down the wall once it has gone. It is two halves
//! (one read, one written, swapped each liquid step), copied with the body, and never read back:
//! only the draw reads it, so it moves nothing and needs no fingerprint.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};

/// How long the wet grid takes to dry to 1/e, in seconds. The look's Dries in reads it as a
/// threshold (`shown = (wet − e) / (1 − e)`, `e = exp(−dries / DRY_SECONDS)`), so it can pick
/// any drying time up to a few times this.
pub const DRY_SECONDS: f32 = 6.0;
/// How far a dry node's wetness moves towards (most of) the node above it, a second.
const DRIP_PER_SECOND: f32 = 6.0;
/// How much mass on a node wets it (a node under resting water at Weight 1 has about 1: four
/// drops of volume 1/4), and the grid's fixed-point scale for mass (`S_M` in liquid_sim.wgsl).
const WETS_FROM: f32 = 0.05;
const S_M: f32 = 1048576.0;
/// How many floats a node the wet grid keeps (liquid_wet.wgsl).
const WET_FLOATS: u64 = 6;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
struct WetGpu {
    grid: [u32; 4],
    rates: [f32; 4],
}

/// The wet grid's pipeline on a card.
pub(super) struct WetPipeline {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
}

impl WetPipeline {
    pub(super) fn new(device: &wgpu::Device) -> WetPipeline {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("liquid wet"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/liquid_wet.wgsl").into()),
        });
        let storage = |binding, read_only| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("liquid wet"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                storage(1, true),
                storage(2, true),
                storage(3, false),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("liquid wet"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("liquid wet"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("cs_wet"),
            compilation_options: Default::default(),
            cache: None,
        });
        WetPipeline { pipeline, layout }
    }
}

/// A body's wet grid: `6 × cells` floats on the card (two halves of wetness, each node's face
/// distance and its side, then its signed distance from its wall's line and its band, which the
/// picture finds the drops' walls by; see liquid_wet.wgsl) and its small uniform.
pub(super) struct WetGrid {
    pub(super) buffer: wgpu::Buffer,
    uniform: wgpu::Buffer,
}

const USAGE: wgpu::BufferUsages =
    wgpu::BufferUsages::STORAGE.union(wgpu::BufferUsages::COPY_SRC).union(wgpu::BufferUsages::COPY_DST);

fn grid_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("liquid wet grid"),
        size,
        usage: USAGE,
        mapped_at_creation: false,
    })
}

fn uniform(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("liquid wet numbers"),
        size: std::mem::size_of::<WetGpu>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

impl WetGrid {
    /// A dry grid of `cells` nodes.
    pub(super) fn new(device: &wgpu::Device, cells: u32) -> WetGrid {
        // New buffers are zeroed: dry.
        WetGrid { buffer: grid_buffer(device, (cells as u64 * WET_FLOATS * 4).max(256)), uniform: uniform(device) }
    }

    /// A copy (queued on `encoder`).
    pub(super) fn copy(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder) -> WetGrid {
        let buffer = grid_buffer(device, self.buffer.size());
        encoder.copy_buffer_to_buffer(&self.buffer, 0, &buffer, 0, self.buffer.size());
        WetGrid { buffer, uniform: uniform(device) }
    }

    pub(super) fn bytes(&self) -> usize {
        self.buffer.size() as usize
    }

    /// Queues one wet pass on `encoder` after a liquid step of `dt` seconds, writing half `half`
    /// from the grid's masses `grid` and walls band `solid`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn run(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipeline: &Arc<WetPipeline>,
        encoder: &mut wgpu::CommandEncoder,
        (cols, rows): (u32, u32),
        half: u32,
        dt: f32,
        (grid, solid): (&wgpu::Buffer, &wgpu::Buffer),
    ) {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let numbers = WetGpu {
            grid: [cols, rows, half & 1, 0],
            rates: [(-dt / DRY_SECONDS).exp(), (dt * DRIP_PER_SECOND).clamp(0.0, 1.0), WETS_FROM * S_M, 0.0],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&numbers));
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("liquid wet"),
            layout: &pipeline.layout,
            entries: &[
                super::entry(0, &self.uniform),
                super::entry(1, grid),
                super::entry(2, solid),
                super::entry(3, &self.buffer),
            ],
        });
        let mut pass = encoder
            .begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("liquid wet"), timestamp_writes: None });
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups((cols * rows).div_ceil(256).max(1), 1, 1);
    }
}

impl super::Body {
    /// Each node's distance from its wall's line and its band (the wet grid's last two numbers a
    /// node, liquid_wet.wgsl), row by row, waiting for the card. For tests.
    pub fn read_walls(&self) -> (Vec<f32>, Vec<f32>) {
        let device = &self.gpu.device;
        let cells = (self.cols * self.rows) as u64;
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("liquid walls read"),
            size: 2 * cells * 4,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("liquid walls read") });
        encoder.copy_buffer_to_buffer(&self.wet.buffer, 4 * cells * 4, &staging, 0, 2 * cells * 4);
        self.gpu.queue.submit([encoder.finish()]);
        staging.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        let both = bytemuck::cast_slice::<u8, f32>(&staging.slice(..).get_mapped_range().expect("mapped")).to_vec();
        let (line, band) = both.split_at(cells as usize);
        (line.to_vec(), band.to_vec())
    }
}

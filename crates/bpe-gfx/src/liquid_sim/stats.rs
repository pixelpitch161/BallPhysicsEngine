//! What the sound reads of each liquid (liquid part 5): per liquid, how many drops, how fast they
//! go, how high they stand and how spread their colours are, summed on the card in integers
//! (`shaders/liquid_stats.wgsl`) when [`super::Body::send_feel`] sends the feel back, so they come
//! back with it ([`super::Feel::stats`]).

use std::sync::{Arc, Mutex};

use super::{MOST_LIQUIDS, WORKGROUP};

/// Words each liquid's sums take.
pub const STAT_WORDS: usize = 12;
/// Words every liquid's sums take together.
pub const STATS_LEN: usize = MOST_LIQUIDS * STAT_WORDS;

/// Where each sum is in a liquid's [`STAT_WORDS`] words.
pub mod stat_at {
    /// How many drops.
    pub const DROPS: usize = 0;
    /// Their speeds (grid units a second × 16, at most 2047 a drop).
    pub const SPEED: usize = 1;
    /// Their rows (grid units down from the top, at most 4094).
    pub const ROW: usize = 2;
    /// Their colour (linear rgb, 0..63 a channel), and its square.
    pub const COLOUR: usize = 3;
    pub const SQUARE: usize = 6;
}

struct Pipe {
    device: wgpu::Device,
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
}

static PIPES: Mutex<Vec<Arc<Pipe>>> = Mutex::new(Vec::new());

fn pipe(device: &wgpu::Device) -> Arc<Pipe> {
    let mut pipes = PIPES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(p) = pipes.iter().find(|p| p.device == *device) {
        return p.clone();
    }
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("liquid stats"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/liquid_stats.wgsl").into()),
    });
    let entry = |binding: u32, read_only: bool| wgpu::BindGroupLayoutEntry {
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
        label: Some("liquid stats"),
        entries: &[entry(0, true), entry(1, true), entry(2, false)],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("liquid stats"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("liquid stats"),
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some("cs_stats"),
        compilation_options: Default::default(),
        cache: None,
    });
    // Pipes of devices no longer used would only grow the list: a program has one or two.
    let p = Arc::new(Pipe { device: device.clone(), pipeline, layout });
    pipes.push(p.clone());
    p
}

/// Sums every liquid's drops (`bound` at most, as many as `counts[1]` says) into a new buffer,
/// on `encoder`, and returns it ([`STATS_LEN`] words, copy-from).
pub(super) fn encode(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    drops: &wgpu::Buffer,
    counts: &wgpu::Buffer,
    bound: u32,
) -> wgpu::Buffer {
    let p = pipe(device);
    let stats = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("liquid stats"),
        size: (STATS_LEN * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("liquid stats"),
        layout: &p.layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: drops.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: counts.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: stats.as_entire_binding() },
        ],
    });
    encoder.clear_buffer(&stats, 0, None);
    {
        let mut pass = encoder
            .begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("liquid stats"), timestamp_writes: None });
        pass.set_pipeline(&p.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(bound.max(1).div_ceil(WORKGROUP), 1, 1);
    }
    stats
}

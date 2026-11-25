//! The bridge to today's liquid drawing: the water's drops handed to bpe-gfx's drops pass and
//! surface pass as a [`DropsSource`], so the new core can be looked at (and filmed beside today's
//! core) before Water mode has its own drawing.
//!
//! Each [`Bridge::update`] rewrites the drops in today's layout on the card (`shaders/bridge.wgsl`).
//! The wet grid it hands over holds the solids as the drawing wants them (each node's distance from
//! the nearest face, every solid a filled wall: the physics' own field, so the liquid is drawn up to
//! the face the drops rest on) and no wetness: the Wet walls look shows nothing through the bridge.

use bpe_gfx::liquid_sim::DropsSource;
use bytemuck::{Pod, Zeroable};

use crate::sim::Water;
use crate::solid::SolidField;

/// Numbers one of today's drops takes (bpe-gfx's `DROP_FLOATS`).
const DROP_FLOATS: u64 = 28;
/// Numbers a node of today's wet grid takes.
const WET_FLOATS: usize = 6;
/// How far from a face (cells) a node still counts as in a wall's band.
const BAND: f32 = 6.0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct BridgeGpu {
    grid: [u32; 4],
}

/// A water's drops as today's drawing reads them.
pub struct Bridge {
    pipeline: wgpu::ComputePipeline,
    group_layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    out: wgpu::Buffer,
    counts: wgpu::Buffer,
    wet: wgpu::Buffer,
    room: u32,
    grid: (u32, u32),
}

impl std::fmt::Debug for Bridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Bridge(room for {})", self.room)
    }
}

fn out_buffer(device: &wgpu::Device, room: u32) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("water bridge drops"),
        size: (u64::from(room) * DROP_FLOATS * 4).max(256),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

impl Bridge {
    /// A bridge for `water`, whose solids are `field` (the one the water was made with).
    pub fn new(water: &Water, field: &SolidField) -> Bridge {
        use wgpu::util::DeviceExt;
        let device = water.gpu().device();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("water bridge"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/bridge.wgsl").into()),
        });
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let read = wgpu::BufferBindingType::Storage { read_only: true };
        let group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("water bridge"),
            entries: &[
                entry(0, wgpu::BufferBindingType::Uniform),
                entry(1, read),
                entry(2, read),
                entry(3, read),
                entry(4, wgpu::BufferBindingType::Storage { read_only: false }),
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("water bridge"),
            bind_group_layouts: &[Some(&group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("water bridge"),
            layout: Some(&layout),
            module: &module,
            entry_point: Some("cs_bridge"),
            compilation_options: Default::default(),
            cache: None,
        });
        let l = water.layout();
        let nodes = l.nodes();
        // Two halves of wetness (none), then each node's face distance, its side, its distance
        // from its wall's line and its band (1: a filled wall).
        let mut wet = vec![0.0f32; WET_FLOATS * nodes];
        for (n, d) in field.in_cells().iter().enumerate() {
            let near = *d < BAND;
            wet[2 * nodes + n] = if near { *d } else { 4.0 };
            wet[3 * nodes + n] = if near { 1.0 } else { 0.0 };
            wet[4 * nodes + n] = if near { *d } else { 0.0 };
            wet[5 * nodes + n] = if near { 1.0 } else { 0.0 };
        }
        let room = water.count().max(1024).next_power_of_two();
        Bridge {
            pipeline,
            group_layout,
            uniform: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("water bridge numbers"),
                size: std::mem::size_of::<BridgeGpu>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            out: out_buffer(device, room),
            counts: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("water bridge counts"),
                contents: bytemuck::cast_slice(&[4u32, 0, 0, 0, 0, 0, 0, 0]),
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::INDIRECT
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
            }),
            wet: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("water bridge wet grid"),
                contents: bytemuck::cast_slice(&wet),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            }),
            room,
            grid: (l.cols, l.rows),
        }
    }

    /// Writes the water's drops, as they are after every step queued so far, for the drawing.
    pub fn update(&mut self, water: &Water) {
        let gpu = water.gpu();
        let device = gpu.device();
        let count = water.count();
        if count > self.room {
            self.room = count.next_power_of_two();
            self.out = out_buffer(device, self.room);
        }
        let l = water.layout();
        gpu.queue().write_buffer(&self.uniform, 0, bytemuck::bytes_of(&BridgeGpu { grid: [l.cols, l.rows, count, 0] }));
        gpu.queue().write_buffer(&self.counts, 0, bytemuck::cast_slice(&[4u32, count, 0, 0]));
        if count == 0 {
            return;
        }
        let (drops, wallc, vel_out) = water.buffers();
        fn whole(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
            wgpu::BindGroupEntry { binding, resource: buffer.as_entire_binding() }
        }
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("water bridge"),
            layout: &self.group_layout,
            entries: &[
                whole(0, &self.uniform),
                whole(1, drops),
                whole(2, wallc),
                whole(3, vel_out),
                whole(4, &self.out),
            ],
        });
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("water bridge") });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("water bridge"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(count.div_ceil(256), 1, 1);
        }
        gpu.queue().submit([encoder.finish()]);
    }

    /// What the renderer draws the drops from (after [`Bridge::update`]).
    pub fn source(&self, water: &Water) -> DropsSource {
        DropsSource {
            device: water.gpu().device().clone(),
            drops: self.out.clone(),
            counts: self.counts.clone(),
            wet: self.wet.clone(),
            grid: self.grid,
            wet_half: 0,
        }
    }
}

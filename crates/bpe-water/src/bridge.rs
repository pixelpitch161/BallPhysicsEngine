//! The bridge to today's liquid drawing: the water's drops handed to bpe-gfx's drops pass and
//! surface pass as a [`DropsSource`], so either engine can be looked at (and filmed beside today's
//! core) before Water mode has its own drawing (part C3).
//!
//! Each [`Bridge::update`] rewrites the drops in today's layout on the card (`shaders/bridge.wgsl`),
//! from a body's [`Feed`] (`grid::Water::feed`, `drops::Water::feed`).
//! The wet grid it hands over holds the solids as the drawing wants them (each node's distance from
//! the nearest face, every solid a filled wall: the physics' own field, so the liquid is drawn up to
//! the face the drops rest on) and no wetness: the Wet walls look shows nothing through the bridge.

use bpe_gfx::liquid_sim::DropsSource;
use bytemuck::{Pod, Zeroable};

use crate::drops::DropBuffers;
use crate::solid::{Layout, SolidField};

/// Numbers one of today's drops takes (bpe-gfx's `DROP_FLOATS`).
const DROP_FLOATS: u64 = 28;
/// Numbers a node of today's wet grid takes.
const WET_FLOATS: usize = 6;
/// How far from a face (cells) a node still counts as in a wall's band.
const BAND: f32 = 6.0;

/// What the bridge reads of a body, whichever engine it is.
pub struct Feed<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub layout: Layout,
    pub count: u32,
    pub buffers: FeedBuffers<'a>,
}

/// A body's drops on the card.
pub enum FeedBuffers<'a> {
    /// The grid core's: its drops, its cells' solid shares, its cells' gathered liquid.
    Grid { drops: &'a wgpu::Buffer, wallc: &'a wgpu::Buffer, vel_out: &'a wgpu::Buffer },
    /// The all-drops engine's, stored by field (each drop holds how full it is).
    Drops(DropBuffers<'a>),
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct BridgeGpu {
    grid: [u32; 4],
}

/// A water's drops as today's drawing reads them.
pub struct Bridge {
    device: wgpu::Device,
    /// For the grid core and for the all-drops engine.
    pipelines: [(wgpu::ComputePipeline, wgpu::BindGroupLayout); 2],
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
    /// The most drops handed to the drawing (today's core's own cap: more would pass the card's
    /// limit on one buffer).
    pub const MOST: u32 = 1 << 20;

    /// A bridge on `device` for a body whose solids are `field` (the one it was made with).
    pub fn new(device: &wgpu::Device, field: &SolidField) -> Bridge {
        use wgpu::util::DeviceExt;
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
        let out = wgpu::BufferBindingType::Storage { read_only: false };
        let pipeline = |name: &str, bindings: &[(u32, wgpu::BufferBindingType)]| {
            let entries: Vec<_> = bindings.iter().map(|(b, ty)| entry(*b, *ty)).collect();
            let group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("water bridge"),
                entries: &entries,
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
                entry_point: Some(name),
                compilation_options: Default::default(),
                cache: None,
            });
            (pipeline, group_layout)
        };
        let uniform = wgpu::BufferBindingType::Uniform;
        let pipelines = [
            pipeline("cs_bridge", &[(0, uniform), (1, read), (2, read), (3, read), (4, out)]),
            pipeline("cs_bridge_drops", &[(0, uniform), (4, out), (5, read), (6, read), (7, read)]),
        ];
        let l = field.layout;
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
        let room = 1024;
        Bridge {
            device: device.clone(),
            pipelines,
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

    /// Writes the body's drops, as they are after everything queued so far, for the drawing.
    pub fn update(&mut self, feed: &Feed) {
        let device = feed.device;
        let count = feed.count.min(Bridge::MOST);
        if count > self.room {
            self.room = count.next_power_of_two();
            self.out = out_buffer(device, self.room);
        }
        let l = feed.layout;
        feed.queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&BridgeGpu { grid: [l.cols, l.rows, count, 0] }));
        feed.queue.write_buffer(&self.counts, 0, bytemuck::cast_slice(&[4u32, count, 0, 0]));
        if count == 0 {
            return;
        }
        fn whole(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
            wgpu::BindGroupEntry { binding, resource: buffer.as_entire_binding() }
        }
        let (k, entries) = match &feed.buffers {
            FeedBuffers::Grid { drops, wallc, vel_out } => (
                0,
                vec![whole(0, &self.uniform), whole(1, drops), whole(2, wallc), whole(3, vel_out), whole(4, &self.out)],
            ),
            FeedBuffers::Drops(d) => {
                (1, vec![whole(0, &self.uniform), whole(4, &self.out), whole(5, d.xv), whole(6, d.nf), whole(7, d.id)])
            }
        };
        let (pipeline, group_layout) = &self.pipelines[k];
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("water bridge"),
            layout: group_layout,
            entries: &entries,
        });
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("water bridge") });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("water bridge"),
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(count.div_ceil(256), 1, 1);
        }
        feed.queue.submit([encoder.finish()]);
    }

    /// What the renderer draws the drops from (after [`Bridge::update`]).
    pub fn source(&self) -> DropsSource {
        DropsSource {
            device: self.device.clone(),
            drops: self.out.clone(),
            counts: self.counts.clone(),
            wet: self.wet.clone(),
            grid: self.grid,
            wet_half: 0,
        }
    }
}

//! Drawing a Huge swarm straight from its particle buffers: one run of instances per thing a
//! layer draws (its own sprites, glitter's stars, each shadow, glow and outline), each an indirect
//! draw on the count the last step left, sitting among the sprites of its list where `at` says.
//! See `shaders/swarm_draw.wgsl`.

use std::collections::BTreeMap;
use std::sync::Arc;

use bpe_core::Blend;
use bytemuck::{Pod, Zeroable};

use super::SwarmSource;
use crate::HDR_FORMAT;

/// Samples of a layer's colours along their gradient.
pub const RAMP: usize = 256;
/// Samples of a sub-layer's own colours.
pub const SUB_RAMP: usize = 64;

/// What a run draws of each particle (`SW_ROLE_*` in swarm_draw.wgsl).
pub const ROLE_OWN: u32 = 0;
pub const ROLE_STAR: u32 = 1;
pub const ROLE_SHADOW: u32 = 2;
pub const ROLE_GLOW: u32 = 3;
pub const ROLE_OUTLINE: u32 = 4;

/// Flags (`SW_*` in swarm_draw.wgsl).
pub const FLUTTERS: u32 = 1;
pub const BALL_COLOURS: u32 = 2;
pub const ADDS: u32 = 4;
pub const SCREEN: u32 = 8;
pub const POSE: u32 = 16;
pub const LOOPED: u32 = 32;

/// Colour follows (`SW_FOLLOW_*`).
pub const FOLLOW_FIXED: u32 = 0;
pub const FOLLOW_LIFE: u32 = 1;
pub const FOLLOW_SPEED: u32 = 2;
pub const FOLLOW_ORDER: u32 = 3;
pub const FOLLOW_PLACE: u32 = 4;
pub const FOLLOW_SCREEN: u32 = 5;
pub const FOLLOW_START: u32 = 6;
pub const FOLLOW_TIME: u32 = 7;

/// How a run draws its swarm's particles. Matches `SwarmLook` in swarm_draw.wgsl (see there
/// for each field).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct SwarmLook {
    pub k0: [u32; 4],
    pub k1: [u32; 4],
    pub k2: [u32; 4],
    pub t0: [f32; 4],
    pub t1: [f32; 4],
    pub t2: [f32; 4],
    pub d0: [f32; 4],
    pub d1: [f32; 4],
    pub d2: [f32; 4],
    pub p0: [f32; 4],
    pub p1: [f32; 4],
    pub cam: [f32; 4],
    pub s0: [f32; 4],
    pub s1: [f32; 4],
    pub c0: [f32; 4],
    pub ramp: [[f32; 4]; RAMP],
    pub sub_ramp: [[f32; 4]; SUB_RAMP],
}

impl Default for SwarmLook {
    fn default() -> Self {
        let mut look = Self::zeroed();
        look.t0 = [1.0 / 120.0, 0.0, 0.0, 1.0];
        look.t2 = [1.0, 1.0, 0.0, 0.0];
        look.d0 = [1.0, 0.0, 0.0, 0.0];
        look.p0 = [1920.0, 0.0, 1.0, 0.0];
        look.cam = [0.0, 0.0, 0.0, 1.0];
        look.c0 = [1.0; 4];
        look.ramp = [[1.0; 4]; RAMP];
        look
    }
}

/// One run of a swarm's particles among the sprites of a list.
#[derive(Clone, Debug, PartialEq)]
pub struct SwarmRun {
    /// How many sprites of the same list (`particles` for `swarms`, `particles_front` for
    /// `swarms_front`) are drawn before it.
    pub at: u32,
    pub source: SwarmSource,
    pub look: Arc<SwarmLook>,
    /// The parent's ball colours by row (linear light), for Colour from Parent ball.
    pub balls: Arc<[[f32; 4]]>,
}

/// The pipelines swarms draw with.
pub(crate) struct SwarmDraw {
    pipeline: wgpu::RenderPipeline,
    each: BTreeMap<u32, wgpu::RenderPipeline>,
    layout: wgpu::PipelineLayout,
    module: wgpu::ShaderModule,
    group_layout: wgpu::BindGroupLayout,
}

fn make(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    fs: &str,
    blend: wgpu::BlendState,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("swarm"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs_swarm"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fs),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: HDR_FORMAT,
                blend: Some(blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

impl SwarmDraw {
    pub(crate) fn new(device: &wgpu::Device, globals_layout: &wgpu::BindGroupLayout) -> Self {
        let read = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("swarm"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                read(1),
                read(2),
                read(3),
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("swarm"),
            bind_group_layouts: &[Some(globals_layout), Some(&group_layout)],
            immediate_size: 0,
        });
        let source = [
            include_str!("../shaders/sprites.wgsl"),
            include_str!("../shaders/each.wgsl"),
            include_str!("../shaders/swarm_draw.wgsl"),
        ]
        .concat();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("swarm"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pipeline = make(device, &layout, &module, "fs_sprite", wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING);
        Self { pipeline, each: BTreeMap::new(), layout, module, group_layout }
    }

    /// Makes the pipeline for swarms in a layer whose things blend with each other by `blend`.
    pub(crate) fn ensure_each(&mut self, device: &wgpu::Device, blend: Blend) {
        let code = blend.shader_code();
        if !self.each.contains_key(&code) {
            let p = make(device, &self.layout, &self.module, "fs_swarm_each", crate::each::blend_state(blend));
            self.each.insert(code, p);
        }
    }

    /// Draws run `index` of a list (the globals bound at group 0); in a layer whose things blend
    /// with each other by `each`, through its pipeline. Binds group 1.
    pub(crate) fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        gpu: &FrameSwarms,
        front: bool,
        index: usize,
        each: Option<Blend>,
    ) {
        let list = if front { &gpu.front } else { &gpu.back };
        let Some(run) = list.get(index) else { return };
        let Some(group) = run.group.as_ref() else { return };
        let pipeline = each.and_then(|b| self.each.get(&b.shader_code())).unwrap_or(&self.pipeline);
        pass.set_pipeline(pipeline);
        pass.set_bind_group(1, group, &[]);
        pass.draw_indirect(&run.counts, 0);
    }
}

/// One run's buffers on the card.
struct RunGpu {
    look: wgpu::Buffer,
    balls: Option<wgpu::Buffer>,
    counts: wgpu::Buffer,
    /// `None` for a swarm on another card: it isn't drawn.
    group: Option<wgpu::BindGroup>,
}

/// A frame's swarm runs on the card, behind the balls and in front.
#[derive(Default)]
pub(crate) struct FrameSwarms {
    back: Vec<RunGpu>,
    front: Vec<RunGpu>,
}

impl FrameSwarms {
    /// Uploads `back` and `front`'s looks and binds their particles.
    pub(crate) fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        draw: &SwarmDraw,
        back: &[SwarmRun],
        front: &[SwarmRun],
    ) {
        for (gpu, list) in [(&mut self.back, back), (&mut self.front, front)] {
            gpu.truncate(list.len());
            while gpu.len() < list.len() {
                gpu.push(RunGpu {
                    look: device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("swarm look"),
                        size: std::mem::size_of::<SwarmLook>() as u64,
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }),
                    balls: None,
                    counts: list[gpu.len()].source.counts.clone(),
                    group: None,
                });
            }
            for (g, run) in gpu.iter_mut().zip(list) {
                g.upload(device, queue, draw, run);
            }
        }
    }
}

impl RunGpu {
    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, draw: &SwarmDraw, run: &SwarmRun) {
        self.group = None;
        if run.source.device != *device {
            return;
        }
        queue.write_buffer(&self.look, 0, bytemuck::bytes_of(&*run.look));
        let balls: &[[f32; 4]] = if run.balls.is_empty() { &[[0.0; 4]] } else { &run.balls };
        let bytes = std::mem::size_of_val(balls) as u64;
        // Exactly as long as the list: the shader reads its length.
        if self.balls.as_ref().is_none_or(|b| b.size() != bytes) {
            self.balls = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("swarm ball colours"),
                size: bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let ball_buffer = self.balls.as_ref().expect("just made");
        queue.write_buffer(ball_buffer, 0, bytemuck::cast_slice(balls));
        self.counts = run.source.counts.clone();
        self.group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("swarm"),
            layout: &draw.group_layout,
            entries: &[
                super::entry(0, &self.look),
                super::entry(1, &run.source.main),
                super::entry(2, &run.source.more),
                super::entry(3, ball_buffer),
            ],
        }));
    }
}

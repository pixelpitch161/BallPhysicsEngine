//! Replaying marks on the graphics card: each flow step draws the marks its canvas got that step
//! into the canvas's three textures (see canvas_stamp.wgsl), in one render pass of instanced quads
//! in the order the marks were made: the stamps, then the lifts that thin paint under a ball.
//! Bleed marks run after, in a pass of their own that reads copies of the textures (see
//! canvas_bleed.wgsl). Blending runs in that order per texel, so the same marks
//! always leave the same texels. Within a pass, a depth test keeps on top the paint of whichever
//! ball last touched a texel, by its world step and when in that step (see canvas_stamp.wgsl);
//! the depth is cleared every pass, as every pass's marks come after the pass before's.

use bpe_runtime::{Mark, MarkShape, MarkTarget};
use bytemuck::{Pod, Zeroable};

use super::pictures::PictureAtlas;
use super::{CanvasTextures, clamp_texels};
use crate::flow::util;

/// The canvas's uniform, laid out as `Canvas` in canvas_stamp.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct StampCanvas {
    size: [f32; 4],
    /// x: how many pictures the atlas holds.
    info: [f32; 4],
}

/// One mark as the stamp shader reads it, laid out as `MarkIn` in canvas_stamp.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct MarkGpu {
    ends: [f32; 4],
    look: [f32; 4],
    colour: [f32; 4],
    clock: [f32; 4],
    colour_to: [f32; 4],
    within: [f32; 4],
}

const MARK_ATTRIBUTES: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4];

/// The shape number canvas_stamp.wgsl draws.
fn shape_number(shape: MarkShape) -> f32 {
    match shape {
        MarkShape::Stroke => 0.0,
        MarkShape::Splat => 1.0,
        MarkShape::Ring => 2.0,
        MarkShape::Star => 3.0,
        MarkShape::Crack => 4.0,
        MarkShape::Burst => 5.0,
        MarkShape::Dots => 6.0,
        MarkShape::Smear => 7.0,
        MarkShape::Drip => 8.0,
        MarkShape::Picture => 9.0,
        MarkShape::Lift => 10.0,
        MarkShape::Bleed => 11.0,
    }
}

impl MarkGpu {
    /// `m` for a world stepping `steps_per_second`, which turns its step into its time, in a
    /// pass whose marks were made over `steps` world steps from step `first`.
    fn new(m: &Mark, steps_per_second: u32, first: u64, steps: u64) -> Self {
        Self {
            ends: [m.from.x, m.from.y, m.to.x, m.to.y],
            look: [m.radius, m.angle, m.variant as f32, shape_number(m.shape)],
            colour: m.colour,
            clock: [
                (m.step as f64 / steps_per_second.max(1) as f64) as f32,
                (m.step - first) as f32,
                steps as f32,
                m.radius_to,
            ],
            colour_to: m.colour_to,
            within: [m.within[0], m.within[1], 0.0, 0.0],
        }
    }
}

/// The stamp pipeline every canvas shares.
pub(crate) struct StampPipeline {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    lift: wgpu::RenderPipeline,
    bleed_layout: wgpu::BindGroupLayout,
    bleed: wgpu::RenderPipeline,
}

/// The vertex state both the stamp and bleed shaders read marks with.
fn mark_vertex<'a>(module: &'a wgpu::ShaderModule, entry: &'a str) -> wgpu::VertexState<'a> {
    wgpu::VertexState {
        module,
        entry_point: Some(entry),
        compilation_options: Default::default(),
        buffers: &[Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<MarkGpu>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &MARK_ATTRIBUTES,
        })],
    }
}

impl StampPipeline {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("canvas stamp layout"),
            entries: &[
                util::uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT, false),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("canvas stamp"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = util::shader(device, "canvas stamp", &[include_str!("../shaders/canvas_stamp.wgsl")]);
        let format = wgpu::TextureFormat::Rgba16Float;
        let body = wgpu::TextureFormat::R16Float;
        let target = |format, blend, write_mask| Some(wgpu::ColorTargetState { format, blend, write_mask });
        let all = wgpu::ColorWrites::ALL;
        let depth = |write, compare| {
            Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(write),
                depth_compare: Some(compare),
                stencil: Default::default(),
                bias: Default::default(),
            })
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("canvas stamp"),
            layout: Some(&pipeline_layout),
            vertex: mark_vertex(&module, "vs_stamp"),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            // When in the pass a texel was last touched: later wins, a tie goes to the later mark.
            depth_stencil: depth(true, wgpu::CompareFunction::GreaterEqual),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_stamp"),
                compilation_options: Default::default(),
                targets: &[
                    // The paint: each mark premultiplied over what is there.
                    target(format, Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING), all),
                    // The time painted: replaced where the shader says (alpha 1), kept where
                    // it does not (alpha 0). Either way the blend is exact.
                    target(format, Some(REPLACE_BY_ALPHA), all),
                    // The thickness: every mark adds its ink.
                    target(body, Some(ADD), all),
                ],
            }),
            multiview_mask: None,
            cache: None,
        });
        // A lift scales the paint and thickness under it and leaves the time alone.
        let lift = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("canvas lift"),
            layout: Some(&pipeline_layout),
            vertex: mark_vertex(&module, "vs_stamp"),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: depth(false, wgpu::CompareFunction::Always),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_lift"),
                compilation_options: Default::default(),
                targets: &[
                    target(format, Some(SCALE), all),
                    target(format, None, wgpu::ColorWrites::empty()),
                    target(body, Some(SCALE), all),
                ],
            }),
            multiview_mask: None,
            cache: None,
        });
        let bleed_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("canvas bleed layout"),
            entries: &[
                util::uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT, false),
                util::float_texture_entry_unfiltered(1, wgpu::ShaderStages::FRAGMENT),
                util::float_texture_entry_unfiltered(2, wgpu::ShaderStages::FRAGMENT),
                util::float_texture_entry_unfiltered(3, wgpu::ShaderStages::FRAGMENT),
            ],
        });
        let bleed_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("canvas bleed"),
            bind_group_layouts: &[Some(&bleed_layout)],
            immediate_size: 0,
        });
        let bleed_module = util::shader(device, "canvas bleed", &[include_str!("../shaders/canvas_bleed.wgsl")]);
        let bleed = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("canvas bleed"),
            layout: Some(&bleed_pipeline_layout),
            vertex: mark_vertex(&bleed_module, "vs_bleed"),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &bleed_module,
                entry_point: Some("fs_bleed"),
                compilation_options: Default::default(),
                targets: &[target(format, None, all), target(format, None, all), target(body, None, all)],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self { layout, pipeline, lift, bleed_layout, bleed }
    }
}

/// The new colour where its alpha is 1, the old where it is 0; the old alpha stays.
const REPLACE_BY_ALPHA: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::SrcAlpha,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
};

/// Adds the new value to what is there, in every channel.
const ADD: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
};

/// What is there times the new value, in every channel: a lift's share left.
const SCALE: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::Src,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::SrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
};

/// The stamp pass's depth: when in the pass each texel was last touched.
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Marks held in a mark buffer before it is made bigger.
const FIRST_MARKS: u64 = 256;

/// One canvas on the graphics card: its textures and what it needs to stamp marks into them.
pub(crate) struct CanvasGpu {
    pub(crate) textures: CanvasTextures,
    /// When in the current pass each texel was last touched, cleared every pass.
    depth: wgpu::TextureView,
    /// The canvas's width and height in world pixels.
    pub(crate) size: [f32; 2],
    /// The texture size the frame asked for, before the cap.
    pub(crate) texels: [u32; 2],
    /// Whose marks it stamps: a canvas's or a skin's.
    target: MarkTarget,
    group: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    /// Copies of the textures the bleed pass reads, made when first needed. Not checkpointed:
    /// each bleed fills them afresh.
    scratch: Option<Scratch>,
    /// The marks of every step of one batch, each step's after the one before.
    marks: wgpu::Buffer,
    /// Marks the buffer holds, and marks written so far this batch.
    capacity: u64,
    used: u64,
}

impl CanvasGpu {
    /// Cleared textures for the canvas or skin `target`, `size` world pixels drawn at `texels`.
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipeline: &StampPipeline,
        atlas: &PictureAtlas,
        target: MarkTarget,
        size: [f32; 2],
        texels: [u32; 2],
    ) -> Self {
        let textures = CanvasTextures::new(device, texels);
        let [w, h] = clamp_texels(texels);
        let depth = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("canvas stamp depth"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("canvas stamp"),
            size: std::mem::size_of::<StampCanvas>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let data = StampCanvas {
            size: [size[0].max(1.0), size[1].max(1.0), w as f32, h as f32],
            info: [atlas.layers as f32, 0.0, 0.0, 0.0],
        };
        queue.write_buffer(&uniform, 0, bytemuck::bytes_of(&data));
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("canvas stamp"),
            layout: &pipeline.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&atlas.view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&atlas.sampler) },
            ],
        });
        Self {
            textures,
            depth,
            size,
            texels,
            target,
            group,
            uniform,
            scratch: None,
            marks: mark_buffer(device, FIRST_MARKS),
            capacity: FIRST_MARKS,
            used: 0,
        }
    }

    /// Records the stamping of this canvas's marks among `marks`, from a world stepping
    /// `steps_per_second`. `first` says the step is the first of a batch, whose marks start
    /// the buffer afresh: every step of a batch is sent to the GPU together.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn step(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipeline: &StampPipeline,
        encoder: &mut wgpu::CommandEncoder,
        marks: &[Mark],
        steps_per_second: u32,
        first: bool,
    ) {
        if first {
            self.used = 0;
        }
        let ours: Vec<&Mark> = marks.iter().filter(|m| m.target == self.target).collect();
        let first_step = ours.iter().map(|m| m.step).min().unwrap_or(0);
        let steps = ours.iter().map(|m| m.step - first_step + 1).max().unwrap_or(1);
        // Stamps, then lifts, then bleeds, each in log order.
        let of = |wanted: fn(MarkShape) -> u8, kind: u8| ours.iter().copied().filter(move |m| wanted(m.shape) == kind);
        let kind = |shape: MarkShape| match shape {
            MarkShape::Lift => 1,
            MarkShape::Bleed => 2,
            _ => 0,
        };
        let stamps: Vec<&Mark> = of(kind, 0).collect();
        let lifts: Vec<&Mark> = of(kind, 1).collect();
        let bleeds: Vec<&Mark> = of(kind, 2).collect();
        let gpu: Vec<MarkGpu> = stamps
            .iter()
            .chain(&lifts)
            .chain(&bleeds)
            .map(|m| MarkGpu::new(m, steps_per_second, first_step, steps))
            .collect();
        if gpu.is_empty() {
            return;
        }
        let count = gpu.len() as u64;
        if self.used + count > self.capacity {
            // The steps already recorded keep the old buffer and what was written to it.
            self.capacity = (self.capacity * 2).max(count);
            self.marks = mark_buffer(device, self.capacity);
            self.used = 0;
        }
        let stride = std::mem::size_of::<MarkGpu>() as u64;
        queue.write_buffer(&self.marks, self.used * stride, bytemuck::cast_slice(&gpu));
        let base = self.used as u32;
        let (stamp_end, lift_end) = (base + stamps.len() as u32, base + (stamps.len() + lifts.len()) as u32);
        self.used += count;
        let attachment = |view| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })
        };
        if !stamps.is_empty() || !lifts.is_empty() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("canvas stamp"),
                color_attachments: &[
                    attachment(&self.textures.paint_view),
                    attachment(&self.textures.aux_view),
                    attachment(&self.textures.body_view),
                ],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.group, &[]);
            pass.set_vertex_buffer(0, self.marks.slice(..));
            if !stamps.is_empty() {
                pass.set_pipeline(&pipeline.pipeline);
                pass.draw(0..4, base..stamp_end);
            }
            if !lifts.is_empty() {
                pass.set_pipeline(&pipeline.lift);
                pass.draw(0..4, stamp_end..lift_end);
            }
        }
        // Bleeds: one pass per step that has any, in order.
        let mut from = lift_end;
        let mut i = 0;
        while i < bleeds.len() {
            let step = bleeds[i].step;
            let n = bleeds[i..].iter().take_while(|m| m.step == step).count();
            let group = &bleeds[i..i + n];
            self.bleed(device, pipeline, encoder, group, from..from + n as u32);
            from += n as u32;
            i += n;
        }
    }

    /// One bleed pass for the Bleed marks `group` (instances `range` of the mark buffer): copies
    /// the texels they cover, grown a little, and redraws them from the copies.
    fn bleed(
        &mut self,
        device: &wgpu::Device,
        pipeline: &StampPipeline,
        encoder: &mut wgpu::CommandEncoder,
        group: &[&Mark],
        range: std::ops::Range<u32>,
    ) {
        let (w, h) = (self.textures.paint.width(), self.textures.paint.height());
        let to_texel = |p: glam::Vec2| {
            [(p.x + self.size[0] * 0.5) / self.size[0] * w as f32, (p.y + self.size[1] * 0.5) / self.size[1] * h as f32]
        };
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for m in group {
            for p in [to_texel(m.from), to_texel(m.to)] {
                for k in 0..2 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
        }
        let clip = |v: f32, max: u32| (v.max(0.0) as u32).min(max);
        let (x0, y0) = (clip(lo[0].floor() - 4.0, w), clip(lo[1].floor() - 4.0, h));
        let (x1, y1) = (clip(hi[0].ceil() + 4.0, w), clip(hi[1].ceil() + 4.0, h));
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        if self.scratch.is_none() {
            self.scratch = Some(Scratch::new(device, pipeline, &self.uniform, self.texels));
        }
        let scratch = self.scratch.as_ref().unwrap();
        let extent = wgpu::Extent3d { width: x1 - x0, height: y1 - y0, depth_or_array_layers: 1 };
        let origin = wgpu::Origin3d { x: x0, y: y0, z: 0 };
        for (from, to) in [
            (&self.textures.paint, &scratch.textures.paint),
            (&self.textures.aux, &scratch.textures.aux),
            (&self.textures.body, &scratch.textures.body),
        ] {
            let side = |texture| wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin,
                aspect: wgpu::TextureAspect::All,
            };
            encoder.copy_texture_to_texture(side(from), side(to), extent);
        }
        let attachment = |view| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("canvas bleed"),
            color_attachments: &[
                attachment(&self.textures.paint_view),
                attachment(&self.textures.aux_view),
                attachment(&self.textures.body_view),
            ],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline.bleed);
        pass.set_bind_group(0, &scratch.group, &[]);
        pass.set_vertex_buffer(0, self.marks.slice(..));
        pass.draw(0..4, range);
    }

    /// The textures that carry the canvas from one step to the next; checkpoints copy them.
    pub(crate) fn state(&self) -> [&wgpu::Texture; 3] {
        [&self.textures.paint, &self.textures.aux, &self.textures.body]
    }
}

/// Copies of a canvas's textures for the bleed pass to read, and what binds them.
struct Scratch {
    textures: CanvasTextures,
    group: wgpu::BindGroup,
}

impl Scratch {
    fn new(device: &wgpu::Device, pipeline: &StampPipeline, uniform: &wgpu::Buffer, texels: [u32; 2]) -> Self {
        let textures = CanvasTextures::new(device, texels);
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("canvas bleed"),
            layout: &pipeline.bleed_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&textures.paint_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&textures.aux_view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&textures.body_view) },
            ],
        });
        Self { textures, group }
    }
}

fn mark_buffer(device: &wgpu::Device, marks: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("canvas marks"),
        size: marks * std::mem::size_of::<MarkGpu>() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

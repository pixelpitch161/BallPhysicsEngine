//! Replaying marks on the graphics card: each flow step draws the marks its canvas got that step
//! into the canvas's two textures (see canvas_stamp.wgsl), in one render pass of instanced quads
//! in the order the marks were made. Blending runs in that order per texel, so the same marks
//! always leave the same texels.

use bpe_core::ThingId;
use bpe_runtime::{Mark, MarkShape};
use bytemuck::{Pod, Zeroable};

use super::{CanvasTextures, clamp_texels};
use crate::flow::util;

/// The canvas's uniform, laid out as `Canvas` in canvas_stamp.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct StampCanvas {
    size: [f32; 4],
}

/// One mark as the stamp shader reads it, laid out as `MarkIn` in canvas_stamp.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct MarkGpu {
    ends: [f32; 4],
    look: [f32; 4],
    colour: [f32; 4],
    clock: [f32; 4],
}

const MARK_ATTRIBUTES: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4];

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
    }
}

impl MarkGpu {
    /// `m` for a world stepping `steps_per_second`, which turns its step into its time.
    fn new(m: &Mark, steps_per_second: u32) -> Self {
        Self {
            ends: [m.from.x, m.from.y, m.to.x, m.to.y],
            look: [m.radius, m.angle, m.variant as f32, shape_number(m.shape)],
            colour: m.colour,
            clock: [(m.step as f64 / steps_per_second.max(1) as f64) as f32, 0.0, 0.0, 0.0],
        }
    }
}

/// The stamp pipeline every canvas shares.
pub(crate) struct StampPipeline {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}

impl StampPipeline {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("canvas stamp layout"),
            entries: &[util::uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT, false)],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("canvas stamp"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = util::shader(device, "canvas stamp", &[include_str!("../shaders/canvas_stamp.wgsl")]);
        let format = wgpu::TextureFormat::Rgba16Float;
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("canvas stamp"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_stamp"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<MarkGpu>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &MARK_ATTRIBUTES,
                })],
            },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_stamp"),
                compilation_options: Default::default(),
                targets: &[
                    // The paint: each mark premultiplied over what is there.
                    Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    // The time painted: replaced where the shader says (alpha 1), kept where
                    // it does not (alpha 0). Either way the blend is exact.
                    Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(REPLACE_BY_ALPHA),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                ],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self { layout, pipeline }
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

/// Marks held in a mark buffer before it is made bigger.
const FIRST_MARKS: u64 = 256;

/// One canvas on the graphics card: its textures and what it needs to stamp marks into them.
pub(crate) struct CanvasGpu {
    pub(crate) textures: CanvasTextures,
    /// The canvas's width and height in world pixels.
    pub(crate) size: [f32; 2],
    /// The texture size the frame asked for, before the cap.
    pub(crate) texels: [u32; 2],
    thing: ThingId,
    group: wgpu::BindGroup,
    /// The marks of every step of one batch, each step's after the one before.
    marks: wgpu::Buffer,
    /// Marks the buffer holds, and marks written so far this batch.
    capacity: u64,
    used: u64,
}

impl CanvasGpu {
    /// Cleared textures for canvas `thing`, `size` world pixels drawn at `texels`.
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipeline: &StampPipeline,
        thing: ThingId,
        size: [f32; 2],
        texels: [u32; 2],
    ) -> Self {
        let textures = CanvasTextures::new(device, texels);
        let [w, h] = clamp_texels(texels);
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("canvas stamp"),
            size: std::mem::size_of::<StampCanvas>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let data = StampCanvas { size: [size[0].max(1.0), size[1].max(1.0), w as f32, h as f32] };
        queue.write_buffer(&uniform, 0, bytemuck::bytes_of(&data));
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("canvas stamp"),
            layout: &pipeline.layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }],
        });
        Self {
            textures,
            size,
            texels,
            thing,
            group,
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
        let ours: Vec<MarkGpu> =
            marks.iter().filter(|m| m.canvas == self.thing).map(|m| MarkGpu::new(m, steps_per_second)).collect();
        if ours.is_empty() {
            return;
        }
        let count = ours.len() as u64;
        if self.used + count > self.capacity {
            // The steps already recorded keep the old buffer and what was written to it.
            self.capacity = (self.capacity * 2).max(count);
            self.marks = mark_buffer(device, self.capacity);
            self.used = 0;
        }
        let stride = std::mem::size_of::<MarkGpu>() as u64;
        queue.write_buffer(&self.marks, self.used * stride, bytemuck::cast_slice(&ours));
        let attachment = |view| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("canvas stamp"),
            color_attachments: &[attachment(&self.textures.paint_view), attachment(&self.textures.aux_view)],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, &self.group, &[]);
        pass.set_vertex_buffer(0, self.marks.slice(..));
        pass.draw(0..4, self.used as u32..(self.used + count) as u32);
        self.used += count;
    }

    /// The textures that carry the canvas from one step to the next; checkpoints copy them.
    pub(crate) fn state(&self) -> [&wgpu::Texture; 2] {
        [&self.textures.paint, &self.textures.aux]
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

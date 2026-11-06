//! Particles and the round bits of trails, as the CPU worked them out: one quad per
//! [`Particle`](crate::Particle) instance, drawn in list order with one pipeline (added light and
//! covering paint alike, see sprites.wgsl), so what a layer's sub-layers put under or over each
//! particle stays there. In a layer whose things blend with each other they mix in by its blend.

use std::collections::BTreeMap;

use bpe_core::Blend;

use crate::{GrowableBuffer, HDR_FORMAT, Particle};

const ATTRIBUTES: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
    0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Uint32x4, 4 => Float32x4, 5 => Float32x4
];

/// The sprites pipeline, and one for each blend things blend with each other by (see each.rs),
/// made the first time a layer needs it.
pub struct Sprites {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::PipelineLayout,
    module: wgpu::ShaderModule,
    each: BTreeMap<u32, wgpu::RenderPipeline>,
}

/// In a layer whose things blend with each other: the sprite as each.wgsl mixes things in, so
/// added light counts as covering and shows when the layer is mixed in.
const EACH_ENTRY: &str = "
@fragment
fn fs_sprite_each(in: SpriteOut) -> @location(0) vec4<f32> {
    return each_out(sprite_colour(in), g.post.w);
}
";

fn make(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    fs: &str,
    blend: wgpu::BlendState,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("sprites"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs_sprite"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Particle>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &ATTRIBUTES,
            })],
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

impl Sprites {
    pub fn new(device: &wgpu::Device, globals_layout: &wgpu::BindGroupLayout) -> Self {
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sprites"),
            bind_group_layouts: &[Some(globals_layout)],
            immediate_size: 0,
        });
        let source = [include_str!("shaders/sprites.wgsl"), include_str!("shaders/each.wgsl"), EACH_ENTRY].concat();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sprites"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pipeline = make(device, &layout, &module, "fs_sprite", wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING);
        Self { pipeline, layout, module, each: BTreeMap::new() }
    }

    /// Makes the pipeline for sprites in a layer whose things blend with each other by `blend`.
    pub(crate) fn ensure_each(&mut self, device: &wgpu::Device, blend: Blend) {
        let code = blend.shader_code();
        if !self.each.contains_key(&code) {
            let p = make(device, &self.layout, &self.module, "fs_sprite_each", crate::each::blend_state(blend));
            self.each.insert(code, p);
        }
    }

    /// Draws the `count` particles uploaded into `buffer`; the globals must be bound at group 0
    /// (a layer's own, naming its blend, when its things blend with each other by `each`, for
    /// which [`Sprites::ensure_each`] made the pipeline).
    pub(crate) fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        buffer: &GrowableBuffer,
        count: u32,
        each: Option<Blend>,
    ) {
        if count == 0 {
            return;
        }
        let pipeline = each.and_then(|b| self.each.get(&b.shader_code())).unwrap_or(&self.pipeline);
        pass.set_pipeline(pipeline);
        pass.set_vertex_buffer(0, buffer.slice());
        pass.draw(0..4, 0..count);
    }
}

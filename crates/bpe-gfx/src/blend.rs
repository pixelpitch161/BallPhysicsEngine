//! The blend composite: mixes one layer of things drawn with the same blend and amount into the
//! picture under them (see blend.wgsl).

use bpe_core::Blend;

use crate::flow::util;
use crate::{HDR_FORMAT, begin_pass};

pub(crate) struct Composite {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    /// One uniform per layer mixed in a frame: every layer is recorded before the frame is
    /// submitted, so each needs its own.
    uniforms: Vec<wgpu::Buffer>,
}

impl Composite {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        use wgpu::ShaderStages as S;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blend layout"),
            entries: &[
                util::uniform_entry(0, S::FRAGMENT, false),
                util::float_texture_entry(1, S::FRAGMENT),
                util::float_texture_entry(2, S::FRAGMENT),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blend"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = util::shader(device, "blend", &[include_str!("shaders/blend.wgsl")]);
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blend composite"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_full"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_blend"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self { layout, pipeline, uniforms: Vec::new() }
    }

    /// Records the mix of `layer` into `picture` with `blend` at `amount`; `under` holds a copy of
    /// `picture` from before. `slot` tells the layers of one frame apart. `each`: the layer's
    /// things blended with each other into it, so it covers wherever any of them drew.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn composite(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        slot: usize,
        blend: Blend,
        amount: f32,
        each: bool,
        under: &wgpu::TextureView,
        layer: &wgpu::TextureView,
        picture: &wgpu::TextureView,
    ) {
        while self.uniforms.len() <= slot {
            self.uniforms.push(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("blend"),
                size: 16,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let uniform = &self.uniforms[slot];
        let how = [blend.shader_code() as f32, amount.clamp(0.0, 1.0), if each { 1.0 } else { 0.0 }, 0.0];
        queue.write_buffer(uniform, 0, bytemuck::cast_slice(&how));
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blend"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(under) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(layer) },
            ],
        });
        let mut pass = begin_pass(encoder, picture, "blend composite", wgpu::LoadOp::Load);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
}

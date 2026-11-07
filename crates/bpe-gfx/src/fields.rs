//! Soft fields of particles (Soft smoke, Flame; see [`Field`]): each field of a frame's lists is
//! splatted into a picture of its own at half the output size right before the render pass that
//! draws its list, then shaded in that pass by one triangle over the whole picture, between the
//! list's sprites where its `at` says (see `Renderer::draw_sprites`).

use std::collections::BTreeMap;

use bpe_core::Blend;

use crate::{Field, FieldLook, Frame, GrowableBuffer, HDR_FORMAT, Splat};

/// The field pictures' format: added up past 1 where many splats meet.
const FIELD_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

const SPLAT_ATTRIBUTES: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4];

/// The pipelines fields draw with, and one for each blend things blend with each other by (see
/// each.rs), made the first time a layer needs it.
pub(crate) struct Fields {
    splat: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    each: BTreeMap<u32, wgpu::RenderPipeline>,
    composite_layout: wgpu::PipelineLayout,
    module: wgpu::ShaderModule,
    group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

/// One field on the GPU: its splats, its look, its picture and the group reading them.
struct FieldGpu {
    splats: GrowableBuffer,
    count: u32,
    look: wgpu::Buffer,
    size: (u32, u32),
    view: Option<wgpu::TextureView>,
    group: Option<wgpu::BindGroup>,
}

/// The fields of one frame's two lists on the GPU: those behind the balls, then those in front.
#[derive(Default)]
pub(crate) struct FrameFields {
    back: Vec<FieldGpu>,
    front: Vec<FieldGpu>,
}

fn composite_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    fs: &str,
    blend: wgpu::BlendState,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("field"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs_field"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
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

impl Fields {
    pub(crate) fn new(device: &wgpu::Device, globals_layout: &wgpu::BindGroupLayout) -> Self {
        let source = [
            include_str!("shaders/sprites.wgsl"),
            include_str!("shaders/each.wgsl"),
            include_str!("shaders/fields.wgsl"),
        ]
        .concat();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fields"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        use wgpu::ShaderStages as S;
        let group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("field"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: S::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: S::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: S::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let splat_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("field splats"),
            bind_group_layouts: &[Some(globals_layout)],
            immediate_size: 0,
        });
        let composite_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("field"),
            bind_group_layouts: &[Some(globals_layout), Some(&group_layout)],
            immediate_size: 0,
        });
        let add = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let splat = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("field splats"),
            layout: Some(&splat_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_splat"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Splat>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &SPLAT_ATTRIBUTES,
                })],
            },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_splat"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: FIELD_FORMAT,
                    blend: Some(wgpu::BlendState { color: add, alpha: add }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let composite = composite_pipeline(
            device,
            &composite_layout,
            &module,
            "fs_field",
            wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("field"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        Self { splat, composite, each: BTreeMap::new(), composite_layout, module, group_layout, sampler }
    }

    /// Makes the pipeline for fields in a layer whose things blend with each other by `blend`.
    pub(crate) fn ensure_each(&mut self, device: &wgpu::Device, blend: Blend) {
        let code = blend.shader_code();
        if !self.each.contains_key(&code) {
            let p = composite_pipeline(
                device,
                &self.composite_layout,
                &self.module,
                "fs_field_each",
                crate::each::blend_state(blend),
            );
            self.each.insert(code, p);
        }
    }

    /// Splats the fields of `f`'s list behind the balls (`front` false) or in front of them into
    /// their pictures, seen through `globals`. Before the pass that draws that list.
    pub(crate) fn splat(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        globals: &wgpu::BindGroup,
        gpu: &FrameFields,
        front: bool,
    ) {
        for field in if front { &gpu.front } else { &gpu.back } {
            let Some(view) = field.view.as_ref() else { continue };
            let mut pass =
                crate::begin_pass(encoder, view, "field splats", wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
            if field.count == 0 {
                continue;
            }
            pass.set_pipeline(&self.splat);
            pass.set_bind_group(0, globals, &[]);
            pass.set_vertex_buffer(0, field.splats.slice());
            pass.draw(0..4, 0..field.count);
        }
    }

    /// Shades field `index` of a list into the pass (the globals bound at group 0); in a layer
    /// whose things blend with each other by `each`, through its pipeline. Binds group 1.
    pub(crate) fn composite(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        gpu: &FrameFields,
        front: bool,
        index: usize,
        each: Option<Blend>,
    ) {
        let list = if front { &gpu.front } else { &gpu.back };
        let Some(group) = list.get(index).and_then(|f| f.group.as_ref()) else { return };
        let pipeline = each.and_then(|b| self.each.get(&b.shader_code())).unwrap_or(&self.composite);
        pass.set_pipeline(pipeline);
        pass.set_bind_group(1, group, &[]);
        pass.draw(0..3, 0..1);
    }
}

impl FrameFields {
    /// Uploads `f`'s fields, their pictures half of `size` (the output's).
    pub(crate) fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        fields: &Fields,
        f: &Frame,
        size: (u32, u32),
    ) {
        let half = (size.0.div_ceil(2).max(1), size.1.div_ceil(2).max(1));
        for (gpu, list) in [(&mut self.back, &f.fields), (&mut self.front, &f.fields_front)] {
            gpu.truncate(list.len());
            while gpu.len() < list.len() {
                gpu.push(FieldGpu {
                    splats: GrowableBuffer::new("field splats"),
                    count: 0,
                    look: device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("field look"),
                        size: std::mem::size_of::<FieldLook>() as u64,
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }),
                    size: (0, 0),
                    view: None,
                    group: None,
                });
            }
            for (g, field) in gpu.iter_mut().zip(list) {
                g.upload(device, queue, fields, field, half);
            }
        }
    }
}

impl FieldGpu {
    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, fields: &Fields, field: &Field, size: (u32, u32)) {
        self.splats.upload(device, queue, bytemuck::cast_slice(&field.splats));
        self.count = field.splats.len() as u32;
        queue.write_buffer(&self.look, 0, bytemuck::bytes_of(&field.look));
        if self.size != size || self.view.is_none() {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("field picture"),
                size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FIELD_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            self.group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("field"),
                layout: &fields.group_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&fields.sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: self.look.as_entire_binding() },
                ],
            }));
            self.view = Some(view);
            self.size = size;
        }
    }
}

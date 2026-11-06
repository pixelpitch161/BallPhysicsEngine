//! The bake's running average (lighting part 3): each sample's HDR picture is folded into a
//! 32-bit float picture as `a + (x − a) / (i + 1)`, back and forth between two, then put back in
//! the HDR picture for the effects, bloom and grade (see [`crate::Renderer::render_samples`]).

use crate::HDR_FORMAT;

const SUM_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;

const SHADER: &str = r#"
@group(0) @binding(0) var<uniform> k: vec4<f32>;
@group(0) @binding(1) var sum: texture_2d<f32>;
@group(0) @binding(2) var add: texture_2d<f32>;

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

// k.x: this sample's weight, 1 / (samples so far); the first sample takes the picture as it is.
@fragment
fn fs_accumulate(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let ip = vec2<i32>(pos.xy);
    let x = textureLoad(add, ip, 0);
    if k.x >= 1.0 {
        return x;
    }
    let a = textureLoad(sum, ip, 0);
    return a + (x - a) * k.x;
}

@fragment
fn fs_put(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(sum, vec2<i32>(pos.xy), 0);
}
"#;

/// A sum's texture and its view.
type Sum = (wgpu::Texture, wgpu::TextureView);

pub(crate) struct Accumulate {
    layout: wgpu::BindGroupLayout,
    accumulate: wgpu::RenderPipeline,
    put: wgpu::RenderPipeline,
    /// The two sums (size, then the pictures), made for the first bake of a size.
    sums: Option<((u32, u32), [Sum; 2])>,
    /// One weight a sample, grown as needed (each sample is submitted before the next is
    /// written, but a buffer per sample keeps them apart anyway).
    weights: Vec<wgpu::Buffer>,
    /// Which sum holds the average so far.
    current: usize,
}

impl Accumulate {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bake average"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let tex = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bake average layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                tex(1),
                tex(2),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bake average pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let make = |entry: &str, format| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_full"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        Accumulate {
            accumulate: make("fs_accumulate", SUM_FORMAT),
            put: make("fs_put", HDR_FORMAT),
            layout,
            sums: None,
            weights: Vec::new(),
            current: 0,
        }
    }

    fn ensure(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        if self.sums.as_ref().is_some_and(|(s, _)| *s == size) {
            return;
        }
        let make = || {
            let t = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("bake average"),
                size: wgpu::Extent3d { width: size.0.max(1), height: size.1.max(1), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: SUM_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let v = t.create_view(&Default::default());
            (t, v)
        };
        self.sums = Some((size, [make(), make()]));
    }

    fn group(
        &self,
        device: &wgpu::Device,
        weight: &wgpu::Buffer,
        sum: &wgpu::TextureView,
        add: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bake average"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: weight.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(sum) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(add) },
            ],
        })
    }

    /// Folds sample `index` (from 0), drawn into `hdr` (`size`), into the average.
    pub fn add(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        size: (u32, u32),
        index: u32,
        hdr: &wgpu::TextureView,
    ) {
        self.ensure(device, size);
        while self.weights.len() <= index as usize {
            self.weights.push(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("bake weight"),
                size: 16,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let weight = &self.weights[index as usize];
        queue.write_buffer(weight, 0, bytemuck::cast_slice(&[1.0 / (index + 1) as f32, 0.0, 0.0, 0.0]));
        if index == 0 {
            self.current = 0;
        }
        let (from, to) = (self.current, 1 - self.current);
        let sums = &self.sums.as_ref().expect("just made").1;
        let group = self.group(device, weight, &sums[from].1, hdr);
        {
            let mut pass =
                crate::begin_pass(encoder, &sums[to].1, "bake average", wgpu::LoadOp::Clear(wgpu::Color::BLACK));
            pass.set_pipeline(&self.accumulate);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.current = to;
    }

    /// Puts the average back into `hdr`.
    pub fn put(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, hdr: &wgpu::TextureView) {
        let Some((_, sums)) = self.sums.as_ref() else { return };
        let weight = &self.weights[0];
        let sum = &sums[self.current].1;
        let group = self.group(device, weight, sum, sum);
        let mut pass = crate::begin_pass(encoder, hdr, "bake average back", wgpu::LoadOp::Clear(wgpu::Color::BLACK));
        pass.set_pipeline(&self.put);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
}

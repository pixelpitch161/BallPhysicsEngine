//! GPU renderer for ball scenes.
//!
//! The same [`Renderer`] draws the editor preview and the exported video. It draws the scene
//! into an HDR texture, adds bloom, and writes the result as sRGB-encoded `Rgba8Unorm`, which
//! egui can show directly and video encoders read as ordinary RGBA bytes.

mod frame;

pub use frame::{BallInstance, FlashInstance, FrameData, TrailVertex};
pub use wgpu;

use bpe_scene::{Container, Scene};
use bytemuck::{Pod, Zeroable};

pub const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const BLOOM_LEVELS: usize = 6;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Globals {
    view: [f32; 4],
    bg_top: [f32; 4],
    bg_bottom: [f32; 4],
    container: [f32; 4],
    container_extra: [f32; 4],
    container_color: [f32; 4],
    look: [f32; 4],
    look2: [f32; 4],
}

impl Globals {
    fn new(scene: &Scene, output_width: u32, time: f64) -> Self {
        let video = &scene.video;
        let look = &scene.look;
        let pixel = video.width as f32 / output_width.max(1) as f32;
        let (container, container_extra, color) = match &scene.container {
            Container::Circle { center, radius, thickness, color } => {
                ([center.x, center.y, *radius, *thickness], [0.0; 4], color)
            }
            Container::Rect { center, size, thickness, color } => {
                ([center.x, center.y, size.x * 0.5, *thickness], [1.0, size.y * 0.5, 0.0, 0.0], color)
            }
        };
        Self {
            view: [2.0 / video.width as f32, 2.0 / video.height as f32, pixel, time as f32],
            bg_top: look.background_top.to_linear(),
            bg_bottom: look.background_bottom.to_linear(),
            container,
            container_extra,
            container_color: color.to_linear(),
            look: [look.ball_glow, look.trail_opacity, look.flash_strength, look.bloom_strength],
            look2: [look.exposure, 0.0, 0.0, 0.0],
        }
    }
}

/// Size-dependent GPU resources, rebuilt when the output size changes.
struct Targets {
    width: u32,
    height: u32,
    hdr: wgpu::TextureView,
    bloom: Vec<wgpu::TextureView>,
    output: wgpu::Texture,
    output_view: wgpu::TextureView,
    /// Bind groups reading the HDR image, then each bloom level, for the down and up passes.
    down_sources: Vec<wgpu::BindGroup>,
    up_sources: Vec<wgpu::BindGroup>,
    composite: wgpu::BindGroup,
}

pub struct Renderer {
    globals: wgpu::Buffer,
    globals_group: wgpu::BindGroup,
    post_layout: wgpu::BindGroupLayout,
    composite_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    background: wgpu::RenderPipeline,
    trail: wgpu::RenderPipeline,
    ball: wgpu::RenderPipeline,
    flash: wgpu::RenderPipeline,
    downsample: wgpu::RenderPipeline,
    upsample: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    trail_buffer: GrowableBuffer,
    ball_buffer: GrowableBuffer,
    flash_buffer: GrowableBuffer,
    targets: Targets,
}

impl Renderer {
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let scene_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/scene.wgsl").into()),
        });
        let post_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("post shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/post.wgsl").into()),
        });

        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let globals_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
        });

        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let sampler_entry = wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let post_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post layout"),
            entries: &[texture_entry(0), sampler_entry],
        });
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("composite layout"),
            entries: &[texture_entry(0), sampler_entry, texture_entry(2)],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("linear clamp"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let scene_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene pipeline layout"),
            bind_group_layouts: &[Some(&globals_layout)],
            immediate_size: 0,
        });
        let post_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post pipeline layout"),
            bind_group_layouts: &[Some(&globals_layout), Some(&post_layout)],
            immediate_size: 0,
        });
        let composite_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("composite pipeline layout"),
            bind_group_layouts: &[Some(&globals_layout), Some(&composite_layout)],
            immediate_size: 0,
        });

        let additive = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::OVER,
        };
        let premultiplied = wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING;

        let make = |label: &str,
                    layout: &wgpu::PipelineLayout,
                    module: &wgpu::ShaderModule,
                    vs: &str,
                    fs: &str,
                    buffers: &[Option<wgpu::VertexBufferLayout>],
                    topology: wgpu::PrimitiveTopology,
                    format: wgpu::TextureFormat,
                    blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState { topology, ..Default::default() },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };

        let trail_attributes = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4, 2 => Float32x2];
        let instance_attributes = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32, 2 => Float32x4];
        let trail_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TrailVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &trail_attributes,
        };
        let instance_layout = |stride| wgpu::VertexBufferLayout {
            array_stride: stride,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &instance_attributes,
        };

        use wgpu::PrimitiveTopology::{TriangleList, TriangleStrip};
        let background = make(
            "background",
            &scene_pipeline_layout,
            &scene_shader,
            "vs_background",
            "fs_background",
            &[],
            TriangleList,
            HDR_FORMAT,
            None,
        );
        let trail = make(
            "trails",
            &scene_pipeline_layout,
            &scene_shader,
            "vs_trail",
            "fs_trail",
            &[Some(trail_layout)],
            TriangleList,
            HDR_FORMAT,
            Some(additive),
        );
        let ball = make(
            "balls",
            &scene_pipeline_layout,
            &scene_shader,
            "vs_ball",
            "fs_ball",
            &[Some(instance_layout(std::mem::size_of::<BallInstance>() as u64))],
            TriangleStrip,
            HDR_FORMAT,
            Some(premultiplied),
        );
        let flash = make(
            "flashes",
            &scene_pipeline_layout,
            &scene_shader,
            "vs_flash",
            "fs_flash",
            &[Some(instance_layout(std::mem::size_of::<FlashInstance>() as u64))],
            TriangleStrip,
            HDR_FORMAT,
            Some(additive),
        );
        let downsample = make(
            "bloom down",
            &post_pipeline_layout,
            &post_shader,
            "vs_fullscreen",
            "fs_downsample",
            &[],
            TriangleList,
            HDR_FORMAT,
            None,
        );
        let upsample = make(
            "bloom up",
            &post_pipeline_layout,
            &post_shader,
            "vs_fullscreen",
            "fs_upsample",
            &[],
            TriangleList,
            HDR_FORMAT,
            Some(additive),
        );
        let composite = make(
            "composite",
            &composite_pipeline_layout,
            &post_shader,
            "vs_fullscreen",
            "fs_composite",
            &[],
            TriangleList,
            OUTPUT_FORMAT,
            None,
        );

        let targets = Targets::new(device, width, height, &post_layout, &composite_layout, &sampler);
        Self {
            globals,
            globals_group,
            post_layout,
            composite_layout,
            sampler,
            background,
            trail,
            ball,
            flash,
            downsample,
            upsample,
            composite,
            trail_buffer: GrowableBuffer::new("trail vertices"),
            ball_buffer: GrowableBuffer::new("ball instances"),
            flash_buffer: GrowableBuffer::new("flash instances"),
            targets,
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.targets.width, self.targets.height)
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if (width, height) != self.size() {
            self.targets =
                Targets::new(device, width, height, &self.post_layout, &self.composite_layout, &self.sampler);
        }
    }

    /// The finished image: sRGB-encoded `Rgba8Unorm`.
    pub fn output_texture(&self) -> &wgpu::Texture {
        &self.targets.output
    }

    pub fn output_view(&self) -> &wgpu::TextureView {
        &self.targets.output_view
    }

    /// Size of one output pixel in world units, for [`FrameData::build`].
    pub fn pixel_size(&self, scene: &Scene) -> f32 {
        scene.video.width as f32 / self.targets.width.max(1) as f32
    }

    /// Draws one frame into the output texture.
    pub fn render(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, scene: &Scene, time: f64, frame: &FrameData) {
        queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&Globals::new(scene, self.targets.width, time)));
        self.trail_buffer.upload(device, queue, bytemuck::cast_slice(&frame.trails));
        self.ball_buffer.upload(device, queue, bytemuck::cast_slice(&frame.balls));
        self.flash_buffer.upload(device, queue, bytemuck::cast_slice(&frame.flashes));

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        {
            let mut pass =
                begin_pass(&mut encoder, &self.targets.hdr, "scene", wgpu::LoadOp::Clear(wgpu::Color::BLACK));
            pass.set_bind_group(0, &self.globals_group, &[]);
            pass.set_pipeline(&self.background);
            pass.draw(0..3, 0..1);
            if !frame.trails.is_empty() {
                pass.set_pipeline(&self.trail);
                pass.set_vertex_buffer(0, self.trail_buffer.slice());
                pass.draw(0..frame.trails.len() as u32, 0..1);
            }
            if !frame.balls.is_empty() {
                pass.set_pipeline(&self.ball);
                pass.set_vertex_buffer(0, self.ball_buffer.slice());
                pass.draw(0..4, 0..frame.balls.len() as u32);
            }
            if !frame.flashes.is_empty() {
                pass.set_pipeline(&self.flash);
                pass.set_vertex_buffer(0, self.flash_buffer.slice());
                pass.draw(0..4, 0..frame.flashes.len() as u32);
            }
        }
        for level in 0..BLOOM_LEVELS {
            let mut pass = begin_pass(
                &mut encoder,
                &self.targets.bloom[level],
                "bloom down",
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            );
            pass.set_pipeline(&self.downsample);
            pass.set_bind_group(0, &self.globals_group, &[]);
            pass.set_bind_group(1, &self.targets.down_sources[level], &[]);
            pass.draw(0..3, 0..1);
        }
        for level in (0..BLOOM_LEVELS - 1).rev() {
            let mut pass = begin_pass(&mut encoder, &self.targets.bloom[level], "bloom up", wgpu::LoadOp::Load);
            pass.set_pipeline(&self.upsample);
            pass.set_bind_group(0, &self.globals_group, &[]);
            pass.set_bind_group(1, &self.targets.up_sources[level], &[]);
            pass.draw(0..3, 0..1);
        }
        {
            let mut pass = begin_pass(
                &mut encoder,
                &self.targets.output_view,
                "composite",
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            );
            pass.set_pipeline(&self.composite);
            pass.set_bind_group(0, &self.globals_group, &[]);
            pass.set_bind_group(1, &self.targets.composite, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit([encoder.finish()]);
    }

    /// Copies the output image back to the CPU as tightly packed RGBA bytes (sRGB-encoded).
    pub fn read_output(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> anyhow::Result<Vec<u8>> {
        let (width, height) = self.size();
        let row = width as usize * 4;
        let padded_row = row.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (padded_row * height as usize) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("readback") });
        encoder.copy_texture_to_buffer(
            self.targets.output.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row as u32),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        device.poll(wgpu::PollType::wait_indefinitely())?;
        receiver.recv()??;
        let mapped = slice.get_mapped_range()?;
        let mut pixels = Vec::with_capacity(row * height as usize);
        for chunk in mapped.chunks(padded_row) {
            pixels.extend_from_slice(&chunk[..row]);
        }
        drop(mapped);
        buffer.unmap();
        Ok(pixels)
    }
}

impl Targets {
    fn new(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        post_layout: &wgpu::BindGroupLayout,
        composite_layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
    ) -> Self {
        let (width, height) = (width.max(1), height.max(1));
        let texture = |label, w: u32, h: u32, format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let render_and_sample = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let hdr = texture("hdr", width, height, HDR_FORMAT, render_and_sample).create_view(&Default::default());
        let bloom: Vec<_> = (0..BLOOM_LEVELS)
            .map(|level| {
                let shift = level as u32 + 1;
                texture("bloom", width >> shift, height >> shift, HDR_FORMAT, render_and_sample)
                    .create_view(&Default::default())
            })
            .collect();
        let output = texture("output", width, height, OUTPUT_FORMAT, render_and_sample | wgpu::TextureUsages::COPY_SRC);
        let output_view = output.create_view(&Default::default());

        let source = |view: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("post source"),
                layout: post_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
                ],
            })
        };
        let down_sources = std::iter::once(&hdr).chain(&bloom[..BLOOM_LEVELS - 1]).map(source).collect();
        let up_sources = bloom[1..].iter().map(source).collect();
        let composite = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("composite"),
            layout: composite_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&hdr) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&bloom[0]) },
            ],
        });
        Self { width, height, hdr, bloom, output, output_view, down_sources, up_sources, composite }
    }
}

fn begin_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    view: &'a wgpu::TextureView,
    label: &str,
    load: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

/// A vertex buffer that grows to fit whatever is uploaded.
struct GrowableBuffer {
    label: &'static str,
    buffer: Option<wgpu::Buffer>,
    len: u64,
}

impl GrowableBuffer {
    fn new(label: &'static str) -> Self {
        Self { label, buffer: None, len: 0 }
    }

    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, bytes: &[u8]) {
        self.len = bytes.len() as u64;
        if bytes.is_empty() {
            return;
        }
        if self.buffer.as_ref().is_none_or(|b| b.size() < self.len) {
            self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(self.label),
                size: self.len.next_power_of_two().max(1024),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        queue.write_buffer(self.buffer.as_ref().unwrap(), 0, bytes);
    }

    fn slice(&self) -> wgpu::BufferSlice<'_> {
        self.buffer.as_ref().expect("buffer uploaded before drawing").slice(..self.len)
    }
}

/// Creates a GPU device without a window, for exporting from the command line.
pub fn headless_device() -> anyhow::Result<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))?;
    let (device, queue) = pollster::block_on(
        adapter.request_device(&wgpu::DeviceDescriptor { label: Some("bpe headless"), ..Default::default() }),
    )?;
    Ok((device, queue))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_sim::Playback;

    #[test]
    fn renders_a_frame_without_errors() {
        let Ok((device, queue)) = headless_device() else {
            eprintln!("no GPU adapter available, skipping");
            return;
        };
        let scene = Scene::default();
        let mut renderer = Renderer::new(&device, 270, 480);
        let mut playback = Playback::new(&scene);
        playback.seek(1.0);
        let frame = FrameData::build(&scene, &playback, renderer.pixel_size(&scene));
        assert!(!frame.trails.is_empty());
        renderer.render(&device, &queue, &scene, 1.0, &frame);
        let pixels = renderer.read_output(&device, &queue).unwrap();
        assert_eq!(pixels.len(), 270 * 480 * 4);
        assert!(pixels.chunks(4).any(|p| p[0] > 100), "expected some bright pixels");
    }
}

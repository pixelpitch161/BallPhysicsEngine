//! GPU passes for the 3D view.

use super::{Glow, MAX_GATES, MAX_GLOW_HITS, RibbonVertex, SpaceBall, SpaceFrame, SpaceView, linear, ring_color};
use bpe_scene::{Container3d, Scene};
use bytemuck::{Pod, Zeroable};

/// Width and height of the shadow map in texels.
pub const SHADOW_SIZE: u32 = 2048;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Matches `struct Space` in space.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct SpaceUniform {
    view_proj: [[f32; 4]; 4],
    inv_view_proj: [[f32; 4]; 4],
    light_view_proj: [[f32; 4]; 4],
    camera: [f32; 4],
    screen: [f32; 4],
    key_dir: [f32; 4],
    key_color: [f32; 4],
    ambient_top: [f32; 4],
    ambient_bottom: [f32; 4],
    material: [f32; 4],
    glass_color: [f32; 4],
    container: [f32; 4],
    bg_top: [f32; 4],
    bg_bottom: [f32; 4],
    light: [f32; 4],
    gates: [[f32; 4]; MAX_GATES],
    gate_colors: [[f32; 4]; MAX_GATES],
    hits: [[f32; 4]; MAX_GLOW_HITS],
    hit_colors: [[f32; 4]; MAX_GLOW_HITS],
}

impl SpaceUniform {
    fn new(scene: &Scene, frame: &SpaceFrame, width: u32, height: u32, time: f64) -> Self {
        let space = &frame.space;
        let lights = &space.lights;
        let material = &space.material;
        let camera = &frame.camera;
        let key = space.key_direction();
        let rgb = |color: bpe_scene::Color, k: f32| {
            let c = linear(color) * k;
            [c.x, c.y, c.z, 0.0]
        };
        let with_w = |mut v: [f32; 4], w: f32| {
            v[3] = w;
            v
        };
        let mut uniform = Self {
            view_proj: camera.view_projection.to_cols_array_2d(),
            inv_view_proj: camera.view_projection.inverse().to_cols_array_2d(),
            light_view_proj: frame.light.view_projection.to_cols_array_2d(),
            camera: [camera.position.x, camera.position.y, camera.position.z, time as f32],
            screen: [width as f32, height as f32, (camera.fov * 0.5).tan(), scene.look.flash_size],
            key_dir: [key.x, key.y, key.z, lights.intensity.max(0.0)],
            key_color: with_w(rgb(lights.key_color, 1.0), if lights.shadows { 1.0 } else { 0.0 }),
            ambient_top: with_w(rgb(lights.ambient_top, lights.ambient.max(0.0)), lights.rim.max(0.0)),
            ambient_bottom: with_w(rgb(lights.ambient_bottom, lights.ambient.max(0.0)), scene.look.ball_glow.max(0.0)),
            material: [
                material.roughness.clamp(0.0, 1.0),
                material.metalness.clamp(0.0, 1.0),
                material.glass.clamp(0.0, 1.0),
                material.edge_glow.max(0.0),
            ],
            glass_color: with_w(rgb(material.glass_color, 1.0), material.walls.max(0.0)),
            container: [frame.container.kind as f32, frame.container.size, frame.container.angle, frame.container.back],
            bg_top: rgb(scene.look.background_top, 1.0),
            bg_bottom: rgb(scene.look.background_bottom, 1.0),
            light: [
                frame.light.depth_scale,
                frame.light.texel,
                frame.gates.len().min(MAX_GATES) as f32,
                frame.wall_glows.len().min(MAX_GLOW_HITS) as f32,
            ],
            gates: [[0.0; 4]; MAX_GATES],
            gate_colors: [[0.0; 4]; MAX_GATES],
            hits: [[0.0; 4]; MAX_GLOW_HITS],
            hit_colors: [[0.0; 4]; MAX_GLOW_HITS],
        };
        let (spacing, rings) = match space.container {
            Container3d::Tunnel { spacing, rings, .. } => (spacing, rings),
            _ => (0.0, 0),
        };
        uniform.gates[0][3] = spacing;
        for (i, gate) in frame.gates.iter().take(MAX_GATES).enumerate() {
            uniform.gates[i] = [gate.z, gate.angle, gate.gap_half, spacing];
            uniform.gate_colors[i] = ring_color(space, gate.index, rings);
        }
        for (i, glow) in frame.wall_glows.iter().take(MAX_GLOW_HITS).enumerate() {
            let p = glow.position;
            uniform.hits[i] = [p.x, p.y, p.z, glow.strength];
            uniform.hit_colors[i] = glow.color;
        }
        uniform
    }
}

struct DepthTarget {
    width: u32,
    height: u32,
    view: wgpu::TextureView,
    glass_group: wgpu::BindGroup,
}

/// Draws [`SpaceFrame`]s. Holds its pipelines, shadow map and depth buffer.
pub struct SpaceRenderer {
    uniform: wgpu::Buffer,
    uniform_group: wgpu::BindGroup,
    textures_layout: wgpu::BindGroupLayout,
    shadow_sampler: wgpu::Sampler,
    shadow_view: wgpu::TextureView,
    /// Textures for the scene pass, which can't read the depth it is drawing into.
    scene_group: wgpu::BindGroup,
    shadow: wgpu::RenderPipeline,
    env: wgpu::RenderPipeline,
    ball: wgpu::RenderPipeline,
    ribbon: wgpu::RenderPipeline,
    glow: wgpu::RenderPipeline,
    glass: wgpu::RenderPipeline,
    balls: GrowableBuffer,
    ribbons: GrowableBuffer,
    glows: GrowableBuffer,
    depth: Option<DepthTarget>,
    /// The camera of the last frame drawn, for [`SpaceRenderer::view`].
    last_camera: Option<(glam::Mat4, glam::Vec3)>,
}

impl SpaceRenderer {
    pub fn new(device: &wgpu::Device, hdr_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("space shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("space.wgsl").into()),
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("space uniform"),
            size: std::mem::size_of::<SpaceUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("space uniform layout"),
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
        let uniform_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("space uniform"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }],
        });
        let depth_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let textures_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("space textures layout"),
            entries: &[
                depth_entry(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
                depth_entry(2),
            ],
        });
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let depth_texture = |label, width, height| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };
        let shadow_view = depth_texture("shadow map", SHADOW_SIZE, SHADOW_SIZE).create_view(&Default::default());
        let placeholder = depth_texture("no depth", 1, 1).create_view(&Default::default());
        let scene_group = textures_group(device, &textures_layout, &shadow_view, &shadow_sampler, &placeholder);

        let layout = |label, groups: &[Option<&wgpu::BindGroupLayout>]| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: groups,
                immediate_size: 0,
            })
        };
        let shadow_layout = layout("space shadow layout", &[Some(&uniform_layout)]);
        let main_layout = layout("space layout", &[Some(&uniform_layout), Some(&textures_layout)]);

        let ball_attributes = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4];
        let ball_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<SpaceBall>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &ball_attributes,
        };
        let glow_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Glow>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &ball_attributes,
        };
        let ribbon_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32, 2 => Float32x4];
        let ribbon_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<RibbonVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ribbon_attributes,
        };
        let additive = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let premultiplied = wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING;
        // Reversed depth: nearer is bigger.
        let depth = |write: bool, compare| {
            Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(write),
                depth_compare: Some(compare),
                stencil: Default::default(),
                bias: Default::default(),
            })
        };
        let make = |label: &str,
                    layout: &wgpu::PipelineLayout,
                    vs: &str,
                    fs: &str,
                    buffers: &[Option<wgpu::VertexBufferLayout>],
                    topology: wgpu::PrimitiveTopology,
                    color: Option<wgpu::BlendState>,
                    depth_stencil: Option<wgpu::DepthStencilState>,
                    has_color: bool| {
            let targets =
                [Some(wgpu::ColorTargetState { format: hdr_format, blend: color, write_mask: wgpu::ColorWrites::ALL })];
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState { topology, ..Default::default() },
                depth_stencil,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: if has_color { &targets } else { &[] },
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        use wgpu::CompareFunction::{Always, GreaterEqual, LessEqual};
        use wgpu::PrimitiveTopology::{TriangleList, TriangleStrip};
        let shadow_depth = Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(LessEqual),
            stencil: Default::default(),
            bias: Default::default(),
        });
        let shadow = make(
            "space shadows",
            &shadow_layout,
            "vs_shadow",
            "fs_shadow",
            &[Some(ball_layout.clone())],
            TriangleStrip,
            None,
            shadow_depth,
            false,
        );
        let env = make(
            "space walls",
            &main_layout,
            "vs_screen",
            "fs_env",
            &[],
            TriangleList,
            None,
            depth(true, Always),
            true,
        );
        let ball = make(
            "space balls",
            &main_layout,
            "vs_ball",
            "fs_ball",
            &[Some(ball_layout)],
            TriangleStrip,
            Some(premultiplied),
            depth(true, GreaterEqual),
            true,
        );
        let ribbon = make(
            "space trails",
            &main_layout,
            "vs_ribbon",
            "fs_ribbon",
            &[Some(ribbon_layout)],
            TriangleList,
            Some(additive),
            depth(false, GreaterEqual),
            true,
        );
        let glow = make(
            "space glows",
            &main_layout,
            "vs_glow",
            "fs_glow",
            &[Some(glow_layout)],
            TriangleStrip,
            Some(additive),
            depth(false, GreaterEqual),
            true,
        );
        let glass = make(
            "space glass",
            &main_layout,
            "vs_screen",
            "fs_glass",
            &[],
            TriangleList,
            Some(premultiplied),
            None,
            true,
        );

        Self {
            uniform,
            uniform_group,
            textures_layout,
            shadow_sampler,
            shadow_view,
            scene_group,
            shadow,
            env,
            ball,
            ribbon,
            glow,
            glass,
            balls: GrowableBuffer::new("space balls"),
            ribbons: GrowableBuffer::new("space trails"),
            glows: GrowableBuffer::new("space glows"),
            depth: None,
            last_camera: None,
        }
    }

    /// Whether the scene is drawn in 3D, so this renderer replaces the 2D passes.
    /// The frame decides, since in the frame where Flat/3D is switched the scene and its
    /// playback can disagree.
    pub fn active(&self, scene: &Scene, frame: &crate::FrameData) -> bool {
        scene.space().is_some() && frame.space.active
    }

    /// Draws the frame into `hdr`, an HDR picture of `size` pixels, replacing what was there.
    /// `scene` should already be at its animated values for this moment.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        hdr: &wgpu::TextureView,
        (width, height): (u32, u32),
        scene: &Scene,
        frame: &crate::FrameData,
        time: f64,
    ) {
        let frame = &frame.space;
        self.ensure_depth(device, width, height);
        let uniform = SpaceUniform::new(scene, frame, width, height, time);
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
        self.balls.upload(device, queue, bytemuck::cast_slice(&frame.balls));
        self.ribbons.upload(device, queue, bytemuck::cast_slice(&frame.ribbons));
        self.glows.upload(device, queue, bytemuck::cast_slice(&frame.glows));
        self.last_camera = Some((frame.camera.view_projection, frame.camera.position));
        let target = self.depth.as_ref().expect("depth target made above");

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("space shadows"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if frame.space.lights.shadows && !frame.balls.is_empty() {
                pass.set_pipeline(&self.shadow);
                pass.set_bind_group(0, &self.uniform_group, &[]);
                pass.set_vertex_buffer(0, self.balls.slice());
                pass.draw(0..4, 0..frame.balls.len() as u32);
            }
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("space scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: hdr,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.uniform_group, &[]);
            pass.set_bind_group(1, &self.scene_group, &[]);
            pass.set_pipeline(&self.env);
            pass.draw(0..3, 0..1);
            if !frame.balls.is_empty() {
                pass.set_pipeline(&self.ball);
                pass.set_vertex_buffer(0, self.balls.slice());
                pass.draw(0..4, 0..frame.balls.len() as u32);
            }
            if !frame.ribbons.is_empty() {
                pass.set_pipeline(&self.ribbon);
                pass.set_vertex_buffer(0, self.ribbons.slice());
                pass.draw(0..frame.ribbons.len() as u32, 0..1);
            }
            if !frame.glows.is_empty() {
                pass.set_pipeline(&self.glow);
                pass.set_vertex_buffer(0, self.glows.slice());
                pass.draw(0..4, 0..frame.glows.len() as u32);
            }
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("space glass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: hdr,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.uniform_group, &[]);
            pass.set_bind_group(1, &target.glass_group, &[]);
            pass.set_pipeline(&self.glass);
            pass.draw(0..3, 0..1);
        }
    }

    /// The camera and depth of the last 3D frame drawn, so particles can be drawn into it.
    pub fn view(&self) -> Option<SpaceView<'_>> {
        let (view_projection, camera_position) = self.last_camera?;
        Some(SpaceView { view_projection, camera_position, depth: &self.depth.as_ref()?.view })
    }

    fn ensure_depth(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        let (width, height) = (width.max(1), height.max(1));
        if self.depth.as_ref().is_some_and(|d| (d.width, d.height) == (width, height)) {
            return;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("space depth"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let glass_group = textures_group(device, &self.textures_layout, &self.shadow_view, &self.shadow_sampler, &view);
        self.depth = Some(DepthTarget { width, height, view, glass_group });
    }
}

fn textures_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    shadow: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    scene_depth: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("space textures"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(shadow) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(scene_depth) },
        ],
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

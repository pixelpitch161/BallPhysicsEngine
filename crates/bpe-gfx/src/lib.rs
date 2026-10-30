//! The modular Ball Studio's renderer.
//!
//! Each thing's drawing part writes instances into a [`Frame`]; the [`Renderer`] draws the frame
//! into an HDR texture, adds bloom, and writes the result as sRGB-encoded `Rgba8Unorm`, which
//! egui can show directly and video encoders read as ordinary RGBA bytes.

mod blend;
#[cfg(test)]
mod blend_tests;
pub mod effects;
pub mod flow;
mod frame;
mod gradients;
#[cfg(test)]
mod paint_tests;
pub mod particles;
#[cfg(test)]
mod pictures;
pub mod text;

use flow::surface::THICKNESS_FORMAT;
pub use flow::{FlowLayers, FlowSource, FlowStatus};
pub use frame::{
    BackgroundPaint, BallInstance, BlendLayer, Burst, DrawCx, Draws, EffectCall, FlashInstance, Flock, Frame,
    GlyphInstance, GpuLiquid, HitMark, LiquidLayer, LookInstance, ModuleDraw, Points, ShapeInstance, SoftVertex, Stage,
    SwarmLayer, SwarmLook, SwarmStart, ThingDraw, TrailVertex, blend_alpha, build_frame, build_frame_at, hit_strength,
    push_streak, push_streak_colours, thing_code,
};
pub use gradients::{ATLAS_WIDTH, GradientAtlas, NO_VARY, PaintGpu};
pub use text::Atlas;
pub use wgpu;

use bpe_runtime::CameraView;
use bytemuck::{Pod, Zeroable};

pub const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
pub(crate) const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// The object picture shader effects on things read: own position, thing code, coverage.
pub(crate) const OBJECT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;
const BLOOM_LEVELS: usize = 6;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Globals {
    view: [f32; 4],
    /// The background paint: atlas row (below 0 paints black), start, scroll, repeat.
    bg_paint: [f32; 4],
    /// The background's place: direction in degrees, span, centre xy.
    bg_place: [f32; 4],
    post: [f32; 4],
    /// The camera: centre xy, zoom, turn in radians.
    camera: [f32; 4],
    /// x: 1 when the background stays on the screen, y: how its place runs (0 none, 1 direction,
    /// 2 centre), z: 1 for hard steps, w: 1 to mix `bg_from` and `bg_to`.
    bg: [f32; 4],
    /// The two colours of a plain blend the background mixes itself (`bg.w` is 1 then).
    bg_from: [f32; 4],
    bg_to: [f32; 4],
}

impl Globals {
    /// The globals for drawing `frame` through `camera`.
    fn new(frame: &Frame, camera: &CameraView, output_width: u32) -> Self {
        let pixel = frame.width as f32 / output_width.max(1) as f32;
        // One output pixel in world units: smaller as the camera zooms in.
        let world_pixel = if camera.zoom == 1.0 { pixel } else { pixel / camera.zoom };
        Self {
            view: [2.0 / frame.width.max(1) as f32, 2.0 / frame.height.max(1) as f32, world_pixel, frame.time],
            bg_paint: [
                if frame.background.row == u32::MAX { -1.0 } else { frame.background.row as f32 },
                frame.background.t0,
                frame.background.scroll,
                frame.background.repeat,
            ],
            bg_place: frame.background.place,
            post: [frame.bloom, frame.exposure, 0.0, 0.0],
            camera: [camera.center.x, camera.center.y, camera.zoom, camera.turn],
            bg: [
                if frame.background_on_screen { 1.0 } else { 0.0 },
                frame.background.mode,
                frame.background.steps,
                if frame.background.ends.is_some() { 1.0 } else { 0.0 },
            ],
            bg_from: frame.background.ends.map_or([0.0; 4], |e| e[0]),
            bg_to: frame.background.ends.map_or([0.0; 4], |e| e[1]),
        }
    }
}

/// Size-dependent GPU resources, rebuilt when the output size changes.
struct Targets {
    width: u32,
    height: u32,
    hdr_texture: wgpu::Texture,
    hdr: wgpu::TextureView,
    /// A copy of the HDR image from before the liquids, which they bend and tint. A blend layer
    /// copies the picture under it here too, for its liquids and for the composite to read.
    behind: wgpu::Texture,
    behind_view: wgpu::TextureView,
    /// What the things of one blend layer draw into, before it is mixed into the HDR image.
    layer: wgpu::TextureView,
    /// Half-size liquid thickness pictures: the splat target and the blur's scratch.
    thickness: wgpu::TextureView,
    blur: wgpu::TextureView,
    /// The same size: the particles' speed over the paint's span, times their thickness, and
    /// the blur's scratch for it.
    speed: wgpu::TextureView,
    speed_blur: wgpu::TextureView,
    thickness_size: (u32, u32),
    /// The object picture, made the first time an effect needs it.
    object: Option<(wgpu::Texture, wgpu::TextureView)>,
    bloom: Vec<wgpu::TextureView>,
    output: wgpu::Texture,
    output_view: wgpu::TextureView,
    /// Bind groups reading the HDR image, then each bloom level, for the down and up passes.
    down_sources: Vec<wgpu::BindGroup>,
    up_sources: Vec<wgpu::BindGroup>,
    composite: wgpu::BindGroup,
}

/// The gradient atlas on the GPU, remade only when the frame has more gradients than it has rows.
struct PaintTexture {
    texture: wgpu::Texture,
    rows: usize,
    group: wgpu::BindGroup,
}

impl PaintTexture {
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, sampler: &wgpu::Sampler, rows: usize) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("gradient atlas"),
            size: wgpu::Extent3d { width: ATLAS_WIDTH as u32, height: rows as u32, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gradient atlas"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
            ],
        });
        Self { texture, rows, group }
    }

    /// Uploads `atlas`, making the texture taller first if its gradients do not fit.
    fn sync(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        atlas: &GradientAtlas,
    ) {
        let max = device.limits().max_texture_dimension_2d as usize;
        if atlas.len() > max {
            static WARNED: std::sync::Once = std::sync::Once::new();
            WARNED.call_once(|| {
                eprintln!("{} gradients in one frame; past {max} they reuse the last one", atlas.len());
            });
        }
        let rows = gradients::uploaded_rows(atlas.len(), max);
        if rows > self.rows {
            *self = Self::new(device, layout, sampler, gradients::atlas_height(rows, max));
        }
        if rows == 0 {
            return;
        }
        let row_bytes = (ATLAS_WIDTH * 4 * 2) as u32;
        queue.write_texture(
            self.texture.as_image_copy(),
            &gradients::half_texels(&atlas.rows()[..rows]),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row_bytes), rows_per_image: None },
            wgpu::Extent3d { width: ATLAS_WIDTH as u32, height: rows as u32, depth_or_array_layers: 1 },
        );
    }
}

/// The glyph atlas on the GPU, remade when the atlas grows or changes.
struct AtlasTexture {
    size: u32,
    generation: u64,
    group: wgpu::BindGroup,
}

pub struct Renderer {
    globals: wgpu::Buffer,
    globals_group: wgpu::BindGroup,
    /// The same globals with no camera, for things stuck to the screen.
    screen_globals: wgpu::Buffer,
    screen_globals_group: wgpu::BindGroup,
    /// The frame's instance lists, its overlay's, and each blend layer's (the frame's layers,
    /// then the overlay's).
    buffers: FrameBuffers,
    overlay: FrameBuffers,
    layer_buffers: Vec<FrameBuffers>,
    blend: blend::Composite,
    post_layout: wgpu::BindGroupLayout,
    composite_layout: wgpu::BindGroupLayout,
    atlas_layout: wgpu::BindGroupLayout,
    /// The gradient atlas: bound at group 1 of the scene passes and group 2 of the glyphs and
    /// particles.
    paint_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    background: wgpu::RenderPipeline,
    trail: wgpu::RenderPipeline,
    shape: wgpu::RenderPipeline,
    soft: wgpu::RenderPipeline,
    ball: wgpu::RenderPipeline,
    look: wgpu::RenderPipeline,
    flash: wgpu::RenderPipeline,
    glyph: wgpu::RenderPipeline,
    /// What is where, for effects on things (see object.wgsl).
    shape_object: wgpu::RenderPipeline,
    ball_object: wgpu::RenderPipeline,
    look_object: wgpu::RenderPipeline,
    chain: effects::chain::Chain,
    particles: particles::Particles,
    downsample: wgpu::RenderPipeline,
    upsample: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    atlas: Option<AtlasTexture>,
    paint: PaintTexture,
    /// The overlay frame's own gradients: its instances name rows of its own atlas.
    overlay_paint: PaintTexture,
    targets: Targets,
    surface: flow::surface::Surface,
    /// The GPU-stepped layers, made the first time a frame is drawn with [`Renderer::render_with_flow`].
    flow: Option<FlowLayers>,
}

/// The instance lists of one frame (the main one, its overlay or a blend layer) on the GPU.
struct FrameBuffers {
    trails: GrowableBuffer,
    shapes: GrowableBuffer,
    soft: GrowableBuffer,
    balls: GrowableBuffer,
    looks: GrowableBuffer,
    flashes: GrowableBuffer,
    glyphs: GrowableBuffer,
    bursts: particles::BurstList,
}

impl Default for FrameBuffers {
    fn default() -> Self {
        Self {
            trails: GrowableBuffer::new("trail vertices"),
            shapes: GrowableBuffer::new("shape instances"),
            soft: GrowableBuffer::new("soft vertices"),
            balls: GrowableBuffer::new("ball instances"),
            looks: GrowableBuffer::new("styled ball instances"),
            flashes: GrowableBuffer::new("flash instances"),
            glyphs: GrowableBuffer::new("glyph instances"),
            bursts: particles::BurstList::default(),
        }
    }
}

impl FrameBuffers {
    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, particles: &particles::Particles, f: &Frame) {
        self.trails.upload(device, queue, bytemuck::cast_slice(&f.trails));
        self.shapes.upload(device, queue, bytemuck::cast_slice(&f.shapes));
        self.soft.upload(device, queue, bytemuck::cast_slice(&f.soft));
        self.balls.upload(device, queue, bytemuck::cast_slice(&f.balls));
        self.looks.upload(device, queue, bytemuck::cast_slice(&f.looks));
        self.flashes.upload(device, queue, bytemuck::cast_slice(&f.flashes));
        self.glyphs.upload(device, queue, bytemuck::cast_slice(&f.glyphs));
        particles.fill(&mut self.bursts, device, queue, particles::pick(&f.bursts, f.time));
    }
}

/// How one frame's things are drawn: through which globals, from which gradient atlas, and
/// whether its liquids and swarms are drawn (the overlay has none).
struct Through {
    globals: wgpu::BindGroup,
    paint: wgpu::BindGroup,
    world: bool,
}

impl Renderer {
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let scene_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene shader"),
            source: wgpu::ShaderSource::Wgsl(
                [
                    gradients::paint_wgsl(1).as_str(),
                    include_str!("shaders/scene.wgsl"),
                    include_str!("shaders/object.wgsl"),
                    include_str!("shaders/ball_look.wgsl"),
                    include_str!("shaders/styled.wgsl"),
                    include_str!("shaders/trail.wgsl"),
                ]
                .concat()
                .into(),
            ),
        });
        let post_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("post shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/post.wgsl").into()),
        });
        let glyph_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glyph shader"),
            source: wgpu::ShaderSource::Wgsl(
                [gradients::paint_wgsl(2).as_str(), include_str!("shaders/glyph.wgsl")].concat().into(),
            ),
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
        let screen_globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screen globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let screen_globals_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("screen globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: screen_globals.as_entire_binding() }],
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
        let atlas_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("atlas layout"),
            entries: &[texture_entry(0), sampler_entry],
        });
        let paint_layout = gradients::paint_layout(device);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("linear clamp"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let scene_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene pipeline layout"),
            bind_group_layouts: &[Some(&globals_layout), Some(&paint_layout)],
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
        let glyph_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("glyph pipeline layout"),
            bind_group_layouts: &[Some(&globals_layout), Some(&atlas_layout), Some(&paint_layout)],
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

        // Every instance carries its paint (`PaintGpu`) as its last three vec4s.
        let shape_attributes = wgpu::vertex_attr_array![
            0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4,
            6 => Float32x4
        ];
        let ball_attributes = wgpu::vertex_attr_array![
            0 => Float32x2, 1 => Float32, 2 => Float32, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4,
            7 => Float32x4
        ];
        let trail_attributes = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4];
        let look_attributes = wgpu::vertex_attr_array![
            0 => Float32x2, 1 => Float32, 2 => Float32, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4,
            6 => Float32x4, 7 => Float32x4, 8 => Float32x4, 9 => Float32x4, 10 => Float32x4, 11 => Float32x4,
            12 => Float32x4
        ];
        let flash_attributes = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32, 2 => Float32x4];
        let soft_attributes = wgpu::vertex_attr_array![
            0 => Float32x2, 1 => Float32, 2 => Float32, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4,
            7 => Float32x2
        ];
        let glyph_attributes = wgpu::vertex_attr_array![
            0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4,
            6 => Float32x4, 7 => Float32x4, 8 => Float32x4
        ];
        let instances = |stride, attributes| wgpu::VertexBufferLayout {
            array_stride: stride,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes,
        };
        let vertices = |stride, attributes| wgpu::VertexBufferLayout {
            array_stride: stride,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes,
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
        let shape = make(
            "shapes",
            &scene_pipeline_layout,
            &scene_shader,
            "vs_shape",
            "fs_shape",
            &[Some(instances(std::mem::size_of::<ShapeInstance>() as u64, &shape_attributes))],
            TriangleStrip,
            HDR_FORMAT,
            Some(premultiplied),
        );
        let soft = make(
            "soft bodies",
            &scene_pipeline_layout,
            &scene_shader,
            "vs_soft",
            "fs_soft",
            &[Some(vertices(std::mem::size_of::<SoftVertex>() as u64, &soft_attributes))],
            TriangleList,
            HDR_FORMAT,
            Some(premultiplied),
        );
        let ball = make(
            "balls",
            &scene_pipeline_layout,
            &scene_shader,
            "vs_ball",
            "fs_ball",
            &[Some(instances(std::mem::size_of::<BallInstance>() as u64, &ball_attributes))],
            TriangleStrip,
            HDR_FORMAT,
            Some(premultiplied),
        );
        let glyph = make(
            "glyphs",
            &glyph_pipeline_layout,
            &glyph_shader,
            "vs_glyph",
            "fs_glyph",
            &[Some(instances(std::mem::size_of::<GlyphInstance>() as u64, &glyph_attributes))],
            TriangleStrip,
            HDR_FORMAT,
            Some(premultiplied),
        );
        let trail = make(
            "trails",
            &scene_pipeline_layout,
            &scene_shader,
            "vs_trail",
            "fs_trail",
            &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<TrailVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &trail_attributes,
            })],
            TriangleList,
            HDR_FORMAT,
            Some(premultiplied),
        );
        let look = make(
            "styled balls",
            &scene_pipeline_layout,
            &scene_shader,
            "vs_look",
            "fs_look",
            &[Some(instances(std::mem::size_of::<LookInstance>() as u64, &look_attributes))],
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
            &[Some(instances(std::mem::size_of::<FlashInstance>() as u64, &flash_attributes))],
            TriangleStrip,
            HDR_FORMAT,
            Some(additive),
        );
        let shape_object = make(
            "shape objects",
            &scene_pipeline_layout,
            &scene_shader,
            "vs_shape",
            "fs_shape_object",
            &[Some(instances(std::mem::size_of::<ShapeInstance>() as u64, &shape_attributes))],
            TriangleStrip,
            OBJECT_FORMAT,
            None,
        );
        let ball_object = make(
            "ball objects",
            &scene_pipeline_layout,
            &scene_shader,
            "vs_ball",
            "fs_ball_object",
            &[Some(instances(std::mem::size_of::<BallInstance>() as u64, &ball_attributes))],
            TriangleStrip,
            OBJECT_FORMAT,
            None,
        );
        let look_object = make(
            "styled ball objects",
            &scene_pipeline_layout,
            &scene_shader,
            "vs_look",
            "fs_look_object",
            &[Some(instances(std::mem::size_of::<LookInstance>() as u64, &look_attributes))],
            TriangleStrip,
            OBJECT_FORMAT,
            None,
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
        let first_height = gradients::atlas_height(0, device.limits().max_texture_dimension_2d as usize);
        let paint = PaintTexture::new(device, &paint_layout, &sampler, first_height);
        let overlay_paint = PaintTexture::new(device, &paint_layout, &sampler, first_height);
        let particles = particles::Particles::new(device, &globals_layout, &paint_layout);
        Self {
            globals,
            globals_group,
            screen_globals,
            screen_globals_group,
            buffers: FrameBuffers::default(),
            overlay: FrameBuffers::default(),
            layer_buffers: Vec::new(),
            blend: blend::Composite::new(device),
            post_layout,
            composite_layout,
            atlas_layout,
            paint_layout,
            sampler,
            background,
            trail,
            shape,
            soft,
            ball,
            look,
            flash,
            glyph,
            shape_object,
            ball_object,
            look_object,
            chain: effects::chain::Chain::new(device),
            particles,
            downsample,
            upsample,
            composite,
            atlas: None,
            paint,
            overlay_paint,
            targets,
            surface: flow::surface::Surface::new(device),
            flow: None,
        }
    }

    /// Uploads the glyph atlas when it changed since the last frame.
    fn sync_atlas(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let atlas = text::glyphs();
        if self.atlas.as_ref().is_some_and(|a| a.size == atlas.size && a.generation == atlas.generation) {
            return;
        }
        let size = atlas.size.max(1);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph atlas"),
            size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            &atlas.texels(),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size), rows_per_image: Some(size) },
            wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
        );
        let view = texture.create_view(&Default::default());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("glyph atlas"),
            layout: &self.atlas_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        self.atlas = Some(AtlasTexture { size: atlas.size, generation: atlas.generation, group });
    }

    pub fn size(&self) -> (u32, u32) {
        (self.targets.width, self.targets.height)
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if self.size() != (width.max(1), height.max(1)) {
            self.targets =
                Targets::new(device, width, height, &self.post_layout, &self.composite_layout, &self.sampler);
        }
    }

    pub fn output_texture(&self) -> &wgpu::Texture {
        &self.targets.output
    }

    pub fn output_view(&self) -> &wgpu::TextureView {
        &self.targets.output_view
    }

    /// Size of one output pixel in world units.
    pub fn pixel_size(&self, frame: &Frame) -> f32 {
        frame.width as f32 / self.targets.width.max(1) as f32
    }

    /// Like [`Renderer::render`], and first steps the frame's GPU liquids and swarms to
    /// `frame.time`.
    pub fn render_with_flow(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame,
        source: &FlowSource,
    ) -> FlowStatus {
        let flow = self.flow.get_or_insert_with(|| FlowLayers::new(device));
        flow.prepare(device, queue, frame, source);
        let status = flow.step_to(device, queue, frame.time as f64, source);
        self.render(device, queue, frame);
        status
    }

    /// The GPU-stepped layers, once a frame has been drawn with [`Renderer::render_with_flow`].
    pub fn flow(&self) -> Option<&FlowLayers> {
        self.flow.as_ref()
    }

    /// Why shader effects in the last frame could not be built, one line each: "{name}: {why}".
    pub fn shader_problems(&self) -> Vec<String> {
        self.chain.problems().map(str::to_owned).collect()
    }

    /// Draws one frame into the output texture. GPU liquids and swarms are stepped by
    /// [`Renderer::render_with_flow`]; here they are only drawn. Liquids are shaded between
    /// the walls and the balls, over a copy of what is behind them. Each blend layer is drawn
    /// into its own picture and mixed in after the frame's flashes (the overlay's layers after
    /// the overlay), so blended things sit above the rest.
    pub fn render(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame) {
        queue.write_buffer(
            &self.globals,
            0,
            bytemuck::bytes_of(&Globals::new(frame, &frame.camera, self.targets.width)),
        );
        let overlay = frame.overlay.as_deref().filter(|_| frame.has_overlay());
        if let Some(o) = overlay {
            let identity = CameraView::default();
            queue.write_buffer(
                &self.screen_globals,
                0,
                bytemuck::bytes_of(&Globals::new(frame, &identity, self.targets.width)),
            );
            self.overlay.upload(device, queue, &self.particles, o);
        }
        self.buffers.upload(device, queue, &self.particles, frame);
        // Every blend layer's lists, the frame's then the overlay's, each in its own buffers.
        let overlay_layers = overlay.map_or(&[][..], |o| &o.layers[..]);
        let layers: Vec<&BlendLayer> = frame.layers.iter().chain(overlay_layers).collect();
        let mut layer_buffers = std::mem::take(&mut self.layer_buffers);
        layer_buffers.resize_with(layer_buffers.len().max(layers.len()), FrameBuffers::default);
        for (b, l) in layer_buffers.iter_mut().zip(&layers) {
            b.upload(device, queue, &self.particles, &l.frame);
        }
        let glyphs = |f: &Frame| !f.glyphs.is_empty() || f.layers.iter().any(|l| !l.frame.glyphs.is_empty());
        if glyphs(frame) || overlay.is_some_and(glyphs) {
            self.sync_atlas(device, queue);
        }
        self.paint.sync(device, queue, &self.paint_layout, &self.sampler, &frame.gradients);
        if let Some(o) = overlay {
            self.overlay_paint.sync(device, queue, &self.paint_layout, &self.sampler, &o.gradients);
        }

        let wants_objects = effects::chain::Chain::wants_objects(frame);
        if wants_objects {
            self.targets.ensure_object(device);
        }
        self.chain.begin_frame();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        let world = Through { globals: self.globals_group.clone(), paint: self.paint.group.clone(), world: true };
        {
            let mut pass =
                begin_pass(&mut encoder, &self.targets.hdr, "scene", wgpu::LoadOp::Clear(wgpu::Color::BLACK));
            pass.set_bind_group(0, &self.globals_group, &[]);
            pass.set_bind_group(1, &self.paint.group, &[]);
            pass.set_pipeline(&self.background);
            pass.draw(0..3, 0..1);
            self.draw_back(&mut pass, frame, &self.buffers, &world);
        }
        if !frame.liquids.is_empty() {
            self.copy_under(&mut encoder);
        }
        let mut liquid_slot = 0;
        let hdr = self.targets.hdr.clone();
        self.draw_liquids(device, queue, &mut encoder, frame, &hdr, &mut liquid_slot);
        {
            let mut pass = begin_pass(&mut encoder, &self.targets.hdr, "balls", wgpu::LoadOp::Load);
            pass.set_bind_group(0, &self.globals_group, &[]);
            pass.set_bind_group(1, &self.paint.group, &[]);
            self.draw_balls(&mut pass, frame, &self.buffers);
        }
        if wants_objects {
            let view = &self.targets.object.as_ref().expect("made above").1;
            let mut pass = begin_pass(&mut encoder, view, "objects", wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
            pass.set_bind_group(0, &self.globals_group, &[]);
            pass.set_bind_group(1, &self.paint.group, &[]);
            self.draw_objects(&mut pass, frame, &self.buffers);
            // Blended things are drawn above the rest, so they are on top here too.
            for (l, b) in frame.layers.iter().zip(&layer_buffers) {
                self.draw_objects(&mut pass, &l.frame, b);
            }
        }
        let object_view = self.targets.object.as_ref().filter(|_| wants_objects).map(|(_, v)| v.clone());
        self.chain.run(
            device,
            queue,
            &mut encoder,
            Stage::Things,
            frame,
            &self.targets.hdr_texture,
            &self.targets.hdr,
            object_view.as_ref(),
        );
        if !frame.flashes.is_empty() || !frame.bursts.is_empty() || !frame.glyphs.is_empty() {
            let mut pass = begin_pass(&mut encoder, &self.targets.hdr, "flashes", wgpu::LoadOp::Load);
            pass.set_bind_group(0, &self.globals_group, &[]);
            self.draw_front(&mut pass, frame, &self.buffers, &world);
        }
        let mut slot = 0;
        for (l, b) in frame.layers.iter().zip(&layer_buffers) {
            self.draw_layer(device, queue, &mut encoder, l, b, &world, slot, &mut liquid_slot);
            slot += 1;
        }
        if let Some(o) = overlay {
            let screen = Through {
                globals: self.screen_globals_group.clone(),
                paint: self.overlay_paint.group.clone(),
                world: false,
            };
            let buffers = std::mem::take(&mut self.overlay);
            self.draw_things(device, queue, &mut encoder, o, &buffers, &screen, &hdr, &mut liquid_slot);
            self.overlay = buffers;
            for (l, b) in o.layers.iter().zip(&layer_buffers[frame.layers.len()..]) {
                self.draw_layer(device, queue, &mut encoder, l, b, &screen, slot, &mut liquid_slot);
                slot += 1;
            }
        }
        self.layer_buffers = layer_buffers;
        self.chain.run(
            device,
            queue,
            &mut encoder,
            Stage::Picture,
            frame,
            &self.targets.hdr_texture,
            &self.targets.hdr,
            object_view.as_ref(),
        );
        self.chain.end_frame();
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

    /// Copies the HDR picture into `behind`: what liquids bend, and what a blend layer is mixed
    /// with.
    fn copy_under(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.copy_texture_to_texture(
            self.targets.hdr_texture.as_image_copy(),
            self.targets.behind.as_image_copy(),
            wgpu::Extent3d { width: self.targets.width, height: self.targets.height, depth_or_array_layers: 1 },
        );
    }

    /// Trails, shapes and soft bodies, then (in the world) swarms: what sits behind the rest.
    fn draw_back(&self, pass: &mut wgpu::RenderPass<'_>, f: &Frame, b: &FrameBuffers, through: &Through) {
        if !f.trails.is_empty() {
            pass.set_pipeline(&self.trail);
            pass.set_vertex_buffer(0, b.trails.slice());
            pass.draw(0..f.trails.len() as u32, 0..1);
        }
        if !f.shapes.is_empty() {
            pass.set_pipeline(&self.shape);
            pass.set_vertex_buffer(0, b.shapes.slice());
            pass.draw(0..4, 0..f.shapes.len() as u32);
        }
        let soft_vertices = f.soft.len() - f.soft.len() % 3;
        if soft_vertices > 0 {
            pass.set_pipeline(&self.soft);
            pass.set_vertex_buffer(0, b.soft.slice());
            pass.draw(0..soft_vertices as u32, 0..1);
        }
        if let (true, Some(flow)) = (through.world, &self.flow) {
            flow.draw_swarms(pass, &through.globals, &through.paint, f);
        }
    }

    /// Each of `f`'s liquids, shaded into `out` over what is there, bending `behind`.
    /// `slot` counts the liquids drawn so far this frame.
    fn draw_liquids(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        f: &Frame,
        out: &wgpu::TextureView,
        slot: &mut usize,
    ) {
        for layer in &f.liquids {
            let points = match &layer.points {
                Points::Cpu(points) => Some(flow::surface::PointsBinding::Cpu(points)),
                Points::Gpu(_) => self.flow.as_ref().and_then(|flow| flow.gpu_points(layer.thing)),
            };
            if let Some(points) = points {
                let world = (f.width, f.height);
                self.surface.draw(
                    device,
                    queue,
                    encoder,
                    &self.targets,
                    *slot,
                    world,
                    &f.camera,
                    layer,
                    points,
                    &self.paint.group,
                    out,
                );
            }
            *slot += 1;
        }
    }

    /// Balls and styled balls.
    fn draw_balls(&self, pass: &mut wgpu::RenderPass<'_>, f: &Frame, b: &FrameBuffers) {
        if !f.balls.is_empty() {
            pass.set_pipeline(&self.ball);
            pass.set_vertex_buffer(0, b.balls.slice());
            pass.draw(0..4, 0..f.balls.len() as u32);
        }
        if !f.looks.is_empty() {
            pass.set_pipeline(&self.look);
            pass.set_vertex_buffer(0, b.looks.slice());
            pass.draw(0..4, 0..f.looks.len() as u32);
        }
    }

    /// Flashes, particles and glyphs, in front of everything; the globals are bound already.
    fn draw_front(&self, pass: &mut wgpu::RenderPass<'_>, f: &Frame, b: &FrameBuffers, through: &Through) {
        if !f.flashes.is_empty() {
            pass.set_pipeline(&self.flash);
            pass.set_bind_group(1, &through.paint, &[]);
            pass.set_vertex_buffer(0, b.flashes.slice());
            pass.draw(0..4, 0..f.flashes.len() as u32);
        }
        self.particles.draw_list(pass, &through.paint, &b.bursts);
        if let (false, Some(atlas)) = (f.glyphs.is_empty(), &self.atlas) {
            pass.set_pipeline(&self.glyph);
            pass.set_bind_group(1, &atlas.group, &[]);
            pass.set_bind_group(2, &through.paint, &[]);
            pass.set_vertex_buffer(0, b.glyphs.slice());
            pass.draw(0..4, 0..f.glyphs.len() as u32);
        }
    }

    /// Where `f`'s shapes, balls and styled balls are, for effects on things.
    fn draw_objects(&self, pass: &mut wgpu::RenderPass<'_>, f: &Frame, b: &FrameBuffers) {
        if !f.shapes.is_empty() {
            pass.set_pipeline(&self.shape_object);
            pass.set_vertex_buffer(0, b.shapes.slice());
            pass.draw(0..4, 0..f.shapes.len() as u32);
        }
        if !f.balls.is_empty() {
            pass.set_pipeline(&self.ball_object);
            pass.set_vertex_buffer(0, b.balls.slice());
            pass.draw(0..4, 0..f.balls.len() as u32);
        }
        if !f.looks.is_empty() {
            pass.set_pipeline(&self.look_object);
            pass.set_vertex_buffer(0, b.looks.slice());
            pass.draw(0..4, 0..f.looks.len() as u32);
        }
    }

    /// Draws everything in `f` into `view` over what is there, in the usual order: trails,
    /// shapes, soft bodies, swarms, liquids, balls, styled balls, flashes, particles and glyphs.
    /// Used for the things stuck to the screen and for each blend layer.
    #[allow(clippy::too_many_arguments)]
    fn draw_things(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        f: &Frame,
        b: &FrameBuffers,
        through: &Through,
        view: &wgpu::TextureView,
        liquid_slot: &mut usize,
    ) {
        {
            let mut pass = begin_pass(encoder, view, "things", wgpu::LoadOp::Load);
            pass.set_bind_group(0, &through.globals, &[]);
            pass.set_bind_group(1, &through.paint, &[]);
            self.draw_back(&mut pass, f, b, through);
        }
        if through.world {
            self.draw_liquids(device, queue, encoder, f, view, liquid_slot);
        }
        let mut pass = begin_pass(encoder, view, "things in front", wgpu::LoadOp::Load);
        pass.set_bind_group(0, &through.globals, &[]);
        pass.set_bind_group(1, &through.paint, &[]);
        self.draw_balls(&mut pass, f, b);
        self.draw_front(&mut pass, f, b, through);
    }

    /// Draws one blend layer's things into the layer picture, then mixes it into the HDR
    /// picture over a copy of what is there. `slot` counts the layers mixed so far this frame.
    #[allow(clippy::too_many_arguments)]
    fn draw_layer(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        l: &BlendLayer,
        b: &FrameBuffers,
        through: &Through,
        slot: usize,
        liquid_slot: &mut usize,
    ) {
        // The picture under the layer: what its liquids bend and what the composite mixes with.
        self.copy_under(encoder);
        drop(begin_pass(encoder, &self.targets.layer, "blend layer", wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)));
        let layer = self.targets.layer.clone();
        self.draw_things(device, queue, encoder, &l.frame, b, through, &layer, liquid_slot);
        self.blend.composite(
            device,
            queue,
            encoder,
            slot,
            l.blend,
            l.amount,
            &self.targets.behind_view,
            &self.targets.layer,
            &self.targets.hdr,
        );
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
        // Effects read the HDR picture while they draw their result beside it, then copy it back;
        // the liquids copy it too, to bend and tint what is behind them.
        let copyable = render_and_sample | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST;
        let hdr_texture = texture("hdr", width, height, HDR_FORMAT, copyable);
        let hdr = hdr_texture.create_view(&Default::default());
        let behind = texture(
            "behind",
            width,
            height,
            HDR_FORMAT,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        );
        let behind_view = behind.create_view(&Default::default());
        let layer =
            texture("blend layer", width, height, HDR_FORMAT, render_and_sample).create_view(&Default::default());
        let thickness_size = ((width / 2).max(1), (height / 2).max(1));
        let thickness =
            texture("liquid thickness", thickness_size.0, thickness_size.1, THICKNESS_FORMAT, render_and_sample)
                .create_view(&Default::default());
        let blur = texture("liquid blur", thickness_size.0, thickness_size.1, THICKNESS_FORMAT, render_and_sample)
            .create_view(&Default::default());
        let speed = texture("liquid speed", thickness_size.0, thickness_size.1, THICKNESS_FORMAT, render_and_sample)
            .create_view(&Default::default());
        let speed_blur =
            texture("liquid speed blur", thickness_size.0, thickness_size.1, THICKNESS_FORMAT, render_and_sample)
                .create_view(&Default::default());
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
        Self {
            width,
            height,
            hdr_texture,
            hdr,
            behind,
            behind_view,
            layer,
            thickness,
            blur,
            speed,
            speed_blur,
            thickness_size,
            object: None,
            bloom,
            output,
            output_view,
            down_sources,
            up_sources,
            composite,
        }
    }

    /// Makes the object picture if it is missing.
    fn ensure_object(&mut self, device: &wgpu::Device) {
        if self.object.is_none() {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("objects"),
                size: wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: OBJECT_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            self.object = Some((texture, view));
        }
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

/// Copies finished frames back to the CPU a few frames behind the drawing, so the graphics card
/// draws the next frame while the last one is being read and encoded. Frames come back in the
/// order they were drawn, as tightly packed RGBA bytes (sRGB-encoded).
pub struct Readback {
    slots: Vec<ReadbackSlot>,
    next: usize,
    width: u32,
    height: u32,
    padded_row: usize,
}

struct ReadbackSlot {
    buffer: wgpu::Buffer,
    /// The copy in flight: its submission and the result of mapping the buffer.
    pending: Option<(wgpu::SubmissionIndex, std::sync::mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>)>,
}

impl Readback {
    /// `depth` frames can be in flight at once.
    pub fn new(device: &wgpu::Device, width: u32, height: u32, depth: usize) -> Self {
        let padded_row = (width as usize * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize);
        let slots = (0..depth.max(1))
            .map(|_| ReadbackSlot {
                buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("readback"),
                    size: (padded_row * height as usize) as u64,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
                pending: None,
            })
            .collect();
        Self { slots, next: 0, width, height, padded_row }
    }

    /// Starts copying the renderer's current output. If all slots are busy, the oldest frame is
    /// finished first and returned.
    pub fn push(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        renderer: &Renderer,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        anyhow::ensure!(renderer.size() == (self.width, self.height), "readback size doesn't match the renderer");
        let index = self.next;
        let oldest = self.take(device, index)?;
        let slot = &mut self.slots[index];
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("readback") });
        encoder.copy_texture_to_buffer(
            renderer.output_texture().as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &slot.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_row as u32),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
        );
        let submission = queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        slot.buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        slot.pending = Some((submission, receiver));
        self.next = (index + 1) % self.slots.len();
        Ok(oldest)
    }

    /// Waits for every frame still in flight and returns them, oldest first.
    pub fn finish(&mut self, device: &wgpu::Device) -> anyhow::Result<Vec<Vec<u8>>> {
        let mut frames = Vec::new();
        for k in 0..self.slots.len() {
            let index = (self.next + k) % self.slots.len();
            frames.extend(self.take(device, index)?);
        }
        Ok(frames)
    }

    fn take(&mut self, device: &wgpu::Device, index: usize) -> anyhow::Result<Option<Vec<u8>>> {
        let slot = &mut self.slots[index];
        let Some((submission, receiver)) = slot.pending.take() else { return Ok(None) };
        device.poll(wgpu::PollType::Wait { submission_index: Some(submission), timeout: None })?;
        receiver.recv()??;
        let row = self.width as usize * 4;
        let mut pixels = Vec::with_capacity(row * self.height as usize);
        {
            let mapped = slot.buffer.slice(..).get_mapped_range()?;
            for chunk in mapped.chunks(self.padded_row) {
                pixels.extend_from_slice(&chunk[..row]);
            }
        }
        slot.buffer.unmap();
        Ok(Some(pixels))
    }
}

/// Creates a GPU device without a window, for exporting from the command line and for tests.
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
mod surface_tests {
    use super::*;
    use bpe_core::ThingId;
    use bpe_runtime::Outline;

    const W: u32 = 270;
    const H: u32 = 480;

    fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
        match headless_device() {
            Ok(pair) => Some(pair),
            Err(_) => {
                eprintln!("no GPU adapter available, skipping");
                None
            }
        }
    }

    /// A 200×200 block of particles around `center`, as a liquid thing's draw part sends them.
    fn block(center: [f32; 2], clarity: f32, refraction: f32) -> LiquidLayer {
        let mut points = Vec::new();
        let spacing = 200.0 / 22.0;
        for row in 0..22 {
            for col in 0..22 {
                let shift = if row % 2 == 1 { spacing * 0.5 } else { 0.0 };
                points.push([
                    center[0] - 100.0 + spacing * 0.5 + col as f32 * spacing + shift,
                    center[1] - 100.0 + spacing * 0.5 + row as f32 * spacing * 0.866,
                    0.0,
                    0.0,
                ]);
            }
        }
        LiquidLayer {
            thing: ThingId(1),
            points: Points::Cpu(points),
            radius: spacing * 0.5,
            color: [0.1, 0.3, 1.0, 1.0],
            follow: 0,
            paint: PaintGpu::default(),
            paint_place: [0.0; 4],
            clarity,
            refraction,
            highlight: 0.8,
            smoothing: 6.0,
            glow: 0.0,
        }
    }

    fn pixel(pixels: &[u8], x: f32, y: f32) -> [u8; 4] {
        let (px, py) = (((x + 540.0) / 4.0) as usize, ((y + 960.0) / 4.0) as usize);
        let i = (py * W as usize + px) * 4;
        [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
    }

    fn render(renderer: &mut Renderer, device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame) -> Vec<u8> {
        renderer.render(device, queue, frame);
        renderer.read_output(device, queue).unwrap()
    }

    #[test]
    fn world_text_draws_without_flashes_or_bursts() {
        let Some((device, queue)) = gpu() else { return };
        let mut renderer = Renderer::new(&device, W, H);
        let mut frame = Frame::default();
        let style = text::TextStyle { color: [1.0, 1.0, 1.0, 1.0], outline: 0.0, outline_color: [0.0; 4], glow: 0.0 };
        let anchor = glam::Vec2::new(0.0, 0.0);
        text::layout_text(
            "HELLO",
            text::FontFace::Anton,
            200.0,
            anchor,
            text::TextAlign::Center,
            0.0,
            usize::MAX,
            &style,
            &mut frame.glyphs,
        );
        assert!(!frame.glyphs.is_empty());
        assert!(frame.flashes.is_empty() && frame.bursts.is_empty());
        let pixels = render(&mut renderer, &device, &queue, &frame);
        let lit = pixels.chunks(4).filter(|p| p[0] > 150 && p[1] > 150 && p[2] > 150).count();
        assert!(lit > 100, "{lit} lit pixels: world text shows on a frame with no flashes or bursts");
    }

    #[test]
    fn cpu_points_draw_a_coloured_surface() {
        let Some((device, queue)) = gpu() else { return };
        let mut renderer = Renderer::new(&device, W, H);
        let mut frame = Frame::default();
        frame.liquids.push(block([0.0, 300.0], 0.0, 0.5));
        let pixels = render(&mut renderer, &device, &queue, &frame);
        let blue = pixels.chunks(4).filter(|p| p[2] > 100 && p[2] as i32 > p[0] as i32 + 40).count();
        assert!(blue > 1200, "{blue} blue pixels for a 50×50 block");
        let far = pixels.chunks(4).enumerate().filter(|(i, p)| {
            let (x, y) = ((i % W as usize) as f32 * 4.0 - 540.0, (i / W as usize) as f32 * 4.0 - 960.0);
            (x.abs() > 160.0 || (y - 300.0).abs() > 160.0) && (p[0] > 40 || p[1] > 40 || p[2] > 40)
        });
        assert_eq!(far.count(), 0, "nothing lights up far from the block");
        // The body is one smooth sheet: the centre pixel and its neighbour agree.
        let a = pixel(&pixels, 0.0, 300.0);
        let b = pixel(&pixels, 8.0, 300.0);
        assert!(a[2] > 120 && (a[2] as i32 - b[2] as i32).abs() < 20, "{a:?} {b:?}");
    }

    #[test]
    fn liquid_speed_paints_the_fast_block() {
        let Some((device, queue)) = gpu() else { return };
        let mut renderer = Renderer::new(&device, W, H);
        let mut frame = Frame { bloom: 0.0, ..Frame::default() };
        let row = frame
            .gradients
            .row(&bpe_core::Gradient::even(&[bpe_core::Color::rgb(0, 0, 255), bpe_core::Color::rgb(255, 0, 0)]));
        // A still block on the left and one racing at 1000 px/s on the right, one liquid.
        let mut liquid = block([-200.0, 300.0], 0.0, 0.0);
        let Points::Cpu(still) = &liquid.points else { unreachable!() };
        let fast = still.iter().map(|p| [p[0] + 400.0, p[1], 1000.0 * 0.6, 1000.0 * 0.8]);
        let all: Vec<_> = still.iter().copied().chain(fast).collect();
        liquid.points = Points::Cpu(all);
        // Green, so a surface that ignores its paint fails.
        liquid.color = [0.0, 1.0, 0.0, 1.0];
        liquid.follow = 3;
        liquid.paint = PaintGpu { row: row as f32, ..PaintGpu::default() };
        liquid.paint_place = [0.0, 1000.0, 0.0, 0.0];
        frame.liquids.push(liquid);
        let pixels = render(&mut renderer, &device, &queue, &frame);
        let (slow, fast) = (pixel(&pixels, -200.0, 300.0), pixel(&pixels, 200.0, 300.0));
        assert!(slow[1] < 60 && fast[1] < 60, "{slow:?} {fast:?}: green is the liquid's own colour");
        assert!(slow[2] > 120 && slow[0] < 60, "{slow:?}: the still block is the first stop");
        assert!(fast[0] > 120 && fast[2] < 60, "{fast:?}: the fast block is the last stop");
    }

    #[test]
    fn liquid_place_is_measured_in_the_world_through_the_camera() {
        let Some((device, queue)) = gpu() else { return };
        let mut renderer = Renderer::new(&device, W, H);
        let mut frame = Frame { bloom: 0.0, ..Frame::default() };
        let row = frame
            .gradients
            .row(&bpe_core::Gradient::even(&[bpe_core::Color::rgb(0, 0, 255), bpe_core::Color::rgb(255, 0, 0)]));
        let mut liquid = block([0.0, 300.0], 0.0, 0.0);
        liquid.color = [0.0, 1.0, 0.0, 1.0];
        liquid.follow = 1;
        liquid.paint = PaintGpu { row: row as f32, ..PaintGpu::default() };
        // Left to right across the block: blue on its left, red on its right.
        liquid.paint_place = [0.0, 200.0, 0.0, 300.0];
        frame.liquids.push(liquid);
        // Turned a quarter round and zoomed in on the block: its left is at the screen's bottom.
        let turn = std::f32::consts::FRAC_PI_2;
        frame.camera = bpe_runtime::CameraView { center: glam::Vec2::new(0.0, 300.0), zoom: 2.0, turn };
        let pixels = render(&mut renderer, &device, &queue, &frame);
        // World x -60 and 60, a fifth of the way in from each end.
        let (world_left, world_right) = (pixel(&pixels, 0.0, 120.0), pixel(&pixels, 0.0, -120.0));
        assert!(world_left[1] < 60 && world_right[1] < 60, "{world_left:?} {world_right:?}: no own green");
        assert!(world_left[2] > world_left[0] + 60, "{world_left:?}: the world's left is blue");
        assert!(world_right[0] > world_right[2] + 60, "{world_right:?}: the world's right is red");
    }

    #[test]
    fn the_surface_sits_behind_the_balls_and_in_front_of_the_wall() {
        let Some((device, queue)) = gpu() else { return };
        let mut renderer = Renderer::new(&device, W, H);
        let ring =
            Outline::Ring { center: glam::Vec2::new(0.0, 300.0), radius: 60.0, sides: 0, angle: 0.0, thickness: 12.0 };
        let mut frame = Frame::default();
        frame.shapes.push(ShapeInstance::outline(&ring, PaintGpu::default(), [1.0, 1.0, 1.0, 1.0], 0.0));
        let plain = render(&mut renderer, &device, &queue, &frame);
        let wall_before = pixel(&plain, 60.0, 300.0);
        assert!(wall_before[0] > 150, "{wall_before:?}: the wall is white");
        frame.liquids.push(block([0.0, 300.0], 0.0, 0.0));
        frame.balls.push(BallInstance {
            center: [0.0, 300.0],
            radius: 30.0,
            glow: 0.0,
            color: [1.0, 1.0, 1.0, 1.0],
            code: [0.0; 4],
            paint: crate::PaintGpu::default(),
        });
        let pixels = render(&mut renderer, &device, &queue, &frame);
        let ball = pixel(&pixels, 0.0, 300.0);
        assert!(ball[0] > 180 && ball[1] > 180, "{ball:?}: the ball stays white over the liquid");
        let wall = pixel(&pixels, 60.0, 300.0);
        assert!(wall[0] < wall_before[0] - 40 && wall[2] > wall[0], "{wall:?}: the wall under the liquid is tinted");
    }

    #[test]
    fn clarity_one_is_see_through() {
        let Some((device, queue)) = gpu() else { return };
        let mut renderer = Renderer::new(&device, W, H);
        let mut frame = Frame::default();
        frame.liquids.push(block([0.0, 300.0], 0.0, 0.0));
        let opaque = pixel(&render(&mut renderer, &device, &queue, &frame), 0.0, 300.0);
        frame.liquids[0].clarity = 1.0;
        let clear = pixel(&render(&mut renderer, &device, &queue, &frame), 0.0, 300.0);
        assert!(opaque[2] > 150, "{opaque:?}");
        assert!(clear[2] < opaque[2] / 2, "{clear:?} over black shows mostly the black behind");
    }
}

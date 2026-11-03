//! The modular Ball Studio's renderer.
//!
//! Each thing's drawing part writes instances into a [`Frame`]; the [`Renderer`] draws the frame
//! into an HDR texture, adds bloom, and writes the result as sRGB-encoded `Rgba8Unorm`, which
//! egui can show directly and video encoders read as ordinary RGBA bytes.

mod blend;
#[cfg(test)]
mod blend_tests;
mod each;
pub mod effects;
mod frame;
#[cfg(test)]
mod grade_tests;
mod gradients;
#[cfg(test)]
mod paint_tests;
pub mod particles;
mod util;
#[cfg(test)]
mod pictures;

pub use frame::{
    BackgroundPaint, BallInstance, BlendLayer, Burst, DrawCx, Draws, EffectCall, FlashInstance, Frame, Grade, HitMark,
    LookInstance, ModuleDraw, ShapeInstance, Stage, ThingDraw, TrailVertex, blend_alpha, build_frame, hit_strength,
    push_streak, push_streak_colours, thing_code,
};
pub use gradients::{ATLAS_WIDTH, GradientAtlas, NO_VARY, PaintGpu};
pub use wgpu;

use bpe_runtime::CameraView;
use bytemuck::{Pod, Zeroable};

pub const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
pub(crate) const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// The object picture shader effects on things read: own position, thing code, coverage.
pub(crate) const OBJECT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;
const BLOOM_LEVELS: usize = 6;

// Every instance carries its paint (`PaintGpu`) as its last three vec4s.
const SHAPE_ATTRIBUTES: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![
    0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4,
    6 => Float32x4
];
const BALL_ATTRIBUTES: [wgpu::VertexAttribute; 8] = wgpu::vertex_attr_array![
    0 => Float32x2, 1 => Float32, 2 => Float32, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4,
    7 => Float32x4
];
const TRAIL_ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4];
const LOOK_ATTRIBUTES: [wgpu::VertexAttribute; 12] = wgpu::vertex_attr_array![
    0 => Float32x2, 1 => Float32, 2 => Float32, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4,
    6 => Float32x4, 7 => Float32x4, 8 => Float32x4, 9 => Float32x4, 10 => Float32x4, 11 => Float32x4
];
const FLASH_ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32, 2 => Float32x4];
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Globals {
    view: [f32; 4],
    /// The background paint: atlas row (below 0 paints black), start, scroll, repeat.
    bg_paint: [f32; 4],
    /// The background's place: direction in degrees, span, centre xy.
    bg_place: [f32; 4],
    /// x: bloom strength, y: exposure, z: 1 to run the colour grade, w: in a layer whose things
    /// blend with each other, the blend's `shader_code` (see each.wgsl).
    post: [f32; 4],
    /// The camera: centre xy, zoom, turn in radians.
    camera: [f32; 4],
    /// x: 1 when the background stays on the screen, y: how its place runs (0 none, 1 direction,
    /// 2 centre), z: 1 for hard steps, w: 1 to mix `bg_from` and `bg_to`.
    bg: [f32; 4],
    /// The two colours of a plain blend the background mixes itself (`bg.w` is 1 then).
    bg_from: [f32; 4],
    bg_to: [f32; 4],
    /// The colour grade: brightness, contrast, saturation, warmth.
    grade: [f32; 4],
    /// The colour grade: tint, vignette, vignette size, gradient map amount.
    grade_more: [f32; 4],
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
            post: [frame.bloom, frame.exposure, if frame.grade.is_default() { 0.0 } else { 1.0 }, 0.0],
            camera: [camera.center.x, camera.center.y, camera.zoom, camera.turn],
            bg: [
                if frame.background_on_screen { 1.0 } else { 0.0 },
                frame.background.mode,
                frame.background.steps,
                if frame.background.ends.is_some() { 1.0 } else { 0.0 },
            ],
            bg_from: frame.background.ends.map_or([0.0; 4], |e| e[0]),
            bg_to: frame.background.ends.map_or([0.0; 4], |e| e[1]),
            grade: [frame.grade.brightness, frame.grade.contrast, frame.grade.saturation, frame.grade.warmth],
            grade_more: [frame.grade.tint, frame.grade.vignette, frame.grade.vignette_size, frame.grade.map_amount],
        }
    }
}

/// Size-dependent GPU resources, rebuilt when the output size changes.
struct Targets {
    width: u32,
    height: u32,
    hdr_texture: wgpu::Texture,
    hdr: wgpu::TextureView,
    /// A copy of the HDR image under a blend layer, for the composite to read.
    behind: wgpu::Texture,
    behind_view: wgpu::TextureView,
    /// What the things of one blend layer draw into, before it is mixed into the HDR image;
    /// made the first time a frame has a layer.
    layer: Option<(wgpu::Texture, wgpu::TextureView)>,
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

/// The colour grade's gradient map, baked into one 256×1 row the composite reads. It is always
/// there, and written only when a frame maps by an amount above 0 with a gradient it does not
/// hold yet.
struct GradeMap {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    /// The gradient baked into the texture, once one has been.
    baked: Option<bpe_core::Gradient>,
}

impl GradeMap {
    fn new(device: &wgpu::Device) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("grade map"),
            size: wgpu::Extent3d { width: ATLAS_WIDTH as u32, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        Self { texture, view, baked: None }
    }

    /// Bakes `grade`'s map into the texture when it is used and not there already.
    fn sync(&mut self, queue: &wgpu::Queue, grade: &Grade) {
        if grade.map_amount <= 0.0 || self.baked.as_ref() == Some(&grade.map) {
            return;
        }
        let mut row = [[0.0; 4]; ATLAS_WIDTH];
        row.copy_from_slice(&grade.map.bake(ATLAS_WIDTH));
        queue.write_texture(
            self.texture.as_image_copy(),
            &gradients::half_texels(&[row]),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(ATLAS_WIDTH as u32 * 8),
                rows_per_image: None,
            },
            wgpu::Extent3d { width: ATLAS_WIDTH as u32, height: 1, depth_or_array_layers: 1 },
        );
        self.baked = Some(grade.map.clone());
    }
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
    /// For layers whose things blend with each other.
    each: each::EachPipelines,
    each_globals: each::EachGlobals,
    globals_layout: wgpu::BindGroupLayout,
    post_layout: wgpu::BindGroupLayout,
    composite_layout: wgpu::BindGroupLayout,
    /// The gradient atlas: bound at group 1 of the scene passes and group 2 of the particles.
    paint_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    background: wgpu::RenderPipeline,
    trail: wgpu::RenderPipeline,
    shape: wgpu::RenderPipeline,
    ball: wgpu::RenderPipeline,
    look: wgpu::RenderPipeline,
    flash: wgpu::RenderPipeline,
    /// What is where, for effects on things (see object.wgsl).
    shape_object: wgpu::RenderPipeline,
    ball_object: wgpu::RenderPipeline,
    look_object: wgpu::RenderPipeline,
    chain: effects::chain::Chain,
    particles: particles::Particles,
    downsample: wgpu::RenderPipeline,
    upsample: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    paint: PaintTexture,
    /// The overlay frame's own gradients: its instances name rows of its own atlas.
    overlay_paint: PaintTexture,
    targets: Targets,
    grade_map: GradeMap,
}

/// The instance lists of one frame (the main one, its overlay or a blend layer) on the GPU.
struct FrameBuffers {
    trails: GrowableBuffer,
    shapes: GrowableBuffer,
    balls: GrowableBuffer,
    looks: GrowableBuffer,
    flashes: GrowableBuffer,
    bursts: particles::BurstList,
}

impl Default for FrameBuffers {
    fn default() -> Self {
        Self {
            trails: GrowableBuffer::new("trail vertices"),
            shapes: GrowableBuffer::new("shape instances"),
            balls: GrowableBuffer::new("ball instances"),
            looks: GrowableBuffer::new("styled ball instances"),
            flashes: GrowableBuffer::new("flash instances"),
            bursts: particles::BurstList::default(),
        }
    }
}

impl FrameBuffers {
    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, particles: &particles::Particles, f: &Frame) {
        self.trails.upload(device, queue, bytemuck::cast_slice(&f.trails));
        self.shapes.upload(device, queue, bytemuck::cast_slice(&f.shapes));
        self.balls.upload(device, queue, bytemuck::cast_slice(&f.balls));
        self.looks.upload(device, queue, bytemuck::cast_slice(&f.looks));
        self.flashes.upload(device, queue, bytemuck::cast_slice(&f.flashes));
        particles.fill(&mut self.bursts, device, queue, particles::pick(&f.bursts, f.time));
    }
}

/// How one frame's things are drawn: through which globals and from which gradient atlas.
#[derive(Clone)]
struct Through {
    globals: wgpu::BindGroup,
    paint: wgpu::BindGroup,
    /// Drawing the world (not the things stuck to the screen).
    world: bool,
    /// The camera the world is seen through: the main frame's, which a blend layer's own copies
    /// may lag behind (the editor's World view sets them after building).
    camera: CameraView,
    /// Drawing into a layer whose things blend with each other by this blend.
    each: Option<bpe_core::Blend>,
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
                    include_str!("shaders/each.wgsl"),
                ]
                .concat()
                .into(),
            ),
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
            entries: &[texture_entry(0), sampler_entry, texture_entry(2), texture_entry(3)],
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

        let instances = |stride, attributes| wgpu::VertexBufferLayout {
            array_stride: stride,
            step_mode: wgpu::VertexStepMode::Instance,
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
            &[Some(instances(std::mem::size_of::<ShapeInstance>() as u64, &SHAPE_ATTRIBUTES))],
            TriangleStrip,
            HDR_FORMAT,
            Some(premultiplied),
        );
        let ball = make(
            "balls",
            &scene_pipeline_layout,
            &scene_shader,
            "vs_ball",
            "fs_ball",
            &[Some(instances(std::mem::size_of::<BallInstance>() as u64, &BALL_ATTRIBUTES))],
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
                attributes: &TRAIL_ATTRIBUTES,
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
            &[Some(instances(std::mem::size_of::<LookInstance>() as u64, &LOOK_ATTRIBUTES))],
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
            &[Some(instances(std::mem::size_of::<FlashInstance>() as u64, &FLASH_ATTRIBUTES))],
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
            &[Some(instances(std::mem::size_of::<ShapeInstance>() as u64, &SHAPE_ATTRIBUTES))],
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
            &[Some(instances(std::mem::size_of::<BallInstance>() as u64, &BALL_ATTRIBUTES))],
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
            &[Some(instances(std::mem::size_of::<LookInstance>() as u64, &LOOK_ATTRIBUTES))],
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

        let each = {
            use each::{Kind, Recipe};
            use wgpu::VertexStepMode::{Instance, Vertex};
            let recipe = |vs, fs, stride: usize, step, attributes, topology| Recipe {
                module: scene_shader.clone(),
                layout: scene_pipeline_layout.clone(),
                vs,
                fs,
                stride: stride as u64,
                step,
                attributes,
                topology,
            };
            use std::mem::size_of;
            each::EachPipelines::new([
                (
                    Kind::Trail,
                    recipe(
                        "vs_trail",
                        "fs_trail_each",
                        size_of::<TrailVertex>(),
                        Vertex,
                        &TRAIL_ATTRIBUTES,
                        TriangleList,
                    ),
                ),
                (
                    Kind::Shape,
                    recipe(
                        "vs_shape",
                        "fs_shape_each",
                        size_of::<ShapeInstance>(),
                        Instance,
                        &SHAPE_ATTRIBUTES,
                        TriangleStrip,
                    ),
                ),
                (
                    Kind::Ball,
                    recipe(
                        "vs_ball",
                        "fs_ball_each",
                        size_of::<BallInstance>(),
                        Instance,
                        &BALL_ATTRIBUTES,
                        TriangleStrip,
                    ),
                ),
                (
                    Kind::Look,
                    recipe(
                        "vs_look",
                        "fs_look_each",
                        size_of::<LookInstance>(),
                        Instance,
                        &LOOK_ATTRIBUTES,
                        TriangleStrip,
                    ),
                ),
                (
                    Kind::Flash,
                    recipe(
                        "vs_flash",
                        "fs_flash_each",
                        size_of::<FlashInstance>(),
                        Instance,
                        &FLASH_ATTRIBUTES,
                        TriangleStrip,
                    ),
                ),
            ])
        };

        let grade_map = GradeMap::new(device);
        let targets = Targets::new(device, width, height, &post_layout, &composite_layout, &sampler, &grade_map.view);
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
            each,
            each_globals: each::EachGlobals::default(),
            globals_layout,
            post_layout,
            composite_layout,
            paint_layout,
            sampler,
            background,
            trail,
            shape,
            ball,
            look,
            flash,
            shape_object,
            ball_object,
            look_object,
            chain: effects::chain::Chain::new(device),
            particles,
            downsample,
            upsample,
            composite,
            paint,
            overlay_paint,
            targets,
            grade_map,
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.targets.width, self.targets.height)
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if self.size() != (width.max(1), height.max(1)) {
            self.targets = Targets::new(
                device,
                width,
                height,
                &self.post_layout,
                &self.composite_layout,
                &self.sampler,
                &self.grade_map.view,
            );
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

    /// Why shader effects in the last frame could not be built, one line each: "{name}: {why}".
    pub fn shader_problems(&self) -> Vec<String> {
        self.chain.problems().map(str::to_owned).collect()
    }

    /// Draws one frame into the output texture. Each blend layer is drawn into its own picture
    /// and mixed in after the frame's flashes (the overlay's layers after the overlay), so
    /// blended things sit above the rest.
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
        self.paint.sync(device, queue, &self.paint_layout, &self.sampler, &frame.gradients);
        self.grade_map.sync(queue, &frame.grade);
        if let Some(o) = overlay {
            self.overlay_paint.sync(device, queue, &self.paint_layout, &self.sampler, &o.gradients);
        }

        if !layers.is_empty() {
            self.targets.ensure_layer(device);
        }
        let wants_objects = effects::chain::Chain::wants_objects(frame);
        if wants_objects {
            self.targets.ensure_object(device);
        }
        self.chain.begin_frame();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        let world = Through {
            globals: self.globals_group.clone(),
            paint: self.paint.group.clone(),
            world: true,
            camera: frame.camera,
            each: None,
        };
        // Counts the blend composites this frame.
        let mut slot = 0;
        {
            let mut pass =
                begin_pass(&mut encoder, &self.targets.hdr, "background", wgpu::LoadOp::Clear(wgpu::Color::BLACK));
            pass.set_bind_group(0, &self.globals_group, &[]);
            pass.set_bind_group(1, &self.paint.group, &[]);
            pass.set_pipeline(&self.background);
            pass.draw(0..3, 0..1);
        }
        {
            let mut pass = begin_pass(&mut encoder, &self.targets.hdr, "scene", wgpu::LoadOp::Load);
            pass.set_bind_group(0, &self.globals_group, &[]);
            pass.set_bind_group(1, &self.paint.group, &[]);
            self.draw_back(&mut pass, frame, &self.buffers, &world);
        }
        {
            let mut pass = begin_pass(&mut encoder, &self.targets.hdr, "balls", wgpu::LoadOp::Load);
            pass.set_bind_group(0, &self.globals_group, &[]);
            pass.set_bind_group(1, &self.paint.group, &[]);
            self.draw_balls(&mut pass, frame, &self.buffers, &world);
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
            effects::chain::Canvas::Main,
            &self.targets.hdr_texture,
            &self.targets.hdr,
            object_view.as_ref(),
        );
        if !frame.flashes.is_empty() || !frame.bursts.is_empty() {
            let mut pass = begin_pass(&mut encoder, &self.targets.hdr, "flashes", wgpu::LoadOp::Load);
            pass.set_bind_group(0, &self.globals_group, &[]);
            self.draw_front(&mut pass, frame, &self.buffers, &world);
        }
        for (i, (l, b)) in frame.layers.iter().zip(&layer_buffers).enumerate() {
            let effects = Some((frame, i, object_view.as_ref()));
            let through = self.layer_through(device, queue, frame, l, Some(i), &world);
            self.draw_layer(device, queue, &mut encoder, l, b, &through, slot, effects);
            slot += 1;
        }
        if let Some(o) = overlay {
            let screen = Through {
                globals: self.screen_globals_group.clone(),
                paint: self.overlay_paint.group.clone(),
                world: false,
                camera: CameraView::default(),
                each: None,
            };
            let buffers = std::mem::take(&mut self.overlay);
            let hdr = self.targets.hdr.clone();
            self.draw_things(&mut encoder, o, &buffers, &screen, &hdr);
            self.overlay = buffers;
            for (l, b) in o.layers.iter().zip(&layer_buffers[frame.layers.len()..]) {
                let through = self.layer_through(device, queue, frame, l, None, &screen);
                self.draw_layer(device, queue, &mut encoder, l, b, &through, slot, None);
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
            effects::chain::Canvas::Main,
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

    /// Copies the HDR picture into `behind`: what a blend layer is mixed with.
    fn copy_under(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.copy_texture_to_texture(
            self.targets.hdr_texture.as_image_copy(),
            self.targets.behind.as_image_copy(),
            wgpu::Extent3d { width: self.targets.width, height: self.targets.height, depth_or_array_layers: 1 },
        );
    }

    /// Trails and shapes: what sits behind the rest.
    fn draw_back(&self, pass: &mut wgpu::RenderPass<'_>, f: &Frame, b: &FrameBuffers, through: &Through) {
        use each::Kind;
        if !f.trails.is_empty() {
            pass.set_pipeline(self.pipeline(&self.trail, Kind::Trail, through));
            pass.set_vertex_buffer(0, b.trails.slice());
            pass.draw(0..f.trails.len() as u32, 0..1);
        }
        if !f.shapes.is_empty() {
            pass.set_pipeline(self.pipeline(&self.shape, Kind::Shape, through));
            pass.set_vertex_buffer(0, b.shapes.slice());
            pass.draw(0..4, 0..f.shapes.len() as u32);
        }
    }

    /// `normal`, or in a layer whose things blend with each other, that layer's pipeline for
    /// `kind` (made by [`each::EachPipelines::ensure`] before the layer is drawn).
    fn pipeline<'a>(
        &'a self,
        normal: &'a wgpu::RenderPipeline,
        kind: each::Kind,
        through: &Through,
    ) -> &'a wgpu::RenderPipeline {
        match through.each {
            Some(blend) => self.each.get(kind, blend.shader_code()),
            None => normal,
        }
    }

    /// Balls and styled balls.
    fn draw_balls(&self, pass: &mut wgpu::RenderPass<'_>, f: &Frame, b: &FrameBuffers, through: &Through) {
        if !f.balls.is_empty() {
            pass.set_pipeline(self.pipeline(&self.ball, each::Kind::Ball, through));
            pass.set_vertex_buffer(0, b.balls.slice());
            pass.draw(0..4, 0..f.balls.len() as u32);
        }
        if !f.looks.is_empty() {
            pass.set_pipeline(self.pipeline(&self.look, each::Kind::Look, through));
            pass.set_vertex_buffer(0, b.looks.slice());
            pass.draw(0..4, 0..f.looks.len() as u32);
        }
    }

    /// Flashes and particles, in front of everything; the globals are bound already.
    fn draw_front(&self, pass: &mut wgpu::RenderPass<'_>, f: &Frame, b: &FrameBuffers, through: &Through) {
        if !f.flashes.is_empty() {
            pass.set_pipeline(self.pipeline(&self.flash, each::Kind::Flash, through));
            pass.set_bind_group(1, &through.paint, &[]);
            pass.set_vertex_buffer(0, b.flashes.slice());
            pass.draw(0..4, 0..f.flashes.len() as u32);
        }
        self.particles.draw_list(pass, &through.paint, &b.bursts);
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
    /// shapes, balls, styled balls, flashes and particles. Used for the things stuck to the
    /// screen and for each blend layer.
    fn draw_things(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        f: &Frame,
        b: &FrameBuffers,
        through: &Through,
        view: &wgpu::TextureView,
    ) {
        {
            let mut pass = begin_pass(encoder, view, "things", wgpu::LoadOp::Load);
            pass.set_bind_group(0, &through.globals, &[]);
            pass.set_bind_group(1, &through.paint, &[]);
            self.draw_back(&mut pass, f, b, through);
        }
        let mut pass = begin_pass(encoder, view, "things in front", wgpu::LoadOp::Load);
        pass.set_bind_group(0, &through.globals, &[]);
        pass.set_bind_group(1, &through.paint, &[]);
        self.draw_balls(&mut pass, f, b, through);
        self.draw_front(&mut pass, f, b, through);
    }

    /// How `l` is drawn: as `through` says, or, when its things blend with each other, through
    /// globals that name its blend (see each.wgsl), with the pipelines it needs made. `index`
    /// is its place in `frame.layers` (none for the overlay's layers, which run no effects).
    ///
    /// This is where a layer's `each` is decided. A Multiply or Darken layer starts as 1 or far
    /// above it, not as nothing, and an effect on one of its things (Things stage) assumes a
    /// layer of colour over nothing: it would paint dark rings round the thing. Such a layer is
    /// drawn as if its things did not blend with each other. Add, Screen, Lighten and Subtract
    /// start as nothing, so they keep blending.
    fn layer_through(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame,
        l: &BlendLayer,
        index: Option<usize>,
        through: &Through,
    ) -> Through {
        let starts_above_nothing = matches!(l.blend, bpe_core::Blend::Multiply | bpe_core::Blend::Darken);
        let has_effects = index.is_some_and(|i| {
            let here = effects::chain::Canvas::Layer(i);
            frame.effects.iter().any(|call| effects::chain::canvas_of(frame, call) == here)
        });
        if !(l.each && l.blend.combines_with_itself()) || (starts_above_nothing && has_effects) {
            return through.clone();
        }
        let code = l.blend.shader_code();
        let mut globals = Globals::new(frame, &through.camera, self.targets.width);
        globals.post[3] = code as f32;
        let key = (code, !through.world);
        let group = self.each_globals.group(device, queue, &self.globals_layout, key, bytemuck::bytes_of(&globals));
        self.each.ensure(device, l.blend, &l.frame);
        Through { globals: group, each: Some(l.blend), ..through.clone() }
    }

    /// Draws one blend layer's things into the layer picture, runs the effects on those things
    /// that belong to it (`effects`: the main frame, the layer's index in it and the object
    /// picture), then mixes it into the HDR picture over a copy of what is there. `slot` counts
    /// the layers mixed so far this frame.
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
        effects: Option<(&Frame, usize, Option<&wgpu::TextureView>)>,
    ) {
        // The picture under the layer: what the composite mixes with.
        self.copy_under(encoder);
        let (texture, layer) = self.targets.layer.clone().expect("made before the frame is drawn");
        // A layer whose things blend with each other starts as its blend's identity.
        let clear = through.each.map_or(wgpu::Color::TRANSPARENT, each::clear);
        drop(begin_pass(encoder, &layer, "blend layer", wgpu::LoadOp::Clear(clear)));
        self.draw_things(encoder, &l.frame, b, through, &layer);
        if let Some((frame, index, objects)) = effects {
            let on = effects::chain::Canvas::Layer(index);
            self.chain.run(device, queue, encoder, Stage::Things, frame, on, &texture, &layer, objects);
        }
        self.blend.composite(
            device,
            queue,
            encoder,
            slot,
            l.blend,
            l.amount,
            through.each.is_some(),
            &self.targets.behind_view,
            &layer,
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
        grade_map: &wgpu::TextureView,
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
        // a blend layer copies it too, to mix with what is under it.
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
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(grade_map) },
            ],
        });
        Self {
            width,
            height,
            hdr_texture,
            hdr,
            behind,
            behind_view,
            layer: None,
            object: None,
            bloom,
            output,
            output_view,
            down_sources,
            up_sources,
            composite,
        }
    }

    /// Makes the blend layer picture if it is missing. Effects on layered things run over it in
    /// place, so it copies like the HDR picture.
    fn ensure_layer(&mut self, device: &wgpu::Device) {
        if self.layer.is_none() {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("blend layer"),
                size: wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: HDR_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            self.layer = Some((texture, view));
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

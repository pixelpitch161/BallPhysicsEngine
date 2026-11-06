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
mod lighting_tests;
mod lighting;
#[cfg(test)]
mod layers_tests;
mod masks;
#[cfg(test)]
mod paint_tests;
#[cfg(test)]
mod paths_tests;
#[cfg(test)]
mod pictures;
mod programs;
#[cfg(test)]
mod programs_tests;
pub mod sprites;
#[cfg(test)]
mod sprites_tests;
#[cfg(test)]
mod sub_layers_tests;
mod util;

pub use frame::{
    Around, BackgroundPaint, BallInstance, BlendLayer, DrawCx, Draws, EffectCall, FlashInstance, Frame, Grade, HitMark,
    LayerGpu, LayeredInstance, LightGpu, LightScene, LightingGpu, LookInstance, MAX_LAYERS, MOST_TRAIL_VERTICES, ModuleDraw, NO_HIT, Particle,
    PathInstance, Place, ShapeInstance, Stage, StreakStyle, ThingDraw, TrailVertex, blend_alpha, build_frame,
    build_frame_with, hit_strength, push_streak, push_streak_colours, push_trail, thing_code,
};
pub use gradients::{ATLAS_WIDTH, GradientAtlas, NO_VARY, PaintGpu};
pub use masks::{MASK_TILE, MASK_TILE_BYTES, MAX_MASK_TILES, TILE_DISTANCE_REACH, tile_distance};
pub use programs::{LayeredRun, LookProgram, ProgramStore, in_ball_problem};
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
const TRAIL_ATTRIBUTES: [wgpu::VertexAttribute; 5] =
    wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32x2, 4 => Float32x2];
const LOOK_ATTRIBUTES: [wgpu::VertexAttribute; 12] = wgpu::vertex_attr_array![
    0 => Float32x2, 1 => Float32, 2 => Float32, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4,
    6 => Float32x4, 7 => Float32x4, 8 => Float32x4, 9 => Float32x4, 10 => Float32x4, 11 => Float32x4
];
pub(crate) const LAYERED_ATTRIBUTES: [wgpu::VertexAttribute; 12] = wgpu::vertex_attr_array![
    0 => Float32x2, 1 => Float32, 2 => Float32, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4,
    6 => Float32x4, 7 => Float32x4, 8 => Uint32x4, 9 => Float32x4, 10 => Float32x4, 11 => Float32x4
];
pub(crate) const PATH_ATTRIBUTES: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
    0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Uint32x4, 5 => Float32x4
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
    /// Lighting: x the light pass (0 the picture as always, 1 lit colour, 2 surface), y how much
    /// the background takes light (see lighting.wgsl).
    light: [f32; 4],
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
            light: [0.0; 4],
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
    /// The layer records of layered balls: group 2 of their pipelines.
    layers_layout: wgpu::BindGroupLayout,
    layered_pipeline_layout: wgpu::PipelineLayout,
    /// The pipelines of stacks with shader layers, maybe shared with other renderers.
    programs: ProgramStore,
    /// The last frame drew some balls without their shader layers, their program still building.
    waiting: bool,
    sampler: wgpu::Sampler,
    background: wgpu::RenderPipeline,
    trail: wgpu::RenderPipeline,
    shape: wgpu::RenderPipeline,
    ball: wgpu::RenderPipeline,
    look: wgpu::RenderPipeline,
    layered: wgpu::RenderPipeline,
    flash: wgpu::RenderPipeline,
    /// What is where, for effects on things (see object.wgsl).
    shape_object: wgpu::RenderPipeline,
    /// Shapes (see paths.wgsl), and where they are for effects on things.
    path: wgpu::RenderPipeline,
    path_object: wgpu::RenderPipeline,
    ball_object: wgpu::RenderPipeline,
    look_object: wgpu::RenderPipeline,
    layered_object: wgpu::RenderPipeline,
    chain: effects::chain::Chain,
    sprites: sprites::Sprites,
    downsample: wgpu::RenderPipeline,
    upsample: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    paint: PaintTexture,
    /// The overlay frame's own gradients: its instances name rows of its own atlas.
    overlay_paint: PaintTexture,
    targets: Targets,
    grade_map: GradeMap,
    /// Lighting's passes and pictures, made the first time a frame is lit.
    lighting: Option<lighting::Lighting>,
    /// The light pass being drawn (see `Globals::light`): 0 outside the light passes.
    light_mode: u32,
    /// The light passes' globals: the world's and the screen's for mode 1, then for mode 2.
    light_globals: Vec<(wgpu::Buffer, wgpu::BindGroup)>,
}

/// The instance lists of one frame (the main one, its overlay or a blend layer) on the GPU.
struct FrameBuffers {
    trails: GrowableBuffer,
    shapes: GrowableBuffer,
    /// Shapes, the points their outlines run through and the bind group reading those, remade
    /// when the buffer grows.
    paths: GrowableBuffer,
    path_points: GrowableBuffer,
    balls: GrowableBuffer,
    looks: GrowableBuffer,
    layered: GrowableBuffer,
    /// The layer records the layered balls name, and the bind group reading them, remade when
    /// the buffer grows.
    look_layers: GrowableBuffer,
    layers_group: Option<wgpu::BindGroup>,
    /// The brush tiles the records name.
    masks: Option<masks::MaskAtlas>,
    /// The frame's time and hits, for shader layers inside balls.
    fx: Option<wgpu::Buffer>,
    flashes: GrowableBuffer,
    particles: GrowableBuffer,
    trails_front: GrowableBuffer,
    particles_front: GrowableBuffer,
}

impl Default for FrameBuffers {
    fn default() -> Self {
        Self {
            trails: GrowableBuffer::new("trail vertices"),
            shapes: GrowableBuffer::new("shape instances"),
            paths: GrowableBuffer::new("path instances"),
            path_points: GrowableBuffer::storage("path points"),
            balls: GrowableBuffer::new("ball instances"),
            looks: GrowableBuffer::new("styled ball instances"),
            layered: GrowableBuffer::new("layered ball instances"),
            look_layers: GrowableBuffer::storage("ball layer records"),
            layers_group: None,
            masks: None,
            fx: None,
            flashes: GrowableBuffer::new("flash instances"),
            particles: GrowableBuffer::new("particles behind"),
            trails_front: GrowableBuffer::new("trail vertices in front"),
            particles_front: GrowableBuffer::new("particles in front"),
        }
    }
}

impl FrameBuffers {
    #[allow(clippy::too_many_arguments)]
    fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layers_layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        fx: &effects::chain::Fx,
        f: &Frame,
    ) {
        self.trails.upload(device, queue, bytemuck::cast_slice(&f.trails));
        self.shapes.upload(device, queue, bytemuck::cast_slice(&f.shapes));
        self.paths.upload(device, queue, bytemuck::cast_slice(&f.paths));
        self.balls.upload(device, queue, bytemuck::cast_slice(&f.balls));
        self.looks.upload(device, queue, bytemuck::cast_slice(&f.looks));
        self.layered.upload(device, queue, bytemuck::cast_slice(&f.layered));
        // Shapes bind the records too, whether they name any or not.
        if !f.layered.is_empty() || !f.paths.is_empty() {
            // A ball naming no records still binds something.
            let records: &[LayerGpu] = if f.look_layers.is_empty() { &[LayerGpu::ZERO] } else { &f.look_layers };
            let grown = self.look_layers.upload(device, queue, bytemuck::cast_slice(records));
            // Circles and balls name no points, but the buffer is still bound.
            let points: &[[f32; 4]] = if f.path_points.is_empty() { &[[0.0; 4]] } else { &f.path_points };
            let points_grown = self.path_points.upload(device, queue, bytemuck::cast_slice(points));
            let atlas = self.masks.get_or_insert_with(|| masks::MaskAtlas::new(device, 1));
            let remade = atlas.upload(device, queue, &f.mask_tiles);
            let fresh = self.fx.is_none();
            let fx_buffer = self.fx.get_or_insert_with(|| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("in-ball effect values"),
                    size: std::mem::size_of::<effects::chain::Fx>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            });
            queue.write_buffer(fx_buffer, 0, bytemuck::bytes_of(fx));
            if grown || points_grown || remade || fresh || self.layers_group.is_none() {
                let buffer = self.look_layers.buffer.as_ref().expect("just uploaded");
                let points = self.path_points.buffer.as_ref().expect("just uploaded");
                self.layers_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("ball layer records"),
                    layout: layers_layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&atlas.view) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
                        wgpu::BindGroupEntry { binding: 3, resource: fx_buffer.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 4, resource: points.as_entire_binding() },
                    ],
                }));
            }
        }
        self.flashes.upload(device, queue, bytemuck::cast_slice(&f.flashes));
        self.particles.upload(device, queue, bytemuck::cast_slice(&f.particles));
        self.trails_front.upload(device, queue, bytemuck::cast_slice(&f.trails_front));
        self.particles_front.upload(device, queue, bytemuck::cast_slice(&f.particles_front));
    }
}

/// The scene shader: everything the scene's pipelines draw with, its layered balls walking their
/// stacks as [`layers_source`] says for `effects`.
pub(crate) fn scene_source(effects: Option<&str>) -> String {
    [
        gradients::paint_wgsl(1).as_str(),
        include_str!("shaders/scene.wgsl"),
        include_str!("shaders/object.wgsl"),
        include_str!("shaders/ball_look.wgsl"),
        include_str!("shaders/styled.wgsl"),
        include_str!("shaders/trail.wgsl"),
        include_str!("shaders/each.wgsl"),
        include_str!("shaders/paths.wgsl"),
        include_str!("shaders/blend_maths.wgsl"),
        layers_source(effects).as_str(),
    ]
    .concat()
}

/// layers.wgsl with its stack walk, and shape_layers.wgsl with the shapes', both with the
/// sub-layers' maths (sub_layers.wgsl): `None` for stacks without shader layers (they are left
/// out); `Some(effects)` adds the walks shader layers sample (`walk_plain`, `walk_shape_plain`,
/// leaving them out) and the walks that run them, with `effects` (the effect code and
/// `run_effect`) after.
pub(crate) fn layers_source(effects: Option<&str>) -> String {
    let walk = include_str!("shaders/layers_walk.wgsl");
    let named = |name: &str, cov: &str, case: &str, shaded: &str| {
        walk.replace("WALK", name)
            .replace("COV_IN", &format!("{cov}_in"))
            .replace("COV_OUT", &format!("{cov}_out"))
            .replace("//SHADER_CASE", case)
            .replace("//GROUP_SHADED", shaded)
    };
    let shape_walk = include_str!("shaders/shape_walk.wgsl");
    let shape_named = |name: &str, cov: &str, case: &str| {
        shape_walk
            .replace("SHAPE_WALK", name)
            .replace("SHAPE_COV_IN", &format!("{cov}_in"))
            .replace("SHAPE_COV_OUT", &format!("{cov}_out"))
            .replace("//SHAPE_SHADER_CASE", case)
    };
    let mut out = String::from(include_str!("shaders/layers.wgsl"));
    out.push_str(include_str!("shaders/sub_layers.wgsl"));
    out.push_str(include_str!("shaders/shape_layers.wgsl"));
    match effects {
        None => {
            out.push_str(&named("walk_layers", "cov", "", ""));
            out.push_str(&shape_named("walk_shape", "shape_cov", ""));
        }
        Some(code) => {
            out.push_str(&named("walk_plain", "cov_plain", "", ""));
            out.push_str(&named(
                "walk_layers",
                "cov",
                include_str!("shaders/layers_shader_case.wgsl"),
                include_str!("shaders/layers_group_shaded.wgsl"),
            ));
            out.push_str(&shape_named("walk_shape_plain", "shape_cov_plain", ""));
            out.push_str(&shape_named("walk_shape", "shape_cov", include_str!("shaders/shape_shader_case.wgsl")));
            out.push_str(code);
        }
    }
    out
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
            source: wgpu::ShaderSource::Wgsl(scene_source(None).into()),
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
        let layers_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ball layers layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<LayerGpu>() as u64),
                    },
                    count: None,
                },
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
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<effects::chain::Fx>() as u64),
                    },
                    count: None,
                },
                // Shapes' outline points (see paths.wgsl).
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let layered_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("layered ball pipeline layout"),
            bind_group_layouts: &[Some(&globals_layout), Some(&paint_layout), Some(&layers_layout)],
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
        let path = make(
            "paths",
            &layered_pipeline_layout,
            &scene_shader,
            "vs_path",
            "fs_path",
            &[Some(instances(std::mem::size_of::<PathInstance>() as u64, &PATH_ATTRIBUTES))],
            TriangleStrip,
            HDR_FORMAT,
            Some(premultiplied),
        );
        let path_object = make(
            "path objects",
            &layered_pipeline_layout,
            &scene_shader,
            "vs_path",
            "fs_path_object",
            &[Some(instances(std::mem::size_of::<PathInstance>() as u64, &PATH_ATTRIBUTES))],
            TriangleStrip,
            OBJECT_FORMAT,
            None,
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
        let layered = make(
            "layered balls",
            &layered_pipeline_layout,
            &scene_shader,
            "vs_layers",
            "fs_layers",
            &[Some(instances(std::mem::size_of::<LayeredInstance>() as u64, &LAYERED_ATTRIBUTES))],
            TriangleStrip,
            HDR_FORMAT,
            Some(premultiplied),
        );
        let layered_object = make(
            "layered ball objects",
            &layered_pipeline_layout,
            &scene_shader,
            "vs_layers",
            "fs_layers_object",
            &[Some(instances(std::mem::size_of::<LayeredInstance>() as u64, &LAYERED_ATTRIBUTES))],
            TriangleStrip,
            OBJECT_FORMAT,
            None,
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
                    Kind::Path,
                    Recipe {
                        layout: layered_pipeline_layout.clone(),
                        ..recipe(
                            "vs_path",
                            "fs_path_each",
                            size_of::<PathInstance>(),
                            Instance,
                            &PATH_ATTRIBUTES,
                            TriangleStrip,
                        )
                    },
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
                    Kind::Layered,
                    Recipe {
                        layout: layered_pipeline_layout.clone(),
                        ..recipe(
                            "vs_layers",
                            "fs_layers_each",
                            size_of::<LayeredInstance>(),
                            Instance,
                            &LAYERED_ATTRIBUTES,
                            TriangleStrip,
                        )
                    },
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
        let sprites = sprites::Sprites::new(device, &globals_layout);
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
            layers_layout,
            layered_pipeline_layout: layered_pipeline_layout.clone(),
            programs: ProgramStore::default(),
            waiting: false,
            sampler,
            background,
            trail,
            shape,
            ball,
            look,
            layered,
            flash,
            shape_object,
            path,
            path_object,
            ball_object,
            look_object,
            layered_object,
            chain: effects::chain::Chain::new(device),
            sprites,
            downsample,
            upsample,
            composite,
            paint,
            overlay_paint,
            targets,
            grade_map,
            lighting: None,
            light_mode: 0,
            light_globals: Vec::new(),
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

    /// Why the in-ball programs the last frame named could not be built, by program key (see
    /// [`LookProgram::key`]).
    pub fn program_errors(&self) -> Vec<(u64, String)> {
        self.programs.lock().errors().map(|(k, e)| (k, e.to_owned())).collect()
    }

    /// Builds in-ball programs in `store` from now on, shared with every renderer using it (on
    /// the same device), and on a background thread if the store says so.
    pub fn share_programs(&mut self, store: &ProgramStore) {
        self.programs = store.clone();
    }

    /// Whether the last frame drew some balls without their shader layers because their program
    /// was still building: draw it again soon and they show.
    pub fn programs_waiting(&self) -> bool {
        self.waiting
    }

    /// The renderer's program store.
    pub fn programs(&self) -> &ProgramStore {
        &self.programs
    }

    /// Why shader effects in the last frame could not be built, one line each: "{name}: {why}".
    pub fn shader_problems(&self) -> Vec<String> {
        self.chain.problems().map(str::to_owned).collect()
    }

    /// Draws one frame into the output texture. Each blend layer is drawn into its own picture
    /// and mixed in where its z-order and place put it among the frame's own things (the
    /// overlay's layers among the overlay's things, the same way).
    pub fn render(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame) {
        queue.write_buffer(
            &self.globals,
            0,
            bytemuck::bytes_of(&Globals::new(frame, &frame.camera, self.targets.width)),
        );
        let overlay = frame.overlay.as_deref().filter(|_| frame.has_overlay());
        // The frame's time and hits, for effects inside balls.
        let fx = effects::chain::base_uniform(frame, frame.width, frame.height);
        if let Some(o) = overlay {
            let identity = CameraView::default();
            queue.write_buffer(
                &self.screen_globals,
                0,
                bytemuck::bytes_of(&Globals::new(frame, &identity, self.targets.width)),
            );
            self.overlay.upload(device, queue, &self.layers_layout, &self.sampler, &fx, o);
        }
        self.buffers.upload(device, queue, &self.layers_layout, &self.sampler, &fx, frame);
        // Every blend layer's lists, the frame's then the overlay's, each in its own buffers.
        let overlay_layers = overlay.map_or(&[][..], |o| &o.layers[..]);
        let layers: Vec<&BlendLayer> = frame.layers.iter().chain(overlay_layers).collect();
        let mut layer_buffers = std::mem::take(&mut self.layer_buffers);
        layer_buffers.resize_with(layer_buffers.len().max(layers.len()), FrameBuffers::default);
        for (b, l) in layer_buffers.iter_mut().zip(&layers) {
            b.upload(device, queue, &self.layers_layout, &self.sampler, &fx, &l.frame);
        }
        // The programs of stacks with shader layers, built the first time a frame names them.
        let frames = std::iter::once((frame, None))
            .chain(overlay.map(|o| (o, None)))
            .chain(layers.iter().map(|l| (&l.frame, l.each.then_some(l.blend))));
        self.waiting = self.programs.lock().ensure(device, &self.layered_pipeline_layout, frames);
        self.paint.sync(device, queue, &self.paint_layout, &self.sampler, &frame.gradients);
        self.grade_map.sync(queue, &frame.grade);
        if let Some(o) = overlay {
            self.overlay_paint.sync(device, queue, &self.paint_layout, &self.sampler, &o.gradients);
        }

        if !layers.is_empty() {
            self.targets.ensure_layer(device);
        }
        let wants_objects = effects::chain::Chain::wants_objects(frame);
        let material = effects::chain::Chain::material(frame);
        if wants_objects {
            self.targets.ensure_object(device);
        }
        self.chain.begin_frame();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        // Counts the blend composites this frame.
        let mut slot = 0;
        let globals = self.globals_group.clone();
        let screen_globals = self.screen_globals_group.clone();
        self.draw_scene(
            device,
            queue,
            &mut encoder,
            frame,
            overlay,
            &layer_buffers,
            &mut slot,
            (wants_objects, material),
            (&globals, &screen_globals),
        );
        // A lit scene: the things again as their lit colour, then as their surface, each into
        // its own picture, then the light mixed into the HDR picture.
        if let Some(scene) = frame.light.as_ref() {
            let lighting = self.lighting.get_or_insert_with(|| lighting::Lighting::new(device));
            lighting.ensure(device, self.targets.width, self.targets.height, scene.settings.more[2]);
            let pictures = lighting.pictures().expect("just made");
            for (mode, (texture, view)) in [1u32, 2].into_iter().zip(pictures) {
                let (world, screen) = self.light_globals(device, queue, frame, mode, scene.settings.ambient[3]);
                let hdr_texture = std::mem::replace(&mut self.targets.hdr_texture, texture);
                let hdr = std::mem::replace(&mut self.targets.hdr, view);
                self.light_mode = mode;
                self.draw_scene(
                    device,
                    queue,
                    &mut encoder,
                    frame,
                    overlay,
                    &layer_buffers,
                    &mut slot,
                    (false, false),
                    (&world, &screen),
                );
                self.light_mode = 0;
                self.targets.hdr_texture = hdr_texture;
                self.targets.hdr = hdr;
            }
            self.copy_under(&mut encoder);
            let lighting = self.lighting.as_mut().expect("made above");
            lighting.run(
                device,
                queue,
                &mut encoder,
                frame,
                scene,
                self.targets.width,
                &self.targets.behind_view,
                &self.targets.hdr,
            );
        }
        let above = BlendLayer::above;
        let object_view = self.targets.object.as_ref().filter(|_| wants_objects).map(|(_, v)| v.clone());
        // A material drew the layers' own object pictures over the full one: effects on the
        // whole picture see every thing again.
        if material && frame.layers.iter().any(above) && effects::chain::Chain::picture_wants_objects(frame) {
            let view = &self.targets.object.as_ref().expect("made above").1;
            let mut pass = begin_pass(&mut encoder, view, "objects", wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
            pass.set_bind_group(0, &self.globals_group, &[]);
            pass.set_bind_group(1, &self.paint.group, &[]);
            self.draw_all_objects(&mut pass, frame, &layer_buffers, true);
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

    /// Draws the frame's things into the HDR picture: the background, the layers under
    /// everything, trails and shapes, the layers behind the balls, the balls, the effects on
    /// things, flashes and what is in front, the layers above, then the things stuck to the
    /// screen. `slot` counts the blend composites. `(wants_objects, material)` are the effects'
    /// needs (both false in the light passes, which run no effects); `globals` the world's and
    /// the screen's globals.
    #[allow(clippy::too_many_arguments)]
    fn draw_scene(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &Frame,
        overlay: Option<&Frame>,
        layer_buffers: &[FrameBuffers],
        slot: &mut usize,
        (wants_objects, material): (bool, bool),
        (globals, screen_globals): (&wgpu::BindGroup, &wgpu::BindGroup),
    ) {
        let world = Through {
            globals: globals.clone(),
            paint: self.paint.group.clone(),
            world: true,
            camera: frame.camera,
            each: None,
        };
        {
            let mut pass =
                begin_pass(encoder, &self.targets.hdr, "background", wgpu::LoadOp::Clear(wgpu::Color::BLACK));
            pass.set_bind_group(0, globals, &[]);
            pass.set_bind_group(1, &self.paint.group, &[]);
            pass.set_pipeline(&self.background);
            pass.draw(0..3, 0..1);
        }
        // Things of a z-order below 0 draw under everything else.
        for (i, (l, b)) in frame.layers.iter().zip(layer_buffers).enumerate().filter(|(_, (l, _))| l.z < 0) {
            self.draw_any_layer(device, queue, encoder, frame, i, l, b, &world, slot, None, false);
        }
        {
            let mut pass = begin_pass(encoder, &self.targets.hdr, "scene", wgpu::LoadOp::Load);
            pass.set_bind_group(0, globals, &[]);
            pass.set_bind_group(1, &self.paint.group, &[]);
            self.draw_back(&mut pass, frame, &self.buffers, &world);
        }
        // Blended things behind the balls (shapes) mix with what is behind them before the balls
        // are drawn over them.
        let behind = |l: &BlendLayer| l.z == 0 && l.behind();
        for (i, (l, b)) in frame.layers.iter().zip(layer_buffers).enumerate().filter(|(_, (l, _))| behind(l)) {
            self.draw_any_layer(device, queue, encoder, frame, i, l, b, &world, slot, None, false);
        }
        {
            let mut pass = begin_pass(encoder, &self.targets.hdr, "balls", wgpu::LoadOp::Load);
            pass.set_bind_group(0, globals, &[]);
            pass.set_bind_group(1, &self.paint.group, &[]);
            self.draw_balls(&mut pass, frame, &self.buffers, &world);
        }
        // Blended things behind the balls go under the rest; other blended things are drawn
        // above the rest, so they are on top here too. With a material on the scene, those
        // above get it on their own layers (see `draw_layer`), so the main picture's leaves them
        // out and the picture under a blended ball is left alone.
        let above = BlendLayer::above;
        if wants_objects {
            let view = &self.targets.object.as_ref().expect("made above").1;
            let mut pass = begin_pass(encoder, view, "objects", wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
            pass.set_bind_group(0, globals, &[]);
            pass.set_bind_group(1, &self.paint.group, &[]);
            self.draw_all_objects(&mut pass, frame, layer_buffers, !material);
        }
        let object_view = self.targets.object.as_ref().filter(|_| wants_objects).map(|(_, v)| v.clone());
        // The light passes run no effects: what effects paint counts as unlit.
        if self.light_mode == 0 {
            self.chain.run(
            device,
            queue,
            encoder,
            Stage::Things,
            frame,
            effects::chain::Canvas::Main,
            &self.targets.hdr_texture,
            &self.targets.hdr,
            object_view.as_ref(),
        );
        }
        if !frame.flashes.is_empty() || !frame.trails_front.is_empty() || !frame.particles_front.is_empty() {
            let mut pass = begin_pass(encoder, &self.targets.hdr, "flashes", wgpu::LoadOp::Load);
            pass.set_bind_group(0, globals, &[]);
            self.draw_front(&mut pass, frame, &self.buffers, &world);
        }
        // Then the blended things of z-order 0 and everything of a higher z-order, in order.
        for (i, (l, b)) in frame.layers.iter().zip(layer_buffers).enumerate().filter(|(_, (l, _))| above(l)) {
            let objects = object_view.as_ref();
            self.draw_any_layer(device, queue, encoder, frame, i, l, b, &world, slot, objects, material);
        }
        if let Some(o) = overlay {
            let screen = Through {
                globals: screen_globals.clone(),
                paint: self.overlay_paint.group.clone(),
                world: false,
                camera: CameraView::default(),
                each: None,
            };
            let buffers = std::mem::take(&mut self.overlay);
            let hdr = self.targets.hdr.clone();
            // In the same order as the frame's own: lower z-orders, the overlay's trails and
            // shapes, the layers behind its balls, its balls and flashes, then the rest.
            let own = &layer_buffers[frame.layers.len()..];
            let mut layers = |r: &mut Self, encoder: &mut wgpu::CommandEncoder, which: fn(&BlendLayer) -> bool| {
                for (l, b) in o.layers.iter().zip(own).filter(|(l, _)| which(l)) {
                    let through = r.layer_through(device, queue, frame, l, None, &screen);
                    r.draw_layer(device, queue, encoder, l, b, &through, *slot, None);
                    *slot += 1;
                }
            };
            layers(self, encoder, |l| l.z < 0);
            self.draw_things_part(encoder, o, &buffers, &screen, &hdr, false);
            layers(self, encoder, |l| l.z == 0 && l.behind());
            self.draw_things_part(encoder, o, &buffers, &screen, &hdr, true);
            layers(self, encoder, |l| l.z > 0 || (l.z == 0 && !l.behind()));
            self.overlay = buffers;
        }
    }

    /// The world's and the screen's globals for light pass `mode` (1 or 2), made the first time.
    fn light_globals(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame,
        mode: u32,
        background_lit: f32,
    ) -> (wgpu::BindGroup, wgpu::BindGroup) {
        while self.light_globals.len() < 4 {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("light pass globals"),
                size: std::mem::size_of::<Globals>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("light pass globals"),
                layout: &self.globals_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
            });
            self.light_globals.push((buffer, group));
        }
        let at = 2 * (mode as usize - 1);
        let mut world = Globals::new(frame, &frame.camera, self.targets.width);
        world.light = [mode as f32, background_lit, 0.0, 0.0];
        let mut screen = Globals::new(frame, &CameraView::default(), self.targets.width);
        screen.light = world.light;
        queue.write_buffer(&self.light_globals[at].0, 0, bytemuck::bytes_of(&world));
        queue.write_buffer(&self.light_globals[at + 1].0, 0, bytemuck::bytes_of(&screen));
        (self.light_globals[at].1.clone(), self.light_globals[at + 1].1.clone())
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
        if !f.particles.is_empty() {
            self.sprites.draw(pass, &b.particles, f.particles.len() as u32, through.each);
            // The sprites bind only the globals: put the paint back for what follows.
            pass.set_bind_group(1, &through.paint, &[]);
        }
        if !f.shapes.is_empty() {
            pass.set_pipeline(self.pipeline(&self.shape, Kind::Shape, through));
            pass.set_vertex_buffer(0, b.shapes.slice());
            pass.draw(0..4, 0..f.shapes.len() as u32);
        }
        if let Some(records) = b.layers_group.as_ref().filter(|_| !f.paths.is_empty()) {
            let plain = self.pipeline(&self.path, Kind::Path, through);
            pass.set_bind_group(2, records, &[]);
            pass.set_vertex_buffer(0, b.paths.slice());
            let each = through.each.map(|b| b.shader_code());
            // Runs of shapes drawn with one pipeline: their Shader layers' program, or, without one
            // (or one not built yet), the plain pipeline, which leaves shader records out.
            let mut start = 0;
            while start < f.paths.len() {
                let program = f.paths[start].layers[2];
                let end =
                    f.paths[start..].iter().position(|p| p.layers[2] != program).map_or(f.paths.len(), |n| start + n);
                let made =
                    f.programs.get(program as usize).and_then(|p| self.programs.lock().shape_pipeline(p.key, each));
                pass.set_pipeline(made.as_ref().unwrap_or(plain));
                pass.draw(0..4, start as u32..end as u32);
                start = end;
            }
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
        if let Some(group) = b.layers_group.as_ref().filter(|_| !f.layered.is_empty()) {
            let plain = self.pipeline(&self.layered, each::Kind::Layered, through);
            pass.set_bind_group(2, group, &[]);
            pass.set_vertex_buffer(0, b.layered.slice());
            let all = [LayeredRun { program: None, start: 0, end: f.layered.len() as u32 }];
            let runs = if f.layered_runs.is_empty() { &all[..] } else { &f.layered_runs[..] };
            let each = through.each.map(|b| b.shader_code());
            for run in runs {
                // A program that didn't build draws with the plain pipeline, leaving shader layers out.
                let program = run.program.and_then(|i| f.programs.get(i as usize));
                let built = program.and_then(|p| self.programs.lock().pipeline(p.key, each));
                let pipeline = built.as_ref().unwrap_or(plain);
                let end = run.end.min(f.layered.len() as u32);
                if run.start < end {
                    pass.set_pipeline(pipeline);
                    pass.draw(0..4, run.start..end);
                }
            }
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
        if !f.trails_front.is_empty() {
            pass.set_pipeline(self.pipeline(&self.trail, each::Kind::Trail, through));
            pass.set_bind_group(1, &through.paint, &[]);
            pass.set_vertex_buffer(0, b.trails_front.slice());
            pass.draw(0..f.trails_front.len() as u32, 0..1);
        }
        if !f.particles_front.is_empty() {
            self.sprites.draw(pass, &b.particles_front, f.particles_front.len() as u32, through.each);
        }
    }

    /// Where `f`'s shapes, balls and styled balls are, for effects on things.
    /// The frame's object picture: the layers drawn before its own things, its own things, then,
    /// when `above`, the layers drawn after them.
    fn draw_all_objects(&self, pass: &mut wgpu::RenderPass<'_>, frame: &Frame, layers: &[FrameBuffers], above: bool) {
        for (l, b) in frame.layers.iter().zip(layers).filter(|(l, _)| !l.above()) {
            self.draw_objects(pass, &l.frame, b);
        }
        self.draw_objects(pass, frame, &self.buffers);
        for (l, b) in frame.layers.iter().zip(layers).filter(|(l, _)| above && l.above()) {
            self.draw_objects(pass, &l.frame, b);
        }
    }

    fn draw_objects(&self, pass: &mut wgpu::RenderPass<'_>, f: &Frame, b: &FrameBuffers) {
        if !f.shapes.is_empty() {
            pass.set_pipeline(&self.shape_object);
            pass.set_vertex_buffer(0, b.shapes.slice());
            pass.draw(0..4, 0..f.shapes.len() as u32);
        }
        if let Some(records) = b.layers_group.as_ref().filter(|_| !f.paths.is_empty()) {
            pass.set_pipeline(&self.path_object);
            pass.set_bind_group(2, records, &[]);
            pass.set_vertex_buffer(0, b.paths.slice());
            pass.draw(0..4, 0..f.paths.len() as u32);
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
        if let Some(group) = b.layers_group.as_ref().filter(|_| !f.layered.is_empty()) {
            pass.set_pipeline(&self.layered_object);
            pass.set_bind_group(2, group, &[]);
            pass.set_vertex_buffer(0, b.layered.slice());
            pass.draw(0..4, 0..f.layered.len() as u32);
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
        self.draw_things_part(encoder, f, b, through, view, false);
        self.draw_things_part(encoder, f, b, through, view, true);
    }

    /// One half of [`Renderer::draw_things`]: the trails and shapes behind (`front` false), or
    /// the balls, styled balls, flashes and particles in front.
    fn draw_things_part(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        f: &Frame,
        b: &FrameBuffers,
        through: &Through,
        view: &wgpu::TextureView,
        front: bool,
    ) {
        let mut pass = begin_pass(encoder, view, if front { "things in front" } else { "things" }, wgpu::LoadOp::Load);
        pass.set_bind_group(0, &through.globals, &[]);
        pass.set_bind_group(1, &through.paint, &[]);
        if front {
            self.draw_balls(&mut pass, f, b, through);
            self.draw_front(&mut pass, f, b, through);
        } else {
            self.draw_back(&mut pass, f, b, through);
        }
    }

    /// Draws `frame`'s layer `l` (its `index`th): a plain one straight into the HDR picture, a
    /// blended one through [`Renderer::draw_layer`], counting it in `slot`. `objects` is the object
    /// picture for its effects. With a `material` on the scene (only passed for layers drawn
    /// after the main picture's effects), a plain layer goes through the layer picture too, so
    /// the material paints its things.
    #[allow(clippy::too_many_arguments)]
    fn draw_any_layer(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &Frame,
        index: usize,
        l: &BlendLayer,
        b: &FrameBuffers,
        world: &Through,
        slot: &mut usize,
        objects: Option<&wgpu::TextureView>,
        material: bool,
    ) {
        if l.plain() && !material {
            let hdr = self.targets.hdr.clone();
            self.draw_things(encoder, &l.frame, b, world, &hdr);
            return;
        }
        let through = self.layer_through(device, queue, frame, l, Some(index), world);
        self.draw_layer(device, queue, encoder, l, b, &through, *slot, Some((frame, index, objects)));
        *slot += 1;
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
        // The light passes draw every layer's things one over another.
        if self.light_mode != 0 {
            return through.clone();
        }
        let starts_above_nothing = matches!(l.blend, bpe_core::Blend::Multiply | bpe_core::Blend::Darken);
        let has_effects = index.is_some_and(|i| effects::chain::layer_has_effects(frame, i));
        if !(l.each && l.blend.combines_with_itself()) || (starts_above_nothing && has_effects) {
            return through.clone();
        }
        let code = l.blend.shader_code();
        let mut globals = Globals::new(frame, &through.camera, self.targets.width);
        globals.post[3] = code as f32;
        let key = (code, !through.world);
        let group = self.each_globals.group(device, queue, &self.globals_layout, key, bytemuck::bytes_of(&globals));
        self.each.ensure(device, l.blend, &l.frame);
        if !l.frame.particles.is_empty() || !l.frame.particles_front.is_empty() {
            self.sprites.ensure_each(device, l.blend);
        }
        Through { globals: group, each: Some(l.blend), ..through.clone() }
    }

    /// Draws one blend layer's things into the layer picture, runs the effects on those things
    /// that belong to it (`effects`: the main frame, the layer's index in it and the object
    /// picture; for a material on the scene, the object picture is redrawn with the layer's
    /// own things), then mixes it into the HDR picture over a copy of what is there. `slot`
    /// counts the layers mixed so far this frame.
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
        if let Some((frame, index, objects)) = effects
            .filter(|(frame, index, _)| self.light_mode == 0 && effects::chain::layer_has_effects(frame, *index))
        {
            use effects::chain::Chain;
            // A material paints the layer's own things, so the object picture shows only them.
            let own = self.targets.object.clone().filter(|_| Chain::material(frame) && Chain::wants_objects(frame));
            if let Some((_, view)) = &own {
                let mut pass =
                    begin_pass(encoder, view, "layer objects", wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
                pass.set_bind_group(0, &self.globals_group, &[]);
                pass.set_bind_group(1, &self.paint.group, &[]);
                self.draw_objects(&mut pass, &l.frame, b);
            }
            let on = effects::chain::Canvas::Layer(index);
            let objects = own.as_ref().map(|(_, v)| v).or(objects);
            self.chain.run(device, queue, encoder, Stage::Things, frame, on, &texture, &layer, objects);
        }
        // The surface keeps the most any layer blocks and stands (see lighting.wgsl).
        let blend = if self.light_mode == 2 { bpe_core::Blend::Lighten } else { l.blend };
        self.blend.composite(
            device,
            queue,
            encoder,
            slot,
            blend,
            l.amount,
            through.each.is_some(),
            l.plain(),
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
    usage: wgpu::BufferUsages,
}

impl GrowableBuffer {
    fn new(label: &'static str) -> Self {
        Self { label, buffer: None, len: 0, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST }
    }

    /// One read by shaders as a storage buffer.
    fn storage(label: &'static str) -> Self {
        Self { usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, ..Self::new(label) }
    }

    /// Writes `bytes`, first making a bigger buffer if they don't fit. True when it made one.
    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, bytes: &[u8]) -> bool {
        self.len = bytes.len() as u64;
        if bytes.is_empty() {
            return false;
        }
        let grown = self.buffer.as_ref().is_none_or(|b| b.size() < self.len);
        if grown {
            self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(self.label),
                size: self.len.next_power_of_two().max(1024),
                usage: self.usage,
                mapped_at_creation: false,
            }));
        }
        queue.write_buffer(self.buffer.as_ref().unwrap(), 0, bytes);
        grown
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

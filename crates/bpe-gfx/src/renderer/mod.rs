//! The [`Renderer`]: what it holds and what it tells the outside. Building it is `new.rs`,
//! drawing a frame `scene.rs` (the passes) and `draw.rs` (the draw calls), its pictures
//! `targets.rs`, and the grade and paint textures `grade.rs` and `paint.rs`.

use super::*;

mod draw;
mod grade;
mod new;
mod paint;
mod scene;
mod targets;

use grade::GradeGpu;
use paint::PaintTexture;
use targets::Targets;

/// What [`Renderer::render_scene`] tells [`Renderer::finish`].
pub(crate) struct Scened {
    pub(crate) wants_objects: bool,
    pub(crate) material: bool,
    pub(crate) moved: bool,
    pub(crate) timed: bool,
}

pub(crate) const BLOOM_LEVELS: usize = 6;

pub(crate) const TRAIL_ATTRIBUTES: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![
    0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32x2, 4 => Float32x2, 5 => Float32x4, 6 => Float32x4
];

pub(crate) const LAYERED_ATTRIBUTES: [wgpu::VertexAttribute; 13] = wgpu::vertex_attr_array![
    0 => Float32x2, 1 => Float32, 2 => Float32, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4,
    6 => Float32x4, 7 => Float32x4, 8 => Uint32x4, 9 => Float32x4, 10 => Float32x4, 11 => Float32x4,
    12 => Float32x4
];

pub(crate) const PATH_ATTRIBUTES: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![
    0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Uint32x4, 5 => Float32x4, 6 => Float32x4
];

pub(crate) const FLASH_ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32, 2 => Float32x4];

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct Globals {
    pub(crate) view: [f32; 4],
    /// The background paint: atlas row (below 0 paints black), start, scroll, repeat.
    pub(crate) bg_paint: [f32; 4],
    /// The background's place: direction in degrees, span, centre xy.
    pub(crate) bg_place: [f32; 4],
    /// x: bloom strength, y: exposure, z: 1 to run the colour grade (`grade.wgsl`), w: in a layer whose things
    /// blend with each other, the blend's `shader_code` (see each.wgsl).
    pub(crate) post: [f32; 4],
    /// The camera: centre xy, zoom, turn in radians.
    pub(crate) camera: [f32; 4],
    /// x: 1 when the background stays on the screen, y: how its place runs (0 none, 1 direction,
    /// 2 centre), z: 1 for hard steps, w: 1 to mix `bg_from` and `bg_to`.
    pub(crate) bg: [f32; 4],
    /// The two colours of a plain blend the background mixes itself (`bg.w` is 1 then).
    pub(crate) bg_from: [f32; 4],
    pub(crate) bg_to: [f32; 4],
    /// Lighting: x the light pass (0 the picture as always, 1 lit colour, 2 surface, 3 the light
    /// things give off), y how much the background takes light, z 1 in Stage (see the lighting shader).
    pub(crate) light: [f32; 4],
    /// The light the background gives off (read in pass 3): its own colour times its strength,
    /// then what its colours are multiplied by (rgb, linear).
    pub(crate) light_emit: [[f32; 4]; 2],
    /// The background's form when it is neither Straight nor Round (`bg.y` 3): its own numbers
    /// (`PaintGpu::form`).
    pub(crate) bg_form: [f32; 4],
    /// x: the background's form code (`PaintGpu::pattern`), yzw spare.
    pub(crate) bg_shape: [f32; 4],
}

impl Globals {
    /// The globals for drawing `frame` through `camera`.
    pub(crate) fn new(frame: &Frame, camera: &CameraView, output_width: u32) -> Self {
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
            post: [frame.bloom, frame.exposure, if frame.grade.is_identity() { 0.0 } else { 1.0 }, 0.0],
            camera: [camera.center.x, camera.center.y, camera.zoom, camera.turn],
            bg: [
                if frame.background_on_screen { 1.0 } else { 0.0 },
                frame.background.mode,
                frame.background.grain,
                if frame.background.ends.is_some() { 1.0 } else { 0.0 },
            ],
            bg_from: frame.background.ends.map_or([0.0; 4], |e| e[0]),
            bg_to: frame.background.ends.map_or([0.0; 4], |e| e[1]),
            light: [0.0; 4],
            light_emit: [[0.0; 4]; 2],
            bg_form: frame.background.form,
            bg_shape: [frame.background.pattern, 0.0, 0.0, 0.0],
        }
    }
}

pub struct Renderer {
    pub(crate) globals: wgpu::Buffer,
    pub(crate) globals_group: wgpu::BindGroup,
    /// The same globals with no camera, for things stuck to the screen.
    pub(crate) screen_globals: wgpu::Buffer,
    pub(crate) screen_globals_group: wgpu::BindGroup,
    /// The frame's instance lists, its overlay's, and each blend layer's (the frame's layers,
    /// then the overlay's).
    pub(crate) buffers: FrameBuffers,
    pub(crate) overlay: FrameBuffers,
    pub(crate) layer_buffers: Vec<FrameBuffers>,
    pub(crate) blend: blend::Composite,
    /// For layers whose things blend with each other.
    pub(crate) each: each::EachPipelines,
    pub(crate) each_globals: each::EachGlobals,
    pub(crate) globals_layout: wgpu::BindGroupLayout,
    pub(crate) post_layout: wgpu::BindGroupLayout,
    pub(crate) composite_layout: wgpu::BindGroupLayout,
    /// The gradient atlas and the Ripples field: bound at group 1 of the scene passes.
    pub(crate) paint_layout: wgpu::BindGroupLayout,
    /// The layer records of layered balls: group 2 of their pipelines.
    pub(crate) layers_layout: wgpu::BindGroupLayout,
    pub(crate) layered_pipeline_layout: wgpu::PipelineLayout,
    /// The pipelines of stacks with shader layers, maybe shared with other renderers.
    pub(crate) programs: ProgramStore,
    /// The last frame drew some balls without their shader layers, their program still building.
    pub(crate) waiting: bool,
    pub(crate) sampler: wgpu::Sampler,
    pub(crate) background: wgpu::RenderPipeline,
    pub(crate) trail: wgpu::RenderPipeline,
    pub(crate) layered: wgpu::RenderPipeline,
    pub(crate) flash: wgpu::RenderPipeline,
    /// Shapes (see paths.wgsl), and where they are for effects on things.
    pub(crate) path: wgpu::RenderPipeline,
    pub(crate) path_object: wgpu::RenderPipeline,
    pub(crate) layered_object: wgpu::RenderPipeline,
    pub(crate) chain: effects::chain::Chain,
    pub(crate) sprites: sprites::Sprites,
    pub(crate) fields: fields::Fields,
    pub(crate) swarms: swarm::draw::SwarmDraw,
    pub(crate) downsample: wgpu::RenderPipeline,
    pub(crate) upsample: wgpu::RenderPipeline,
    pub(crate) composite: wgpu::RenderPipeline,
    pub(crate) paint: PaintTexture,
    /// What Ripples measure from, built for frames that have them (bound with the atlas).
    pub(crate) ripples: ripples::Ripples,
    /// Liquids' field and passes (particles part 6), made the first time a frame has one.
    pub(crate) liquid: Option<liquid::LiquidPass>,
    /// The overlay frame's own gradients: its instances name rows of its own atlas.
    pub(crate) overlay_paint: PaintTexture,
    pub(crate) targets: Targets,
    pub(crate) grade: GradeGpu,
    /// The Histogram of the finished picture, made the first time the editor asks for it.
    pub(crate) histogram: Option<crate::grade::Histogram>,
    /// Lighting's passes and pictures, made the first time a frame is lit.
    pub(crate) lighting: Option<lighting::Lighting>,
    /// The Stage camera's pass (lighting part 5), made the first time a frame is seen moved.
    pub(crate) stage_camera: Option<stage_camera_pass::StageCameraPass>,
    /// Times the light passes, when the device can (`None` inside: it can't; tried once).
    pub(crate) light_timer: Option<Option<timing::LightTimer>>,
    /// The last frame's lighting: how many lights it drew, `None` when it wasn't lit.
    pub(crate) lit_lights: Option<usize>,
    /// The lighting field is as fine, in the scene, as the video's (`preview_field_like_video`), not the Quality's
    /// share of this renderer's own size.
    pub(crate) field_like_video: bool,
    /// Whether the lighting lists each tile's lights (review S19, [`Renderer::set_light_tiles`]).
    pub(crate) light_tiles: bool,
    /// The bloom picture the composite reads is black (bloom 0 skipped its chain, review S24).
    pub(crate) bloom_black: bool,
    /// The light pass being drawn (see `Globals::light`): 0 outside the light passes.
    pub(crate) light_mode: u32,
    /// Whether the light passes draw a Stage (its surface picture keeps no blended cover).
    pub(crate) light_stage: bool,
    /// The bake's running average (lighting part 3), made for the first baked frame.
    pub(crate) accumulate: Option<accumulate::Accumulate>,
    /// The light passes' globals: the world's and the screen's for mode 1, then for mode 2.
    pub(crate) light_globals: Vec<(wgpu::Buffer, wgpu::BindGroup)>,
}

/// The instance lists of one frame (the main one, its overlay or a blend layer) on the GPU.
pub(crate) struct FrameBuffers {
    pub(crate) trails: GrowableBuffer,
    /// Shapes, the points their outlines run through and the bind group reading those, remade
    /// when the buffer grows.
    pub(crate) paths: GrowableBuffer,
    pub(crate) path_points: GrowableBuffer,
    pub(crate) layered: GrowableBuffer,
    /// The layer records the layered balls name, and the bind group reading them, remade when
    /// the buffer grows.
    pub(crate) look_layers: GrowableBuffer,
    pub(crate) layers_group: Option<wgpu::BindGroup>,
    /// The brush tiles the records name.
    pub(crate) masks: Option<masks::MaskAtlas>,
    /// The frame's time and hits, for shader layers inside balls.
    pub(crate) fx: Option<wgpu::Buffer>,
    pub(crate) flashes: GrowableBuffer,
    pub(crate) particles: GrowableBuffer,
    pub(crate) trails_front: GrowableBuffer,
    pub(crate) particles_front: GrowableBuffer,
    /// Its soft fields of particles.
    pub(crate) fields: fields::FrameFields,
    /// Its Huge swarms' runs.
    pub(crate) swarms: swarm::draw::FrameSwarms,
}

impl Default for FrameBuffers {
    fn default() -> Self {
        Self {
            trails: GrowableBuffer::new("trail vertices"),
            paths: GrowableBuffer::new("path instances"),
            path_points: GrowableBuffer::storage("path points"),
            layered: GrowableBuffer::new("layered ball instances"),
            look_layers: GrowableBuffer::storage("ball layer records"),
            layers_group: None,
            masks: None,
            fx: None,
            flashes: GrowableBuffer::new("flash instances"),
            particles: GrowableBuffer::new("particles behind"),
            trails_front: GrowableBuffer::new("trail vertices in front"),
            particles_front: GrowableBuffer::new("particles in front"),
            fields: fields::FrameFields::default(),
            swarms: swarm::draw::FrameSwarms::default(),
        }
    }
}

impl FrameBuffers {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layers_layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        fx: &effects::chain::Fx,
        f: &Frame,
    ) {
        self.trails.upload(device, queue, bytemuck::cast_slice(&f.trails));
        self.paths.upload(device, queue, bytemuck::cast_slice(&f.paths));
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

/// How one frame's things are drawn: through which globals and from which gradient atlas.
#[derive(Clone)]
pub(crate) struct Through {
    pub(crate) globals: wgpu::BindGroup,
    pub(crate) paint: wgpu::BindGroup,
    /// Drawing the world (not the things stuck to the screen).
    pub(crate) world: bool,
    /// The camera the world is seen through: the main frame's, which a blend layer's own copies
    /// may lag behind (the editor's World view sets them after building).
    pub(crate) camera: CameraView,
    /// Drawing into a layer whose things blend with each other by this blend.
    pub(crate) each: Option<bpe_core::Blend>,
}

pub(crate) fn begin_pass<'a>(
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
pub(crate) struct GrowableBuffer {
    pub(crate) label: &'static str,
    pub(crate) buffer: Option<wgpu::Buffer>,
    pub(crate) len: u64,
    pub(crate) usage: wgpu::BufferUsages,
}

impl GrowableBuffer {
    pub(crate) fn new(label: &'static str) -> Self {
        Self { label, buffer: None, len: 0, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST }
    }

    /// One read by shaders as a storage buffer.
    pub(crate) fn storage(label: &'static str) -> Self {
        Self { usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, ..Self::new(label) }
    }

    /// Writes `bytes`, first making a bigger buffer if they don't fit. True when it made one.
    pub(crate) fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, bytes: &[u8]) -> bool {
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

    /// Makes sure the buffer holds at least `size` bytes (its contents are the shader's to write).
    pub(crate) fn reserve(&mut self, device: &wgpu::Device, size: u64) {
        self.len = size;
        if self.buffer.as_ref().is_none_or(|b| b.size() < size) {
            self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(self.label),
                size: size.next_power_of_two().max(1024),
                usage: self.usage,
                mapped_at_creation: false,
            }));
        }
    }

    pub(crate) fn slice(&self) -> wgpu::BufferSlice<'_> {
        self.buffer.as_ref().expect("buffer uploaded before drawing").slice(..self.len)
    }
}

impl Renderer {
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

    /// Why the in-ball programs the last frame named could not be built, by program key (see
    /// [`LookProgram::key`]).
    pub fn program_errors(&self) -> Vec<(u64, String)> {
        self.programs.lock().errors().map(|(k, e)| (k, e.to_owned())).collect()
    }

    /// Forgets what the effects remember of earlier frames: the next frame starts them afresh.
    pub fn forget_effect_history(&mut self) {
        self.chain.forget_history();
        self.forget_memory();
    }

    /// Builds in-ball programs in `store` from now on, shared with every renderer using it (on
    /// the same device), and on a background thread if the store says so.
    pub fn share_programs(&mut self, store: &ProgramStore) {
        self.programs = store.clone();
        self.chain.share(store);
    }

    /// For the editor's preview, drawn smaller than the video: size the lighting field so a texel covers as much of
    /// the scene as in the video at the same Quality (`lighting::field_divisor`), so the preview's shadows look like
    /// the export's. Off (the Quality's share of this renderer's size) for exports and the picture tests.
    pub fn preview_field_like_video(&mut self, on: bool) {
        self.field_like_video = on;
    }

    /// Whether the last frame left something out because its shaders were still building (in
    /// a background store): balls without their shader layers, a lit frame drawn unlit, a
    /// picture effect left out. Draw it again soon and they show.
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

    /// Whether the last lit frame's remembered light (glow in the dark) started again from nothing
    /// partway through the video (a seek, or after [`Renderer::forget_memory`]), so it shows less
    /// than the video will: drawing the frames before it in order warms it up.
    pub fn memory_cold(&self) -> bool {
        self.lighting.as_ref().is_some_and(|l| l.memory_cold())
    }

    /// Forgets the remembered light (glow in the dark): the scene changed.
    pub fn forget_memory(&mut self) {
        if let Some(l) = self.lighting.as_mut() {
            l.forget_memory();
        }
    }

    /// The last frame's lighting, when it was lit: how many lights it drew and, on a device made
    /// with [`TIMING_FEATURES`], how long the GPU spent on the light passes lately (ms).
    /// Whether the last frame built the Ripples field (only frames with a gradient laid out as
    /// Ripples do).
    pub fn ripples_built(&self) -> bool {
        self.ripples.built()
    }

    /// How many photons the last lit frame sent through glass that bends light (glass and light
    /// part 3; 0 when it sent none or the frame wasn't lit).
    pub fn bent_photons(&self) -> u32 {
        self.lighting.as_ref().filter(|_| self.lit_lights.is_some()).map_or(0, |l| l.bent_photons())
    }

    /// Whether each pixel takes only the lights its tile lists (review S19; on unless a test turns
    /// it off to check the lists change nothing). The next lit frame follows it.
    pub fn set_light_tiles(&mut self, on: bool) {
        self.light_tiles = on;
    }

    /// How many bind groups the lighting has made (review S24: it keeps them between frames; for tests).
    pub fn lighting_binds_made(&self) -> usize {
        self.lighting.as_ref().map_or(0, |l| l.binds_made())
    }

    pub fn lighting_stats(&self) -> Option<(usize, Option<f32>)> {
        let ms = self.light_timer.as_ref().and_then(|t| t.as_ref()).and_then(|t| t.last_ms);
        self.lit_lights.map(|n| (n, ms))
    }

    /// Starts counting the finished picture into the Histogram (Grade mode's scope), unless a
    /// count is still on its way; [`Renderer::histogram`] gives it once the GPU has done it.
    /// Only the editor asks: an export never does.
    pub fn request_histogram(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let histogram = self.histogram.get_or_insert_with(|| crate::grade::Histogram::new(device));
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("histogram") });
        let size = (self.targets.width, self.targets.height);
        if histogram.encode(device, &mut encoder, &self.targets.output_view, size) {
            queue.submit([encoder.finish()]);
            histogram.after_submit();
        }
    }

    /// The Histogram's latest counts, once the GPU has them (looks without waiting).
    pub fn histogram(&mut self, device: &wgpu::Device) -> Option<Box<crate::grade::HistogramBins>> {
        let histogram = self.histogram.as_mut()?;
        if histogram.busy() {
            let _ = device.poll(wgpu::PollType::Poll);
        }
        histogram.take()
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

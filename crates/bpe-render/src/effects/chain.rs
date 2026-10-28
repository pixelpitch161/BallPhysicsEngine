//! Runs a scene's effects, in order, over the HDR picture.

use super::{EffectInfo, EffectLibrary, MAX_PARAMS};
use crate::frame::FrameData;
use bpe_scene::Scene;
use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

pub const MAX_HITS: usize = 16;
/// Size of one block preview in the thumbnail atlas: a 9:16 picture.
pub const THUMB_SIZE: (u32, u32) = (72, 128);
/// Block previews per row of the atlas.
pub const THUMB_COLUMNS: u32 = 8;
/// Most block previews drawn in one frame.
pub const MAX_THUMBS: usize = 64;
const THUMB_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// A jump in time bigger than this (a seek, or a new export) starts feedback effects afresh.
const MAX_STEP: f64 = 0.25;

/// Matches `struct Fx` in prelude.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct FxUniform {
    resolution: [f32; 2],
    time: f32,
    amount: f32,
    pulse: f32,
    hit_count: u32,
    frame: f32,
    dt: f32,
    scale: f32,
    level: f32,
    view: u32,
    pad: f32,
    audio: [f32; 4],
    params: [f32; MAX_PARAMS],
    hits: [[f32; 4]; MAX_HITS],
    hit_colors: [[f32; 4]; MAX_HITS],
}

/// Asks the chain to draw small previews of a node effect's blocks, see [`EffectChain::set_thumbnails`].
#[derive(Clone, Debug)]
pub struct ThumbnailRequest {
    /// Index in the scene's effect list of the node effect being edited.
    pub effect: usize,
    /// The effect built with [`EffectInfo::graph_thumbnails`].
    pub info: Arc<EffectInfo>,
    /// Which blocks to draw (values of `fx.view`, from 1), one tile each in this order.
    pub views: Vec<u32>,
}

/// Where tile `i` of the thumbnail atlas is, in pixels: x, y, width, height.
pub fn thumbnail_tile(i: usize) -> [u32; 4] {
    let (w, h) = THUMB_SIZE;
    let i = i as u32;
    [(i % THUMB_COLUMNS) * w, (i / THUMB_COLUMNS) * h, w, h]
}

/// Size of the whole thumbnail atlas in pixels.
pub fn thumbnail_atlas_size() -> (u32, u32) {
    (THUMB_SIZE.0 * THUMB_COLUMNS, THUMB_SIZE.1 * (MAX_THUMBS as u32).div_ceil(THUMB_COLUMNS))
}

#[derive(Default)]
struct Thumbnails {
    request: Option<ThumbnailRequest>,
    atlas: Option<(wgpu::Texture, wgpu::TextureView)>,
    pipeline: Option<(Arc<EffectInfo>, Option<wgpu::RenderPipeline>)>,
    building: Option<Building>,
    uniforms: Vec<wgpu::Buffer>,
    /// Whether the atlas holds this request's pictures.
    drawn: bool,
}

struct Pipeline {
    info: Arc<EffectInfo>,
    /// `None` when the GPU refused the shader even though naga accepted it.
    pipeline: Option<wgpu::RenderPipeline>,
}

/// A new version of a pipeline being built on another thread, see
/// [`EffectChain::set_background_builds`].
struct Building {
    info: Arc<EffectInfo>,
    done: Receiver<Result<wgpu::RenderPipeline, String>>,
}

impl Building {
    /// Starts building `info`'s pipeline with fragment entry point `entry` on another thread.
    fn start(
        device: &wgpu::Device,
        layout: &wgpu::PipelineLayout,
        info: &Arc<EffectInfo>,
        entry: &'static str,
        format: wgpu::TextureFormat,
    ) -> Self {
        let (sender, done) = std::sync::mpsc::channel();
        let (device, layout, job) = (device.clone(), layout.clone(), info.clone());
        std::thread::spawn(move || {
            if let Ok(code) = &job.compiled {
                _ = sender.send(build_pipeline(&device, &layout, &job.id, code, entry, format));
            }
        });
        Self { info: info.clone(), done }
    }

    /// The finished pipeline, `Some(Err)` if the GPU refused it or the build died, or `None`
    /// while it is still building.
    fn poll(&self) -> Option<Result<wgpu::RenderPipeline, String>> {
        match self.done.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("the shader build stopped unexpectedly".into())),
        }
    }
}

/// Whether a pipeline built for `a` also draws `b`: the same shader code. Moving a block or a
/// note around a node effect makes a new effect with the same code.
fn same_code(a: &Arc<EffectInfo>, b: &Arc<EffectInfo>) -> bool {
    Arc::ptr_eq(a, b) || (a.id == b.id && a.compiled == b.compiled)
}

/// Builds the render pipeline for one effect shader, or says why the GPU refused it.
fn build_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    label: &str,
    code: &str,
    entry: &str,
    format: wgpu::TextureFormat,
) -> Result<wgpu::RenderPipeline, String> {
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(code.into()),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
        }),
        multiview_mask: None,
        cache: None,
    });
    match pollster::block_on(scope.pop()) {
        None => Ok(pipeline),
        Some(error) => Err(error.to_string()),
    }
}

/// Per-position state: the uniform buffer and, for feedback effects, last frame's output.
struct Slot {
    id: String,
    uniform: wgpu::Buffer,
    history: Option<(wgpu::Texture, wgpu::TextureView)>,
    frames: u32,
}

pub(crate) struct EffectChain {
    library: EffectLibrary,
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    sampler: wgpu::Sampler,
    black: wgpu::TextureView,
    pipelines: HashMap<String, Pipeline>,
    /// New versions of pipelines being built on other threads, by effect id.
    building: HashMap<String, Building>,
    /// Whether changed effects are rebuilt on other threads, see [`EffectChain::set_background_builds`].
    background: bool,
    slots: Vec<Slot>,
    scratch: Option<(wgpu::Texture, wgpu::TextureView)>,
    last_time: Option<f64>,
    gpu_errors: Vec<(String, String)>,
    thumbnails: Thumbnails,
}

impl EffectChain {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
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
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effect layout"),
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
                texture_entry(1),
                texture_entry(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("effect pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("effect sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let black = make_texture(device, 1, 1, format, "effect black").1;
        Self {
            library: EffectLibrary::new(),
            layout,
            pipeline_layout,
            sampler,
            black,
            pipelines: HashMap::new(),
            building: HashMap::new(),
            background: false,
            slots: Vec::new(),
            scratch: None,
            last_time: None,
            gpu_errors: Vec::new(),
            thumbnails: Thumbnails::default(),
        }
    }

    /// Starts (or with `None` stops) drawing block previews of a node effect into the thumbnail
    /// atlas on every frame this chain renders. Costs nothing while off.
    pub fn set_thumbnails(&mut self, request: Option<ThumbnailRequest>) {
        let thumbs = &mut self.thumbnails;
        thumbs.drawn = false;
        thumbs.request = request.map(|mut r| {
            r.views.truncate(MAX_THUMBS);
            r
        });
    }

    /// The thumbnail atlas (see [`thumbnail_tile`]), made on first use. It holds sRGB-encoded
    /// colours like the main output, so it can be shown as it is.
    pub fn thumbnail_atlas(&mut self, device: &wgpu::Device) -> &wgpu::TextureView {
        let (width, height) = thumbnail_atlas_size();
        &self.thumbnails.atlas.get_or_insert_with(|| make_texture(device, width, height, THUMB_FORMAT, "thumbnails")).1
    }

    /// Whether the last frame drew the requested thumbnails.
    pub fn thumbnails_drawn(&self) -> bool {
        self.thumbnails.drawn
    }

    pub fn library(&self) -> &EffectLibrary {
        &self.library
    }

    pub fn set_library(&mut self, library: EffectLibrary) {
        self.library = library;
        self.pipelines.clear();
        self.building.clear();
        self.gpu_errors.clear();
    }

    /// With `true`, an effect that changes keeps drawing its last version while the new one is
    /// built on another thread, so editing a node effect doesn't stall the editor while the GPU
    /// driver compiles it. A new effect is still built straight away. Exports leave this off so
    /// every frame uses the effects exactly as they are.
    pub fn set_background_builds(&mut self, on: bool) {
        self.background = on;
    }

    /// Whether a changed effect or block preview is still being built on another thread, so the
    /// picture will change once it is ready.
    pub fn building(&self) -> bool {
        !self.building.is_empty() || self.thumbnails.building.is_some()
    }

    /// Puts pipelines finished on other threads to use.
    fn finish_builds(&mut self) {
        let finished: Vec<_> = self.building.iter().filter_map(|(id, b)| Some((id.clone(), b.poll()?))).collect();
        for (id, result) in finished {
            let building = self.building.remove(&id).unwrap();
            self.install(building.info, result);
        }
        let thumbs = &mut self.thumbnails;
        if let Some(result) = thumbs.building.as_ref().and_then(|b| b.poll()) {
            let info = thumbs.building.take().unwrap().info;
            thumbs.pipeline = Some((info, result.ok()));
        }
    }

    fn install(&mut self, info: Arc<EffectInfo>, result: Result<wgpu::RenderPipeline, String>) {
        self.gpu_errors.retain(|(id, _)| *id != info.id);
        if let Err(error) = &result {
            self.gpu_errors.push((info.id.clone(), error.clone()));
        }
        self.pipelines.insert(info.id.clone(), Pipeline { info, pipeline: result.ok() });
    }

    /// Effects that passed the shader check but that the GPU still refused, as (id, message).
    pub fn gpu_errors(&self) -> &[(String, String)] {
        &self.gpu_errors
    }

    /// Forgets feedback history, so the next frame starts the echo effects from black.
    pub fn reset(&mut self) {
        self.last_time = None;
    }

    /// Applies the scene's enabled effects to `hdr` in place, recording the passes into `encoder`.
    #[allow(clippy::too_many_arguments)]
    pub fn apply(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        hdr: &wgpu::Texture,
        hdr_view: &wgpu::TextureView,
        scene: &Scene,
        time: f64,
        frame: &FrameData,
    ) {
        let active: Vec<_> = scene
            .effects
            .iter()
            .enumerate()
            .filter(|(_, e)| e.enabled && e.amount > 0.0)
            .filter_map(|(index, effect)| {
                let info = self.library.get_in(&effect.shader, &scene.shader_graphs)?;
                info.compiled.is_ok().then_some((index, effect, info))
            })
            .collect();
        self.finish_builds();
        for (_, _, info) in &active {
            self.prepare(device, info);
        }
        // Each effect draws with the version its pipeline was built from, which can be the one
        // before while a changed effect builds on another thread.
        let active: Vec<_> = active
            .into_iter()
            .filter_map(|(index, effect, info)| {
                let built = self.pipelines.get(&info.id)?;
                built.pipeline.is_some().then(|| (index, effect, built.info.clone()))
            })
            .collect();
        self.thumbnails.drawn = false;
        let thumbs_here = self.thumbnails.request.as_ref().map(|r| r.effect);
        let thumbs_in_chain = active.iter().any(|(index, _, _)| Some(*index) == thumbs_here);
        if active.is_empty() {
            self.last_time = None;
            if thumbs_here.is_some() {
                let base = self.base_uniform(scene, time, frame, hdr, 1.0 / scene.video.fps.max(1) as f32);
                self.draw_thumbnails(device, queue, encoder, hdr_view, None, base, scene);
            }
            return;
        }

        let (width, height) = (hdr.width(), hdr.height());
        let fresh = match self.last_time {
            Some(last) => time < last || time - last > MAX_STEP,
            None => true,
        };
        let dt = match self.last_time {
            Some(last) if !fresh => (time - last) as f32,
            _ => 1.0 / scene.video.fps.max(1) as f32,
        };
        self.last_time = Some(time);
        if self.scratch.as_ref().is_none_or(|(t, _)| t.width() != width || t.height() != height) {
            self.scratch = Some(make_texture(device, width, height, hdr.format(), "effect scratch"));
        }

        let base = self.base_uniform(scene, time, frame, hdr, dt);
        if thumbs_here.is_some() && !thumbs_in_chain {
            self.draw_thumbnails(device, queue, encoder, hdr_view, None, base, scene);
        }

        let mut in_scratch = false;
        for (index, (scene_index, effect, info)) in active.iter().enumerate() {
            if Some(*scene_index) == thumbs_here {
                let source = if in_scratch { &self.scratch.as_ref().unwrap().1 } else { hdr_view };
                let source = source.clone();
                let prev = self.slots.get(index).and_then(|s| s.history.as_ref()).map(|(_, v)| v.clone());
                self.draw_thumbnails(device, queue, encoder, &source, prev.as_ref(), base, scene);
            }
            let pipeline = self.pipelines[&info.id].pipeline.as_ref().unwrap();

            if self.slots.len() <= index {
                self.slots.push(Slot {
                    id: String::new(),
                    uniform: device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("effect uniform"),
                        size: std::mem::size_of::<FxUniform>() as u64,
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }),
                    history: None,
                    frames: 0,
                });
            }
            let slot = &mut self.slots[index];
            if slot.id != info.id || fresh {
                slot.id = info.id.clone();
                slot.frames = 0;
            }
            if info.uses_prev {
                let stale = slot.history.as_ref().is_none_or(|(t, _)| t.width() != width || t.height() != height);
                if stale {
                    slot.history = Some(make_texture(device, width, height, hdr.format(), "effect history"));
                    slot.frames = 0;
                }
                if slot.frames == 0 {
                    clear(encoder, &slot.history.as_ref().unwrap().1);
                }
            }

            let mut uniform = base;
            uniform.amount = effect.amount.clamp(0.0, 1.0);
            uniform.frame = slot.frames as f32;
            uniform.params = info.values(&effect.params);
            queue.write_buffer(&slot.uniform, 0, bytemuck::bytes_of(&uniform));
            slot.frames = slot.frames.saturating_add(1);

            let (scratch_texture, scratch_view) = self.scratch.as_ref().unwrap();
            let (source, target, target_texture) =
                if in_scratch { (scratch_view, hdr_view, hdr) } else { (hdr_view, scratch_view, scratch_texture) };
            let prev = match (&slot.history, info.uses_prev) {
                (Some((_, view)), true) => view,
                _ => &self.black,
            };
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("effect inputs"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: slot.uniform.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(source) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(prev) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            });
            {
                let mut pass = crate::begin_pass(encoder, target, "effect", wgpu::LoadOp::Clear(wgpu::Color::BLACK));
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            if info.uses_prev
                && let Some((history, _)) = &slot.history
            {
                copy(encoder, target_texture, history);
            }
            in_scratch = !in_scratch;
        }
        if in_scratch {
            copy(encoder, &self.scratch.as_ref().unwrap().0, hdr);
        }
    }

    /// The uniform values every effect in this frame shares.
    fn base_uniform(&self, scene: &Scene, time: f64, frame: &FrameData, hdr: &wgpu::Texture, dt: f32) -> FxUniform {
        let (width, height) = (hdr.width(), hdr.height());
        let video = &scene.video;
        let mut base = FxUniform::zeroed();
        base.resolution = [width as f32, height as f32];
        base.time = time as f32;
        base.pulse = frame.pulse;
        base.audio = frame.audio.map(|v| if v.is_finite() { v } else { 0.0 });
        base.level = if frame.level.is_finite() { frame.level } else { 0.0 };
        base.dt = dt;
        base.scale = width as f32 / video.width.max(1) as f32;
        base.hit_count = frame.hits.len().min(MAX_HITS) as u32;
        for (i, hit) in frame.hits.iter().take(MAX_HITS).enumerate() {
            let uv = [hit.position[0] / video.width as f32 + 0.5, hit.position[1] / video.height as f32 + 0.5];
            base.hits[i] = [uv[0], uv[1], hit.age, hit.strength];
            base.hit_colors[i] = hit.color;
        }
        base
    }

    /// Draws the requested block previews from `source`, the picture coming into the effect.
    #[allow(clippy::too_many_arguments)]
    fn draw_thumbnails(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        prev: Option<&wgpu::TextureView>,
        base: FxUniform,
        scene: &Scene,
    ) {
        let Some(request) = self.thumbnails.request.clone() else { return };
        let Ok(code) = &request.info.compiled else { return };
        let current = self.thumbnails.pipeline.as_ref().is_some_and(|(info, _)| same_code(info, &request.info));
        if current {
            self.thumbnails.pipeline.as_mut().unwrap().0 = request.info.clone();
        } else if self.background && self.thumbnails.pipeline.is_some() {
            if self.thumbnails.building.is_none() {
                let building = Building::start(device, &self.pipeline_layout, &request.info, "fs_thumb", THUMB_FORMAT);
                self.thumbnails.building = Some(building);
            }
        } else {
            let result =
                build_pipeline(device, &self.pipeline_layout, "block previews", code, "fs_thumb", THUMB_FORMAT);
            self.thumbnails.pipeline = Some((request.info.clone(), result.ok()));
        }
        let Some((built, Some(_))) = &self.thumbnails.pipeline else { return };
        // While a new version builds, the old one stands in only if it has the same blocks in
        // the same order; otherwise the previews would land on the wrong blocks.
        let blocks =
            |info: &EffectInfo| info.graph.as_ref().and_then(|g| g.generated.as_ref()).map(|g| g.thumbnails.clone());
        if blocks(built) != blocks(&request.info) {
            return;
        }
        let built = built.clone();
        self.thumbnail_atlas(device);
        let thumbs = &mut self.thumbnails;
        while thumbs.uniforms.len() < request.views.len() {
            thumbs.uniforms.push(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("block preview uniform"),
                size: std::mem::size_of::<FxUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let params = scene.effects.get(request.effect).map(|e| built.values(&e.params)).unwrap_or_default();
        let groups: Vec<_> = request
            .views
            .iter()
            .zip(&thumbs.uniforms)
            .map(|(&view, buffer)| {
                let mut uniform = base;
                uniform.amount = 1.0;
                uniform.view = view;
                uniform.params = params;
                queue.write_buffer(buffer, 0, bytemuck::bytes_of(&uniform));
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("block preview inputs"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(source) },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(prev.unwrap_or(&self.black)),
                        },
                        wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    ],
                })
            })
            .collect();
        let atlas = &thumbs.atlas.as_ref().unwrap().1;
        let Some((_, Some(pipeline))) = &thumbs.pipeline else { return };
        let mut pass = crate::begin_pass(encoder, atlas, "block previews", wgpu::LoadOp::Clear(wgpu::Color::BLACK));
        pass.set_pipeline(pipeline);
        for (i, group) in groups.iter().enumerate() {
            let [x, y, w, h] = thumbnail_tile(i);
            pass.set_viewport(x as f32, y as f32, w as f32, h as f32, 0.0, 1.0);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        }
        thumbs.drawn = true;
    }

    /// Builds the pipeline for an effect, or rebuilds it when the library has a newer version.
    fn prepare(&mut self, device: &wgpu::Device, info: &Arc<EffectInfo>) {
        if let Some(built) = self.pipelines.get_mut(&info.id)
            && same_code(&built.info, info)
        {
            built.info = info.clone();
            return;
        }
        if info.compiled.is_err() {
            return;
        }
        if self.background && self.pipelines.contains_key(&info.id) {
            // Keep drawing the old version until the new one is ready. Once a build finishes
            // and the effect has changed again since, the next frame starts another.
            if !self.building.contains_key(&info.id) {
                let building = Building::start(device, &self.pipeline_layout, info, "fs_main", crate::HDR_FORMAT);
                self.building.insert(info.id.clone(), building);
            }
            return;
        }
        let Ok(source) = &info.compiled else { return };
        let result = build_pipeline(device, &self.pipeline_layout, &info.id, source, "fs_main", crate::HDR_FORMAT);
        self.install(info.clone(), result);
    }
}

fn make_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    label: &str,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    (texture, view)
}

fn clear(encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
    crate::begin_pass(encoder, view, "effect history clear", wgpu::LoadOp::Clear(wgpu::Color::BLACK));
}

fn copy(encoder: &mut wgpu::CommandEncoder, from: &wgpu::Texture, to: &wgpu::Texture) {
    encoder.copy_texture_to_texture(from.as_image_copy(), to.as_image_copy(), from.size());
}

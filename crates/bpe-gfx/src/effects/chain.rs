//! The effect chain: runs a frame's shader effects over the HDR picture, in place, one stage at
//! a time. Each effect is compiled on first sight and kept; an effect that reads its previous
//! frame keeps a history texture under the call's key.

use std::collections::{BTreeMap, BTreeSet};

use bytemuck::{Pod, Zeroable};

use super::{MAX_PARAM_FLOATS, assemble, needs_history};
use crate::{EffectCall, Frame, HDR_FORMAT, OBJECT_FORMAT, Stage};

pub(crate) const MAX_HITS: usize = 16;
/// How far back effects see hits, in seconds.
const HIT_MEMORY: f32 = 3.0;
/// A jump in time bigger than this (a seek, or a new export) starts feedback effects afresh.
pub(crate) const MAX_STEP: f32 = 0.25;
/// The hit speed at which a hit is at full strength.
const FULL_HIT_SPEED: f32 = 600.0;
/// Compiled effects kept beyond the ones a frame uses, before unused ones are dropped.
const KEEP_COMPILED: usize = 64;

/// Matches `struct Fx` in prelude.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Fx {
    resolution: [f32; 2],
    time: f32,
    blend: f32,
    pulse: f32,
    hit_count: u32,
    frame: f32,
    dt: f32,
    scale: f32,
    level: f32,
    view: u32,
    apply_to: f32,
    in_layer: f32,
    _pad: [f32; 3],
    audio: [f32; 4],
    params: [f32; MAX_PARAM_FLOATS],
    hits: [[f32; 4]; MAX_HITS],
    hit_colors: [[f32; 4]; MAX_HITS],
}

struct Compiled {
    pipeline: wgpu::RenderPipeline,
    history: bool,
}

/// One call's state from frame to frame.
struct Slot {
    history: Option<(wgpu::Texture, wgpu::TextureView)>,
    frames: u32,
    last_time: Option<f32>,
    used: bool,
}

pub(crate) struct Chain {
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    sampler: wgpu::Sampler,
    black: wgpu::TextureView,
    /// Stands in for the object picture when nothing drew one: no things anywhere.
    no_objects: wgpu::TextureView,
    compiled: BTreeMap<u64, Result<Compiled, String>>,
    /// The effects this frame asked for, for the problem list and for pruning.
    used: BTreeSet<u64>,
    uniforms: Vec<wgpu::Buffer>,
    used_uniforms: usize,
    slots: BTreeMap<u64, Slot>,
    scratch: Option<(wgpu::Texture, wgpu::TextureView)>,
}

/// The key an effect's text is cached under: FNV-1a of the text.
pub(crate) fn key_of(wgsl: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in wgsl.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// The `// name:` line of an effect, for messages.
fn name_of(wgsl: &str) -> String {
    super::header_lines(wgsl)
        .filter_map(|(_, l)| l.trim().strip_prefix("name:"))
        .map(|n| n.trim().to_owned())
        .find(|n| !n.is_empty())
        .unwrap_or_else(|| "effect".to_owned())
}

impl Chain {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let texture_entry = |binding, filterable| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable },
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
                texture_entry(1, true),
                texture_entry(2, true),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                texture_entry(4, false),
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
        let black = make_texture(device, 1, 1, HDR_FORMAT, "effect black").1;
        let no_objects = make_texture(device, 1, 1, OBJECT_FORMAT, "no objects").1;
        Self {
            layout,
            pipeline_layout,
            sampler,
            black,
            no_objects,
            compiled: BTreeMap::new(),
            used: BTreeSet::new(),
            uniforms: Vec::new(),
            used_uniforms: 0,
            slots: BTreeMap::new(),
            scratch: None,
        }
    }

    /// Whether this frame needs the object picture drawn: an effect on one thing, or one that
    /// reads what is where itself (a material on the scene paints every thing).
    pub(crate) fn wants_objects(frame: &Frame) -> bool {
        frame.effects.iter().any(|e| e.blend > 0.0 && (e.apply_to > 0.5 || super::needs_objects(&e.wgsl)))
    }

    /// Starts a frame: forgets which effects and uniforms the last one used.
    pub(crate) fn begin_frame(&mut self) {
        self.used.clear();
        self.used_uniforms = 0;
        for slot in self.slots.values_mut() {
            slot.used = false;
        }
    }

    /// Ends a frame: drops the histories of calls that are gone, and old compiled effects once
    /// there are many.
    pub(crate) fn end_frame(&mut self) {
        self.slots.retain(|_, s| s.used);
        if self.compiled.len() > KEEP_COMPILED {
            let used = &self.used;
            self.compiled.retain(|k, _| used.contains(k));
        }
    }

    /// Why effects this frame asked for could not be built, one line each.
    pub(crate) fn problems(&self) -> impl Iterator<Item = &str> {
        self.used.iter().filter_map(|k| self.compiled.get(k)?.as_ref().err().map(String::as_str))
    }

    fn compile(&mut self, device: &wgpu::Device, wgsl: &str) -> u64 {
        let key = key_of(wgsl);
        self.used.insert(key);
        if !self.compiled.contains_key(&key) {
            let result = build(device, &self.pipeline_layout, wgsl);
            self.compiled.insert(key, result);
        }
        key
    }

    /// Runs `frame`'s effects of `stage` that `on` picks for this picture, in order, over `hdr`
    /// in place. `hdr` is a blend layer's picture when `on` says so: the effects then keep its
    /// cover (alpha) and premultiply their colour by it.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        stage: Stage,
        frame: &Frame,
        on: Canvas,
        hdr: &wgpu::Texture,
        hdr_view: &wgpu::TextureView,
        objects: Option<&wgpu::TextureView>,
    ) {
        let mut active = Vec::new();
        let picked = |e: &&EffectCall| e.stage == stage && e.blend > 0.0 && on.runs(frame, e);
        for call in frame.effects.iter().filter(picked) {
            let key = self.compile(device, &call.wgsl);
            if self.compiled.get(&key).is_some_and(|c| c.is_ok()) {
                active.push((call, key));
            }
        }
        if active.is_empty() {
            return;
        }
        let (width, height) = (hdr.width(), hdr.height());
        if self.scratch.as_ref().is_none_or(|(t, _)| t.width() != width || t.height() != height) {
            self.scratch = Some(make_texture(device, width, height, HDR_FORMAT, "effect scratch"));
        }
        let objects = objects.unwrap_or(&self.no_objects).clone();
        let base = base_uniform(frame, width, height);

        let mut in_scratch = false;
        for (call, key) in active {
            let compiled = match self.compiled.get(&key) {
                Some(Ok(c)) => c,
                _ => continue,
            };
            let slot =
                self.slots.entry(call.key).or_insert(Slot { history: None, frames: 0, last_time: None, used: true });
            slot.used = true;
            let fresh = match slot.last_time {
                Some(last) => frame.time < last || frame.time - last > MAX_STEP,
                None => true,
            };
            let dt = match slot.last_time {
                Some(last) if !fresh => frame.time - last,
                _ => 1.0 / 60.0,
            };
            slot.last_time = Some(frame.time);
            if fresh {
                slot.frames = 0;
            }
            if compiled.history {
                let stale = slot.history.as_ref().is_none_or(|(t, _)| t.width() != width || t.height() != height);
                if stale {
                    slot.history = Some(make_texture(device, width, height, HDR_FORMAT, "effect history"));
                    slot.frames = 0;
                }
                if slot.frames == 0 {
                    clear(encoder, &slot.history.as_ref().expect("made above").1);
                }
            } else {
                slot.history = None;
            }

            let mut uniform = base;
            uniform.blend = call.blend.clamp(0.0, 1.0);
            uniform.frame = slot.frames as f32;
            uniform.dt = dt;
            uniform.params = call.params;
            uniform.apply_to = call.apply_to;
            uniform.in_layer = if matches!(on, Canvas::Layer(_)) { 1.0 } else { 0.0 };
            slot.frames = slot.frames.saturating_add(1);
            if self.uniforms.len() <= self.used_uniforms {
                self.uniforms.push(device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("effect uniform"),
                    size: std::mem::size_of::<Fx>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            }
            let buffer = &self.uniforms[self.used_uniforms];
            self.used_uniforms += 1;
            queue.write_buffer(buffer, 0, bytemuck::bytes_of(&uniform));

            let (scratch_texture, scratch_view) = self.scratch.as_ref().expect("made above");
            let (source, target_view, target_texture) =
                if in_scratch { (scratch_view, hdr_view, hdr) } else { (hdr_view, scratch_view, scratch_texture) };
            let prev = match (&slot.history, compiled.history) {
                (Some((_, view)), true) => view,
                _ => &self.black,
            };
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("effect inputs"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(source) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(prev) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&objects) },
                ],
            });
            {
                let mut pass =
                    crate::begin_pass(encoder, target_view, "effect", wgpu::LoadOp::Clear(wgpu::Color::BLACK));
                pass.set_pipeline(&compiled.pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            if compiled.history
                && let Some((history, _)) = &slot.history
            {
                copy(encoder, target_texture, history);
            }
            in_scratch = !in_scratch;
        }
        if in_scratch {
            copy(encoder, &self.scratch.as_ref().expect("made above").0, hdr);
        }
    }
}

/// Which picture a run of the chain paints: the main one, or one of the frame's blend layers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Canvas {
    Main,
    Layer(usize),
}

impl Canvas {
    /// Whether `call` runs on this picture. An effect on things at the Things stage runs where
    /// its thing is drawn: on the main picture if the thing draws anything there, else on the
    /// first blend layer it draws into, before that layer is mixed in (so the picture under a
    /// blended thing is left alone). Every other effect runs on the main picture.
    fn runs(self, frame: &Frame, call: &EffectCall) -> bool {
        self == canvas_of(frame, call)
    }
}

/// See [`Canvas::runs`].
pub(crate) fn canvas_of(frame: &Frame, call: &EffectCall) -> Canvas {
    if call.stage != Stage::Things
        || call.apply_to <= 0.5
        || frame.layers.is_empty()
        || frame.draws_thing(call.apply_to)
    {
        return Canvas::Main;
    }
    match frame.layers.iter().position(|l| l.frame.draws_thing(call.apply_to)) {
        Some(i) => Canvas::Layer(i),
        None => Canvas::Main,
    }
}

/// The uniform values every effect in a frame shares: the picture, the time and the hits.
fn base_uniform(frame: &Frame, width: u32, height: u32) -> Fx {
    let mut base = Fx::zeroed();
    base.resolution = [width as f32, height as f32];
    base.time = frame.time;
    base.scale = width as f32 / frame.width.max(1) as f32;
    // Newest hits first, the ones young enough for effects to still care about.
    let mut hits: Vec<_> = frame
        .hits
        .iter()
        .filter(|h| {
            let age = frame.time - h.time;
            (0.0..HIT_MEMORY).contains(&age)
        })
        .collect();
    hits.sort_by(|a, b| b.time.total_cmp(&a.time));
    let mut energy = 0.0;
    for (i, hit) in hits.iter().enumerate() {
        let age = frame.time - hit.time;
        let strength = (hit.speed / FULL_HIT_SPEED).clamp(0.0, 1.5);
        energy += strength * (-6.0 * age).exp();
        if i < MAX_HITS {
            // Where the hit is in the picture, through the camera.
            let at = frame.camera.to_screen(glam::Vec2::from(hit.point));
            let uv = [at.x / frame.width.max(1) as f32 + 0.5, at.y / frame.height.max(1) as f32 + 0.5];
            base.hits[i] = [uv[0], uv[1], age, strength];
            base.hit_colors[i] = hit.color;
        }
    }
    base.hit_count = hits.len().min(MAX_HITS) as u32;
    base.pulse = 1.0 - (-energy).exp();
    base.audio = [base.pulse, base.pulse, base.pulse, 0.0];
    base.level = base.pulse;
    base
}

/// Builds the render pipeline for one effect, or says why it could not be.
fn build(device: &wgpu::Device, layout: &wgpu::PipelineLayout, wgsl: &str) -> Result<Compiled, String> {
    let name = name_of(wgsl);
    let full = assemble(wgsl).map_err(|e| format!("{name}: {e}"))?;
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(&name),
        source: wgpu::ShaderSource::Wgsl(full.into()),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(&name),
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
            entry_point: Some("fs_main"),
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
    match pollster::block_on(scope.pop()) {
        None => Ok(Compiled { pipeline, history: needs_history(wgsl) }),
        Some(error) => {
            Err(format!("{name}: {}", error.to_string().lines().next().unwrap_or("the GPU refused the shader")))
        }
    }
}

pub(crate) fn make_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    label: &str,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 },
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{
        BallInstance, EffectCall, FlashInstance, Frame, HitMark, Renderer, ShapeInstance, headless_device, thing_code,
    };
    use bpe_core::ThingId;
    use bpe_runtime::Outline;
    use glam::Vec2;

    pub(crate) const W: u32 = 270;
    pub(crate) const H: u32 = 480;
    pub(crate) const A: Vec2 = Vec2::new(-200.0, 0.0);
    pub(crate) const B: Vec2 = Vec2::new(200.0, 0.0);
    pub(crate) const WALL: Vec2 = Vec2::new(0.0, -450.0);
    pub(crate) const EMPTY: Vec2 = Vec2::new(0.0, -200.0);

    const GREEN: &str =
        "// name: Green\nfn effect(uv: vec2<f32>, p: Params) -> vec3<f32> { return vec3<f32>(0.0, 1.0, 0.0); }\n";

    /// Ball A (thing 2) left, ball B (thing 3) right, a box wall (thing 1) whose top runs along
    /// y = -450, no bloom.
    pub(crate) fn two_balls_frame() -> Frame {
        let mut frame = Frame { bloom: 0.0, ..Frame::default() };
        let wall = Outline::Box { center: Vec2::ZERO, half: Vec2::new(500.0, 450.0), angle: 0.0, thickness: 24.0 };
        frame.shapes.push(
            ShapeInstance::outline(&wall, crate::PaintGpu::default(), [1.0, 1.0, 1.0, 1.0], 0.0)
                .code(thing_code(ThingId(1))),
        );
        let ball = |center: Vec2, thing: u32, seed: f32| BallInstance {
            center: center.into(),
            radius: 70.0,
            glow: 0.0,
            color: [1.0, 0.25, 0.25, 1.0],
            code: [thing_code(ThingId(thing)) + seed * 0.5, seed, 0.0, 0.0],
            paint: crate::PaintGpu::default(),
        };
        frame.balls.push(ball(A, 2, 0.2));
        frame.balls.push(ball(B, 3, 0.6));
        frame
    }

    pub(crate) fn call(wgsl: &'static str, apply_to: f32, stage: Stage) -> EffectCall {
        EffectCall { key: 1, wgsl: wgsl.into(), params: [0.0; 16], blend: 1.0, apply_to, stage }
    }

    pub(crate) fn render(frame: &Frame) -> Option<Vec<u8>> {
        let Ok((device, queue)) = headless_device() else {
            eprintln!("no GPU adapter available, skipping");
            return None;
        };
        let mut renderer = Renderer::new(&device, W, H);
        renderer.render(&device, &queue, frame);
        Some(renderer.read_output(&device, &queue).unwrap())
    }

    /// The pixel at a world position.
    pub(crate) fn pixel(pixels: &[u8], at: Vec2) -> [u8; 3] {
        let x = ((at.x / 1080.0 + 0.5) * W as f32) as usize;
        let y = ((at.y / 1920.0 + 0.5) * H as f32) as usize;
        let i = (y * W as usize + x) * 4;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    }

    fn green(p: [u8; 3]) -> bool {
        p[1] > 200 && p[0] < 40 && p[2] < 40
    }

    fn brightness(pixels: &[u8]) -> u64 {
        pixels.chunks(4).map(|p| p[0] as u64 + p[1] as u64 + p[2] as u64).sum()
    }

    #[test]
    fn keys_and_names_come_from_the_text() {
        assert_ne!(key_of("a"), key_of("b"));
        assert_eq!(key_of(GREEN), key_of(GREEN));
        assert_eq!(name_of(GREEN), "Green");
        assert_eq!(name_of("fn effect("), "effect");
        assert_eq!(std::mem::size_of::<Fx>(), 656);
    }

    #[test]
    fn the_base_uniform_lists_the_newest_hits_and_their_pulse() {
        let mut frame = Frame { time: 2.0, ..Frame::default() };
        let hit = |time: f32, speed: f32| HitMark {
            thing: ThingId(2),
            other: Some(ThingId(1)),
            point: [540.0, 0.0],
            normal: [0.0, -1.0],
            time,
            speed,
            color: [1.0, 0.0, 0.0, 1.0],
        };
        frame.hits.push(hit(1.0, 300.0));
        frame.hits.push(hit(1.9, 600.0));
        frame.hits.push(hit(-5.0, 600.0)); // too old
        frame.hits.push(hit(2.5, 600.0)); // not yet
        let fx = base_uniform(&frame, 270, 480);
        assert_eq!(fx.hit_count, 2);
        assert!((fx.hits[0][2] - 0.1).abs() < 1e-5, "newest first: {:?}", fx.hits[0]);
        assert_eq!(fx.hits[0][3], 1.0);
        assert_eq!(fx.hits[0][..2], [1.0, 0.5]);
        assert_eq!(fx.hits[1][3], 0.5);
        assert!(fx.pulse > 0.4 && fx.pulse < 0.7, "{}", fx.pulse);
        assert_eq!(fx.audio[0], fx.pulse);
        assert_eq!(fx.scale, 0.25);
        assert_eq!(base_uniform(&Frame::default(), 270, 480).pulse, 0.0);
    }

    #[test]
    fn hit_effect_follows_camera() {
        let mut frame = Frame { time: 1.0, ..Frame::default() };
        frame.camera = bpe_runtime::CameraView { center: glam::Vec2::new(300.0, 0.0), zoom: 1.0, turn: 0.0 };
        frame.hits.push(HitMark {
            thing: ThingId(2),
            other: None,
            point: [300.0, 0.0],
            normal: [0.0, -1.0],
            time: 0.95,
            speed: 600.0,
            color: [1.0; 4],
        });
        let fx = base_uniform(&frame, 270, 480);
        assert_eq!(fx.hits[0][..2], [0.5, 0.5], "a hit at the camera's centre is mid-picture");
    }

    #[test]
    fn a_picture_effect_paints_everything_and_a_material_only_its_thing() {
        let Some(before) = render(&two_balls_frame()) else { return };
        assert!(!green(pixel(&before, A)) && pixel(&before, WALL)[0] > 150, "{:?}", pixel(&before, WALL));
        let mut picture = two_balls_frame();
        picture.effects.push(call(GREEN, 0.0, Stage::Picture));
        let after = render(&picture).unwrap();
        for at in [A, B, WALL, EMPTY] {
            assert!(green(pixel(&after, at)), "{at:?}: {:?}", pixel(&after, at));
        }
        let mut ball_b = two_balls_frame();
        ball_b.effects.push(call(GREEN, thing_code(ThingId(3)), Stage::Picture));
        let after = render(&ball_b).unwrap();
        assert!(green(pixel(&after, B)), "{:?}", pixel(&after, B));
        for at in [A, WALL, EMPTY] {
            assert_eq!(pixel(&after, at), pixel(&before, at), "{at:?} changed");
        }
        let mut wall = two_balls_frame();
        wall.effects.push(call(GREEN, thing_code(ThingId(1)), Stage::Things));
        let after = render(&wall).unwrap();
        assert!(green(pixel(&after, WALL)), "{:?}", pixel(&after, WALL));
        assert_eq!(pixel(&after, A), pixel(&before, A));
        let mut nobody = two_balls_frame();
        nobody.effects.push(call(GREEN, thing_code(ThingId(9)), Stage::Picture));
        assert_eq!(render(&nobody).unwrap(), before, "a thing that isn't there changes nothing");
    }

    #[test]
    fn a_material_finds_a_thing_drawn_in_a_blend_layer() {
        let mut frame = two_balls_frame();
        let b = frame.balls.pop().unwrap();
        frame.layer_mut(bpe_core::Blend::Screen, 1.0, false).balls.push(b);
        frame.effects.push(call(GREEN, thing_code(ThingId(3)), Stage::Picture));
        let Some(after) = render(&frame) else { return };
        assert!(green(pixel(&after, B)), "{:?}: the layered ball is in the object picture", pixel(&after, B));
        assert!(!green(pixel(&after, A)) && !green(pixel(&after, EMPTY)));
    }

    #[test]
    fn a_material_on_a_layered_thing_paints_the_layer_not_the_picture_under_it() {
        // Ball B drawn half see-through over a grey picture, with a green material on it.
        let grey = |frame: &mut Frame| {
            let c = [0.3, 0.3, 0.3, 1.0];
            frame.background = crate::BackgroundPaint { row: 0, ends: Some([c, c]), ..Default::default() };
        };
        let mut frame = two_balls_frame();
        grey(&mut frame);
        let b = frame.balls.pop().unwrap();
        frame.layer_mut(bpe_core::Blend::Normal, 0.5, false).balls.push(b);
        let Some(plain) = render(&frame) else { return };
        frame.effects.push(call(GREEN, thing_code(ThingId(3)), Stage::Things));
        let after = render(&frame).unwrap();
        // Half green over grey: no red from the ball's own colour shows through.
        let p = pixel(&after, B);
        assert!(p[1] > p[0] + 60 && p[1] > p[2] + 60, "{p:?}: the material paints the layered ball");
        let mut green_under = two_balls_frame();
        grey(&mut green_under);
        green_under.balls.pop();
        let grey_only = render(&green_under).unwrap();
        let g = pixel(&grey_only, B);
        assert!(p[0] <= g[0] + 2, "{p:?} vs grey {g:?}: the ball's red is gone, not mixed over green");
        for at in [A, WALL, EMPTY] {
            assert_eq!(pixel(&after, at), pixel(&plain, at), "{at:?}: the layer stays see-through elsewhere");
        }
    }

    #[test]
    fn a_material_on_a_multiply_or_darken_thing_blending_with_each_other_leaves_no_dark_ring() {
        // Ball B multiplied (or darkened) over a light grey picture, a material on it. Those
        // layers start from 1 or far above it, which a material cannot paint over cleanly, so
        // with a material they are drawn as if the switch were off.
        for blend in [bpe_core::Blend::Multiply, bpe_core::Blend::Darken] {
            let shot = |each: bool| {
                let mut frame = two_balls_frame();
                let c = [0.6, 0.6, 0.6, 1.0];
                frame.background = crate::BackgroundPaint { row: 0, ends: Some([c, c]), ..Default::default() };
                let b = frame.balls.pop().unwrap();
                frame.layer_mut(blend, 1.0, each).balls.push(b);
                assert_eq!(frame.layers[0].each, each);
                frame.effects.push(call(GREEN, thing_code(ThingId(3)), Stage::Things));
                render(&frame)
            };
            let Some(off) = shot(false) else { return };
            let on = shot(true).unwrap();
            // The ball's edge, where a ring would show.
            let edge = B + Vec2::new(69.0, 0.0);
            assert_eq!(pixel(&on, edge), pixel(&off, edge), "{blend:?}: the edge");
            assert!(on == off, "{blend:?}: the same picture as with the switch off");
        }
    }

    #[test]
    fn blend_zero_leaves_the_picture_unchanged() {
        let Some(before) = render(&two_balls_frame()) else { return };
        let mut off = two_balls_frame();
        off.effects.push(EffectCall { blend: 0.0, ..call(GREEN, 0.0, Stage::Picture) });
        assert_eq!(render(&off).unwrap(), before);
    }

    #[test]
    fn a_broken_effect_is_skipped_and_reported() {
        let Ok((device, queue)) = headless_device() else { return };
        let mut renderer = Renderer::new(&device, W, H);
        let mut frame = two_balls_frame();
        renderer.render(&device, &queue, &frame);
        let plain = renderer.read_output(&device, &queue).unwrap();
        frame.effects.push(EffectCall {
            key: 9,
            wgsl: "// name: Broken\nfn effect(uv: vec2<f32>, p: Params) -> vec3<f32> { return nonsense; }".into(),
            params: [0.0; 16],
            blend: 1.0,
            apply_to: 0.0,
            stage: Stage::Picture,
        });
        frame.effects.push(EffectCall {
            key: 10,
            wgsl: "// name: Unfinished\nfn nothing() {}".into(),
            params: [0.0; 16],
            blend: 1.0,
            apply_to: 0.0,
            stage: Stage::Picture,
        });
        renderer.render(&device, &queue, &frame);
        let problems = renderer.shader_problems();
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems.iter().any(|p| p.starts_with("Broken: ")), "{problems:?}");
        assert!(problems.iter().any(|p| p.starts_with("Unfinished: ") && p.contains("fn effect(")), "{problems:?}");
        assert_eq!(renderer.read_output(&device, &queue).unwrap(), plain, "the broken effects changed nothing");
        frame.effects.clear();
        renderer.render(&device, &queue, &frame);
        assert!(renderer.shader_problems().is_empty(), "problems clear once the effects are gone");
    }

    #[test]
    fn echo_builds_up_and_restarts_after_a_seek() {
        const ECHO: &str = "// name: Echo test\nfn effect(uv: vec2<f32>, p: Params) -> vec3<f32> { return prev(uv) * 0.9 + sample(uv); }\n";
        let Ok((device, queue)) = headless_device() else { return };
        let mut renderer = Renderer::new(&device, W, H);
        let mut frame = two_balls_frame();
        frame.effects.push(call(ECHO, 0.0, Stage::Picture));
        let mut at = |t: f32| {
            frame.time = t;
            renderer.render(&device, &queue, &frame);
            renderer.read_output(&device, &queue).unwrap()
        };
        let first = at(0.0);
        let b0 = brightness(&first);
        let b1 = brightness(&at(1.0 / 60.0));
        let b2 = brightness(&at(2.0 / 60.0));
        assert!(b1 > b0 && b2 > b1, "{b0} {b1} {b2}");
        let jumped = at(5.0);
        assert_eq!(brightness(&jumped), b0, "a jump starts the echo afresh");
        assert!(brightness(&at(5.0 + 1.0 / 60.0)) > b0);
        assert!(brightness(&at(3.0)) == b0, "going backwards starts afresh too");
    }

    #[test]
    fn every_builtin_effect_changes_the_picture() {
        use super::super::{BUILTINS, defaults};
        let Ok((device, queue)) = headless_device() else { return };
        let mut renderer = Renderer::new(&device, W, H);
        // Bigger balls, a blue one on the right, and a hit at the left one a tenth of a second ago.
        let mut base = two_balls_frame();
        for b in &mut base.balls {
            b.radius = 100.0;
        }
        base.balls[1].color = [0.25, 0.25, 1.0, 1.0];
        base.hits.push(HitMark {
            thing: ThingId(2),
            other: Some(ThingId(1)),
            point: A.into(),
            normal: [0.0, -1.0],
            time: 1.9,
            speed: 600.0,
            color: [1.0, 0.25, 0.25, 1.0],
        });
        let second = 2.0 + 1.0 / 60.0;
        let render_at = |renderer: &mut Renderer, frame: &mut Frame, t: f32| {
            frame.time = t;
            renderer.render(&device, &queue, frame);
            renderer.read_output(&device, &queue).unwrap()
        };
        let plain = render_at(&mut renderer, &mut base.clone(), second);
        for builtin in &BUILTINS {
            let header = builtin.header();
            let material = header.stage == Stage::Things;
            let mut frame = base.clone();
            let mut params = defaults(header);
            if builtin.id == "echo" {
                // Still balls echo onto themselves; a zoom makes the ghosts show.
                params[1] = 1.05;
            }
            frame.effects.push(EffectCall {
                key: 1,
                wgsl: builtin.wgsl.into(),
                params,
                blend: 1.0,
                apply_to: if material { thing_code(ThingId(2)) } else { 0.0 },
                stage: header.stage,
            });
            // Twice, so feedback effects have a previous frame to show.
            render_at(&mut renderer, &mut frame, 2.0);
            let after = render_at(&mut renderer, &mut frame, second);
            assert!(renderer.shader_problems().is_empty(), "{}: {:?}", builtin.id, renderer.shader_problems());
            let changed = plain.chunks(4).zip(after.chunks(4)).filter(|(a, b)| a[..3] != b[..3]).count();
            assert!(changed * 500 > plain.len() / 4, "{}: only {changed} pixels changed", builtin.id);
            if material {
                assert_eq!(pixel(&after, B), pixel(&plain, B), "{}: the other ball changed", builtin.id);
                assert_ne!(pixel(&after, A), pixel(&plain, A), "{}: its own ball didn't change", builtin.id);
            }
        }
    }

    #[test]
    fn only_effects_that_read_the_object_picture_want_it() {
        let hit_glow = crate::effects::Builtin::find("hit_glow").unwrap().wgsl;
        let lava = crate::effects::Builtin::find("lava").unwrap().wgsl;
        let mut frame = Frame::default();
        frame.effects.push(call(hit_glow, 0.0, Stage::Things));
        assert!(!Chain::wants_objects(&frame), "a flare over the whole picture reads no objects");
        frame.effects.push(call(lava, 0.0, Stage::Things));
        assert!(Chain::wants_objects(&frame), "lava on the scene paints every thing");
        frame.effects.clear();
        frame.effects.push(call(hit_glow, 2.0, Stage::Things));
        assert!(Chain::wants_objects(&frame), "an effect on one thing needs its target");
    }

    #[test]
    fn things_stage_effects_run_before_flashes() {
        let flash = FlashInstance { center: [0.0, 0.0], size: 300.0, color: [3.0, 3.0, 3.0, 1.0] };
        let mut things = two_balls_frame();
        things.flashes.push(flash);
        things.effects.push(call(GREEN, 0.0, Stage::Things));
        let Some(after) = render(&things) else { return };
        let centre = pixel(&after, Vec2::ZERO);
        assert!(centre[0] > 100 && centre[2] > 100, "the flash lands on top of a things-stage effect: {centre:?}");
        let mut picture = two_balls_frame();
        picture.flashes.push(flash);
        picture.effects.push(call(GREEN, 0.0, Stage::Picture));
        let after = render(&picture).unwrap();
        assert!(green(pixel(&after, Vec2::ZERO)), "{:?}", pixel(&after, Vec2::ZERO));
    }
}

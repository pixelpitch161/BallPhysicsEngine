//! What to draw for one frame, built on the CPU by each thing's drawing part.

use std::borrow::Cow;
use std::collections::BTreeMap;

use bpe_core::{Animation, Inputs, Resolved, ResolvedThing, Scene, SlotId, Thing, ThingId};
use bpe_runtime::{EventKind, Log, Outline, World};
use bytemuck::{Pod, Zeroable};
use glam::Vec2;

/// Hits older than this no longer count towards [`hit_strength`].
const HIT_WINDOW: f64 = 0.3;
/// A hit's strength fades by e every this many seconds.
const HIT_FADE: f32 = 0.1;
/// A hit at this speed (px/s) counts as a full one.
const FULL_HIT: f32 = 600.0;

/// How hard things are hitting at `time`, 0 to 1, for pulses that follow hits: every hit in the
/// last 0.3 s counts `min(speed / 600, 1)`, fading with age, and the sum is clamped.
pub fn hit_strength(log: &Log, time: f64, steps_per_second: u32) -> f32 {
    let rate = steps_per_second.max(1);
    let events = log.events();
    let first_step = ((time - HIT_WINDOW) * rate as f64).ceil().max(0.0) as u64;
    let last_step = (time * rate as f64 + 1e-6).floor().max(0.0) as u64;
    let start = events.partition_point(|e| e.step < first_step);
    let mut sum = 0.0f32;
    for e in &events[start..] {
        if e.step > last_step {
            break;
        }
        if e.kind != EventKind::Hit {
            continue;
        }
        let age = (time - e.time(rate)).max(0.0) as f32;
        sum += (e.speed / FULL_HIT).min(1.0) * (-age / HIT_FADE).exp();
    }
    if sum.is_finite() { sum.clamp(0.0, 1.0) } else { 0.0 }
}

/// A ball: a lit sphere with a halo.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct BallInstance {
    pub center: [f32; 2],
    pub radius: f32,
    /// How brightly the halo shines.
    pub glow: f32,
    /// Linear light. An additive colour has alpha -1, see [`blend_alpha`].
    pub color: [f32; 4],
    /// The thing code plus half the seed (see [`thing_code`]), the seed, then two spare.
    pub code: [f32; 4],
}

/// A ball with its own style: a shape, a fill, an outline and a halo.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct LookInstance {
    pub center: [f32; 2],
    pub radius: f32,
    /// 0 to 1, picks the pattern's own variation.
    pub seed: f32,
    /// Linear light; alpha -1 is additive, see [`blend_alpha`].
    pub color: [f32; 4],
    /// The pattern's or gradient's other colour.
    pub color2: [f32; 4],
    pub outline_color: [f32; 4],
    /// Shape code, fill code, pattern size, roundness.
    pub form: [f32; 4],
    /// Outline width in pixels, glow brightness, opacity, angle in radians.
    pub look: [f32; 4],
    /// Direction of travel, scale along it, scale across it.
    pub stretch: [f32; 4],
    /// Glow size as a fraction of the radius, the thing code, two spare.
    pub extent: [f32; 4],
}

/// A wall outline, drawn as an instanced quad with a distance function. `c.x` picks the shape:
/// 0 ring or polygon, 1 box, 2 capsule (a filled rounded line between two points).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct ShapeInstance {
    /// Centre, angle.
    pub a: [f32; 4],
    /// Ring: radius, sides. Box: half width, half height. Capsule: both ends.
    pub b: [f32; 4],
    /// Kind, half line width (a capsule's radius), glow strength, thing code.
    pub c: [f32; 4],
    /// Linear-light colour. Alpha scales the line, not the glow. Below zero the line is added
    /// light (see [`blend_alpha`]).
    pub color: [f32; 4],
}

impl ShapeInstance {
    pub fn outline(outline: &Outline, color: [f32; 4], glow: f32) -> Self {
        let color = blend_alpha(color);
        match *outline {
            Outline::Ring { center, radius, sides, angle, thickness } => Self {
                a: [center.x, center.y, angle, 0.0],
                b: [radius, sides as f32, 0.0, 0.0],
                c: [0.0, thickness * 0.5, glow, 0.0],
                color,
            },
            Outline::Box { center, half, angle, thickness } => Self {
                a: [center.x, center.y, angle, 0.0],
                b: [half.x, half.y, 0.0, 0.0],
                c: [1.0, thickness * 0.5, glow, 0.0],
                color,
            },
        }
    }

    /// A filled rounded line from `from` to `to`, `radius` wide on each side. The same point
    /// twice is a filled disc.
    pub fn capsule(from: glam::Vec2, to: glam::Vec2, radius: f32, color: [f32; 4], glow: f32) -> Self {
        let mid = (from + to) * 0.5;
        Self {
            a: [mid.x, mid.y, 0.0, 0.0],
            b: [from.x, from.y, to.x, to.y],
            c: [2.0, radius, glow, 0.0],
            color: blend_alpha(color),
        }
    }
    /// Marks the shape as belonging to a thing, so an effect can paint just that thing.
    pub fn code(mut self, code: f32) -> Self {
        self.c[3] = code;
        self
    }
}

/// The code a thing has in the object picture: its id plus one, so 0 means nothing. A ball adds
/// half its seed (0 to 1) on top, so `floor(code) - 1` is the thing and `fract(code) * 2` the seed.
pub fn thing_code(id: ThingId) -> f32 {
    id.0 as f32 + 1.0
}

/// A soft additive glow where something hit. The colour carries its strength.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct FlashInstance {
    pub center: [f32; 2],
    pub size: f32,
    pub color: [f32; 4],
}

/// One corner of a soft body's triangles: jelly, rope and cloth are drawn as triangle lists
/// whose edges fade over the last pixel.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct SoftVertex {
    pub position: [f32; 2],
    /// How far inside the shape's edge this corner is, in world units; the fragment fades to
    /// nothing at 0.
    pub inside: f32,
    pub _pad: f32,
    /// Linear light. A negative alpha is additive (see [`blend_alpha`]).
    pub color: [f32; 4],
}

/// One glyph of text, drawn from the signed-distance atlas.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct GlyphInstance {
    /// World rectangle: left, top, right, bottom.
    pub rect: [f32; 4],
    /// Atlas rectangle in texture coordinates: left, top, right, bottom.
    pub uv: [f32; 4],
    /// Linear light fill. A negative alpha is additive.
    pub color: [f32; 4],
    pub outline_color: [f32; 4],
    /// Outline width in world units, glow strength, world units per distance unit (how far one
    /// full step of the atlas value is in the world), unused.
    pub params: [f32; 4],
}

/// One corner of a trail strip.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct TrailVertex {
    pub position: [f32; 2],
    /// Signed distance from the centre line, and the half width at this point.
    pub edge: [f32; 2],
    /// Linear light; alpha -1 is additive (the light itself fades along the streak).
    pub color: [f32; 4],
}

/// Appends a streak through `points` (newest first) as a triangle list: two triangles per
/// segment. The width shrinks from `half_width` at the head to `half_width * (1 - taper)` at the
/// tail, and the colour fades out along it. `pixel` is a pixel in world units, the anti-aliasing
/// margin each edge keeps. Fewer than two points draw nothing.
pub fn push_streak(
    out: &mut Vec<TrailVertex>,
    points: &[Vec2],
    half_width: f32,
    taper: f32,
    color: [f32; 4],
    pixel: f32,
) {
    let n = points.len();
    if n < 2 || half_width <= 0.0 {
        return;
    }
    // Each segment's direction, worked out once: every segment's corners need its neighbours' too.
    let directions: Vec<Option<Vec2>> = points.windows(2).map(|w| (w[1] - w[0]).try_normalize()).collect();
    let direction = |i: usize| directions[i];
    // At a gentle bend the two segments share a side vector so the strip has no gaps. At a sharp
    // bend (a bounce) each segment keeps its own, which avoids a folded spike at the corner.
    let side_at = |i: usize, own: Vec2| {
        let neighbour = if i == 0 || i + 1 >= n {
            None
        } else if own == direction(i).unwrap_or(own) {
            direction(i - 1)
        } else {
            direction(i)
        };
        match neighbour {
            Some(other) if other.dot(own) > 0.5 => (other + own).normalize().perp(),
            _ => own.perp(),
        }
    };
    let taper = taper.clamp(0.0, 1.0);
    let vertex = |i: usize, side: Vec2, sign: f32| {
        let t = i as f32 / (n - 1) as f32;
        let width = half_width * (1.0 - t * taper);
        let reach = width + pixel;
        let fade = (1.0 - t) * (1.0 - t);
        let color = if color[3] < 0.0 {
            [color[0] * fade, color[1] * fade, color[2] * fade, -1.0]
        } else {
            [color[0], color[1], color[2], color[3] * fade]
        };
        TrailVertex { position: (points[i] + side * reach * sign).into(), edge: [reach * sign, width], color }
    };
    for i in 0..n - 1 {
        let Some(own) = direction(i) else { continue };
        let (s0, s1) = (side_at(i, own), side_at(i + 1, own));
        let (a, b) = (vertex(i, s0, 1.0), vertex(i, s0, -1.0));
        let (c, d) = (vertex(i + 1, s1, 1.0), vertex(i + 1, s1, -1.0));
        out.extend_from_slice(&[a, b, c, b, d, c]);
    }
}

/// A puff of particles let off at one moment. The GPU works every particle out from the burst
/// and its age, so nothing is kept between frames.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Burst {
    pub origin: [f32; 2],
    /// The way the puff flies, unit length.
    pub normal: [f32; 2],
    /// When it went off, in seconds into the clip.
    pub time: f32,
    /// Linear light.
    pub color: [f32; 4],
    pub count: u32,
    /// Pixels per second.
    pub speed: f32,
    /// How wide the puff fans out, in degrees.
    pub spread: f32,
    /// Pixels.
    pub size: f32,
    /// Seconds each particle lasts.
    pub life: f32,
    /// Pixels per second squared, downwards.
    pub gravity: f32,
    pub drag: f32,
    /// 0 sparks, 1 smoke, 2 confetti, 3 stars, 4 shards, 5 rings.
    pub shape: u32,
    /// Added light rather than covering paint.
    pub additive: bool,
    pub seed: u32,
}

/// Where something hit recently, for modules that react to hits: particles, shader pulses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HitMark {
    /// The thing that hit (a ball group).
    pub thing: ThingId,
    /// What it hit, when known.
    pub other: Option<ThingId>,
    pub point: [f32; 2],
    /// Unit length, pointing from what was hit towards the thing that hit it.
    pub normal: [f32; 2],
    /// Seconds into the clip.
    pub time: f32,
    pub speed: f32,
    /// The hitting thing's linear-light colour.
    pub color: [f32; 4],
}

/// When a shader effect runs: on the things before the flashes and particles are drawn over
/// them, or on the whole picture after.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Things,
    Picture,
}

/// One run of a shader effect over the picture.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectCall {
    /// Tells this call's history (for effects that read their previous frame) apart from any
    /// other's, frame after frame.
    pub key: u64,
    /// The effect file: a header and `fn effect(uv, p)`, see `effects`.
    pub wgsl: Cow<'static, str>,
    /// The effect's params packed in header order, colours taking three.
    pub params: [f32; 16],
    /// How much of the effect shows, 0 to 1.
    pub blend: f32,
    /// 0 for the whole picture, else the [`thing_code`] of the one thing it paints.
    pub apply_to: f32,
    pub stage: Stage,
}

/// A colour with alpha 0 but some light is an additive colour (the colour picker's "Additive"
/// blending) and gets an alpha of -1. The shaders read a negative alpha as "add this much light
/// and cover nothing", so it can still fade like any other alpha.
pub fn blend_alpha(c: [f32; 4]) -> [f32; 4] {
    if c[3] == 0.0 && (c[0] > 0.0 || c[1] > 0.0 || c[2] > 0.0) { [c[0], c[1], c[2], -1.0] } else { c }
}

/// Everything the renderer needs for one frame: the picture's globals and the instance lists.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// The video's size in pixels; world coordinates are these pixels, origin at the centre, y
    /// down.
    pub width: u32,
    pub height: u32,
    pub time: f32,
    pub background_top: [f32; 4],
    pub background_bottom: [f32; 4],
    pub bloom: f32,
    pub exposure: f32,
    pub shapes: Vec<ShapeInstance>,
    pub balls: Vec<BallInstance>,
    pub flashes: Vec<FlashInstance>,
    /// Triangles, three vertices each, drawn between the walls and the balls.
    pub soft: Vec<SoftVertex>,
    /// Text, drawn last.
    pub glyphs: Vec<GlyphInstance>,
    pub looks: Vec<LookInstance>,
    pub trails: Vec<TrailVertex>,
    pub bursts: Vec<Burst>,
    /// Recent hits, filled by the things' drawing parts for the modules' ones.
    pub hits: Vec<HitMark>,
    pub effects: Vec<EffectCall>,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            width: 1080,
            height: 1920,
            time: 0.0,
            background_top: [0.0, 0.0, 0.0, 1.0],
            background_bottom: [0.0, 0.0, 0.0, 1.0],
            bloom: 0.0,
            exposure: 1.0,
            shapes: Vec::new(),
            balls: Vec::new(),
            flashes: Vec::new(),
            soft: Vec::new(),
            glyphs: Vec::new(),
            looks: Vec::new(),
            trails: Vec::new(),
            bursts: Vec::new(),
            hits: Vec::new(),
            effects: Vec::new(),
        }
    }
}

impl Frame {
    /// Empties the instance lists and keeps the globals.
    pub fn clear(&mut self) {
        self.shapes.clear();
        self.balls.clear();
        self.flashes.clear();
        self.soft.clear();
        self.glyphs.clear();
        self.looks.clear();
        self.trails.clear();
        self.bursts.clear();
        self.hits.clear();
        self.effects.clear();
    }
}

/// What a thing's drawing part can read.
pub struct DrawCx<'a> {
    pub thing: &'a Thing,
    pub resolved: &'a ResolvedThing,
    /// The whole resolved scene as it is at this moment, keyframes and pulses applied.
    pub scene: &'a Resolved,
    pub world: &'a World,
    pub log: &'a Log,
    /// The moment of the run being drawn, in seconds: the clock of the world, the log and the
    /// keyframes.
    pub time: f64,
    /// Seconds into the video for this frame. The same as `time` unless the clip is beat-synced,
    /// when the run is warped onto the video; clocks, show windows and beats follow this one.
    pub video_time: f64,
}

/// How a thing kind draws itself.
pub trait ThingDraw: Send + Sync + 'static {
    /// The thing kind's id, as registered in `bpe_core`.
    fn kind(&self) -> &'static str;
    fn draw(&self, cx: &DrawCx, frame: &mut Frame);
}

/// How a module kind draws for every thing whose slot it fills: a trail behind the balls, a
/// burst where they hit, a shader over them. Runs after every thing has drawn itself.
pub trait ModuleDraw: Send + Sync + 'static {
    /// The module kind's id, as registered in `bpe_core`.
    fn kind(&self) -> &'static str;
    /// `slot` is the slot this module fills on `cx.thing`; its settings are
    /// `cx.resolved.slots[&slot].params`.
    fn draw(&self, cx: &DrawCx, slot: SlotId, frame: &mut Frame);
}

/// Every registered drawing part, by kind id.
#[derive(Default)]
pub struct Draws {
    things: BTreeMap<&'static str, Box<dyn ThingDraw>>,
    modules: BTreeMap<&'static str, Box<dyn ModuleDraw>>,
}

impl Draws {
    pub fn add(&mut self, d: impl ThingDraw) {
        let id = d.kind();
        assert!(!self.things.contains_key(id), "draw registered twice: {id}");
        self.things.insert(id, Box::new(d));
    }

    pub fn add_module(&mut self, d: impl ModuleDraw) {
        let id = d.kind();
        assert!(!self.modules.contains_key(id), "module draw registered twice: {id}");
        self.modules.insert(id, Box::new(d));
    }

    pub fn thing(&self, kind: &str) -> Option<&dyn ThingDraw> {
        self.things.get(kind).map(|d| d.as_ref())
    }

    pub fn module(&self, kind: &str) -> Option<&dyn ModuleDraw> {
        self.modules.get(kind).map(|d| d.as_ref())
    }
}

/// Fills `frame` for `time`: the lists are emptied, the scene's keyframes and pulses are
/// applied to a copy of the resolved scene for this moment, then every resolved thing in id
/// order whose kind has a drawing part draws, then every module with a drawing part draws for
/// each thing (in id order) and slot (in slot order) it fills.
pub fn build_frame(
    scene: &Scene,
    resolved: &Resolved,
    world: &World,
    log: &Log,
    time: f64,
    draws: &Draws,
    frame: &mut Frame,
) {
    build_frame_at(scene, resolved, world, log, time, time, draws, frame);
}

/// [`build_frame`] for a beat-synced clip, where the moment of the run shown (`time`) and the
/// moment of the video (`video_time`) differ: keys follow the run, beats and clocks the video.
#[allow(clippy::too_many_arguments)]
pub fn build_frame_at(
    scene: &Scene,
    resolved: &Resolved,
    world: &World,
    log: &Log,
    time: f64,
    video_time: f64,
    draws: &Draws,
    frame: &mut Frame,
) {
    frame.clear();
    let animation = Animation::new(scene, resolved);
    let animated;
    let resolved = if animation.is_empty() {
        resolved
    } else {
        let mut copy = resolved.clone();
        let inputs = Inputs {
            pulse: world.pulse_inputs.as_deref(),
            hits: hit_strength(log, time, world.steps_per_second),
            video_time: (video_time != time).then_some(video_time),
        };
        animation.apply(&mut copy, time, &inputs);
        animated = copy;
        &animated
    };
    for (id, r) in &resolved.things {
        let Some(thing) = scene.things.get(id) else { continue };
        let Some(d) = draws.thing(&r.kind) else { continue };
        d.draw(&DrawCx { thing, resolved: r, scene: resolved, world, log, time, video_time }, frame);
    }
    for (id, r) in &resolved.things {
        let Some(thing) = scene.things.get(id) else { continue };
        for slot in SlotId::ALL {
            let Some(s) = r.slots.get(&slot) else { continue };
            let Some(d) = draws.module(&s.kind) else { continue };
            d.draw(&DrawCx { thing, resolved: r, scene: resolved, world, log, time, video_time }, slot, frame);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Address, Ease, Owner, Value, resolve, testkit};
    use bpe_runtime::{Event, Log, Party, Sims, World};

    struct FakeScene;
    impl ThingDraw for FakeScene {
        fn kind(&self) -> &'static str {
            "scene"
        }
        fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
            frame.width = 100;
            frame.height = 200;
            frame.time = cx.time as f32;
            frame.bloom = 0.5;
        }
    }

    struct FakeBalls;
    impl ThingDraw for FakeBalls {
        fn kind(&self) -> &'static str {
            "balls"
        }
        fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
            let radius = match cx.resolved.params.get("radius") {
                Some(Value::Number(r)) => *r as f32,
                _ => 0.0,
            };
            frame.balls.push(BallInstance { center: [1.0, 2.0], radius, glow: 0.0, color: [1.0; 4], code: [0.0; 4] });
            frame.flashes.push(FlashInstance { center: [0.0; 2], size: 1.0, color: [1.0; 4] });
        }
    }

    #[test]
    fn build_frame_fills_from_each_thing_and_starts_clean() {
        let reg = testkit::registry();
        let mut scene = bpe_core::Scene::empty(&reg);
        let balls = testkit::push(&mut scene, "balls", bpe_core::ThingId::ROOT);
        scene.things.get_mut(&balls).unwrap().params.set("radius", Value::Number(9.0));
        testkit::push(&mut scene, "folder", bpe_core::ThingId::ROOT);
        let resolved = resolve(&scene, &reg);
        let sims = Sims::default();
        let world = World::build(&scene, &resolved, &sims);
        let mut draws = Draws::default();
        draws.add(FakeScene);
        draws.add(FakeBalls);
        let mut frame = Frame::default();
        for _ in 0..2 {
            build_frame(&scene, &resolved, &world, &Log::default(), 1.5, &draws, &mut frame);
        }
        assert_eq!((frame.width, frame.height, frame.time, frame.bloom), (100, 200, 1.5, 0.5));
        assert_eq!(frame.balls.len(), 1, "lists are cleared between frames");
        assert_eq!(frame.balls[0].radius, 9.0);
        assert_eq!(frame.flashes.len(), 1);
        assert!(frame.shapes.is_empty());
        assert!(draws.thing("folder").is_none());
    }

    #[test]
    fn a_keyed_setting_changes_the_frame_at_its_time() {
        let reg = testkit::registry();
        let mut scene = bpe_core::Scene::empty(&reg);
        let balls = testkit::push(&mut scene, "balls", bpe_core::ThingId::ROOT);
        let size = Address { owner: Owner::Thing(balls), slot: None, key: "radius".into() };
        let first = scene.set_key(&reg, &size, 0.0, Value::Number(2.0)).unwrap();
        scene.set_key(&reg, &size, 2.0, Value::Number(10.0)).unwrap();
        scene.set_ease(&size, first, Ease::Linear);
        let resolved = resolve(&scene, &reg);
        let world = World::build(&scene, &resolved, &Sims::default());
        let mut draws = Draws::default();
        draws.add(FakeBalls);
        let mut frame = Frame::default();
        let radius_at = |time: f64, frame: &mut Frame| {
            build_frame(&scene, &resolved, &world, &Log::default(), time, &draws, frame);
            frame.balls[0].radius
        };
        assert_eq!(radius_at(0.0, &mut frame), 2.0);
        assert!((radius_at(1.0, &mut frame) - 6.0).abs() < 1e-5);
        assert_eq!(radius_at(5.0, &mut frame), 10.0);
        assert_eq!(resolved.things[&balls].params.get("radius"), Some(&Value::Number(6.0)), "the input is untouched");
    }

    #[test]
    fn a_beat_synced_frame_hands_draws_both_clocks_and_pulses_on_the_video_beat() {
        struct Clock;
        impl ThingDraw for Clock {
            fn kind(&self) -> &'static str {
                "scene"
            }
            fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
                frame.time = cx.time as f32;
                frame.exposure = cx.video_time as f32;
                frame.bloom = match cx.resolved.slots[&SlotId::Look].params.get("glow") {
                    Some(Value::Number(g)) => *g as f32,
                    _ => -1.0,
                };
            }
        }
        let reg = testkit::registry();
        let mut scene = bpe_core::Scene::empty(&reg);
        let look = scene.add_module(&reg, "look.plain", "Plain").unwrap();
        scene.link(&reg, ThingId::ROOT, SlotId::Look, look).unwrap();
        let glow = Address { owner: Owner::Module(look), slot: None, key: "glow".into() };
        scene.set_pulse(&reg, &glow, bpe_core::PulseSource::Beat, Value::Number(1.0), 0.5).unwrap();
        let resolved = resolve(&scene, &reg);
        let mut world = World::build(&scene, &resolved, &Sims::default());
        world.pulse_inputs = Some(std::sync::Arc::new(bpe_core::PulseInputs { beats: vec![2.0], bands: None }));
        let mut draws = Draws::default();
        draws.add(Clock);
        let mut frame = Frame::default();
        let base = match resolved.things[&ThingId::ROOT].slots[&SlotId::Look].params.get("glow") {
            Some(Value::Number(g)) => *g as f32,
            _ => panic!("no glow"),
        };
        build_frame_at(&scene, &resolved, &world, &Log::default(), 1.0, 2.0, &draws, &mut frame);
        assert_eq!((frame.time, frame.exposure), (1.0, 2.0));
        assert!((frame.bloom - (base + 1.0)).abs() < 1e-5, "the beat is now on the video's clock: {}", frame.bloom);
        build_frame(&scene, &resolved, &world, &Log::default(), 1.0, &draws, &mut frame);
        assert_eq!((frame.time, frame.exposure), (1.0, 1.0));
        assert!((frame.bloom - base).abs() < 1e-5, "on the run's clock the beat is still to come");
    }

    #[test]
    fn hit_strength_decays_and_saturates() {
        let hit = |step: u64, speed: f32| Event {
            step,
            kind: EventKind::Hit,
            a: Party { thing: bpe_core::ThingId(2), item: 0 },
            b: None,
            point: glam::Vec2::ZERO,
            normal: glam::Vec2::X,
            speed,
        };
        let mut log = Log::default();
        log.push(hit(240, 600.0));
        assert_eq!(hit_strength(&log, 0.5, 240), 0.0, "a hit in the future does not count");
        assert!((hit_strength(&log, 1.0, 240) - 1.0).abs() < 1e-6);
        assert!((hit_strength(&log, 1.1, 240) - (-1.0f32).exp()).abs() < 1e-4);
        assert_eq!(hit_strength(&log, 1.5, 240), 0.0, "hits older than 0.3 s are gone");
        let mut many = Log::default();
        for _ in 0..3 {
            many.push(hit(240, 300.0));
        }
        assert_eq!(hit_strength(&many, 1.0, 240), 1.0, "three half hits saturate");
        assert!(hit_strength(&Log::default(), 1.0, 240) == 0.0);
    }

    #[test]
    fn soft_and_glyph_lists_are_cleared() {
        let mut frame = Frame::default();
        frame.soft.push(SoftVertex::default());
        frame.glyphs.push(GlyphInstance::default());
        frame.clear();
        assert!(frame.soft.is_empty() && frame.glyphs.is_empty());
        assert_eq!(std::mem::size_of::<SoftVertex>(), 32);
        assert_eq!(std::mem::size_of::<GlyphInstance>(), 80);
    }

    #[test]
    fn module_draws_run_after_thing_draws_for_every_slot_they_fill() {
        struct Marker;
        impl ModuleDraw for Marker {
            fn kind(&self) -> &'static str {
                "look.plain"
            }
            fn draw(&self, cx: &DrawCx, slot: SlotId, frame: &mut Frame) {
                assert_eq!(slot, SlotId::Look);
                assert_eq!(frame.balls.len(), 1, "the things drew first");
                frame.flashes.push(FlashInstance { center: [cx.thing.id.0 as f32, 0.0], ..Default::default() });
            }
        }
        let reg = testkit::registry();
        let mut scene = bpe_core::Scene::empty(&reg);
        let look = scene.add_module(&reg, "look.plain", "Plain").unwrap();
        scene.link(&reg, ThingId::ROOT, SlotId::Look, look).unwrap();
        let balls = testkit::push(&mut scene, "balls", ThingId::ROOT);
        let resolved = resolve(&scene, &reg);
        assert_eq!(resolved.things[&balls].slots[&SlotId::Look].from, ThingId::ROOT);
        let world = World::build(&scene, &resolved, &Sims::default());
        let mut draws = Draws::default();
        draws.add(FakeScene);
        draws.add(FakeBalls);
        draws.add_module(Marker);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &draws, &mut frame);
        // The balls' own flash first, then the marker's for the root and for the balls.
        let centres: Vec<f32> = frame.flashes.iter().map(|f| f.center[0]).collect();
        assert_eq!(centres, vec![0.0, 0.0, balls.0 as f32]);
        assert!(draws.module("look.plain").is_some() && draws.module("look.other").is_none());
        frame.clear();
        assert!(frame.flashes.is_empty() && frame.effects.is_empty() && frame.hits.is_empty());
    }

    #[test]
    fn a_streak_tapers_from_the_head_to_the_tail() {
        let mut out = Vec::new();
        let points = [Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0), Vec2::new(200.0, 0.0)];
        push_streak(&mut out, &points, 10.0, 1.0, [1.0, 0.5, 0.0, 1.0], 1.0);
        assert_eq!(out.len(), 12, "two segments, two triangles each");
        assert!(out.iter().any(|v| v.edge[1] == 10.0), "full width at the head");
        assert!(out.iter().any(|v| v.edge[1] == 0.0 && v.edge[0].abs() == 1.0), "a pixel's margin at the tail");
        assert!(out.iter().any(|v| v.color[3] == 0.0), "faded out at the tail");
        assert!(out.iter().all(|v| v.position[1].abs() <= 11.0), "the strip stays within reach of the line");
        out.clear();
        push_streak(&mut out, &points, 10.0, 0.5, [1.0, 0.5, 0.0, 1.0], 1.0);
        assert!(out.iter().any(|v| v.edge[1] == 5.0), "half the width at the tail with taper 0.5");
        out.clear();
        push_streak(&mut out, &points[..1], 10.0, 1.0, [1.0; 4], 1.0);
        assert!(out.is_empty());
    }

    #[test]
    fn an_additive_streak_fades_its_light_not_its_alpha() {
        let mut out = Vec::new();
        push_streak(&mut out, &[Vec2::ZERO, Vec2::X * 50.0], 4.0, 1.0, [1.0, 1.0, 1.0, -1.0], 1.0);
        assert!(out.iter().all(|v| v.color[3] == -1.0));
        assert!(out.iter().any(|v| v.color[0] == 1.0), "full light at the head");
        assert!(out.iter().any(|v| v.color[0] == 0.0), "the tail adds no light");
    }

    #[test]
    fn thing_codes_start_at_one() {
        assert_eq!(thing_code(ThingId::ROOT), 1.0);
        assert_eq!(thing_code(ThingId(41)), 42.0);
        assert_eq!(ShapeInstance::default().code(7.0).c[3], 7.0);
        assert_eq!(std::mem::size_of::<LookInstance>(), 128);
        assert_eq!(std::mem::size_of::<BallInstance>(), 48);
    }

    #[test]
    fn outlines_and_blending_follow_the_old_renderer() {
        use bpe_runtime::Outline;
        let ring =
            Outline::Ring { center: glam::Vec2::new(1.0, 2.0), radius: 50.0, sides: 6, angle: 0.3, thickness: 4.0 };
        let s = ShapeInstance::outline(&ring, [1.0, 0.5, 0.0, 1.0], 0.1);
        assert_eq!(s.a, [1.0, 2.0, 0.3, 0.0]);
        assert_eq!(s.b, [50.0, 6.0, 0.0, 0.0]);
        assert_eq!(s.c, [0.0, 2.0, 0.1, 0.0]);
        let b = Outline::Box { center: glam::Vec2::ZERO, half: glam::Vec2::new(3.0, 4.0), angle: 0.0, thickness: 2.0 };
        assert_eq!(ShapeInstance::outline(&b, [1.0; 4], 0.0).c, [1.0, 1.0, 0.0, 0.0]);
        let capsule = ShapeInstance::capsule(glam::Vec2::new(0.0, 0.0), glam::Vec2::new(4.0, 0.0), 1.5, [1.0; 4], 0.2);
        assert_eq!(capsule.a, [2.0, 0.0, 0.0, 0.0]);
        assert_eq!(capsule.b, [0.0, 0.0, 4.0, 0.0]);
        assert_eq!(capsule.c, [2.0, 1.5, 0.2, 0.0]);
        assert_eq!(blend_alpha([1.0, 0.0, 0.0, 0.0]), [1.0, 0.0, 0.0, -1.0]);
        assert_eq!(blend_alpha([0.0, 0.0, 0.0, 0.0]), [0.0; 4]);
        assert_eq!(blend_alpha([0.2, 0.0, 0.0, 1.0]), [0.2, 0.0, 0.0, 1.0]);
    }
}

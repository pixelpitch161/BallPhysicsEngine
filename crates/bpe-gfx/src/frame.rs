//! What to draw for one frame, built on the CPU by each thing's drawing part.

use std::borrow::Cow;
use std::collections::BTreeMap;

use bpe_core::{Animation, Blend, Inputs, Resolved, ResolvedThing, Scene, SlotId, Thing, ThingId};
use bpe_runtime::{CameraView, EventKind, Log, Outline, World};
use bytemuck::{Pod, Zeroable};
use glam::Vec2;

use crate::gradients::{GradientAtlas, PaintGpu};

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
    /// A gradient painted inside the lit ball in place of its colour, when its row is not -1.
    pub paint: PaintGpu,
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
    /// A gradient painted inside the fill in place of its colours, when its row is not -1.
    pub paint: PaintGpu,
}

/// A wall outline, drawn as an instanced quad with a distance function. `c.x` picks the shape:
/// 0 ring or polygon, 1 box, 2 capsule (a filled rounded line between two points).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct ShapeInstance {
    /// Centre, angle.
    pub a: [f32; 4],
    /// Ring: radius, sides. Box: half width, half height, corner radius. Capsule: half length.
    pub b: [f32; 4],
    /// Kind (0 ring or polygon, 1 box, 2 capsule), half line width, glow strength, thing code.
    pub c: [f32; 4],
    /// Linear-light colour. Alpha scales the line, not the glow. Below zero the line is added
    /// light (see [`blend_alpha`]).
    pub color: [f32; 4],
    /// A gradient painted on the line in place of its colour, when its row is not -1: an inside
    /// pattern across the shape's bounding box, or (pattern 4, Path) along the outline. The glow
    /// keeps `color`.
    pub paint: PaintGpu,
}

impl ShapeInstance {
    /// A closed wall's outline, painted with `paint` (row -1 for plain `color`).
    pub fn outline(outline: &Outline, paint: PaintGpu, color: [f32; 4], glow: f32) -> Self {
        let color = blend_alpha(color);
        match *outline {
            Outline::Ring { center, radius, sides, angle, thickness } => Self {
                a: [center.x, center.y, angle, 0.0],
                b: [radius, sides as f32, 0.0, 0.0],
                c: [0.0, thickness * 0.5, glow, 0.0],
                color,
                paint,
            },
            Outline::Box { center, half, angle, thickness } => Self {
                a: [center.x, center.y, angle, 0.0],
                b: [half.x, half.y, 0.0, 0.0],
                c: [1.0, thickness * 0.5, glow, 0.0],
                color,
                paint,
            },
            Outline::Rounded { center, half, corner, angle, thickness } => Self {
                a: [center.x, center.y, angle, 0.0],
                b: [half.x, half.y, corner, 0.0],
                c: [1.0, thickness * 0.5, glow, 0.0],
                color,
                paint,
            },
        }
    }

    /// A bar with round ends: the line from `from` to `to`, `half_width` thick each side.
    pub fn capsule(from: glam::Vec2, to: glam::Vec2, half_width: f32, color: [f32; 4], glow: f32) -> Self {
        let mid = (from + to) * 0.5;
        let along = to - from;
        Self {
            a: [mid.x, mid.y, along.to_angle(), 0.0],
            b: [along.length() * 0.5, 0.0, 0.0, 0.0],
            c: [2.0, half_width, glow, 0.0],
            color: blend_alpha(color),
            paint: PaintGpu::default(),
        }
    }

    /// A filled disc: a ring as thick as it is wide.
    pub fn disc(center: glam::Vec2, radius: f32, color: [f32; 4], glow: f32) -> Self {
        Self {
            a: [center.x, center.y, 0.0, 0.0],
            b: [radius, 0.0, 0.0, 0.0],
            c: [0.0, radius, glow, 0.0],
            color: blend_alpha(color),
            paint: PaintGpu::default(),
        }
    }

    /// Marks the shape as belonging to a thing, so an effect can paint just that thing.
    pub fn code(mut self, code: f32) -> Self {
        self.c[3] = code;
        self
    }

    /// Paints a gradient on the shape in place of its colour (see [`ShapeInstance::paint`]).
    pub fn painted(mut self, paint: PaintGpu) -> Self {
        self.paint = paint;
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

/// A liquid thing's particles, drawn as one surface. A CPU liquid sends its points every frame;
/// a GPU liquid describes itself and the renderer steps it (see [`crate::flow`]).
#[derive(Clone, Debug, PartialEq)]
pub struct LiquidLayer {
    pub thing: ThingId,
    pub points: Points,
    /// Particle radius in world pixels.
    pub radius: f32,
    /// Linear light: the whole liquid's colour when `follow` is 0.
    pub color: [f32; 4],
    /// 0: the liquid is `color`. 1: each pixel takes `paint` at its place along a direction,
    /// 2: at its distance from a centre (see `paint_place`). 3: at the mean speed of the
    /// particles under it, over `paint_place`'s span.
    pub follow: u32,
    /// The gradient the liquid is painted from when `follow` is not 0; only its row, start,
    /// steps and repeat are read.
    pub paint: PaintGpu,
    /// Where a place runs in world pixels: the direction in degrees, the span, the centre.
    pub paint_place: [f32; 4],
    /// How much of what is behind shows through, 0 to 1.
    pub clarity: f32,
    /// How much the surface bends what is behind it.
    pub refraction: f32,
    /// Strength of the shine along the surface.
    pub highlight: f32,
    /// How smooth the surface is, in particle radii.
    pub smoothing: f32,
    /// Light the liquid gives off.
    pub glow: f32,
}

/// Where a liquid's particles come from.
#[derive(Clone, Debug, PartialEq)]
pub enum Points {
    /// Positions and velocities simulated on the CPU, one per particle.
    Cpu(Vec<[f32; 4]>),
    /// A liquid the renderer simulates itself.
    Gpu(GpuLiquid),
}

/// What a GPU liquid needs to start and step itself.
#[derive(Clone, Debug, PartialEq)]
pub struct GpuLiquid {
    pub count: u32,
    /// The block the particles start in: min x, min y, max x, max y.
    pub region: [f32; 4],
    /// Seconds the pour takes; 0 starts with every particle in place.
    pub pour_seconds: f32,
    pub pour_from: [f32; 2],
    pub seed: u32,
    pub viscosity: f32,
    pub stiffness: f32,
    /// How hard balls push the liquid aside.
    pub ball_push: f32,
    pub gravity_scale: f32,
    pub bounciness: f32,
}

/// A swarm thing: thousands of glowing points that flock, stepped and drawn by the GPU.
#[derive(Clone, Debug, PartialEq)]
pub struct SwarmLayer {
    pub thing: ThingId,
    pub count: u32,
    pub start: SwarmStart,
    pub center: [f32; 2],
    pub seed: u32,
    /// Seconds over which the points are released; 0 releases them all at once.
    pub release_seconds: f32,
    pub look: SwarmLook,
    pub motion: Flock,
}

/// Where a swarm's points begin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwarmStart {
    /// Spread over the whole arena.
    Fill,
    /// Packed around the swarm's centre.
    Centre,
    /// On a ring around the centre.
    Ring,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SwarmLook {
    /// Point size in world pixels.
    pub size: f32,
    pub brightness: f32,
    /// What picks a point's place along `paint`: 0 its speed over `place`'s span, 1 the nearest
    /// ball (the ball's colour, not the paint), 2 a place at random for each point, 3 its place
    /// along a direction, 4 its distance from a centre (both from `place`). 5 paints every point
    /// `color`.
    pub follow: u32,
    /// The gradient the points take their colour from; only its row, start, steps and repeat
    /// are read.
    pub paint: PaintGpu,
    /// Where a place runs in world pixels: the direction in degrees, the span, the centre. The
    /// span is also the speed that reaches the end of the colours.
    pub place: [f32; 4],
    /// Linear light: every point's colour for follow 5, and a point's colour before it has a
    /// nearest ball.
    pub color: [f32; 4],
    /// Each point's own colour variation, worked out per point on the GPU: hue turned by up to
    /// this many degrees, saturation added and brightness scaled by up to these, and how many
    /// times a second the variation wobbles (0 holds each point's still).
    pub vary: [f32; 4],
}

/// How a swarm moves. Each field matches a setting of the Flocking module.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flock {
    pub top_speed: f32,
    pub drag: f32,
    pub flow: f32,
    pub swirl_size: f32,
    pub swirl_speed: f32,
    pub flock: f32,
    pub separation: f32,
    pub attraction: f32,
    pub reach: f32,
    pub avoid_walls: f32,
    pub gravity_scale: f32,
}

impl Default for Flock {
    fn default() -> Self {
        Self {
            top_speed: 260.0,
            drag: 0.6,
            flow: 1.0,
            swirl_size: 180.0,
            swirl_speed: 0.05,
            flock: 0.4,
            separation: 0.3,
            attraction: 0.6,
            reach: 220.0,
            avoid_walls: 1.0,
            gravity_scale: 0.0,
        }
    }
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
    /// A gradient painted across the body in place of its colour, when its row is not -1. Every
    /// corner of one body carries the same paint.
    pub paint: PaintGpu,
    /// Where the corner is in the body's bounding box, -1 to 1 each way from its centre: the
    /// space the paint's inside pattern is drawn in.
    pub local: [f32; 2],
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
    /// A gradient painted inside the fill in place of its colour, when its row is not -1.
    pub paint: PaintGpu,
    /// The world rectangle the paint's inside pattern spans, -1 to 1 across it: the glyph's own
    /// `rect` when each letter is painted on its own, or the whole text's.
    pub local: [f32; 4],
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
    streak(out, points, half_width, taper, |_| color, pixel);
}

/// [`push_streak`] with a colour for each point, `colors[i]` at `points[i]`; each fades out along
/// the streak as a single colour would. A point past the end of `colors` takes the last one.
pub fn push_streak_colours(
    out: &mut Vec<TrailVertex>,
    points: &[Vec2],
    half_width: f32,
    taper: f32,
    colors: &[[f32; 4]],
    pixel: f32,
) {
    let Some(&last) = colors.last() else { return };
    streak(out, points, half_width, taper, |i| colors.get(i).copied().unwrap_or(last), pixel);
}

fn streak(
    out: &mut Vec<TrailVertex>,
    points: &[Vec2],
    half_width: f32,
    taper: f32,
    color_at: impl Fn(usize) -> [f32; 4],
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
        let color = color_at(i);
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
    /// The gradient the particles take their colour from, for `follow` 1 and 2.
    pub paint: PaintGpu,
    /// 0: every particle is `color`. 1: each runs along `paint` over its life. 2: each takes one
    /// of `paint`'s stops at random, `paint.scale` of them evenly spaced on a Steps row.
    pub follow: u32,
    /// How much of a colour painted on the GPU (`follow` 1 and 2) shows, 0 to 1: its alpha, or
    /// for added light its light, is scaled by this. `color` comes faded already.
    pub amount: f32,
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

/// How the background pass paints: one gradient of the frame's atlas, its place measured on the
/// screen or in the world (see `Frame::background_on_screen`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BackgroundPaint {
    /// The gradient's atlas row; `u32::MAX` paints black.
    pub row: u32,
    /// How the place runs: the direction in degrees, the span, and the centre xy.
    pub place: [f32; 4],
    /// Where along the gradient the paint starts, 0 to 1 (a Time or Beat paint gives its whole
    /// place here).
    pub t0: f32,
    /// Gradients per second the place slides along.
    pub scroll: f32,
    /// 0 clamp, 1 repeat, 2 mirror.
    pub repeat: f32,
    /// 0 paints the colour at `t0` everywhere, 1 works the place out along the direction, 2
    /// outward from the centre.
    pub mode: f32,
    /// 1 picks the nearest baked colour instead of blending neighbours.
    pub steps: f32,
    /// A plain two-colour blend (linear light) the shader mixes itself, exactly, instead of
    /// reading the baked row, whose half floats are a hair off for very dark colours.
    pub ends: Option<[[f32; 4]; 2]>,
}

impl Default for BackgroundPaint {
    fn default() -> Self {
        Self {
            row: u32::MAX,
            place: [90.0, 1920.0, 0.0, 0.0],
            t0: 0.0,
            scroll: 0.0,
            repeat: 0.0,
            mode: 0.0,
            steps: 0.0,
            ends: None,
        }
    }
}

/// Everything the renderer needs for one frame: the picture's globals and the instance lists.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// The video's size in pixels; world coordinates are these pixels, origin at the centre, y
    /// down.
    pub width: u32,
    pub height: u32,
    pub time: f32,
    /// The colours behind everything, painted by the background pass.
    pub background: BackgroundPaint,
    pub bloom: f32,
    pub exposure: f32,
    pub shapes: Vec<ShapeInstance>,
    pub balls: Vec<BallInstance>,
    pub flashes: Vec<FlashInstance>,
    pub liquids: Vec<LiquidLayer>,
    pub swarms: Vec<SwarmLayer>,
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
    /// The gradients the instances paint with, one atlas row each.
    pub gradients: GradientAtlas,
    /// What the camera shows: everything above is drawn through it.
    pub camera: CameraView,
    /// The background's place is measured on the screen, still while the camera moves; off, it
    /// is measured in the world and moves with the camera.
    pub background_on_screen: bool,
    /// Things stuck to the screen, drawn after the main lists with no camera. Its own camera,
    /// background and overlay are unused.
    pub overlay: Option<Box<Frame>>,
    /// The scene's video size. Usually `width` by `height`; the editor's World view draws a
    /// picture of another shape, and the GPU layers keep stepping in the video's.
    pub video: (u32, u32),
    /// Things drawn with a blend other than Normal at full amount, one layer per blend and
    /// amount, in the order they were first drawn into. Each is mixed into the picture after
    /// this frame's own lists.
    pub layers: Vec<BlendLayer>,
    /// The colour grade run over the finished picture. Only the top frame's counts.
    pub grade: Grade,
}

/// The scene's colour grade, run in screen space on the finished, display-ready picture (after
/// the effects, bloom and tonemapping). At its defaults it changes nothing: the composite skips
/// it altogether.
#[derive(Clone, Debug, PartialEq)]
pub struct Grade {
    /// -1 to 1: the colour is multiplied by 1 plus this.
    pub brightness: f32,
    /// -1 to 1: spreads the colours from (or pulls them into) the middle grey.
    pub contrast: f32,
    /// -1 (grey) to 1 (twice as colourful).
    pub saturation: f32,
    /// -1 (cool, bluer) to 1 (warm, redder).
    pub warmth: f32,
    /// -1 (greener) to 1 (magenta).
    pub tint: f32,
    /// 0 to 1: how dark the corners go.
    pub vignette: f32,
    /// 0 to 1: how far from the middle the darkening starts.
    pub vignette_size: f32,
    /// The colours the picture's brightness is mapped to, dark to light.
    pub map: bpe_core::Gradient,
    /// 0 to 1: how much of the mapped colour replaces the picture's own.
    pub map_amount: f32,
}

impl Default for Grade {
    fn default() -> Self {
        Self {
            brightness: 0.0,
            contrast: 0.0,
            saturation: 0.0,
            warmth: 0.0,
            tint: 0.0,
            vignette: 0.0,
            vignette_size: 0.5,
            map: bpe_core::Gradient::even(&[bpe_core::Color::rgb(0, 0, 0), bpe_core::Color::WHITE]),
            map_amount: 0.0,
        }
    }
}

impl Grade {
    /// Whether the grade changes nothing: every value at its default. The map only counts once
    /// its amount is above 0, and the vignette's size once the vignette is above 0.
    pub fn is_default(&self) -> bool {
        let d = Self::default();
        self.brightness == d.brightness
            && self.contrast == d.contrast
            && self.saturation == d.saturation
            && self.warmth == d.warmth
            && self.tint == d.tint
            && self.vignette == d.vignette
            && (self.vignette == 0.0 || self.vignette_size == d.vignette_size)
            && self.map_amount == d.map_amount
    }
}

/// What things with one blend and amount drew, mixed into the picture under them as one.
#[derive(Clone, Debug, PartialEq)]
pub struct BlendLayer {
    pub blend: Blend,
    /// 0 to 1.
    pub amount: f32,
    /// Its things blend with each other by `blend` too, not only with the picture under them:
    /// the layer starts as the blend's identity, each thing is mixed into it by the blend, and
    /// the layer then counts as covering everywhere one drew. Only for a blend that
    /// [combines with itself](Blend::combines_with_itself).
    pub each: bool,
    /// The layer's own lists. Its globals follow the frame it belongs to; it paints from that
    /// frame's gradient atlas, so its own `gradients` stay empty, and the hits and effects drawn
    /// into it are handed to that frame.
    pub frame: Frame,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            width: 1080,
            height: 1920,
            time: 0.0,
            background: BackgroundPaint::default(),
            bloom: 0.0,
            exposure: 1.0,
            shapes: Vec::new(),
            balls: Vec::new(),
            flashes: Vec::new(),
            liquids: Vec::new(),
            swarms: Vec::new(),
            soft: Vec::new(),
            glyphs: Vec::new(),
            looks: Vec::new(),
            trails: Vec::new(),
            bursts: Vec::new(),
            hits: Vec::new(),
            effects: Vec::new(),
            gradients: GradientAtlas::default(),
            camera: CameraView::default(),
            background_on_screen: true,
            overlay: None,
            video: (1080, 1920),
            layers: Vec::new(),
            grade: Grade::default(),
        }
    }
}

impl Frame {
    /// Empties the instance lists and the gradients and keeps the globals.
    pub fn clear(&mut self) {
        self.shapes.clear();
        self.balls.clear();
        self.flashes.clear();
        self.liquids.clear();
        self.swarms.clear();
        self.soft.clear();
        self.glyphs.clear();
        self.looks.clear();
        self.trails.clear();
        self.bursts.clear();
        self.hits.clear();
        self.effects.clear();
        self.gradients.clear();
        self.background = BackgroundPaint::default();
        self.camera = CameraView::default();
        self.layers.clear();
        self.grade = Grade::default();
        if let Some(overlay) = &mut self.overlay {
            overlay.clear();
        }
    }

    /// The frame things drawn with `blend` at `amount` go into: this one for Normal at full
    /// amount, else the layer for exactly that blend, amount and `each` (whether its things
    /// blend with each other, kept only where the blend [combines with
    /// itself](Blend::combines_with_itself)), made on first use. The layer takes this frame's
    /// size, moment, camera and picture settings.
    pub fn layer_mut(&mut self, blend: Blend, amount: f32, each: bool) -> &mut Frame {
        if !layered(blend, amount) {
            return self;
        }
        // -0 and 0 are one amount (their bits differ).
        let amount = amount + 0.0;
        let each = each && blend.combines_with_itself();
        let same = |l: &BlendLayer| l.blend == blend && l.amount.to_bits() == amount.to_bits() && l.each == each;
        let i = match self.layers.iter().position(same) {
            Some(i) => i,
            None => {
                self.layers.push(BlendLayer { blend, amount, each, frame: Frame::default() });
                self.layers.len() - 1
            }
        };
        let (width, height, time, video, camera) = (self.width, self.height, self.time, self.video, self.camera);
        let (bloom, exposure, on_screen) = (self.bloom, self.exposure, self.background_on_screen);
        let f = &mut self.layers[i].frame;
        (f.width, f.height, f.time, f.video, f.camera) = (width, height, time, video, camera);
        (f.bloom, f.exposure, f.background_on_screen) = (bloom, exposure, on_screen);
        f
    }

    /// Whether this frame's own lists (not its layers') draw the thing with `code` (see
    /// [`thing_code`]) into the object picture: a wall, a ball or a styled ball of it.
    pub(crate) fn draws_thing(&self, code: f32) -> bool {
        let is = |c: f32| (c.floor() - code).abs() < 0.5;
        self.balls.iter().any(|b| is(b.code[0]))
            || self.looks.iter().any(|l| is(l.extent[1]))
            || self.shapes.iter().any(|s| is(s.c[3]))
    }

    /// Whether none of the lists the picture is drawn from has anything in them.
    fn draws_nothing(&self) -> bool {
        self.shapes.is_empty()
            && self.balls.is_empty()
            && self.flashes.is_empty()
            && self.liquids.is_empty()
            && self.swarms.is_empty()
            && self.soft.is_empty()
            && self.glyphs.is_empty()
            && self.looks.is_empty()
            && self.trails.is_empty()
            && self.bursts.is_empty()
            && self.layers.is_empty()
    }

    /// The frame stuck-to-screen things draw into, made on first use with this frame's size and
    /// moment.
    pub fn overlay_mut(&mut self) -> &mut Frame {
        let (width, height, time, video) = (self.width, self.height, self.time, self.video);
        let overlay = self.overlay.get_or_insert_with(Box::default);
        (overlay.width, overlay.height, overlay.time, overlay.video) = (width, height, time, video);
        overlay
    }

    /// Moves the overlay's shader effects, hit particles and hit marks into this frame: those of
    /// stuck things work on the whole picture like everyone else's, and the overlay only holds
    /// what is drawn as shapes. Particles in one of the overlay's layers go into this frame's
    /// layer of the same blend and amount, and a layer left with nothing to draw is dropped.
    pub fn lift_overlay_extras(&mut self) {
        let Some(o) = self.overlay.as_deref_mut() else { return };
        let (effects, mut bursts, hits) =
            (std::mem::take(&mut o.effects), std::mem::take(&mut o.bursts), std::mem::take(&mut o.hits));
        let mut layered: Vec<(Blend, f32, bool, Vec<Burst>)> = o
            .layers
            .iter_mut()
            .filter(|l| !l.frame.bursts.is_empty())
            .map(|l| (l.blend, l.amount, l.each, std::mem::take(&mut l.frame.bursts)))
            .collect();
        o.layers.retain(|l| !l.frame.draws_nothing());
        // A burst names its gradient by the overlay's atlas row: give it the same row here.
        for b in bursts.iter_mut().chain(layered.iter_mut().flat_map(|(_, _, _, list)| list.iter_mut())) {
            if b.paint.row >= 0.0
                && let Some(baked) = o.gradients.rows().get(b.paint.row as usize)
            {
                b.paint.row = self.gradients.adopt(baked) as f32;
            }
        }
        self.effects.extend(effects);
        self.bursts.extend(bursts);
        self.hits.extend(hits);
        for (blend, amount, each, list) in layered {
            self.layer_mut(blend, amount, each).bursts.extend(list);
        }
    }

    /// Whether the overlay has anything to draw.
    pub fn has_overlay(&self) -> bool {
        self.overlay.as_ref().is_some_and(|o| {
            !(o.shapes.is_empty()
                && o.balls.is_empty()
                && o.flashes.is_empty()
                && o.soft.is_empty()
                && o.glyphs.is_empty()
                && o.looks.is_empty()
                && o.trails.is_empty()
                && o.layers.is_empty())
        })
    }

    /// Looks at the frame through `camera` on a picture `size` world pixels across, its blend
    /// layers with it. Things stuck to the screen take the new size and keep no camera.
    pub fn set_view(&mut self, camera: CameraView, size: (u32, u32)) {
        self.camera = camera;
        (self.width, self.height) = size;
        for l in &mut self.layers {
            l.frame.set_view(camera, size);
        }
        if let Some(o) = &mut self.overlay {
            let camera = o.camera;
            o.set_view(camera, size);
        }
    }

    /// Drops balls, looks and flashes the camera can't see, so a big world draws only what is
    /// near the shot. Blend layers are culled through this frame's camera and size, whatever
    /// their own say.
    pub fn cull(&mut self) {
        let (camera, size) = (self.camera, (self.width, self.height));
        self.cull_through(camera, size);
    }

    fn cull_through(&mut self, camera: CameraView, (width, height): (u32, u32)) {
        if camera.is_identity() {
            return;
        }
        let video = Vec2::new(width as f32, height as f32);
        let sees = |c: [f32; 2], r: f32| camera.sees(video, Vec2::from(c), r + CULL_MARGIN);
        self.balls.retain(|b| sees(b.center, b.radius * 2.0));
        self.looks.retain(|l| sees(l.center, l.radius * 2.0));
        self.flashes.retain(|f| sees(f.center, f.size));
        for l in &mut self.layers {
            l.frame.cull_through(camera, (width, height));
        }
    }
}

/// Whether things drawn with `blend` at `amount` need a layer of their own.
fn layered(blend: Blend, amount: f32) -> bool {
    !(blend == Blend::Normal && amount >= 1.0)
}

/// Runs `draw` on what things with `blend` at `amount` (blending with each other when `each`)
/// draw into: `target` itself, or its layer (see [`Frame::layer_mut`]). A layer borrows
/// `target`'s gradient atlas, hits and effects while it is drawn into, so its paint rows are the
/// rows the renderer uploads and every module reads every hit. A layer nothing was drawn into is
/// not kept.
fn draw_routed(target: &mut Frame, blend: Blend, amount: f32, each: bool, draw: impl FnOnce(&mut Frame)) {
    if !layered(blend, amount) {
        draw(target);
        return;
    }
    let before = target.layers.len();
    let take =
        |f: &mut Frame| (std::mem::take(&mut f.gradients), std::mem::take(&mut f.hits), std::mem::take(&mut f.effects));
    let shared = take(target);
    let layer = target.layer_mut(blend, amount, each);
    (layer.gradients, layer.hits, layer.effects) = shared;
    draw(layer);
    let shared = take(layer);
    (target.gradients, target.hits, target.effects) = shared;
    if target.layers.len() > before && target.layers.last().is_some_and(|l| l.frame.draws_nothing()) {
        target.layers.pop();
    }
}

/// World units past the edge of the shot that are still drawn, for glows and halos.
const CULL_MARGIN: f32 = 64.0;

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
    /// Whether the module mixes Normal (at any amount) and Add in by itself, drawing straight
    /// into the frame; any other blend still goes into a layer.
    fn draws_add_itself(&self) -> bool {
        false
    }
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
/// each thing (in id order) and slot (in slot order) it fills. Things stuck to the screen draw
/// into the overlay. A thing follows its look's blend and a module its own: anything but Normal
/// at full amount goes into a layer (see [`Frame::layer_mut`]), except Normal and Add on a module
/// that [draws them itself](ModuleDraw::draws_add_itself). The root, the scene, never blends.
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
        let ((blend, amount), each) = match r.slots.get(&SlotId::Look) {
            Some(look) if *id != ThingId::ROOT => {
                (Blend::from_params(&look.params), Blend::each_from_params(&look.params))
            }
            _ => ((Blend::Normal, 1.0), false),
        };
        let target = if r.screen { frame.overlay_mut() } else { &mut *frame };
        let cx = DrawCx { thing, resolved: r, scene: resolved, world, log, time, video_time };
        draw_routed(target, blend, amount, each, |into| d.draw(&cx, into));
    }
    for (id, r) in &resolved.things {
        let Some(thing) = scene.things.get(id) else { continue };
        for slot in SlotId::ALL {
            let Some(s) = r.slots.get(&slot) else { continue };
            let Some(d) = draws.module(&s.kind) else { continue };
            let (mut blend, mut amount) = Blend::from_params(&s.params);
            if d.draws_add_itself() && matches!(blend, Blend::Normal | Blend::Add) {
                (blend, amount) = (Blend::Normal, 1.0);
            }
            let each = Blend::each_from_params(&s.params);
            let target = if r.screen { frame.overlay_mut() } else { &mut *frame };
            let cx = DrawCx { thing, resolved: r, scene: resolved, world, log, time, video_time };
            draw_routed(target, blend, amount, each, |into| d.draw(&cx, slot, into));
        }
    }
    frame.camera = world.camera;
    for l in &mut frame.layers {
        l.frame.camera = world.camera;
    }
    frame.lift_overlay_extras();
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
            frame.balls.push(BallInstance { center: [1.0, 2.0], radius, color: [1.0; 4], ..BallInstance::default() });
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
        assert_eq!(std::mem::size_of::<SoftVertex>(), 88);
        assert_eq!(std::mem::size_of::<GlyphInstance>(), 144);
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
    fn layer_mut_keeps_normal_at_full_amount_in_the_frame_and_groups_the_rest_by_mode_and_amount() {
        use bpe_core::Blend;
        let mut frame = Frame { width: 100, height: 200, time: 1.5, video: (300, 400), ..Frame::default() };
        frame.camera.zoom = 2.0;
        frame.bloom = 0.25;
        frame.layer_mut(Blend::Normal, 1.0, false).balls.push(BallInstance::default());
        assert_eq!(frame.balls.len(), 1, "Normal at full amount is the frame itself");
        assert!(frame.layers.is_empty());
        frame.layer_mut(Blend::Multiply, 1.0, false).balls.push(BallInstance::default());
        frame.layer_mut(Blend::Screen, 0.5, false).looks.push(LookInstance::default());
        frame.layer_mut(Blend::Multiply, 1.0, false).balls.push(BallInstance::default());
        frame.layer_mut(Blend::Normal, 0.5, false).shapes.push(ShapeInstance::default());
        let kept: Vec<(Blend, f32)> = frame.layers.iter().map(|l| (l.blend, l.amount)).collect();
        assert_eq!(kept, [(Blend::Multiply, 1.0), (Blend::Screen, 0.5), (Blend::Normal, 0.5)], "first-use order");
        let l = &frame.layers[0].frame;
        assert_eq!(l.balls.len(), 2, "the same mode and amount share a layer");
        assert_eq!((l.width, l.height, l.time, l.video), (100, 200, 1.5, (300, 400)));
        assert_eq!((l.camera, l.bloom), (frame.camera, 0.25));
        frame.clear();
        assert!(frame.layers.is_empty(), "clear empties the layers");
    }

    #[test]
    fn each_layers_and_plain_layers_do_not_share() {
        use bpe_core::Blend;
        let mut frame = Frame::default();
        frame.layer_mut(Blend::Screen, 1.0, true).balls.push(BallInstance::default());
        frame.layer_mut(Blend::Screen, 1.0, false).balls.push(BallInstance::default());
        frame.layer_mut(Blend::Screen, 1.0, true).balls.push(BallInstance::default());
        let kept: Vec<(Blend, f32, bool)> = frame.layers.iter().map(|l| (l.blend, l.amount, l.each)).collect();
        assert_eq!(kept, [(Blend::Screen, 1.0, true), (Blend::Screen, 1.0, false)]);
        assert_eq!(frame.layers[0].frame.balls.len(), 2);
        // A mode that cannot combine with itself drops the switch, and so shares.
        frame.layer_mut(Blend::Overlay, 1.0, true).balls.push(BallInstance::default());
        frame.layer_mut(Blend::Overlay, 1.0, false).balls.push(BallInstance::default());
        assert_eq!((frame.layers.len(), frame.layers[2].each, frame.layers[2].frame.balls.len()), (3, false, 2));
        frame.layer_mut(Blend::Normal, 1.0, true).balls.push(BallInstance::default());
        assert_eq!((frame.layers.len(), frame.balls.len()), (3, 1), "Normal at full amount is the frame itself");
    }

    #[test]
    fn a_layer_counts_towards_the_overlay_and_is_culled_with_the_frame() {
        use bpe_core::Blend;
        let mut frame = Frame::default();
        frame.overlay_mut();
        assert!(!frame.has_overlay());
        frame.overlay_mut().layer_mut(Blend::Screen, 1.0, false).balls.push(BallInstance::default());
        assert!(frame.has_overlay(), "a stuck blended thing is something to draw");
        frame.camera.zoom = 4.0;
        let layer = frame.layer_mut(Blend::Multiply, 1.0, false);
        layer.balls.push(BallInstance { center: [0.0, 0.0], radius: 5.0, ..BallInstance::default() });
        layer.balls.push(BallInstance { center: [5000.0, 0.0], radius: 5.0, ..BallInstance::default() });
        frame.cull();
        assert_eq!(frame.layers[0].frame.balls.len(), 1, "what the camera can't see is dropped in layers too");
    }

    #[test]
    fn a_layer_is_culled_through_the_frames_camera_after_the_view_changes() {
        use bpe_core::Blend;
        // Built through a camera zoomed in on the middle of the video...
        let mut frame = Frame::default();
        frame.camera.zoom = 4.0;
        let layer = frame.layer_mut(Blend::Screen, 1.0, false);
        layer.balls.push(BallInstance { center: [2000.0, 0.0], radius: 5.0, ..BallInstance::default() });
        layer.balls.push(BallInstance { center: [90_000.0, 0.0], radius: 5.0, ..BallInstance::default() });
        // ...then looked at by the editor's World view, zoomed far out, on a wider picture.
        frame.camera = CameraView { center: Vec2::ZERO, zoom: 0.1, turn: 0.0 };
        (frame.width, frame.height) = (2000, 1000);
        frame.cull();
        let kept: Vec<[f32; 2]> = frame.layers[0].frame.balls.iter().map(|b| b.center).collect();
        assert_eq!(kept, [[2000.0, 0.0]], "the layer is culled through the frame's own view");
    }

    #[test]
    fn minus_zero_and_zero_amounts_share_a_layer() {
        use bpe_core::Blend;
        let mut frame = Frame::default();
        frame.layer_mut(Blend::Screen, -0.0, false).balls.push(BallInstance::default());
        frame.layer_mut(Blend::Screen, 0.0, false).balls.push(BallInstance::default());
        assert_eq!(frame.layers.len(), 1);
        assert_eq!(frame.layers[0].amount.to_bits(), 0.0f32.to_bits());
    }

    #[test]
    fn set_view_moves_the_layers_with_the_frame() {
        use bpe_core::Blend;
        let mut frame = Frame::default();
        frame.layer_mut(Blend::Screen, 1.0, false).balls.push(BallInstance::default());
        frame.overlay_mut().layer_mut(Blend::Add, 0.5, false).balls.push(BallInstance::default());
        let camera = CameraView { center: Vec2::new(10.0, 20.0), zoom: 0.5, turn: 0.25 };
        frame.set_view(camera, (300, 200));
        let l = &frame.layers[0].frame;
        assert_eq!((l.camera, l.width, l.height), (camera, 300, 200));
        let o = frame.overlay.as_deref().unwrap();
        assert_eq!((o.width, o.height, o.camera), (300, 200, CameraView::default()), "stuck things keep no camera");
        let ol = &o.layers[0].frame;
        assert_eq!((ol.width, ol.height, ol.camera), (300, 200, CameraView::default()));
    }

    #[test]
    fn lifting_moves_a_stuck_layers_bursts_into_the_frames_layer_with_their_atlas_rows() {
        use bpe_core::{Blend, Color, Gradient};
        let mut frame = Frame::default();
        frame.gradients.row(&Gradient::solid(Color::rgb(0, 255, 0)));
        let o = frame.overlay_mut();
        o.gradients.row(&Gradient::solid(Color::rgb(255, 255, 255)));
        let red = o.gradients.row(&Gradient::solid(Color::rgb(255, 0, 0)));
        let burst = Burst {
            origin: [0.0; 2],
            normal: [0.0, -1.0],
            time: 0.0,
            color: [1.0; 4],
            count: 4,
            speed: 1.0,
            spread: 1.0,
            size: 1.0,
            life: 1.0,
            gravity: 0.0,
            drag: 0.0,
            shape: 0,
            additive: false,
            seed: 1,
            paint: PaintGpu { row: red as f32, ..PaintGpu::default() },
            follow: 1,
            amount: 1.0,
        };
        o.layer_mut(Blend::Screen, 0.5, false).bursts.push(burst);
        frame.lift_overlay_extras();
        let o = frame.overlay.as_deref().unwrap();
        assert!(o.layers.is_empty(), "a layer left with nothing to draw is dropped");
        assert!(frame.bursts.is_empty());
        assert_eq!((frame.layers.len(), frame.layers[0].blend, frame.layers[0].amount), (1, Blend::Screen, 0.5));
        let lifted = frame.layers[0].frame.bursts[0];
        let row = frame.gradients.rows()[lifted.paint.row as usize];
        assert_eq!(row[0], Color::rgb(255, 0, 0).to_linear(), "the burst paints from the frame's atlas now");
    }

    struct Painter;
    impl ThingDraw for Painter {
        fn kind(&self) -> &'static str {
            "balls"
        }
        fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
            let red = bpe_core::Gradient::solid(bpe_core::Color::rgb(255, 0, 0));
            let row = frame.gradients.row(&red);
            let paint = PaintGpu { row: row as f32, ..PaintGpu::default() };
            frame.balls.push(BallInstance { paint, ..BallInstance::default() });
            frame.hits.push(HitMark {
                thing: cx.thing.id,
                other: None,
                point: [0.0; 2],
                normal: [0.0, 1.0],
                time: 0.0,
                speed: 1.0,
                color: [1.0; 4],
            });
        }
    }

    /// A module that marks each thing it draws for with a trail vertex, and says whether it saw
    /// the thing's hit.
    struct Sees(bool);
    impl ModuleDraw for Sees {
        fn kind(&self) -> &'static str {
            "look.plain"
        }
        fn draws_add_itself(&self) -> bool {
            self.0
        }
        fn draw(&self, cx: &DrawCx, _slot: SlotId, frame: &mut Frame) {
            let seen = frame.hits.iter().any(|h| h.thing == cx.thing.id);
            frame.trails.push(TrailVertex { color: [seen as u32 as f32; 4], ..TrailVertex::default() });
        }
    }

    /// One ball group with a look slot whose blend and amount are set straight on the resolved
    /// scene, drawn by [`Painter`] and the look's module by [`Sees`].
    fn routed(blend: &str, amount: f64, screen: bool, adds_itself: bool) -> Frame {
        routed_each(blend, amount, screen, adds_itself, false)
    }

    /// [`routed`], with the look's Blend with each other switch set to `each`.
    fn routed_each(blend: &str, amount: f64, screen: bool, adds_itself: bool, each: bool) -> Frame {
        let reg = testkit::registry();
        let mut scene = bpe_core::Scene::empty(&reg);
        let look = scene.add_module(&reg, "look.plain", "Plain").unwrap();
        let balls = testkit::push(&mut scene, "balls", ThingId::ROOT);
        scene.link(&reg, balls, SlotId::Look, look).unwrap();
        let mut resolved = resolve(&scene, &reg);
        let r = resolved.things.get_mut(&balls).unwrap();
        r.screen = screen;
        let params = &mut r.slots.get_mut(&SlotId::Look).unwrap().params;
        params.set("blend", Value::Choice(blend.into()));
        params.set("blend_amount", Value::Number(amount));
        if each {
            params.set("blend_each", Value::Bool(true));
        }
        let world = World::build(&scene, &resolved, &Sims::default());
        let mut draws = Draws::default();
        draws.add(Painter);
        draws.add_module(Sees(adds_itself));
        let mut frame = Frame::default();
        frame.gradients.row(&bpe_core::Gradient::solid(bpe_core::Color::rgb(0, 255, 0)));
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &draws, &mut frame);
        frame
    }

    #[test]
    fn a_blended_thing_draws_into_a_layer_that_paints_from_the_frames_atlas() {
        use bpe_core::{Blend, Color};
        let plain = routed("normal", 1.0, false, false);
        assert!(plain.layers.is_empty());
        assert_eq!((plain.balls.len(), plain.trails.len()), (1, 1));

        let frame = routed("multiply", 1.0, false, false);
        assert!(frame.balls.is_empty() && frame.trails.is_empty());
        assert_eq!(frame.layers.len(), 1, "the thing and its module share the layer");
        let layer = &frame.layers[0];
        assert_eq!((layer.blend, layer.amount), (Blend::Multiply, 1.0));
        assert_eq!((layer.frame.balls.len(), layer.frame.trails.len()), (1, 1));
        assert!(layer.frame.gradients.is_empty(), "a layer keeps no atlas of its own");
        let row = layer.frame.balls[0].paint.row as usize;
        assert_eq!(
            frame.gradients.rows()[row][0],
            Color::rgb(255, 0, 0).to_linear(),
            "its row is in the frame's atlas"
        );
        assert_eq!(frame.hits.len(), 1, "the hit is the frame's, for every module to read");
        assert!(frame.layers[0].frame.hits.is_empty());
        assert_eq!(layer.frame.trails[0].color[0], 1.0, "a layered module still sees the thing's hits");
    }

    #[test]
    fn a_thing_blending_with_each_other_draws_with_its_look_into_an_each_layer() {
        use bpe_core::Blend;
        let frame = routed_each("multiply", 1.0, false, false, true);
        assert_eq!(frame.layers.len(), 1, "the thing and its look's module share the layer");
        let layer = &frame.layers[0];
        assert_eq!((layer.blend, layer.amount, layer.each), (Blend::Multiply, 1.0, true));
        assert_eq!((layer.frame.balls.len(), layer.frame.trails.len()), (1, 1));
        assert!(!routed("multiply", 1.0, false, false).layers[0].each, "off by default");
    }

    #[test]
    fn a_module_that_adds_itself_keeps_normal_and_add_in_the_frame() {
        use bpe_core::Blend;
        for (blend, amount) in [("normal", 0.5), ("add", 0.25), ("add", 1.0)] {
            let frame = routed(blend, amount, false, true);
            assert_eq!(frame.trails.len(), 1, "{blend} {amount}: the module draws in the frame");
            assert_eq!(frame.layers.len(), 1, "{blend} {amount}: the thing itself is layered");
            assert!(frame.layers[0].frame.trails.is_empty());
        }
        let frame = routed("screen", 1.0, false, true);
        assert!(frame.trails.is_empty());
        assert_eq!((frame.layers.len(), frame.layers[0].blend), (1, Blend::Screen));
        assert_eq!(frame.layers[0].frame.trails.len(), 1);
    }

    #[test]
    fn a_stuck_blended_thing_draws_into_the_overlays_layer() {
        let frame = routed("screen", 0.5, true, false);
        assert!(frame.layers.is_empty());
        let o = frame.overlay.as_deref().unwrap();
        assert!(o.balls.is_empty());
        assert_eq!((o.layers.len(), o.layers[0].frame.balls.len()), (1, 1));
        assert!(frame.has_overlay());
        let row = o.layers[0].frame.balls[0].paint.row as usize;
        assert!(row < o.gradients.len(), "its row is in the overlay's atlas");
    }

    #[test]
    fn a_layer_nothing_was_drawn_into_is_dropped() {
        struct Nothing;
        impl ThingDraw for Nothing {
            fn kind(&self) -> &'static str {
                "balls"
            }
            fn draw(&self, _: &DrawCx, frame: &mut Frame) {
                frame.gradients.row(&bpe_core::Gradient::solid(bpe_core::Color::rgb(1, 2, 3)));
            }
        }
        let reg = testkit::registry();
        let mut scene = bpe_core::Scene::empty(&reg);
        let look = scene.add_module(&reg, "look.plain", "Plain").unwrap();
        let balls = testkit::push(&mut scene, "balls", ThingId::ROOT);
        scene.link(&reg, balls, SlotId::Look, look).unwrap();
        let mut resolved = resolve(&scene, &reg);
        let params = &mut resolved.things.get_mut(&balls).unwrap().slots.get_mut(&SlotId::Look).unwrap().params;
        params.set("blend", Value::Choice("darken".into()));
        let world = World::build(&scene, &resolved, &Sims::default());
        let mut draws = Draws::default();
        draws.add(Nothing);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &draws, &mut frame);
        assert!(frame.layers.is_empty());
        assert_eq!(frame.gradients.len(), 1, "what it baked is still in the frame's atlas");
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
        assert_eq!(std::mem::size_of::<LookInstance>(), 176);
        assert_eq!(std::mem::size_of::<BallInstance>(), 96);
    }

    #[test]
    fn outlines_and_blending_follow_the_old_renderer() {
        use bpe_runtime::Outline;
        let ring =
            Outline::Ring { center: glam::Vec2::new(1.0, 2.0), radius: 50.0, sides: 6, angle: 0.3, thickness: 4.0 };
        let s = ShapeInstance::outline(&ring, PaintGpu::default(), [1.0, 0.5, 0.0, 1.0], 0.1);
        assert_eq!(s.a, [1.0, 2.0, 0.3, 0.0]);
        assert_eq!(s.b, [50.0, 6.0, 0.0, 0.0]);
        assert_eq!(s.c, [0.0, 2.0, 0.1, 0.0]);
        let b = Outline::Box { center: glam::Vec2::ZERO, half: glam::Vec2::new(3.0, 4.0), angle: 0.0, thickness: 2.0 };
        assert_eq!(ShapeInstance::outline(&b, PaintGpu::default(), [1.0; 4], 0.0).c, [1.0, 1.0, 0.0, 0.0]);
        let r = Outline::Rounded {
            center: glam::Vec2::ZERO,
            half: glam::Vec2::new(30.0, 40.0),
            corner: 10.0,
            angle: 0.1,
            thickness: 2.0,
        };
        let s = ShapeInstance::outline(&r, PaintGpu::default(), [1.0; 4], 0.0);
        assert_eq!((s.c[0], s.b), (1.0, [30.0, 40.0, 10.0, 0.0]));
        let cap = ShapeInstance::capsule(glam::Vec2::new(-10.0, 0.0), glam::Vec2::new(10.0, 0.0), 3.0, [1.0; 4], 0.2);
        assert_eq!((cap.c[0], cap.c[1], cap.c[2], cap.b[0]), (2.0, 3.0, 0.2, 10.0));
        assert_eq!(cap.a, [0.0, 0.0, 0.0, 0.0]);
        let up = ShapeInstance::capsule(glam::Vec2::ZERO, glam::Vec2::new(0.0, 4.0), 1.0, [1.0; 4], 0.0);
        assert!((up.a[2] - std::f32::consts::FRAC_PI_2).abs() < 1e-6 && up.a[1] == 2.0);
        let disc = ShapeInstance::disc(glam::Vec2::new(1.0, 2.0), 7.0, [1.0; 4], 0.0);
        assert_eq!((disc.c[0], disc.c[1], disc.b[0], disc.a[0]), (0.0, 7.0, 7.0, 1.0));
        assert_eq!(blend_alpha([1.0, 0.0, 0.0, 0.0]), [1.0, 0.0, 0.0, -1.0]);
        assert_eq!(blend_alpha([0.0, 0.0, 0.0, 0.0]), [0.0; 4]);
        assert_eq!(blend_alpha([0.2, 0.0, 0.0, 1.0]), [0.2, 0.0, 0.0, 1.0]);
    }
}

#[cfg(test)]
mod flow_tests {
    use super::*;
    use bpe_core::ThingId;

    #[test]
    fn clearing_a_frame_empties_its_flow_layers_too() {
        let mut frame = Frame::default();
        frame.liquids.push(LiquidLayer {
            thing: ThingId(1),
            points: Points::Cpu(vec![[0.0; 4]]),
            radius: 3.0,
            color: [0.0, 0.5, 1.0, 1.0],
            follow: 0,
            paint: PaintGpu::default(),
            paint_place: [0.0; 4],
            clarity: 0.45,
            refraction: 0.5,
            highlight: 0.8,
            smoothing: 6.0,
            glow: 0.1,
        });
        frame.swarms.push(SwarmLayer {
            thing: ThingId(2),
            count: 10,
            start: SwarmStart::Ring,
            center: [0.0; 2],
            seed: 1,
            release_seconds: 0.0,
            look: SwarmLook {
                size: 1.0,
                brightness: 0.2,
                follow: 0,
                paint: PaintGpu::default(),
                place: [0.0; 4],
                color: [1.0; 4],
                vary: [0.0; 4],
            },
            motion: Flock::default(),
        });
        let copy = frame.clone();
        assert_eq!(copy, frame);
        frame.clear();
        assert!(frame.liquids.is_empty() && frame.swarms.is_empty());
        assert_eq!(frame, Frame::default());
    }
}

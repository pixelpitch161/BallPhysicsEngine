//! What to draw for one frame, built on the CPU by each thing's drawing part.

use std::borrow::Cow;
use std::collections::BTreeMap;

use bpe_core::{Animation, Blend, Inputs, Resolved, ResolvedThing, Scene, SlotId, Thing, ThingId};
use bpe_runtime::{CameraView, LevelsAt, LightAt, LightLevelsAt, Log, SoundAt, World};
use bytemuck::{Pod, Zeroable};
use glam::Vec2;

use crate::gradients::{GradientAtlas, PaintGpu};

pub use bpe_runtime::hit_strength;

/// One layer of a ball's stack of layers, as layers.wgsl draws it. A thing's stack is written
/// once per frame into [`Frame::look_layers`]; each of its balls is a [`LayeredInstance`] naming
/// the range of records it is drawn with. Bottom first.
///
/// | code | layer   | `amount` (x is always opacity) | `p0`                                   | `p1`              | flags                    |
/// | ---- | ------- | ------------------------------ | -------------------------------------- | ----------------- | ------------------------ |
/// | 0    | body    | 1                              | -                                      | -                 | -                        |
/// | 1    | pattern | opacity                        | -                                      | -                 | 1 turns with the ball    |
/// | 2    | outline | opacity                        | width px, sits (0 in, 1 centre, 2 out) | -                 | 1 colour from the body   |
/// | 3    | shine   | opacity, strength              | spot xy, size                          | -                 | -                        |
/// | 4    | glow    | opacity, strength              | size in radii                          | -                 | 1 colour from the body   |
/// | 5    | shading | opacity                        | kind (0 linear, 1 radial, 2 brush), softness, space (0 ball, 1 screen), amount | linear: from xy, to xy; radial: centre xy, size; brush: tile in w | 2 radial darkens the middle |
/// | 6    | shader  | opacity                        | speed, scale, space, effect slot       | -                 | -                        |
/// | 8    | group   | -                              | -                                      | -                 | -                        |
/// | 9    | group shader | amount                    | speed, scale, 1 over the whole ball, effect slot | around in radii (0 inside only) | - |
///
/// The group record (code 8) mixes the ball's group colour (`LayeredInstance::group`) over the
/// whole ball as it stands there, by the mode in `range.w`. Flag 8 ([`LayerGpu::GROUPED`]) on any
/// other record mixes the group colour into that layer's own colour first.
///
/// A group shader record (code 9) runs an effect across the whole group: its uv is the ball's
/// point in the square of `LayeredInstance::place`. Over the whole ball (`p0.z` 1) it is drawn as
/// a layer of its own, by its blend; otherwise it changes the layers that name it: a drawn
/// record's `amount.z` is the first group record's index and `amount.w` a bit mask of the group
/// records after it (bit 0 that first one) whose effect is mixed into its colour. Group shaders
/// only run in a stack's in-ball program.
///
/// `head.y` is the layer's blend against the layers under it in the same ball
/// ([`Blend::shader_code`]); `head.w` how many sub-layer records follow it (they change it and
/// are skipped as layers). `colour` is the layer's own linear colour; `paint` what a pattern
/// paints (its variation comes from the ball's instance); `extra` a shader layer's effect
/// settings. A cut's target is a record index within the ball's range; a brush tile indexes
/// [`Frame::mask_tiles`] (-1 none).
///
/// Sub-layer records (codes 20..29) follow their layer's record, in order, on balls and shapes
/// alike (sub_layers.wgsl). Sizes are in the walk's unit: ball radii, or a shape's world pixels;
/// outline widths are pixels on both. Flag 1 on Glow and Outline takes the layer's own colour.
///
/// | code | sub-layer     | `amount`                                    | `p0`                                                   | `p1`                               |
/// | ---- | ------------- | ------------------------------------------- | ------------------------------------------------------ | ---------------------------------- |
/// | 20   | cut           | -, target record (-1 none), parent angle, shape | from (0 layer below, 1 layer, 2 shape, 3 brush, 4 light, 5 dark), invert, softness, moves (0 layer, 1 ball, 2 screen) | angle rad, size, threshold, tile |
/// | 21   | glow          | opacity, strength                           | size, where (0 outside, 1 inside, 2 both)              | -                                  |
/// | 22   | outline       | opacity                                     | width px, sits (0 in, 1 centre, 2 out)                 | -                                  |
/// | 23   | shadow        | opacity                                     | offset xy, softness, moves (0 screen, 1 ball)          | -                                  |
/// | 24   | shading       | as a shading layer's (5)                    |                                                        |                                    |
/// | 25   | shine         | as a shine layer's (3)                      |                                                        |                                    |
/// | 26   | shader        | as a shader layer's (6)                     |                                                        |                                    |
/// | 27   | recolour      | opacity, amount                             | mode (0 tint, 1 hue, 2 gradient, 3 light), turn in turns, brightness, contrast | saturation |
/// | 28   | flash on hit  | strength, length s, 1 follows impact        | -                                                      | -                                  |
/// | 29   | colour on hit | turn per hit in turns                       | -                                                      | -                                  |
///
/// A recolour's `colour` is its tint and `paint` its gradient (always a row); a shadow's or
/// flash's `colour` is its colour.
///
/// A shape ([`PathInstance::layers`]) names records of its own, walked by shape_walk.wgsl, the
/// Body's first. Lengths are world pixels; `lo`, `hi` are where Across starts and ends along
/// `dir`, in shape units; runs are 0 across, 1 from the middle (across the band of a hollow
/// shape), 2 along the edge, 3 across the line (outline) or out from the edge (glow):
///
/// | code | layer               | `amount`           | `p0`                                         | `p1`           | flag 1                   |
/// | ---- | ------------------- | ------------------ | -------------------------------------------- | -------------- | ------------------------ |
/// | 0    | body                | opacity            | -                                            | -              | -                        |
/// | 10   | colour and gradient | opacity            | runs                                         | dir xy, lo, hi | -                        |
/// | 2    | outline             | opacity            | width, sits (0 in, 1 centre, 2 out), runs    | dir xy, lo, hi | the colour under it      |
/// | 4    | glow                | opacity, strength  | size, -, runs                                | dir xy, lo, hi | the colour under it      |
/// | 6    | shader              | opacity            | speed, scale, space (0 shape, 1 screen), effect slot | -      | -                        |
///
/// A shape record's `colour` is its own (one) colour and `paint` its gradient, read straight
/// along its run (`paint.row` -1 for one colour).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct LayerGpu {
    /// Kind code, blend code, flags, spare.
    pub head: [u32; 4],
    pub amount: [f32; 4],
    pub p0: [f32; 4],
    pub p1: [f32; 4],
    pub colour: [f32; 4],
    pub paint: PaintGpu,
    pub extra: [[f32; 4]; 4],
    /// How the layer takes the scene's lights (read only when the scene is lit): its Lit (0 as
    /// today, 1 fully lit), how much it blocks light, and on a ball's body record its Bulge.
    pub light: [f32; 4],
    /// The light it gives off (its Gives off light sub-layers, read only in the emission pass):
    /// `[0]` own colour times strength (rgb, linear) and the flare on hits (w); `[1]` what its own
    /// colours are multiplied by (rgb) and the flare's length in seconds (w). All 0 gives none.
    pub emit: [[f32; 4]; 2],
    /// Lighting part 4 (read only when the scene is lit): how bright it glows under UV light (0
    /// not at all), 1 when it shows only under UV light, how much it catches the light in the air
    /// (smoke), and its thing's link number (Lights up).
    pub more: [f32; 4],
}

impl LayerGpu {
    /// All zeros, for binding something when a frame has no records.
    pub const ZERO: LayerGpu = LayerGpu {
        head: [0; 4],
        amount: [0.0; 4],
        p0: [0.0; 4],
        p1: [0.0; 4],
        colour: [0.0; 4],
        paint: PaintGpu {
            row: -1.0,
            t0: 0.0,
            pattern: 0.0,
            grain: 0.0,
            angle: 0.0,
            scale: 1.0,
            scroll: 0.0,
            repeat: 0.0,
            vary: [0.0, 0.0, 1.0, 0.0],
            form: [0.0; 4],
            place: [0.0; 4],
        },
        extra: [[0.0; 4]; 4],
        light: [0.0; 4],
        emit: [[0.0; 4]; 2],
        more: [0.0; 4],
    };
    pub const BODY: u32 = 0;
    pub const PATTERN: u32 = 1;
    pub const OUTLINE: u32 = 2;
    pub const SHINE: u32 = 3;
    pub const GLOW: u32 = 4;
    pub const SHADING: u32 = 5;
    pub const SHADER: u32 = 6;
    /// Sub-layer records (see the table above).
    pub const SUB_CUT: u32 = 20;
    pub const SUB_GLOW: u32 = 21;
    pub const SUB_OUTLINE: u32 = 22;
    pub const SUB_SHADOW: u32 = 23;
    pub const SUB_SHADING: u32 = 24;
    pub const SUB_SHINE: u32 = 25;
    pub const SUB_SHADER: u32 = 26;
    pub const SUB_RECOLOUR: u32 = 27;
    pub const SUB_FLASH: u32 = 28;
    pub const SUB_HUE: u32 = 29;
    /// A shape's Colour and gradient layer (shape records only, see below).
    pub const SHAPE_PAINT: u32 = 10;
    /// A radial shading darkens the middle instead of the outside.
    pub const INVERT: u32 = 2;
    /// The editor's view shows this layer's cuts alone: white where they let it show.
    pub const SOLO: u32 = 4;
    /// The group's colour over the whole ball (see the table above).
    pub const GROUP: u32 = 8;
    /// An effect across the whole group (see the table above).
    pub const GROUP_SHADER: u32 = 9;
    /// The pattern turns with the ball's heading, or the outline or glow takes the body's colour.
    pub const FLAG: u32 = 1;
    /// The group's colour is mixed into this layer's colour.
    pub const GROUPED: u32 = 8;
    /// This ball doesn't wear the layer (odd ones out): it is passed over as if it weren't in
    /// the stack, though it keeps its place so record indices hold.
    pub const UNWORN: u32 = 16;
    /// Glass and light part 1: `head[2]` carries the thing's (or a layer's own Surface's) Tints
    /// light in bits 16..23 and Deeper when thick in bits 24..31, each in 64ths (see
    /// [`LayerGpu::tint_bits`]); the flags above stay in the low bits.
    pub const TINT_SHIFT: u32 = 16;
    pub const DEEPER_SHIFT: u32 = 24;
    pub const TINT_BITS: u32 = 0xffff_0000;

    /// Tints light and Deeper when thick packed for `head[2]`: each rounded to 64ths, 0 to 255.
    pub fn tint_bits(tints: f32, deeper: f32) -> u32 {
        let pack = |v: f32| if v.is_finite() { (v * 64.0).round().clamp(0.0, 255.0) as u32 } else { 0 };
        (pack(tints) << Self::TINT_SHIFT) | (pack(deeper) << Self::DEEPER_SHIFT)
    }
}

/// The most layers one ball is drawn with; the rest of a longer stack is left out.
pub const MAX_LAYERS: u32 = 128;

/// The hit data of a ball or shape never hit: seconds since its last hit (a long time), that
/// hit's strength, how many hits, spare.
pub const NO_HIT: [f32; 4] = [1e6, 0.0, 0.0, 0.0];

/// One ball drawn with a stack of layers (see [`LayerGpu`]).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct LayeredInstance {
    pub center: [f32; 2],
    pub radius: f32,
    /// 0 to 1, the ball's own number.
    pub seed: f32,
    /// The body's colour after the reactions, linear light; alpha -1 adds its light (see
    /// [`blend_alpha`]).
    pub body: [f32; 4],
    /// Direction of travel, scale along it, scale across it.
    pub stretch: [f32; 4],
    /// Shape code, roundness, angle in radians, how far the stack reaches past the centre in
    /// radii (the quad's half size before stretching).
    pub form: [f32; 4],
    /// Opacity, thing code (see [`thing_code`]), the ball's heading in degrees for patterns that
    /// turn with it, 1 when the shape faces its motion (the heading is then already turned in).
    pub misc: [f32; 4],
    /// The ball's own colour variation for patterns painted from their own gradient.
    pub vary: [f32; 4],
    /// First record in the frame's `look_layers`, how many, spare, and how the group colour mixes
    /// in: 0 not at all, else one more than the blend's [`Blend::shader_code`] (1 replaces).
    pub range: [u32; 4],
    /// The ball's colour from its group (linear, straight), and how much of it mixes in.
    pub group: [f32; 4],
    /// Where the ball started in its group, from the group's middle (world units), and the
    /// group's side (world units): the square group shaders spread across.
    pub place: [f32; 4],
    /// Seconds since the ball's last hit, that hit's strength (speed over a full flash's, at
    /// most 1.5), how many hits it has had, spare: what Flash on hit and Colour on hit sub-layers
    /// read. [`NO_HIT`] for a ball never hit.
    pub hit: [f32; 4],
    /// The clip a piece of a shattered ball draws through ([`PathInstance::clip`]), its corners
    /// in the quad's own frame (`local`, radii, before the shape's turn).
    pub clip: [f32; 4],
}

impl Default for LayeredInstance {
    fn default() -> Self {
        Self {
            center: [0.0; 2],
            radius: 0.0,
            seed: 0.0,
            body: [0.0; 4],
            stretch: [0.0; 4],
            form: [0.0; 4],
            misc: [0.0; 4],
            vary: [0.0; 4],
            range: [0; 4],
            group: [0.0; 4],
            place: [0.0; 4],
            hit: NO_HIT,
            clip: [0.0; 4],
        }
    }
}

/// `Frame::path_points[k].z` of a letter contour's first point ([`Frame::push_letter`]): its `w`
/// is the contour's point count less one, so the point that closes it is `k + w`.
pub const CONTOUR_START: f32 = 2.0;
/// `Frame::path_points[k].z` of a letter contour's last point: `w` back is its first.
pub const CONTOUR_END: f32 = 1.0;
/// `PathInstance.b[1]` of a fast letter, whose distance comes from a tile ([`Frame::push_glyph`]).
pub const PATH_GLYPH: f32 = 3.0;

/// A Shape: a closed outline, a circle or a list of points (`Frame::path_points`) in the shape's
/// own units, filled or drawn as a band, with a one-pixel soft edge at any zoom. Points are placed
/// by turning them by `a.z`, scaling them by `a.w` and moving them to `a.xy`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct PathInstance {
    /// Centre x, y, angle (radians clockwise), scale (world pixels per shape unit).
    pub a: [f32; 4],
    /// Half the band's thickness in world pixels (0 = filled), kind (0 circle, 1 points, 2
    /// ellipse, 3 a fast letter read from a tile), the first point in `Frame::path_points` and how
    /// many points (an ellipse: its half-axes in shape units; a fast letter: its tile record in
    /// `path_points` and its tile in `Frame::mask_tiles`).
    pub b: [f32; 4],
    /// Thing code, how far the outline reaches from the centre (shape units), how far its layers
    /// reach past its outline (world pixels: an outline outside it, a glow), and how far the
    /// outline reaches from the middle for layers running from the middle (shape units).
    pub c: [f32; 4],
    /// Linear-light colour, when it draws plain (no records).
    pub color: [f32; 4],
    /// Its layers: the first record in `Frame::look_layers`, how many (0 draws it plain in
    /// `color`), the in-ball program its Shader layers run in (an index into `Frame::programs`,
    /// [`PathInstance::NO_PROGRAM`] for none; read on the CPU only), spare. See the shape records
    /// table on [`LayerGpu`].
    pub layers: [u32; 4],
    /// Hits on the shape, as [`LayeredInstance::hit`].
    pub hit: [f32; 4],
    /// What a broken thing's look draws through (rules part 3): the first corner of a convex clip
    /// in `Frame::path_points` (in the shape's own units), how many (0: no clip), how far it has
    /// faded (0 shows it whole, 1 not at all), spare. All 0 draws it as ever.
    pub clip: [f32; 4],
}

impl Default for PathInstance {
    fn default() -> Self {
        Self { a: [0.0; 4], b: [0.0; 4], c: [0.0; 4], color: [0.0; 4], layers: [0; 4], hit: NO_HIT, clip: [0.0; 4] }
    }
}

impl PathInstance {
    /// `layers[2]` of a shape with no program: its Shader layers, if any, are left out.
    pub const NO_PROGRAM: u32 = u32::MAX;

    /// Whether it draws a filled shape rather than a band.
    pub fn solid(&self) -> bool {
        self.b[0] <= 0.0
    }

    /// How far it reaches from its centre, in world pixels, its layers included.
    pub fn reach(&self) -> f32 {
        self.c[1] * self.a[3] + self.b[0] + self.c[2].max(0.0)
    }

    /// How its colours are changed (Text part 5), from `clip[3]`.
    pub fn style(&self) -> PathStyle {
        PathStyle::decode(self.clip[3])
    }

    /// Changes its colours as `style` says (into `clip[3]`; [`PathStyle::NONE`] draws as ever).
    pub fn set_style(&mut self, style: PathStyle) {
        self.clip[3] = style.encode();
    }
}

/// Which colour channel a styled instance draws (Text part 5's Colour split): see [`PathStyle`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PathChannel {
    /// Its whole colour, as ever.
    #[default]
    All,
    /// Its red alone, added as light (covering nothing).
    RedLight,
    /// Its green alone, covering what is behind as the whole colour would.
    GreenCovering,
    /// Its blue alone, added as light.
    BlueLight,
}

impl PathChannel {
    fn code(self) -> u32 {
        match self {
            PathChannel::All => 0,
            PathChannel::RedLight => 1,
            PathChannel::GreenCovering => 2,
            PathChannel::BlueLight => 3,
        }
    }

    /// Whether it adds light rather than covering (it must draw after the covering copy).
    pub fn adds_light(self) -> bool {
        matches!(self, PathChannel::RedLight | PathChannel::BlueLight)
    }
}

/// How a shape instance's colours are changed after its layers are worked out (Text part 5:
/// Echo's and Mirror's colour turns, Colour split's and Glitch's channels). Carried in
/// `PathInstance.clip[3]` as `channel + 4·hue_q + 4096·colourise_q` (hue in 1024ths of a turn,
/// colourise in 15ths), exact in an f32; 0 changes nothing. `paths.wgsl::path_styled` reads it: in
/// the colour passes the hue turns and the colour moves toward the pure hue, in the picture passes
/// a channel copy draws only its channel (the red and blue copies adding light, so a green copy
/// drawn first and red and blue added after give the whole colour back), and in the light passes
/// that read shapes as blockers the red and blue copies draw nothing and the green one draws whole.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PathStyle {
    pub channel: PathChannel,
    /// Turns round the colour wheel, 0 to 1.
    pub hue: f32,
    /// How far its colours move toward the pure hue at their brightness, 0 to 1.
    pub colourise: f32,
}

impl PathStyle {
    pub const NONE: PathStyle = PathStyle { channel: PathChannel::All, hue: 0.0, colourise: 0.0 };

    pub fn encode(self) -> f32 {
        let hue = if self.hue.is_finite() { self.hue.rem_euclid(1.0) } else { 0.0 };
        let hue_q = ((hue * 1024.0).round() as u32) % 1024;
        let colourise = if self.colourise.is_finite() { self.colourise.clamp(0.0, 1.0) } else { 0.0 };
        let colourise_q = (colourise * 15.0).round() as u32;
        (self.channel.code() + 4 * hue_q + 4096 * colourise_q) as f32
    }

    pub fn decode(v: f32) -> PathStyle {
        if !v.is_finite() || v < 0.5 {
            return PathStyle::NONE;
        }
        let v = (v.round() as u32).min(65535);
        let channel = match v % 4 {
            1 => PathChannel::RedLight,
            2 => PathChannel::GreenCovering,
            3 => PathChannel::BlueLight,
            _ => PathChannel::All,
        };
        PathStyle { channel, hue: ((v / 4) % 1024) as f32 / 1024.0, colourise: (v / 4096) as f32 / 15.0 }
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

/// One corner of a trail strip.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct TrailVertex {
    pub position: [f32; 2],
    /// Signed distance from the centre line, and the half width at this point.
    pub edge: [f32; 2],
    /// Linear light; alpha -1 is additive (the light itself fades along the streak).
    pub color: [f32; 4],
    /// How soft its edges are and how much of it shows (see [`StreakStyle`]).
    pub style: [f32; 2],
    /// Its layer's Lit and how much it blocks light (read only when the scene is lit).
    pub light: [f32; 2],
    /// The light it gives off, straight linear rgb at full cover (read only in the emission
    /// pass); w spare.
    pub emit: [f32; 4],
    /// Lighting part 4, as [`LayerGpu::more`]: UV glow, only under UV, catches beams, link number.
    pub extra: [f32; 4],
}

/// How a streak's edges look: its own, a soft glow, or only a band along its edge.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StreakStyle {
    /// 0 keeps a crisp one-pixel edge; 1 is soft from the centre line out to twice the width.
    pub softness: f32,
    /// 0 fills it; otherwise only the outer `band` share of its half width shows (an outline).
    pub band: f32,
}

/// Appends a strip through `points` (newest first) as a triangle list, two triangles per
/// segment, `half_widths[i]` wide and coloured `colors[i]` at `points[i]` (a point past the end
/// of either takes the last one), drawn as `style` says. Nothing fades or narrows here: the
/// caller has done that. `pixel` is a pixel in world units, the anti-aliasing margin each edge
/// keeps. Fewer than two points draw nothing.
pub fn push_trail(
    out: &mut Vec<TrailVertex>,
    points: &[Vec2],
    half_widths: &[f32],
    colors: &[[f32; 4]],
    style: StreakStyle,
    pixel: f32,
) {
    let (Some(&w), Some(&c)) = (half_widths.last(), colors.last()) else { return };
    strip(
        out,
        points,
        |i| half_widths.get(i).copied().unwrap_or(w),
        |i| colors.get(i).copied().unwrap_or(c),
        style,
        pixel,
    );
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
    let taper = taper.clamp(0.0, 1.0);
    let along = |i: usize| i as f32 / (n - 1) as f32;
    let width_at = |i: usize| half_width * (1.0 - along(i) * taper);
    let faded = |i: usize| {
        let fade = (1.0 - along(i)) * (1.0 - along(i));
        let color = color_at(i);
        if color[3] < 0.0 {
            [color[0] * fade, color[1] * fade, color[2] * fade, -1.0]
        } else {
            [color[0], color[1], color[2], color[3] * fade]
        }
    };
    strip(out, points, width_at, faded, StreakStyle::default(), pixel);
}

/// The strip under [`push_trail`] and [`push_streak`]: each point `width_at` wide and `color_at`.
fn strip(
    out: &mut Vec<TrailVertex>,
    points: &[Vec2],
    width_at: impl Fn(usize) -> f32,
    color_at: impl Fn(usize) -> [f32; 4],
    style: StreakStyle,
    pixel: f32,
) {
    let n = points.len();
    if n < 2 {
        return;
    }
    let (softness, band) = (style.softness.clamp(0.0, 1.0), style.band.clamp(0.0, 1.0));
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
    let vertex = |i: usize, side: Vec2, sign: f32| {
        let width = width_at(i).max(0.0);
        // A soft edge reaches out past the width.
        let reach = width * (1.0 + softness) + pixel;
        TrailVertex {
            position: (points[i] + side * reach * sign).into(),
            edge: [reach * sign, width],
            color: color_at(i),
            style: [softness, band],
            light: [0.0; 2],
            emit: [0.0; 4],
            extra: [0.0; 4],
        }
    };
    // Sharp corners (bounces) get a join: both segments meet at the inner mitre point (so the
    // strip doesn't cover the inside of the corner twice) and the outside is filled round (so it
    // has no notch). Each join triangle's edge values are true distances from one segment's
    // centre line, so the shader's antialiasing and soft edges carry on across it.
    let corners: Vec<Option<Corner>> = (0..n)
        .map(|i| {
            if i == 0 || i + 1 >= n {
                return None;
            }
            let (d1, d2) = (direction(i - 1)?, direction(i)?);
            if d1.dot(d2) > 0.5 {
                return None;
            }
            let (n1, n2) = (d1.perp(), d2.perp());
            let s = if d2.dot(n1) > 0.0 { 1.0 } else { -1.0 };
            let reach = width_at(i).max(0.0) * (1.0 + softness) + pixel;
            let denom = 1.0 + n1.dot(n2);
            let m = (n1 + n2) * (s * reach / denom.max(1e-6));
            let (before, after) = (points[i].distance(points[i - 1]), points[i + 1].distance(points[i]));
            let mitre =
                (denom > 1e-3 && -m.dot(d1) <= before * 0.999 && m.dot(d2) <= after * 0.999).then_some(points[i] + m);
            Some(Corner { inner: s, mitre, n1, n2, reach })
        })
        .collect();
    let joined = |i: usize, v: TrailVertex, sign: f32| match corners[i] {
        Some(Corner { inner, mitre: Some(m), .. }) if sign == inner => TrailVertex { position: m.into(), ..v },
        _ => v,
    };
    for i in 0..n - 1 {
        let Some(own) = direction(i) else { continue };
        let (s0, s1) = (side_at(i, own), side_at(i + 1, own));
        let (a, b) = (joined(i, vertex(i, s0, 1.0), 1.0), joined(i, vertex(i, s0, -1.0), -1.0));
        let (c, d) = (joined(i + 1, vertex(i + 1, s1, 1.0), 1.0), joined(i + 1, vertex(i + 1, s1, -1.0), -1.0));
        out.extend_from_slice(&[a, b, c, b, d, c]);
    }
    for (i, corner) in corners.iter().enumerate() {
        let Some(k) = corner else { continue };
        let p = points[i];
        let base = vertex(i, k.n1, 1.0);
        let at = |position: Vec2, x: f32| TrailVertex { position: position.into(), edge: [x, base.edge[1]], ..base };
        let (u0, u1) = (-k.n1 * k.inner, -k.n2 * k.inner);
        let middle = at(p, 0.0);
        if let Some(m) = k.mitre {
            // The two triangles between each segment's own outer corner, the middle and the mitre.
            out.extend_from_slice(&[at(p + u0 * k.reach, -k.reach), middle, at(m, k.reach)]);
            out.extend_from_slice(&[at(p + u1 * k.reach, -k.reach), middle, at(m, k.reach)]);
        }
        // A fan round the outside, always turning the inside's way (on an exact reversal the turn's
        // sign is the sign of zero), at most 30 degrees a step (within 0.6 px of round at 20 px
        // wide). The same count whatever the width, so a trail's sub-layers match it vertex for
        // vertex, and few, so a bouncy crowd keeps within the trails' budget.
        let turn = k.inner * u0.angle_to(u1).abs();
        let steps = ((turn.abs() / 0.53).ceil() as usize).clamp(1, 6);
        let mut last = u0;
        for s in 1..=steps {
            let next = Vec2::from_angle(turn * s as f32 / steps as f32).rotate(u0);
            out.extend_from_slice(&[middle, at(p + last * k.reach, k.reach), at(p + next * k.reach, k.reach)]);
            last = next;
        }
    }
}

/// A sharp corner of a strip (see [`strip`]): which side is inside (+1 or -1 times the side
/// vectors), where the two inner edges meet (when that falls within both segments), the two
/// segments' side vectors and how far the strip reaches out there.
#[derive(Clone, Copy)]
struct Corner {
    inner: f32,
    mitre: Option<Vec2>,
    n1: Vec2,
    n2: Vec2,
    reach: f32,
}

/// One particle (or a round bit of a trail) as the GPU draws it: the CPU has worked out where it
/// is and how it looks. Matches `SpriteIn` in sprites.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct Particle {
    /// Centre (world pixels), turn (radians), half size (pixels; a ring's radius).
    pub place: [f32; 4],
    /// Linear light. A negative alpha adds `|alpha|` of the light and covers nothing.
    pub color: [f32; 4],
    /// Half length along the turn (sparks, streaks, confetti, shards), shade (1 as it is),
    /// softness 0 to 1 (see [`StreakStyle`]), band 0 to 1 (0 filled, else only the outer share
    /// of its size shows).
    pub style: [f32; 4],
    /// Its shape: 0 sparks, 1 smoke, 2 confetti, 3 stars, 4 shards, 5 rings, 6 dots, 7 hearts,
    /// 8 bubbles, 9 streaks; three spare.
    pub shape: [u32; 4],
    /// Its layer's Lit, how much it blocks light and how much it bends light (its index of
    /// refraction, glass and light part 3: above 1 it is free glass), read only when the scene is
    /// lit; one spare.
    pub light: [f32; 4],
    /// The light it gives off, straight linear rgb at full cover (read only in the emission
    /// pass); w spare.
    pub emit: [f32; 4],
    /// Lighting part 4, as [`LayerGpu::more`]: UV glow, only under UV, catches beams, link number.
    pub extra: [f32; 4],
}

impl Particle {
    /// A particle of `shape` at `at`, turned `angle` radians, `size` pixels (half size), in
    /// `color`, shaded 1, crisp and filled.
    pub fn new(shape: u32, at: Vec2, angle: f32, size: f32, color: [f32; 4]) -> Self {
        Self {
            place: [at.x, at.y, angle, size],
            color,
            style: [size, 1.0, 0.0, 0.0],
            shape: [shape, 0, 0, 0],
            light: [0.0; 4],
            emit: [0.0; 4],
            extra: [0.0; 4],
        }
    }

    /// Where it is.
    pub fn at(&self) -> Vec2 {
        Vec2::new(self.place[0], self.place[1])
    }
}

/// A field's look: smoke, shaded by its density.
pub const FIELD_SMOKE: f32 = 0.0;
/// A field's look: flame, shaded through its heat colours.
pub const FIELD_FLAME: f32 = 1.0;
/// How many stops a field's heat colours are baked into.
pub const FIELD_HEAT_STOPS: usize = 32;

/// One soft blob a [`Field`] is splatted from. Matches `SplatIn` in fields.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct Splat {
    /// Centre (world pixels), radius (world pixels), weight (how much it adds where it is thickest).
    pub place: [f32; 4],
    /// Its colour (straight, linear light); w spare.
    pub color: [f32; 4],
}

/// How a [`Field`] is shaded where it is drawn. Matches `FieldLook` in fields.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct FieldLook {
    /// Mode ([`FIELD_SMOKE`], [`FIELD_FLAME`]), adds (1: adds its light and covers nothing),
    /// amount (0 to 1, times what it shows), strength (smoke's Thickness, flame's Heat).
    pub kind: [f32; 4],
    /// Noise detail: amount 0 to 1, size (world pixels), drift x, drift y (world pixels a second).
    pub detail: [f32; 4],
    /// Smoke: shading, the way the light comes from (radians), dark inside, relief (world pixels
    /// it stands up where thickest: above 0, in a lit scene, the real lights shade it instead of
    /// its own light, W15). Flame: bend (world pixels), flicker 0 to 1, flicker speed (per
    /// second), brightness.
    pub shade: [f32; 4],
    /// As [`Particle::light`], [`Particle::emit`] and [`Particle::extra`] (lighting).
    pub light: [f32; 4],
    pub emit: [f32; 4],
    pub extra: [f32; 4],
    /// Flame's heat colours from cold to hot (straight, linear, alpha), evenly spaced.
    pub heat: [[f32; 4]; FIELD_HEAT_STOPS],
}

impl Default for FieldLook {
    fn default() -> Self {
        Self {
            kind: [FIELD_SMOKE, 0.0, 1.0, 1.0],
            detail: [0.0, 60.0, 0.0, 0.0],
            shade: [0.0; 4],
            light: [0.0; 4],
            emit: [0.0; 4],
            extra: [0.0; 4],
            heat: [[1.0; 4]; FIELD_HEAT_STOPS],
        }
    }
}

/// Particles drawn as one soft field (Soft smoke, Flame): the GPU splats them into a picture at
/// half size, then shades it where the field sits in its list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Field {
    /// How many sprites of the same list (`particles` for `fields`, `particles_front` for
    /// `fields_front`) are drawn before it.
    pub at: u32,
    pub splats: Vec<Splat>,
    pub look: FieldLook,
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
    /// How much of the effect shows (its Mix): 1 all of it, 0 none (skipped); past 1 pushes it
    /// further, below 0 the other way.
    pub blend: f32,
    /// 0 for the whole picture, else the [`thing_code`] of the one thing it paints.
    pub apply_to: f32,
    pub stage: Stage,
    /// How the effect's result mixes onto what it took (Normal replaces it).
    pub mode: Blend,
    /// What of the picture the effect works on (read at the Picture stage on the main picture).
    pub takes: Takes,
    /// For [`Takes::Glow`]: the brightness above which the picture counts as glow.
    pub glow_above: f32,
    /// How many seconds of earlier frames the effect remembers (0 for none), for the editor's
    /// warm-up after a seek.
    pub memory: f32,
}

impl EffectCall {
    /// A call of `wgsl` over the whole picture at `stage`, fully mixed in, Normal, taking the
    /// whole picture, its params all 0.
    pub fn new(key: u64, wgsl: impl Into<Cow<'static, str>>, stage: Stage) -> Self {
        Self {
            key,
            wgsl: wgsl.into(),
            params: [0.0; 16],
            blend: 1.0,
            apply_to: 0.0,
            stage,
            mode: Blend::Normal,
            takes: Takes::Whole,
            glow_above: 0.9,
            memory: 0.0,
        }
    }
}

/// What part of the picture an effect works on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Takes {
    /// The whole picture.
    #[default]
    Whole,
    /// The light that falls on lit things (needs a Lighting card; else the effect changes
    /// nothing).
    Light,
    /// Only what is brighter than the call's `glow_above`.
    Glow,
}

impl Takes {
    /// The shader's code: 0 whole, 1 light, 2 glow.
    pub fn code(self) -> f32 {
        match self {
            Takes::Whole => 0.0,
            Takes::Light => 1.0,
            Takes::Glow => 2.0,
        }
    }
}

/// One live ball, as the picture effects see it (their ball list).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BallMark {
    /// Centre, world pixels.
    pub center: [f32; 2],
    pub radius: f32,
    /// Its body colour, linear light.
    pub color: [f32; 3],
    /// 0 to 1, the same in every frame.
    pub seed: f32,
    /// Seconds since its last hit (a large number when never hit).
    pub hit_age: f32,
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
    /// outward from the centre, 3 by the form `pattern`.
    pub mode: f32,
    /// The gradient's grain, in 1/255 steps (see [`crate::PaintGpu::grain`]).
    pub grain: f32,
    /// A plain two-colour blend (linear light) the shader mixes itself, exactly, instead of
    /// reading the baked row, whose half floats are a hair off for very dark colours.
    pub ends: Option<[[f32; 4]; 2]>,
    /// With `mode` 3, the form laid over the place (`PaintGpu::pattern`): the place measured from
    /// the centre in spans, turned by the direction.
    pub pattern: f32,
    /// The form's own numbers (`PaintGpu::form`).
    pub form: [f32; 4],
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
            grain: 0.0,
            ends: None,
            pattern: 0.0,
            form: [0.0; 4],
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
    /// Shapes, and the points (xy, two spare) their outlines run through.
    pub paths: Vec<PathInstance>,
    pub path_points: Vec<[f32; 4]>,
    pub flashes: Vec<FlashInstance>,
    /// Balls drawn with a stack of layers, and the stacks' records they name.
    pub layered: Vec<LayeredInstance>,
    pub look_layers: Vec<LayerGpu>,
    /// The brush tiles the records name (`MASK_TILE`² bytes each) with a hash to tell them apart.
    pub mask_tiles: Vec<(u64, std::sync::Arc<[u8]>)>,
    /// The in-ball programs stacks with shader layers draw with, and which instances of
    /// `layered` draw with which (in order). No runs draws every instance with the plain
    /// pipeline.
    pub programs: Vec<crate::LookProgram>,
    pub layered_runs: Vec<crate::LayeredRun>,
    pub trails: Vec<TrailVertex>,
    /// Particles and trail bits behind the balls, drawn right after `trails`.
    pub particles: Vec<Particle>,
    /// Trails and particles in front of the balls, drawn after them (and after the effects on
    /// things), trails first.
    pub trails_front: Vec<TrailVertex>,
    pub particles_front: Vec<Particle>,
    /// Soft fields of particles behind the balls and in front of them, each drawn among the
    /// sprites of `particles` / `particles_front` where its `at` says.
    pub fields: Vec<Field>,
    pub fields_front: Vec<Field>,
    /// Huge swarms (particles part 8) behind the balls and in front of them, each drawn from its
    /// particle buffers among the sprites of `particles` / `particles_front` where its `at` says.
    pub swarms: Vec<crate::SwarmRun>,
    pub swarms_front: Vec<crate::SwarmRun>,
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
    /// The scene's lights, when it has a Lighting card (only the top frame's counts). `None`
    /// draws the picture exactly as it was before lighting.
    pub light: Option<LightScene>,
    /// Every live ball, for the picture effects' ball list (filled only when the scene has an
    /// Effects card). Blend layers' frames hold the balls drawn into them.
    pub ball_marks: Vec<BallMark>,
    /// Whether some paint in the frame lays its colours out as Ripples: things then push their
    /// [`RippleSource`]s and the renderer builds the distance field Ripples read. Blend layers'
    /// frames take it from the frame they are made in.
    pub ripples: bool,
    /// What Ripples measure from (balls and shape outlines), in this frame and its layers' frames.
    pub ripple_sources: Vec<RippleSource>,
    /// The last owner number given out by [`Frame::ripple_owner`].
    pub ripple_owners: u32,
    /// The beat at this moment: its phase 0 to 1, the bar's phase 0 to 1, beats per second, and
    /// 1 when the scene has a tempo (else all 0). Only the top frame's counts.
    pub beat: [f32; 4],
    /// Which sample of a baked frame this is (lighting part 3), `None` drawn live.
    pub bake: Option<crate::bake::BakeSample>,
    /// Liquids (particles part 6): particles run together into a surface, each drawn behind the
    /// balls (after the trails and sprites behind them) or in front of them (after the flashes).
    pub liquids: Vec<crate::liquid::Liquid>,
}

/// One quad of a Light beam's band (glass and light part 2, `RibbonGpu` in the lighting shader): the
/// stretch between segment `a` of one ray and segment `b` of the next, added over the lit picture.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct RibbonGpu {
    /// One ray's segment, from xy to zw (world).
    pub a: [f32; 4],
    /// The next ray's segment, from xy to zw (world).
    pub b: [f32; 4],
    /// Its colour (linear, times strength and glow) and the band's edge softness (0 to 1).
    pub c: [f32; 4],
    /// Where across the band each of the two rays is (0 to 1), how far apart they were at the
    /// lamp (world), spare.
    pub d: [f32; 4],
}

/// One light as the lighting pass reads it (`LightGpu` in the lighting shader).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct LightGpu {
    /// Place xy (world), height above the picture (a sun's: sine of its elevation), kind (0 point,
    /// 1 spot, 2 sun, 3 area, 4 strip).
    pub p0: [f32; 4],
    /// Colour times strength, linear; w spare.
    pub p1: [f32; 4],
    /// Reach, falloff (0 real, 1 soft, 2 linear, 3 none), size, shadow strength.
    pub p2: [f32; 4],
    /// Direction it points (unit xy), cosine of the cone's inner and outer edge.
    pub p3: [f32; 4],
    /// Width, length (area), first segment and how many (strip).
    pub p4: [f32; 4],
    /// Shadow colour (linear rgb), 1 when it casts shadows.
    pub p5: [f32; 4],
    /// How far short of the lamp shadows stop, the sun's size (tan of its radius), area shape (0
    /// rectangle, 1 disc), the light's own shadow softness.
    pub p6: [f32; 4],
    /// How much its light shows in the scene's haze (Beam in haze), 1 when it is ultraviolet, the
    /// share of an ultraviolet light that is ordinary light, a laser's glow.
    pub p7: [f32; 4],
    /// Its gobo: kind (0 none, 1 stripes, 2 dots, 3 window, 4 leaves, 5 water caustics, 6 rings,
    /// 7 clouds), size (world units), sharpness (0 soft to 1 crisp), depth (how dark its shade).
    pub p8: [f32; 4],
    /// The gobo's turn (cos, sin) and how far it has drifted (world units, in its own frame).
    pub p9: [f32; 4],
    /// Lights up: mode (0 everything, 1 only these, 2 everything but these), the first link
    /// number in [`LightScene::links`], how many, 1 when it lights the background too.
    pub p10: [f32; 4],
    /// A laser's width (world units) and how bright it glows where it hits; two spare.
    pub p11: [f32; 4],
}

impl LightGpu {
    pub const ZERO: LightGpu = LightGpu {
        p0: [0.0; 4],
        p1: [0.0; 4],
        p2: [0.0; 4],
        p3: [0.0; 4],
        p4: [0.0; 4],
        p5: [0.0; 4],
        p6: [0.0; 4],
        p7: [0.0; 4],
        p8: [0.0; 4],
        p9: [0.0; 4],
        p10: [0.0; 4],
        p11: [0.0; 4],
    };
}

/// The scene's Lighting card as the renderer reads it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LightingGpu {
    /// Ambient light (linear rgb times its strength), and how much the background takes light.
    pub ambient: [f32; 4],
    /// Shadow style (0 soft, 1 hard, 2 toon, 3 off), softness, length, contact shading.
    pub shadows: [f32; 4],
    /// Contact reach, toon steps, quality (0 draft .. 3 ultra), 1 when light through see-through
    /// things takes their colour (Coloured light through glass, glass and light part 1).
    pub more: [f32; 4],
    /// Bounce light, glow reach (world units), Light blooms, spare.
    pub glow: [f32; 4],
    /// The haze: its colour times its amount (linear rgb), how much it dims what is behind it.
    pub air: [f32; 4],
    /// The background's own light (Gives off light): `[0]` own colour times strength, `[1]` what
    /// the background's colours are multiplied by (rgb, linear; w spare).
    pub background: [[f32; 4]; 2],
    /// Part 4, the haze moving: how (0 still, 1 drifting, 2 swirling), speed, size (world units),
    /// patchiness (0 even, 1 in clouds).
    pub haze: [f32; 4],
    /// How far it has drifted (xy, world units: the clouds are read that far back), how much
    /// balls stir it, spare.
    pub haze_more: [f32; 4],
    /// Underwater: how much (0 not), caustics, caustic size (world units), caustic speed.
    pub water: [f32; 4],
    /// The water's colour (linear rgb), sun rays.
    pub water_colour: [f32; 4],
    /// Where the water starts (world y, down is deeper), how deep its colour takes over, spare.
    pub water_depth: [f32; 4],
    /// Glow in the dark: how bright (0 none), fade time (s), charge speed, what remembers (0 the
    /// background, 1 everything).
    pub memory: [f32; 4],
    /// The glow's own colour (linear rgb), 1 to glow in the light's own colour instead.
    pub memory_colour: [f32; 4],
    /// Stage (lighting part 2a): 1 in Stage (0 Flat), the back wall's depth, the shapes' front
    /// (z, toward the camera), k = tan(looking down) (0 with no floor).
    pub stage: [f32; 4],
    /// The floor: kind (0 none, 1 at the world y in `y`, 2 at the picture's bottom), its y, its
    /// surface (0 matte, 1 glossy, 2 mirror), the wall's surface (0 matte, 1 glossy).
    pub floor: [f32; 4],
    /// The floor's blur and reflection; two spare.
    pub floor_more: [f32; 4],
    /// The sky's top colour (linear rgb) and Sky light.
    pub sky_top: [f32; 4],
    /// The sky's bottom colour (linear rgb); w spare.
    pub sky_bottom: [f32; 4],
    /// Depth of field (part 2b, Stage): on (1/0), focus depth (z), blur (px of blur a px of depth
    /// away gives), and Reflection depth (how many mirror bounces a reflection follows).
    pub dof: [f32; 4],
    /// The bake (part 3): samples a pixel, light bounces, bake reflections, shutter (0 without
    /// motion blur).
    pub bake: [f32; 4],
    /// 1 when the video bakes, 1 with noise clean-up, glass caustics, 1 when the bake traces bounced light.
    pub bake_more: [f32; 4],
    /// Glass and light part 3: Bent light rays (a multiplier on the photons), Bent light softness
    /// (how wide each photon's line is splatted), part 4a's Mirror light (how much the light
    /// mirrors throw is), one spare.
    pub bent: [f32; 4],
    /// Glass and light part 4b, rainbows in rain: Rainbow (0 none), Rainbow size (world units: the
    /// 42° bow's radius), Second bow, Dark band.
    pub rainbow: [f32; 4],
    /// Where the bow's middle is (world xy) and 1 when set by hand (else the renderer puts it
    /// opposite the brightest sun); one spare.
    pub rainbow_at: [f32; 4],
}

/// How a surface takes light (`SurfaceGpu` in the lighting shader): an entry of
/// [`LightScene::surfaces`], named by the surface index every record of a thing carries in
/// [`LayerGpu::light`]`[3]`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct SurfaceGpu {
    /// Roughness, metal, clear coat, see-through.
    pub a: [f32; 4],
    /// F0 (how much it reflects looked at straight on), toon steps (0 smooth), what it mirrors (0
    /// nothing, 1 the sky, 2 the scene), kind ([`SurfaceGpu::SHEET`], `BALL`, `SHAPE`).
    pub b: [f32; 4],
    /// Part 2b: rainbow split, soak, film, flakes.
    pub c: [f32; 4],
    /// Flake size (of a ball's radius), brushing, brush lines (0 spun round the middle, 1 straight),
    /// brush angle (radians).
    pub d: [f32; 4],
    /// Frost, rim, inner glow, bends light (index of refraction).
    pub e: [f32; 4],
    /// Glass and light part 4a: throws light, facets (0 smooth), facet gaps, 0.
    pub f: [f32; 4],
    /// Part 4b: Dichroic, Dichroic shift and Dichroic hue (both in turns), Fire.
    pub g: [f32; 4],
}

impl SurfaceGpu {
    /// Trails, particles and anything else with no Surface: matte and flat.
    pub const SHEET: f32 = 0.0;
    pub const BALL: f32 = 1.0;
    pub const SHAPE: f32 = 2.0;
    /// A plain sheet: matte, reflecting nothing (index 0 of every table).
    pub const MATTE: SurfaceGpu = SurfaceGpu {
        a: [1.0, 0.0, 0.0, 0.0],
        b: [0.0; 4],
        c: [0.0; 4],
        d: [0.0; 4],
        e: [0.0, 0.0, 0.0, 1.0],
        f: [0.0; 4],
        g: [0.0; 4],
    };
    /// Added to a record's surface index (`LayerGpu::light[3]`) when the surface is see-through:
    /// the behind picture leaves such records out (see [`LightScene::refracts`]). Every other
    /// reader takes the index modulo this (and the glass flag modulo [`SurfaceGpu::OWN_FLAG`]).
    pub const GLASS_FLAG: f32 = 4096.0;
    /// Added to a layer's record when the layer wears a surface of its own (a Surface sub-layer):
    /// it then covers the surface picture inside its thing even when it doesn't block light.
    pub const OWN_FLAG: f32 = 8192.0;
}

/// A shape whose outline is a mirror (part 2b's Mirror walls; `MirrorGpu` in the lighting shader): its
/// outline is `count` segments of [`LightScene::segments`] from `first`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct MirrorGpu {
    /// First segment, how many, which side shows the mirror (1 outside, 2 inside, 3 both),
    /// strength.
    pub a: [f32; 4],
    /// Squeeze, the shape's half-size (world), half its band's thickness (world), 0.
    pub b: [f32; 4],
    /// The mirror's tint (linear rgb); w spare.
    pub c: [f32; 4],
}

/// A ball as a sphere on the stage (`SphereGpu` in the lighting shader).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct SphereGpu {
    /// Centre xy (world, on the picture plane), radius, see-through.
    pub a: [f32; 4],
    /// The colour of the light it lets through (linear rgb), and its Deeper when thick (glass and
    /// light part 1).
    pub b: [f32; 4],
    /// Its surface index (part 2b: what a reflection of it takes light as), its Tints light (glass
    /// and light part 1), its turn (radians, glass and light part 4a: a mirror ball's tiles turn
    /// with it), its thing's z-order (balls with a higher one, or the same and listed later, are
    /// drawn over it).
    pub c: [f32; 4],
}

impl LightingGpu {
    /// Whether the background gives off any light.
    pub fn background_glows(&self) -> bool {
        self.background.iter().any(|e| e[..3].iter().any(|c| *c != 0.0))
    }

    /// Whether light through see-through things takes their colour (Coloured light through glass).
    pub fn glass_colour(&self) -> bool {
        self.more[3] > 0.5
    }

    /// Whether the haze moves (drifts or swirls): the extras pass then works out its density.
    pub fn haze_moves(&self) -> bool {
        self.haze[0] > 0.5 && self.air[..3].iter().any(|c| *c != 0.0)
    }

    /// Whether the picture remembers light (Glow in the dark).
    pub fn remembers(&self) -> bool {
        self.memory[0] != 0.0
    }
}

/// A lit frame's lights: the Lighting card's settings, every light, and the segments strip
/// lights run along (xy to zw, world).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LightScene {
    pub settings: LightingGpu,
    pub lights: Vec<LightGpu>,
    pub segments: Vec<[f32; 4]>,
    /// What the light pass shows: the lit picture, or (the editor's views) the light alone.
    pub show: LightShow,
    /// Whether any thing drawn gives off light (a shown Gives off light sub-layer): the frame is
    /// then drawn once more as the light things give off, for the light field.
    pub glows: bool,
    /// Whether any layer glows under UV light (part 4): with an ultraviolet light the frame is
    /// drawn once more as what glows under UV.
    pub uv: bool,
    /// Whether any layer catches the light in the air (smoke).
    pub smoke: bool,
    /// Whether any layer makes rainbows (rain, glass and light part 4b).
    pub rain: bool,
    /// The light probe's scene (glass and light part 4b): lasers pass dichroic glass as plain glass,
    /// so the probe's light stays as before dichroic glass.
    pub plain_glass: bool,
    /// The things lights light up or leave out (Lights up), by link number; each light names a
    /// run of them (`LightGpu::p10`).
    pub links: Vec<f32>,
    /// What stirs moving haze: each ball's place and velocity (world), then its radius.
    pub stirs: Vec<[[f32; 4]; 2]>,
    /// The scene's own link number (the background's, for Lights up).
    pub background_link: f32,
    /// How each surface takes light, by surface index; 0 (and any index never set) is
    /// [`SurfaceGpu::MATTE`].
    pub surfaces: Vec<SurfaceGpu>,
    /// Every ball as a sphere: shadows and contact shading in Stage, reflections in both.
    pub spheres: Vec<SphereGpu>,
    /// The Camera thing's Stage camera, when it is on (lighting part 5): the lit picture is seen
    /// again from it. Only read in Stage.
    pub camera: Option<crate::StageCamera>,
    /// Shapes whose outline is a mirror (part 2b), at most `MOST_MIRRORS`.
    pub mirrors: Vec<MirrorGpu>,
    /// Whether something drawn is see-through (part 2b): the frame is then drawn once more without
    /// it, the picture glass bends.
    pub refracts: bool,
    /// Light beams' bands (glass and light part 2), added over the lit picture after the mix.
    pub ribbons: Vec<RibbonGpu>,
    /// Glass and light part 3: the shapes (and shape layers) whose glass bends lamp light, by
    /// surface index. Free glass (bending particles, glass liquid) is found in the frame itself
    /// ([`Frame::free_glass_box`]).
    pub benders: Vec<Bender>,
    /// Glass and light part 4a: the shapes (and shape layers) and ball things whose surface throws
    /// lamp light back onto the room like a mirror, at most [`MOST_THROWERS`].
    pub throwers: Vec<Thrower>,
    /// Glass and light part 4a: shapes that carry light along their outline (light pipes), at most
    /// [`MOST_PIPES`]; [`LightScene::light_pipes`] turns them into strip lights.
    pub pipes: Vec<Pipe>,
}

/// The most things that throw light in a frame (the rest throw none).
pub const MOST_THROWERS: usize = 64;
/// The most light pipes in a frame (the rest carry none).
pub const MOST_PIPES: usize = 16;

/// A thing whose surface throws lamp light back like a mirror (glass and light part 4a): its box
/// (world), its surface index, what it gives back of each channel of the light meeting it (already
/// times Throws light), how fast a rough surface blurs what it throws (Roughness squared: the
/// thrown light's half width grows by this much a unit of path), its Facets (0 smooth), its Facet
/// gaps, and whether it is balls (whose normals come from their spheres).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Thrower {
    pub lo: [f32; 2],
    pub hi: [f32; 2],
    pub index: u32,
    pub colour: [f32; 3],
    pub widen: f32,
    pub facets: f32,
    pub gaps: f32,
    pub ball: bool,
}

/// A shape that carries light along its outline (glass and light part 4a's light pipes): its
/// closed outline (world), how far its light carries (world), how brightly it glows, its colour's
/// hue (linear rgb, its brightest channel 1), its index of refraction (corners sharper than its
/// critical angle let light out), and the width its glow is drawn with (world).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pipe {
    pub points: Vec<Vec2>,
    pub carries: f32,
    pub glow: f32,
    pub tint: [f32; 3],
    pub bends: f32,
    pub width: f32,
}

/// The most mirror walls a frame takes (the rest are left out).
pub const MOST_MIRRORS: usize = 16;

/// The bender id of free glass (glass and light part 3): bending particles and glass liquid, which
/// mark the surface picture with their index of refraction instead of a surface index.
pub const FREE_BENDER: u32 = 4096;

/// Glass that bends lamp light in a lit frame (glass and light part 3): its box (world, smallest
/// and largest corner) and its surface index ([`FREE_BENDER`] for free glass). The photon pass
/// sends each light's photons toward its box.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bender {
    pub lo: [f32; 2],
    pub hi: [f32; 2],
    pub index: u32,
}

impl Bender {
    /// A box round `points` (world), grown by `margin` on every side; `None` with no finite point.
    pub fn round(points: impl IntoIterator<Item = Vec2>, margin: f32, index: u32) -> Option<Bender> {
        let mut lo = Vec2::splat(f32::INFINITY);
        let mut hi = Vec2::splat(f32::NEG_INFINITY);
        for p in points.into_iter().filter(|p| p.is_finite()) {
            lo = lo.min(p);
            hi = hi.max(p);
        }
        (lo.x <= hi.x && lo.y <= hi.y).then(|| Bender {
            lo: (lo - Vec2::splat(margin)).to_array(),
            hi: (hi + Vec2::splat(margin)).to_array(),
            index,
        })
    }

    /// The smallest box holding both.
    pub fn union(self, other: Bender) -> Bender {
        Bender {
            lo: [self.lo[0].min(other.lo[0]), self.lo[1].min(other.lo[1])],
            hi: [self.hi[0].max(other.hi[0]), self.hi[1].max(other.hi[1])],
            index: self.index,
        }
    }
}

/// The least Bends light that bends (glass and light part 3): 1 is straight through.
pub const BENDS_FROM: f32 = 1.0001;

/// A trail's or particle's `extra[2]` (glass and light part 4b): Catches beams alone, exactly, or,
/// when the layer makes rainbows (`makes` above 0), `−(round(makes × 255) + 256 × round(catches ×
/// 64))`, a whole number sprites.wgsl's and scene.wgsl's `tag_catch`/`tag_rain` read back.
pub fn pack_tags(catches: f32, makes: f32) -> f32 {
    let catches = if catches.is_finite() { catches.max(0.0) } else { 0.0 };
    let m = if makes.is_finite() { (makes.clamp(0.0, 1.0) * 255.0).round() } else { 0.0 };
    if m < 1.0 {
        return catches;
    }
    -(m + 256.0 * (catches * 64.0).round().min(65535.0))
}

/// What [`pack_tags`] packed: Catches beams and Makes rainbows.
pub fn tag_parts(z: f32) -> (f32, f32) {
    if z >= 0.0 || !z.is_finite() {
        return (z.max(0.0), 0.0);
    }
    let v = -z;
    let hi = (v / 256.0).floor();
    (hi / 64.0, (v - 256.0 * hi) / 255.0)
}

impl LightScene {
    /// Whether a light is ultraviolet.
    pub fn has_uv_light(&self) -> bool {
        self.lights.iter().any(|l| l.p7[1] > 0.5)
    }

    /// Whether the frame needs its tags picture (smoke, Lights up): drawn in light pass 6.
    pub fn needs_tags(&self) -> bool {
        self.smoke || self.lights.iter().any(|l| l.p10[0] > 0.5) || self.rainbows_in_rain()
    }

    /// Whether `frame` is drawn once more as the tint picture (light pass 8, glass and light part
    /// 1): light through see-through things takes their colour, something lights the scene and
    /// something drawn is see-through.
    pub fn needs_tint(&self, frame: &Frame) -> bool {
        self.settings.glass_colour() && (!self.lights.is_empty() || self.glows) && frame.see_through_blockers()
    }

    /// Whether rain shows a rainbow: Rainbow is on and something makes rainbows (the tags
    /// picture's B).
    pub fn rainbows_in_rain(&self) -> bool {
        self.rain && self.settings.rainbow[0] > 0.0
    }

    /// Whether the frame needs its UV picture: drawn in light pass 5.
    pub fn needs_uv(&self) -> bool {
        self.uv && self.has_uv_light()
    }

    /// Adds a thing that throws light, unless the frame has [`MOST_THROWERS`] already.
    pub fn push_thrower(&mut self, thrower: Thrower) {
        if self.throwers.len() < MOST_THROWERS {
            self.throwers.push(thrower);
        }
    }

    /// Adds a light pipe, unless the frame has [`MOST_PIPES`] already.
    pub fn push_pipe(&mut self, pipe: Pipe) {
        if self.pipes.len() < MOST_PIPES {
            self.pipes.push(pipe);
        }
    }

    /// Sets surface `index`, growing the table with matte sheets.
    pub fn set_surface(&mut self, index: u32, surface: SurfaceGpu) {
        let i = index as usize;
        if self.surfaces.len() <= i {
            self.surfaces.resize(i + 1, SurfaceGpu::MATTE);
        }
        self.surfaces[i] = surface;
    }
}

/// What a lit frame shows. Only the editor sets anything but the picture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LightShow {
    /// The lit picture.
    #[default]
    Picture,
    /// The light arriving at each pixel, as it would fall on a white surface.
    LightOnly,
    /// How bright that light is, through a heat palette.
    HeatMap,
}

impl LightShow {
    /// Its number in the lighting shader.
    pub fn code(self) -> f32 {
        match self {
            Self::Picture => 0.0,
            Self::LightOnly => 1.0,
            Self::HeatMap => 2.0,
        }
    }
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

/// Where a layer's things draw among the others of their z-order, in drawing order. At z-order 0
/// the frame's own lists are its plain things: trails and shapes, then the `Behind` and
/// `BehindOver` layers, then its balls and flashes, then the `Blend` and `Over` layers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Place {
    /// Plain shapes of a z-order other than 0 (drawn straight in, not mixed).
    Back,
    /// Blended things that sit behind the balls (shapes): mixed with what is behind them only.
    Behind,
    /// What a split shape paints normally over its blended part, still behind the balls.
    BehindOver,
    /// Plain balls, trails and particles of a z-order other than 0 (drawn straight in).
    Front,
    /// Other blended things.
    #[default]
    Blend,
    /// What a split ball paints normally over its blended part.
    Over,
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
    /// The z-order of its things: layers below 0 are drawn before the frame's own lists, above
    /// 0 after everything at 0.
    pub z: i32,
    /// Where it draws among the layers of its z-order.
    pub place: Place,
    /// The layer's own lists. Its globals follow the frame it belongs to; it paints from that
    /// frame's gradient atlas, so its own `gradients` stay empty, and the hits and effects drawn
    /// into it are handed to that frame.
    pub frame: Frame,
}

impl BlendLayer {
    /// Its things sit behind the balls of its z-order (shapes).
    pub fn behind(&self) -> bool {
        matches!(self.place, Place::Back | Place::Behind | Place::BehindOver)
    }

    /// Whether it is drawn after the frame's own things and the Things-stage effects on them:
    /// a higher z-order, or at 0 not behind the balls.
    pub fn above(&self) -> bool {
        self.z > 0 || (self.z == 0 && !self.behind())
    }

    /// A layer of things drawn straight into the picture, not mixed (things at a z-order
    /// other than 0 that don't blend).
    pub fn plain(&self) -> bool {
        matches!(self.place, Place::Back | Place::Front)
    }
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
            paths: Vec::new(),
            path_points: Vec::new(),
            flashes: Vec::new(),
            layered: Vec::new(),
            look_layers: Vec::new(),
            mask_tiles: Vec::new(),
            programs: Vec::new(),
            layered_runs: Vec::new(),
            trails: Vec::new(),
            particles: Vec::new(),
            trails_front: Vec::new(),
            particles_front: Vec::new(),
            fields: Vec::new(),
            fields_front: Vec::new(),
            swarms: Vec::new(),
            swarms_front: Vec::new(),
            hits: Vec::new(),
            effects: Vec::new(),
            gradients: GradientAtlas::default(),
            camera: CameraView::default(),
            background_on_screen: true,
            overlay: None,
            video: (1080, 1920),
            layers: Vec::new(),
            grade: Grade::default(),
            light: None,
            ball_marks: Vec::new(),
            ripples: false,
            ripple_sources: Vec::new(),
            ripple_owners: 0,
            beat: [0.0; 4],
            bake: None,
            liquids: Vec::new(),
        }
    }
}

/// The most Ripples sources a frame keeps (its layers' together); past it they are left out.
pub const MOST_RIPPLE_SOURCES: usize = 16384;
/// The most segments one outline gives Ripples.
pub const MOST_RIPPLE_SEGMENTS: usize = 256;

/// Something Ripples measure from: a circle (a ball) or a thick segment of a shape's outline, in
/// world pixels. Matches the sources in ripples.wgsl and paint.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct RippleSource {
    /// From xy, to xy (a circle: its centre twice).
    pub a: [f32; 4],
    /// Radius (half the band of a segment), its owner's number, 0 a ball or 1 a shape, spare.
    pub b: [f32; 4],
    /// Its owner's centre xy (what a paint inside it skips), two spare.
    pub c: [f32; 4],
}

impl Frame {
    /// A new owner number for [`Frame::push_ripple_ball`] and [`Frame::push_ripple_outline`]:
    /// the sources of one thing (a ball, a shape) share one, and Ripples measure from the two
    /// nearest owners.
    pub fn ripple_owner(&mut self) -> u32 {
        self.ripple_owners += 1;
        self.ripple_owners
    }

    /// A ball as a Ripples source (only when the frame has Ripples).
    pub fn push_ripple_ball(&mut self, centre: [f32; 2], radius: f32) {
        if !self.ripples || self.ripple_sources.len() >= MOST_RIPPLE_SOURCES {
            return;
        }
        let owner = self.ripple_owner() as f32;
        let r = if radius.is_finite() { radius.max(0.0) } else { 0.0 };
        self.ripple_sources.push(RippleSource {
            a: [centre[0], centre[1], centre[0], centre[1]],
            b: [r, owner, 0.0, 0.0],
            c: [centre[0], centre[1], 0.0, 0.0],
        });
    }

    /// A closed outline through `points` (world pixels), its band `half` wide each side, owned by
    /// a thing centred at `centre`, as Ripples sources: at most [`MOST_RIPPLE_SEGMENTS`]
    /// segments, every few corners when it has more.
    pub fn push_ripple_outline(&mut self, centre: [f32; 2], points: &[[f32; 2]], half: f32) {
        self.push_ripple_shape(centre, std::slice::from_ref(&points), half);
    }

    /// Several closed outlines (a letter's contours) as one Ripples owner centred at `centre`:
    /// at most [`MOST_RIPPLE_SEGMENTS`] segments across them, every few corners when they have more.
    pub fn push_ripple_shape<P: AsRef<[[f32; 2]]>>(&mut self, centre: [f32; 2], contours: &[P], half: f32) {
        let total: usize = contours.iter().map(|c| c.as_ref().len()).sum();
        if !self.ripples || total == 0 {
            return;
        }
        let owner = self.ripple_owner() as f32;
        let half = if half.is_finite() { half.max(0.0) } else { 0.0 };
        let step = total.div_ceil(MOST_RIPPLE_SEGMENTS).max(1);
        for points in contours {
            let points = points.as_ref();
            // A small contour (an i's dot) keeps at least three corners.
            let step = step.min(points.len() / 3).max(1);
            let kept: Vec<[f32; 2]> = points.iter().step_by(step).copied().collect();
            for k in 0..kept.len() {
                if self.ripple_sources.len() >= MOST_RIPPLE_SOURCES {
                    return;
                }
                let (p, q) = (kept[k], kept[(k + 1) % kept.len()]);
                self.ripple_sources.push(RippleSource {
                    a: [p[0], p[1], q[0], q[1]],
                    b: [half, owner, 1.0, 0.0],
                    c: [centre[0], centre[1], 0.0, 0.0],
                });
            }
        }
    }

    /// Every Ripples source of this frame and its layers' frames, each frame's owners numbered
    /// after the last's so no two things share one.
    pub fn all_ripple_sources(&self) -> Vec<RippleSource> {
        let mut all = Vec::new();
        let mut base = 0.0;
        self.gather_ripple_sources(&mut all, &mut base);
        all.truncate(MOST_RIPPLE_SOURCES);
        all
    }

    fn gather_ripple_sources(&self, all: &mut Vec<RippleSource>, base: &mut f32) {
        all.extend(
            self.ripple_sources.iter().map(|s| RippleSource { b: [s.b[0], s.b[1] + *base, s.b[2], s.b[3]], ..*s }),
        );
        *base += self.ripple_owners as f32;
        for l in &self.layers {
            l.frame.gather_ripple_sources(all, base);
        }
    }
}

impl Frame {
    /// Empties the instance lists and the gradients and keeps the globals.
    pub fn clear(&mut self) {
        self.paths.clear();
        self.path_points.clear();
        self.flashes.clear();
        self.layered.clear();
        self.look_layers.clear();
        self.mask_tiles.clear();
        self.programs.clear();
        self.layered_runs.clear();
        self.trails.clear();
        self.particles.clear();
        self.trails_front.clear();
        self.particles_front.clear();
        self.fields.clear();
        self.fields_front.clear();
        self.liquids.clear();
        self.swarms.clear();
        self.swarms_front.clear();
        self.hits.clear();
        self.effects.clear();
        self.gradients.clear();
        self.background = BackgroundPaint::default();
        self.camera = CameraView::default();
        self.layers.clear();
        self.grade = Grade::default();
        self.light = None;
        self.ball_marks.clear();
        self.ripples = false;
        self.ripple_sources.clear();
        self.ripple_owners = 0;
        self.beat = [0.0; 4];
        self.bake = None;
        if let Some(overlay) = &mut self.overlay {
            overlay.clear();
        }
    }

    /// The longest memory (seconds) of this frame's effects and its glow in the dark: how far
    /// back the editor replays after a seek so the picture matches a straight run (0 when
    /// nothing remembers). Remembered light fades by e^(-t / fade), under 1/4096 after about
    /// 8.3 fades.
    pub fn memory(&self) -> f32 {
        let glow = self
            .light
            .as_ref()
            .filter(|l| l.settings.remembers())
            .map_or(0.0, |l| (l.settings.memory[1].max(1e-3) * 4096f32.ln()).min(crate::effects::MAX_MEMORY));
        self.effects.iter().filter(|e| e.blend != 0.0).map(|e| e.memory).fold(glow, f32::max)
    }

    /// The frame things drawn with `blend` at `amount` go into: this one for Normal at full
    /// amount, else the layer for exactly that blend, amount and `each` (whether its things
    /// blend with each other, kept only where the blend [combines with
    /// itself](Blend::combines_with_itself)), made on first use. The layer takes this frame's
    /// size, moment, camera and picture settings.
    pub fn layer_mut(&mut self, blend: Blend, amount: f32, each: bool) -> &mut Frame {
        self.layer_behind_mut(blend, amount, each, false)
    }

    /// [`Frame::layer_mut`] for things that sit behind the balls when `behind` (see
    /// [`Place::Behind`]): a layer of their own, mixed in before the balls are drawn.
    pub fn layer_behind_mut(&mut self, blend: Blend, amount: f32, each: bool, behind: bool) -> &mut Frame {
        self.target_mut(0, blend, amount, each, behind)
    }

    /// The frame a thing of z-order `z` drawn with `blend` at `amount` goes into: at z-order 0
    /// as [`Frame::layer_behind_mut`]; at another, a plain layer of that z-order (`Back` for
    /// things `behind` the balls, `Front` for the rest) when it doesn't blend, else its blend
    /// layer of that z-order.
    pub fn target_mut(&mut self, z: i32, blend: Blend, amount: f32, each: bool, behind: bool) -> &mut Frame {
        if !layered(blend, amount) {
            if z == 0 {
                return self;
            }
            let place = if behind { Place::Back } else { Place::Front };
            return self.layer_at(z, place, Blend::Normal, 1.0, false);
        }
        // -0 and 0 are one amount (their bits differ).
        let amount = amount + 0.0;
        let each = each && blend.combines_with_itself();
        self.layer_at(z, if behind { Place::Behind } else { Place::Blend }, blend, amount, each)
    }

    /// The layer of z-order `z` at `place` with exactly `blend`, `amount` and `each`, made on
    /// first use, with this frame's size, moment, camera and picture settings.
    fn layer_at(&mut self, z: i32, place: Place, blend: Blend, amount: f32, each: bool) -> &mut Frame {
        let same = |l: &BlendLayer| {
            l.z == z && l.place == place && l.blend == blend && l.amount.to_bits() == amount.to_bits() && l.each == each
        };
        let i = match self.layers.iter().position(same) {
            Some(i) => i,
            None => {
                self.layers.push(BlendLayer { blend, amount, each, z, place, frame: Frame::default() });
                self.layers.len() - 1
            }
        };
        let (width, height, time, video, camera) = (self.width, self.height, self.time, self.video, self.camera);
        let (bloom, exposure, on_screen) = (self.bloom, self.exposure, self.background_on_screen);
        let f = &mut self.layers[i].frame;
        (f.width, f.height, f.time, f.video, f.camera) = (width, height, time, video, camera);
        (f.bloom, f.exposure, f.background_on_screen) = (bloom, exposure, on_screen);
        f.ripples = self.ripples;
        f
    }

    /// The layer of what things paint normally over their blended parts (the layers of a ball
    /// above its Blend layer): a Normal layer at full amount, which [`Frame::layer_mut`] never
    /// makes. [`build_frame`] sorts it after every other layer of z-order 0, so it is mixed in
    /// last.
    pub fn over_layer_mut(&mut self) -> &mut Frame {
        self.over_mut(0, false)
    }

    /// The layer of what things of z-order `z` paint normally over their blended parts: `Over`,
    /// or `BehindOver` for things `behind` the balls (a shape's layers above its Blend layer).
    pub fn over_mut(&mut self, z: i32, behind: bool) -> &mut Frame {
        let place = if behind { Place::BehindOver } else { Place::Over };
        self.layer_at(z, place, Blend::Normal, 1.0, false)
    }

    /// Puts the layers in drawing order: by z-order, then by [`Place`], keeping the order they
    /// were first drawn into otherwise.
    fn sort_layers(&mut self) {
        self.layers.sort_by_key(|l| (l.z, l.place));
    }

    /// Whether this frame's own lists (not its layers') draw the thing with `code` (see
    /// [`thing_code`]) into the object picture: a ball or a shape of it.
    pub(crate) fn draws_thing(&self, code: f32) -> bool {
        let is = |c: f32| (c.floor() - code).abs() < 0.5;
        self.layered.iter().any(|l| is(l.misc[1])) || self.paths.iter().any(|p| is(p.c[0]))
    }

    /// Whether anything it (or a layer or its overlay) draws gives off light: a layer record, a
    /// trail vertex or a particle with an emission.
    pub fn gives_light(&self) -> bool {
        let on = |e: &[f32; 4]| e[..3].iter().any(|c| *c != 0.0);
        // A Heat glow that lights the room counts too (lighting part 4).
        let heat = |r: &LayerGpu| r.head[0] == LayerGpu::SUB_FLASH && r.amount[3] > 0.5 && r.amount[2] > 0.0;
        self.look_layers.iter().any(|r| r.emit.iter().any(on) || heat(r))
            || self.trails.iter().chain(&self.trails_front).any(|v| on(&v.emit))
            || self.particles.iter().chain(&self.particles_front).any(|p| on(&p.emit))
            || self.layers.iter().any(|l| l.frame.gives_light())
            || self.overlay.as_deref().is_some_and(Frame::gives_light)
    }

    /// Whether anything it (or a layer or its overlay) draws blocks light but lets some through
    /// (glass and light part 1): a record that blocks light at less than 1 or with an opacity
    /// under 1, a layered ball at an opacity under 1, or any trail, particle or field that blocks
    /// light (their soft edges let light through). Only then is the tint picture drawn. It may
    /// answer yes for a frame whose see-through parts end up hidden: that costs only the pass.
    pub fn see_through_blockers(&self) -> bool {
        let part = |b: f32| b > 0.0 && b < 1.0;
        self.look_layers.iter().any(|r| part(r.light[1]) || (r.light[1] > 0.0 && r.amount[0] < 1.0))
            || self.layered.iter().any(|l| l.misc[0] < 1.0)
            || self.trails.iter().chain(&self.trails_front).any(|v| v.light[1] > 0.0)
            || self.particles.iter().chain(&self.particles_front).any(|p| p.light[1] > 0.0)
            || self.fields.iter().chain(&self.fields_front).any(|f| f.look.light[1] > 0.0)
            || self.liquids.iter().any(|l| l.style.real() && part(l.style.d[2]))
            || self.layers.iter().any(|l| l.frame.see_through_blockers())
            || self.overlay.as_deref().is_some_and(Frame::see_through_blockers)
    }

    /// Glass and light part 3: the box round everything drawn as free glass (particles whose
    /// layer bends light, `Particle::light[2]` above 1, and Glass liquids that bend light,
    /// `LiquidStyle::c[2]`), in the frame and its blend layers' frames (not the overlay: what is
    /// stuck to the screen bends no lamp light), as a [`FREE_BENDER`]; `None` when nothing is.
    pub fn free_glass_box(&self) -> Option<Bender> {
        let sprites =
            self.particles.iter().chain(&self.particles_front).filter(|p| p.light[2] > BENDS_FROM).filter_map(|p| {
                let reach = p.place[3].abs().max(p.style[0].abs()) * 1.5 + 2.0;
                Bender::round([p.at()], reach, FREE_BENDER)
            });
        let blobs = self.liquids.iter().filter(|l| l.style.c[2] > BENDS_FROM).flat_map(|l| {
            l.blobs.iter().filter_map(|b| {
                Bender::round([Vec2::new(b.place[0], b.place[1])], b.place[2].abs() * 2.0 + 2.0, FREE_BENDER)
            })
        });
        let inner = self.layers.iter().filter_map(|l| l.frame.free_glass_box());
        sprites.chain(blobs).chain(inner).reduce(Bender::union)
    }

    /// Whether none of the lists the picture is drawn from has anything in them.
    fn draws_nothing(&self) -> bool {
        self.paths.is_empty()
            && self.flashes.is_empty()
            && self.layered.is_empty()
            && self.trails.is_empty()
            && self.particles.is_empty()
            && self.trails_front.is_empty()
            && self.particles_front.is_empty()
            && self.fields.is_empty()
            && self.fields_front.is_empty()
            && self.swarms.is_empty()
            && self.swarms_front.is_empty()
            && self.layers.is_empty()
            && self.liquids.is_empty()
    }

    /// Adds a shape: a circle of radius `scale` when `points` is None, else the closed outline
    /// through `points` (shape units, turned by `angle`, scaled by `scale`). `half_thickness` 0
    /// fills it. It draws plain in `color` until its `layers` name records. Gives its index in
    /// `paths`.
    #[allow(clippy::too_many_arguments)]
    pub fn push_path(
        &mut self,
        center: Vec2,
        angle: f32,
        scale: f32,
        half_thickness: f32,
        points: Option<&[Vec2]>,
        color: [f32; 4],
        code: f32,
    ) -> usize {
        let first = self.path_points.len();
        let (kind, reach) = match points {
            None => (0.0, 1.0),
            Some(ps) => {
                self.path_points.extend(ps.iter().map(|p| [p.x, p.y, 0.0, 0.0]));
                (1.0, ps.iter().map(|p| p.length()).fold(0.0, f32::max))
            }
        };
        let count = self.path_points.len() - first;
        self.paths.push(PathInstance {
            a: [center.x, center.y, angle, scale],
            b: [half_thickness.max(0.0), kind, first as f32, count as f32],
            c: [code, reach, 0.0, reach],
            color,
            layers: [0, 0, PathInstance::NO_PROGRAM, 0],
            hit: NO_HIT,
            clip: [0.0; 4],
        });
        self.paths.len() - 1
    }

    /// Adds an exact ellipse with half-axes `axes` (shape units, the larger one at most 1) to the
    /// shapes, as [`Frame::push_path`] adds a circle or a list of points (kind 2: see
    /// [`PathInstance`]), giving its index in `paths`.
    #[allow(clippy::too_many_arguments)]
    pub fn push_ellipse(
        &mut self,
        center: Vec2,
        angle: f32,
        scale: f32,
        half_thickness: f32,
        axes: Vec2,
        color: [f32; 4],
        code: f32,
    ) -> usize {
        let reach = axes.x.max(axes.y);
        self.paths.push(PathInstance {
            a: [center.x, center.y, angle, scale],
            b: [half_thickness.max(0.0), 2.0, axes.x, axes.y],
            c: [code, reach, 0.0, reach],
            color,
            layers: [0, 0, PathInstance::NO_PROGRAM, 0],
            hit: NO_HIT,
            clip: [0.0; 4],
        });
        self.paths.len() - 1
    }

    /// Adds one letter of a Text (Text part 1): a filled shape at scale 1 (shape units are pixels)
    /// whose outline is several closed `contours` (holes included, even-odd), turned by `angle`
    /// about `center`. Each contour's first point is marked [`CONTOUR_START`] and its last
    /// [`CONTOUR_END`] in `path_points[k].z`, with `w` its point count less one, so the shaders walk
    /// each contour's own edges (a shape's points keep 0 and close round the whole list as ever).
    /// The quad covers only the letter's box grown by `margin` px (its band, what its layers reach
    /// past it): written as the instance's 4-corner `clip`. A letter with no contour of 3 or more
    /// points draws nothing (`None`).
    pub fn push_letter(
        &mut self,
        center: Vec2,
        angle: f32,
        contours: &[Vec<Vec2>],
        margin: f32,
        color: [f32; 4],
        code: f32,
    ) -> Option<usize> {
        let first = self.path_points.len();
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        let mut reach = 0.0f32;
        for c in contours.iter().filter(|c| c.len() >= 3 && c.iter().all(|p| p.is_finite())) {
            let last = (c.len() - 1) as f32;
            for (k, p) in c.iter().enumerate() {
                let z = if k == 0 {
                    CONTOUR_START
                } else if k + 1 == c.len() {
                    CONTOUR_END
                } else {
                    0.0
                };
                self.path_points.push([p.x, p.y, z, if z > 0.0 { last } else { 0.0 }]);
                lo = lo.min(*p);
                hi = hi.max(*p);
                reach = reach.max(p.length());
            }
        }
        let count = self.path_points.len() - first;
        if count == 0 {
            return None;
        }
        let margin = if margin.is_finite() { margin.max(0.0) } else { 0.0 };
        let (lo, hi) = (lo - Vec2::splat(margin), hi + Vec2::splat(margin));
        let clip_first = self.path_points.len();
        // Round the positive way (clockwise on screen), as a convex clip's corners go.
        for p in [Vec2::new(lo.x, lo.y), Vec2::new(hi.x, lo.y), Vec2::new(hi.x, hi.y), Vec2::new(lo.x, hi.y)] {
            self.path_points.push([p.x, p.y, 0.0, 0.0]);
        }
        self.paths.push(PathInstance {
            a: [center.x, center.y, angle, 1.0],
            b: [0.0, 1.0, first as f32, count as f32],
            c: [code, reach, 0.0, reach],
            color,
            layers: [0, 0, PathInstance::NO_PROGRAM, 0],
            hit: NO_HIT,
            clip: [clip_first as f32, 4.0, 0.0, 0.0],
        });
        Some(self.paths.len() - 1)
    }

    /// Where tile `hash` (`bytes`, as [`Frame::mask_tiles`] holds them) is in this frame's tiles,
    /// adding it if it isn't there yet; `None` past the most a frame holds.
    pub fn mask_tile(&mut self, hash: u64, bytes: &std::sync::Arc<[u8]>) -> Option<u32> {
        if let Some(i) = self.mask_tiles.iter().position(|(h, _)| *h == hash) {
            return Some(i as u32);
        }
        if self.mask_tiles.len() >= crate::MAX_MASK_TILES || bytes.len() != crate::MASK_TILE_BYTES {
            return None;
        }
        self.mask_tiles.push((hash, bytes.clone()));
        Some(self.mask_tiles.len() as u32 - 1)
    }

    /// A fast letter (Text part 2): a shape at `center` turned `angle` (scale 1) whose distance is
    /// read from distance tile `tile` of [`Frame::mask_tiles`] instead of an outline ([`PATH_GLYPH`]).
    /// The tile's middle sits at `tile_at` in the shape's own frame, turned `turn`, `half` px from its
    /// middle to its sides; the quad covers the tile (its 4-corner `clip`). See paths.wgsl.
    #[allow(clippy::too_many_arguments)]
    pub fn push_glyph(
        &mut self,
        center: Vec2,
        angle: f32,
        tile_at: Vec2,
        turn: f32,
        half: f32,
        tile: u32,
        color: [f32; 4],
        code: f32,
    ) -> Option<usize> {
        if !(tile_at.is_finite() && turn.is_finite() && half.is_finite() && half > 0.0) {
            return None;
        }
        let record = self.path_points.len();
        self.path_points.push([tile_at.x, tile_at.y, turn, half]);
        let clip_first = self.path_points.len();
        let r = Vec2::from_angle(turn);
        let mut reach = 0.0f32;
        // Round the positive way (clockwise on screen), as a convex clip's corners go.
        for c in [Vec2::new(-1.0, -1.0), Vec2::new(1.0, -1.0), Vec2::new(1.0, 1.0), Vec2::new(-1.0, 1.0)] {
            let p = tile_at + r.rotate(c * half);
            reach = reach.max(p.length());
            self.path_points.push([p.x, p.y, 0.0, 0.0]);
        }
        self.paths.push(PathInstance {
            a: [center.x, center.y, angle, 1.0],
            b: [0.0, PATH_GLYPH, record as f32, tile as f32],
            c: [code, reach, 0.0, reach],
            color,
            layers: [0, 0, PathInstance::NO_PROGRAM, 0],
            hit: NO_HIT,
            clip: [clip_first as f32, 4.0, 0.0, 0.0],
        });
        Some(self.paths.len() - 1)
    }

    /// The frame stuck-to-screen things draw into, made on first use with this frame's size and
    /// moment.
    pub fn overlay_mut(&mut self) -> &mut Frame {
        let (width, height, time, video) = (self.width, self.height, self.time, self.video);
        let ripples = self.ripples;
        let overlay = self.overlay.get_or_insert_with(Box::default);
        (overlay.width, overlay.height, overlay.time, overlay.video) = (width, height, time, video);
        overlay.ripples = ripples;
        overlay
    }

    /// Moves the overlay's shader effects and hit marks into this frame: those of stuck things
    /// work on the whole picture like everyone else's. So do its Ripples sources and the marks
    /// in the effects' ball list (its blend layers' too), taken from the screen to the world
    /// where they show through this frame's camera. A layer left with nothing to draw is dropped.
    pub fn lift_overlay_extras(&mut self) {
        let camera = self.camera;
        let Some(o) = self.overlay.as_deref_mut() else { return };
        let (effects, hits) = (std::mem::take(&mut o.effects), std::mem::take(&mut o.hits));
        let mut sources = Vec::new();
        let mut owners = self.ripple_owners as f32;
        o.gather_ripple_sources(&mut sources, &mut owners);
        let mut marks = std::mem::take(&mut o.ball_marks);
        for l in &mut o.layers {
            marks.append(&mut l.frame.ball_marks);
            l.frame.ripple_sources.clear();
            l.frame.ripple_owners = 0;
        }
        (o.ripple_sources, o.ripple_owners) = (Vec::new(), 0);
        o.layers.retain(|l| !l.frame.draws_nothing());
        self.effects.extend(effects);
        self.hits.extend(hits);
        let world = |x: f32, y: f32| camera.to_world(Vec2::new(x, y)).to_array();
        let zoom = if camera.zoom.is_finite() && camera.zoom > 0.0 { camera.zoom } else { 1.0 };
        for s in &mut sources {
            let (p, q, c) = (world(s.a[0], s.a[1]), world(s.a[2], s.a[3]), world(s.c[0], s.c[1]));
            (s.a, s.b[0], s.c) = ([p[0], p[1], q[0], q[1]], s.b[0] / zoom, [c[0], c[1], s.c[2], s.c[3]]);
        }
        self.ripple_sources.extend(sources);
        self.ripple_owners = owners as u32;
        for mut m in marks {
            (m.center, m.radius) = (world(m.center[0], m.center[1]), m.radius / zoom);
            self.ball_marks.push(m);
        }
    }

    /// Whether the overlay has anything to draw.
    pub fn has_overlay(&self) -> bool {
        self.overlay.as_ref().is_some_and(|o| {
            !(o.paths.is_empty()
                && o.flashes.is_empty()
                && o.layered.is_empty()
                && o.trails.is_empty()
                && o.particles.is_empty()
                && o.trails_front.is_empty()
                && o.particles_front.is_empty()
                && o.fields.is_empty()
                && o.fields_front.is_empty()
                && o.swarms.is_empty()
                && o.swarms_front.is_empty()
                && o.layers.is_empty()
                && o.liquids.is_empty())
        })
    }

    /// The moved Stage camera this frame is seen through (lighting part 5): a lit frame in Stage
    /// whose light scene has a camera. `None` leaves the picture as the front shot.
    pub fn stage_view(&self) -> Option<crate::StageView> {
        let light = self.light.as_ref()?;
        let cam = light.camera.as_ref()?;
        let s = &light.settings;
        if s.stage[0] < 0.5 {
            return None;
        }
        let size = glam::Vec2::new(self.width.max(1) as f32, self.height.max(1) as f32);
        let floor = match s.floor[0] {
            k if k > 1.5 => Some(self.camera.to_world(glam::Vec2::new(0.0, size.y * 0.5)).y),
            k if k > 0.5 => Some(s.floor[1]),
            _ => None,
        };
        let layout = crate::StageLayout { back: s.stage[1], k: s.stage[3], floor };
        Some(crate::StageView::solve(cam, &self.camera, size, layout))
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

    /// Cuts its trail and particle lists (its overlay's and blend layers' too) to what the
    /// graphics card can hold in one buffer, keeping the first: a last guard, far above what any
    /// layer draws on its own.
    pub fn fit_lists(&mut self) {
        self.fit_lists_to(MOST_TRAIL_VERTICES, MOST_SPRITES);
    }

    fn fit_lists_to(&mut self, vertices: usize, sprites: usize) {
        // Whole triangles.
        let vertices = vertices - vertices % 6;
        self.trails.truncate(vertices);
        self.trails_front.truncate(vertices);
        self.particles.truncate(sprites);
        self.particles_front.truncate(sprites);
        for (fields, len) in
            [(&mut self.fields, self.particles.len()), (&mut self.fields_front, self.particles_front.len())]
        {
            for f in fields.iter_mut() {
                f.at = f.at.min(len as u32);
                f.splats.truncate(sprites);
            }
        }
        for (swarms, len) in
            [(&mut self.swarms, self.particles.len()), (&mut self.swarms_front, self.particles_front.len())]
        {
            for r in swarms.iter_mut() {
                r.at = r.at.min(len as u32);
            }
        }
        if let Some(o) = &mut self.overlay {
            o.fit_lists_to(vertices, sprites);
        }
        for l in &mut self.layers {
            l.frame.fit_lists_to(vertices, sprites);
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
        self.paths.retain(|p| sees([p.a[0], p.a[1]], p.reach()));
        let seen: Vec<bool> = self
            .layered
            .iter()
            .map(|l| {
                let along = l.stretch[2].max(l.stretch[3]).max(1.0);
                sees(l.center, l.radius * l.form[3].max(1.0) * along)
            })
            .collect();
        // Each run names its instances by place, so the runs move with the instances kept.
        let mut kept_before = vec![0u32; seen.len() + 1];
        for (i, &s) in seen.iter().enumerate() {
            kept_before[i + 1] = kept_before[i] + u32::from(s);
        }
        let place = |i: u32| kept_before[(i as usize).min(seen.len())];
        self.layered_runs = self
            .layered_runs
            .iter()
            .map(|r| crate::LayeredRun { start: place(r.start), end: place(r.end), ..*r })
            .filter(|r| r.end > r.start)
            .collect();
        let mut i = 0;
        self.layered.retain(|_| {
            i += 1;
            seen[i - 1]
        });
        self.flashes.retain(|f| sees(f.center, f.size));
        let reach = |p: &Particle| p.place[3].max(p.style[0]) * (1.0 + p.style[2]) + 4.0;
        let splat = |s: &Splat| sees([s.place[0], s.place[1]], s.place[2] * 1.6 + 4.0);
        cull_sprites(
            &mut self.particles,
            &mut self.fields,
            &mut self.swarms,
            |p| sees([p.place[0], p.place[1]], reach(p)),
            splat,
        );
        cull_sprites(
            &mut self.particles_front,
            &mut self.fields_front,
            &mut self.swarms_front,
            |p| sees([p.place[0], p.place[1]], reach(p)),
            splat,
        );
        for l in &mut self.layers {
            l.frame.cull_through(camera, (width, height));
        }
    }
}

/// Keeps the sprites of `list` that `keep` says and the splats of `fields` that `keep_splat`
/// says, each field (and each swarm run of `swarms`) still sitting between the same kept sprites.
fn cull_sprites(
    list: &mut Vec<Particle>,
    fields: &mut [Field],
    swarms: &mut [crate::SwarmRun],
    keep: impl Fn(&Particle) -> bool,
    keep_splat: impl Fn(&Splat) -> bool,
) {
    if fields.is_empty() && swarms.is_empty() {
        list.retain(keep);
        return;
    }
    let seen: Vec<bool> = list.iter().map(&keep).collect();
    let mut kept_before = vec![0u32; seen.len() + 1];
    for (i, &s) in seen.iter().enumerate() {
        kept_before[i + 1] = kept_before[i] + u32::from(s);
    }
    for f in fields.iter_mut() {
        f.at = kept_before[(f.at as usize).min(seen.len())];
        f.splats.retain(&keep_splat);
    }
    for r in swarms.iter_mut() {
        r.at = kept_before[(r.at as usize).min(seen.len())];
    }
    let mut i = 0;
    list.retain(|_| {
        i += 1;
        seen[i - 1]
    });
}

/// The z-order `thing` draws at: its own, within [`Thing::Z_ORDER_RANGE`]; things stuck to the
/// screen draw over the world anyway and keep 0.
fn z_order(thing: &Thing, r: &ResolvedThing) -> i32 {
    if r.screen { 0 } else { thing.z_order.clamp(*Thing::Z_ORDER_RANGE.start(), *Thing::Z_ORDER_RANGE.end()) }
}

/// Whether things drawn with `blend` at `amount` need a layer of their own.
fn layered(blend: Blend, amount: f32) -> bool {
    !(blend == Blend::Normal && amount >= 1.0)
}

/// Runs `draw` on what a thing of z-order `z` drawn with `blend` at `amount` (blending with
/// each other when `each`, behind the balls of its z-order when `behind`) draws into: `target`
/// itself, or its layer (see [`Frame::target_mut`]). A layer borrows `target`'s gradient atlas,
/// hits and effects while it is drawn into, so its paint rows are the rows the renderer uploads
/// and every module reads every hit. A layer nothing was drawn into is not kept.
fn draw_routed_at(
    target: &mut Frame,
    z: i32,
    blend: Blend,
    amount: f32,
    each: bool,
    behind: bool,
    draw: impl FnOnce(&mut Frame),
) {
    if z == 0 && !layered(blend, amount) {
        draw(target);
        return;
    }
    draw_into_layer(target, |t| t.target_mut(z, blend, amount, each, behind), draw);
}

/// Runs `draw` on the over layer of z-order `z` (see [`Frame::over_mut`]), borrowing `target`'s
/// atlas, hits and effects as [`draw_routed_at`] does.
fn draw_over_routed(target: &mut Frame, z: i32, behind: bool, draw: impl FnOnce(&mut Frame)) {
    draw_into_layer(target, |t| t.over_mut(z, behind), draw);
}

fn draw_into_layer(target: &mut Frame, layer: impl FnOnce(&mut Frame) -> &mut Frame, draw: impl FnOnce(&mut Frame)) {
    let before = target.layers.len();
    let take =
        |f: &mut Frame| (std::mem::take(&mut f.gradients), std::mem::take(&mut f.hits), std::mem::take(&mut f.effects));
    let shared = take(target);
    let layer = layer(target);
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
    /// The moment being drawn, in seconds: the clock of the world, the log and the keyframes.
    pub time: f64,
    /// The scene as saved: its tree (a thing's parent, siblings and order) and its keyframes.
    pub tree: &'a Scene,
    /// The scene's light probe, when some look follows light (lighting part 7: a ball's place in
    /// a Light or Shadow spread). Never read by anything that makes light.
    pub light: Option<&'a dyn LightAt>,
    /// How far each sound is up at a moment (Follow sound), when some setting follows sound: a
    /// Text's Follow sound move reads each letter's level at its own moment through it.
    pub sound: Option<&'a dyn SoundAt>,
}

/// How a thing kind draws itself.
pub trait ThingDraw: Send + Sync + 'static {
    /// The thing kind's id, as registered in `bpe_core`.
    fn kind(&self) -> &'static str;
    fn draw(&self, cx: &DrawCx, frame: &mut Frame);
    /// How the thing mixes with what is under it (blend, amount, blend with each other), when it
    /// says so itself rather than through its Look card's `blend` settings.
    fn blend(&self, _resolved: &ResolvedThing) -> Option<(Blend, f32, bool)> {
        None
    }
    /// Whether the thing sits behind the balls (a shape): when it blends, its layer is mixed in
    /// before the balls are drawn, so it never tints them.
    fn behind(&self) -> bool {
        false
    }
    /// What the thing draws over its blended part, painted normally whatever its blend: the
    /// layers of a ball above its Blend layer. Drawn right after [`ThingDraw::draw`].
    fn draw_over(&self, _cx: &DrawCx, _frame: &mut Frame) {}
    /// What the thing draws round itself, each with a blend of its own (its trails and
    /// particles), bottom first: each one's blend, amount, and whether it sits behind the balls.
    /// Normal (at any amount) and Add draw straight into the frame, mixing themselves in; any
    /// other blend draws into a layer of its own. Drawn after every thing has drawn itself.
    fn around(&self, _resolved: &ResolvedThing) -> Vec<Around> {
        Vec::new()
    }
    /// Draws the `index`th of what [`ThingDraw::around`] lists.
    fn draw_around(&self, _cx: &DrawCx, _index: usize, _frame: &mut Frame) {}
    /// Adds the thing's lights to a lit frame's `scene`. Called only when the scene has a
    /// Lighting card (its draw set [`Frame::light`]), after every thing has drawn itself.
    fn light(&self, _cx: &DrawCx, _scene: &mut LightScene) {}
}

/// One of the things a thing draws round itself (a trail, particles), with its own blend.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Around {
    pub blend: Blend,
    pub amount: f32,
    /// Behind the balls: a layer of its own is mixed in before they are drawn.
    pub behind: bool,
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
    build_frame_with(scene, resolved, world, log, time, draws, None, frame);
}

/// [`build_frame`] with the scene's sound, for settings that follow it.
#[allow(clippy::too_many_arguments)]
pub fn build_frame_with(
    scene: &Scene,
    resolved: &Resolved,
    world: &World,
    log: &Log,
    time: f64,
    draws: &Draws,
    sound: Option<&dyn SoundAt>,
    frame: &mut Frame,
) {
    build_frame_sensing(scene, resolved, world, log, time, draws, sound, None, frame);
}

/// [`build_frame_with`] with the scene's light probe too, for looks that follow light (Follow
/// light, Light and Shadow spreads). The lights themselves never follow light: the editor
/// doesn't offer it on anything that makes light.
#[allow(clippy::too_many_arguments)]
pub fn build_frame_sensing(
    scene: &Scene,
    resolved: &Resolved,
    world: &World,
    log: &Log,
    time: f64,
    draws: &Draws,
    sound: Option<&dyn SoundAt>,
    light: Option<&dyn LightAt>,
    frame: &mut Frame,
) {
    frame.clear();
    let animation = Animation::new(scene, resolved);
    let animated;
    // What the scene's rules changed (rules part 1): settings, then broken things are gone.
    let ruled = world.rules().filter(|r| r.changes_picture());
    // Gradients' companions (Bands, Grain) are applied once a frame, after the animation.
    let resolved = if animation.is_empty() && !bpe_core::gradient_extras::any_in(resolved) && ruled.is_none() {
        resolved
    } else {
        let mut copy = resolved.clone();
        let levels = sound.map(|sound| LevelsAt { sound, world, log });
        let lit = light.map(|light| LightLevelsAt { light, world, log });
        let inputs = Inputs {
            hits: hit_strength(log, time, world.steps_per_second),
            sound: levels.as_ref().map(|l| l as &dyn bpe_core::SoundLevels),
            light: lit.as_ref().map(|l| l as &dyn bpe_core::LightLevels),
            health: world.health_levels(),
        };
        animation.apply(&mut copy, time, &inputs);
        if let Some(rules) = ruled {
            rules.apply_whole(&mut copy, time, |_| true);
            // A broken thing whose Break look still plays out stays, for its look to draw.
            for id in rules.broken() {
                if !rules.showing(id, time) {
                    copy.things.remove(&id);
                }
            }
        }
        bpe_core::gradient_extras::apply_moves(&mut copy, time);
        animated = copy;
        &animated
    };
    // Things push what Ripples measure from only when some gradient is laid out as Ripples.
    frame.ripples = bpe_core::gradient_extras::any_ripples(resolved);
    for (id, r) in &resolved.things {
        let Some(thing) = scene.things.get(id) else { continue };
        let Some(d) = draws.thing(&r.kind) else { continue };
        let ((blend, amount), each) = match (r.slots.get(&SlotId::Look), d.blend(r)) {
            _ if *id == ThingId::ROOT => ((Blend::Normal, 1.0), false),
            (_, Some((blend, amount, each))) => ((blend, amount), each),
            (Some(look), None) => (Blend::from_params(&look.params), Blend::each_from_params(&look.params)),
            (None, None) => ((Blend::Normal, 1.0), false),
        };
        let target = if r.screen { frame.overlay_mut() } else { &mut *frame };
        let cx = DrawCx { thing, resolved: r, scene: resolved, world, log, time, tree: scene, light, sound };
        let z = z_order(thing, r);
        draw_routed_at(target, z, blend, amount, each, d.behind(), |into| d.draw(&cx, into));
        // Only a thing that blends has parts to paint over the blend: into the over layer of its
        // z-order, which is mixed in after its blend layers (a shape's before the balls).
        if layered(blend, amount) {
            let target = if r.screen { frame.overlay_mut() } else { &mut *frame };
            draw_over_routed(target, z, d.behind(), |into| d.draw_over(&cx, into));
        }
    }
    for (id, r) in &resolved.things {
        let Some(thing) = scene.things.get(id) else { continue };
        let Some(d) = draws.thing(&r.kind) else { continue };
        for (index, around) in d.around(r).into_iter().enumerate() {
            let (blend, amount) = match around.blend {
                Blend::Normal | Blend::Add => (Blend::Normal, 1.0),
                other => (other, around.amount),
            };
            let target = if r.screen { frame.overlay_mut() } else { &mut *frame };
            let cx = DrawCx { thing, resolved: r, scene: resolved, world, log, time, tree: scene, light, sound };
            let z = z_order(thing, r);
            draw_routed_at(target, z, blend, amount, false, around.behind, |into| d.draw_around(&cx, index, into));
        }
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
            let cx = DrawCx { thing, resolved: r, scene: resolved, world, log, time, tree: scene, light, sound };
            let z = z_order(thing, r);
            draw_routed_at(target, z, blend, amount, each, false, |into| d.draw(&cx, slot, into));
        }
    }
    // A lit scene's lights, only when its Lighting card set `frame.light`.
    if let Some(mut lit) = frame.light.take() {
        for (id, r) in &resolved.things {
            let Some(thing) = scene.things.get(id) else { continue };
            let Some(d) = draws.thing(&r.kind) else { continue };
            // What makes light never reads the light probe (one-way sensing).
            let cx = DrawCx { thing, resolved: r, scene: resolved, world, log, time, tree: scene, light: None, sound };
            d.light(&cx, &mut lit);
        }
        // Light pipes (glass and light part 4a) carry the light every thing has made.
        lit.light_pipes();
        lit.glows = frame.gives_light();
        frame.light = Some(lit);
    }
    frame.sort_layers();
    if let Some(o) = frame.overlay.as_deref_mut() {
        o.sort_layers();
    }
    frame.camera = world.camera;
    for l in &mut frame.layers {
        l.frame.camera = world.camera;
    }
    frame.lift_overlay_extras();
    frame.fit_lists();
}

/// The most trail vertices one list of a frame holds (about 115 MB).
pub const MOST_TRAIL_VERTICES: usize = 3_000_000;
/// The most particles one list of a frame holds (about 61 MB).
pub const MOST_SPRITES: usize = 1_000_000;

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Address, Ease, Value, resolve, testkit};

    /// Whether `p` lies inside one of the strip's triangles.
    fn strip_covers(out: &[TrailVertex], p: Vec2) -> bool {
        out.chunks(3).any(|t| {
            let [a, b, c] = [t[0].position, t[1].position, t[2].position].map(Vec2::from);
            let side = |u: Vec2, v: Vec2| (v - u).perp_dot(p - u);
            let (x, y, z) = (side(a, b), side(b, c), side(c, a));
            (x >= -1e-3 && y >= -1e-3 && z >= -1e-3) || (x <= 1e-3 && y <= 1e-3 && z <= 1e-3)
        })
    }

    #[test]
    fn a_strip_s_sharp_corners_are_round_outside_and_straight_runs_get_nothing_more() {
        let style = StreakStyle::default();
        // Straight: two triangles a segment, nothing else.
        let mut straight = Vec::new();
        let line = [Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0), Vec2::new(200.0, 0.0)];
        strip(&mut straight, &line, |_| 10.0, |_| [1.0; 4], style, 1.0);
        assert_eq!(straight.len(), 12);
        // A bounce: down to the floor at (100, 100) and back up. Outside the corner, straight below
        // it, the strip used to leave a square notch.
        let mut bounce = Vec::new();
        let v = [Vec2::new(0.0, 0.0), Vec2::new(100.0, 100.0), Vec2::new(200.0, 0.0)];
        strip(&mut bounce, &v, |_| 10.0, |_| [1.0; 4], style, 1.0);
        assert!(bounce.len() > 12, "the corner gets a join");
        assert!(strip_covers(&bounce, Vec2::new(100.0, 109.0)), "the outside of the corner is filled");
        assert!(!strip_covers(&bounce, Vec2::new(100.0, 115.0)), "but no further than the strip reaches");
        // Every triangle is finite.
        assert!(bounce.iter().all(|v| v.position.iter().chain(&v.edge).all(|x| x.is_finite())));
        // Straight back the way it came (a ball rolling into a wall): the end is still rounded,
        // whichever way the sign of zero falls.
        for back in [Vec2::new(50.0, 0.0), Vec2::new(-50.0, 0.0)] {
            let mut reverse = Vec::new();
            let (corner, way) = (Vec2::new(100.0, 0.0) * back.x.signum(), back.x.signum());
            strip(&mut reverse, &[Vec2::ZERO, corner, back], |_| 10.0, |_| [1.0; 4], style, 1.0);
            assert!(strip_covers(&reverse, corner + Vec2::new(9.0 * way, 0.0)), "the far end is capped");
            assert!(strip_covers(&reverse, corner + Vec2::new(6.0 * way, 6.0)), "round, both sides");
            assert!(strip_covers(&reverse, corner + Vec2::new(6.0 * way, -6.0)), "round, both sides");
        }
    }

    #[test]
    fn free_glass_is_bending_sprites_and_glass_liquid() {
        let mut frame = Frame::default();
        assert_eq!(frame.free_glass_box(), None);
        let mut bubble = Particle::new(0, Vec2::new(100.0, 50.0), 0.0, 10.0, [1.0; 4]);
        frame.particles.push(bubble);
        assert_eq!(frame.free_glass_box(), None, "a sprite that doesn't bend");
        bubble.light[2] = 1.33;
        frame.particles.push(bubble);
        let one = frame.free_glass_box().expect("a bending sprite");
        assert_eq!(one.index, FREE_BENDER);
        assert!(one.lo[0] <= 85.0 && one.hi[0] >= 115.0 && one.lo[1] <= 35.0 && one.hi[1] >= 65.0, "{one:?}");
        let blob = crate::liquid::LiquidBlob { place: [-200.0, 0.0, 20.0, 1.0], colour: [1.0; 4] };
        let mut style = crate::liquid::LiquidStyle { a: [0.5, 1.0, 1.0, 4.0], ..Default::default() };
        frame.liquids.push(crate::liquid::Liquid { blobs: vec![blob], style, front: false });
        assert_eq!(frame.free_glass_box(), Some(one), "a glass liquid with Bends light 1");
        style.c[2] = 1.33;
        frame.liquids.push(crate::liquid::Liquid { blobs: vec![blob], style, front: false });
        let both = frame.free_glass_box().unwrap();
        assert!(both.lo[0] <= -240.0 && both.hi[0] >= 115.0, "{both:?}");
        // Inside a blend layer's own frame too.
        let mut outer = Frame::default();
        outer.layers.push(BlendLayer { blend: Blend::Add, amount: 1.0, each: false, z: 0, place: Place::Blend, frame });
        assert_eq!(outer.free_glass_box(), Some(both));
    }
    use bpe_runtime::{Event, EventKind, Log, Party, Sims, World};

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
            frame.layered.push(LayeredInstance {
                center: [1.0, 2.0],
                radius,
                body: [1.0; 4],
                ..LayeredInstance::default()
            });
            frame.flashes.push(FlashInstance { center: [0.0; 2], size: 1.0, color: [1.0; 4] });
        }
    }

    #[test]
    fn oversized_lists_are_cut_to_whole_triangles_that_fit() {
        let mut f = Frame { trails: vec![TrailVertex::default(); 107], ..Frame::default() };
        f.particles = vec![Particle::new(0, Vec2::ZERO, 0.0, 1.0, [1.0; 4]); 50];
        let mut inner = Frame { trails_front: vec![TrailVertex::default(); 107], ..Frame::default() };
        inner.particles_front = f.particles.clone();
        f.overlay = Some(Box::new(inner));
        f.fit_lists_to(100, 40);
        assert_eq!((f.trails.len(), f.particles.len()), (96, 40));
        let o = f.overlay.as_ref().unwrap();
        assert_eq!((o.trails_front.len(), o.particles_front.len()), (96, 40));
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
        assert_eq!(frame.layered.len(), 1, "lists are cleared between frames");
        assert_eq!(frame.layered[0].radius, 9.0);
        assert_eq!(frame.flashes.len(), 1);
        assert!(frame.paths.is_empty());
        assert!(draws.thing("folder").is_none());
    }

    #[test]
    fn a_keyed_setting_changes_the_frame_at_its_time() {
        let reg = testkit::registry();
        let mut scene = bpe_core::Scene::empty(&reg);
        let balls = testkit::push(&mut scene, "balls", bpe_core::ThingId::ROOT);
        let size = Address { thing: balls, slot: None, key: "radius".into() };
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
            frame.layered[0].radius
        };
        assert_eq!(radius_at(0.0, &mut frame), 2.0);
        assert!((radius_at(1.0, &mut frame) - 6.0).abs() < 1e-5);
        assert_eq!(radius_at(5.0, &mut frame), 10.0);
        assert_eq!(resolved.things[&balls].params.get("radius"), Some(&Value::Number(6.0)), "the input is untouched");
    }

    #[test]
    fn layer_records_and_layered_instances_fit_the_gpu() {
        assert_eq!(std::mem::size_of::<LayerGpu>() % 16, 0);
        assert_eq!(std::mem::size_of::<LayerGpu>(), 288, "as layers.wgsl lays it out");
        assert!(std::mem::size_of::<LayeredInstance>() <= 176);
        assert_eq!(std::mem::size_of::<LayeredInstance>() % 16, 0);
        let mut f = Frame::default();
        f.layered.push(LayeredInstance { misc: [1.0, 7.25, 0.0, 0.0], ..Default::default() });
        f.look_layers.push(LayerGpu::default());
        assert!(f.draws_thing(7.0) && !f.draws_nothing());
        f.clear();
        assert!(f.layered.is_empty() && f.look_layers.is_empty());
    }

    struct OwnBlend;
    impl ThingDraw for OwnBlend {
        fn kind(&self) -> &'static str {
            "balls"
        }
        fn draw(&self, _cx: &DrawCx, frame: &mut Frame) {
            frame.layered.push(LayeredInstance::default());
        }
        fn blend(&self, _r: &ResolvedThing) -> Option<(Blend, f32, bool)> {
            Some((Blend::Screen, 0.5, false))
        }
    }

    #[test]
    fn a_thing_that_names_its_own_blend_is_drawn_into_that_layer() {
        let reg = testkit::registry();
        let mut scene = bpe_core::Scene::empty(&reg);
        testkit::push(&mut scene, "balls", bpe_core::ThingId::ROOT);
        let resolved = resolve(&scene, &reg);
        let world = World::build(&scene, &resolved, &Sims::default());
        let mut draws = Draws::default();
        draws.add(OwnBlend);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &draws, &mut frame);
        assert!(frame.layered.is_empty());
        assert_eq!(frame.layers.len(), 1);
        assert_eq!((frame.layers[0].blend, frame.layers[0].amount), (Blend::Screen, 0.5));
        assert_eq!(frame.layers[0].frame.layered.len(), 1);
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
            at: 0.0,
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
    fn module_draws_run_after_thing_draws_for_every_slot_they_fill() {
        struct Marker;
        impl ModuleDraw for Marker {
            fn kind(&self) -> &'static str {
                "look.plain"
            }
            fn draw(&self, cx: &DrawCx, slot: SlotId, frame: &mut Frame) {
                assert_eq!(slot, SlotId::Look);
                assert_eq!(frame.layered.len(), 1, "the things drew first");
                frame.flashes.push(FlashInstance { center: [cx.thing.id.0 as f32, 0.0], ..Default::default() });
            }
        }
        let reg = testkit::registry();
        let mut scene = bpe_core::Scene::empty(&reg);
        scene.add_slot(&reg, ThingId::ROOT, SlotId::Look, "look.plain").unwrap();
        let balls = testkit::push(&mut scene, "balls", ThingId::ROOT);
        scene.add_slot(&reg, balls, SlotId::Look, "look.plain").unwrap();
        let resolved = resolve(&scene, &reg);
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
        frame.layer_mut(Blend::Normal, 1.0, false).layered.push(LayeredInstance::default());
        assert_eq!(frame.layered.len(), 1, "Normal at full amount is the frame itself");
        assert!(frame.layers.is_empty());
        frame.layer_mut(Blend::Multiply, 1.0, false).layered.push(LayeredInstance::default());
        frame.layer_mut(Blend::Screen, 0.5, false).paths.push(PathInstance::default());
        frame.layer_mut(Blend::Multiply, 1.0, false).layered.push(LayeredInstance::default());
        frame.layer_mut(Blend::Normal, 0.5, false).paths.push(PathInstance::default());
        let kept: Vec<(Blend, f32)> = frame.layers.iter().map(|l| (l.blend, l.amount)).collect();
        assert_eq!(kept, [(Blend::Multiply, 1.0), (Blend::Screen, 0.5), (Blend::Normal, 0.5)], "first-use order");
        let l = &frame.layers[0].frame;
        assert_eq!(l.layered.len(), 2, "the same mode and amount share a layer");
        assert_eq!((l.width, l.height, l.time, l.video), (100, 200, 1.5, (300, 400)));
        assert_eq!((l.camera, l.bloom), (frame.camera, 0.25));
        frame.clear();
        assert!(frame.layers.is_empty(), "clear empties the layers");
    }

    #[test]
    fn each_layers_and_plain_layers_do_not_share() {
        use bpe_core::Blend;
        let mut frame = Frame::default();
        frame.layer_mut(Blend::Screen, 1.0, true).layered.push(LayeredInstance::default());
        frame.layer_mut(Blend::Screen, 1.0, false).layered.push(LayeredInstance::default());
        frame.layer_mut(Blend::Screen, 1.0, true).layered.push(LayeredInstance::default());
        let kept: Vec<(Blend, f32, bool)> = frame.layers.iter().map(|l| (l.blend, l.amount, l.each)).collect();
        assert_eq!(kept, [(Blend::Screen, 1.0, true), (Blend::Screen, 1.0, false)]);
        assert_eq!(frame.layers[0].frame.layered.len(), 2);
        // A mode that cannot combine with itself drops the switch, and so shares.
        frame.layer_mut(Blend::Overlay, 1.0, true).layered.push(LayeredInstance::default());
        frame.layer_mut(Blend::Overlay, 1.0, false).layered.push(LayeredInstance::default());
        assert_eq!((frame.layers.len(), frame.layers[2].each, frame.layers[2].frame.layered.len()), (3, false, 2));
        frame.layer_mut(Blend::Normal, 1.0, true).layered.push(LayeredInstance::default());
        assert_eq!((frame.layers.len(), frame.layered.len()), (3, 1), "Normal at full amount is the frame itself");
    }

    #[test]
    fn a_layer_counts_towards_the_overlay_and_is_culled_with_the_frame() {
        use bpe_core::Blend;
        let mut frame = Frame::default();
        frame.overlay_mut();
        assert!(!frame.has_overlay());
        frame.overlay_mut().layer_mut(Blend::Screen, 1.0, false).layered.push(LayeredInstance::default());
        assert!(frame.has_overlay(), "a stuck blended thing is something to draw");
        frame.camera.zoom = 4.0;
        let layer = frame.layer_mut(Blend::Multiply, 1.0, false);
        layer.layered.push(LayeredInstance { center: [0.0, 0.0], radius: 5.0, ..LayeredInstance::default() });
        layer.layered.push(LayeredInstance { center: [5000.0, 0.0], radius: 5.0, ..LayeredInstance::default() });
        frame.cull();
        assert_eq!(frame.layers[0].frame.layered.len(), 1, "what the camera can't see is dropped in layers too");
    }

    #[test]
    fn culling_layered_balls_keeps_each_with_its_own_run() {
        let mut frame = Frame::default();
        frame.camera.zoom = 4.0;
        let at = |x: f32| LayeredInstance { center: [x, 0.0], radius: 5.0, ..LayeredInstance::default() };
        // A plain thing whose ball leaves the shot, then a thing with a program: one seen, one not.
        frame.layered = vec![at(5000.0), at(0.0), at(10.0), at(-6000.0)];
        frame.layered_runs = vec![
            crate::LayeredRun { program: None, start: 0, end: 1 },
            crate::LayeredRun { program: Some(3), start: 1, end: 4 },
        ];
        frame.cull();
        assert_eq!(frame.layered.len(), 2);
        assert_eq!(frame.layered_runs, [crate::LayeredRun { program: Some(3), start: 0, end: 2 }]);
    }

    #[test]
    fn a_layer_is_culled_through_the_frames_camera_after_the_view_changes() {
        use bpe_core::Blend;
        // Built through a camera zoomed in on the middle of the video...
        let mut frame = Frame::default();
        frame.camera.zoom = 4.0;
        let layer = frame.layer_mut(Blend::Screen, 1.0, false);
        layer.layered.push(LayeredInstance { center: [2000.0, 0.0], radius: 5.0, ..LayeredInstance::default() });
        layer.layered.push(LayeredInstance { center: [90_000.0, 0.0], radius: 5.0, ..LayeredInstance::default() });
        // ...then looked at by the editor's World view, zoomed far out, on a wider picture.
        frame.camera = CameraView { center: Vec2::ZERO, zoom: 0.1, turn: 0.0 };
        (frame.width, frame.height) = (2000, 1000);
        frame.cull();
        let kept: Vec<[f32; 2]> = frame.layers[0].frame.layered.iter().map(|b| b.center).collect();
        assert_eq!(kept, [[2000.0, 0.0]], "the layer is culled through the frame's own view");
    }

    #[test]
    fn minus_zero_and_zero_amounts_share_a_layer() {
        use bpe_core::Blend;
        let mut frame = Frame::default();
        frame.layer_mut(Blend::Screen, -0.0, false).layered.push(LayeredInstance::default());
        frame.layer_mut(Blend::Screen, 0.0, false).layered.push(LayeredInstance::default());
        assert_eq!(frame.layers.len(), 1);
        assert_eq!(frame.layers[0].amount.to_bits(), 0.0f32.to_bits());
    }

    #[test]
    fn set_view_moves_the_layers_with_the_frame() {
        use bpe_core::Blend;
        let mut frame = Frame::default();
        frame.layer_mut(Blend::Screen, 1.0, false).layered.push(LayeredInstance::default());
        frame.overlay_mut().layer_mut(Blend::Add, 0.5, false).layered.push(LayeredInstance::default());
        let camera = CameraView { center: Vec2::new(10.0, 20.0), zoom: 0.5, turn: 0.25 };
        frame.set_view(camera, (300, 200));
        let l = &frame.layers[0].frame;
        assert_eq!((l.camera, l.width, l.height), (camera, 300, 200));
        let o = frame.overlay.as_deref().unwrap();
        assert_eq!((o.width, o.height, o.camera), (300, 200, CameraView::default()), "stuck things keep no camera");
        let ol = &o.layers[0].frame;
        assert_eq!((ol.width, ol.height, ol.camera), (300, 200, CameraView::default()));
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
            frame.look_layers.push(LayerGpu { paint, ..LayerGpu::ZERO });
            frame.layered.push(LayeredInstance { range: [0, 1, 0, 0], ..LayeredInstance::default() });
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
        let balls = testkit::push(&mut scene, "balls", ThingId::ROOT);
        scene.add_slot(&reg, balls, SlotId::Look, "look.plain").unwrap();

        let mut resolved = resolve(&scene, &reg);
        let r = std::sync::Arc::make_mut(resolved.things.get_mut(&balls).unwrap());
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
        assert_eq!((plain.layered.len(), plain.trails.len()), (1, 1));

        let frame = routed("multiply", 1.0, false, false);
        assert!(frame.layered.is_empty() && frame.trails.is_empty());
        assert_eq!(frame.layers.len(), 1, "the thing and its module share the layer");
        let layer = &frame.layers[0];
        assert_eq!((layer.blend, layer.amount), (Blend::Multiply, 1.0));
        assert_eq!((layer.frame.layered.len(), layer.frame.trails.len()), (1, 1));
        assert!(layer.frame.gradients.is_empty(), "a layer keeps no atlas of its own");
        let row = layer.frame.look_layers[0].paint.row as usize;
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
        assert_eq!((layer.frame.layered.len(), layer.frame.trails.len()), (1, 1));
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
        assert!(o.layered.is_empty());
        assert_eq!((o.layers.len(), o.layers[0].frame.layered.len()), (1, 1));
        assert!(frame.has_overlay());
        let row = o.layers[0].frame.look_layers[0].paint.row as usize;
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
        let balls = testkit::push(&mut scene, "balls", ThingId::ROOT);
        scene.add_slot(&reg, balls, SlotId::Look, "look.plain").unwrap();

        let mut resolved = resolve(&scene, &reg);
        let params = &mut std::sync::Arc::make_mut(resolved.things.get_mut(&balls).unwrap())
            .slots
            .get_mut(&SlotId::Look)
            .unwrap()
            .params;
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
    }

    #[test]
    fn blend_alpha_marks_additive_colours() {
        assert_eq!(blend_alpha([1.0, 0.0, 0.0, 0.0]), [1.0, 0.0, 0.0, -1.0]);
        assert_eq!(blend_alpha([0.0, 0.0, 0.0, 0.0]), [0.0; 4]);
        assert_eq!(blend_alpha([0.2, 0.0, 0.0, 1.0]), [0.2, 0.0, 0.0, 1.0]);
    }
}

#[cfg(test)]
mod tint_tests {
    use super::*;

    #[test]
    fn tint_bits_pack_into_the_high_bits() {
        assert_eq!(LayerGpu::tint_bits(1.0, 0.5), (64 << 16) | (32 << 24));
        assert_eq!(LayerGpu::tint_bits(-1.0, 9.0), 255 << 24);
        assert_eq!(LayerGpu::tint_bits(f32::NAN, 0.0), 0);
        for (t, d) in [(1.0, 0.0), (0.3, 0.7), (3.0, 3.0)] {
            assert_eq!(LayerGpu::tint_bits(t, d) & !LayerGpu::TINT_BITS, 0, "the flags' bits stay free");
        }
    }

    fn record(blocks: f32, opacity: f32) -> LayerGpu {
        let mut r = LayerGpu::ZERO;
        r.light[1] = blocks;
        r.amount[0] = opacity;
        r
    }

    #[test]
    fn only_see_through_blockers_ask_for_the_tint_picture() {
        let mut frame = Frame::default();
        assert!(!frame.see_through_blockers());
        frame.look_layers.push(record(1.0, 1.0));
        frame.look_layers.push(record(0.0, 0.3));
        assert!(!frame.see_through_blockers(), "solid, or not blocking at all");
        frame.look_layers.push(record(0.5, 1.0));
        assert!(frame.see_through_blockers());
        frame.look_layers.clear();
        frame.look_layers.push(record(1.0, 0.5));
        assert!(frame.see_through_blockers(), "a solid record at half opacity");
        let mut trails = Frame::default();
        trails.trails.push(TrailVertex { light: [1.0, 0.0], ..Default::default() });
        assert!(!trails.see_through_blockers(), "a trail that blocks no light");
        trails.trails.push(TrailVertex { light: [1.0, 1.0], ..Default::default() });
        assert!(trails.see_through_blockers());
        let mut layered = Frame::default();
        let mut inner = Frame::default();
        inner.particles.push(Particle { light: [1.0, 1.0, 0.0, 0.0], ..Default::default() });
        layered.layers.push(BlendLayer {
            blend: bpe_core::Blend::Add,
            amount: 1.0,
            each: false,
            z: 0,
            place: Place::Front,
            frame: inner,
        });
        assert!(layered.see_through_blockers(), "a blend layer's particle");
    }

    #[test]
    fn the_tint_pass_needs_glass_colour_a_light_and_something_see_through() {
        let mut frame = Frame::default();
        frame.look_layers.push(LayerGpu { light: [1.0, 0.5, 0.0, 0.0], ..LayerGpu::ZERO });
        let mut scene = LightScene::default();
        scene.settings.more[3] = 1.0;
        assert!(!scene.needs_tint(&frame), "no light");
        scene.lights.push(LightGpu::default());
        assert!(scene.needs_tint(&frame));
        scene.settings.more[3] = 0.0;
        assert!(!scene.needs_tint(&frame), "Coloured light through glass off");
        scene.settings.more[3] = 1.0;
        assert!(!scene.needs_tint(&Frame::default()), "nothing see-through");
    }
}

//! What the GPU reads per drawn thing: layer records, layered balls, path pieces, flashes and
//! hit marks.

use super::*;

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
    pub(super) fn code(self) -> u32 {
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

//! What the lighting reads from a frame: lights, surfaces, mirrors, glass spheres, throwers,
//! pipes and benders ([`LightScene`]), and what a lit preview shows ([`LightShow`]).

use super::*;

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

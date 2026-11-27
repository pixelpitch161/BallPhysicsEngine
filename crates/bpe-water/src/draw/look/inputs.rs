//! What the look drawing is told by the cards (Water mode part 6): each liquid's Look card, each
//! solid's Material, the Back, the Light card and the Lamps, at one moment. Colours are sRGB from 0
//! to 1 (as the cards hold them); gradients are [`GRADIENT_SAMPLES`] linear samples. `Default` is the
//! cards' defaults (the kit's `look_of` of a new scene matches them).

use glam::Vec2;

use crate::draw::plain::{PlainLiquid, PlainLook};

/// How many samples a gradient is given in.
pub const GRADIENT_SAMPLES: usize = 64;
/// The most lamps that light a Water scene (the first in the tree's order).
pub const MOST_LAMPS: usize = 16;

fn srgb(r: u8, g: u8, b: u8) -> [f32; 3] {
    [f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0]
}

/// What a liquid's colour is read from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColourFrom {
    #[default]
    ItsOwn,
    Speed,
    Depth,
}

/// What makes a liquid glow.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GlowFrom {
    #[default]
    Always,
    Speed,
}

/// One liquid's Look card.
#[derive(Clone, Debug, PartialEq)]
pub struct LiquidLook {
    pub colour: [f32; 3],
    pub colour_from: ColourFrom,
    /// Linear samples, read from 0 (slow, or at the surface) to 1 (fast, or deep).
    pub gradient: Vec<[f32; 3]>,
    /// World px (0: no depth, and opaque).
    pub clarity: f32,
    pub deep: [f32; 3],
    pub solid: f32,
    /// World px.
    pub bend: f32,
    pub split: f32,
    pub smoothing: f32,
    pub streaks: f32,
    pub shine: f32,
    pub shine_size: f32,
    pub edge_sheen: f32,
    pub ripples: f32,
    pub thin_film: f32,
    pub glow: f32,
    pub glow_colour: [f32; 3],
    pub glow_from: GlowFrom,
    /// World px (0: none).
    pub outline: f32,
    pub outline_colour: [f32; 3],
    pub bands: u32,
    pub band_softness: f32,
    /// Extras (part 6b), drawn from the moment: froth where it churns, its trapped air drawn as
    /// bubbles and small bubbles rising in it, mist round fast lone drops, sparks behind fast ones.
    pub foam: f32,
    /// World px.
    pub foam_size: f32,
    pub foam_colour: [f32; 3],
    pub bubbles: f32,
    /// World px.
    pub bubble_size: f32,
    pub bubble_colour: [f32; 3],
    /// Seconds a rising bubble lives.
    pub bubbles_life: f32,
    pub mist: f32,
    /// World px.
    pub mist_size: f32,
    pub mist_colour: [f32; 3],
    pub sparks: f32,
    /// World px.
    pub spark_size: f32,
    pub spark_colour: [f32; 3],
    /// A spark's trail, in seconds of its flight.
    pub sparks_life: f32,
}

impl Default for LiquidLook {
    /// The Look card's defaults.
    fn default() -> Self {
        LiquidLook {
            colour: srgb(0x2f, 0x9b, 0xff),
            colour_from: ColourFrom::ItsOwn,
            gradient: Vec::new(),
            clarity: 200.0,
            deep: srgb(0x0b, 0x2a, 0x4f),
            solid: 0.0,
            bend: 12.0,
            split: 0.0,
            smoothing: 0.5,
            streaks: 1.0,
            shine: 0.5,
            shine_size: 0.3,
            edge_sheen: 0.3,
            ripples: 0.0,
            thin_film: 0.0,
            glow: 0.0,
            glow_colour: [1.0; 3],
            glow_from: GlowFrom::Always,
            outline: 0.0,
            outline_colour: [0.0; 3],
            bands: 0,
            band_softness: 0.2,
            foam: 0.0,
            foam_size: 6.0,
            foam_colour: [1.0; 3],
            bubbles: 0.0,
            bubble_size: 6.0,
            bubble_colour: [1.0; 3],
            bubbles_life: 1.0,
            mist: 0.0,
            mist_size: 6.0,
            mist_colour: [1.0; 3],
            sparks: 0.0,
            spark_size: 6.0,
            spark_colour: srgb(0xff, 0xb0, 0x40),
            sparks_life: 1.0,
        }
    }
}

/// What a solid is made of.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MaterialKind {
    #[default]
    Rock,
    Glass,
    Paper,
    Basalt,
    Flat,
}

/// A solid's Material card (its Wetting is physics, not looks).
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialLook {
    pub kind: MaterialKind,
    pub colour: [f32; 3],
    pub second: [f32; 3],
    /// World px.
    pub texture: f32,
    /// World px.
    pub depth: f32,
    pub roughness: f32,
    /// World px (0: none).
    pub outline: f32,
    pub outline_colour: [f32; 3],
    pub wet_marks: f32,
}

impl Default for MaterialLook {
    /// The Material card's defaults.
    fn default() -> Self {
        MaterialLook {
            kind: MaterialKind::Rock,
            colour: srgb(0x6b, 0x5e, 0x52),
            second: srgb(0x3d, 0x35, 0x2f),
            texture: 120.0,
            depth: 12.0,
            roughness: 0.6,
            outline: 0.0,
            outline_colour: [0.0; 3],
            wet_marks: 0.5,
        }
    }
}

/// What shows behind the course.
#[derive(Clone, Debug, PartialEq)]
pub enum BackLook {
    /// The course's rock, this much darker (0 the same, 1 black).
    DarkerRock(f32),
    Colour([f32; 3]),
    /// Linear samples along `angle` (degrees; 90 runs down).
    Gradient {
        samples: Vec<[f32; 3]>,
        angle: f32,
    },
    /// A PNG, cover-fitted to the course: the one the scene keeps under the card's name, or else the
    /// file the name is the path of.
    Picture(BackPicture),
}

/// The Back's Picture: the name on the card, and the PNG's bytes when the scene keeps a picture
/// under it.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct BackPicture {
    pub name: String,
    pub kept: Option<std::sync::Arc<[u8]>>,
}

impl BackPicture {
    /// A PNG file at `path`.
    pub fn file(path: impl Into<String>) -> BackPicture {
        BackPicture { name: path.into(), kept: None }
    }
}

impl Default for BackLook {
    fn default() -> Self {
        BackLook::DarkerRock(0.5)
    }
}

/// How carefully the light is worked out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Quality {
    Draft,
    #[default]
    Good,
    Best,
}

impl Quality {
    /// The light map's texel, world px.
    pub fn light_texel(self) -> f32 {
        match self {
            Quality::Draft => 8.0,
            Quality::Good => 4.0,
            Quality::Best => 2.0,
        }
    }

    /// The spread light's base texel, world px.
    pub fn spread_texel(self) -> f32 {
        4.0 * self.light_texel()
    }
}

/// The Light card.
#[derive(Clone, Debug, PartialEq)]
pub struct LightLook {
    pub ambient: f32,
    pub bounce: f32,
    pub caustics: f32,
    pub shafts: f32,
    pub quality: Quality,
}

impl Default for LightLook {
    fn default() -> Self {
        LightLook { ambient: 1.0, bounce: 0.3, caustics: 1.0, shafts: 0.0, quality: Quality::Good }
    }
}

/// The kinds of lamp.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LampKind {
    #[default]
    Spot,
    Strip,
    Sun,
}

/// A Lamp at one moment.
#[derive(Clone, Debug, PartialEq)]
pub struct Lamp {
    pub kind: LampKind,
    /// World px (a Sun's isn't used).
    pub place: Vec2,
    /// Degrees: 90 shines straight down.
    pub aim: f32,
    pub colour: [f32; 3],
    pub brightness: f32,
    /// Degrees (a Spot's).
    pub spread: f32,
    /// World px (a Strip's).
    pub length: f32,
    pub softness: f32,
    pub shadows: bool,
}

impl Default for Lamp {
    /// The Lamp card's defaults, at the world's middle.
    fn default() -> Self {
        Lamp {
            kind: LampKind::Spot,
            place: Vec2::ZERO,
            aim: 90.0,
            colour: srgb(0xff, 0xe2, 0xb8),
            brightness: 1.0,
            spread: 60.0,
            length: 400.0,
            softness: 0.3,
            shadows: true,
        }
    }
}

/// The picture's finish: the Picture card's bloom and exposure, the scene's Effects over the
/// picture and its Grade, as a Balls frame's finish runs them (`bpe_gfx::Renderer::finish_picture`).
#[derive(Clone, Debug, PartialEq)]
pub struct FinishLook {
    pub bloom: f32,
    pub exposure: f32,
    /// Bottom first; only the calls at [`bpe_gfx::Stage::Picture`] run.
    pub effects: Vec<bpe_gfx::EffectCall>,
    pub grade: bpe_gfx::GradeStack,
    /// The video's size (the scene's Width and Height) when the picture is the camera's shot: the
    /// Effects and the Grade measure in the video's pixels (across: a picture of another shape is
    /// the video's width wide), so a smaller picture (the editor's, or an export at a lower scale)
    /// looks like the video scaled down. `(0, 0)`: the picture's own.
    pub video: (u32, u32),
}

impl Default for FinishLook {
    fn default() -> Self {
        FinishLook {
            bloom: 0.0,
            exposure: 1.0,
            effects: Vec::new(),
            grade: bpe_gfx::GradeStack::default(),
            video: (0, 0),
        }
    }
}

impl FinishLook {
    /// The finish without the Effects and the Grade, which are made for the shot's shape (the
    /// Water stage's Whole course view).
    pub fn without_shot_finish(&self) -> FinishLook {
        FinishLook { bloom: self.bloom, exposure: self.exposure, ..FinishLook::default() }
    }
}

/// Everything the look drawing is told at one moment.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Look {
    /// By kind (a kind past the list takes the list's last; none: [`LiquidLook::default`]), at most
    /// [`crate::drops::numbers::MOST_LIQUIDS`].
    pub liquids: Vec<LiquidLook>,
    /// The course's rock (owner 0 in the solid field).
    pub rock: MaterialLook,
    /// Owner `k` from 1 is `solids[k - 1]`; past the list, or past [`super::super::MOST_COLOURED`],
    /// the rock's.
    pub solids: Vec<MaterialLook>,
    pub back: BackLook,
    pub light: LightLook,
    /// At most [`MOST_LAMPS`] are drawn.
    pub lamps: Vec<Lamp>,
    /// Water seconds (ripples move on it).
    pub time: f32,
    pub finish: FinishLook,
}

impl Look {
    /// The plain drawing's inputs as a look: each liquid opaque from its Colour to its Deep colour,
    /// with no bend or shine; flat solids in their colours, a flat back, no lamps.
    pub fn plain(plain: &PlainLook) -> Look {
        let flat =
            |colour: [f32; 3]| MaterialLook { kind: MaterialKind::Flat, colour, depth: 0.0, ..MaterialLook::default() };
        let liquid = |l: &PlainLiquid| LiquidLook {
            colour: l.colour,
            deep: l.deep,
            clarity: l.clarity,
            solid: 1.0,
            bend: 0.0,
            shine: 0.0,
            edge_sheen: 0.0,
            streaks: 0.0,
            ..LiquidLook::default()
        };
        Look {
            liquids: plain.liquids.iter().map(liquid).collect(),
            rock: flat(plain.rock),
            solids: plain.solids.iter().copied().map(flat).collect(),
            back: BackLook::Colour(plain.back),
            ..Look::default()
        }
    }
}

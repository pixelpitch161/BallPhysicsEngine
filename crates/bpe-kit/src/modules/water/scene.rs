//! The cards on a Water scene's settings: **Water**, **Light** and **Back**.

use bpe_core::{Choice, Color, Gradient, Settings};
use glam::Vec2;

/// One screen of course, in pixels, whatever size the video is.
pub const SCREEN: Vec2 = Vec2::new(1080.0, 1920.0);
/// The most screens a course covers in all.
const MOST_SCREENS: u32 = 16;

/// How finely the water is worked out while editing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum DraftDetail {
    Coarse,
    #[default]
    Normal,
    Fine,
}

impl DraftDetail {
    /// The size of one cell of water, in pixels.
    pub fn cell_px(self) -> f32 {
        match self {
            DraftDetail::Coarse => 12.0,
            DraftDetail::Normal => 8.0,
            DraftDetail::Fine => 6.0,
        }
    }
}

/// How finely the water is worked out for the finished video.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum FinalDetail {
    Coarse,
    #[default]
    Normal,
    Fine,
}

impl FinalDetail {
    /// The size of one cell of water, in pixels.
    pub fn cell_px(self) -> f32 {
        match self {
            FinalDetail::Coarse => 3.0,
            FinalDetail::Normal => 2.0,
            FinalDetail::Fine => 1.5,
        }
    }
}

/// The look a new Water scene was started with. The looks themselves arrive in a later part;
/// until then this only remembers the pick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum StartingLook {
    #[default]
    Plain,
    LampLitRock,
    GlassLab,
    Neon,
    PaintOnPaper,
    Molten,
    Cartoon,
}

impl StartingLook {
    pub const ALL: [StartingLook; 7] = [
        StartingLook::Plain,
        StartingLook::LampLitRock,
        StartingLook::GlassLab,
        StartingLook::Neon,
        StartingLook::PaintOnPaper,
        StartingLook::Molten,
        StartingLook::Cartoon,
    ];

    /// The name on its tile.
    pub fn title(self) -> &'static str {
        match self {
            StartingLook::Plain => "Plain",
            StartingLook::LampLitRock => "Lamp-lit rock",
            StartingLook::GlassLab => "Glass lab",
            StartingLook::Neon => "Neon in the dark",
            StartingLook::PaintOnPaper => "Paint on paper",
            StartingLook::Molten => "Molten",
            StartingLook::Cartoon => "Cartoon",
        }
    }

    /// One line saying what it looks like.
    pub fn words(self) -> &'static str {
        match self {
            StartingLook::Plain => "Blue water in grey rock, nothing set up.",
            StartingLook::LampLitRock => "Clear water in warm rock, lit by a lamp from above.",
            StartingLook::GlassLab => "Coloured liquids in clean glass, bright and even.",
            StartingLook::Neon => "Glowing liquid in the dark, lighting the walls round it.",
            StartingLook::PaintOnPaper => "Thick paint that mixes and stains the paper.",
            StartingLook::Molten => "Lava that glows hot and crusts over as it cools.",
            StartingLook::Cartoon => "Flat colours with outlines and banded shading.",
        }
    }

    /// Its liquid's colour and its solid's colour, for the tile's picture.
    pub fn swatch(self) -> (Color, Color) {
        match self {
            StartingLook::Plain => (Color::rgb(0x2f, 0x9b, 0xff), Color::rgb(0x55, 0x5b, 0x66)),
            StartingLook::LampLitRock => (Color::rgb(0x4f, 0xb4, 0xe8), Color::rgb(0x7a, 0x5c, 0x44)),
            StartingLook::GlassLab => (Color::rgb(0x35, 0xd0, 0xa8), Color::rgb(0xc9, 0xd6, 0xe2)),
            StartingLook::Neon => (Color::rgb(0xff, 0x3d, 0xc8), Color::rgb(0x12, 0x10, 0x1e)),
            StartingLook::PaintOnPaper => (Color::rgb(0xe8, 0x4a, 0x3a), Color::rgb(0xef, 0xe7, 0xd6)),
            StartingLook::Molten => (Color::rgb(0xff, 0x7a, 0x18), Color::rgb(0x24, 0x1c, 0x1a)),
            StartingLook::Cartoon => (Color::rgb(0x3a, 0xa0, 0xff), Color::rgb(0xff, 0xd2, 0x4a)),
        }
    }
}

/// The Water card: the water of the whole scene.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct WaterWorld {
    /// Which way the water falls and how hard. Down is positive y.
    #[setting(label = "Gravity", unit = "px/s²", simple, keyframe, group = "Course")]
    pub gravity: Vec2,
    /// How many screens wide the course is. A course covers at most 16 screens in all.
    #[setting(label = "Screens wide", range(1, 8), simple, group = "Course")]
    pub screens_wide: u32,
    /// How many screens tall the course is. A course covers at most 16 screens in all.
    #[setting(label = "Screens tall", range(1, 8), simple, group = "Course")]
    pub screens_tall: u32,
    /// The look this scene was started with. The looks arrive in a later part of Water mode;
    /// until then this only remembers the pick.
    #[setting(label = "Starting look", group = "Course")]
    pub look: StartingLook,
    /// How finely the water is worked out while you edit: coarser plays faster.
    #[setting(label = "Draft detail", simple, group = "Detail")]
    pub draft_detail: DraftDetail,
    /// How finely the water is worked out for the finished video: finer takes longer to make.
    #[setting(label = "Final detail", simple, group = "Detail")]
    pub final_detail: FinalDetail,
    /// Air caught under the water (by a pour plunging in, or a hole carved under it) rises out as
    /// bubbles and bursts at the top. Off, it stays a hole until the water closes it.
    #[setting(label = "Trapped air", simple, group = "Trapped air")]
    pub trapped_air: bool,
    /// How fast trapped air rises: 1 is real air in water, more rises faster, 0 leaves it hanging.
    #[setting(
        label = "Bubbles rise",
        when = "trapped_air",
        range(0, 1e3),
        span(0, 4),
        unit = "×",
        keyframe,
        group = "Trapped air"
    )]
    pub bubble_rise: f32,
}

impl Default for WaterWorld {
    fn default() -> Self {
        Self {
            gravity: Vec2::new(0.0, 4000.0),
            screens_wide: 1,
            screens_tall: 1,
            look: StartingLook::Plain,
            draft_detail: DraftDetail::Normal,
            final_detail: FinalDetail::Normal,
            trapped_air: true,
            bubble_rise: 1.0,
        }
    }
}

impl WaterWorld {
    /// The course's size in screens, wide then tall: each 1 to 8, and no more than 16 in all
    /// (the height gives way).
    pub fn screens(&self) -> (u32, u32) {
        let wide = self.screens_wide.clamp(1, 8);
        let tall = self.screens_tall.clamp(1, 8).min(MOST_SCREENS / wide);
        (wide, tall)
    }

    /// The course's size in pixels. It is centred on the world's middle.
    pub fn course_size(&self) -> Vec2 {
        let (wide, tall) = self.screens();
        Vec2::new(SCREEN.x * wide as f32, SCREEN.y * tall as f32)
    }
}

/// How carefully the water's light is worked out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum LightQuality {
    Draft,
    #[default]
    Good,
    Best,
}

/// The Light card: the light the lamps don't give.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct WaterLightCard {
    /// Light that is everywhere, lamp or no lamp.
    #[setting(label = "Ambient", range(0, 1e9), span(0, 2), simple, keyframe, group = "Light")]
    pub ambient: f32,
    /// How much light bounces off the rock and the water onto what is near.
    #[setting(label = "Bounce", range(0, 1e9), span(0, 1), keyframe, group = "Light")]
    pub bounce: f32,
    /// The bright rippling patterns water throws on what is under it.
    #[setting(label = "Caustics", range(0, 1e9), span(0, 2), simple, keyframe, group = "Light")]
    pub caustics: f32,
    /// Beams of light showing in the water and the air.
    #[setting(label = "Shafts", range(0, 1e9), span(0, 1), keyframe, group = "Light")]
    pub shafts: f32,
    /// How carefully the light is worked out: Draft is quickest, Best is for the finished video.
    #[setting(label = "Quality", group = "Quality")]
    pub quality: LightQuality,
}

impl Default for WaterLightCard {
    fn default() -> Self {
        Self { ambient: 0.3, bounce: 0.3, caustics: 1.0, shafts: 0.0, quality: LightQuality::Good }
    }
}

/// What shows where the course is carved away.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum BackShows {
    /// The course's own rock, darker, as if further back.
    #[default]
    DarkerRock,
    Colour,
    Gradient,
    Picture,
}

/// The Back card: what is behind the water.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct BackCard {
    #[setting(label = "Shows", simple, group = "Back")]
    pub shows: BackShows,
    /// How much darker than the rock in front: 0 the same, 1 black.
    #[setting(label = "Darker", when = "shows=darker_rock", range(0, 1), span(0, 1), keyframe, group = "Back")]
    pub darker: f32,
    #[setting(label = "Colour", when = "shows=colour", keyframe, group = "Back")]
    pub colour: Color,
    #[setting(label = "Gradient", when = "shows=gradient", keyframe, group = "Back")]
    pub gradient: Gradient,
    /// Which way the gradient runs.
    #[setting(label = "Direction", when = "shows=gradient", span(-180, 180), unit = "°", group = "Back")]
    pub angle: f32,
    /// A PNG picture shown behind the course.
    #[setting(label = "Picture", when = "shows=picture", unit = "picture", group = "Back")]
    pub picture: String,
}

impl Default for BackCard {
    fn default() -> Self {
        Self {
            shows: BackShows::DarkerRock,
            darker: 0.5,
            colour: Color::rgb(0x14, 0x18, 0x1e),
            gradient: Gradient::even(&[Color::rgb(0x0e, 0x1c, 0x33), Color::rgb(0x05, 0x07, 0x0b)]),
            angle: 90.0,
            picture: String::new(),
        }
    }
}

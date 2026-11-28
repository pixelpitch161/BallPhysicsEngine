//! The Water card on a Water scene's settings (its **Light** and **Back** cards are in [`super::look`]).

use bpe_core::{Choice, Settings};
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
    /// How finely the water is worked out while you edit: coarser plays faster.
    #[setting(label = "Draft detail", simple, group = "Detail")]
    pub draft_detail: DraftDetail,
    /// How finely the water is worked out for the finished video: finer takes longer to make.
    #[setting(label = "Final detail", simple, group = "Detail")]
    pub final_detail: FinalDetail,
    /// Air caught under the water (by a pour plunging in, or a hole carved under it) rises out as
    /// bubbles and bursts at the top. Off, it stays a hole until the water closes it.
    #[setting(label = "Trapped air", group = "Trapped air")]
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
    /// How fast the water plays in the video: 1 is real time, 0.25 plays it four times slower (the
    /// final keeps four times the frames there, so it stays smooth), 2 twice as fast. Keyframe it
    /// to ramp into slow motion and out again. Slow motion makes the video longer.
    #[setting(label = "Speed", range(0.125, 4), span(0.125, 2), unit = "×", simple, keyframe, group = "Slow motion")]
    pub speed: f32,
    /// When the video reaches To, it plays the water backwards to From, so the colours that ran
    /// together come apart again, then forwards again from From and on to the end. Nothing is
    /// worked out again: the video plays the final's own frames, and it is twice the stretch longer.
    #[setting(label = "Un-mix", group = "Un-mix")]
    pub unmix: bool,
    #[setting(label = "From", when = "unmix", range(0, 600), span(0, 30), unit = "s", group = "Un-mix")]
    pub unmix_from: f32,
    #[setting(label = "To", when = "unmix", range(0, 600), span(0, 30), unit = "s", group = "Un-mix")]
    pub unmix_to: f32,
}

impl Default for WaterWorld {
    fn default() -> Self {
        Self {
            gravity: Vec2::new(0.0, 4000.0),
            screens_wide: 1,
            screens_tall: 1,
            draft_detail: DraftDetail::Normal,
            final_detail: FinalDetail::Normal,
            trapped_air: true,
            bubble_rise: 1.0,
            speed: 1.0,
            unmix: false,
            unmix_from: 2.0,
            unmix_to: 5.0,
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

//! Scene defaults: the root of every scene. Its settings are the video, the world and the song,
//! and its slots are what every thing uses unless it sets its own.

pub mod sim;

use bpe_core::{Color, SettingDef, Settings, SlotId, ThingKind};
use glam::Vec2;

pub struct SceneKind;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct SceneSettings {
    /// How long the video runs.
    #[setting(label = "Length", range(1, 600), unit = "s", simple)]
    pub duration: f32,
    /// How hard everything is pulled, and which way. Down is positive y.
    #[setting(label = "Gravity", unit = "px/s²", simple, keyframe, random)]
    pub gravity: Vec2,
    /// The colour at the top of the background.
    #[setting(label = "Background top", simple, keyframe, random)]
    pub background_top: Color,
    /// The colour at the bottom of the background.
    #[setting(label = "Background bottom", simple, keyframe, random)]
    pub background_bottom: Color,
    /// How much bright things bleed light around them.
    #[setting(label = "Bloom", range(0, 1), simple, keyframe, random, pulse)]
    pub bloom: f32,
    /// Chord names with how many seconds each lasts, repeating.
    #[setting(label = "Chords", simple, group = "Sound")]
    pub chords: String,
    /// The video's width in pixels.
    #[setting(label = "Width", range(2, 7680), unit = "px", group = "Video")]
    pub width: u32,
    /// The video's height in pixels.
    #[setting(label = "Height", range(2, 7680), unit = "px", group = "Video")]
    pub height: u32,
    /// Frames per second.
    #[setting(label = "Frame rate", range(1, 240), unit = "fps", group = "Video")]
    pub fps: u32,
    /// How many times a second the physics moves everything.
    #[setting(label = "Physics rate", range(60, 2000), unit = "per s", group = "Video")]
    pub steps_per_second: u32,
    /// How bright the whole picture is.
    #[setting(label = "Exposure", range(0.1, 4), keyframe, pulse)]
    pub exposure: f32,
    /// How loud the finished soundtrack is.
    #[setting(label = "Loudness", range(-30, -6), unit = "dB", group = "Sound")]
    pub loudness: f32,
    /// Which of the chord's notes each hit picks. Change it for a different tune.
    #[setting(label = "Seed", range(0, 1000000), group = "Sound", random)]
    pub seed: u32,
}

impl Default for SceneSettings {
    fn default() -> Self {
        Self {
            duration: 20.0,
            gravity: Vec2::new(0.0, 500.0),
            background_top: Color::rgb(0x02, 0x02, 0x2a),
            background_bottom: Color::rgb(0x01, 0x00, 0x06),
            bloom: 0.15,
            chords: "C 4.2, Dsus4 4, Am 5, C 4, Am 3.1, Dsus4 3".into(),
            width: 1080,
            height: 1920,
            fps: 60,
            steps_per_second: 240,
            exposure: 1.0,
            loudness: -14.0,
            seed: 1,
        }
    }
}

impl ThingKind for SceneKind {
    fn id(&self) -> &'static str {
        "scene"
    }
    fn name(&self) -> &'static str {
        "Scene defaults"
    }
    fn icon(&self) -> &'static str {
        "scene"
    }
    fn help(&self) -> &'static str {
        "The video, the world and the song, and the modules every thing uses unless it sets its own."
    }
    fn settings(&self) -> &'static [SettingDef] {
        SceneSettings::defs()
    }
    fn slots(&self) -> &'static [SlotId] {
        &SlotId::ALL
    }
    fn default_slots(&self) -> &'static [SlotId] {
        &SlotId::ALL
    }
}

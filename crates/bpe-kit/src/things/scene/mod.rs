//! Scene defaults: the root of every scene. Its settings are the video, the world and the song,
//! and its slots are what every thing uses unless it sets its own.

pub mod draw;
pub mod sim;

pub use sim::{SceneSim, SceneState};

use bpe_core::{Choice, Color, ProgressionRef, SettingDef, Settings, SlotId, ThingKind};
use glam::Vec2;

pub struct SceneKind;

/// How hits and the beat are brought together.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum BeatSync {
    /// The video runs at its own pace.
    #[default]
    Off,
    /// The video's clock is gently stretched between hits so they land on beats; the physics
    /// is left alone.
    Warp,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct SceneSettings {
    /// How long the video runs.
    #[setting(label = "Length", range(1, 600), unit = "s", simple)]
    pub duration: f32,
    /// How hard everything is pulled, and which way. Down is positive y.
    #[setting(label = "Gravity", unit = "px/s²", simple, keyframe)]
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
    /// The chord progression sound modules play from unless they pick their own.
    #[setting(label = "Chords", simple, random, group = "Sound")]
    pub chords: ProgressionRef,
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
    /// A song file (mp3, wav, ogg or flac) played under the sounds, whose beats the scene can
    /// follow.
    #[setting(label = "Song", unit = "file", group = "Song")]
    pub song: String,
    /// How far into the song the video starts. Negative starts the song late.
    #[setting(label = "Song starts at", when = "song", range(-60, 60), unit = "s", group = "Song")]
    pub song_offset: f32,
    #[setting(label = "Song volume", when = "song", range(-40, 6), unit = "dB", group = "Song")]
    pub song_volume: f32,
    /// How much the song dips while the hit sounds play.
    #[setting(label = "Ducks under hits", when = "song", range(0, 1), group = "Song")]
    pub ducking: f32,
    /// A steady beat; 0 follows the song's beats, or 120 without a song.
    #[setting(label = "Tempo", range(0, 300), unit = "bpm", group = "Beat")]
    pub bpm: f32,
    #[setting(label = "Beat sync", group = "Beat")]
    pub beat_sync: BeatSync,
    /// Hits may land on halves or quarters of a beat too.
    #[setting(label = "Beats split into", range(1, 4), group = "Beat")]
    pub subdivisions: u32,
    /// How much faster or slower than real time the video may run between hits.
    #[setting(label = "Stretch", range(0, 0.9), group = "Beat")]
    pub warp_strength: f32,
}

impl Default for SceneSettings {
    fn default() -> Self {
        Self {
            duration: 20.0,
            gravity: Vec2::new(0.0, 500.0),
            background_top: Color::rgb(0x02, 0x02, 0x2a),
            background_bottom: Color::rgb(0x01, 0x00, 0x06),
            bloom: 0.15,
            chords: ProgressionRef(None),
            width: 1080,
            height: 1920,
            fps: 60,
            steps_per_second: 240,
            exposure: 1.0,
            loudness: -14.0,
            seed: 1,
            song: String::new(),
            song_offset: 0.0,
            song_volume: -6.0,
            ducking: 0.35,
            bpm: 0.0,
            beat_sync: BeatSync::Off,
            subdivisions: 1,
            warp_strength: 0.5,
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

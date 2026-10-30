//! Scene defaults: the root of every scene. Its settings are the video, the world and the song,
//! and its slots are what every thing uses unless it sets its own.

pub mod draw;
pub mod sim;

pub use sim::{SceneSim, SceneState};

use crate::paint::Paint;
use bpe_core::{Choice, Color, Gradient, ProgressionRef, SettingDef, Settings, SlotId, ThingKind};
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

/// The master stage's clipper.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Clip {
    #[default]
    Off,
    /// Rounds the peaks off.
    Soft,
    /// Cuts the peaks flat.
    Hard,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct SceneSettings {
    /// How long the video runs.
    #[setting(label = "Length", range(1, 600), unit = "s", simple)]
    pub duration: f32,
    /// How hard everything is pulled, and which way. Down is positive y.
    #[setting(label = "Gravity", unit = "px/s²", simple, keyframe)]
    pub gravity: Vec2,
    /// The colours behind everything, at any angle or spreading from a centre.
    #[setting(
        paint,
        label = "Background",
        prefix = "background_",
        follows = "place|time|beat",
        screen_only,
        simple,
        no_vary
    )]
    pub background: Paint,
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
    /// Keep the background still on the screen while the camera moves; off paints it across
    /// the world instead.
    #[setting(label = "Background stays on screen", group = "Video")]
    pub background_on_screen: bool,
    /// Frames per second.
    #[setting(label = "Frame rate", range(1, 240), unit = "fps", group = "Video")]
    pub fps: u32,
    /// How many times a second the physics moves everything.
    #[setting(label = "Physics rate", range(60, 2000), unit = "per s", group = "Video")]
    pub steps_per_second: u32,
    /// How bright the whole picture is.
    #[setting(label = "Exposure", range(0.1, 4), keyframe, pulse)]
    pub exposure: f32,
    /// How loud the finished soundtrack is, measured as real loudness (LUFS). -14 is the usual
    /// level for TikTok, Reels and Shorts.
    #[setting(label = "Loudness", range(-30, -6), unit = "LUFS", group = "Master")]
    pub loudness: f32,
    /// Shaves the peaks off before the limiter, for a louder, harder sound.
    #[setting(label = "Clipper", group = "Master")]
    pub clipper: Clip,
    /// How hard the clipper is pushed.
    #[setting(label = "Clipper drive", range(0, 24), unit = "dB", when = "clipper=soft|hard", group = "Master")]
    pub clip_drive: f32,
    /// The loudest the soundtrack ever gets, between samples too.
    #[setting(label = "Ceiling", range(-6, 0), unit = "dBTP", group = "Master")]
    pub ceiling: f32,
    /// Lets the limiter recover slower on dense music and faster on sparse hits.
    #[setting(label = "Auto release", group = "Master")]
    pub auto_release: bool,
    /// How fast the limiter recovers.
    #[setting(label = "Release", range(10, 1000), unit = "ms", when = "!auto_release", group = "Master")]
    pub release: f32,
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
    /// How far past the edge of the shot a hit fades to silence, in shot widths; 0 plays every
    /// hit at full volume wherever it is.
    #[setting(label = "Off-shot fade", range(0, 5), unit = "shot widths", group = "Camera sound")]
    pub off_shot_fade: f32,
}

impl Default for SceneSettings {
    fn default() -> Self {
        Self {
            duration: 20.0,
            gravity: Vec2::new(0.0, 500.0),
            background: Paint {
                colours: Gradient::even(&[Color::rgb(0x02, 0x02, 0x2a), Color::rgb(0x01, 0x00, 0x06)]),
                ..Paint::default()
            },
            bloom: 0.15,
            chords: ProgressionRef(None),
            width: 1080,
            height: 1920,
            fps: 60,
            steps_per_second: 240,
            exposure: 1.0,
            loudness: -14.0,
            clipper: Clip::Off,
            clip_drive: 0.0,
            ceiling: -1.0,
            auto_release: true,
            release: 100.0,
            seed: 1,
            song: String::new(),
            song_offset: 0.0,
            song_volume: -6.0,
            ducking: 0.35,
            bpm: 0.0,
            beat_sync: BeatSync::Off,
            subdivisions: 1,
            warp_strength: 0.5,
            background_on_screen: true,
            off_shot_fade: 0.0,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_background_is_one_paint_that_follows_place_time_or_beat_on_the_screen() {
        let defs = SceneSettings::defs();
        let keys: Vec<_> = defs.iter().map(|d| d.key).collect();
        for gone in ["background_top", "background_bottom", "background_space"] {
            assert!(!keys.contains(&gone), "{gone}");
        }
        assert_eq!(keys.iter().filter(|k| **k == "background_on_screen").count(), 1);
        for key in ["background_colours", "background_follow", "background_angle", "background_span"] {
            assert!(keys.contains(&key), "{key}");
        }
        let follow = defs.iter().find(|d| d.key == "background_follow").unwrap();
        assert_eq!(follow.ty, bpe_core::ValueType::Choice(&["place", "time", "beat"]));
        let d = SceneSettings::default().background;
        assert_eq!((d.angle, d.span, d.centre), (90.0, 1920.0, Vec2::ZERO));
        assert_eq!(d.colours.stops.len(), 2);
    }
}

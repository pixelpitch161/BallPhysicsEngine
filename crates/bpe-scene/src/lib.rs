//! Scene description: everything that defines one video.
//!
//! A scene is plain data, saved as RON. The simulation, renderer, audio mixer and
//! exporter all read from it, and nothing else. World coordinates are pixels of the
//! output video, with the origin at the centre of the frame and y pointing down.

mod color;

pub use color::Color;
pub use glam::Vec2;

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Scene {
    pub name: String,
    pub video: Video,
    pub physics: Physics,
    pub container: Container,
    pub spawners: Vec<Spawner>,
    pub rules: BounceRules,
    pub look: Look,
    pub sound: Sound,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Video {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Length of the clip in seconds.
    pub duration: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Physics {
    /// Pixels per second squared. Positive y pulls down.
    pub gravity: Vec2,
    /// Fixed simulation rate. Higher is more precise; 240 is plenty for most scenes.
    pub steps_per_second: u32,
    /// Fraction of speed kept along the wall normal on each bounce. 1.0 is perfectly elastic.
    pub restitution: f32,
    /// Fraction of velocity lost per second to air resistance.
    pub air_drag: f32,
    /// Whether balls bounce off each other. Off means they pass through one another.
    pub ball_collisions: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Container {
    Circle { center: Vec2, radius: f32, thickness: f32, color: Color },
    Rect { center: Vec2, size: Vec2, thickness: f32, color: Color },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Spawner {
    /// A rectangular block of balls, coloured top to bottom by a gradient.
    Grid {
        center: Vec2,
        columns: u32,
        rows: u32,
        spacing: f32,
        radius: f32,
        velocity: Vec2,
        /// Colours from the top row to the bottom row, spread evenly.
        gradient: Vec<Color>,
    },
    Single {
        position: Vec2,
        velocity: Vec2,
        radius: f32,
        color: Color,
    },
}

/// Changes applied to a ball each time it hits the wall.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BounceRules {
    /// Speed multiplier per wall hit. 1.0 leaves speed unchanged.
    pub speed_gain: f32,
    /// Speed cap in pixels per second, so speed gain cannot run away.
    pub max_speed: f32,
    /// Pixels added to the radius per wall hit.
    pub grow: f32,
    pub max_radius: f32,
    /// Colours a ball steps through on each wall hit. Empty keeps its colour.
    pub recolor: Vec<Color>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Look {
    pub background_top: Color,
    pub background_bottom: Color,
    /// How far back in time each ball's streak reaches, in seconds. 0 disables trails.
    pub trail_seconds: f32,
    /// Streak width at the ball, as a fraction of the ball diameter.
    pub trail_width: f32,
    /// Streak brightness at the ball, 0 to 1.
    pub trail_opacity: f32,
    /// Brightness of the soft halo around each ball.
    pub ball_glow: f32,
    /// Strength of the light flash where a ball hits the wall.
    pub flash_strength: f32,
    pub flash_seconds: f32,
    pub flash_size: f32,
    /// How much bright areas bleed light around them.
    pub bloom_strength: f32,
    pub exposure: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sound {
    pub enabled: bool,
    /// Chords played in order, then repeated. Notes for each wall hit come from the current chord.
    pub progression: Vec<ChordStep>,
    /// Lowest and highest MIDI note to pick from (60 is middle C).
    pub low_note: u8,
    pub high_note: u8,
    pub instrument: Instrument,
    /// Wall hits slower than this (pixels per second along the wall normal) make no sound.
    pub min_impact_speed: f32,
    /// Impact speed that plays at full note volume.
    pub full_volume_speed: f32,
    /// How far notes are panned left and right by where the hit happens. 0 is mono.
    pub stereo_width: f32,
    /// Overall loudness target in LUFS-like dB (about -14 for social media).
    pub target_loudness: f32,
    /// Seed for picking notes, so the same scene always sounds the same.
    pub seed: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChordStep {
    /// Chord name such as "C", "Am", "G7", "Dsus4", "Fmaj7".
    pub chord: String,
    pub seconds: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Instrument {
    /// A soft synthesised pluck. `decay` is seconds to fade by 60 dB, `brightness` 0 to 1.
    Pluck { decay: f32, brightness: f32 },
    /// A WAV file played at the pitch of `root_note` and pitched up or down from there.
    Sample { path: String, root_note: u8 },
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            name: "Reference reel".into(),
            video: Video::default(),
            physics: Physics::default(),
            container: Container::Circle {
                center: Vec2::ZERO,
                radius: 508.0,
                thickness: 3.0,
                color: Color::rgb(0xd8, 0xdc, 0xd0),
            },
            spawners: vec![Spawner::Grid {
                center: Vec2::new(37.0, -127.0),
                columns: 28,
                rows: 28,
                spacing: 19.0,
                radius: 4.5,
                velocity: Vec2::new(-480.0, 265.0),
                gradient: vec![
                    Color::rgb(0x3e, 0x9b, 0xff),
                    Color::rgb(0x8a, 0x4d, 0xff),
                    Color::rgb(0xff, 0x3e, 0xc8),
                    Color::rgb(0xff, 0x6a, 0x5c),
                    Color::rgb(0xff, 0xb8, 0x2e),
                ],
            }],
            rules: BounceRules::default(),
            look: Look::default(),
            sound: Sound::default(),
        }
    }
}

impl Default for Video {
    fn default() -> Self {
        Self { width: 1080, height: 1920, fps: 60, duration: 23.0 }
    }
}

impl Default for Physics {
    fn default() -> Self {
        Self {
            gravity: Vec2::new(0.0, 160.0),
            steps_per_second: 240,
            restitution: 1.0,
            air_drag: 0.0,
            ball_collisions: false,
        }
    }
}

impl Default for BounceRules {
    fn default() -> Self {
        Self { speed_gain: 1.0, max_speed: 3000.0, grow: 0.0, max_radius: 60.0, recolor: Vec::new() }
    }
}

impl Default for Look {
    fn default() -> Self {
        Self {
            background_top: Color::rgb(0x02, 0x02, 0x2a),
            background_bottom: Color::rgb(0x01, 0x00, 0x06),
            trail_seconds: 0.3,
            trail_width: 0.3,
            trail_opacity: 0.4,
            ball_glow: 0.12,
            flash_strength: 0.4,
            flash_seconds: 0.25,
            flash_size: 26.0,
            bloom_strength: 0.15,
            exposure: 1.0,
        }
    }
}

impl Default for Sound {
    fn default() -> Self {
        let step = |chord: &str, seconds| ChordStep { chord: chord.into(), seconds };
        Self {
            enabled: true,
            progression: vec![
                step("C", 4.2),
                step("Dsus4", 4.0),
                step("Am", 5.0),
                step("C", 4.0),
                step("Am", 3.1),
                step("Dsus4", 3.0),
            ],
            low_note: 48,
            high_note: 84,
            instrument: Instrument::Pluck { decay: 0.9, brightness: 0.5 },
            min_impact_speed: 20.0,
            full_volume_speed: 600.0,
            stereo_width: 0.6,
            target_loudness: -14.0,
            seed: 1,
        }
    }
}

impl Scene {
    /// Built-in starting points shown in the editor's Presets menu.
    pub fn presets() -> Vec<(&'static str, Scene)> {
        vec![("Ball grid in a ring (reference reel)", Scene::default()), ("Growing ball", Scene::growing_ball())]
    }

    /// One ball in a ring that grows and speeds up a little on every bounce.
    pub fn growing_ball() -> Self {
        Self {
            name: "Growing ball".into(),
            physics: Physics { gravity: Vec2::new(0.0, 900.0), ..Physics::default() },
            spawners: vec![Spawner::Single {
                position: Vec2::new(-120.0, -200.0),
                velocity: Vec2::new(420.0, 0.0),
                radius: 18.0,
                color: Color::rgb(0xff, 0x5c, 0xa8),
            }],
            rules: BounceRules { speed_gain: 1.02, grow: 2.5, max_radius: 300.0, ..BounceRules::default() },
            look: Look { trail_seconds: 0.5, trail_width: 0.9, trail_opacity: 0.5, ball_glow: 0.3, ..Look::default() },
            sound: Sound {
                progression: vec![
                    ChordStep { chord: "Am".into(), seconds: 2.0 },
                    ChordStep { chord: "F".into(), seconds: 2.0 },
                    ChordStep { chord: "C".into(), seconds: 2.0 },
                    ChordStep { chord: "G".into(), seconds: 2.0 },
                ],
                low_note: 57,
                high_note: 81,
                instrument: Instrument::Pluck { decay: 1.4, brightness: 0.45 },
                full_volume_speed: 1200.0,
                ..Sound::default()
            },
            video: Video { duration: 20.0, ..Video::default() },
            ..Self::default()
        }
    }

    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path.as_ref())?;
        Self::from_ron(&text)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        std::fs::write(path.as_ref(), self.to_ron()?)?;
        Ok(())
    }

    pub fn from_ron(text: &str) -> anyhow::Result<Self> {
        Ok(ron::from_str(text)?)
    }

    pub fn to_ron(&self) -> anyhow::Result<String> {
        let pretty = ron::ser::PrettyConfig::new().struct_names(true);
        Ok(ron::ser::to_string_pretty(self, pretty)?)
    }

    /// Number of video frames in the clip.
    pub fn frame_count(&self) -> u32 {
        (self.video.duration * self.video.fps as f32).round().max(1.0) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ron_round_trip() {
        let scene = Scene::default();
        let text = scene.to_ron().unwrap();
        assert_eq!(Scene::from_ron(&text).unwrap(), scene);
    }

    #[test]
    fn missing_fields_use_defaults() {
        let scene = Scene::from_ron("(name: \"Sparse\")").unwrap();
        assert_eq!(scene.name, "Sparse");
        assert_eq!(scene.video, Video::default());
    }
}

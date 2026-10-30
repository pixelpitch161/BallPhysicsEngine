//! Drum machine: every hit plays a drum, picked by how hard the hit was, and beats can play
//! one too.

pub mod mix;

use bpe_core::{Choice, ModuleKind, SettingDef, Settings, SlotId};

pub struct DrumSoundKind;

/// A piece of the kit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Piece {
    /// Picked by the hit's speed (hi-hat, snare, kick), or by the beat's place in the bar.
    #[default]
    Auto,
    Kick,
    Snare,
    HiHat,
    OpenHat,
    Clap,
    Rim,
    Tom,
    Crash,
    Ride,
    /// Plays nothing.
    Silent,
}

impl Piece {
    /// The General MIDI drum key of the piece. Auto is the closed hi-hat; Silent has none.
    pub fn key(self) -> Option<u8> {
        Some(match self {
            Piece::Auto | Piece::HiHat => 42,
            Piece::Kick => 36,
            Piece::Snare => 38,
            Piece::OpenHat => 46,
            Piece::Clap => 39,
            Piece::Rim => 37,
            Piece::Tom => 45,
            Piece::Crash => 49,
            Piece::Ride => 51,
            Piece::Silent => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct DrumSound {
    /// What a hit plays.
    #[setting(label = "Drum", simple, random)]
    pub piece: Piece,
    #[setting(label = "Volume", range(-40, 6), unit = "dB", simple, keyframe)]
    pub volume: f32,
    /// Where the channel sits from left (-1) to right (1), on top of each hit's own place.
    #[setting(label = "Pan", range(-1, 1), keyframe)]
    pub pan: f32,
    /// What every beat plays, when the scene has a tempo or a song.
    #[setting(label = "On beats", simple, random)]
    pub on_beat: Piece,
    #[setting(label = "Beat volume", range(-40, 6), unit = "dB", simple)]
    pub beat_volume: f32,
    /// Hits slower than this are silent.
    #[setting(label = "Quietest hit", range(0, 3000), unit = "px/s")]
    pub quietest_hit: f32,
    /// Hits this fast play at full strength.
    #[setting(label = "Loudest hit", range(1, 10000), unit = "px/s")]
    pub loudest_hit: f32,
    /// How far left and right drums spread, following where the hit was.
    #[setting(label = "Stereo width", range(0, 1))]
    pub stereo_width: f32,
}

impl Default for DrumSound {
    fn default() -> Self {
        Self {
            piece: Piece::Auto,
            volume: 0.0,
            pan: 0.0,
            on_beat: Piece::Silent,
            beat_volume: -3.0,
            quietest_hit: 20.0,
            loudest_hit: 600.0,
            stereo_width: 0.5,
        }
    }
}

impl ModuleKind for DrumSoundKind {
    fn id(&self) -> &'static str {
        "sound.drums"
    }
    fn name(&self) -> &'static str {
        "Drum machine"
    }
    fn help(&self) -> &'static str {
        "Every hit plays a drum, and beats can play one too."
    }
    fn slot(&self) -> SlotId {
        SlotId::Sound
    }
    fn settings(&self) -> &'static [SettingDef] {
        DrumSound::defs()
    }
}

//! Notes: every hit plays a note of the current chord on an instrument.

pub mod mix;

use bpe_core::{ModuleKind, SettingDef, Settings, SlotId};
use bpe_mix::Instrument;

pub struct NoteSoundKind;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct NoteSound {
    /// What plays the note.
    #[setting(label = "Instrument", simple, random)]
    pub instrument: Instrument,
    /// How loud these notes are.
    #[setting(label = "Volume", range(-40, 6), unit = "dB", simple, keyframe)]
    pub volume: f32,
    /// The lowest MIDI note a hit may play.
    #[setting(label = "Lowest note", range(0, 127), simple)]
    pub low_note: u32,
    /// The highest MIDI note a hit may play.
    #[setting(label = "Highest note", range(0, 127), simple)]
    pub high_note: u32,
    /// Hits slower than this are silent.
    #[setting(label = "Quietest hit", range(0, 3000), unit = "px/s")]
    pub quietest_hit: f32,
    /// Hits this fast play at full strength.
    #[setting(label = "Loudest hit", range(1, 10000), unit = "px/s")]
    pub loudest_hit: f32,
    /// How far left and right notes spread, following where the hit was.
    #[setting(label = "Stereo width", range(0, 1))]
    pub stereo_width: f32,
    /// Pluck only.
    #[setting(label = "Decay", range(0.05, 5), unit = "s")]
    pub decay: f32,
    /// Pluck only.
    #[setting(label = "Brightness", range(0, 1))]
    pub brightness: f32,
}

impl Default for NoteSound {
    fn default() -> Self {
        Self {
            instrument: Instrument::Pluck,
            volume: 0.0,
            low_note: 48,
            high_note: 84,
            quietest_hit: 20.0,
            loudest_hit: 600.0,
            stereo_width: 0.6,
            decay: 0.9,
            brightness: 0.5,
        }
    }
}

impl ModuleKind for NoteSoundKind {
    fn id(&self) -> &'static str {
        "sound.notes"
    }
    fn name(&self) -> &'static str {
        "Notes"
    }
    fn help(&self) -> &'static str {
        "Every hit plays a note of the current chord."
    }
    fn slot(&self) -> SlotId {
        SlotId::Sound
    }
    fn settings(&self) -> &'static [SettingDef] {
        NoteSound::defs()
    }
}

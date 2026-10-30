//! Melody: every hit plays the next step of a tune read from a MIDI file.

pub mod mix;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use bpe_core::{InstrumentRef, ModuleKind, SettingDef, Settings, SlotId};
use bpe_music::midi::MidiFile;

pub struct MelodySoundKind;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct MelodySound {
    /// A MIDI file, or one of the tunes that come with the program (`builtin:twinkle`,
    /// `builtin:ode-to-joy`, `builtin:fur-elise`).
    #[setting(label = "Tune", unit = "file", simple)]
    pub file: String,
    #[setting(label = "Instrument", simple, random)]
    pub instrument: InstrumentRef,
    #[setting(label = "Volume", range(-40, 6), unit = "dB", simple, keyframe)]
    pub volume: f32,
    /// Semitones up or down.
    #[setting(label = "Transpose", range(-24, 24), unit = "semitones", simple)]
    pub transpose: i32,
    /// Which part of the file to play; 0 is the busiest part.
    #[setting(label = "Part", range(0, 64))]
    pub track: u32,
    /// Notes that start together in the file play together on one hit.
    #[setting(label = "Play chords together")]
    pub chords: bool,
    /// Loudness follows how hard the hit was, rather than the file's.
    #[setting(label = "Velocity from hit")]
    pub velocity_from_hit: bool,
    /// Hits slower than this are silent.
    #[setting(label = "Quietest hit", range(0, 3000), unit = "px/s")]
    pub quietest_hit: f32,
    /// Hits this fast play at full strength.
    #[setting(label = "Loudest hit", range(1, 10000), unit = "px/s")]
    pub loudest_hit: f32,
    /// How far left and right notes spread, following where the hit was.
    #[setting(label = "Stereo width", range(0, 1))]
    pub stereo_width: f32,
}

impl Default for MelodySound {
    fn default() -> Self {
        Self {
            file: "builtin:twinkle".into(),
            instrument: InstrumentRef(None),
            volume: 0.0,
            transpose: 0,
            track: 0,
            chords: true,
            velocity_from_hit: true,
            quietest_hit: 20.0,
            loudest_hit: 600.0,
            stereo_width: 0.5,
        }
    }
}

impl ModuleKind for MelodySoundKind {
    fn id(&self) -> &'static str {
        "sound.melody"
    }
    fn name(&self) -> &'static str {
        "Melody"
    }
    fn help(&self) -> &'static str {
        "Every hit plays the next note of a tune from a MIDI file."
    }
    fn slot(&self) -> SlotId {
        SlotId::Sound
    }
    fn settings(&self) -> &'static [SettingDef] {
        MelodySound::defs()
    }
}

static MIDIS: Mutex<BTreeMap<String, Option<Arc<MidiFile>>>> = Mutex::new(BTreeMap::new());

/// The tune named by a Melody's "file": a `builtin:` name, a bare built-in name, or a path to
/// a `.mid` file, read once; `None` when it is empty or can't be read (remembered too).
pub fn load_tune(file: &str) -> Option<Arc<MidiFile>> {
    let name = file.trim();
    if name.is_empty() {
        return None;
    }
    let path = if bpe_music::midi::BUILTIN.iter().any(|(id, ..)| *id == name) {
        format!("builtin:{name}")
    } else {
        name.to_owned()
    };
    let mut tunes = MIDIS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(tune) = tunes.get(&path) {
        return tune.clone();
    }
    let tune = bpe_music::midi::load(&path, None).ok().map(Arc::new);
    tunes.insert(path, tune.clone());
    tune
}

/// Forgets a loaded tune, so a changed file is read again.
pub fn forget_tune(file: &str) {
    MIDIS.lock().unwrap_or_else(|e| e.into_inner()).remove(file.trim());
}

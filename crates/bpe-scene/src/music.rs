//! Instruments, the song track, beat sync and MIDI melodies. Owned by the music workstream,
//! which fills in the placeholders here.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Instrument {
    /// A soft synthesised pluck. `decay` is seconds to fade by 60 dB, `brightness` 0 to 1.
    Pluck { decay: f32, brightness: f32 },
    /// A WAV file played at the pitch of `root_note` and pitched up or down from there.
    Sample { path: String, root_note: u8 },
    /// A built-in instrument by name, such as "marimba". Plays the pluck until the instrument
    /// library is in.
    Library { name: String },
}

/// Which instrument plays which kind of event.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceMap {}

/// The song mixed under the bounces, beat sync and MIDI melody.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Music {}

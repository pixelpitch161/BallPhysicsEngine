//! Instruments: library items that sound modules play notes through, either one of the
//! built-in sounds or a synth patch. This is only the saved data; bpe-synth and bpe-mix play it.

use serde::{Deserialize, Serialize};

/// How long a note's gate stays open.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum NoteLength {
    Beats(f32),
    Seconds(f32),
}

impl Default for NoteLength {
    fn default() -> Self {
        NoteLength::Beats(1.0)
    }
}

impl NoteLength {
    /// The gate in frames at `rate`, given the tempo.
    pub fn frames(self, seconds_per_beat: f32, rate: f32) -> usize {
        let seconds = match self {
            NoteLength::Beats(b) => b * seconds_per_beat,
            NoteLength::Seconds(s) => s,
        };
        if seconds.is_finite() { (seconds.clamp(0.0, 600.0) * rate) as usize } else { 0 }
    }
}

/// How an instrument's notes become voices.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceSettings {
    pub note_length: NoteLength,
    /// Most voices at once; the oldest is stolen first.
    pub limit: u16,
    /// One voice; a new note glides from the last.
    pub mono: bool,
    /// Seconds, in mono.
    pub glide: f32,
    /// In mono, whether a new note while one is held starts the envelopes again.
    pub retrigger: bool,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self { note_length: NoteLength::default(), limit: 16, mono: false, glide: 0.1, retrigger: true }
    }
}

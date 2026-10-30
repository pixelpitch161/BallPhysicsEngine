//! Instruments: library items that sound modules play notes through, either one of the
//! built-in sounds or a synth patch. This is only the saved data; bpe-synth and bpe-mix play it.

use crate::{SettingValue, Value, ValueType};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One instrument in a scene's library. Never reused within a scene.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InstrumentId(pub u32);

/// What sound modules play their notes through.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Instrument {
    pub name: String,
    pub source: InstrumentSource,
    pub voices: VoiceSettings,
    /// Its own effects, run once over all its voices on a channel. No blocks passes the sound
    /// through.
    #[serde(skip_serializing_if = "no_blocks")]
    pub fx: bpe_graph::Graph,
}

fn no_blocks(g: &bpe_graph::Graph) -> bool {
    g.nodes.is_empty()
}

impl Default for Instrument {
    fn default() -> Self {
        Self {
            name: "Pluck".into(),
            source: InstrumentSource::default(),
            voices: VoiceSettings::default(),
            fx: bpe_graph::Graph::default(),
        }
    }
}

/// Where an instrument's sound comes from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum InstrumentSource {
    /// One of the built-in sounds, by its snake_case name (`pluck`, `marimba`...), with its knobs
    /// (the pluck has `decay` and `brightness`).
    BuiltIn { which: String, knobs: BTreeMap<String, f32> },
    /// A synth patch: a graph of the synth catalog's blocks.
    Patch { graph: bpe_graph::Graph },
}

impl Default for InstrumentSource {
    fn default() -> Self {
        InstrumentSource::BuiltIn { which: "pluck".into(), knobs: BTreeMap::new() }
    }
}

/// A setting that points at an instrument in the library, or at Scene defaults' instrument.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InstrumentRef(pub Option<InstrumentId>);

impl SettingValue for InstrumentRef {
    fn value_type() -> ValueType {
        ValueType::Instrument
    }
    fn to_value(&self) -> Value {
        Value::Int(self.0.map_or(-1, |id| id.0 as i64))
    }
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::Int(-1) => Some(InstrumentRef(None)),
            Value::Int(x) => u32::try_from(*x).ok().map(|id| InstrumentRef(Some(InstrumentId(id)))),
            _ => None,
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_instrument_ref_round_trips() {
        assert_eq!(InstrumentRef(None).to_value(), Value::Int(-1));
        assert_eq!(InstrumentRef::from_value(&Value::Int(4)), Some(InstrumentRef(Some(InstrumentId(4)))));
        assert_eq!(InstrumentRef::from_value(&Value::Int(-1)), Some(InstrumentRef(None)));
        assert_eq!(InstrumentRef::from_value(&Value::Int(-3)), None);
        assert!(ValueType::Instrument.accepts(&Value::Int(2)));
        assert!(!ValueType::Instrument.accepts(&Value::Number(2.0)));
    }

    #[test]
    fn an_instrument_saves_and_loads() {
        let mut graph = bpe_graph::Graph::new("Patch");
        graph.nodes.push(bpe_graph::Node { id: 1, kind: "osc".into(), ..Default::default() });
        let i = Instrument {
            name: "Bell".into(),
            source: InstrumentSource::Patch { graph },
            voices: VoiceSettings { note_length: NoteLength::Seconds(0.5), mono: true, ..VoiceSettings::default() },
            fx: bpe_graph::Graph::default(),
        };
        let text = ron::to_string(&i).unwrap();
        assert_eq!(ron::from_str::<Instrument>(&text).unwrap(), i);
    }
}

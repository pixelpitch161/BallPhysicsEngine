//! Chord progressions: library items that sound modules share or own, with a key, a meter and
//! chords measured in beats. Names, numerals and notes are worked out elsewhere (bpe-mix); this
//! is only the saved data.

use crate::{SettingValue, Value, ValueType};
use serde::{Deserialize, Serialize};

/// One progression in a scene's library. Never reused within a scene.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProgressionId(pub u32);

/// The scale a key is built on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    Major,
    Minor,
    Dorian,
    Phrygian,
    Lydian,
    Mixolydian,
    Locrian,
}

impl Mode {
    pub const ALL: [Mode; 7] =
        [Mode::Major, Mode::Minor, Mode::Dorian, Mode::Phrygian, Mode::Lydian, Mode::Mixolydian, Mode::Locrian];

    /// The name shown in the editor.
    pub fn label(self) -> &'static str {
        match self {
            Mode::Major => "major",
            Mode::Minor => "minor",
            Mode::Dorian => "Dorian",
            Mode::Phrygian => "Phrygian",
            Mode::Lydian => "Lydian",
            Mode::Mixolydian => "Mixolydian",
            Mode::Locrian => "Locrian",
        }
    }
}

/// A key: a root pitch class (0 = C ... 11 = B) and a mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MusicKey {
    pub root: u8,
    pub mode: Mode,
}

/// The basic triad a chord is built on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Quality {
    #[default]
    Major,
    Minor,
    Diminished,
    Augmented,
}

impl Quality {
    pub const ALL: [Quality; 4] = [Quality::Major, Quality::Minor, Quality::Diminished, Quality::Augmented];
}

/// What is added to (or swapped into) the triad.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Extension {
    #[default]
    Triad,
    Seven,
    Maj7,
    Sus2,
    Sus4,
    Add9,
    Nine,
    Eleven,
    Thirteen,
}

impl Extension {
    pub const ALL: [Extension; 9] = [
        Extension::Triad,
        Extension::Seven,
        Extension::Maj7,
        Extension::Sus2,
        Extension::Sus4,
        Extension::Add9,
        Extension::Nine,
        Extension::Eleven,
        Extension::Thirteen,
    ];
}

/// The longest a chord may last, in beats.
pub const MAX_BEATS: u16 = 256;

/// One chord and how many beats it lasts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Chord {
    /// Pitch class, 0 = C. Absolute, so a chord borrowed from another key is just a chord.
    pub root: u8,
    pub quality: Quality,
    pub ext: Extension,
    /// The bass note's pitch class for slash chords and inversions; `None` plays the root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bass: Option<u8>,
    pub beats: u16,
}

impl Chord {
    /// A plain chord of `beats` beats.
    pub fn new(root: u8, quality: Quality, ext: Extension, beats: u16) -> Chord {
        Chord { root, quality, ext, bass: None, beats }
    }

    /// Pitch classes folded into 0..=11 and the length into 1..=[`MAX_BEATS`].
    pub fn clamp(self) -> Chord {
        Chord { root: self.root % 12, bass: self.bass.map(|b| b % 12), beats: self.beats.clamp(1, MAX_BEATS), ..self }
    }
}

/// A key, a meter and chords that repeat once they run out.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Progression {
    pub name: String,
    pub key: MusicKey,
    /// Beats per bar and the beat's note value, such as (4, 4) or (6, 8).
    pub meter: (u8, u8),
    pub chords: Vec<Chord>,
}

impl Default for Progression {
    fn default() -> Self {
        Progression { name: String::new(), key: MusicKey::default(), meter: (4, 4), chords: Vec::new() }
    }
}

impl Progression {
    /// What a new scene starts with: C, Dsus4, Am, C, Am, Dsus4 in C major.
    pub fn starter() -> Progression {
        use Extension::{Sus4, Triad};
        use Quality::{Major, Minor};
        Progression {
            name: "Starter".into(),
            key: MusicKey { root: 0, mode: Mode::Major },
            meter: (4, 4),
            chords: vec![
                Chord::new(0, Major, Triad, 8),
                Chord::new(2, Major, Sus4, 8),
                Chord::new(9, Minor, Triad, 8),
                Chord::new(0, Major, Triad, 8),
                Chord::new(9, Minor, Triad, 4),
                Chord::new(2, Major, Sus4, 4),
            ],
        }
    }

    /// How many beats one time through every chord takes.
    pub fn total_beats(&self) -> u32 {
        self.chords.iter().map(|c| c.beats as u32).sum()
    }

    /// Moves the key to `root`, and every chord's root and bass by the same interval.
    pub fn transpose_to(&mut self, root: u8) {
        let shift = (root % 12 + 12 - self.key.root % 12) % 12;
        self.key.root = root % 12;
        for c in &mut self.chords {
            c.root = (c.root % 12 + shift) % 12;
            c.bass = c.bass.map(|b| (b % 12 + shift) % 12);
        }
    }
}

/// A setting that names a progression: `None` uses the built-in starter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ProgressionRef(pub Option<ProgressionId>);

impl SettingValue for ProgressionRef {
    fn value_type() -> ValueType {
        ValueType::Progression
    }
    fn to_value(&self) -> Value {
        Value::Int(self.0.map_or(-1, |id| id.0 as i64))
    }
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::Int(-1) => Some(ProgressionRef(None)),
            Value::Int(x) => u32::try_from(*x).ok().map(|id| ProgressionRef(Some(ProgressionId(id)))),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_progression_ref_round_trips_through_a_value() {
        assert_eq!(ProgressionRef(None).to_value(), Value::Int(-1));
        assert_eq!(ProgressionRef::from_value(&Value::Int(3)), Some(ProgressionRef(Some(ProgressionId(3)))));
        assert_eq!(ProgressionRef::from_value(&Value::Int(-1)), Some(ProgressionRef(None)));
        assert_eq!(ProgressionRef::from_value(&Value::Number(1.0)), None);
        assert_eq!(ProgressionRef::from_value(&Value::Int(-5)), None);
    }

    #[test]
    fn transposing_moves_roots_and_bass_and_wraps() {
        let mut p = Progression::starter();
        p.chords.push(Chord { bass: Some(4), ..Chord::new(0, Quality::Major, Extension::Triad, 4) });
        p.transpose_to(10);
        assert_eq!(p.key.root, 10);
        assert_eq!(p.chords[0].root, 10);
        assert_eq!(p.chords[1].root, 0, "D up a minor seventh wraps to C");
        assert_eq!(p.chords.last().unwrap().bass, Some(2));
        assert!(p.chords.iter().all(|c| c.root < 12 && c.bass.is_none_or(|b| b < 12)));
    }

    #[test]
    fn starter_lasts_forty_beats() {
        assert_eq!(Progression::starter().total_beats(), 40);
    }

    #[test]
    fn a_progression_saves_and_loads() {
        let p = Progression::starter();
        let text = ron::to_string(&p).unwrap();
        assert_eq!(ron::from_str::<Progression>(&text).unwrap(), p);
    }

    #[test]
    fn clamp_folds_pitches_and_limits_length() {
        let c = Chord { root: 14, bass: Some(25), beats: 0, ..Chord::default() }.clamp();
        assert_eq!((c.root, c.bass, c.beats), (2, Some(1), 1));
        assert_eq!(Chord { beats: 999, ..Chord::default() }.clamp().beats, MAX_BEATS);
    }
}

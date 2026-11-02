//! Notes: every hit plays a note of the current chord on an instrument.

pub mod graph;
pub mod mix;

use bpe_core::{Choice, InstrumentRef, ModuleKind, ProgressionRef, SettingDef, Settings, SlotId, Target};
use bpe_mix::PicksFrom;

pub struct NoteSoundKind;

/// Which of the allowed notes a hit plays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Order {
    #[default]
    Random,
    /// Climbing with how hard the hit was.
    Speed,
    /// Climbing with how often the ball has hit.
    Hits,
    /// The next note up on every hit, starting again at the bottom.
    ArpUp,
    /// The next note down on every hit, starting again at the top.
    ArpDown,
    /// Up then down again, without playing the top and bottom twice.
    ArpUpDown,
    /// Higher up the frame plays higher.
    Height,
}

/// How many notes a hit plays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Plays {
    /// One note.
    #[default]
    Single,
    /// Every note of the chord at once.
    FullChord,
    /// Only the chord's bass note.
    Root,
}

/// Which side of a hit this sound speaks for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum PlaysWhen {
    /// When this thing's own balls hit something.
    #[default]
    Hitting,
    /// When something hits this thing.
    BeingHit,
    Both,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct NoteSound {
    /// What plays the notes: an instrument in the library, or the built-in Pluck when none is picked.
    #[setting(label = "Instrument", simple, random, group = "Instrument")]
    pub instrument: InstrumentRef,
    /// The lowest MIDI note a hit may play.
    #[setting(label = "Lowest note", range(0, 127), simple, group = "Notes")]
    pub low_note: u32,
    /// The highest MIDI note a hit may play.
    #[setting(label = "Highest note", range(0, 127), simple, group = "Notes")]
    pub high_note: u32,
    /// The chord's own notes, those and the rest of the key, or any note of the key.
    #[setting(label = "Picks from", group = "Notes")]
    pub picks_from: PicksFrom,
    /// Which of the allowed notes a hit plays: random, by the hit's speed, by the ball's hits,
    /// an arpeggio, or by how high the hit is.
    #[setting(label = "Order", simple, random, group = "Notes")]
    pub order: Order,
    /// One note, the whole chord, or only its bass note.
    #[setting(label = "Plays", group = "Notes")]
    pub plays: Plays,
    /// Hits faster than this play the whole chord; slower ones play one note. 0 always plays
    /// the whole chord.
    #[setting(label = "Full chord above", range(0, 10000), unit = "px/s", when = "plays=full_chord", group = "Notes")]
    pub full_chord_speed: f32,
    /// How many octaves the whole chord is spread over.
    #[setting(label = "Chord spread", range(1, 3), unit = "octaves", when = "plays=full_chord", group = "Notes")]
    pub chord_spread: u32,
    /// Whether this sounds when its own balls hit something, when something hits it, or both.
    #[setting(label = "Plays when", group = "Which hits")]
    pub plays_when: PlaysWhen,
    /// Hits against anything else are silent.
    #[setting(label = "Only hits against", group = "Which hits")]
    pub sounds_for: Target,
    /// Two notes from this thing closer than this play as one.
    #[setting(label = "Shortest gap", range(0, 1), unit = "s", group = "Which hits")]
    pub min_gap: f32,
    /// Hits slower than this are silent.
    #[setting(label = "Quietest hit", range(0, 3000), unit = "px/s", group = "Which hits")]
    pub quietest_hit: f32,
    /// Hits this fast play at full strength.
    #[setting(label = "Loudest hit", range(1, 10000), unit = "px/s", group = "Which hits")]
    pub loudest_hit: f32,
    /// How loud these notes are.
    #[setting(label = "Volume", range(-40, 6), unit = "dB", simple, keyframe, group = "Level")]
    pub volume: f32,
    /// Where the channel sits from left (-1) to right (1), on top of each hit's own place.
    #[setting(label = "Pan", range(-1, 1), keyframe, group = "Level")]
    pub pan: f32,
    /// How far left and right notes spread, following where the hit was.
    #[setting(label = "Stereo width", range(0, 1), group = "Level")]
    pub stereo_width: f32,
    /// The chord progression these notes follow: one in the library, or the built-in starter when none is picked.
    #[setting(label = "Chords", simple, random, group = "Chords")]
    pub chords: ProgressionRef,
}

impl Default for NoteSound {
    fn default() -> Self {
        Self {
            instrument: InstrumentRef(None),
            volume: 0.0,
            pan: 0.0,
            low_note: 48,
            high_note: 84,
            chords: ProgressionRef(None),
            order: Order::Random,
            plays: Plays::Single,
            full_chord_speed: 0.0,
            chord_spread: 1,
            picks_from: PicksFrom::ChordTones,
            plays_when: PlaysWhen::Hitting,
            sounds_for: Target::Anything,
            min_gap: 0.0,
            quietest_hit: 20.0,
            loudest_hit: 600.0,
            stereo_width: 0.6,
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
    fn template(&self) -> Option<bpe_graph::Graph> {
        Some(graph::template())
    }
}

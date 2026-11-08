//! The choices of a shape's Notes: its key and scale, how hits pick notes from a slot (Plays)
//! and how the slots take turns (Slots change). The settings themselves are `notes_…` fields of
//! [`super::VoiceSettings`].

use bpe_core::Choice;

/// A key's root note.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Choice)]
pub enum Root {
    #[default]
    C,
    Cs,
    D,
    Ds,
    E,
    F,
    Fs,
    G,
    Gs,
    A,
    As,
    B,
}

impl Root {
    pub const ALL: [Root; 12] = [
        Root::C,
        Root::Cs,
        Root::D,
        Root::Ds,
        Root::E,
        Root::F,
        Root::Fs,
        Root::G,
        Root::Gs,
        Root::A,
        Root::As,
        Root::B,
    ];

    /// Semitones above C, 0..12.
    pub fn semitone(self) -> u8 {
        self as u8
    }

    /// The root `semitone` (any number, taken mod 12) semitones above C.
    pub fn from_semitone(semitone: i32) -> Root {
        Root::ALL[semitone.rem_euclid(12) as usize]
    }

    /// Its name, written with "#" (the editor's font has no sharp sign).
    pub fn label(self) -> &'static str {
        NOTE_NAMES[self as usize]
    }
}

/// The twelve note names from C, sharps written "#".
pub const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// A scale.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Choice)]
pub enum Scale {
    #[default]
    Major,
    Minor,
    Dorian,
    Phrygian,
    Lydian,
    Mixolydian,
    HarmonicMinor,
    MelodicMinor,
    MajorPentatonic,
    MinorPentatonic,
    Blues,
    Chromatic,
}

impl Scale {
    pub const ALL: [Scale; 12] = [
        Scale::Major,
        Scale::Minor,
        Scale::Dorian,
        Scale::Phrygian,
        Scale::Lydian,
        Scale::Mixolydian,
        Scale::HarmonicMinor,
        Scale::MelodicMinor,
        Scale::MajorPentatonic,
        Scale::MinorPentatonic,
        Scale::Blues,
        Scale::Chromatic,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Scale::Major => "Major",
            Scale::Minor => "Minor",
            Scale::Dorian => "Dorian",
            Scale::Phrygian => "Phrygian",
            Scale::Lydian => "Lydian",
            Scale::Mixolydian => "Mixolydian",
            Scale::HarmonicMinor => "Harmonic minor",
            Scale::MelodicMinor => "Melodic minor",
            Scale::MajorPentatonic => "Major pentatonic",
            Scale::MinorPentatonic => "Minor pentatonic",
            Scale::Blues => "Blues",
            Scale::Chromatic => "Chromatic",
        }
    }
}

/// How each hit picks notes from the slot playing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum Plays {
    /// Any note, each hit on its own.
    #[default]
    Random,
    /// Every note once, in a new random order each time round.
    Shuffle,
    Up,
    Down,
    UpDown,
    DownUp,
    /// From the outside in: lowest, highest, second lowest, second highest…
    Converge,
    /// From the inside out.
    Diverge,
    /// One note up or down from the last.
    RandomWalk,
    /// In the order the notes were clicked.
    OrderClicked,
    /// Ball n plays the slot's nth note.
    ByBall,
    /// Side 1 plays the lowest note, side 2 the next…
    BySide,
    /// A Text's letter 1 plays the lowest note, letter 2 the next… (Text part 3; a shape is all
    /// letter 1).
    ByLetter,
    /// A Text's letters A to G (either case) play their own note names, in the octave of the
    /// slot's lowest note (C5 when it is empty); any other letter plays nothing.
    SpellNotes,
    /// Every note together (or strummed).
    AllAtOnce,
}

impl Plays {
    pub const ALL: [Plays; 15] = [
        Plays::Random,
        Plays::Shuffle,
        Plays::Up,
        Plays::Down,
        Plays::UpDown,
        Plays::DownUp,
        Plays::Converge,
        Plays::Diverge,
        Plays::RandomWalk,
        Plays::OrderClicked,
        Plays::ByBall,
        Plays::BySide,
        Plays::ByLetter,
        Plays::SpellNotes,
        Plays::AllAtOnce,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Plays::Random => "Random",
            Plays::Shuffle => "Shuffle",
            Plays::Up => "Up",
            Plays::Down => "Down",
            Plays::UpDown => "Up-down",
            Plays::DownUp => "Down-up",
            Plays::Converge => "Converge",
            Plays::Diverge => "Diverge",
            Plays::RandomWalk => "Random walk",
            Plays::OrderClicked => "Order clicked",
            Plays::ByBall => "By ball",
            Plays::BySide => "By side",
            Plays::ByLetter => "By letter",
            Plays::SpellNotes => "Spell notes",
            Plays::AllAtOnce => "All at once",
        }
    }
}

/// Which way a strummed chord runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum StrumDir {
    #[default]
    Up,
    Down,
    Random,
}

impl StrumDir {
    pub const ALL: [StrumDir; 3] = [StrumDir::Up, StrumDir::Down, StrumDir::Random];

    pub fn label(self) -> &'static str {
        match self {
            StrumDir::Up => "Up",
            StrumDir::Down => "Down",
            StrumDir::Random => "Random",
        }
    }
}

/// What moves the shape on to its next slot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum Change {
    /// Every N beats or bars, on the scene's bar lines.
    #[default]
    ByTempo,
    /// Every N seconds.
    ByTime,
    /// Every N hits on this shape.
    ByHits,
    /// Only where a Slot keyframe says.
    OnlyKeyframes,
}

impl Change {
    pub const ALL: [Change; 4] = [Change::ByTempo, Change::ByTime, Change::ByHits, Change::OnlyKeyframes];

    pub fn label(self) -> &'static str {
        match self {
            Change::ByTempo => "By tempo",
            Change::ByTime => "By time",
            Change::ByHits => "By hits",
            Change::OnlyKeyframes => "Only on keyframes",
        }
    }
}

/// The unit By tempo counts in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum BeatsOrBars {
    Beats,
    #[default]
    Bars,
}

impl BeatsOrBars {
    pub fn label(self) -> &'static str {
        match self {
            BeatsOrBars::Beats => "Beats",
            BeatsOrBars::Bars => "Bars",
        }
    }
}

/// The order the slots take turns in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum SlotOrder {
    #[default]
    Forward,
    Reverse,
    /// There and back, without playing the end slots twice.
    PingPong,
    Random,
    /// Every slot once, in a new random order each time round.
    Shuffle,
}

impl SlotOrder {
    pub const ALL: [SlotOrder; 5] =
        [SlotOrder::Forward, SlotOrder::Reverse, SlotOrder::PingPong, SlotOrder::Random, SlotOrder::Shuffle];

    pub fn label(self) -> &'static str {
        match self {
            SlotOrder::Forward => "Forward",
            SlotOrder::Reverse => "Reverse",
            SlotOrder::PingPong => "Ping-pong",
            SlotOrder::Random => "Random",
            SlotOrder::Shuffle => "Shuffle",
        }
    }
}

/// The most slots a shape has.
pub const MAX_SLOTS: usize = 16;

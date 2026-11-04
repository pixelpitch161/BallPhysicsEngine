//! Chord presets: 40 progressions in 8 moods. Each is written as Roman numerals in its own mode
//! (major or minor) and played in the shape's key: a minor-sounding key plays a major preset
//! from its relative major and the other way round, so the chords stay in the key.

use super::scale::scale_steps;
use super::voicing::{ChordSpec, Voicing, voice_progression};
use bpe_core::NoteSlot;
use bpe_kit::sound::{NOTE_NAMES, Root, Scale};
use std::ops::RangeInclusive;
use std::sync::OnceLock;

/// A preset's mood, the menu's groups.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mood {
    Pop,
    Emotional,
    Epic,
    Dark,
    Dreamy,
    Jazzy,
    LoFi,
    Classic,
}

impl Mood {
    pub const ALL: [Mood; 8] =
        [Mood::Pop, Mood::Emotional, Mood::Epic, Mood::Dark, Mood::Dreamy, Mood::Jazzy, Mood::LoFi, Mood::Classic];

    pub fn label(self) -> &'static str {
        match self {
            Mood::Pop => "Pop",
            Mood::Emotional => "Emotional",
            Mood::Epic => "Epic",
            Mood::Dark => "Dark",
            Mood::Dreamy => "Dreamy",
            Mood::Jazzy => "Jazzy",
            Mood::LoFi => "Lo-fi",
            Mood::Classic => "Classic",
        }
    }
}

/// One progression.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preset {
    pub mood: Mood,
    pub name: &'static str,
    /// Its chords as Roman numerals, joined by "–". Upper case is major, lower case minor; "°"
    /// diminished, "ø7" half-diminished, "b" a flattened root; "7", "maj7", "sus4", "add9" and
    /// "9" as written.
    pub numerals: &'static str,
    /// Whether the numerals count from a minor key (i is the tonic, III and VI are major).
    pub minor: bool,
}

const fn p(mood: Mood, name: &'static str, numerals: &'static str, minor: bool) -> Preset {
    Preset { mood, name, numerals, minor }
}

use Mood::*;

/// Every preset, five per mood.
pub static PRESETS: [Preset; 40] = [
    p(Pop, "Axis", "I–V–vi–IV", false),
    p(Pop, "Fifties", "I–vi–IV–V", false),
    p(Pop, "Lift", "I–IV–vi–V", false),
    p(Pop, "Turnaround", "IV–I–V–vi", false),
    p(Pop, "Stomp", "I–V–IV–V", false),
    p(Emotional, "Sad pop", "vi–IV–I–V", false),
    p(Emotional, "Night drive", "i–VI–III–VII", true),
    p(Emotional, "Longing", "vi–V–IV–V", false),
    p(Emotional, "Tears", "i–iv–VI–V", true),
    p(Emotional, "Hope", "IV–V–iii–vi", false),
    p(Epic, "Battle", "i–VII–VI–VII", true),
    p(Epic, "Anthem", "VI–VII–i–i", true),
    p(Epic, "Titan", "i–VI–iv–V", true),
    p(Epic, "Victory", "I–bVII–IV–I", false),
    p(Epic, "Horizon", "i–III–VII–VI", true),
    p(Dark, "Descent", "i–VII–VI–V", true),
    p(Dark, "Shadow", "i–iv–i–V", true),
    p(Dark, "Dread", "i–bII–i–bII", true),
    p(Dark, "Haunt", "i–VI–ii°–V", true),
    p(Dark, "Abyss", "i–v–VI–iv", true),
    p(Dreamy, "Float", "Imaj7–IVmaj7", false),
    p(Dreamy, "Cloud", "I–iii–IV–iv", false),
    p(Dreamy, "Lydian", "I–II–IV–I", false),
    p(Dreamy, "Haze", "IVmaj7–Imaj7–vi7–V", false),
    p(Dreamy, "Drift", "Iadd9–vi7–IVmaj7–Vsus4", false),
    p(Jazzy, "Two-five-one", "ii7–V7–Imaj7–vi7", false),
    p(Jazzy, "Rhythm", "Imaj7–vi7–ii7–V7", false),
    p(Jazzy, "Minor two-five", "iiø7–V7–i7–i7", true),
    p(Jazzy, "Backdoor", "ii7–bVII7–Imaj7–Imaj7", false),
    p(Jazzy, "Bossa", "Imaj7–II7–ii7–V7", false),
    p(LoFi, "Study", "IVmaj7–iii7–vi7–Imaj7", false),
    p(LoFi, "Rainy", "ii7–V7–iii7–vi7", false),
    p(LoFi, "Tape", "Imaj7–iii7–vi7–IVmaj7", false),
    p(LoFi, "Late night", "vi7–IVmaj7–Imaj7–V7", false),
    p(LoFi, "Coffee", "IVmaj7–V7–iii7–vi7", false),
    p(Classic, "Cadence", "I–IV–V–I", false),
    p(Classic, "Canon", "I–V–vi–iii–IV–I–IV–V", false),
    p(Classic, "Twelve-bar blues", "I7–I7–I7–I7–IV7–IV7–I7–I7–V7–IV7–I7–V7", false),
    p(Classic, "Circle", "vi–ii–V–I", false),
    p(Classic, "Doo-wop", "I–vi–ii–V", false),
];

/// The semitones above the tonic of each Roman numeral in a major and a minor key.
const MAJOR_DEGREES: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
const MINOR_DEGREES: [u8; 7] = [0, 2, 3, 5, 7, 8, 10];

/// One parsed chord: its spec and the name it adds after its root note.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Parsed {
    spec: ChordSpec,
    suffix: &'static str,
}

/// Parses one numeral such as "bVII7", "iiø7", "IVmaj7".
fn parse(token: &str, minor: bool) -> Option<Parsed> {
    let (flat, rest) = match token.strip_prefix('b') {
        Some(r) => (1u8, r),
        None => (0, token),
    };
    let end = rest.find(|c: char| !matches!(c, 'I' | 'V' | 'i' | 'v')).unwrap_or(rest.len());
    let (roman, tail) = rest.split_at(end);
    let degree = match roman.to_ascii_uppercase().as_str() {
        "I" => 0,
        "II" => 1,
        "III" => 2,
        "IV" => 3,
        "V" => 4,
        "VI" => 5,
        "VII" => 6,
        _ => return None,
    };
    let upper = roman.chars().next()?.is_ascii_uppercase();
    let degrees = if minor { MINOR_DEGREES } else { MAJOR_DEGREES };
    let root = (degrees[degree] + 12 - flat) % 12;
    let (triad, seventh) = if upper { ([0, 4, 7], 10) } else { ([0, 3, 7], 10) };
    let mut spec = ChordSpec { root, triad, seventh, written_seventh: false, extra: 0 };
    // The diatonic seventh, when sevenths are asked for: what the mode's scale has above the
    // root (a borrowed root takes the dominant or minor seventh).
    let steps = scale_steps(if minor { Scale::Minor } else { Scale::Major });
    let in_mode = |semi: u8| steps.contains(&(semi % 12));
    if in_mode(root) {
        spec.seventh = if in_mode(root + 11) { 11 } else { 10 };
        if upper && degree == 4 {
            spec.seventh = 10;
        }
    }
    let suffix: &'static str = match tail {
        "" => {
            if upper {
                ""
            } else {
                "m"
            }
        }
        "7" => {
            spec.written_seventh = true;
            spec.seventh = 10;
            if upper { "7" } else { "m7" }
        }
        "maj7" => {
            spec.written_seventh = true;
            spec.seventh = 11;
            "maj7"
        }
        "°" => {
            spec.triad = [0, 3, 6];
            spec.seventh = 10;
            "dim"
        }
        "ø7" => {
            spec.triad = [0, 3, 6];
            spec.seventh = 10;
            spec.written_seventh = true;
            "m7b5"
        }
        "sus4" => {
            spec.triad = [0, 5, 7];
            "sus4"
        }
        "add9" => {
            spec.extra = 14;
            "add9"
        }
        "9" => {
            spec.written_seventh = true;
            spec.seventh = 10;
            spec.extra = 14;
            if upper { "9" } else { "m9" }
        }
        _ => return None,
    };
    if !upper && tail.is_empty() {
        spec.triad = [0, 3, 7];
    }
    Some(Parsed { spec, suffix })
}

fn parsed(p: &Preset) -> &'static [Parsed] {
    static ALL: OnceLock<Vec<Vec<Parsed>>> = OnceLock::new();
    let all = ALL.get_or_init(|| {
        PRESETS
            .iter()
            .map(|p| p.numerals.split('–').map(|t| parse(t, p.minor).expect("a preset's numeral parses")).collect())
            .collect()
    });
    let i = PRESETS.iter().position(|q| q == p).unwrap_or(0);
    &all[i]
}

/// Whether a scale sounds minor (its third is a minor third).
fn minor_scale(scale: Scale) -> bool {
    let steps = scale_steps(scale);
    steps.contains(&3) && !steps.contains(&4)
}

/// The preset's tonic (a pitch class) in a shape's key: the key's root, or its relative major
/// or minor when the preset's mode and the key's differ.
pub fn tonic(p: &Preset, root: Root, scale: Scale) -> u8 {
    let r = root.semitone();
    match (p.minor, minor_scale(scale)) {
        (false, true) => (r + 3) % 12,
        (true, false) => (r + 9) % 12,
        _ => r,
    }
}

/// The chords' names in the shape's key: "C", "G", "Am", "F".
pub fn chord_labels(p: &Preset, root: Root, scale: Scale) -> Vec<String> {
    let t = tonic(p, root, scale);
    parsed(p).iter().map(|c| format!("{}{}", NOTE_NAMES[((t + c.spec.root) % 12) as usize], c.suffix)).collect()
}

/// The preset's slots in the shape's key: one per chord, voiced inside `range`.
pub fn realise(
    p: &Preset,
    root: Root,
    scale: Scale,
    range: RangeInclusive<u8>,
    style: Voicing,
    add_bass: bool,
) -> Vec<NoteSlot> {
    let specs: Vec<ChordSpec> = parsed(p).iter().map(|c| c.spec).collect();
    voice_progression(tonic(p, root, scale), &specs, range, style, add_bass)
        .into_iter()
        .map(|notes| NoteSlot { notes, order_clicked: Vec::new() })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::chord_name::chord_name;
    use super::*;

    #[test]
    fn forty_presets_in_eight_moods() {
        assert_eq!(PRESETS.len(), 40);
        for m in Mood::ALL {
            assert_eq!(PRESETS.iter().filter(|p| p.mood == m).count(), 5, "{m:?}");
        }
        let names: std::collections::BTreeSet<_> = PRESETS.iter().map(|p| p.name).collect();
        assert_eq!(names.len(), 40, "every name once");
        for p in &PRESETS {
            let slots = realise(p, Root::C, Scale::Major, 48..=83, Voicing::Triads, false);
            assert!((2..=16).contains(&slots.len()), "{}", p.name);
            assert!(slots.iter().all(|s| s.is_valid() && s.notes.len() >= 3), "{}", p.name);
            // The voiced notes name the chord the label says (or the same notes read another
            // way: Am7 over G is also C6 over G, Gsus4 over C is Csus2).
            let same = |a: &str, b: &str| {
                a == b || matches!((a, b), ("C6", "Am7") | ("Am7", "C6") | ("Csus2", "Gsus4") | ("Gsus4", "Csus2"))
            };
            for (s, label) in slots.iter().zip(chord_labels(p, Root::C, Scale::Major)) {
                let name = chord_name(&s.notes);
                let head = name.split('/').next().unwrap_or("");
                assert!(same(head, &label), "{} {:?}: {name} for {label}", p.name, s.notes);
            }
        }
    }

    #[test]
    fn presets_play_in_the_shapes_key() {
        let axis = &PRESETS[0];
        assert_eq!(chord_labels(axis, Root::C, Scale::Major), ["C", "G", "Am", "F"]);
        assert_eq!(chord_labels(axis, Root::G, Scale::Major), ["G", "D", "Em", "C"]);
        // A minor key plays a major preset from its relative major.
        assert_eq!(chord_labels(axis, Root::A, Scale::Minor), ["C", "G", "Am", "F"]);
        let night = PRESETS.iter().find(|p| p.name == "Night drive").unwrap();
        assert_eq!(chord_labels(night, Root::A, Scale::Minor), ["Am", "F", "C", "G"]);
        assert_eq!(chord_labels(night, Root::C, Scale::Major), ["Am", "F", "C", "G"]);
        let study = PRESETS.iter().find(|p| p.name == "Study").unwrap();
        assert_eq!(chord_labels(study, Root::C, Scale::Major), ["Fmaj7", "Em7", "Am7", "Cmaj7"]);
        let minor25 = PRESETS.iter().find(|p| p.name == "Minor two-five").unwrap();
        assert_eq!(chord_labels(minor25, Root::C, Scale::Minor), ["Dm7b5", "G7", "Cm7", "Cm7"]);
        // Sevenths add the key's own seventh; Add bass puts the root below.
        let s = realise(axis, Root::C, Scale::Major, 48..=83, Voicing::Sevenths, true);
        assert_eq!(s[0].notes.len(), 5);
        assert_eq!(chord_name(&s[0].notes[1..]).split('/').next(), Some("Cmaj7"));
        assert_eq!(chord_name(&s[1].notes[1..]).split('/').next(), Some("G7"));
    }
}

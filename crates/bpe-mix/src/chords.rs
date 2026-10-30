//! Chord theory for progressions: scales, chord tones, names and numerals, the notes a hit may
//! pick, and where in a progression a moment of the video falls.

use bpe_core::{Choice, Chord, Extension, Mode, MusicKey, Progression, Quality};

/// Semitones above the root of each mode's seven notes.
fn mode_steps(mode: Mode) -> [u8; 7] {
    match mode {
        Mode::Major => [0, 2, 4, 5, 7, 9, 11],
        Mode::Minor => [0, 2, 3, 5, 7, 8, 10],
        Mode::Dorian => [0, 2, 3, 5, 7, 9, 10],
        Mode::Phrygian => [0, 1, 3, 5, 7, 8, 10],
        Mode::Lydian => [0, 2, 4, 6, 7, 9, 11],
        Mode::Mixolydian => [0, 2, 4, 5, 7, 9, 10],
        Mode::Locrian => [0, 1, 3, 5, 6, 8, 10],
    }
}

/// The pitch classes of the key's scale, starting on its root.
pub fn scale(key: MusicKey) -> [u8; 7] {
    mode_steps(key.mode).map(|s| (key.root % 12 + s) % 12)
}

fn quality_of(third: u8, fifth: u8) -> Quality {
    match (third, fifth) {
        (3, 6) => Quality::Diminished,
        (4, 8) => Quality::Augmented,
        (3, _) => Quality::Minor,
        _ => Quality::Major,
    }
}

/// The triad built in thirds on the scale's `degree` (0-based), lasting `beats`.
pub fn diatonic(key: MusicKey, degree: usize, beats: u16) -> Chord {
    let s = scale(key);
    let d = degree % 7;
    let up = |n: usize| (s[(d + n) % 7] + 12 - s[d]) % 12;
    Chord::new(s[d], quality_of(up(2), up(4)), Extension::Triad, beats)
}

/// Chords commonly borrowed into the key from its parallel mode, leaving out any the key
/// already has.
pub fn borrowed(key: MusicKey) -> Vec<Chord> {
    let r = key.root % 12;
    let major_ish = scale(key)[2] == (r + 4) % 12;
    let picks: &[(u8, Quality)] = if major_ish {
        &[(5, Quality::Minor), (8, Quality::Major), (10, Quality::Major), (2, Quality::Major)]
    } else {
        &[(7, Quality::Major), (5, Quality::Major), (1, Quality::Major)]
    };
    picks
        .iter()
        .map(|&(step, q)| Chord::new((r + step) % 12, q, Extension::Triad, 4))
        .filter(|c| function(c, key) == Function::Borrowed)
        .collect()
}

/// Semitones above the root of every note in the chord, lowest first.
pub fn intervals(c: &Chord) -> Vec<u8> {
    let mut v: Vec<u8> = match c.quality {
        Quality::Major => vec![0, 4, 7],
        Quality::Minor => vec![0, 3, 7],
        Quality::Diminished => vec![0, 3, 6],
        Quality::Augmented => vec![0, 4, 8],
    };
    match c.ext {
        Extension::Triad => {}
        Extension::Sus2 => v[1] = 2,
        Extension::Sus4 => v[1] = 5,
        Extension::Seven => v.push(10),
        Extension::Maj7 => v.push(11),
        Extension::Add9 => v.push(14),
        Extension::Nine => v.extend([10, 14]),
        Extension::Eleven => v.extend([10, 14, 17]),
        Extension::Thirteen => v.extend([10, 14, 21]),
    }
    v.sort_unstable();
    v
}

/// The chord's lowest note's pitch class: the slash bass, or the root.
pub fn bass_note(c: &Chord) -> u8 {
    c.bass.unwrap_or(c.root) % 12
}

const SHARPS: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
const FLATS: [&str; 12] = ["C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B"];

/// Whether the key is written with flats: its relative major is C, F, Bb, Eb, Ab, Db or Gb
/// (C has no sharps or flats of its own, and its borrowed chords read better as flats).
fn flat_key(key: MusicKey) -> bool {
    let offset = match key.mode {
        Mode::Major => 0,
        Mode::Dorian => 2,
        Mode::Phrygian => 4,
        Mode::Lydian => 5,
        Mode::Mixolydian => 7,
        Mode::Minor => 9,
        Mode::Locrian => 11,
    };
    [0, 5, 10, 3, 8, 1, 6].contains(&((key.root % 12 + 12 - offset) % 12))
}

/// A pitch class spelled the way the key writes it.
pub fn note_name(pitch_class: u8, key: MusicKey) -> &'static str {
    if flat_key(key) { FLATS[(pitch_class % 12) as usize] } else { SHARPS[(pitch_class % 12) as usize] }
}

/// Every chord-name ending: (ending, quality, extension). The first ending for a pair is the one
/// [`chord_name`] writes; the rest are other spellings [`parse_chord`] accepts.
const SUFFIXES: &[(&str, Quality, Extension)] = {
    use Extension::*;
    use Quality::*;
    &[
        ("", Major, Triad),
        ("m", Minor, Triad),
        ("dim", Diminished, Triad),
        ("aug", Augmented, Triad),
        ("7", Major, Seven),
        ("m7", Minor, Seven),
        ("m7b5", Diminished, Seven),
        ("aug7", Augmented, Seven),
        ("maj7", Major, Maj7),
        ("m(maj7)", Minor, Maj7),
        ("dim(maj7)", Diminished, Maj7),
        ("aug(maj7)", Augmented, Maj7),
        ("sus2", Major, Sus2),
        ("sus4", Major, Sus4),
        ("add9", Major, Add9),
        ("madd9", Minor, Add9),
        ("dim(add9)", Diminished, Add9),
        ("aug(add9)", Augmented, Add9),
        ("9", Major, Nine),
        ("m9", Minor, Nine),
        ("dim9", Diminished, Nine),
        ("aug9", Augmented, Nine),
        ("11", Major, Eleven),
        ("m11", Minor, Eleven),
        ("dim11", Diminished, Eleven),
        ("aug11", Augmented, Eleven),
        ("13", Major, Thirteen),
        ("m13", Minor, Thirteen),
        ("dim13", Diminished, Thirteen),
        ("aug13", Augmented, Thirteen),
        // Other spellings.
        ("maj", Major, Triad),
        ("M", Major, Triad),
        ("min", Minor, Triad),
        ("°", Diminished, Triad),
        ("+", Augmented, Triad),
        ("min7", Minor, Seven),
        ("ø", Diminished, Seven),
        ("M7", Major, Maj7),
        ("mmaj7", Minor, Maj7),
        ("sus", Major, Sus4),
    ]
};

/// The chord's name in the key's spelling, such as "Am7", "Bm7b5" or "Cmaj7/E".
pub fn chord_name(c: &Chord, key: MusicKey) -> String {
    let suffix = match c.ext {
        Extension::Sus2 => "sus2",
        Extension::Sus4 => "sus4",
        _ => SUFFIXES.iter().find(|(_, q, e)| *q == c.quality && *e == c.ext).map_or("", |s| s.0),
    };
    let mut name = format!("{}{suffix}", note_name(c.root, key));
    if let Some(b) = c.bass.filter(|&b| b % 12 != c.root % 12) {
        name += "/";
        name += note_name(b, key);
    }
    name
}

/// Reads a chord name such as "F#m", "Bb7", "Dsus4", "Amaj7", "Bm7b5" or "C/E". The chord
/// lasts 4 beats. `None` for names it doesn't know.
pub fn parse_chord(name: &str) -> Option<Chord> {
    let name = name.trim().replace('♭', "b").replace('♯', "#");
    let (main, bass) = match name.split_once('/') {
        Some((m, b)) => (m, Some(b)),
        None => (name.as_str(), None),
    };
    let (root, rest) = parse_root(main)?;
    let &(_, quality, ext) = SUFFIXES.iter().find(|(s, ..)| *s == rest)?;
    let bass = match bass {
        Some(b) => match parse_root(b)? {
            (pc, "") => Some(pc),
            _ => return None,
        },
        None => None,
    };
    Some(Chord { bass: bass.filter(|&b| b != root), ..Chord::new(root, quality, ext, 4) })
}

/// A note letter with an optional sharp or flat, and what follows it.
fn parse_root(text: &str) -> Option<(u8, &str)> {
    let mut chars = text.chars();
    let base: i32 = match chars.next()?.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest = chars.as_str();
    let (shift, rest) = if let Some(r) = rest.strip_prefix('#') {
        (1, r)
    } else if let Some(r) = rest.strip_prefix('b') {
        (-1, r)
    } else {
        (0, rest)
    };
    Some(((base + shift).rem_euclid(12) as u8, rest))
}

/// The chord's Roman numeral in the key, such as "I", "ii°", "bVI" or "V7".
pub fn numeral(c: &Chord, key: MusicKey) -> String {
    let step = (c.root % 12 + 12 - key.root % 12) % 12;
    let sharp_four = if key.mode == Mode::Locrian { "bV" } else { "#IV" };
    let upper = ["I", "bII", "II", "bIII", "III", "IV", sharp_four, "V", "bVI", "VI", "bVII", "VII"][step as usize];
    let mut n = match c.quality {
        Quality::Minor | Quality::Diminished => upper.to_lowercase(),
        _ => upper.to_owned(),
    };
    match c.quality {
        Quality::Diminished => n.push('°'),
        Quality::Augmented => n.push('+'),
        _ => {}
    }
    match c.ext {
        Extension::Seven => n.push('7'),
        Extension::Maj7 => n.push_str("M7"),
        _ => {}
    }
    n
}

/// What a chord does in the key, for colouring it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Function {
    /// I, iii, vi: at rest.
    Home,
    /// ii, IV: moving away.
    Away,
    /// V, vii: pulling back home.
    Pull,
    /// Not one of the key's own chords.
    Borrowed,
}

/// What the chord does in the key. A sus chord counts by its root alone.
pub fn function(c: &Chord, key: MusicKey) -> Function {
    let Some(degree) = scale(key).iter().position(|&p| p == c.root % 12) else { return Function::Borrowed };
    let sus = matches!(c.ext, Extension::Sus2 | Extension::Sus4);
    if !sus && diatonic(key, degree, 1).quality != c.quality {
        return Function::Borrowed;
    }
    match degree {
        0 | 2 | 5 => Function::Home,
        1 | 3 => Function::Away,
        _ => Function::Pull,
    }
}

/// Which notes a hit may pick from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum PicksFrom {
    /// Only the chord's own notes.
    #[default]
    ChordTones,
    /// The chord's notes and the rest of the key's scale.
    ChordAndScale,
    /// Any note of the key's scale.
    Scale,
}

/// The MIDI notes between `low` and `high` (either way round) whose pitch class is allowed,
/// lowest first.
pub fn candidates(c: &Chord, key: MusicKey, from: PicksFrom, low: u8, high: u8) -> Vec<u8> {
    let mut classes: Vec<u8> = Vec::new();
    if from != PicksFrom::Scale {
        classes.extend(intervals(c).iter().map(|i| (c.root + i) % 12));
        classes.push(bass_note(c));
    }
    if from != PicksFrom::ChordTones {
        classes.extend(scale(key));
    }
    let (lo, hi) = (low.min(high).min(127), low.max(high).min(127));
    (lo..=hi).filter(|k| classes.contains(&(k % 12))).collect()
}

/// The whole chord voiced upward from `low`: the bass note first, then the chord's notes stacked
/// over `spread` octaves (1 to 3). Notes above `high` (or 127) are left out.
pub fn voicing(c: &Chord, low: u8, high: u8, spread: u8) -> Vec<u8> {
    let (lo, hi) = (low.min(high) as u32, (low.max(high).min(127)) as u32);
    let bass_pc = bass_note(c) as u32;
    let bass = lo + (bass_pc + 12 - lo % 12) % 12;
    let root = bass + (c.root as u32 % 12 + 12 - bass_pc) % 12;
    let mut notes = vec![bass];
    for octave in 0..spread.clamp(1, 3) as u32 {
        notes.extend(intervals(c).iter().map(|&i| root + 12 * octave + i as u32));
    }
    notes.sort_unstable();
    notes.dedup();
    notes.into_iter().filter(|&n| n >= lo && n <= hi).map(|n| n as u8).collect()
}

/// Where each moment of the video falls in a progression, with chords laid on a beat grid.
#[derive(Clone, Debug)]
pub struct ChordClock {
    chords: Vec<Chord>,
    /// Each chord's first beat within one time through.
    starts: Vec<u32>,
    total: u32,
    beats: Vec<f64>,
    /// Seconds per beat past the end of the grid.
    period: f64,
}

/// Seconds per beat when the grid doesn't say.
const FALLBACK_PERIOD: f64 = 0.5;

impl ChordClock {
    /// `beats` are the beat times in video seconds. An empty progression plays the starter.
    pub fn new(p: &Progression, beats: &[f64]) -> ChordClock {
        let chords = if p.chords.is_empty() { Progression::starter().chords } else { p.chords.clone() };
        let mut starts = Vec::with_capacity(chords.len());
        let mut total = 0u32;
        for c in &chords {
            starts.push(total);
            total += c.beats.max(1) as u32;
        }
        let period = match beats {
            [.., a, b] if b > a => b - a,
            _ => FALLBACK_PERIOD,
        };
        ChordClock { chords, starts, total, beats: beats.to_vec(), period }
    }

    /// The beat count at `video_time`: whole beats since the first, with grid gaps honoured
    /// and the last gap carried on past the end.
    fn beat(&self, t: f64) -> u64 {
        let first = self.beats.first().copied().unwrap_or(0.0);
        if t <= first || !t.is_finite() {
            return 0;
        }
        let i = self.beats.partition_point(|&b| b <= t);
        let last = self.beats.len().saturating_sub(1);
        if self.beats.len() >= 2 && i <= last {
            return (i - 1) as u64;
        }
        let from = self.beats.last().copied().unwrap_or(0.0);
        last as u64 + ((t - from) / self.period).floor() as u64
    }

    /// How many chords have started up to `video_time`, counting every time through.
    pub fn step(&self, video_time: f64) -> u64 {
        let beat = self.beat(video_time);
        let cycles = beat / self.total as u64;
        let within = (beat % self.total as u64) as u32;
        let i = self.starts.partition_point(|&s| s <= within) - 1;
        cycles * self.chords.len() as u64 + i as u64
    }

    /// The video time beat `n` starts: the inverse of [`ChordClock::beat`].
    fn beat_time(&self, n: u64) -> f64 {
        if self.beats.len() >= 2 && (n as usize) < self.beats.len() {
            return self.beats[n as usize];
        }
        let last = self.beats.len().saturating_sub(1) as u64;
        self.beats.last().copied().unwrap_or(0.0) + n.saturating_sub(last) as f64 * self.period
    }

    /// Every chord from the start of the video to `duration`, as (start, end, chord) in video
    /// seconds, end to end. The last one stops at `duration`.
    pub fn spans(&self, duration: f64) -> Vec<(f64, f64, Chord)> {
        let mut out = Vec::new();
        let (mut beat, mut start) = (0u64, 0.0);
        while start < duration && out.len() < 100_000 {
            let within = (beat % self.total as u64) as u32;
            let i = self.starts.partition_point(|&s| s <= within) - 1;
            beat += self.chords[i].beats.max(1) as u64;
            let end = self.beat_time(beat).min(duration);
            out.push((start, end, self.chords[i]));
            start = end;
        }
        out
    }

    /// The chord playing at `video_time`, and its place in the progression.
    pub fn at(&self, video_time: f64) -> (usize, &Chord) {
        let i = (self.step(video_time) % self.chords.len() as u64) as usize;
        (i, &self.chords[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(root: u8, mode: Mode) -> MusicKey {
        MusicKey { root, mode }
    }

    #[test]
    fn spans_cover_the_video_and_cycle() {
        let beats: Vec<f64> = (0..50).map(|b| b as f64 * 0.5).collect();
        let spans = ChordClock::new(&Progression::starter(), &beats).spans(25.0);
        assert_eq!(spans[0], (0.0, 4.0, Progression::starter().chords[0]));
        assert!(spans.windows(2).all(|w| w[0].1 == w[1].0), "end to end");
        assert_eq!(spans.last().unwrap().1, 25.0);
        // 40 beats a time through is 20 s, so the seventh span is the first chord again.
        assert_eq!(spans[6].0, 20.0);
        assert_eq!(spans[6].2, Progression::starter().chords[0]);
        assert!(ChordClock::new(&Progression::starter(), &[]).spans(0.0).is_empty());
    }

    #[test]
    fn scales_match_the_modes() {
        assert_eq!(scale(key(9, Mode::Minor)), [9, 11, 0, 2, 4, 5, 7]);
        assert_eq!(scale(key(2, Mode::Dorian)), [2, 4, 5, 7, 9, 11, 0]);
    }

    #[test]
    fn diatonic_triads_in_a_minor() {
        let k = key(9, Mode::Minor);
        let chords: Vec<Chord> = (0..7).map(|d| diatonic(k, d, 4)).collect();
        let names: Vec<String> = chords.iter().map(|c| chord_name(c, k)).collect();
        let numerals: Vec<String> = chords.iter().map(|c| numeral(c, k)).collect();
        assert_eq!(names, ["Am", "Bdim", "C", "Dm", "Em", "F", "G"]);
        assert_eq!(numerals, ["i", "ii°", "bIII", "iv", "v", "bVI", "bVII"]);
    }

    #[test]
    fn extensions_and_names() {
        let c = Key::c();
        assert_eq!(chord_name(&Chord::new(9, Quality::Minor, Extension::Seven, 4), c), "Am7");
        assert_eq!(chord_name(&Chord::new(11, Quality::Diminished, Extension::Seven, 4), c), "Bm7b5");
        let slash = Chord { bass: Some(4), ..Chord::new(0, Quality::Major, Extension::Maj7, 4) };
        assert_eq!(chord_name(&slash, c), "Cmaj7/E");
        assert_eq!(chord_name(&Chord::new(3, Quality::Major, Extension::Triad, 4), key(3, Mode::Major)), "Eb");
        assert_eq!(chord_name(&Chord::new(6, Quality::Major, Extension::Triad, 4), key(2, Mode::Major)), "F#");
    }

    #[test]
    fn parse_reads_what_name_writes() {
        for root in 0..12 {
            for q in Quality::ALL {
                for ext in Extension::ALL {
                    if matches!(ext, Extension::Sus2 | Extension::Sus4) && q != Quality::Major {
                        continue; // a sus chord's name says nothing of its third
                    }
                    let c = Chord::new(root, q, ext, 4);
                    let name = chord_name(&c, Key::c());
                    assert_eq!(parse_chord(&name), Some(c), "{name}");
                }
            }
        }
        assert_eq!(parse_chord("Bb7"), Some(Chord::new(10, Quality::Major, Extension::Seven, 4)));
        assert_eq!(parse_chord("F#m"), Some(Chord::new(6, Quality::Minor, Extension::Triad, 4)));
        assert_eq!(parse_chord("Dsus4"), Some(Chord::new(2, Quality::Major, Extension::Sus4, 4)));
        assert_eq!(parse_chord("Bm7b5"), Some(Chord::new(11, Quality::Diminished, Extension::Seven, 4)));
        assert_eq!(parse_chord("C/E").and_then(|c| c.bass), Some(4));
        assert_eq!(parse_chord("H"), None);
        assert_eq!(parse_chord("Cweird"), None);
        assert_eq!(parse_chord("C/Q"), None);
    }

    #[test]
    fn functions_colour_like_the_mockup() {
        let k = key(9, Mode::Minor);
        let f = |root, q| function(&Chord::new(root, q, Extension::Triad, 4), k);
        assert_eq!(f(9, Quality::Minor), Function::Home);
        assert_eq!(f(2, Quality::Minor), Function::Away);
        assert_eq!(f(4, Quality::Minor), Function::Pull);
        assert_eq!(f(4, Quality::Major), Function::Borrowed);
        assert_eq!(function(&Chord::new(2, Quality::Major, Extension::Sus4, 4), Key::c()), Function::Away);
    }

    #[test]
    fn borrowed_chords_leave_out_the_keys_own() {
        let names: Vec<String> = borrowed(Key::c()).iter().map(|c| chord_name(c, Key::c())).collect();
        assert_eq!(names, ["Fm", "Ab", "Bb", "D"]);
        let mixo = key(7, Mode::Mixolydian);
        assert!(borrowed(mixo).iter().all(|c| function(c, mixo) == Function::Borrowed));
    }

    #[test]
    fn candidates_respect_the_range() {
        let c = Chord::new(0, Quality::Major, Extension::Triad, 4);
        assert_eq!(candidates(&c, Key::c(), PicksFrom::ChordTones, 60, 72), [60, 64, 67, 72]);
        assert_eq!(candidates(&c, Key::c(), PicksFrom::ChordTones, 72, 60), [60, 64, 67, 72]);
        assert!(candidates(&c, Key::c(), PicksFrom::ChordTones, 61, 63).is_empty());
        assert_eq!(candidates(&c, Key::c(), PicksFrom::Scale, 60, 64), [60, 62, 64]);
    }

    #[test]
    fn voicing_drops_notes_above_the_range() {
        let c = Chord::new(0, Quality::Major, Extension::Triad, 4);
        let v = voicing(&c, 120, 127, 3);
        assert!(!v.is_empty());
        assert!(v.iter().all(|&n| (120..=127).contains(&n) && [0, 4, 7].contains(&(n % 12))), "{v:?}");
        assert_eq!(voicing(&c, 48, 84, 1), [48, 52, 55]);
        let slash = Chord { bass: Some(4), ..Chord::new(0, Quality::Major, Extension::Maj7, 4) };
        assert_eq!(voicing(&slash, 36, 60, 1), [40, 48, 52, 55, 59]);
    }

    fn two_chords(beats: u16) -> Progression {
        Progression {
            chords: vec![
                Chord::new(0, Quality::Major, Extension::Triad, beats),
                Chord::new(5, Quality::Major, Extension::Triad, beats),
            ],
            ..Progression::default()
        }
    }

    #[test]
    fn the_clock_follows_beats_and_cycles() {
        let clock = ChordClock::new(&two_chords(2), &[0.0, 0.5, 1.0, 1.5]);
        assert_eq!(clock.at(0.2).0, 0);
        assert_eq!(clock.at(1.1).0, 1);
        assert_eq!(clock.at(2.1).0, 0);
        assert_eq!(clock.at(-3.0).0, 0);
        assert_eq!(clock.step(2.1), 2);
    }

    #[test]
    fn the_clock_extends_past_the_grid() {
        let p = Progression {
            chords: (0..3).map(|r| Chord::new(r, Quality::Major, Extension::Triad, 1)).collect(),
            ..Progression::default()
        };
        let clock = ChordClock::new(&p, &[0.0, 0.5]);
        assert_eq!(clock.at(1.2).0, 2);
        assert_eq!(clock.at(1.6).0, 0);
        let uneven = ChordClock::new(&p, &[0.0, 0.4, 1.0]);
        assert_eq!(uneven.at(0.9).0, 1, "a long gap in a song's beats is still one beat");
    }

    #[test]
    fn an_empty_progression_plays_the_starter() {
        let clock = ChordClock::new(&Progression::default(), &[]);
        assert_eq!(clock.at(0.0).1, &Progression::starter().chords[0]);
    }

    struct Key;
    impl Key {
        fn c() -> MusicKey {
            MusicKey::default()
        }
    }
}

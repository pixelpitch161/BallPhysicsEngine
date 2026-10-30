//! Named chord progressions to start from, picked by mood.

use crate::chords::diatonic;
use bpe_core::{Chord, Extension, Mode, MusicKey};

/// A progression written as scale degrees, so it fits any key.
#[derive(Clone, Copy, Debug)]
pub struct Preset {
    pub name: &'static str,
    pub mood: &'static str,
    pub mode: Mode,
    /// 0-based scale degrees and what each adds to its triad.
    pub degrees: &'static [(u8, Extension)],
    /// How long each chord lasts.
    pub beats: u16,
}

impl Preset {
    /// The chords in the key on `root`, in this preset's mode.
    pub fn chords(&self, root: u8) -> Vec<Chord> {
        let key = MusicKey { root: root % 12, mode: self.mode };
        self.degrees.iter().map(|&(d, ext)| Chord { ext, ..diatonic(key, d as usize, self.beats) }).collect()
    }
}

use Extension::{Maj7, Seven, Sus2, Sus4, Triad as T};
use Mode::{Dorian, Lydian, Major, Minor, Mixolydian, Phrygian};

const fn p(name: &'static str, mood: &'static str, mode: Mode, degrees: &'static [(u8, Extension)]) -> Preset {
    Preset { name, mood, mode, degrees, beats: 4 }
}

/// Every preset, grouped loosely by mood.
pub const PRESETS: &[Preset] = &[
    p("Pop anthem", "uplifting", Major, &[(0, T), (4, T), (5, T), (3, T)]),
    p("Hopeful lift", "uplifting", Major, &[(3, T), (4, Sus4), (5, T), (5, T)]),
    p("Sunrise", "uplifting", Major, &[(3, T), (0, T), (4, T), (5, T)]),
    p("Three-chord rock", "bright", Major, &[(0, T), (3, T), (4, T), (0, T)]),
    p("Mixolydian rock", "bright", Mixolydian, &[(0, T), (6, T), (3, T), (0, T)]),
    p("Lydian float", "dreamy", Lydian, &[(0, T), (1, T), (0, Maj7), (1, T)]),
    p("Dreamy loop", "dreamy", Minor, &[(0, Seven), (5, Maj7), (2, Maj7), (6, T)]),
    p("Sad loop", "emotional", Major, &[(5, T), (3, T), (0, T), (4, T)]),
    p("Royal road", "emotional", Major, &[(3, Maj7), (4, Seven), (2, Seven), (5, T)]),
    p("Ballad", "emotional", Major, &[(0, T), (2, T), (3, T), (4, T)]),
    p("Fifties", "nostalgic", Major, &[(0, T), (5, T), (3, T), (4, T)]),
    p("Canon", "nostalgic", Major, &[(0, T), (4, T), (5, T), (2, T), (3, T), (0, T), (3, T), (4, T)]),
    p("Jazz turnaround", "chill", Major, &[(1, Seven), (4, Seven), (0, Maj7), (5, Seven)]),
    p("Lo-fi drift", "chill", Major, &[(3, Maj7), (2, Seven), (1, Seven), (0, Maj7)]),
    p("Dorian vamp", "chill", Dorian, &[(0, Seven), (3, T), (0, Seven), (3, T)]),
    p("Open sky", "chill", Major, &[(0, Sus2), (3, Sus2), (5, T), (4, Sus4)]),
    p("Andalusian", "dramatic", Minor, &[(0, T), (6, T), (5, T), (4, T)]),
    p("Epic minor", "dramatic", Minor, &[(0, T), (5, T), (2, T), (6, T)]),
    p("Cinematic", "dramatic", Minor, &[(0, T), (5, T), (6, T), (0, T)]),
    p("Dark pulse", "dark", Minor, &[(0, T), (0, T), (5, T), (4, T)]),
    p("Minor lament", "dark", Minor, &[(0, T), (6, T), (5, T), (6, T)]),
    p("Night drive", "dark", Minor, &[(0, T), (3, T), (6, T), (5, T)]),
    p("Phrygian tension", "dark", Phrygian, &[(0, T), (1, T), (0, T), (1, T)]),
];

/// Every mood, in the order presets list them.
pub fn moods() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for p in PRESETS {
        if !out.contains(&p.mood) {
            out.push(p.mood);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chords::chord_name;

    #[test]
    fn presets_are_plenty_and_valid() {
        assert!(PRESETS.len() >= 20);
        for p in PRESETS {
            assert!(p.degrees.iter().all(|&(d, _)| d < 7), "{}", p.name);
            assert!(p.chords(0).iter().all(|c| c.beats >= 1), "{}", p.name);
        }
        let names: std::collections::BTreeSet<_> = PRESETS.iter().map(|p| p.name).collect();
        assert_eq!(names.len(), PRESETS.len(), "names are unique");
    }

    #[test]
    fn pop_anthem_in_g() {
        let key = MusicKey { root: 7, mode: Mode::Major };
        let names: Vec<String> = PRESETS[0].chords(7).iter().map(|c| chord_name(c, key)).collect();
        assert_eq!(names, ["G", "D", "Em", "C"]);
    }

    #[test]
    fn moods_are_unique_and_in_order() {
        let m = moods();
        assert_eq!(m[0], "uplifting");
        assert_eq!(m.len(), m.iter().collect::<std::collections::BTreeSet<_>>().len());
    }
}

//! Voice leading: a progression's chords placed so each moves as little as it can from the
//! last, inside the keyboard's range.

use std::ops::RangeInclusive;

/// One chord of a progression: its root (semitones above the progression's tonic), its triad
/// (semitones above its root) and the seventh it takes when sevenths are asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChordSpec {
    pub root: u8,
    pub triad: [u8; 3],
    /// Semitones above the root of its seventh (10, 11, or 9 for a diminished seventh).
    pub seventh: u8,
    /// The progression writes it with its seventh (it keeps it as Triads too).
    pub written_seventh: bool,
    /// Extra notes the progression writes (an added ninth: 14), 0 for none.
    pub extra: u8,
}

/// How the chords are voiced.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Voicing {
    /// Three notes (or four where the progression writes a seventh).
    #[default]
    Triads,
    /// Every chord with its seventh.
    Sevenths,
    /// Triads opened out: the third an octave up.
    Spread,
}

impl Voicing {
    pub const ALL: [Voicing; 3] = [Voicing::Triads, Voicing::Sevenths, Voicing::Spread];

    pub fn label(self) -> &'static str {
        match self {
            Voicing::Triads => "Triads",
            Voicing::Sevenths => "Sevenths",
            Voicing::Spread => "Spread",
        }
    }
}

/// The chord's notes as semitones above its root, close voiced, for `style`.
fn intervals(c: &ChordSpec, style: Voicing) -> Vec<u8> {
    let mut v = c.triad.to_vec();
    if style == Voicing::Sevenths || c.written_seventh {
        v.push(c.seventh);
    }
    if c.extra > 0 {
        v.push(c.extra);
    }
    v
}

/// Every placement of the chord (each inversion, each octave) inside `range`, sorted.
fn placements(tonic: i32, c: &ChordSpec, ivs: &[u8], range: &RangeInclusive<u8>) -> Vec<Vec<u8>> {
    let (lo, hi) = (*range.start() as i32, *range.end() as i32);
    let root = tonic + c.root as i32;
    let mut out = Vec::new();
    let n = ivs.len();
    for inv in 0..n {
        // Inversion `inv`: the lowest `inv` notes go up an octave.
        let mut notes: Vec<i32> =
            ivs.iter().enumerate().map(|(i, &x)| root + x as i32 + if i < inv { 12 } else { 0 }).collect();
        notes.sort_unstable();
        let span = notes[n - 1] - notes[0];
        if span > hi - lo {
            continue;
        }
        let first = notes[0];
        // Every octave shift that keeps the chord inside the range.
        let mut k = (lo - first).div_euclid(12) - 1;
        while first + 12 * k <= hi {
            let moved: Vec<i32> = notes.iter().map(|x| x + 12 * k).collect();
            if moved[0] >= lo && moved[n - 1] <= hi {
                out.push(moved.iter().map(|&x| x as u8).collect());
            }
            k += 1;
        }
    }
    out.sort();
    out.dedup();
    out
}

/// How far the notes move from `a` to `b`: paired lowest to lowest when they have as many
/// notes, else each note to the nearest of the other chord, both ways.
fn motion(a: &[u8], b: &[u8]) -> i32 {
    if a.len() == b.len() {
        return a.iter().zip(b).map(|(x, y)| (*x as i32 - *y as i32).abs()).sum();
    }
    let near = |n: u8, set: &[u8]| set.iter().map(|&m| (n as i32 - m as i32).abs()).min().unwrap_or(0);
    a.iter().map(|&n| near(n, b)).sum::<i32>() + b.iter().map(|&n| near(n, a)).sum::<i32>()
}

/// `chords` (roots above `tonic`, a MIDI note's pitch class) voiced inside `range`: the first in
/// root position nearest the middle of the range, each next in the inversion and octave that
/// moves least from the last (ties: the lower). Spread then raises each chord's third an octave
/// (or, at the top of the range, drops its root one); Add bass puts the root an octave below
/// the chord's lowest note.
pub fn voice_progression(
    tonic: u8,
    chords: &[ChordSpec],
    range: RangeInclusive<u8>,
    style: Voicing,
    add_bass: bool,
) -> Vec<Vec<u8>> {
    let mid = (*range.start() as f32 + *range.end() as f32) / 2.0;
    let tonic = (tonic % 12) as i32;
    let mut out: Vec<Vec<u8>> = Vec::with_capacity(chords.len());
    for (i, c) in chords.iter().enumerate() {
        let ivs = intervals(c, if style == Voicing::Spread { Voicing::Triads } else { style });
        let mut options = placements(tonic, c, &ivs, &range);
        if options.is_empty() {
            // A range too small for the chord: root position from the range's bottom.
            let root = *range.start() as i32 + (tonic + c.root as i32 - *range.start() as i32).rem_euclid(12);
            options.push(ivs.iter().map(|&x| (root + x as i32).clamp(0, 127) as u8).collect());
        }
        let pick = match out.last() {
            None => {
                let root_pc = ((tonic + c.root as i32) % 12) as u8;
                let mean = |v: &Vec<u8>| v.iter().map(|&x| x as f32).sum::<f32>() / v.len() as f32;
                options
                    .iter()
                    .filter(|o| o[0] % 12 == root_pc)
                    .min_by(|a, b| (mean(a) - mid).abs().total_cmp(&(mean(b) - mid).abs()).then(a[0].cmp(&b[0])))
                    .or_else(|| options.first())
                    .cloned()
                    .unwrap_or_default()
            }
            Some(_) if i > 0 => {
                let last = &out[i - 1];
                options.iter().min_by_key(|o| (motion(last, o), o[0])).cloned().unwrap_or_default()
            }
            Some(_) => unreachable!(),
        };
        out.push(pick);
    }
    // Spread and bass go on after the leading, so the chords keep their close movement.
    for (notes, c) in out.iter_mut().zip(chords) {
        let root_pc = ((tonic + c.root as i32) % 12) as u8;
        if style == Voicing::Spread {
            let third_pc = ((tonic + c.root as i32 + c.triad[1] as i32) % 12) as u8;
            if let Some(i) = notes.iter().position(|n| n % 12 == third_pc) {
                if notes[i] as i32 + 12 <= *range.end() as i32 {
                    notes[i] += 12;
                } else if let Some(r) = notes.iter().position(|n| n % 12 == root_pc)
                    && notes[r] >= 12
                {
                    notes[r] -= 12;
                }
            }
        }
        notes.sort_unstable();
        if add_bass && let Some(&low) = notes.first() {
            let mut bass = low as i32 - 12;
            bass -= (bass - root_pc as i32).rem_euclid(12);
            if bass >= 0 {
                notes.insert(0, bass as u8);
            }
        }
        notes.dedup();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAJ: [u8; 3] = [0, 4, 7];
    const MIN: [u8; 3] = [0, 3, 7];

    fn chord(root: u8, triad: [u8; 3]) -> ChordSpec {
        ChordSpec { root, triad, seventh: 10, written_seventh: false, extra: 0 }
    }

    #[test]
    fn voice_leading_minimises_motion() {
        // I–V–vi–IV in C on the default keyboard (C4..B6 = 48..83).
        let prog = [chord(0, MAJ), chord(7, MAJ), chord(9, MIN), chord(5, MAJ)];
        let v = voice_progression(0, &prog, 48..=83, Voicing::Triads, false);
        assert_eq!(v[0], [60, 64, 67], "root position, in the middle");
        for w in v.windows(2) {
            assert!(motion(&w[0], &w[1]) <= 6, "{v:?}");
        }
        assert_eq!(v, [vec![60, 64, 67], vec![59, 62, 67], vec![60, 64, 69], vec![60, 65, 69]]);
        for c in &v {
            assert!(c.iter().all(|n| (48..=83).contains(n)));
        }
    }

    #[test]
    fn sevenths_spread_and_bass() {
        let prog = [ChordSpec { seventh: 11, ..chord(0, MAJ) }];
        let v = voice_progression(0, &prog, 48..=83, Voicing::Sevenths, false);
        assert_eq!(v[0].len(), 4);
        let v = voice_progression(0, &[chord(0, MAJ)], 48..=83, Voicing::Spread, false);
        assert_eq!(v[0], [60, 67, 76], "the third an octave up");
        let v = voice_progression(0, &[chord(0, MAJ)], 48..=83, Voicing::Triads, true);
        assert_eq!(v[0], [48, 60, 64, 67], "the root an octave below");
        // A tiny range still gives the chord.
        let v = voice_progression(0, &[chord(0, MAJ)], 60..=64, Voicing::Triads, false);
        assert_eq!(v[0].len(), 3);
    }
}

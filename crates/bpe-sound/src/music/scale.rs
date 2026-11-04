//! Scales, and moving notes from one key into another.

use bpe_kit::sound::{Root, Scale};

/// The semitones above the root that a scale plays, lowest first.
pub fn scale_steps(scale: Scale) -> &'static [u8] {
    match scale {
        Scale::Major => &[0, 2, 4, 5, 7, 9, 11],
        Scale::Minor => &[0, 2, 3, 5, 7, 8, 10],
        Scale::Dorian => &[0, 2, 3, 5, 7, 9, 10],
        Scale::Phrygian => &[0, 1, 3, 5, 7, 8, 10],
        Scale::Lydian => &[0, 2, 4, 6, 7, 9, 11],
        Scale::Mixolydian => &[0, 2, 4, 5, 7, 9, 10],
        Scale::HarmonicMinor => &[0, 2, 3, 5, 7, 8, 11],
        Scale::MelodicMinor => &[0, 2, 3, 5, 7, 9, 11],
        Scale::MajorPentatonic => &[0, 2, 4, 7, 9],
        Scale::MinorPentatonic => &[0, 3, 5, 7, 10],
        Scale::Blues => &[0, 3, 5, 6, 7, 10],
        Scale::Chromatic => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
    }
}

/// Whether MIDI note `midi` is in the key.
pub fn in_scale(root: Root, scale: Scale, midi: u8) -> bool {
    let step = (midi as i32 - root.semitone() as i32).rem_euclid(12) as u8;
    scale_steps(scale).contains(&step)
}

/// The scale degree (index into [`scale_steps`]) of `midi`, if it is in the key.
fn degree(root: Root, scale: Scale, midi: i32) -> Option<usize> {
    let step = (midi - root.semitone() as i32).rem_euclid(12) as u8;
    scale_steps(scale).iter().position(|&s| s == step)
}

/// The note of the key nearest `midi`, ties going down.
fn nearest_in(root: Root, scale: Scale, midi: i32) -> i32 {
    for d in 0..12 {
        for n in [midi - d, midi + d] {
            if (0..=127).contains(&n) && in_scale(root, scale, n as u8) {
                return n;
            }
        }
    }
    midi.clamp(0, 127)
}

/// `notes` moved from one key into another: every note moves by the root interval taken the
/// nearest way (−6..+5 semitones). A note of the old scale keeps its scale degree when both
/// scales have as many notes (C major's E becomes G major's B); any other note goes to the
/// nearest note of the new scale, ties down. Notes are never dropped: a note that would land
/// on one already taken goes to the next free note of the key, up first. The result is sorted.
pub fn move_into_key(notes: &[u8], from: (Root, Scale), to: (Root, Scale)) -> Vec<u8> {
    let shift = {
        let d = (to.0.semitone() as i32 - from.0.semitone() as i32).rem_euclid(12);
        if d > 5 { d - 12 } else { d }
    };
    let (old, new) = (scale_steps(from.1), scale_steps(to.1));
    let mut out: Vec<u8> = Vec::with_capacity(notes.len());
    for &n in notes {
        let moved = n as i32 + shift;
        let target = match degree(from.0, from.1, n as i32) {
            Some(d) if old.len() == new.len() => {
                // The same degree of the new scale, in the octave nearest the moved note.
                let pc = (to.0.semitone() as i32 + new[d] as i32).rem_euclid(12);
                let base = moved - (moved - pc).rem_euclid(12);
                [base - 12, base, base + 12].into_iter().min_by_key(|c| ((c - moved).abs(), *c)).unwrap_or(base)
            }
            _ => nearest_in(to.0, to.1, moved),
        };
        // Off either end of the MIDI range: an octave back in, so it stays in the key.
        let mut target = target;
        while target > 127 {
            target -= 12;
        }
        while target < 0 {
            target += 12;
        }
        let free = |c: i32| (0..=127).contains(&c) && !out.contains(&(c as u8));
        let pick = if free(target) {
            target
        } else {
            (1..128)
                .flat_map(|d| [target + d, target - d])
                .find(|&c| free(c) && in_scale(to.0, to.1, c as u8))
                .or_else(|| (1..128).flat_map(|d| [target + d, target - d]).find(|&c| free(c)))
                .unwrap_or(target)
        };
        out.push(pick as u8);
    }
    out.sort_unstable();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn major_and_minor_steps() {
        assert_eq!(scale_steps(Scale::Major), [0, 2, 4, 5, 7, 9, 11]);
        assert_eq!(scale_steps(Scale::Minor), [0, 2, 3, 5, 7, 8, 10]);
        assert_eq!(scale_steps(Scale::Chromatic).len(), 12);
        assert_eq!(scale_steps(Scale::MajorPentatonic).len(), 5);
        assert!(in_scale(Root::C, Scale::Major, 60));
        assert!(!in_scale(Root::C, Scale::Major, 61));
        assert!(in_scale(Root::A, Scale::Minor, 60), "C is in A minor");
        assert!(in_scale(Root::Fs, Scale::Major, 70), "A# is in F# major");
        for s in Scale::ALL {
            let steps = scale_steps(s);
            assert_eq!(steps[0], 0, "{s:?}");
            assert!(steps.windows(2).all(|w| w[0] < w[1]) && steps.iter().all(|&x| x < 12), "{s:?}");
        }
    }

    #[test]
    fn move_into_key_keeps_degrees() {
        let c = (Root::C, Scale::Major);
        assert_eq!(move_into_key(&[60, 64, 67], c, (Root::G, Scale::Major)), [55, 59, 62]);
        assert_eq!(move_into_key(&[60, 64, 67], c, (Root::D, Scale::Major)), [62, 66, 69]);
        // Same root, the minor scale: the degrees move to the minor's.
        assert_eq!(move_into_key(&[60, 64, 67, 71], c, (Root::C, Scale::Minor)), [60, 63, 67, 70]);
        // A note outside the old key goes to the nearest of the new one, ties down.
        assert_eq!(move_into_key(&[61], c, (Root::C, Scale::Major)), [60]);
        // To a pentatonic: snapped, never dropped (E and F both want E; F takes the next free).
        let moved = move_into_key(&[64, 65], c, (Root::C, Scale::MajorPentatonic));
        assert_eq!(moved.len(), 2);
        assert!(moved.iter().all(|&n| in_scale(Root::C, Scale::MajorPentatonic, n)), "{moved:?}");
        // Nothing moves when the key stays.
        assert_eq!(move_into_key(&[57, 60, 64], c, c), [57, 60, 64]);
        // The ends of the MIDI range hold.
        assert_eq!(move_into_key(&[127], c, (Root::Cs, Scale::Major)), [116], "G9 to G#8, an octave in");
    }
}

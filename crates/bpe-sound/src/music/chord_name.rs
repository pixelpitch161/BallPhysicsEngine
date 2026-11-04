//! Naming the chord a slot's notes make: "Am", "F", "C/G", "G7", "Dm7b5".

use bpe_kit::sound::NOTE_NAMES;

/// The name of MIDI note `note`, FL Studio style: 60 is "C5".
pub fn note_name(note: u8) -> String {
    format!("{}{}", NOTE_NAMES[note as usize % 12], note / 12)
}

/// Chord shapes by their semitones above the root, in the order they are tried, and the name
/// each adds after the root.
const CHORDS: [(&[u8], &str); 16] = [
    (&[0, 4, 7], ""),
    (&[0, 3, 7], "m"),
    (&[0, 3, 6], "dim"),
    (&[0, 4, 8], "aug"),
    (&[0, 2, 7], "sus2"),
    (&[0, 5, 7], "sus4"),
    (&[0, 4, 7, 10], "7"),
    (&[0, 4, 7, 11], "maj7"),
    (&[0, 3, 7, 10], "m7"),
    (&[0, 3, 6, 10], "m7b5"),
    (&[0, 3, 6, 9], "dim7"),
    (&[0, 4, 7, 9], "6"),
    (&[0, 3, 7, 9], "m6"),
    (&[0, 2, 4, 7], "add9"),
    (&[0, 2, 4, 7, 10], "9"),
    (&[0, 2, 3, 7, 10], "m9"),
];

/// The name of the chord `notes` make. A chord whose lowest note is not its root is written
/// with that note after a slash ("C/G"). One note is that note's name ("C5"); notes that make
/// no chord here give "".
pub fn chord_name(notes: &[u8]) -> String {
    let Some(&bass) = notes.iter().min() else { return String::new() };
    let mut pcs: Vec<u8> = notes.iter().map(|n| n % 12).collect();
    pcs.sort_unstable();
    pcs.dedup();
    if pcs.len() == 1 {
        return if notes.len() == 1 { note_name(bass) } else { NOTE_NAMES[bass as usize % 12].to_owned() };
    }
    let bass_pc = bass % 12;
    // The bass as root first, so C E G A is C6 and A C E G is Am7.
    let mut roots = vec![bass_pc];
    roots.extend(pcs.iter().copied().filter(|&p| p != bass_pc));
    for &root in &roots {
        for &(shape, suffix) in &CHORDS {
            let mut rel: Vec<u8> = pcs.iter().map(|&p| (p + 12 - root) % 12).collect();
            rel.sort_unstable();
            if rel == shape {
                let name = format!("{}{}", NOTE_NAMES[root as usize], suffix);
                return if root == bass_pc { name } else { format!("{name}/{}", NOTE_NAMES[bass_pc as usize]) };
            }
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chord_names_table() {
        let table: [(&[u8], &str); 16] = [
            (&[57, 60, 64], "Am"),
            (&[53, 57, 60], "F"),
            (&[55, 60, 64], "C/G"),
            (&[55, 59, 62, 65], "G7"),
            (&[62, 65, 68, 72], "Dm7b5"),
            (&[60], "C5"),
            (&[60, 64, 67, 71], "Cmaj7"),
            (&[60, 63, 66], "Cdim"),
            (&[60, 62, 67], "Csus2"),
            (&[57, 60, 64, 67], "Am7"),
            (&[60, 64, 67, 69], "C6"),
            (&[64, 67, 72], "C/E"),
            (&[48, 60, 64, 67], "C"),
            (&[59, 62, 65, 68], "Bdim7"),
            (&[], ""),
            (&[60, 61], ""),
        ];
        for (notes, name) in table {
            assert_eq!(chord_name(notes), name, "{notes:?}");
        }
        assert_eq!(note_name(61), "C#5");
    }
}

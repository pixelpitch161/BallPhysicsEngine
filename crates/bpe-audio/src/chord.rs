//! Chord names to pitch classes.

/// Parses a chord name such as "C", "F#m", "Bb7", "Dsus4" or "Amaj7" into pitch classes
/// (0 = C, 1 = C#, ... 11 = B). Returns `None` for names it does not understand.
pub fn pitch_classes(name: &str) -> Option<Vec<u8>> {
    let name = name.trim();
    let mut chars = name.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    let mut root: i32 = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let mut rest = chars.as_str();
    if let Some(r) = rest.strip_prefix('#') {
        root += 1;
        rest = r;
    } else if let Some(r) = rest.strip_prefix('b') {
        root -= 1;
        rest = r;
    }
    let intervals: &[i32] = match rest {
        "" | "maj" => &[0, 4, 7],
        "m" | "min" => &[0, 3, 7],
        "7" => &[0, 4, 7, 10],
        "maj7" => &[0, 4, 7, 11],
        "m7" | "min7" => &[0, 3, 7, 10],
        "6" => &[0, 4, 7, 9],
        "m6" => &[0, 3, 7, 9],
        "9" => &[0, 4, 7, 10, 14],
        "add9" => &[0, 4, 7, 14],
        "madd9" => &[0, 3, 7, 14],
        "sus2" => &[0, 2, 7],
        "sus4" | "sus" => &[0, 5, 7],
        "7sus4" => &[0, 5, 7, 10],
        "dim" => &[0, 3, 6],
        "aug" | "+" => &[0, 4, 8],
        "5" => &[0, 7],
        _ => return None,
    };
    let mut classes: Vec<u8> = intervals.iter().map(|i| (root + i).rem_euclid(12) as u8).collect();
    classes.sort_unstable();
    classes.dedup();
    Some(classes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_chords() {
        assert_eq!(pitch_classes("C"), Some(vec![0, 4, 7]));
        assert_eq!(pitch_classes("Am"), Some(vec![0, 4, 9]));
        assert_eq!(pitch_classes("Dsus4"), Some(vec![2, 7, 9]));
        assert_eq!(pitch_classes("Bb7"), Some(vec![2, 5, 8, 10]));
        assert_eq!(pitch_classes("F#m"), Some(vec![1, 6, 9]));
        assert_eq!(pitch_classes("H"), None);
        assert_eq!(pitch_classes("Cweird"), None);
    }
}

//! A chord progression laid out in time, with the playable notes of each chord worked out.

/// The chords of a scene, repeating, with the notes each one offers between two MIDI keys.
#[derive(Clone, Debug, PartialEq)]
pub struct Chords {
    /// Each step's end time in the cycle, and its notes.
    steps: Vec<(f64, Vec<u8>)>,
    cycle: f64,
}

impl Chords {
    /// Parses steps of `"<chord> <seconds>"` separated by commas, such as `"C 4.2, Am 5"`.
    /// A step whose chord name is unknown or whose seconds don't parse is skipped. With no
    /// usable steps at all, one 1 s step of C major stands in (or every note in range, when C
    /// major has none there). Notes run from `low` to `high`, either way round.
    pub fn parse(text: &str, low: u8, high: u8) -> Chords {
        let (low, high) = (low.min(high).min(127), low.max(high).min(127));
        let in_range = |classes: &[u8]| -> Vec<u8> { (low..=high).filter(|k| classes.contains(&(k % 12))).collect() };
        let mut steps = Vec::new();
        let mut end = 0.0;
        for step in text.split(',') {
            let mut parts = step.split_whitespace();
            let (Some(name), Some(seconds)) = (parts.next(), parts.next()) else { continue };
            let (Some(classes), Ok(seconds)) = (pitch_classes(name), seconds.parse::<f64>()) else { continue };
            if seconds <= 0.0 || !seconds.is_finite() {
                continue;
            }
            end += seconds;
            steps.push((end, in_range(&classes)));
        }
        if steps.is_empty() {
            let mut notes = in_range(&[0, 4, 7]);
            if notes.is_empty() {
                notes = (low..=high).collect();
            }
            steps.push((1.0, notes));
            end = 1.0;
        }
        Chords { steps, cycle: end }
    }

    /// The notes of the chord playing at `time` seconds.
    pub fn notes_at(&self, time: f64) -> &[u8] {
        let t = time.rem_euclid(self.cycle);
        let i = self.steps.partition_point(|(end, _)| *end <= t).min(self.steps.len() - 1);
        &self.steps[i].1
    }

    /// How long one time through every chord takes.
    pub fn cycle(&self) -> f64 {
        self.cycle
    }
}

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
    fn chords_parse_and_skip_what_they_dont_know() {
        let c = Chords::parse("C 2, Xy 3, Am 1", 60, 72);
        assert_eq!(c.notes_at(0.5), &[60, 64, 67, 72]);
        assert_eq!(c.notes_at(2.5), &[60, 64, 69, 72]);
        assert_eq!(c.notes_at(3.5), &[60, 64, 67, 72]); // the cycle is 3 s
        assert_eq!(c.notes_at(-1.0), &[60, 64, 69, 72]); // wraps round the cycle
        assert_eq!(Chords::parse("", 60, 67).notes_at(0.0), &[60, 64, 67]);
        assert_eq!(Chords::parse("nonsense", 61, 61).notes_at(0.0), &[61]);
        assert_eq!(Chords::parse("G 1", 72, 60).notes_at(0.0), &[62, 67, 71]);
        assert_eq!(Chords::parse("C 0, Am 1", 60, 72).notes_at(0.5), &[60, 64, 69, 72]);
    }

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

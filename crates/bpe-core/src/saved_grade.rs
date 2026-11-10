//! A colour grade kept to come back to (colour grading part 6): the Grade card's **snapshots**
//! (named versions of the whole grade, kept on the card) and **grade files** (`.grade`, one grade
//! to use in another project). Both hold a [`SavedGrade`]: the card's settings, its masks, parts
//! and panels, and the keyframes and pulses of its settings.

use crate::{Layer, Params, Pulse, Scene, Track, Value};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The Grade card's setting that holds its snapshots.
pub const GRADE_SNAPSHOTS_KEY: &str = "snapshots";

/// The ending of a grade file's name.
pub const GRADE_EXTENSION: &str = "grade";

/// A whole grade: what the Grade card holds but its snapshots, and its keys and pulses by the
/// setting's key on the card.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedGrade {
    #[serde(skip_serializing_if = "Params::is_empty")]
    pub params: Params,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<Layer>,
    #[serde(skip_serializing_if = "is_zero")]
    pub used_ids: u32,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub tracks: BTreeMap<String, Track>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub pulses: BTreeMap<String, Pulse>,
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

/// One snapshot: a name and the grade as it was when it was saved (or last updated).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GradeSnapshot {
    pub name: String,
    #[serde(default)]
    pub grade: SavedGrade,
}

impl SavedGrade {
    /// Whether every number in it is finite and it holds no snapshots of its own.
    pub fn is_valid(&self) -> bool {
        let keyed = self.tracks.values().flat_map(|t| t.keys.iter().map(|k| &k.value));
        let pulsed = self.pulses.values().map(|p| &p.amount);
        self.params.0.values().chain(keyed).chain(pulsed).all(finite)
    }
}

impl GradeSnapshot {
    pub fn is_valid(&self) -> bool {
        self.grade.is_valid()
    }
}

/// Whether a saved value is finite all through, and not snapshots (never saved in snapshots).
fn finite(v: &Value) -> bool {
    match v {
        Value::Number(x) => x.is_finite(),
        Value::Vec2([x, y]) => x.is_finite() && y.is_finite(),
        Value::Points(ps) => ps.iter().flatten().all(|x| x.is_finite()),
        Value::Strokes(s) => s.is_finite(),
        Value::Curve(c) => c.iter().all(|p| p.x.is_finite() && p.y.is_finite() && p.bend.is_finite()),
        Value::Snapshots(_) => false,
        _ => true,
    }
}

/// What a `.grade` file holds: the scene file's format number and the grade.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct GradeFile {
    format: u32,
    grade: SavedGrade,
}

/// `grade` as the text of a `.grade` file.
pub fn grade_file_text(grade: &SavedGrade) -> String {
    let file = GradeFile { format: Scene::FORMAT, grade: grade.clone() };
    ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default()).expect("grades always serialize")
}

/// The grade a `.grade` file's `text` holds, or why it can't be read, in plain words. The grade
/// still needs cleaning against the registry (`save::clean_card`) before it goes on a scene.
pub fn read_grade_file(text: &str) -> Result<SavedGrade, String> {
    let file: GradeFile = ron::from_str(text).map_err(|_| "That file isn't a saved grade.".to_string())?;
    if file.format == 0 {
        return Err("That file isn't a saved grade.".into());
    }
    if file.format > Scene::FORMAT {
        return Err("That grade was saved by a newer version of Ball Studio.".into());
    }
    if file.format < Scene::OLDEST_FORMAT {
        return Err("That grade was saved by an earlier version of Ball Studio that this one can't open.".into());
    }
    if !file.grade.is_valid() {
        return Err("That grade file is damaged.".into());
    }
    Ok(file.grade)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Key, LayerId};

    fn grade() -> SavedGrade {
        let mut params = Params::default();
        params.set("0/1/exposure", Value::Number(0.5));
        let layers = vec![Layer {
            id: LayerId(0),
            kind: "grade.whole".into(),
            name: "Whole picture".into(),
            ..Default::default()
        }];
        let mut tracks = BTreeMap::new();
        tracks.insert(
            "1/contrast".to_string(),
            Track { keys: vec![Key { time: 1.0, value: Value::Number(20.0), ease: Default::default() }], range: None },
        );
        SavedGrade { params, layers, used_ids: 0, tracks, pulses: BTreeMap::new() }
    }

    #[test]
    fn a_grade_file_reads_back_what_was_written() {
        let g = grade();
        let text = grade_file_text(&g);
        assert_eq!(read_grade_file(&text), Ok(g));
    }

    #[test]
    fn a_newer_broken_or_other_file_says_so() {
        let newer = grade_file_text(&grade()).replacen(&format!("format: {}", Scene::FORMAT), "format: 999", 1);
        assert!(read_grade_file(&newer).unwrap_err().contains("newer"));
        assert!(read_grade_file("not ron at all").unwrap_err().contains("isn't a saved grade"));
        assert!(read_grade_file("(grade: ())").unwrap_err().contains("isn't a saved grade"), "no format");
    }

    #[test]
    fn nan_and_snapshots_inside_snapshots_are_refused() {
        let mut g = grade();
        assert!(g.is_valid());
        g.params.set("0/1/exposure", Value::Number(f64::NAN));
        assert!(!g.is_valid());
        let mut g = grade();
        g.params.set(GRADE_SNAPSHOTS_KEY, Value::Snapshots(vec![]));
        assert!(!g.is_valid(), "a snapshot holds no snapshots");
        let mut g = grade();
        g.tracks.get_mut("1/contrast").unwrap().keys[0].value = Value::Number(f64::INFINITY);
        assert!(!g.is_valid(), "a key's value too");
    }
}

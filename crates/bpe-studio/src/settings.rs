//! Remembers the last scene file and the score weights between runs, in small files in the
//! user's settings folder. The old and new editors share the folder and keep separate files.

use bpe_kit::score::Parts;
use std::path::{Path, PathBuf};

/// The user's settings folder for Ball Studio.
pub fn settings_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("Ball Studio"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|d| d.join("ball-studio"))
    }
}

/// How many recently opened scene files the Start window lists.
pub const RECENT: usize = 8;

const LAST_SCENE: &str = "last-scene-next.txt";
const RECENT_SCENES: &str = "recent-scenes-next.txt";

/// The scene file that was open last time, if it still exists.
pub fn last_scene() -> Option<PathBuf> {
    last_scene_in(&settings_dir()?)
}

pub fn last_scene_in(dir: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(dir.join(LAST_SCENE)).ok()?;
    let path = PathBuf::from(text.trim());
    path.is_file().then_some(path)
}

/// The scene files opened or saved lately that still exist, newest first.
pub fn recent_scenes() -> Vec<PathBuf> {
    settings_dir().map(|d| recent_scenes_in(&d)).unwrap_or_default()
}

pub fn recent_scenes_in(dir: &Path) -> Vec<PathBuf> {
    read_recent(dir).into_iter().filter(|p| p.is_file()).take(RECENT).collect()
}

fn read_recent(dir: &Path) -> Vec<PathBuf> {
    let text = std::fs::read_to_string(dir.join(RECENT_SCENES)).unwrap_or_default();
    text.lines().map(str::trim).filter(|l| !l.is_empty()).map(PathBuf::from).collect()
}

/// Remembers `path` as the scene to open next time, and at the top of the recent files.
pub fn remember_scene(path: &Path) {
    if let Some(dir) = settings_dir() {
        remember_scene_in(&dir, path);
    }
}

pub fn remember_scene_in(dir: &Path, path: &Path) {
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(dir.join(LAST_SCENE), path.display().to_string());
    let mut recent = read_recent(dir);
    recent.retain(|p| p != path && p.is_file());
    recent.insert(0, path.to_path_buf());
    recent.truncate(RECENT);
    let text: Vec<String> = recent.iter().map(|p| p.display().to_string()).collect();
    let _ = std::fs::write(dir.join(RECENT_SCENES), text.join("\n"));
}

/// Where "My presets" live.
pub fn presets_dir() -> Option<PathBuf> {
    settings_dir().map(|d| d.join("presets"))
}

fn weights_file() -> Option<PathBuf> {
    settings_dir().map(|d| d.join("score-weights.ron"))
}

/// The score weights the user taught last time, or the defaults.
pub fn load_weights() -> Parts {
    weights_file().and_then(|f| load_weights_from(&f)).unwrap_or(Parts::WEIGHTS)
}

pub fn load_weights_from(path: &Path) -> Option<Parts> {
    ron::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

pub fn save_weights(weights: &Parts) {
    if let Some(file) = weights_file() {
        let _ = save_weights_to(&file, weights);
    }
}

pub fn save_weights_to(path: &Path, weights: &Parts) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = ron::ser::to_string_pretty(weights, Default::default()).map_err(std::io::Error::other)?;
    std::fs::write(path, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_scenes_keep_the_last_eight_existing_files_newest_first() {
        let dir = std::env::temp_dir().join(format!("ball-studio-recent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(recent_scenes_in(&dir).is_empty());
        let files: Vec<PathBuf> = (0..10).map(|i| dir.join(format!("scene {i}.ron"))).collect();
        for file in &files {
            std::fs::write(file, "").unwrap();
            remember_scene_in(&dir, file);
        }
        remember_scene_in(&dir, &files[5]);
        let recent = recent_scenes_in(&dir);
        assert_eq!(recent.len(), RECENT);
        assert_eq!(recent[0], files[5], "opened again, so it moves to the top");
        assert_eq!(recent[1], files[9]);
        assert_eq!(recent.iter().filter(|p| **p == files[5]).count(), 1, "no doubles");
        std::fs::remove_file(&files[9]).unwrap();
        assert!(!recent_scenes_in(&dir).contains(&files[9]), "a file that is gone is left out");
        assert_eq!(last_scene_in(&dir), Some(files[5].clone()));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn score_weights_round_trip_and_missing_means_defaults() {
        let dir = std::env::temp_dir().join(format!("ball-studio-weights-{}", std::process::id()));
        let file = dir.join("score-weights.ron");
        assert_eq!(load_weights_from(&file), None);
        let taught = Parts { rhythm: 1.4, ..Parts::WEIGHTS };
        save_weights_to(&file, &taught).unwrap();
        assert_eq!(load_weights_from(&file), Some(taught));
        let _ = std::fs::remove_dir_all(dir);
    }
}

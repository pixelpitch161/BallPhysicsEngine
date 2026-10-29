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

fn settings_file() -> Option<PathBuf> {
    settings_dir().map(|d| d.join("last-scene-next.txt"))
}

/// The scene file that was open last time, if it still exists.
pub fn last_scene() -> Option<PathBuf> {
    let text = std::fs::read_to_string(settings_file()?).ok()?;
    let path = PathBuf::from(text.trim());
    path.is_file().then_some(path)
}

pub fn remember_scene(path: &std::path::Path) {
    if let Some(file) = settings_file() {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(file, path.display().to_string());
    }
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

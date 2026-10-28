//! Remembers the last scene file between runs, in a tiny text file in the user's settings folder.

use std::path::PathBuf;

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
    settings_dir().map(|d| d.join("last-scene.txt"))
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

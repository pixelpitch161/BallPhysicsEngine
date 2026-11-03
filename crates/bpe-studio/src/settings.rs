//! Remembers the recent scene files and the layout between runs, in small files in the
//! user's settings folder. The old and new editors share the folder and keep separate files.

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

/// How many recently opened scene files the Recent menu lists.
pub const RECENT: usize = 8;

const RECENT_SCENES: &str = "recent-scenes-next.txt";

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

/// Puts `path` at the top of the recent files.
pub fn remember_scene(path: &Path) {
    if let Some(dir) = settings_dir() {
        remember_scene_in(&dir, path);
    }
}

pub fn remember_scene_in(dir: &Path, path: &Path) {
    let _ = std::fs::create_dir_all(dir);
    let mut recent = read_recent(dir);
    recent.retain(|p| p != path && p.is_file());
    recent.insert(0, path.to_path_buf());
    recent.truncate(RECENT);
    let text: Vec<String> = recent.iter().map(|p| p.display().to_string()).collect();
    let _ = std::fs::write(dir.join(RECENT_SCENES), text.join("\n"));
}

const INSPECTOR_WIDTH: &str = "inspector-width-v2.txt";

/// The inspector's width when nothing is saved.
pub const INSPECTOR_DEFAULT: f32 = crate::style::INSPECTOR_W;
/// The narrowest and widest the inspector can be dragged.
pub const INSPECTOR_RANGE: std::ops::RangeInclusive<f32> = 320.0..=640.0;

/// The inspector width the user dragged to last time, or the default.
pub fn load_inspector_width() -> f32 {
    settings_dir().map_or(INSPECTOR_DEFAULT, |d| load_inspector_width_in(&d))
}

/// The saved inspector width in `dir`: the default when the file is missing or unreadable,
/// and kept between the narrowest and widest otherwise.
pub fn load_inspector_width_in(dir: &Path) -> f32 {
    let text = std::fs::read_to_string(dir.join(INSPECTOR_WIDTH)).unwrap_or_default();
    match text.trim().parse::<f32>() {
        Ok(w) if w.is_finite() => w.clamp(*INSPECTOR_RANGE.start(), *INSPECTOR_RANGE.end()),
        _ => INSPECTOR_DEFAULT,
    }
}

/// Remembers the inspector's width for next time.
pub fn save_inspector_width(w: f32) {
    if let Some(dir) = settings_dir() {
        save_inspector_width_in(&dir, w);
    }
}

pub fn save_inspector_width_in(dir: &Path, w: f32) {
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(dir.join(INSPECTOR_WIDTH), format!("{}", w.round()));
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
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_saved_width_is_clamped_and_garbage_ignored() {
        let dir = std::env::temp_dir().join(format!("ball-studio-width-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(load_inspector_width_in(&dir), 370.0, "a missing file");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(INSPECTOR_WIDTH), "1000").unwrap();
        assert_eq!(load_inspector_width_in(&dir), 640.0);
        std::fs::write(dir.join(INSPECTOR_WIDTH), "abc").unwrap();
        assert_eq!(load_inspector_width_in(&dir), 370.0);
        save_inspector_width_in(&dir, 500.0);
        assert_eq!(load_inspector_width_in(&dir), 500.0);
        let _ = std::fs::remove_dir_all(dir);
    }
}

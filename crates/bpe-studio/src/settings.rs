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

const SOUND_WIDTH: &str = "sound-inspector-width.txt";

/// The inspector's width while a shape's Sound tab is open, when nothing is saved: Osc A and
/// Osc B side by side.
pub const SOUND_WIDTH_DEFAULT: f32 = 760.0;
/// The narrowest and widest the inspector can be dragged on the Sound tab.
pub const SOUND_WIDTH_RANGE: std::ops::RangeInclusive<f32> = 640.0..=960.0;

/// The Sound tab's width the user dragged to last time, or the default.
pub fn load_sound_width() -> f32 {
    settings_dir().map_or(SOUND_WIDTH_DEFAULT, |d| load_sound_width_in(&d))
}

pub fn load_sound_width_in(dir: &Path) -> f32 {
    let text = std::fs::read_to_string(dir.join(SOUND_WIDTH)).unwrap_or_default();
    match text.trim().parse::<f32>() {
        Ok(w) if w.is_finite() => w.clamp(*SOUND_WIDTH_RANGE.start(), *SOUND_WIDTH_RANGE.end()),
        _ => SOUND_WIDTH_DEFAULT,
    }
}

pub fn save_sound_width(w: f32) {
    if let Some(dir) = settings_dir() {
        save_sound_width_in(&dir, w);
    }
}

pub fn save_sound_width_in(dir: &Path, w: f32) {
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(dir.join(SOUND_WIDTH), format!("{}", w.round()));
}

const LISTEN: &str = "listening.txt";

/// The editor's own listening: the speaker on or off and its volume. Only what you hear; the
/// export is not changed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ListenSettings {
    pub on: bool,
    /// 0..1.
    pub volume: f32,
}

impl Default for ListenSettings {
    fn default() -> Self {
        ListenSettings { on: true, volume: 0.8 }
    }
}

pub fn load_listen() -> ListenSettings {
    settings_dir().map_or_else(ListenSettings::default, |d| load_listen_in(&d))
}

/// The saved listening settings in `dir` (`on 0.8` or `off 0.5`), or the defaults.
pub fn load_listen_in(dir: &Path) -> ListenSettings {
    let text = std::fs::read_to_string(dir.join(LISTEN)).unwrap_or_default();
    let mut words = text.split_whitespace();
    let on = match words.next() {
        Some("on") => true,
        Some("off") => false,
        _ => return ListenSettings::default(),
    };
    let volume = words.next().and_then(|v| v.parse::<f32>().ok()).filter(|v| v.is_finite());
    ListenSettings { on, volume: volume.map_or(ListenSettings::default().volume, |v| v.clamp(0.0, 1.0)) }
}

pub fn save_listen(listen: ListenSettings) {
    if let Some(dir) = settings_dir() {
        save_listen_in(&dir, listen);
    }
}

pub fn save_listen_in(dir: &Path, listen: ListenSettings) {
    let _ = std::fs::create_dir_all(dir);
    let word = if listen.on { "on" } else { "off" };
    let _ = std::fs::write(dir.join(LISTEN), format!("{word} {:.3}", listen.volume));
}

const BOTTOM_VIEW: &str = "bottom-view.txt";

/// What the bottom panel shows: the timeline or the scene's rules (the editor's own, never saved
/// in a scene).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BottomView {
    #[default]
    Timeline,
    Rules,
}

pub fn load_bottom_view() -> BottomView {
    settings_dir().map_or_else(BottomView::default, |d| load_bottom_view_in(&d))
}

/// The bottom view saved in `dir` (`timeline` or `rules`); the timeline when there is none.
pub fn load_bottom_view_in(dir: &Path) -> BottomView {
    match std::fs::read_to_string(dir.join(BOTTOM_VIEW)).unwrap_or_default().trim() {
        "rules" => BottomView::Rules,
        _ => BottomView::Timeline,
    }
}

pub fn save_bottom_view(view: BottomView) {
    if let Some(dir) = settings_dir() {
        save_bottom_view_in(&dir, view);
    }
}

pub fn save_bottom_view_in(dir: &Path, view: BottomView) {
    let _ = std::fs::create_dir_all(dir);
    let word = match view {
        BottomView::Timeline => "timeline",
        BottomView::Rules => "rules",
    };
    let _ = std::fs::write(dir.join(BOTTOM_VIEW), word);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bottom_view_round_trips_and_defaults_to_the_timeline() {
        let dir = std::env::temp_dir().join(format!("ball-studio-bottom-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(load_bottom_view_in(&dir), BottomView::Timeline, "a missing file");
        save_bottom_view_in(&dir, BottomView::Rules);
        assert_eq!(load_bottom_view_in(&dir), BottomView::Rules);
        std::fs::write(dir.join(BOTTOM_VIEW), "sideways").unwrap();
        assert_eq!(load_bottom_view_in(&dir), BottomView::Timeline, "an odd file");
        save_bottom_view_in(&dir, BottomView::Timeline);
        assert_eq!(load_bottom_view_in(&dir), BottomView::Timeline);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn listening_settings_round_trip_and_default_to_on_at_80_percent() {
        let dir = std::env::temp_dir().join(format!("ball-studio-listen-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(load_listen_in(&dir), ListenSettings { on: true, volume: 0.8 });
        save_listen_in(&dir, ListenSettings { on: false, volume: 0.25 });
        assert_eq!(load_listen_in(&dir), ListenSettings { on: false, volume: 0.25 });
        let _ = std::fs::remove_dir_all(&dir);
    }

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

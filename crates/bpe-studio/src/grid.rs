//! The editor's grid: lines over the preview and the snapping of drags to them. The grid is
//! measured from the video's centre, lives only in the editor (never in the video or the scene
//! file) and is remembered between runs in the settings folder.

use crate::settings::settings_dir;
use eframe::egui::{Color32, Painter, Pos2, Rect, Stroke};
use glam::Vec2;
use std::path::Path;

/// The grid spacing the toolbar allows, in world pixels.
pub const SPACING: std::ops::RangeInclusive<f32> = 10.0..=400.0;

/// Lines closer than this on screen are too dense to help, so every other one is skipped.
const MIN_LINE_GAP: f32 = 6.0;

const GRID_FILE: &str = "grid-next.txt";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    /// Draw the grid lines over the preview.
    pub show: bool,
    /// Drags land on the grid. Works with the lines hidden too.
    pub snap: bool,
    /// The size of one square, in world pixels.
    pub spacing: f32,
}

impl Default for Grid {
    fn default() -> Self {
        Self { show: false, snap: false, spacing: 60.0 }
    }
}

impl Grid {
    /// The grid step drags snap to, when snapping is on and not held off (Alt).
    pub fn step(&self, held_off: bool) -> Option<f32> {
        (self.snap && !held_off).then_some(self.spacing)
    }

    /// Draws the lines over the picture: faint squares and stronger lines through the centre.
    /// `centre` is the video's centre on screen and `scale` screen points per world pixel.
    pub fn draw(&self, painter: &Painter, rect: Rect, centre: Pos2, scale: f32) {
        if !self.show {
            return;
        }
        let mut gap = self.spacing * scale;
        while gap > 0.0 && gap < MIN_LINE_GAP {
            gap *= 2.0;
        }
        if gap <= 0.0 {
            return;
        }
        let faint = Stroke::new(1.0, Color32::from_white_alpha(28));
        let middle = Stroke::new(1.0, Color32::from_white_alpha(80));
        let first = ((rect.left() - centre.x) / gap).ceil() as i32;
        let last = ((rect.right() - centre.x) / gap).floor() as i32;
        for i in first..=last {
            let x = centre.x + i as f32 * gap;
            let stroke = if i == 0 { middle } else { faint };
            painter.vline(x, rect.y_range(), stroke);
        }
        let first = ((rect.top() - centre.y) / gap).ceil() as i32;
        let last = ((rect.bottom() - centre.y) / gap).floor() as i32;
        for i in first..=last {
            let y = centre.y + i as f32 * gap;
            let stroke = if i == 0 { middle } else { faint };
            painter.hline(rect.x_range(), y, stroke);
        }
    }

    /// The grid as it was last time, or the defaults.
    pub fn load() -> Self {
        settings_dir().and_then(|d| Self::load_from(&d.join(GRID_FILE))).unwrap_or_default()
    }

    pub fn load_from(path: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        let mut grid = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            match key.trim() {
                "show" => grid.show = value.trim() == "true",
                "snap" => grid.snap = value.trim() == "true",
                "spacing" => {
                    if let Ok(spacing) = value.trim().parse::<f32>() {
                        grid.spacing = spacing.clamp(*SPACING.start(), *SPACING.end());
                    }
                }
                _ => {}
            }
        }
        Some(grid)
    }

    pub fn save(&self) {
        if let Some(file) = settings_dir().map(|d| d.join(GRID_FILE)) {
            let _ = self.save_to(&file);
        }
    }

    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, format!("show={}\nsnap={}\nspacing={}\n", self.show, self.snap, self.spacing))
    }
}

/// `p` moved to the nearest grid point, or left alone without a step.
pub fn snap_point(p: Vec2, step: Option<f32>) -> Vec2 {
    match step {
        Some(step) => (p / step).round() * step,
        None => p,
    }
}

/// A length rounded to whole grid steps (at least one), or left alone without a step.
pub fn snap_length(length: f32, step: Option<f32>) -> f32 {
    match step {
        Some(step) => ((length / step).round() * step).max(step),
        None => length,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_and_lengths_snap_to_whole_steps() {
        assert_eq!(snap_point(Vec2::new(29.0, -31.0), Some(60.0)), Vec2::new(0.0, -60.0));
        assert_eq!(snap_point(Vec2::new(29.0, -31.0), None), Vec2::new(29.0, -31.0));
        assert_eq!(snap_length(95.0, Some(60.0)), 120.0);
        assert_eq!(snap_length(10.0, Some(60.0)), 60.0, "never shrinks to nothing");
        assert_eq!(snap_length(95.0, None), 95.0);
    }

    #[test]
    fn alt_holds_snapping_off() {
        let grid = Grid { snap: true, ..Grid::default() };
        assert_eq!(grid.step(false), Some(60.0));
        assert_eq!(grid.step(true), None);
        assert_eq!(Grid::default().step(false), None, "snap starts off");
    }

    #[test]
    fn the_grid_round_trips_and_missing_means_defaults() {
        let dir = std::env::temp_dir().join(format!("ball-studio-grid-{}", std::process::id()));
        let file = dir.join(GRID_FILE);
        assert_eq!(Grid::load_from(&file), None);
        let grid = Grid { show: true, snap: true, spacing: 45.0 };
        grid.save_to(&file).unwrap();
        assert_eq!(Grid::load_from(&file), Some(grid));
        std::fs::write(&file, "spacing=9000\n").unwrap();
        assert_eq!(Grid::load_from(&file).unwrap().spacing, 400.0, "kept in range");
        let _ = std::fs::remove_dir_all(dir);
    }
}

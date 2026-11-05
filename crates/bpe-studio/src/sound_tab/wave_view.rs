//! The oscillator's waveform view: two cycles of the wave as Warp bends it, drawn live, with a
//! line where each cycle starts at the Phase knob (and, with Random phase, the band notes may
//! start in).

use crate::style;
use bpe_dsp::{WarpModeId, WaveShape};
use bpe_kit::sound::{WarpMode, Wave};
use eframe::egui::{self, Id, Pos2, Sense, Shape, Stroke, Ui, Vec2};

/// The view's height.
pub const HEIGHT: f32 = 64.0;
/// Points drawn across it.
const POINTS: usize = 240;

/// The wave's shape in the tables, or `None` for noise.
pub fn shape_of(wave: Wave) -> Option<WaveShape> {
    match wave {
        Wave::Sine => Some(WaveShape::Sine),
        Wave::Triangle => Some(WaveShape::Triangle),
        Wave::Saw => Some(WaveShape::Saw),
        Wave::Square => Some(WaveShape::Square),
        Wave::Noise => None,
    }
}

/// The levels (-1..1) drawn across the view, left to right: two cycles of `wave` bent by `mode`
/// at `warp` (0..1), or a stretch of noise.
pub fn levels(wave: Wave, mode: WarpMode, warp: f32) -> Vec<f32> {
    match shape_of(wave) {
        Some(shape) => {
            let cycle = bpe_dsp::warp::draw(shape, WarpModeId::from_index(mode.index()), warp, POINTS / 2);
            cycle.iter().chain(cycle.iter()).copied().collect()
        }
        None => {
            let mut noise = bpe_dsp::Noise::new(7);
            (0..POINTS).map(|_| noise.white() * 0.8).collect()
        }
    }
}

/// Where the view keeps the levels it last drew (for tests).
pub fn drawn_id(id: Id) -> Id {
    id.with("drawn")
}

/// Where a note starts in the two cycles drawn, 0..1 across the view, at `phase` degrees.
pub fn phase_x(phase: f32) -> [f32; 2] {
    let t = (phase / 360.0).rem_euclid(1.0) * 0.5;
    [t, 0.5 + t]
}

/// Where the Phase line goes: at `degrees`, in `colour`, with notes starting anywhere in the
/// `spread` (0..1 of a cycle) after it.
#[derive(Clone, Copy, Debug)]
pub struct PhaseMark {
    pub degrees: f32,
    pub spread: f32,
    pub colour: egui::Color32,
}

/// Draws the view the whole width of `ui`, named `name`, with the Phase line when `phase` is
/// given (not for noise).
pub fn show(
    ui: &mut Ui,
    id: Id,
    name: &str,
    wave: Wave,
    mode: WarpMode,
    warp: f32,
    phase: Option<PhaseMark>,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), HEIGHT), Sense::hover());
    super::name_it(ui, &response, name);
    let levels = levels(wave, mode, warp);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, style::RADIUS, style::GROUND);
        painter.rect_stroke(rect, style::RADIUS, Stroke::new(1.0, style::LINE_SOFT), egui::StrokeKind::Inside);
        let mid = rect.center().y;
        painter.line_segment(
            [Pos2::new(rect.left(), mid), Pos2::new(rect.right(), mid)],
            Stroke::new(1.0, style::LINE_SOFT),
        );
        if let Some(mark) = phase.filter(|_| shape_of(wave).is_some()) {
            let x_of = |t: f32| rect.left() + rect.width() * t;
            for t in phase_x(mark.degrees) {
                let spread = mark.spread.clamp(0.0, 1.0) * 0.5;
                if spread > 0.0 {
                    // Two cycles are drawn, so a cycle is half the width; wrap the band around.
                    let (a, b) = (t, t + spread);
                    let cycle = if t < 0.5 { (0.0, 0.5) } else { (0.5, 1.0) };
                    let band = |a: f32, b: f32| {
                        let r = egui::Rect::from_x_y_ranges(x_of(a)..=x_of(b), rect.y_range());
                        painter.rect_filled(r, 0.0, mark.colour.gamma_multiply(0.12));
                    };
                    band(a.max(cycle.0), b.min(cycle.1));
                    if a < cycle.0 {
                        band(cycle.1 - (cycle.0 - a), cycle.1);
                    }
                    if b > cycle.1 {
                        band(cycle.0, cycle.0 + (b - cycle.1));
                    }
                }
                let x = x_of(t).round() + 0.5;
                painter.line_segment(
                    [Pos2::new(x, rect.top() + 2.0), Pos2::new(x, rect.bottom() - 2.0)],
                    Stroke::new(1.0, mark.colour),
                );
            }
        }
        let half = rect.height() * 0.5 - 6.0;
        let n = levels.len().max(2);
        let points: Vec<Pos2> = levels
            .iter()
            .enumerate()
            .map(|(i, v)| {
                Pos2::new(rect.left() + rect.width() * i as f32 / (n - 1) as f32, mid - v.clamp(-1.0, 1.0) * half)
            })
            .collect();
        painter.add(Shape::line(points, Stroke::new(1.6, style::TEXT)));
    }
    ui.data_mut(|d| d.insert_temp(drawn_id(id), levels));
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warp_bends_the_drawn_wave() {
        let flat = levels(Wave::Saw, WarpMode::BendUp, 0.0);
        let bent = levels(Wave::Saw, WarpMode::BendUp, 0.6);
        assert_eq!(flat.len(), POINTS);
        assert_ne!(flat, bent);
        assert_eq!(levels(Wave::Noise, WarpMode::Sync, 0.0), levels(Wave::Noise, WarpMode::Sync, 0.9));
    }

    #[test]
    fn the_phase_line_sits_where_each_cycle_starts() {
        assert_eq!(phase_x(0.0), [0.0, 0.5]);
        assert_eq!(phase_x(90.0), [0.125, 0.625]);
        assert_eq!(phase_x(360.0), [0.0, 0.5]);
        assert_eq!(phase_x(-90.0), [0.375, 0.875]);
    }
}

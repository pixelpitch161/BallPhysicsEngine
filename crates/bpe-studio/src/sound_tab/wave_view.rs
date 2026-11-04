//! The oscillator's waveform view: two cycles of the wave as Warp bends it, drawn live.

use crate::style;
use bpe_dsp::{WarpModeId, WaveShape};
use bpe_kit::sound::{Wave, WarpMode};
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

/// Draws the view the whole width of `ui`, named `name`.
pub fn show(ui: &mut Ui, id: Id, name: &str, wave: Wave, mode: WarpMode, warp: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), HEIGHT), Sense::hover());
    super::name_it(ui, &response, name);
    let levels = levels(wave, mode, warp);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, style::RADIUS, style::GROUND);
        painter.rect_stroke(rect, style::RADIUS, Stroke::new(1.0, style::LINE_SOFT), egui::StrokeKind::Inside);
        let mid = rect.center().y;
        painter.line_segment([Pos2::new(rect.left(), mid), Pos2::new(rect.right(), mid)], Stroke::new(1.0, style::LINE_SOFT));
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
}

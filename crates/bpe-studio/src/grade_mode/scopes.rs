//! The Histogram: how many of the finished picture's pixels sit at each level of red, green,
//! blue and brightness, darks on the left. Counted on the GPU from the preview's picture
//! (`bpe_gfx::grade::Histogram`); the heights are square roots, so a few bright pixels still
//! show beside a dark picture's mass.

use crate::style;
use bpe_gfx::grade::HistogramBins;
use eframe::egui::{self, Pos2, Rect, Sense, Stroke, Ui};

pub const HEIGHT: f32 = 84.0;

pub fn histogram(ui: &mut Ui, bins: Option<&HistogramBins>) -> egui::Response {
    let width = ui.available_width().max(60.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, HEIGHT), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, "Histogram"));
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, style::RADIUS, style::GROUND);
    for i in 1..4 {
        painter.vline(rect.left() + rect.width() * i as f32 / 4.0, rect.y_range(), Stroke::new(1.0, style::LINE_SOFT));
    }
    let Some(bins) = bins else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Counting\u{2026}",
            egui::FontId::proportional(11.0),
            style::FAINT,
        );
        return response;
    };
    let inner = rect.shrink2(egui::vec2(1.0, 3.0));
    // The tallest bin of any channel fills the height (a picture of one flat colour still
    // reads).
    let top = bins.iter().flat_map(|c| c.iter()).copied().max().unwrap_or(0).max(1) as f32;
    let height = |n: u32| (n as f32 / top).sqrt() * inner.height();
    let x_of = |i: usize| inner.left() + inner.width() * (i as f32 + 0.5) / 256.0;
    // Brightness as a filled shape, the channels as lines over it.
    let bar = inner.width() / 256.0;
    for (i, &n) in bins[3].iter().enumerate() {
        if n > 0 {
            let x = x_of(i);
            let r = Rect::from_min_max(
                Pos2::new(x - bar * 0.5, inner.bottom() - height(n)),
                Pos2::new(x + bar * 0.5 + 0.5, inner.bottom()),
            );
            painter.rect_filled(r, 0.0, style::CHANNEL_LUMA);
        }
    }
    for (c, ink) in [(0, style::CHANNEL_RED), (1, style::CHANNEL_GREEN), (2, style::CHANNEL_BLUE)] {
        let line: Vec<Pos2> =
            bins[c].iter().enumerate().map(|(i, &n)| Pos2::new(x_of(i), inner.bottom() - height(n))).collect();
        painter.add(egui::Shape::line(line, Stroke::new(1.0, ink)));
    }
    response
}

//! Level and loudness meters for the Mixer: two peak bars, the loudness readouts and the loudness
//! graph with its -14 LUFS mark.

use super::fonts::mono;
use super::tokens::*;
use bpe_sound::MeterReading;
use eframe::egui::{self, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};

/// The bottom of the peak bars, in dB.
const FLOOR_DB: f32 = -60.0;
/// The bottom and top of the loudness graph, in LUFS.
const GRAPH_LOW: f32 = -40.0;
const GRAPH_HIGH: f32 = 0.0;

/// How high up a bar (0 at the floor, 1 at 0 dB) a level sits.
fn bar_fill(db: f32) -> f32 {
    if !db.is_finite() {
        return 0.0;
    }
    ((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0)
}

/// A loudness or level readout: one decimal, or a dash for silence.
pub(crate) fn readout(v: f32) -> String {
    if v.is_finite() && v > -200.0 { format!("{v:.1}") } else { "\u{2013}".to_owned() }
}

/// Two peak bars (left, right) `height` tall, from -60 dB up to 0 dB.
pub fn level_meter(ui: &mut Ui, peak_db_l: f32, peak_db_r: f32, height: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(14.0, height), Sense::hover());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        for (i, db) in [peak_db_l, peak_db_r].into_iter().enumerate() {
            let x = rect.left() + i as f32 * 8.0;
            let bar = Rect::from_min_max(Pos2::new(x, rect.top()), Pos2::new(x + 6.0, rect.bottom()));
            painter.rect_filled(bar, 1.0, GROUND);
            let fill = bar_fill(db);
            if fill > 0.0 {
                let top = bar.bottom() - bar.height() * fill;
                let ink = if db >= -0.05 { TEXT_STRONG } else { MUTED };
                painter.rect_filled(Rect::from_min_max(Pos2::new(bar.left(), top), bar.max), 1.0, ink);
            }
        }
        // A tick at -6 dB.
        let y = rect.bottom() - rect.height() * bar_fill(-6.0);
        painter.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(1.0, LINE));
    }
    let (l, r) = (readout(peak_db_l), readout(peak_db_r));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::Meter, true, format!("Level {l} {r}")));
    response
}

/// The loudness readouts in a row: Momentary, Short-term, Whole clip and True peak (on two
/// rows when narrow).
pub fn loudness_row(ui: &mut Ui, reading: &MeterReading) {
    ui.with_layout(egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true), |ui| {
        for (name, value, unit) in [
            ("Momentary", reading.momentary, "LUFS"),
            ("Short-term", reading.short_term, "LUFS"),
            ("Whole clip", reading.integrated, "LUFS"),
            ("True peak", reading.true_peak, "dBTP"),
        ] {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(name).small().color(MUTED));
                ui.label(egui::RichText::new(format!("{} {unit}", readout(value))).font(mono(12.0)).color(TEXT));
            });
            ui.add_space(6.0);
        }
    });
}

/// A graph of loudness over time (oldest first, LUFS), with a line at `mark` (-14 for the apps).
pub fn loudness_graph(ui: &mut Ui, history: &[f32], mark: f32) -> egui::Response {
    let width = ui.available_width().max(60.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 64.0), Sense::hover());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_filled(rect, RADIUS, GROUND);
        let y_of = |lufs: f32| {
            let t = ((lufs - GRAPH_LOW) / (GRAPH_HIGH - GRAPH_LOW)).clamp(0.0, 1.0);
            rect.bottom() - rect.height() * t
        };
        let y = y_of(mark);
        painter.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(1.0, FAINT));
        painter.text(
            Pos2::new(rect.right() - 3.0, y - 2.0),
            egui::Align2::RIGHT_BOTTOM,
            format!("{mark:.0}"),
            mono(10.0),
            FAINT,
        );
        if history.len() >= 2 {
            let step = rect.width() / (history.len() - 1) as f32;
            let points: Vec<Pos2> = history
                .iter()
                .enumerate()
                .map(|(i, &v)| {
                    Pos2::new(rect.left() + i as f32 * step, if v.is_finite() { y_of(v) } else { rect.bottom() })
                })
                .collect();
            painter.add(Shape::line(points, Stroke::new(1.5, TEXT)));
        }
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::Image, true, "Loudness graph"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};

    #[test]
    fn silence_reads_as_a_dash_and_levels_fill_from_the_floor() {
        assert_eq!(readout(f32::NEG_INFINITY), "\u{2013}");
        assert_eq!(readout(-14.04), "-14.0");
        assert_eq!(bar_fill(f32::NEG_INFINITY), 0.0);
        assert_eq!(bar_fill(0.0), 1.0);
        assert_eq!(bar_fill(-30.0), 0.5);
    }

    #[test]
    fn the_loudness_row_names_each_reading() {
        let reading = MeterReading { momentary: -14.0, integrated: -15.5, ..MeterReading::SILENT };
        let h = Harness::builder().with_size(egui::vec2(420.0, 200.0)).allow_missing_glyphs().build_ui(move |ui| {
            super::super::apply(ui.ctx());
            loudness_row(ui, &reading);
            loudness_graph(ui, &[-20.0, -14.0, -12.0], -14.0);
        });
        for name in ["Momentary", "Short-term", "Whole clip", "True peak", "-14.0 LUFS", "-15.5 LUFS"] {
            assert!(h.query_by_label(name).is_some(), "{name}");
        }
        assert!(h.query_by_label("Loudness graph").is_some());
    }
}

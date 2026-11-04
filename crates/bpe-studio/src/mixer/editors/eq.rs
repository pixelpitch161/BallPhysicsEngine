//! The EQ editor: the curve over a log frequency grid with the analyser behind it and a point
//! per band that is on. Drag a point for frequency and gain, roll the wheel over it for Q,
//! double-click the curve to turn on a band there, right-click a point to turn it off or reset
//! it. The band chips under the curve pick a band for its own settings.

use super::super::Mixer;
use super::super::cx::MixCx;
use super::{panel, params};
use crate::sound_tab::name_it;
use crate::style;
use bpe_core::{Choice, LayerId, Settings, Value};
use bpe_dsp::fx::analyser::Analyser;
use bpe_dsp::fx::eq::band_response_db;
use bpe_kit::sound::fx::{AnalyserMode, BAND_FREQS, BandKind, EqBand, EqSettings, Slope, StereoPlace};
use bpe_sound::fx_chain::eq_bands;
use bpe_sound::{FX_BINS, Meters};
use eframe::egui::{self, Align2, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, Vec2};

/// The curve's height.
pub const CURVE_H: f32 = 300.0;
const LOW_HZ: f32 = 20.0;
const HIGH_HZ: f32 = 20_000.0;
/// The curve shows ±18 dB.
const RANGE_DB: f32 = 18.0;
/// The analyser shows -90..0 dB.
const FLOOR_DB: f32 = -90.0;
/// How near the pointer must be to grab a point.
const GRAB: f32 = 10.0;

/// Where frequency `f` and gain `db` sit in `rect`.
pub struct Plot {
    pub rect: Rect,
}

impl Plot {
    pub fn x(&self, f: f32) -> f32 {
        let t = (f.max(LOW_HZ) / LOW_HZ).ln() / (HIGH_HZ / LOW_HZ).ln();
        self.rect.left() + self.rect.width() * t.clamp(0.0, 1.0)
    }
    pub fn y(&self, db: f32) -> f32 {
        self.rect.center().y - (db / RANGE_DB).clamp(-1.0, 1.0) * self.rect.height() / 2.0
    }
    pub fn freq(&self, x: f32) -> f32 {
        let t = ((x - self.rect.left()) / self.rect.width()).clamp(0.0, 1.0);
        LOW_HZ * (HIGH_HZ / LOW_HZ).powf(t)
    }
    pub fn gain(&self, y: f32) -> f32 {
        ((self.rect.center().y - y) / (self.rect.height() / 2.0) * RANGE_DB).clamp(-RANGE_DB, RANGE_DB)
    }
    pub fn at(&self, f: f32, db: f32) -> Pos2 {
        Pos2::new(self.x(f), self.y(db))
    }
}

/// Whether a band of `kind` has a gain (cuts and the notch don't).
pub fn has_gain(kind: BandKind) -> bool {
    matches!(kind, BandKind::LowShelf | BandKind::Bell | BandKind::HighShelf)
}

fn key(band: usize, k: &str) -> String {
    EqSettings::band_key(band + 1, k)
}

/// Where band `b`'s point sits.
fn point(plot: &Plot, b: &EqBand) -> Pos2 {
    plot.at(b.freq, if has_gain(b.kind) { b.gain } else { 0.0 })
}

/// The EQ editor for layer `id`.
pub fn show(ui: &mut Ui, cx: &mut MixCx, mixer: &mut Mixer, meters: Option<&Meters>, id: LayerId) {
    let defs = EqSettings::defs();
    let set = EqSettings::from_params(&params(cx, defs));
    let bands = set.bands();
    panel(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Analyser").color(style::MUTED));
            cx.chips(
                ui,
                "analyser",
                &[(AnalyserMode::Post, "Post"), (AnalyserMode::Pre, "Pre"), (AnalyserMode::Off, "Off")],
            );
            ui.add_space(12.0);
            cx.knob_auto(ui, "output", "Output");
            cx.knob_auto(ui, "mix", "Mix");
        });
        curve(ui, cx, mixer, meters, id, &set);
        // The band chips: lit when picked, a dot under the ones that are on.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (i, b) in bands.iter().enumerate() {
                let text = if b.on { format!("{}", i + 1) } else { format!("({})", i + 1) };
                let chip = ui.add(style::toggle_button(&text, mixer.eq_band == i).min_size(egui::vec2(34.0, 22.0)));
                name_it(ui, &chip, &format!("Band {}", i + 1));
                let chip = chip.on_hover_text(if b.on { "On" } else { "Off" });
                if chip.clicked() {
                    mixer.eq_band = i;
                }
            }
        });
        let i = mixer.eq_band.min(7);
        let b = bands[i];
        ui.horizontal(|ui| {
            let r = cx.switch(ui, &key(i, "on"), &format!("Band {} on", i + 1));
            let _ = r;
            ui.label(RichText::new(format!("Band {}", i + 1)).font(style::semibold(13.0)));
            ui.add_space(8.0);
            let kinds: Vec<(BandKind, &str)> = BandKind::ALL.iter().map(|k| (*k, k.label())).collect();
            cx.chips(ui, &key(i, "kind"), &kinds);
        });
        ui.horizontal(|ui| {
            if matches!(b.kind, BandKind::LowCut | BandKind::HighCut) {
                ui.label(RichText::new("Slope").color(style::MUTED));
                let slopes: Vec<(Slope, &str)> = Slope::ALL.iter().map(|s| (*s, s.label())).collect();
                cx.chips(ui, &key(i, "slope"), &slopes);
                ui.add_space(8.0);
            }
            ui.label(RichText::new("Stereo").color(style::MUTED));
            let places: Vec<(StereoPlace, &str)> = StereoPlace::ALL.iter().map(|p| (*p, p.label())).collect();
            cx.chips(ui, &key(i, "place"), &places);
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            cx.knob_auto(ui, &key(i, "freq"), "Frequency");
            if has_gain(b.kind) {
                cx.knob_auto(ui, &key(i, "gain"), "Gain");
            }
            cx.knob(ui, &key(i, "q"), "Q", &|v| format!("{v:.2}"));
        });
    });
}

/// The curve with its grid, analyser and points, and what dragging and clicking it does.
fn curve(ui: &mut Ui, cx: &mut MixCx, mixer: &mut Mixer, meters: Option<&Meters>, id: LayerId, set: &EqSettings) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, CURVE_H), Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, "EQ curve"));
    let plot = Plot { rect };
    let bands = set.bands();
    let pointer = response.interact_pointer_pos().or(response.hover_pos());
    let nearest = |p: Pos2| -> Option<usize> {
        (0..8)
            .filter(|&i| bands[i].on)
            .map(|i| (i, point(&plot, bands[i]).distance(p)))
            .filter(|(_, d)| *d <= GRAB)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    };
    let hovered = pointer.and_then(nearest);

    // Dragging a point.
    if response.drag_started() {
        // Where the press began: the drag starts only once the pointer has moved a little.
        let origin = ui.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos());
        mixer.eq_drag = origin.and_then(nearest);
        if let Some(i) = mixer.eq_drag {
            mixer.eq_band = i;
        }
    }
    if response.dragged()
        && let (Some(i), Some(p)) = (mixer.eq_drag, response.interact_pointer_pos())
    {
        let f = (plot.freq(p.x) * 10.0).round() / 10.0;
        cx.set(&key(i, "freq"), Value::Number(f as f64));
        if has_gain(bands[i].kind) {
            let g = (plot.gain(p.y) * 10.0).round() / 10.0;
            cx.set(&key(i, "gain"), Value::Number(g as f64));
        }
    }
    if response.drag_stopped() {
        mixer.eq_drag = None;
    }
    // A click picks a band; the wheel over a point changes its Q.
    if response.clicked()
        && let Some(i) = pointer.and_then(nearest)
    {
        mixer.eq_band = i;
    }
    if let Some(i) = hovered {
        let notches: f32 = ui.input(|inp| {
            inp.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::MouseWheel { delta, .. } if delta.y != 0.0 => Some(delta.y.signum()),
                    _ => None,
                })
                .sum()
        });
        if notches != 0.0 {
            let q = (bands[i].q * 1.15f32.powf(notches)).clamp(0.1, 18.0);
            cx.set(&key(i, "q"), Value::Number(((q * 100.0).round() / 100.0) as f64));
            ui.input_mut(|inp| inp.smooth_scroll_delta = Vec2::ZERO);
        }
    }
    // A double-click on the curve away from the points turns on the off band nearest there.
    if response.double_clicked()
        && let Some(p) = pointer
        && nearest(p).is_none()
    {
        let f = plot.freq(p.x);
        let off = (0..8).filter(|&i| !bands[i].on).min_by(|&a, &b| {
            let d = |i: usize| (bands[i].freq / f).ln().abs();
            d(a).total_cmp(&d(b))
        });
        if let Some(i) = off {
            cx.set(&key(i, "on"), Value::Bool(true));
            cx.set(&key(i, "freq"), Value::Number(((f * 10.0).round() / 10.0) as f64));
            if has_gain(bands[i].kind) {
                cx.set(&key(i, "gain"), Value::Number(((plot.gain(p.y) * 10.0).round() / 10.0) as f64));
            }
            mixer.eq_band = i;
        }
    }
    // Right-click a point: turn it off, or put it back as it started.
    if response.secondary_clicked() {
        mixer.eq_menu = pointer.and_then(nearest);
    }
    if let Some(i) = mixer.eq_menu {
        let mut keep = false;
        response.context_menu(|ui| {
            keep = true;
            ui.label(RichText::new(format!("Band {}", i + 1)).color(style::MUTED));
            if ui.button("Turn off").clicked() {
                cx.set(&key(i, "on"), Value::Bool(false));
                ui.close();
            }
            if ui.button("Delete").on_hover_text("Turns it off and puts it back as it started").clicked() {
                let start = EqBand::bell(BAND_FREQS[i]);
                cx.set(&key(i, "on"), Value::Bool(false));
                cx.set(&key(i, "kind"), Value::Choice(start.kind.name().into()));
                cx.set(&key(i, "freq"), Value::Number(start.freq as f64));
                cx.set(&key(i, "gain"), Value::Number(0.0));
                cx.set(&key(i, "q"), Value::Number(start.q as f64));
                ui.close();
            }
        });
        if !keep {
            mixer.eq_menu = None;
        }
    }

    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, style::RADIUS, style::GROUND);
    // The grid.
    for f in [50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0] {
        let x = plot.x(f);
        painter
            .line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(1.0, style::LINE_SOFT));
        let text = if f >= 1000.0 { format!("{}k", f / 1000.0) } else { format!("{f:.0}") };
        painter.text(
            Pos2::new(x + 3.0, rect.bottom() - 3.0),
            Align2::LEFT_BOTTOM,
            text,
            style::mono(10.0),
            style::FAINT,
        );
    }
    for db in [-12.0, -6.0, 0.0, 6.0, 12.0] {
        let y = plot.y(db);
        let ink = if db == 0.0 { style::LINE } else { style::LINE_SOFT };
        painter.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(1.0, ink));
        painter.text(
            Pos2::new(rect.left() + 3.0, y - 1.0),
            Align2::LEFT_BOTTOM,
            format!("{db:+.0}"),
            style::mono(10.0),
            style::FAINT,
        );
    }
    // The analyser behind.
    let mut bins = [0.0f32; FX_BINS];
    if set.analyser != AnalyserMode::Off && meters.is_some_and(|m| m.read_fx_bins(id.0, &mut bins)) {
        let mut points = vec![Pos2::new(rect.left(), rect.bottom())];
        for (i, db) in bins.iter().enumerate() {
            let t = ((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0);
            points.push(Pos2::new(plot.x(Analyser::bin_hz(i)), rect.bottom() - t * rect.height()));
        }
        points.push(Pos2::new(rect.right(), rect.bottom()));
        painter.add(Shape::line(points[1..points.len() - 1].to_vec(), Stroke::new(1.0, style::FAINT)));
    }
    // The curve: every band that is on (Side-only bands leave the middle as it is).
    let params = eq_bands(set);
    let steps = (rect.width() as usize).max(2);
    let line: Vec<Pos2> = (0..=steps)
        .map(|s| {
            let x = rect.left() + rect.width() * s as f32 / steps as f32;
            let f = plot.freq(x);
            let db: f32 = set.output
                + params
                    .iter()
                    .filter(|b| b.place != bpe_dsp::fx::eq::Place::Side)
                    .map(|b| band_response_db(b, f))
                    .sum::<f32>();
            Pos2::new(x, plot.y(db))
        })
        .collect();
    painter.add(Shape::line(line, Stroke::new(2.0, style::TEXT_STRONG)));
    // The points.
    for (i, b) in bands.iter().enumerate().filter(|(_, b)| b.on) {
        let c = point(&plot, b);
        let hot = hovered == Some(i) || mixer.eq_drag == Some(i) || mixer.eq_band == i;
        painter.circle(c, 8.0, if hot { style::TEXT } else { style::RAISED }, Stroke::new(1.0, style::TEXT));
        painter.text(
            c,
            Align2::CENTER_CENTER,
            format!("{}", i + 1),
            style::mono(10.0),
            if hot { style::ON_LIGHT } else { style::TEXT },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plot_maps_both_ways() {
        let plot = Plot { rect: Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 300.0)) };
        assert!((plot.freq(plot.x(1000.0)) - 1000.0).abs() < 0.01);
        assert!((plot.gain(plot.y(6.0)) - 6.0).abs() < 1e-4);
        assert_eq!(plot.y(0.0), 150.0);
    }
}

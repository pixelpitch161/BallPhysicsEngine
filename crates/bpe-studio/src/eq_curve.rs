//! An 8-band EQ's curve, shared by the Mixer's EQ editor and an oscillator's EQ layer: where a
//! frequency and a gain sit, a point per band that is on, and what the pointer does on it. Drag
//! a point for frequency and gain, roll the wheel over it for Q, double-click the curve to turn
//! on the off band nearest there, right-click a point to turn it off or put it back.

use crate::style;
use bpe_core::{Choice, Value};
use bpe_dsp::fx::eq::{BandParams, Place, band_response_db};
use bpe_kit::sound::fx::{BandKind, EqBand, EqSettings};
use eframe::egui::{self, Align2, Painter, Pos2, Rect, RichText, Shape, Stroke, Ui, Vec2};

const LOW_HZ: f32 = 20.0;
const HIGH_HZ: f32 = 20_000.0;
/// The curve shows ±18 dB.
const RANGE_DB: f32 = 18.0;
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

/// Where band `b`'s point sits.
pub fn point(plot: &Plot, b: &EqBand) -> Pos2 {
    plot.at(b.freq, if has_gain(b.kind) { b.gain } else { 0.0 })
}

/// The band picked for its own settings, the one being dragged and the one whose menu is open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pick {
    pub band: usize,
    pub drag: Option<usize>,
    pub menu: Option<usize>,
}

/// What the pointer does on the curve in `response`. `set(band, key, value)` writes band
/// `band`'s (0..8) setting `key` (`freq`, `gain`, ...). Gives the band whose point is hovered.
pub fn interact(
    ui: &mut Ui,
    response: &egui::Response,
    plot: &Plot,
    bands: [&EqBand; 8],
    pick: &mut Pick,
    set: &mut dyn FnMut(usize, &str, Value),
) -> Option<usize> {
    let pointer = response.interact_pointer_pos().or(response.hover_pos());
    let nearest = |p: Pos2| -> Option<usize> {
        (0..8)
            .filter(|&i| bands[i].on)
            .map(|i| (i, point(plot, bands[i]).distance(p)))
            .filter(|(_, d)| *d <= GRAB)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    };
    let hovered = pointer.and_then(nearest);

    // Dragging a point.
    let grab_id = response.id.with("eq grab");
    if response.drag_started() {
        // Where the press began: the drag starts only once the pointer has moved a little.
        let origin = ui.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos());
        pick.drag = origin.and_then(nearest);
        if let (Some(i), Some(o)) = (pick.drag, origin) {
            pick.band = i;
            // The point moves by how far the pointer goes, wherever on it the press was.
            let offset = point(plot, bands[i]) - o;
            ui.data_mut(|d| d.insert_temp(grab_id, offset));
        }
    }
    if response.dragged()
        && let (Some(i), Some(p)) = (pick.drag, response.interact_pointer_pos())
    {
        let offset: Vec2 = ui.data(|d| d.get_temp(grab_id)).unwrap_or(Vec2::ZERO);
        let p = p + offset;
        // Only what changed is written (a sideways drag leaves the gain, and its keys, alone).
        let f = (plot.freq(p.x) * 10.0).round() / 10.0;
        if (f - bands[i].freq).abs() > 0.05 {
            set(i, "freq", Value::Number(f as f64));
        }
        if has_gain(bands[i].kind) {
            let g = (plot.gain(p.y) * 10.0).round() / 10.0;
            if (g - bands[i].gain).abs() > 0.05 {
                set(i, "gain", Value::Number(g as f64));
            }
        }
    }
    if response.drag_stopped() {
        pick.drag = None;
    }
    // A click picks a band; the wheel over a point changes its Q.
    if response.clicked()
        && let Some(i) = hovered
    {
        pick.band = i;
    }
    if let Some(i) = hovered {
        let notches = style::wheel_notches(ui, response.id.with(i), false) as f32;
        if notches != 0.0 {
            let q = (bands[i].q * 1.15f32.powf(notches)).clamp(0.1, 18.0);
            set(i, "q", Value::Number(((q * 100.0).round() / 100.0) as f64));
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
            set(i, "on", Value::Bool(true));
            set(i, "freq", Value::Number(((f * 10.0).round() / 10.0) as f64));
            if has_gain(bands[i].kind) {
                set(i, "gain", Value::Number(((plot.gain(p.y) * 10.0).round() / 10.0) as f64));
            }
            pick.band = i;
        }
    }
    // Right-click a point: turn it off, or put it back as it started.
    if response.secondary_clicked() {
        pick.menu = hovered;
    }
    if let Some(i) = pick.menu {
        let mut keep = false;
        response.context_menu(|ui| {
            keep = true;
            ui.label(RichText::new(format!("Band {}", i + 1)).color(style::MUTED));
            if ui.button("Turn off").clicked() {
                set(i, "on", Value::Bool(false));
                ui.close();
            }
            if ui.button("Delete").on_hover_text("Turns it off and puts it back as it started").clicked() {
                let fresh = EqSettings::default();
                let start = fresh.bands()[i];
                set(i, "on", Value::Bool(false));
                set(i, "kind", Value::Choice(start.kind.name().into()));
                set(i, "freq", Value::Number(start.freq as f64));
                set(i, "gain", Value::Number(start.gain as f64));
                set(i, "q", Value::Number(start.q as f64));
                set(i, "slope", Value::Choice(start.slope.name().into()));
                set(i, "place", Value::Choice(start.place.name().into()));
                ui.close();
            }
        });
        if !keep {
            pick.menu = None;
        }
    }
    hovered
}

/// The grid: frequency lines and gain lines, with their numbers when `labels` (a small view
/// keeps only the 0 dB line and the decades).
pub fn paint_grid(painter: &Painter, plot: &Plot, labels: bool) {
    let rect = plot.rect;
    let freqs: &[f32] = if labels {
        &[50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0]
    } else {
        &[100.0, 1000.0, 10_000.0]
    };
    for &f in freqs {
        let x = plot.x(f);
        painter
            .line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(1.0, style::LINE_SOFT));
        if labels {
            let text = if f >= 1000.0 { format!("{}k", f / 1000.0) } else { format!("{f:.0}") };
            painter.text(
                Pos2::new(x + 3.0, rect.bottom() - 3.0),
                Align2::LEFT_BOTTOM,
                text,
                style::mono(10.0),
                style::FAINT,
            );
        }
    }
    let gains: &[f32] = if labels { &[-12.0, -6.0, 0.0, 6.0, 12.0] } else { &[0.0] };
    for &db in gains {
        let y = plot.y(db);
        let ink = if db == 0.0 { style::LINE } else { style::LINE_SOFT };
        painter.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(1.0, ink));
        if labels {
            painter.text(
                Pos2::new(rect.left() + 3.0, y - 1.0),
                Align2::LEFT_BOTTOM,
                format!("{db:+.0}"),
                style::mono(10.0),
                style::FAINT,
            );
        }
    }
}

/// The curve's gain in dB at `f`: every band that is on (Side-only bands leave the middle as
/// it is) and the output.
pub fn curve_db(bands: &[BandParams; 8], output: f32, f: f32) -> f32 {
    output + bands.iter().filter(|b| b.place != Place::Side).map(|b| band_response_db(b, f)).sum::<f32>()
}

/// The curve of `bands` and `output` across the plot.
pub fn paint_curve(painter: &Painter, plot: &Plot, bands: &[BandParams; 8], output: f32, stroke: Stroke) {
    let rect = plot.rect;
    let steps = (rect.width() as usize).max(2);
    let line: Vec<Pos2> = (0..=steps)
        .map(|s| {
            let x = rect.left() + rect.width() * s as f32 / steps as f32;
            Pos2::new(x, plot.y(curve_db(bands, output, plot.freq(x))))
        })
        .collect();
    painter.add(Shape::line(line, stroke));
}

/// A numbered point per band that is on, lit when hovered, dragged or picked.
pub fn paint_points(painter: &Painter, plot: &Plot, bands: [&EqBand; 8], pick: &Pick, hovered: Option<usize>, r: f32) {
    for (i, b) in bands.iter().enumerate().filter(|(_, b)| b.on) {
        let c = point(plot, b);
        let hot = hovered == Some(i) || pick.drag == Some(i) || pick.band == i;
        painter.circle(c, r, if hot { style::TEXT } else { style::RAISED }, Stroke::new(1.0, style::TEXT));
        painter.text(
            c,
            Align2::CENTER_CENTER,
            format!("{}", i + 1),
            style::mono(r + 2.0),
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

    #[test]
    fn the_curve_sums_the_bands_on_and_the_output() {
        let mut eq =
            EqSettings { band5: EqBand { on: true, gain: 6.0, ..EqBand::bell(1000.0) }, ..EqSettings::default() };
        let bands = bpe_sound::fx_chain::eq_bands(&eq);
        assert!((curve_db(&bands, 0.0, 1000.0) - 6.0).abs() < 0.05);
        assert!((curve_db(&bands, -3.0, 1000.0) - 3.0).abs() < 0.05);
        assert!(curve_db(&bands, 0.0, 20.0) < -6.0, "the 30 Hz cut");
        eq.band5.place = bpe_kit::sound::fx::StereoPlace::Side;
        assert!(curve_db(&bpe_sound::fx_chain::eq_bands(&eq), 0.0, 1000.0).abs() < 0.05, "Side leaves the middle");
    }
}

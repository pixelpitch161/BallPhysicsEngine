//! The drawings on a layer strip: a filter's real response, a distortion's transfer curve, a
//! modulated wave and an EQ's curve, small in the folded strip and 64 px tall when it is open.
//! A filter's open view has a handle: sideways for Cutoff (even in octaves), up and down for
//! Reso; an EQ's has a point per band that is on ([`crate::eq_curve`]).

use crate::eq_curve::{self, Plot};
use crate::style;
use bpe_dsp::{FilterParams, response_db, transfer};
use bpe_kit::sound::{
    ChainEqSettings, DistKind, DistortionSettings, FilterKind, FilterSettings, ModKind, ModulationSettings,
};
use bpe_sound::chain::{filter_type, shaper_type};
use bpe_sound::fx_chain::bands_of;
use eframe::egui::{self, Painter, Pos2, Rect, Shape, Stroke};

/// The folded strip's view.
pub const MINI: egui::Vec2 = egui::vec2(76.0, 32.0);
/// The open strip's view height.
pub const HEIGHT: f32 = 64.0;
/// The frequencies a filter view spans.
pub const LOW_HZ: f64 = 20.0;
pub const HIGH_HZ: f64 = 20_000.0;
/// The levels a filter view spans, in dB.
const DB_SPAN: f32 = 30.0;

/// Where `hz` sits across a filter view, 0 at the left.
pub fn x_of_hz(hz: f64) -> f64 {
    ((hz.clamp(LOW_HZ, HIGH_HZ) / LOW_HZ).ln() / (HIGH_HZ / LOW_HZ).ln()).clamp(0.0, 1.0)
}

/// The frequency at `t` across a filter view.
pub fn hz_at(t: f64) -> f64 {
    LOW_HZ * (HIGH_HZ / LOW_HZ).powf(t.clamp(0.0, 1.0))
}

/// Middle C: the note a view draws key tracking at.
const MIDDLE_C_HZ: f32 = 261.63;

/// The filter as the engine runs it for middle C.
pub fn filter_params(f: &FilterSettings) -> FilterParams {
    let kt = (f.key_track / 100.0).clamp(0.0, 1.0);
    let cutoff = f.cutoff.max(1.0);
    let cutoff_hz = if f.kind == FilterKind::Comb { cutoff.powf(1.0 - kt) * MIDDLE_C_HZ.powf(kt) } else { cutoff };
    FilterParams {
        kind: filter_type(f.kind),
        slope24: f.slope == bpe_kit::sound::Slope::Db24,
        cutoff_hz,
        reso: (f.reso / 100.0).clamp(0.0, 1.0),
        drive: (f.drive / 100.0).clamp(0.0, 1.0),
        gain_db: f.gain,
        vowel: f.vowel,
        shift: f.shift,
    }
}

/// The filter's response in dB at `n` frequencies, evenly in octaves from 20 Hz to 20 kHz.
pub fn filter_curve(f: &FilterSettings, n: usize) -> Vec<f32> {
    let p = filter_params(f);
    (0..n).map(|i| response_db(&p, hz_at(i as f64 / (n - 1).max(1) as f64) as f32)).collect()
}

/// The distortion's curve at `n` inputs from -1 to 1 (before Tone and Output).
pub fn transfer_curve(d: &DistortionSettings, n: usize) -> Vec<f32> {
    let kind = shaper_type(d.kind);
    let drive = if d.kind == DistKind::Bitcrush { d.bits } else { d.drive };
    let bias = (d.bias / 100.0).clamp(-1.0, 1.0);
    (0..n)
        .map(|i| {
            let x = -1.0 + 2.0 * i as f32 / (n - 1).max(1) as f32;
            transfer(kind, drive, bias, x)
        })
        .collect()
}

/// Two cycles of a sine moved by a sine source an octave down, as the layer would move it.
pub fn modulated_wave(m: &ModulationSettings, n: usize) -> Vec<f32> {
    let a = (m.amount / 100.0).clamp(0.0, 1.0);
    let tau = std::f32::consts::TAU;
    (0..n)
        .map(|i| {
            let t = i as f32 / (n - 1).max(1) as f32;
            let src = (tau * t).sin();
            let carrier = 2.0 * tau * t;
            match m.kind {
                ModKind::Fm => (carrier + 3.0 * a * src).sin(),
                ModKind::PhaseDistortion => (carrier + 3.0 * a * src.max(0.0)).sin(),
                ModKind::Am => carrier.sin() * (1.0 + a * src) / (1.0 + a),
                ModKind::Ring => carrier.sin() * (1.0 - a + a * src),
            }
        })
        .collect()
}

/// The view's ground and frame.
pub fn ground(painter: &Painter, rect: Rect) {
    painter.rect_filled(rect, style::RADIUS, style::GROUND);
    painter.rect_stroke(rect, style::RADIUS, Stroke::new(1.0, style::LINE_SOFT), egui::StrokeKind::Inside);
}

/// `values` drawn left to right across `rect`, `lo` at the bottom and `hi` at the top, with a
/// faint line at `mid`.
pub fn line(painter: &Painter, rect: Rect, values: &[f32], lo: f32, hi: f32, mid: f32, ink: egui::Color32) {
    let inner = rect.shrink(3.0);
    let y_of = |v: f32| {
        let t = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
        inner.bottom() - t * inner.height()
    };
    painter.line_segment(
        [Pos2::new(inner.left(), y_of(mid)), Pos2::new(inner.right(), y_of(mid))],
        Stroke::new(1.0, style::LINE_SOFT),
    );
    let n = values.len().max(2);
    let points: Vec<Pos2> = values
        .iter()
        .enumerate()
        .map(|(i, v)| Pos2::new(inner.left() + inner.width() * i as f32 / (n - 1) as f32, y_of(*v)))
        .collect();
    painter.add(Shape::line(points, Stroke::new(1.4, ink)));
}

/// What a strip draws.
pub enum View<'a> {
    Modulation(&'a ModulationSettings),
    Filter(&'a FilterSettings),
    Distortion(&'a DistortionSettings),
    Eq(&'a ChainEqSettings),
}

/// Draws `view` into `rect`.
pub fn paint(painter: &Painter, rect: Rect, view: &View, ink: egui::Color32) {
    ground(painter, rect);
    let n = (rect.width() as usize).clamp(24, 240);
    match view {
        View::Filter(f) => line(painter, rect, &filter_curve(f, n), -DB_SPAN, DB_SPAN * 0.6, 0.0, ink),
        View::Distortion(d) => line(painter, rect, &transfer_curve(d, n), -1.1, 1.1, 0.0, ink),
        View::Modulation(m) => line(painter, rect, &modulated_wave(m, n), -1.1, 1.1, 0.0, ink),
        View::Eq(e) => {
            let plot = eq_plot(rect);
            eq_curve::paint_grid(painter, &plot, false);
            eq_curve::paint_curve(painter, &plot, &bands_of(e.bands()), e.output, Stroke::new(1.4, ink));
        }
    }
}

/// Where an EQ's curve sits in a view's `rect`.
pub fn eq_plot(rect: Rect) -> Plot {
    Plot { rect: rect.shrink(3.0) }
}

/// Where a filter view's handle sits in `rect`: across at Cutoff (Vowel for a formant), up with
/// Reso.
pub fn handle_at(rect: Rect, f: &FilterSettings) -> Pos2 {
    let inner = rect.shrink(6.0);
    let x = if f.kind == FilterKind::Formant { f.vowel as f64 / 4.0 } else { x_of_hz(f.cutoff as f64) };
    let y = (f.reso / 100.0).clamp(0.0, 1.0);
    Pos2::new(inner.left() + inner.width() * x as f32, inner.bottom() - inner.height() * y)
}

/// The handle's place in `rect` read back: across (0..1) and Reso (0..100).
pub fn handle_values(rect: Rect, at: Pos2) -> (f64, f64) {
    let inner = rect.shrink(6.0);
    let x = ((at.x - inner.left()) / inner.width()).clamp(0.0, 1.0) as f64;
    let y = ((inner.bottom() - at.y) / inner.height()).clamp(0.0, 1.0) as f64;
    (x, (y * 100.0).round())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_filter_view_spans_20_hz_to_20_khz_in_octaves() {
        assert!((hz_at(0.0) - 20.0).abs() < 1e-9);
        assert!((hz_at(1.0) - 20_000.0).abs() < 1e-6);
        assert!((x_of_hz(hz_at(0.37)) - 0.37).abs() < 1e-9);
        let f = FilterSettings::default();
        let curve = filter_curve(&f, 100);
        assert!(curve[0].abs() < 1.0, "a low pass passes the lows: {}", curve[0]);
        assert!(curve[99] < -20.0, "and cuts the highs: {}", curve[99]);
    }

    #[test]
    fn the_handle_reads_back_where_it_was_put() {
        let rect = Rect::from_min_size(Pos2::new(10.0, 20.0), egui::vec2(300.0, 64.0));
        let f = FilterSettings { cutoff: 1000.0, reso: 40.0, ..FilterSettings::default() };
        let (x, reso) = handle_values(rect, handle_at(rect, &f));
        assert!((hz_at(x) - 1000.0).abs() < 1.0);
        assert_eq!(reso, 40.0);
    }

    #[test]
    fn the_eq_view_reads_its_bands() {
        let rect = Rect::from_min_size(Pos2::new(0.0, 0.0), egui::vec2(300.0, 64.0));
        let mut e = ChainEqSettings::default();
        e.band5.on = true;
        e.band5.gain = 12.0;
        let plot = eq_plot(rect);
        let at = eq_curve::point(&plot, &e.band5);
        assert!(at.y < rect.center().y, "a lift sits above the middle");
        assert!((plot.freq(at.x) - 1000.0).abs() < 1.0);
        let db = eq_curve::curve_db(&bands_of(e.bands()), e.output, 1000.0);
        assert!((db - 12.0).abs() < 0.1, "{db}");
    }

    #[test]
    fn views_follow_their_settings() {
        let soft = DistortionSettings::default();
        let hard = DistortionSettings { drive: 36.0, ..DistortionSettings::default() };
        assert_ne!(transfer_curve(&soft, 32), transfer_curve(&hard, 32));
        let none = ModulationSettings { amount: 0.0, ..ModulationSettings::default() };
        let lots = ModulationSettings { amount: 90.0, ..ModulationSettings::default() };
        assert_ne!(modulated_wave(&none, 32), modulated_wave(&lots, 32));
    }
}

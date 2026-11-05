//! The EQ editor: the curve over a log frequency grid with the analyser behind it and a point
//! per band that is on (what the pointer does on it is [`crate::eq_curve`]'s, shared with an
//! oscillator's EQ layer). The band chips under the curve pick a band for its own settings.

use super::super::Mixer;
use super::super::cx::MixCx;
use super::{panel, params};
use crate::eq_curve::{self, Pick};
pub use crate::eq_curve::{Plot, has_gain};
use crate::sound_tab::name_it;
use crate::style;
use bpe_core::{LayerId, Settings};
use bpe_dsp::fx::analyser::Analyser;
use bpe_kit::sound::fx::{AnalyserMode, BandKind, EqSettings, Slope, StereoPlace};
use bpe_sound::fx_chain::eq_bands;
use bpe_sound::{FX_BINS, Meters};
use eframe::egui::{self, Pos2, RichText, Sense, Shape, Stroke, Ui, Vec2};

/// The curve's height.
pub const CURVE_H: f32 = 300.0;
/// The analyser shows -90..0 dB.
const FLOOR_DB: f32 = -90.0;

fn key(band: usize, k: &str) -> String {
    EqSettings::band_key(band + 1, k)
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
    let mut pick = Pick { band: mixer.eq_band, drag: mixer.eq_drag, menu: mixer.eq_menu };
    let hovered = eq_curve::interact(ui, &response, &plot, bands, &mut pick, &mut |i, k, v| cx.set(&key(i, k), v));
    (mixer.eq_band, mixer.eq_drag, mixer.eq_menu) = (pick.band, pick.drag, pick.menu);

    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, style::RADIUS, style::GROUND);
    eq_curve::paint_grid(&painter, &plot, true);
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
    eq_curve::paint_curve(&painter, &plot, &eq_bands(set), set.output, Stroke::new(2.0, style::TEXT_STRONG));
    eq_curve::paint_points(&painter, &plot, bands, &pick, hovered, 8.0);
}

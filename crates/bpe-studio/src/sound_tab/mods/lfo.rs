//! An LFO's editor: its shape (with a drawing of it), Hz or Tempo, Rate, Phase, Fade in, Delay,
//! Smooth and when it starts.

use super::super::TabCx;
use crate::style;
use bpe_core::{Choice, LayerId, Value, layered_key};
use bpe_dsp::lfo::shape_at;
use bpe_kit::sound::{LfoShape, LfoStart, NoteFeel, NoteLength};
use eframe::egui::{self, RichText, Sense, Shape, Stroke, Ui, Vec2};

fn hz(v: f64) -> String {
    if v < 1.0 { format!("{v:.2} Hz") } else { format!("{v:.1} Hz") }
}

fn degrees(v: f64) -> String {
    format!("{v:.0}°")
}

fn seconds(v: f64) -> String {
    format!("{v:.2} s")
}

fn percent(v: f64) -> String {
    format!("{v:.0} %")
}

/// Draws LFO `layer`'s editor.
pub fn show(ui: &mut Ui, cx: &mut TabCx, layer: LayerId) {
    let key = |k: &str| layered_key(layer, k);
    let shape: LfoShape = cx.choice(&key("shape"));
    ui.horizontal(|ui| {
        let chips: Vec<(LfoShape, &str)> = LfoShape::ALL.iter().map(|s| (*s, s.label())).collect();
        cx.chips(ui, &key("shape"), &chips);
    });
    ui.horizontal_top(|ui| {
        preview(ui, cx, shape, cx.number(&key("phase")) / 360.0, layer);
        ui.add_space(8.0);
        ui.vertical(|ui| {
            let sync = cx.flag(&key("sync"));
            ui.horizontal(|ui| {
                for (on, name) in [(false, "Hz"), (true, "Tempo")] {
                    let chip = ui.add(style::toggle_button(name, sync == on).min_size(egui::vec2(0.0, 22.0)));
                    if chip.clicked() && sync != on {
                        cx.set(&key("sync"), Value::Bool(on));
                    }
                }
                if sync {
                    ui.add_space(6.0);
                    note_menu(ui, cx, &key("rate_note"));
                    cx.chips(
                        ui,
                        &key("rate_feel"),
                        &[
                            (NoteFeel::Straight, "Straight"),
                            (NoteFeel::Dotted, "Dotted"),
                            (NoteFeel::Triplet, "Triplet"),
                        ],
                    );
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if !sync {
                    cx.knob(ui, &key("rate_hz"), "Rate", &hz);
                }
                cx.knob(ui, &key("phase"), "Phase", &degrees);
                cx.knob(ui, &key("fade_in"), "Fade in", &seconds);
                cx.knob(ui, &key("delay"), "Delay", &seconds);
                cx.knob(ui, &key("smooth"), "Smooth", &percent);
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Start").small().color(style::MUTED));
                cx.chips(
                    ui,
                    &key("start"),
                    &[(LfoStart::Restart, "Restart on hit"), (LfoStart::Free, "Free"), (LfoStart::PerNote, "Per note")],
                );
            });
        });
    });
}

/// The note length menu of a synced LFO ("1/4", "2 bars").
fn note_menu(ui: &mut Ui, cx: &mut TabCx, key: &str) {
    let now: NoteLength = cx.choice(key);
    let r = ui.menu_button(now.label(), |ui| {
        for n in &NoteLength::ALL {
            if ui.add(style::toggle_button(n.label(), *n == now)).clicked() {
                cx.set(key, Value::Choice(n.name().into()));
                ui.close();
            }
        }
    });
    super::super::name_it(ui, &r.response, "Rate note");
}

/// Two cycles of the wave, from its phase.
fn preview(ui: &mut Ui, cx: &TabCx, shape: LfoShape, phase: f64, layer: LayerId) {
    let (rect, r) = ui.allocate_exact_size(Vec2::new(150.0, 56.0), Sense::hover());
    super::super::name_it(ui, &r, &format!("{} wave", shape.label()));
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    painter.rect_filled(rect, style::RADIUS, style::GROUND);
    let wave = bpe_sound::mods::lfo::wave_of(shape);
    let seed = layer.0 as u64 + 1;
    let inner = rect.shrink(4.0);
    let n = 120;
    let points: Vec<_> = (0..=n)
        .map(|i| {
            let x = i as f64 / n as f64;
            let v = shape_at(wave, 2.0 * x + phase, seed);
            egui::pos2(inner.left() + inner.width() * x as f32, inner.bottom() - inner.height() * v)
        })
        .collect();
    painter.add(Shape::line(points, Stroke::new(1.5, cx.colour)));
}

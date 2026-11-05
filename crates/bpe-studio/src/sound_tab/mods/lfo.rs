//! An LFO's editor, laid out like a synth's LFO page: the tools (Preset, grid, Snap, the paint
//! tools, Upside down, Reverse, Clear), the drawn shape across the whole width with a playhead,
//! then Hz or Tempo, when it starts, Once, and the Rate, Phase, Fade in, Delay and Smooth knobs.

use super::super::{SoundTab, TabCx, TabEdit};
use super::lfo_view::{self, Tool};
use crate::style;
use bpe_core::{Choice, CurvePoint, LayerId, Value, layered_key};
use bpe_kit::sound::lfo_curve;
use bpe_kit::sound::{LfoPreset, LfoShape, LfoStart, NoteFeel, NoteLength};
use bpe_sound::Meters;
use eframe::egui::{self, RichText, Ui};

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

/// LFO `layer`'s drawn shape (a valid one, whatever is stored).
pub fn curve(cx: &TabCx, layer: LayerId) -> Vec<CurvePoint> {
    match cx.get(&layered_key(layer, "curve")) {
        Value::Curve(c) if bpe_core::curve_is_valid(&c) => c,
        _ => LfoPreset::Sine.points(),
    }
}

/// Puts `points` in as the drawn shape (as its own undo step when `step`).
fn set_curve(cx: &mut TabCx, layer: LayerId, points: Vec<CurvePoint>, step: bool) {
    if step {
        cx.edits.push(TabEdit::OwnStep);
    }
    cx.set(&layered_key(layer, "curve"), Value::Curve(lfo_curve::tidy(&points)));
}

/// Draws LFO `layer`'s editor. `meters` (while playing) moves the playhead.
pub fn show(ui: &mut Ui, cx: &mut TabCx, tab: &mut SoundTab, layer: LayerId, meters: Option<&Meters>) {
    let key = |k: &str| layered_key(layer, k);
    let shape: LfoShape = cx.choice(&key("shape"));
    let drawn = shape == LfoShape::Drawn;
    let points = curve(cx, layer);
    let tool = tab.lfo_tool;

    // The tools (wrapping onto a second row when the panel is narrow).
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        preset_menu(ui, cx, layer, shape);
        ui.add_space(4.0);
        ui.label(RichText::new("Grid").small().color(style::MUTED));
        grid_number(ui, cx, &key("grid_x"), "Grid across", 1..=32);
        ui.label(RichText::new("×").color(style::MUTED));
        grid_number(ui, cx, &key("grid_y"), "Grid up", 1..=16);
        let snap = cx.flag(&key("snap"));
        let r = ui.add(style::toggle_button("Snap", snap).min_size(egui::vec2(0.0, 22.0)));
        if r.on_hover_text("Points snap to the grid (hold Shift to place freely)").clicked() {
            cx.set(&key("snap"), Value::Bool(!snap));
        }
        ui.add_space(4.0);
        ui.add_enabled_ui(drawn, |ui| {
            for t in Tool::ALL {
                let r = ui.add(style::toggle_button(t.label(), tool == t).min_size(egui::vec2(0.0, 22.0)));
                let hint = match t {
                    Tool::Points => "Drag, add and bend points",
                    Tool::Step => "Paint flat steps, a grid cell at a time",
                    Tool::RampUp => "Paint ramps rising to where you point",
                    Tool::RampDown => "Paint ramps falling from where you point",
                };
                if r.on_hover_text(hint).clicked() {
                    tab.lfo_tool = t;
                }
            }
            ui.add_space(4.0);
            if ui.button("Upside down").clicked() {
                set_curve(cx, layer, lfo_curve::flip_y(&points), true);
            }
            if ui.button("Reverse").clicked() {
                set_curve(cx, layer, lfo_curve::flip_x(&points), true);
            }
            if ui.button("Clear").on_hover_text("A flat line to draw from").clicked() {
                set_curve(cx, layer, lfo_curve::flat(), true);
            }
        });
    });

    // The shape.
    let playhead = meters.and_then(|m| m.mod_position(cx.id.0, layer));
    if playhead.is_some() {
        ui.ctx().request_repaint();
    }
    let view = lfo_view::View {
        points: &points,
        wave: bpe_sound::mods::lfo::wave_of(shape),
        seed: bpe_sound::mods::lfo::seed_of(cx.id, layer),
        grid: (cx.number(&key("grid_x")) as i32, cx.number(&key("grid_y")) as i32),
        snap: cx.flag(&key("snap")),
        tool: if drawn { tool } else { Tool::Points },
        playhead,
        colour: cx.colour,
    };
    let id = egui::Id::unique(("lfo view", cx.id, layer));
    let out = lfo_view::show(ui, id, &format!("{} shape", shape.label()), &view);
    if let Some(p) = out.points
        && p != points
    {
        set_curve(cx, layer, p, out.new_step);
    }
    if !drawn {
        ui.label(
            RichText::new("A new random value every cycle. Pick a drawn preset to draw your own shape.")
                .small()
                .color(style::MUTED),
        );
    }

    // Rate and when it starts.
    ui.add_space(2.0);
    let sync = cx.flag(&key("sync"));
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (on, name) in [(false, "Hz"), (true, "Tempo")] {
            let chip = ui.add(style::toggle_button(name, sync == on).min_size(egui::vec2(0.0, 22.0)));
            if chip.clicked() && sync != on {
                cx.set(&key("sync"), Value::Bool(on));
            }
        }
        if sync {
            note_menu(ui, cx, &key("rate_note"));
            cx.chips(
                ui,
                &key("rate_feel"),
                &[(NoteFeel::Straight, "Straight"), (NoteFeel::Dotted, "Dotted"), (NoteFeel::Triplet, "Triplet")],
            );
        }
        ui.add_space(10.0);
        cx.chips(
            ui,
            &key("start"),
            &[(LfoStart::Free, "Free"), (LfoStart::Restart, "Restart on hit"), (LfoStart::PerNote, "Per note")],
        );
        let once = cx.flag(&key("once"));
        let r = ui.add(style::toggle_button("Once", once).min_size(egui::vec2(0.0, 22.0)));
        if r.on_hover_text("Play the shape one time from its start and hold its end, like an envelope").clicked() {
            cx.set(&key("once"), Value::Bool(!once));
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
}

/// The Preset menu: the drawn presets, then the random shapes.
fn preset_menu(ui: &mut Ui, cx: &mut TabCx, layer: LayerId, shape: LfoShape) {
    let key = layered_key(layer, "shape");
    let r = ui.menu_button("Preset", |ui| {
        for p in LfoPreset::ALL {
            if ui.button(p.label()).clicked() {
                cx.edits.push(TabEdit::OwnStep);
                if shape != LfoShape::Drawn {
                    cx.set(&key, Value::Choice(LfoShape::Drawn.name().into()));
                }
                cx.set(&layered_key(layer, "curve"), Value::Curve(p.points()));
                ui.close();
            }
        }
        ui.separator();
        for s in [LfoShape::Random, LfoShape::SmoothRandom] {
            if ui.add(style::toggle_button(s.label(), shape == s)).clicked() {
                cx.edits.push(TabEdit::OwnStep);
                cx.set(&key, Value::Choice(s.name().into()));
                ui.close();
            }
        }
        if shape != LfoShape::Drawn {
            ui.separator();
            if ui.button("Back to the drawn shape").clicked() {
                cx.edits.push(TabEdit::OwnStep);
                cx.set(&key, Value::Choice(LfoShape::Drawn.name().into()));
                ui.close();
            }
        }
    });
    super::super::name_it(ui, &r.response, "LFO preset");
}

/// A whole number of grid cells, dragged or typed.
fn grid_number(ui: &mut Ui, cx: &mut TabCx, key: &str, name: &str, range: std::ops::RangeInclusive<i64>) {
    let mut n = cx.number(key).round() as i64;
    let r = ui.add(egui::DragValue::new(&mut n).range(range).speed(0.05));
    super::super::name_it(ui, &r, name);
    let r = r.on_hover_text(name);
    if r.changed() {
        cx.set(key, Value::Int(n));
    }
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

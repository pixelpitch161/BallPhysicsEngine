//! The knobs a modulator moves, under its editor: each route's knob ("Osc A · Cutoff"), its
//! range in the knob's own units ("1.8 kHz → 6.2 kHz"), its Amount (−100..+100 %), Inv (the
//! modulator upside down), ± (both ways from the knob) and × to remove it.

use super::super::{TabCx, db, name_it, number};
use crate::style::{self, Knob};
use bpe_core::{LayerId, SettingDef, SlotId, split_layered};
use bpe_sound::modmatrix::{Combine, combine_of, route_ends};
use eframe::egui::{self, RichText, Sense, Ui, Vec2};

/// Draws modulator `layer`'s routes ("<name> modulates").
pub fn show(ui: &mut Ui, cx: &mut TabCx, layer: LayerId, name: &str) {
    let routes = cx.routes();
    ui.add_space(4.0);
    ui.label(RichText::new(format!("{name} modulates")).small().color(style::MUTED));
    let followers = crate::follow::followers(cx.scene, &cx.kit.registry, cx.id, layer);
    if !routes.iter().any(|r| r.source == layer) && followers.is_empty() {
        ui.label(RichText::new("Nothing yet. Drag the handle on its tile onto a knob.").small().color(style::FAINT));
    }
    for (i, r) in routes.iter().enumerate().filter(|(_, r)| r.source == layer) {
        let Some(def) = cx.def(&r.target) else { continue };
        let base = number(&cx.get(&r.target)).unwrap_or(0.0);
        let range = def.range.unwrap_or((0.0, 1.0));
        let (at0, at1) = route_ends(r, base, range);
        let scale = combine_of(&r.target) == Combine::Scale;
        ui.horizontal(|ui| {
            let (dot, _) = ui.allocate_exact_size(Vec2::splat(6.0), Sense::hover());
            ui.painter().rect_filled(dot, 0.0, cx.colour);
            ui.label(RichText::new(target_words(cx, &r.target)).color(style::TEXT));
            ui.label(
                RichText::new(format!("{} → {}", real(at0, def), real(at1, def)))
                    .font(style::mono(11.0))
                    .color(style::MUTED),
            );
            ui.spacing_mut().item_spacing.x = 4.0;
            let mut amount = r.amount as f64 * 100.0;
            let k = Knob::new(&mut amount, -100.0..=100.0, 50.0, "Amount")
                .format(&|v| format!("{v:+.0} %"))
                .bipolar(true)
                .width(54.0)
                .show(ui);
            let mut new = r.clone();
            if k.changed {
                new.amount = (amount / 100.0) as f32;
            }
            let inv = ui.add(style::toggle_button("Inv", r.invert)).on_hover_text("Turn the modulator upside down");
            name_it(ui, &inv, "Inv");
            if inv.clicked() {
                new.invert = !r.invert;
            }
            if !scale {
                let both = ui.add(style::toggle_button("±", r.bipolar)).on_hover_text("Move both ways from the knob");
                name_it(ui, &both, "Both ways");
                if both.clicked() {
                    new.bipolar = !r.bipolar;
                }
            }
            let x = ui.add(egui::Button::new("×").frame(false)).on_hover_text("Stop modulating this knob");
            name_it(ui, &x, "Remove route");
            let mut all = routes.clone();
            if x.clicked() {
                all.remove(i);
                cx.set_routes(all);
                cx.edits.push(super::super::TabEdit::OwnStep);
            } else if new != *r {
                all[i] = new;
                cx.set_routes(all);
            }
        });
    }
    // Settings of things that follow it (Follow sound), by the thing's name.
    for (a, words) in followers {
        ui.horizontal(|ui| {
            let (dot, _) = ui.allocate_exact_size(Vec2::splat(6.0), Sense::hover());
            ui.painter().circle_stroke(dot.center(), 2.5, egui::Stroke::new(1.0, cx.colour));
            ui.label(RichText::new(&words).color(style::TEXT));
            ui.label(RichText::new("follows").small().color(style::MUTED));
            let x = ui.add(egui::Button::new("×").frame(false)).on_hover_text("Stop following");
            name_it(ui, &x, &format!("Stop {words} following"));
            if x.clicked() {
                cx.edits.push(super::super::TabEdit::Unfollow(a));
                cx.edits.push(super::super::TabEdit::OwnStep);
            }
        });
    }
}

/// A knob value in its own units: "1.8 kHz", "-6.0 dB", "120 ms", "40 %".
pub fn real(v: f64, def: &SettingDef) -> String {
    match def.unit {
        "dB" => db(v),
        "Hz" if v.abs() >= 1000.0 => format!("{:.1} kHz", v / 1000.0),
        "Hz" => format!("{v:.0} Hz"),
        "ms" if v.abs() >= 1000.0 => format!("{:.2} s", v / 1000.0),
        "" => format!("{v:.2}"),
        unit if v.fract().abs() < 1e-9 => format!("{v:.0} {unit}"),
        unit => format!("{v:.1} {unit}"),
    }
}

/// "Osc A · Gain" for `a_gain`, "Output · Volume" for `out_volume`, "Filter 1 · Cutoff" for a
/// layer's knob.
pub fn target_words(cx: &TabCx, key: &str) -> String {
    let label = cx.def(key).map_or(key, |d| d.label);
    if let Some((layer, _)) = split_layered(key) {
        let owner = cx
            .scene
            .card(cx.id, SlotId::Sound)
            .and_then(|c| c.layer(layer))
            .map_or_else(|| "Layer".to_owned(), |l| l.name.clone());
        return format!("{owner} · {label}");
    }
    let owner = if key.starts_with("a_") {
        "Osc A"
    } else if key.starts_with("b_") {
        "Osc B"
    } else if key.starts_with("out_") {
        "Output"
    } else if key.starts_with("play_") {
        "Playing"
    } else {
        "Sound"
    };
    format!("{owner} · {label}")
}

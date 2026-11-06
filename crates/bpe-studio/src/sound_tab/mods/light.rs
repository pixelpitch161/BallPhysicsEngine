//! A Light modulator's editor (lighting part 7): what it reads (the light on the balls or their
//! shadow), Dark and Bright, Curve, Smooth, Invert, which balls it follows and hears, and a live
//! Now meter. Without a Lighting card it says there is nothing to hear yet.

use super::super::TabCx;
use super::distance::now_meter;
use crate::style;
use bpe_core::{LayerId, Value, layered_key};
use bpe_kit::sound::{LightFollows, LightReads};
use bpe_sound::Meters;
use eframe::egui::{RichText, Ui};

fn level(v: f64) -> String {
    format!("{:.0} %", v * 100.0)
}

fn ms(v: f64) -> String {
    format!("{v:.0} ms")
}

fn signed(v: f64) -> String {
    format!("{v:+.0}")
}

fn balls(v: f64) -> String {
    if v.round() == 1.0 { "1 ball".into() } else { format!("{v:.0} balls") }
}

/// Draws Light `layer`'s editor.
pub fn show(ui: &mut Ui, cx: &mut TabCx, layer: LayerId, meters: Option<&Meters>) {
    let key = |k: &str| layered_key(layer, k);
    if !bpe_kit::sensing::has_lighting(cx.scene) {
        ui.label(
            RichText::new(
                "The scene has no lighting yet: add a light (Lights tab) and this hears how lit the balls are.",
            )
            .small()
            .color(style::FAINT),
        );
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new("Reads").small().color(style::MUTED));
        cx.chips(ui, &key("reads"), &[(LightReads::Light, "Light on the balls"), (LightReads::Shadow, "Their shadow")]);
        ui.add_space(8.0);
        cx.switch(ui, &key("invert"), "Invert");
        ui.label(RichText::new("Invert").small().color(style::MUTED));
    });
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        cx.knob(ui, &key("dark"), "Dark", &level).on_hover_text("This much light or less gives 0");
        cx.knob(ui, &key("bright"), "Bright", &level).on_hover_text("This much light or more gives 1");
        cx.knob(ui, &key("curve"), "Curve", &signed);
        cx.knob(ui, &key("smooth"), "Smooth", &ms);
        if cx.choice::<LightFollows>(&key("follows")) == LightFollows::HowManyLit {
            cx.knob(ui, &key("lit_at"), "Lit at", &level).on_hover_text("A ball with this much light counts as lit");
            cx.knob(ui, &key("full_at"), "Full at", &balls).on_hover_text("This many lit balls gives 1");
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Follows").small().color(style::MUTED));
        cx.chips(
            ui,
            &key("follows"),
            &[
                (LightFollows::Average, "Average"),
                (LightFollows::BrightestBall, "Brightest ball"),
                (LightFollows::DarkestBall, "Darkest ball"),
                (LightFollows::HowManyLit, "How many lit"),
                (LightFollows::EachNotesBall, "Each note's ball"),
            ],
        );
    });
    ui.horizontal(|ui| {
        let label = ui.label(RichText::new("Heard from").small().color(style::MUTED));
        let mut target = match cx.get(&key("heard_from")) {
            Value::Target(t) => t,
            _ => bpe_core::Target::Anything,
        };
        let r = crate::widgets::target_picker(ui, &mut target, cx.choices).labelled_by(label.id);
        if r.changed() {
            cx.set(&key("heard_from"), Value::Target(target));
        }
        ui.add_space(12.0);
        now_meter(ui, cx, meters.and_then(|m| m.mod_value(cx.id.0, layer)));
    });
}

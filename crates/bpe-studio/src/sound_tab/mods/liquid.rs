//! A Liquid modulator's editor (liquid part 5): what it reads (Slosh, Mix amount, Level or
//! Amount), which Liquid things, Full at (Slosh), Smooth, Invert and a live Now meter. Without a
//! Liquid thing it says there is nothing to hear yet.

use super::super::TabCx;
use super::distance::now_meter;
use crate::style;
use bpe_core::{LayerId, Value, layered_key};
use bpe_kit::sound::LiquidReads;
use bpe_sound::Meters;
use eframe::egui::{RichText, Ui};

fn ms(v: f64) -> String {
    format!("{v:.0} ms")
}

fn speed(v: f64) -> String {
    format!("{v:.0} px/s")
}

/// Draws Liquid `layer`'s editor.
pub fn show(ui: &mut Ui, cx: &mut TabCx, layer: LayerId, meters: Option<&Meters>) {
    let key = |k: &str| layered_key(layer, k);
    if !cx.scene.things.values().any(|t| t.kind == bpe_kit::things::liquid::KIND) {
        ui.label(
            RichText::new("The scene has no liquid yet: add a Liquid thing and this hears it.")
                .small()
                .color(style::FAINT),
        );
    }
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Reads").small().color(style::MUTED)).on_hover_text(
            "Slosh: how fast the liquid moves. Mix amount: how evenly its colours have blended. Level: how high \
             it stands. Amount: its drops against its Most drops",
        );
        cx.chips(
            ui,
            &key("reads"),
            &[
                (LiquidReads::Slosh, "Slosh"),
                (LiquidReads::Mixed, "Mix amount"),
                (LiquidReads::Height, "Level"),
                (LiquidReads::Amount, "Amount"),
            ],
        );
        ui.add_space(8.0);
        cx.switch(ui, &key("invert"), "Invert");
        ui.label(RichText::new("Invert").small().color(style::MUTED));
    });
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        if cx.choice::<LiquidReads>(&key("reads")) == LiquidReads::Slosh {
            cx.knob(ui, &key("full_at"), "Full at", &speed)
                .on_hover_text("The liquid moving this fast on average gives 1");
        }
        cx.knob(ui, &key("smooth"), "Smooth", &ms);
    });
    ui.horizontal(|ui| {
        let label = ui.label(RichText::new("Which liquid").small().color(style::MUTED));
        let mut target = match cx.get(&key("liquid")) {
            Value::Target(t) => t,
            _ => bpe_core::Target::Anything,
        };
        let r = crate::widgets::target_picker(ui, &mut target, cx.choices).labelled_by(label.id);
        if r.changed() {
            cx.set(&key("liquid"), Value::Target(target));
        }
        ui.add_space(12.0);
        now_meter(ui, cx, meters.and_then(|m| m.mod_value(cx.id.0, layer)));
    });
}

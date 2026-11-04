//! Where the shape's sound goes: Volume, Pan, Follow hit (the pan pulled toward where the hit is
//! across the picture), the bus and a level meter.

use super::{TabCx, name_it, osc::pan, panel};
use crate::style;
use bpe_core::{Address, SlotId, ThingId, Value};
use eframe::egui::{self, RichText, Ui};

fn volume(v: f64) -> String {
    super::db(v)
}

fn percent(v: f64) -> String {
    format!("{v:.0} %")
}

/// Draws the Output panel; `level` is the shape's peak (dB) since the last look.
pub fn show(ui: &mut Ui, cx: &mut TabCx, level: f32) {
    panel(ui, |ui| {
        ui.label(RichText::new("Output").font(style::semibold(13.0)));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            cx.knob(ui, "out_volume", "Volume", &volume);
            cx.knob(ui, "out_pan", "Pan", &pan);
            cx.knob(ui, "out_follow_hit", "Follow hit", &percent)
                .on_hover_text("Pulls the pan toward where the hit is across the picture");
            ui.add_space(12.0);
            style::level_meter(ui, level, level, 64.0).on_hover_text("This shape's level");
        });
        let main = (cx.number("out_bus").round() as i64).clamp(1, 3) as u8;
        ui.horizontal(|ui| {
            let label = ui.label(RichText::new("Bus").small().color(style::MUTED));
            ui.spacing_mut().item_spacing.x = 2.0;
            for n in 1..=3u8 {
                let chip = ui.add(style::toggle_button(&n.to_string(), n == main).min_size(egui::vec2(24.0, 22.0)));
                let chip = chip.labelled_by(label.id).on_hover_text(format!("Play into Bus {n}"));
                name_it(ui, &chip, &format!("Bus {n}"));
                if chip.clicked() && n != main {
                    cx.set("out_bus", Value::Int(n as i64));
                }
            }
            ui.add_space(10.0);
            ui.spacing_mut().item_spacing.x = 0.0;
            for n in (1..=3u8).filter(|n| *n != main) {
                cx.knob(ui, &format!("out_send{n}"), &format!("Send {n}"), &volume)
                    .on_hover_text(format!("How much also goes to Bus {n}"));
            }
        });
        // A bus that is off silences what plays into it.
        let off = |n: u8| !bus_on(cx, n);
        if off(main) {
            warn(ui, &format!("Bus {main} is off: this shape is silent until it's on in the Mixer"));
        }
        for n in (1..=3u8).filter(|n| *n != main) {
            let up = bpe_sound::sends(cx.number(&format!("out_send{n}")) as f32);
            if up && off(n) {
                warn(ui, &format!("Bus {n} is off: the send to it is silent"));
            }
        }
    });
}

/// Whether bus `n` is on at the playhead.
fn bus_on(cx: &TabCx, n: u8) -> bool {
    let key = bpe_kit::sound::MixerSettings::bus_key(n, "on");
    let a = Address::on(ThingId::ROOT, SlotId::Mixer, &key);
    match cx.scene.value_at_time(&cx.kit.registry, &a, cx.time) {
        Some(Value::Bool(on)) => on,
        // Not set: Bus 1 starts on, the others off.
        _ => n == 1,
    }
}

/// A warning line.
fn warn(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).small().color(style::TEXT_STRONG));
}

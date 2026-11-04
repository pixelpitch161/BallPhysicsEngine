//! A Distance modulator's editor: where it measures from, Near and Far, Curve, Smooth, Invert,
//! which balls it follows, which it hears, and a live Now meter.

use super::super::TabCx;
use crate::style;
use bpe_core::{LayerId, Value, layered_key};
use bpe_kit::sound::{Follows, Measure};
use bpe_sound::Meters;
use eframe::egui::{self, RichText, Sense, Ui, Vec2};

fn px(v: f64) -> String {
    format!("{v:.0} px")
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

/// Draws Distance `layer`'s editor.
pub fn show(ui: &mut Ui, cx: &mut TabCx, layer: LayerId, meters: Option<&Meters>) {
    let key = |k: &str| layered_key(layer, k);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Measure from").small().color(style::MUTED));
        cx.chips(ui, &key("measure"), &[(Measure::Centre, "Centre"), (Measure::Edge, "Edge")]);
        ui.add_space(8.0);
        cx.switch(ui, &key("invert"), "Invert");
        ui.label(RichText::new("Invert").small().color(style::MUTED));
    });
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        cx.knob(ui, &key("near"), "Near", &px).on_hover_text("At this distance or closer it gives 1");
        cx.knob(ui, &key("far"), "Far", &px).on_hover_text("At this distance or further it gives 0");
        cx.knob(ui, &key("curve"), "Curve", &signed);
        cx.knob(ui, &key("smooth"), "Smooth", &ms);
        if cx.choice::<Follows>(&key("follows")) == Follows::HowMany {
            cx.knob(ui, &key("full_at"), "Full at", &balls).on_hover_text("This many balls in range gives 1");
        }
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Follows").small().color(style::MUTED));
        cx.chips(
            ui,
            &key("follows"),
            &[
                (Follows::Nearest, "Nearest"),
                (Follows::EachNotesBall, "Each note's ball"),
                (Follows::Average, "Average"),
                (Follows::HowMany, "How many in range"),
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

/// "Now": a bar of the modulator's value as it plays (empty while stopped).
pub fn now_meter(ui: &mut Ui, cx: &TabCx, value: Option<f32>) {
    ui.label(RichText::new("Now").small().color(style::MUTED));
    let (rect, r) = ui.allocate_exact_size(Vec2::new(120.0, 8.0), Sense::hover());
    let v = value.unwrap_or(0.0).clamp(0.0, 1.0);
    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, style::LINE);
    if v > 0.0 {
        let lit = egui::Rect::from_min_size(rect.min, Vec2::new(rect.width() * v, rect.height()));
        painter.rect_filled(lit, 2.0, cx.colour);
    }
    let text = match value {
        Some(v) => format!("{:.0} %", v * 100.0),
        None => "–".into(),
    };
    super::super::name_it(ui, &r, &format!("Now {text}"));
    ui.label(RichText::new(text).font(style::mono(11.0)).color(style::TEXT));
    if value.is_some() {
        ui.ctx().request_repaint();
    }
}

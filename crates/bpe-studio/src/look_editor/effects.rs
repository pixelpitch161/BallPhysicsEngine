//! The scene's Effects card: its stack of effects (the top one first, applied bottom first), an
//! add menu with the effects over the picture, the materials and own code, and under the list
//! the picked effect's settings (an own effect's code box first).

use bpe_core::{LayerId, LayerSection, SettingDef, SlotId, ThingId, Value};
use bpe_gfx::Stage;
use bpe_kit::modules::effects::{self as fx, TakesChoice};
use eframe::egui::{self, RichText, Ui};

use super::{LookCx, code_box, layer_list, layer_settings};
use crate::inspector::{Edit, Inspector};
use crate::style;

/// The picked effect: the one the user picked, or the top one.
fn picked(inspector: &Inspector, card: &bpe_core::Slot, id: ThingId) -> Option<LayerId> {
    let chosen = inspector.selected_layer.get(&id).copied().filter(|l| card.layer(*l).is_some());
    chosen.or_else(|| card.layers.last().map(|l| l.id))
}

/// Draws the Effects card's list and the picked effect's settings.
pub fn show(ui: &mut Ui, inspector: &mut Inspector, cx: &LookCx, edits: &mut Vec<Edit>) {
    let Some(card) = cx.card() else { return };
    header(ui, cx, edits);
    let tops: Vec<&bpe_core::Layer> = layer_list::top_level(card).collect();
    let Some(picked) = picked(inspector, card, cx.id) else {
        let words = "None yet: add an effect to change the whole picture. They apply from the bottom up.";
        ui.label(RichText::new(words).small().color(style::FAINT));
        return;
    };
    let mut rows = Vec::new();
    for layer in tops.iter().rev() {
        rows.push((layer.id, layer_list::row(ui, inspector, cx, card, layer, picked, edits)));
    }
    layer_list::section_drop_target(ui, cx, card, &rows, LayerSection::Effect, edits);

    let Some(layer) = card.layer(picked) else { return };
    let reg = cx.reg();
    let Some(kind) = reg.layer(&layer.kind) else {
        ui.label(RichText::new(format!("Its kind \"{}\" isn't installed.", layer.kind)).color(style::TEXT));
        return;
    };
    ui.add_space(8.0);
    style::divider(ui);
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(&layer.name).font(style::semibold(13.0)));
        if layer.name != kind.name() {
            ui.label(RichText::new(kind.name()).small().color(style::MUTED));
        }
    })
    .response
    .on_hover_text(kind.help());
    ui.add_space(2.0);
    let get = |key: &str| cx.scene.value_at_time(reg, &cx.at(picked, key), cx.time);
    let mut own_rows = Vec::new();
    if layer.kind == fx::OWN {
        let code = match get("code") {
            Some(Value::Text(t)) => t,
            _ => fx::own_start().to_owned(),
        };
        if let Some(changed) = code_box::show_checked(ui, ("effect", cx.id, picked.0), &code, &fx::own_problem) {
            edits.push(Edit::SetSlot(cx.slot, bpe_core::layered_key(picked, "code"), Value::Text(changed)));
        }
        own_rows.push("code");
    }
    let lit = cx.scene.card(ThingId::ROOT, SlotId::Lighting).is_some();
    let takes = get("takes").and_then(|v| <TakesChoice as bpe_core::SettingValue>::from_value(&v));
    if takes == Some(TakesChoice::LightOnly) && !lit {
        let words = "Light only needs a Lighting card: until the scene has one, this effect changes nothing.";
        ui.label(RichText::new(words).small().color(style::MUTED));
    }
    let defs: Vec<SettingDef> =
        kind.settings().iter().chain(kind.dynamic_defs(&get)).filter(|d| !own_rows.contains(&d.key)).cloned().collect();
    layer_settings(ui, inspector, cx, picked, &defs, edits);
}

/// The list's title and its add menu: the effects over the picture, the materials, own code.
fn header(ui: &mut Ui, cx: &LookCx, edits: &mut Vec<Edit>) {
    let reg = cx.reg();
    let kinds: Vec<&dyn bpe_core::LayerKind> =
        reg.layers_for(fx::EFFECTS).filter(|k| k.section() == LayerSection::Effect).collect();
    let stage = |k: &&dyn bpe_core::LayerKind| fx::builtin_of(k.id()).map(|b| b.header().stage);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Effects").font(style::medium(12.0)).color(style::MUTED));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button(RichText::new("+ Add effect").small(), |ui| {
                ui.set_min_width(190.0);
                let mut item = |ui: &mut Ui, k: &dyn bpe_core::LayerKind| {
                    if ui.button(k.name()).on_hover_text(k.help()).clicked() {
                        edits.push(Edit::AddLayer(cx.slot, k.id()));
                        ui.close();
                    }
                };
                ui.label(RichText::new("Over the picture").small().color(style::MUTED));
                for k in kinds.iter().filter(|k| stage(k) == Some(Stage::Picture)) {
                    item(ui, *k);
                }
                ui.separator();
                ui.label(RichText::new("Materials (paint the things)").small().color(style::MUTED));
                for k in kinds.iter().filter(|k| stage(k) == Some(Stage::Things)) {
                    item(ui, *k);
                }
                ui.separator();
                for k in kinds.iter().filter(|k| k.id() == fx::OWN) {
                    item(ui, *k);
                }
            });
        });
    });
}

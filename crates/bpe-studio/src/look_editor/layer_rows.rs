//! Settings of a layer that have rows of their own above its settings grid: a mask's From (as
//! buttons) and the layer it follows (a menu of the layers under its layer), and a Shader
//! layer's Effect (a menu) and own code (a code box).

use bpe_core::{Layer, LayerId, Slot, Value, layered_key};
use bpe_gfx::effects::Builtin;
use bpe_kit::modules::look::layers::{self as kinds, MaskFrom, shader};
use eframe::egui::{self, RichText, Ui};

use super::{LookCx, code_box, layer_list};
use crate::inspector::{Edit, Inspector};
use crate::style;
use crate::widgets::{grid_label, settings_like_grid, widget_cell};

/// The mask's From buttons: each choice, its words and its help.
const FROM: [(MaskFrom, &str, &str); 6] = [
    (MaskFrom::LayerBelow, "Layer below", "Shows where the layer just under it covers the ball"),
    (MaskFrom::Layer, "Layer", "Shows where another layer under it covers the ball"),
    (MaskFrom::Shape, "Shape", "Shows inside a shape: half, wedge, quarters, ring, circle or band"),
    (MaskFrom::Brush, "Brush", "Shows where you paint on the ball in the view above"),
    (MaskFrom::Light, "Light", "Shows where the layers under it are light"),
    (MaskFrom::Dark, "Dark", "Shows where the layers under it are dark"),
];

/// Draws `layer`'s own rows and returns the keys of the settings they show, which the settings
/// grid leaves out.
pub fn show(
    ui: &mut Ui,
    inspector: &mut Inspector,
    cx: &LookCx,
    card: &Slot,
    layer: &Layer,
    edits: &mut Vec<Edit>,
) -> Vec<&'static str> {
    let _ = inspector;
    match (layer.parent, layer.kind.as_str()) {
        (Some(parent), _) => {
            mask_rows(ui, cx, card, layer, parent, edits);
            vec!["from", "target"]
        }
        (None, kinds::SHADER | kinds::GROUP_SHADER) => {
            shader_rows(ui, cx, layer.id, edits);
            vec!["effect", "code"]
        }
        _ => Vec::new(),
    }
}

/// The value of `layer`'s setting `key` at the playhead.
fn value(cx: &LookCx, layer: LayerId, key: &str) -> Option<Value> {
    cx.scene.value_at_time(cx.reg(), &cx.at(layer, key), cx.time)
}

fn set(cx: &LookCx, layer: LayerId, key: &str, v: Value) -> Edit {
    Edit::SetSlot(cx.slot, layered_key(layer, key), v)
}

fn mask_rows(ui: &mut Ui, cx: &LookCx, card: &Slot, layer: &Layer, parent: LayerId, edits: &mut Vec<Edit>) {
    let from = value(cx, layer.id, "from").and_then(|v| <MaskFrom as bpe_core::SettingValue>::from_value(&v));
    let from = from.unwrap_or_default();
    let right = ui.available_rect_before_wrap().right();
    settings_like_grid(("mask rows", cx.id, layer.id.0)).show(ui, |ui| {
        grid_label(ui, cx.label_width, "From").on_hover_text("What the mask shows its layer by");
        widget_cell(ui, right, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
                for (choice, words, help) in FROM {
                    let on = from == choice;
                    let button = style::toggle_button(words, on).frame_when_inactive(true);
                    if ui.add(button).on_hover_text(help).clicked() && !on {
                        edits.push(set(cx, layer.id, "from", bpe_core::SettingValue::to_value(&choice)));
                    }
                }
                let delete = ui.add(egui::Button::new(RichText::new("Delete mask").small()).frame(false));
                if delete.on_hover_text("Takes the mask away: the layer shows everywhere again").clicked() {
                    edits.push(Edit::RemoveLayer(cx.slot, layer.id));
                }
            });
        });
        ui.end_row();
        if from == MaskFrom::Layer {
            grid_label(ui, cx.label_width, "Layer").on_hover_text("The layer the mask follows");
            let below = layer_list::layers_below(cx, card, parent);
            let target = match value(cx, layer.id, "target") {
                Some(Value::Int(t)) => t,
                _ => -1,
            };
            let current = below.iter().find(|l| i64::from(l.id.0) == target);
            let words = current.map_or("Pick a layer".to_owned(), |l| l.name.clone());
            widget_cell(ui, right, |ui| {
                let menu = style::raised(ui, |ui| {
                    ui.menu_button(words, |ui| {
                        ui.set_min_width(150.0);
                        for l in below.iter().rev() {
                            let text = if l.enabled { l.name.clone() } else { format!("{} (hidden)", l.name) };
                            if ui.selectable_label(i64::from(l.id.0) == target, text).clicked() {
                                edits.push(set(cx, layer.id, "target", Value::Int(i64::from(l.id.0))));
                                ui.close();
                            }
                        }
                    })
                });
                menu.response
            });
            ui.end_row();
        }
    });
}

/// The effect's name as the menu shows it.
fn effect_words(effect: &str) -> String {
    if effect == shader::OWN {
        return "Own code".to_owned();
    }
    Builtin::find(effect).map_or_else(|| effect.to_owned(), |b| b.header().name.clone())
}

fn shader_rows(ui: &mut Ui, cx: &LookCx, layer: LayerId, edits: &mut Vec<Edit>) {
    let effect = match value(cx, layer, "effect") {
        Some(Value::Choice(e)) => e,
        _ => "lava".to_owned(),
    };
    let right = ui.available_rect_before_wrap().right();
    settings_like_grid(("shader rows", cx.id, layer.0)).show(ui, |ui| {
        let help = "Which effect paints the layer: one that comes with Ball Studio, or your own code";
        grid_label(ui, cx.label_width, "Effect").on_hover_text(help);
        widget_cell(ui, right, |ui| {
            let menu = style::raised(ui, |ui| {
                ui.menu_button(effect_words(&effect), |ui| {
                    ui.set_min_width(160.0);
                    for id in shader::EFFECTS.iter().filter(|e| **e != shader::OWN) {
                        let item = ui.selectable_label(effect == *id, effect_words(id));
                        let about = Builtin::find(id).map_or("", |b| b.header().about.as_str());
                        if item.on_hover_text(about).clicked() {
                            edits.push(set(cx, layer, "effect", Value::Choice((*id).into())));
                            ui.close();
                        }
                    }
                    ui.separator();
                    let own = ui.selectable_label(effect == shader::OWN, "Own code");
                    if own.on_hover_text("Write the effect yourself, in the effect file form").clicked() {
                        edits.push(set(cx, layer, "effect", Value::Choice(shader::OWN.into())));
                        ui.close();
                    }
                })
            });
            menu.response
        });
        ui.end_row();
    });
    if effect == shader::OWN {
        let code = match value(cx, layer, "code") {
            Some(Value::Text(t)) => t,
            _ => shader::own_start().to_owned(),
        };
        if let Some(changed) = code_box::show(ui, (cx.id, layer.0), &code) {
            edits.push(set(cx, layer, "code", Value::Text(changed)));
        }
    }
}

//! Follow…: the menu that wires a knob to something the balls do, and the small mark a
//! followed knob wears.

use crate::theme;
use bpe_core::{ModuleId, Scene, SlotId};
use bpe_synth::{FollowInput, HIT_INPUTS, InputDef, LIVE_INPUTS, SCENE_INPUTS};
use eframe::egui::{self, Sense, Ui};

/// The names of the scene's sound modules other than `except`, for Channel level.
pub fn channel_names(scene: &Scene, except: Option<ModuleId>) -> Vec<String> {
    let mut names: Vec<String> = scene
        .library
        .modules
        .iter()
        .filter(|(id, m)| m.slot == SlotId::Sound && Some(**id) != except)
        .map(|(_, m)| m.name.clone())
        .collect();
    names.dedup();
    names
}

fn pick(kind: &str, input: &str, channel: &str) -> FollowInput {
    FollowInput { kind: kind.into(), input: input.into(), channel: channel.into() }
}

fn list(ui: &mut Ui, kind: &str, table: &[InputDef], channels: &[String], out: &mut Option<FollowInput>) {
    for def in table {
        if def.name == "channel" {
            ui.menu_button(def.label, |ui| {
                if channels.is_empty() {
                    ui.weak("No other sounds");
                }
                for c in channels {
                    if ui.button(c).clicked() {
                        *out = Some(pick(kind, def.name, c));
                        ui.close();
                    }
                }
            });
        } else if ui.button(def.label).clicked() {
            *out = Some(pick(kind, def.name, ""));
            ui.close();
        }
    }
}

/// The "Follow…" submenu, inside a context menu. At-the-hit and live inputs are offered only
/// for a patch (`patch`), since only voices have a hit; Scene inputs always.
pub fn menu(ui: &mut Ui, patch: bool, channels: &[String]) -> Option<FollowInput> {
    let mut out = None;
    ui.menu_button("Follow…", |ui| {
        if patch {
            ui.menu_button("At the hit", |ui| list(ui, "hit_input", HIT_INPUTS, channels, &mut out));
            ui.menu_button("Live", |ui| list(ui, "live_input", LIVE_INPUTS, channels, &mut out));
        }
        ui.menu_button("Scene", |ui| list(ui, "scene_input", SCENE_INPUTS, channels, &mut out));
    })
    .response
    .on_hover_text("Move this knob with something the balls do");
    out
}

/// A small wave after a followed knob's name, saying what it follows.
pub fn mark(ui: &mut Ui, label: Option<&str>) {
    let Some(label) = label else { return };
    let text = format!("Follows {label}");
    let (rect, response) = ui.allocate_exact_size(egui::vec2(14.0, 10.0), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &text));
    let points: Vec<egui::Pos2> = (0..=12)
        .map(|k| {
            let t = k as f32 / 12.0;
            egui::pos2(rect.left() + t * rect.width(), rect.center().y - (t * std::f32::consts::TAU).sin() * 3.5)
        })
        .collect();
    ui.painter().add(egui::Shape::line(points, egui::Stroke::new(1.5, theme::ACCENT)));
    response.on_hover_text(text);
}

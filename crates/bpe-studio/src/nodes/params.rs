//! The control for one block setting, shared by the canvas and the module cards. Target
//! settings (who, what, target, into) get a picker that names the scene's things, tags and
//! kinds instead of a text box.

use crate::theme;
use bpe_core::{Scene, Target};
use bpe_graph::{Param, ParamSpec};
use bpe_vm::{target_from_text, target_to_text};
use eframe::egui::{self, Id, Ui};
use std::collections::BTreeSet;

/// The setting names that hold a target, so they get the picker.
pub fn is_target_param(name: &str) -> bool {
    matches!(name, "who" | "what" | "target" | "into")
}

/// A target as the picker shows it.
pub fn target_words(scene: &Scene, target: &Target) -> String {
    match target {
        Target::Me => "Me".into(),
        Target::Anything => "Anything".into(),
        Target::Thing(id) => scene.things.get(id).map_or_else(|| "Missing thing".into(), |t| t.name.clone()),
        Target::Tag(tag) => format!("#{tag}"),
        Target::Kind(kind) => format!("Any {kind}"),
    }
}

/// Shows the control for `param`, named for screen readers by the label with id `label`, and
/// returns whether it changed. A number spanning the whole default range gets a drag box; a
/// narrower one gets a slider.
pub fn param_control(ui: &mut Ui, spec: &ParamSpec, param: &mut Param, scene: &Scene, label: Id) -> bool {
    let before = param.clone();
    match param {
        Param::Number(v) => {
            let (lo, hi) = spec.range;
            if hi > lo && (hi - lo) < 20_000.0 {
                ui.spacing_mut().slider_width = 70.0;
                // Even steps, like the inspector's sliders. A template may hold a value past the
                // slider's end; only the user's own edits are clamped, so drawing a block never
                // changes it.
                let feel = crate::widgets::number_feel((hi - lo) as f64, false);
                let slider = egui::Slider::new(v, lo..=hi)
                    .show_value(true)
                    .smart_aim(false)
                    .step_by(feel.step)
                    .max_decimals(feel.decimals)
                    .drag_value_speed(feel.drag_speed)
                    .clamping(egui::SliderClamping::Edits);
                ui.add(slider).labelled_by(label);
            } else {
                let mut drag = egui::DragValue::new(v).speed(((hi - lo) / 300.0).max(0.001));
                if hi > lo {
                    drag = drag.range(lo..=hi).clamp_existing_to_range(false);
                }
                ui.add(drag).labelled_by(label);
            }
        }
        Param::Bool(b) => {
            ui.checkbox(b, "").labelled_by(label);
        }
        Param::Text(t) if is_target_param(&spec.name) => {
            target_picker(ui, t, scene);
        }
        Param::Text(t) => {
            ui.add(egui::TextEdit::singleline(t).desired_width(100.0)).labelled_by(label);
        }
        Param::Color(c) => {
            let mut rgba = egui::Rgba::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
            let response =
                egui::color_picker::color_edit_button_rgba(ui, &mut rgba, egui::color_picker::Alpha::OnlyBlend);
            if response.labelled_by(label).changed() {
                let [r, g, b, a] = rgba.to_rgba_unmultiplied();
                *c = [r, g, b, a];
            }
        }
        Param::Vector(v) => {
            ui.add(egui::DragValue::new(&mut v[1]).speed(1.0).prefix("y ")).labelled_by(label);
            ui.add(egui::DragValue::new(&mut v[0]).speed(1.0).prefix("x ")).labelled_by(label);
        }
        Param::Choice(choice) => {
            egui::ComboBox::from_id_salt(label.with(&spec.name))
                .selected_text(choice.as_str())
                .width(96.0)
                .show_ui(ui, |ui| {
                    for option in &spec.choices {
                        ui.selectable_value(choice, option.clone(), option);
                    }
                })
                .response
                .labelled_by(label);
        }
    }
    *param != before
}

/// A menu of everything a target can name: me, anything, each thing, each tag, each kind. The
/// button carries the current choice as its own name.
fn target_picker(ui: &mut Ui, text: &mut String, scene: &Scene) {
    let current = target_from_text(text);
    let mut pick = |ui: &mut Ui, target: Target| {
        let words = target_words(scene, &target);
        if ui.selectable_label(current == target, words).clicked() {
            *text = target_to_text(&target);
            ui.close();
        }
    };
    ui.menu_button(target_words(scene, &current), |ui| {
        ui.set_min_width(160.0);
        pick(ui, Target::Me);
        pick(ui, Target::Anything);
        if !scene.things.is_empty() {
            ui.separator();
            ui.label(egui::RichText::new("Things").small().color(theme::MUTED));
            for id in scene.things.keys() {
                pick(ui, Target::Thing(*id));
            }
        }
        let tags: BTreeSet<&String> = scene.things.values().flat_map(|t| t.tags.iter()).collect();
        if !tags.is_empty() {
            ui.separator();
            ui.label(egui::RichText::new("Tags").small().color(theme::MUTED));
            for tag in tags {
                pick(ui, Target::Tag(tag.clone()));
            }
        }
        let kinds: BTreeSet<&String> = scene.things.values().map(|t| &t.kind).collect();
        if !kinds.is_empty() {
            ui.separator();
            ui.label(egui::RichText::new("Kinds").small().color(theme::MUTED));
            for kind in kinds {
                pick(ui, Target::Kind(kind.clone()));
            }
        }
    });
}

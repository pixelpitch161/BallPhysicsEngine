//! The Lighting card's lighting scenes (lighting part 6): what shows at the playhead, a row per
//! scene with its menu, and "+ Save the lights as a scene". The scenes' settings (Scenes play,
//! Show scene, Crossfade…) are rows of the card's own grid above.

use bpe_core::{Registry, Scene, SlotId, ThingId, Value};
use bpe_kit::modules::light_scenes as scenes;
use eframe::egui::{self, RichText, Ui};

use crate::inspector::{Edit, Inspector};
use crate::style;

/// A change to the lighting scenes, applied after the inspector is drawn.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SceneEdit {
    /// Saves the lights as they are at the playhead as a new scene.
    Add,
    /// Saves the lights as they are at the playhead over a scene, keeping its name.
    Update(usize),
    Rename(usize, String),
    Duplicate(usize),
    /// Moves a scene from one place in the list to another.
    Move(usize, usize),
    Remove(usize),
    /// Writes a scene's saved settings back onto its lights.
    PutOn(usize),
}

/// Applies `edit` to the scene; whether anything changed.
pub(crate) fn apply(scene: &mut Scene, reg: &Registry, edit: SceneEdit, time: f64) -> bool {
    match edit {
        SceneEdit::Add => scenes::add_scene(scene, reg, time).is_ok(),
        SceneEdit::Update(i) => scenes::update_scene(scene, reg, i, time).is_ok(),
        SceneEdit::Rename(i, name) => !name.trim().is_empty() && scenes::rename_scene(scene, reg, i, &name).is_ok(),
        SceneEdit::Duplicate(i) => scenes::duplicate_scene(scene, reg, i).is_ok(),
        SceneEdit::Move(from, to) => from != to && scenes::move_scene(scene, reg, from, to).is_ok(),
        SceneEdit::Remove(i) => scenes::remove_scene(scene, reg, i).is_ok(),
        SceneEdit::PutOn(i) => scenes::put_on_lights(scene, reg, i, time).is_ok(),
    }
}

/// Whether the scene has any light or rig to save in a lighting scene.
fn has_lights(scene: &Scene) -> bool {
    scene.things.values().any(|t| t.kind == bpe_kit::things::light::LIGHT || t.kind == bpe_kit::things::rig::RIG)
}

/// The scenes list under the Lighting card's settings.
pub(crate) fn show(
    ui: &mut Ui,
    inspector: &mut Inspector,
    scene: &Scene,
    reg: &Registry,
    time: f64,
    edits: &mut Vec<Edit>,
) {
    let list = scenes::scenes_of(scene);
    let play = scene
        .value_at_time(reg, &bpe_core::Address::on(ThingId::ROOT, SlotId::Lighting, "scenes_play"), time)
        .and_then(|v| match v {
            Value::Choice(c) => Some(c),
            _ => None,
        })
        .unwrap_or_default();
    ui.add_space(4.0);
    if !list.is_empty() {
        let words = match play.as_str() {
            "off" => "Showing: the lights' own settings (Scenes play is Off)".to_owned(),
            "on_hits" => "Showing: the next scene on each heard hit".to_owned(),
            _ => format!("Showing: {}", scenes::words_at(scene, reg, time).unwrap_or_default()),
        };
        ui.label(RichText::new(words).small().color(style::MUTED));
    }
    let on_timeline = play == "on_the_timeline";
    let last = list.len().saturating_sub(1);
    for (i, s) in list.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{}", i + 1)).small().color(style::MUTED));
            match &mut inspector.renaming_scene {
                Some((at, text)) if *at == i => {
                    let field = ui.add(egui::TextEdit::singleline(text).desired_width(140.0));
                    field.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Scene name"));
                    field.request_focus();
                    if field.lost_focus() {
                        let name = std::mem::take(text);
                        edits.push(Edit::LightScene(SceneEdit::Rename(i, name)));
                        inspector.renaming_scene = None;
                    }
                }
                _ => {
                    let name = ui.label(RichText::new(&s.name).color(style::TEXT));
                    if name.double_clicked() {
                        inspector.renaming_scene = Some((i, s.name.clone()));
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let menu = ui.menu_button("…", |ui| {
                    ui.set_min_width(170.0);
                    let mut item = |ui: &mut Ui, enabled: bool, label: &str, help: &str, edit: SceneEdit| {
                        if ui.add_enabled(enabled, egui::Button::new(label)).on_hover_text(help).clicked() {
                            edits.push(Edit::LightScene(edit));
                            ui.close();
                        }
                    };
                    if ui.button("Rename").clicked() {
                        inspector.renaming_scene = Some((i, s.name.clone()));
                        ui.close();
                    }
                    item(
                        ui,
                        true,
                        "Update from the lights",
                        "Saves the lights as they are now over this scene",
                        SceneEdit::Update(i),
                    );
                    item(
                        ui,
                        true,
                        "Put on the lights",
                        "Gives the lights this scene's settings (as keys where they have keys)",
                        SceneEdit::PutOn(i),
                    );
                    item(ui, true, "Duplicate", "A copy of this scene right after it", SceneEdit::Duplicate(i));
                    item(ui, i > 0, "Move up", "Moves the scene up the list", SceneEdit::Move(i, i.saturating_sub(1)));
                    item(
                        ui,
                        i < last,
                        "Move down",
                        "Moves the scene down the list",
                        SceneEdit::Move(i, (i + 1).min(last)),
                    );
                    item(ui, true, "Delete", "Takes the scene away", SceneEdit::Remove(i));
                });
                let name = format!("{} menu", s.name);
                menu.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
                if on_timeline {
                    let here = ui.add(egui::Button::new(RichText::new("Show here").small()));
                    let label = format!("Show {} here", s.name);
                    here.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
                    if here
                        .on_hover_text("Shows this scene from the playhead (a key when Show scene has keys)")
                        .clicked()
                    {
                        edits.push(Edit::SetSlot(
                            SlotId::Lighting,
                            scenes::SHOW_SCENE.into(),
                            Value::Int(i as i64 + 1),
                        ));
                    }
                }
            });
        });
    }
    let can = has_lights(scene);
    let add = ui
        .add_enabled(can, egui::Button::new("+ Save the lights as a scene"))
        .on_hover_text("Saves every light's and rig's settings at the playhead as a new lighting scene")
        .on_disabled_hover_text("Add a light or a rig first");
    if add.clicked() {
        edits.push(Edit::LightScene(SceneEdit::Add));
    }
}

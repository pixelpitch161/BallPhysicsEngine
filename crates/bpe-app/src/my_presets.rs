//! "My presets" menus: save a ball look, a tab's settings or a whole scene under a name, and
//! reuse it in any scene. Presets live as files in the user's settings folder (see
//! `bpe_scene::SavedPreset`), so every scene sees the same ones.

use crate::theme;
use bpe_scene::{BallStyle, PresetData, PresetKind, SavedPreset, Scene};
use eframe::egui::{self, Popup, PopupCloseBehavior, RichText, Ui};
use std::path::{Path, PathBuf};

/// The folder presets are saved in: `presets` in the settings folder, or `BPE_PRESETS_DIR`.
pub fn folder() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("BPE_PRESETS_DIR") {
        return Some(PathBuf::from(dir));
    }
    crate::settings::settings_dir().map(|d| d.join("presets"))
}

fn base_dir_id() -> egui::Id {
    egui::Id::unique("my-presets-base-dir")
}

/// Tells the menus which folder the open scene is in, so saved presets keep finding its files.
pub fn publish_base_dir(ctx: &egui::Context, base_dir: Option<PathBuf>) {
    ctx.data_mut(|d| d.insert_temp(base_dir_id(), base_dir));
}

fn base_dir(ctx: &egui::Context) -> Option<PathBuf> {
    ctx.data(|d| d.get_temp::<Option<PathBuf>>(base_dir_id())).flatten()
}

/// Saves `data` as `name`, with its file paths made absolute. Returns a line for the user.
pub fn save(ctx: &egui::Context, name: &str, mut data: PresetData) -> String {
    let Some(dir) = folder() else { return "Couldn't find a settings folder to save presets in.".into() };
    if let Some(base) = base_dir(ctx) {
        data.absolute_paths(&base);
    }
    let kind = data.kind();
    let preset = SavedPreset { name: name.trim().to_string(), data };
    match preset.save(&dir) {
        Ok(_) => format!("Saved \"{}\" to your {}s.", preset.name, kind.name()),
        Err(error) => format!("Couldn't save the preset: {error}"),
    }
}

/// What the user did in a presets menu.
enum Pick {
    Apply(PresetData),
    Save(String),
}

/// Menu state kept between frames: the name being typed, a pending delete and the last message.
#[derive(Clone, Default)]
struct MenuState {
    name: String,
    deleting: Option<String>,
    message: Option<String>,
}

/// The "My presets" button and its menu, for one kind of preset.
fn menu(ui: &mut Ui, kind: PresetKind, id: egui::Id, can_save: bool) -> Option<Pick> {
    let button = ui
        .button(RichText::new("My presets…").color(theme::ACCENT_TEXT))
        .on_hover_text(format!("Save this {} to reuse in other scenes, or use one you saved", kind.name()));
    let mut pick = None;
    let state_id = id.with("state");
    Popup::menu(&button).id(id).close_behavior(PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        ui.set_min_width(280.0);
        let mut state: MenuState = ui.ctx().data(|d| d.get_temp(state_id)).unwrap_or_default();
        let Some(dir) = folder() else {
            ui.label("Couldn't find a settings folder to keep presets in.");
            return;
        };
        ui.label(RichText::new(format!("MY {}S", kind.name().to_uppercase())).size(12.0).color(theme::MUTED));
        let saved = SavedPreset::list(&dir, kind);
        if saved.is_empty() {
            ui.label(RichText::new("Nothing saved yet.").color(theme::MUTED));
        }
        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
            for preset in &saved {
                ui.horizontal(|ui| {
                    let deleting = state.deleting.as_deref() == Some(preset.name.as_str());
                    if deleting {
                        ui.label(format!("Delete \"{}\"?", preset.name));
                        if ui.small_button("Delete").clicked() {
                            state.message = Some(match preset.delete(&dir) {
                                Ok(()) => format!("Deleted \"{}\".", preset.name),
                                Err(error) => format!("Couldn't delete it: {error}"),
                            });
                            state.deleting = None;
                        }
                        if ui.small_button("Keep").clicked() {
                            state.deleting = None;
                        }
                        return;
                    }
                    let name = egui::Button::new(&preset.name).min_size(egui::vec2(200.0, 0.0));
                    if ui.add(name).on_hover_text(format!("Use this. It replaces {}.", kind.replaces())).clicked() {
                        pick = Some(Pick::Apply(preset.data.clone()));
                        state.message = None;
                        ui.close();
                    }
                    if ui.small_button("Delete").on_hover_text("Delete this preset").clicked() {
                        state.deleting = Some(preset.name.clone());
                    }
                });
            }
        });
        ui.separator();
        ui.label(RichText::new(format!("Save this {} as", kind.name())).color(theme::MUTED));
        ui.horizontal(|ui| {
            let field = ui.add(egui::TextEdit::singleline(&mut state.name).desired_width(170.0).hint_text("Name"));
            let name = state.name.trim().to_string();
            let replace = SavedPreset::exists(&dir, kind, &name);
            let label = if replace { "Replace" } else { "Save" };
            let save =
                ui.add_enabled(can_save && !name.is_empty(), egui::Button::new(label)).on_hover_text(if replace {
                    "A preset with this name is saved already. This replaces it."
                } else {
                    ""
                });
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && can_save;
            if (save.clicked() || entered) && !name.is_empty() {
                pick = Some(Pick::Save(name));
                state.message = None;
                state.name.clear();
            }
        });
        if let Some(message) = &state.message {
            ui.label(RichText::new(message).small().color(theme::MUTED));
        }
        ui.ctx().data_mut(|d| d.insert_temp(state_id, state));
    });
    pick
}

/// A note shown under the menu after it saved, until the menu is used again.
fn remember_message(ctx: &egui::Context, id: egui::Id, message: String) {
    let state_id = id.with("state");
    ctx.data_mut(|d| {
        let state = d.get_temp_mut_or_default::<MenuState>(state_id);
        state.message = Some(message);
    });
}

/// The "My presets" menu for a tab's settings, at the top of the tab. Using a preset replaces
/// that part of `scene`.
pub fn bar(ui: &mut Ui, kind: PresetKind, scene: &mut Scene) {
    let id = egui::Id::unique(("my-presets", kind.folder()));
    let mut pick = None;
    ui.horizontal(|ui| {
        pick = menu(ui, kind, id, true);
    });
    ui.add_space(4.0);
    match pick {
        Some(Pick::Apply(data)) => data.apply(scene, None),
        Some(Pick::Save(name)) => {
            if let Some(data) = PresetData::capture(scene, kind) {
                let message = save(ui.ctx(), &name, data);
                remember_message(ui.ctx(), id, message);
            }
        }
        None => {}
    }
}

/// The "My presets" menu for one ball group's look. Saving needs the group to have its own look.
pub fn ball_look_menu(ui: &mut Ui, index: usize, style: &mut Option<BallStyle>) {
    let id = egui::Id::unique(("my-presets", PresetKind::BallLook.folder(), index));
    match menu(ui, PresetKind::BallLook, id, style.is_some()) {
        Some(Pick::Apply(PresetData::BallLook(saved))) => *style = Some(saved),
        Some(Pick::Save(name)) => {
            if let Some(current) = style.clone() {
                let message = save(ui.ctx(), &name, PresetData::BallLook(current));
                remember_message(ui.ctx(), id, message);
            }
        }
        _ => {}
    }
}

/// Every whole scene the user saved, for the gallery.
pub fn scenes() -> Vec<SavedPreset> {
    folder().map_or_else(Vec::new, |dir| SavedPreset::list(&dir, PresetKind::Scene))
}

/// Deletes a saved scene preset.
pub fn delete(preset: &SavedPreset) -> anyhow::Result<()> {
    let dir = folder().ok_or_else(|| anyhow::anyhow!("no settings folder"))?;
    preset.delete(&dir)
}

/// Saves `scene` as a whole-scene preset called `name`. `base_dir` is the scene file's folder.
pub fn save_scene(name: &str, scene: &Scene, base_dir: Option<&Path>) -> anyhow::Result<()> {
    let dir = folder().ok_or_else(|| anyhow::anyhow!("no settings folder"))?;
    let mut data = PresetData::Scene(Box::new(scene.clone()));
    if let Some(base) = base_dir {
        data.absolute_paths(base);
    }
    SavedPreset { name: name.trim().to_string(), data }.save(&dir)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::inspector::{self, Inspector, Tab};
    use bpe_scene::{PresetKind, SavedPreset, Scene};
    use eframe::egui;
    use egui_kittest::{Harness, kittest::Queryable};

    struct Panel {
        inspector: Inspector,
        scene: Scene,
    }

    fn harness(tab: Tab, scene: Scene) -> Harness<'static, Panel> {
        let panel = Panel { inspector: Inspector { tab, ..Inspector::default() }, scene };
        let mut harness = Harness::builder()
            .with_size(egui::vec2(460.0, 3000.0))
            .with_step_dt(1.0 / 60.0)
            .allow_missing_glyphs()
            .with_context_setup(crate::theme::apply)
            .build_ui_state(|ui, p: &mut Panel| inspector::show(ui, &mut p.inspector, &mut p.scene), panel);
        harness.run();
        harness
    }

    fn click(harness: &mut Harness<'_, Panel>, label: &str) {
        harness.get_by_label(label).click();
        harness.run();
    }

    /// Saves the Look tab as a preset through its menu, then uses it on another scene.
    #[test]
    fn save_a_look_and_use_it_in_another_scene() {
        let dir = std::env::temp_dir().join(format!("bpe-my-presets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // SAFETY: only these tests read the variable, and they all set the same value.
        unsafe { std::env::set_var("BPE_PRESETS_DIR", &dir) };

        let mut scene = Scene::default();
        scene.look.trail_seconds = 0.77;
        let mut first = harness(Tab::Look, scene);
        click(&mut first, "My presets…");
        first.get_by_role(egui::accesskit::Role::TextInput).click();
        first.run();
        first.get_by_role(egui::accesskit::Role::TextInput).type_text("Long trails");
        first.run();
        click(&mut first, "Save");
        let saved = SavedPreset::list(&dir, PresetKind::Look);
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].name, "Long trails");

        let mut other = harness(Tab::Look, Scene::default());
        click(&mut other, "My presets…");
        click(&mut other, "Long trails");
        assert_eq!(other.state().scene.look.trail_seconds, 0.77);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

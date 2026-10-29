//! The Library window: every shared module by slot, to rename, duplicate, delete, edit and drag
//! onto a thing in the tree.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::presets::{self, Preset};
use crate::settings;
use crate::theme;
use crate::widgets::{GridCx, TargetChoices, settings_grid_marked};
use bpe_core::{Address, Depth, EditError, ModuleId, Owner, Resolved, Scene, SlotId, ThingId};
use bpe_kit::Kit;
use eframe::egui::{self, Key, RichText, Sense, Ui};

pub struct LibraryWindow {
    pub open: bool,
    pub selected: Option<ModuleId>,
    /// The depth each module's settings are open at.
    pub depth: BTreeMap<ModuleId, Depth>,
    renaming: Option<ModuleId>,
    rename_text: String,
    focus_rename: bool,
    /// Where "My presets" are kept; `None` shows none.
    pub presets_dir: Option<PathBuf>,
    /// The presets as last read; read again when the window opens and after a save or delete.
    presets: Vec<Preset>,
    presets_read: bool,
    /// The preset whose Delete was clicked, waiting for "Delete for good" or "Keep".
    confirm_delete: Option<usize>,
}

impl Default for LibraryWindow {
    fn default() -> Self {
        Self {
            open: false,
            selected: None,
            depth: BTreeMap::new(),
            renaming: None,
            rename_text: String::new(),
            focus_rename: false,
            presets_dir: settings::presets_dir(),
            presets: Vec::new(),
            presets_read: false,
            confirm_delete: None,
        }
    }
}

impl LibraryWindow {
    /// Reads the presets folder again at the next frame.
    pub fn look_again(&mut self) {
        self.presets_read = false;
    }

    fn read_presets(&mut self) {
        if self.presets_read {
            return;
        }
        self.presets_read = true;
        self.confirm_delete = None;
        self.presets = self.presets_dir.as_deref().map(presets::list_in).unwrap_or_default();
    }
}

#[derive(Default)]
pub struct LibraryOutcome {
    pub edited: bool,
    /// The things to select instead of the current selection.
    pub select: Option<Vec<ThingId>>,
    /// A word for the status line about something that was refused.
    pub message: Option<String>,
}

enum Action {
    Select(ModuleId),
    StartRename(ModuleId),
    Rename(ModuleId, String),
    /// A module of this kind, named after it.
    New(&'static str, &'static str),
    Duplicate(ModuleId),
    Delete(ModuleId),
    SelectUsers(ModuleId),
    Set(ModuleId, &'static str, bpe_core::Value),
    Lock(ModuleId, &'static str, bool),
    SavePreset(ModuleId),
    /// A preset by its place in the list.
    AddPreset(usize),
    DeletePreset(usize),
    /// Delete was clicked on a preset: ask before the file goes.
    AskDeletePreset(usize),
    KeepPreset,
}

/// Draws the window when it is open and applies what was done in it.
pub fn show(
    ctx: &egui::Context,
    lib: &mut LibraryWindow,
    scene: &mut Scene,
    kit: &Kit,
    resolved: &Resolved,
) -> LibraryOutcome {
    let mut out = LibraryOutcome::default();
    if !lib.open {
        lib.presets_read = false;
        return out;
    }
    lib.read_presets();
    let reg = &kit.registry;
    let mut action: Option<Action> = None;
    let mut open = lib.open;
    egui::Window::new("Library").open(&mut open).default_size([560.0, 640.0]).min_size([420.0, 320.0]).show(
        ctx,
        |ui| {
            egui::Panel::left("library-list").resizable(true).default_size(230.0).show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    list(ui, lib, scene, kit, &mut action);
                });
            });
            egui::CentralPanel::default().show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    details(ui, lib, scene, kit, resolved, &mut action);
                });
            });
        },
    );
    lib.open = open;

    match action {
        None => {}
        Some(Action::Select(m)) => lib.selected = Some(m),
        Some(Action::StartRename(m)) => {
            lib.renaming = Some(m);
            lib.rename_text = scene.library.modules.get(&m).map(|m| m.name.clone()).unwrap_or_default();
            lib.focus_rename = true;
            lib.selected = Some(m);
        }
        Some(Action::Rename(m, name)) => {
            lib.renaming = None;
            let name = name.trim();
            if !name.is_empty() && scene.library.modules.get(&m).is_some_and(|x| x.name != name) {
                out.edited |= scene.rename_module(m, name).is_ok();
            }
        }
        Some(Action::New(kind, name)) => match scene.add_module(reg, kind, name) {
            Ok(m) => {
                lib.selected = Some(m);
                out.edited = true;
            }
            Err(e) => out.message = Some(e.to_string()),
        },
        Some(Action::Duplicate(m)) => match scene.duplicate_module(m) {
            Ok(copy) => {
                lib.selected = Some(copy);
                out.edited = true;
            }
            Err(e) => out.message = Some(e.to_string()),
        },
        Some(Action::Delete(m)) => match scene.remove_module(m) {
            Ok(()) => {
                if lib.selected == Some(m) {
                    lib.selected = None;
                }
                out.edited = true;
            }
            Err(e) => out.message = Some(e.to_string()),
        },
        Some(Action::SelectUsers(m)) => out.select = Some(scene.users_of(m)),
        Some(Action::Set(m, key, value)) => {
            let a = Address { owner: Owner::Module(m), slot: None, key: key.into() };
            match scene.set_at(reg, &a, value) {
                Ok(()) => out.edited = true,
                Err(e) => out.message = Some(e.to_string()),
            }
        }
        Some(Action::Lock(m, key, lock)) => {
            let a = Address { owner: Owner::Module(m), slot: None, key: key.into() };
            out.edited |= scene.set_locked(a, lock);
        }
        Some(Action::SavePreset(m)) => {
            out.message = Some(match (lib.presets_dir.as_deref(), scene.library.modules.get(&m)) {
                (Some(dir), Some(module)) => match presets::save_in(dir, module) {
                    Ok(_) => format!("Saved the preset {}.", module.name),
                    Err(e) => format!("Couldn't save the preset: {e:#}"),
                },
                (None, _) => "There is no settings folder to keep presets in.".to_owned(),
                (_, None) => EditError::MissingModule(m).to_string(),
            });
            lib.look_again();
        }
        Some(Action::AddPreset(i)) => {
            if let Some(preset) = lib.presets.get(i) {
                let id = presets::add_to_scene(scene, &preset.module);
                lib.selected = Some(id);
                out.edited = true;
                out.message = Some(format!("Added {} to the scene.", scene.library.modules[&id].name));
            }
        }
        Some(Action::AskDeletePreset(i)) => lib.confirm_delete = Some(i),
        Some(Action::KeepPreset) => lib.confirm_delete = None,
        Some(Action::DeletePreset(i)) => {
            lib.confirm_delete = None;
            if let Some(preset) = lib.presets.get(i) {
                out.message = Some(match presets::delete(preset) {
                    Ok(()) => format!("Deleted the preset {}.", preset.module.name),
                    Err(e) => format!("Couldn't delete the preset: {e}"),
                });
                lib.look_again();
            }
        }
    }
    out
}

/// The left side: every module under its slot, with a "+ New" menu per slot.
fn list(ui: &mut Ui, lib: &mut LibraryWindow, scene: &Scene, kit: &Kit, action: &mut Option<Action>) {
    let reg = &kit.registry;
    for slot in SlotId::ALL {
        let modules: Vec<&bpe_core::Module> = scene.library.modules.values().filter(|m| m.slot == slot).collect();
        let kinds: Vec<&dyn bpe_core::ModuleKind> = reg.modules_for(slot).collect();
        if modules.is_empty() && kinds.is_empty() {
            continue;
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(slot.name()).strong().color(theme::MUTED));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled_ui(!kinds.is_empty(), |ui| {
                    ui.menu_button(format!("+ New {}", slot.name()), |ui| {
                        for kind in &kinds {
                            if ui.button(kind.name()).on_hover_text(kind.help()).clicked() {
                                *action = Some(Action::New(kind.id(), kind.name()));
                                ui.close();
                            }
                        }
                    })
                    .response
                    .on_disabled_hover_text("No module kind fills this slot yet");
                });
            });
        });
        if modules.is_empty() {
            ui.label(RichText::new("No modules yet").small().color(theme::MUTED));
        }
        for m in modules {
            module_row(ui, lib, scene, kit, m, action);
        }
    }
    if lib.presets_dir.is_some() {
        ui.add_space(10.0);
        ui.label(RichText::new("My presets").strong().color(theme::MUTED))
            .on_hover_text("Modules you saved with \"Save as preset\", kept in your settings folder for every scene");
        if lib.presets.is_empty() {
            ui.label(
                RichText::new("None yet. Right-click a module above to save it as a preset.")
                    .small()
                    .color(theme::MUTED),
            );
        }
        for (i, preset) in lib.presets.iter().enumerate() {
            preset_row(ui, kit, i, preset, lib.confirm_delete == Some(i), action);
        }
    }
}

/// One saved preset: its name and slot, and buttons to add it to the scene or delete it.
fn preset_row(ui: &mut Ui, kit: &Kit, index: usize, preset: &Preset, confirming: bool, action: &mut Option<Action>) {
    let module = &preset.module;
    let known = kit.registry.module(&module.kind).is_some();
    // The name gets its own line: the list is narrow and its buttons would cover it.
    ui.horizontal(|ui| {
        ui.label(RichText::new(module.slot.name()).small().color(theme::MUTED));
        if !known {
            ui.label(RichText::new("missing kind").small().color(theme::ERROR))
                .on_hover_text(format!("This version of Ball Studio doesn't know \"{}\"", module.kind));
        }
        ui.add(egui::Label::new(&module.name).truncate()).on_hover_text(preset.path.display().to_string());
    });
    ui.horizontal(|ui| {
        // A preset file can't be undone, so Delete asks once more.
        if confirming {
            let delete = egui::Button::new(RichText::new("Delete for good").color(theme::ERROR)).small();
            if ui.add(delete).on_hover_text("Delete the preset file; this can't be undone").clicked() {
                *action = Some(Action::DeletePreset(index));
            }
            if ui.small_button("Keep").clicked() {
                *action = Some(Action::KeepPreset);
            }
            return;
        }
        if ui
            .add_enabled(known, egui::Button::new("Add to scene").small())
            .on_hover_text("A copy of this preset in the scene's Library")
            .clicked()
        {
            *action = Some(Action::AddPreset(index));
        }
        if ui.small_button("Delete").on_hover_text("Delete the preset file").clicked() {
            *action = Some(Action::AskDeletePreset(index));
        }
    });
    ui.add_space(4.0);
}

/// One module in the list: its draggable name, how many things use it, and a note when its
/// kind is missing.
fn module_row(
    ui: &mut Ui,
    lib: &mut LibraryWindow,
    scene: &Scene,
    kit: &Kit,
    m: &bpe_core::Module,
    action: &mut Option<Action>,
) {
    let id = m.id;
    ui.horizontal(|ui| {
        if lib.renaming == Some(id) {
            let edit_id = ui.make_persistent_id(("rename-module", id));
            if std::mem::take(&mut lib.focus_rename) {
                let mut state = egui::text_edit::TextEditState::default();
                let end = egui::text::CCursor::new(lib.rename_text.chars().count());
                state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), end)));
                state.store(ui.ctx(), edit_id);
                ui.memory_mut(|mem| mem.request_focus(edit_id));
            }
            let label = ui.label(RichText::new("Name").small().color(theme::MUTED));
            let edit = egui::TextEdit::singleline(&mut lib.rename_text).id(edit_id).desired_width(f32::INFINITY);
            let response = ui.add(edit).labelled_by(label.id);
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                lib.renaming = None;
            } else if response.lost_focus() || ui.input(|i| i.key_pressed(Key::Enter)) {
                *action = Some(Action::Rename(id, lib.rename_text.clone()));
            }
            return;
        }
        let users = scene.users_of(id).len();
        let selected = lib.selected == Some(id);
        let button = egui::Button::selectable(selected, &m.name).sense(Sense::click_and_drag());
        let response = ui.add(button).on_hover_text("Drag onto a thing in the tree to use it there");
        response.dnd_set_drag_payload(id);
        if response.double_clicked() {
            *action = Some(Action::StartRename(id));
        } else if response.clicked() {
            *action = Some(Action::Select(id));
        }
        response.context_menu(|ui| {
            if ui.button("Rename").clicked() {
                *action = Some(Action::StartRename(id));
                ui.close();
            }
            if ui.button("Duplicate").clicked() {
                *action = Some(Action::Duplicate(id));
                ui.close();
            }
            if lib.presets_dir.is_some() && ui.button("Save as preset").clicked() {
                *action = Some(Action::SavePreset(id));
                ui.close();
            }
            if ui.add_enabled(users == 0, egui::Button::new("Delete")).clicked() {
                *action = Some(Action::Delete(id));
                ui.close();
            }
        });
        if users > 0 {
            ui.label(RichText::new(format!("×{users}")).small().color(theme::MUTED))
                .on_hover_text(format!("Used by {users} thing{}", if users == 1 { "" } else { "s" }));
        }
        if kit.registry.module(&m.kind).is_none() {
            ui.label(RichText::new("missing kind").small().color(theme::ERROR))
                .on_hover_text(format!("This version of Ball Studio doesn't know \"{}\"", m.kind));
        }
    });
}

/// The right side: the selected module's name, kind, users, actions and settings.
fn details(
    ui: &mut Ui,
    lib: &mut LibraryWindow,
    scene: &Scene,
    kit: &Kit,
    resolved: &Resolved,
    action: &mut Option<Action>,
) {
    let _ = resolved;
    let reg = &kit.registry;
    let Some(m) = lib.selected.and_then(|m| scene.library.modules.get(&m)) else {
        ui.add_space(8.0);
        ui.label(RichText::new("Pick a module on the left to see it here.").color(theme::MUTED));
        return;
    };
    let id = m.id;
    ui.label(RichText::new(&m.name).heading());
    match reg.module(&m.kind) {
        Some(kind) => {
            ui.label(RichText::new(format!("{} · {}", m.slot.name(), kind.name())).color(theme::MUTED))
                .on_hover_text(kind.help());
        }
        None => {
            ui.label(RichText::new(format!("Its kind \"{}\" isn't installed", m.kind)).color(theme::ERROR));
        }
    }
    let users = scene.users_of(id);
    let names: Vec<&str> = users.iter().filter_map(|u| scene.things.get(u).map(|t| t.name.as_str())).collect();
    if names.is_empty() {
        ui.label(RichText::new("Not used yet").color(theme::MUTED));
    } else {
        ui.label(format!("Used by: {}", names.join(", ")));
    }
    ui.horizontal_wrapped(|ui| {
        if ui.add_enabled(!users.is_empty(), egui::Button::new("Select users")).clicked() {
            *action = Some(Action::SelectUsers(id));
        }
        if ui.button("Duplicate").on_hover_text("A copy with the same settings").clicked() {
            *action = Some(Action::Duplicate(id));
        }
        if lib.presets_dir.is_some()
            && ui
                .button("Save as preset")
                .on_hover_text("Keep these settings under My presets, for every scene")
                .clicked()
        {
            *action = Some(Action::SavePreset(id));
        }
        let refused = EditError::InUse(users.clone()).to_string();
        if ui
            .add_enabled(users.is_empty(), egui::Button::new("Delete"))
            .on_disabled_hover_text(format!("{refused} · Select users"))
            .clicked()
        {
            *action = Some(Action::Delete(id));
        }
    });
    ui.separator();
    let Some(kind) = reg.module(&m.kind) else { return };
    ui.horizontal(|ui| {
        ui.label(RichText::new("Settings").strong());
        ui.add_space(8.0);
        crate::inspector::depth_switch(ui, lib.depth.entry(id).or_insert(Depth::Simple), None);
    });
    let depth = lib.depth.get(&id).copied().unwrap_or(Depth::Simple);
    let choices = TargetChoices::from_scene(scene, reg);
    let get = |key: &str| {
        let a = Address { owner: Owner::Module(id), slot: None, key: key.into() };
        scene.value_at(reg, &a).expect("a module's settings all resolve")
    };
    let locked = |key: &str| scene.is_locked(&Address { owner: Owner::Module(id), slot: None, key: key.into() });
    let cx =
        GridCx { choices: &choices, overridden: &|_| false, inherited: &|_| None, marks: &|_| None, locked: &locked };
    let out = settings_grid_marked(ui, ("library", id), kind.settings(), depth, get, &cx);
    if let Some((key, lock)) = out.locks.into_iter().next() {
        *action = Some(Action::Lock(id, key, lock));
    }
    if let Some((key, value)) = out.changes.into_iter().next() {
        *action = Some(Action::Set(id, key, value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{SlotLink, resolve};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };

    struct State {
        lib: LibraryWindow,
        scene: Scene,
        kit: Kit,
        resolved: Resolved,
        selected: Option<Vec<ThingId>>,
    }

    fn harness() -> Harness<'static, State> {
        let kit = bpe_kit::kit();
        let scene = bpe_kit::new_scene(&kit.registry);
        let resolved = resolve(&scene, &kit.registry);
        let lib = LibraryWindow { open: true, presets_dir: None, ..LibraryWindow::default() };
        let state = State { lib, scene, kit, resolved, selected: None };
        let mut h = Harness::builder()
            .with_size(egui::vec2(640.0, 760.0))
            .with_step_dt(1.0 / 60.0)
            .with_max_steps(120)
            .allow_missing_glyphs()
            .with_context_setup(crate::theme::apply)
            .build_ui_state(
                |ui, s: &mut State| {
                    let out = show(ui.ctx(), &mut s.lib, &mut s.scene, &s.kit, &s.resolved);
                    if out.edited {
                        s.resolved = resolve(&s.scene, &s.kit.registry);
                    }
                    if out.select.is_some() {
                        s.selected = out.select;
                    }
                },
                state,
            );
        h.run();
        h
    }

    #[test]
    fn the_library_lists_modules_by_slot_with_their_users() {
        let mut h = harness();
        assert!(h.query_by_label_contains("Plain look").is_some());
        assert!(h.query_by_label_contains("Ball look").is_some());
        h.get_by_label_contains("Ball look").click();
        h.run();
        assert!(h.query_by_label_contains("Used by: Ball").is_some());
        h.get_by_label("Select users").click();
        h.run();
        assert_eq!(h.state().selected, Some(vec![ThingId(2)]));
        h.get_by_label("Duplicate").click();
        h.run();
        let s = h.state();
        let copy = s.scene.library.modules.values().find(|m| m.name == "Ball look copy").expect("a copy");
        assert_eq!(s.lib.selected, Some(copy.id));
        assert!(h.query_by_label("Not used yet").is_some());
    }

    #[test]
    fn delete_is_refused_while_in_use_and_names_the_users() {
        let mut h = harness();
        h.get_by_label_contains("Ball look").click();
        h.run();
        assert!(h.get_by_label("Delete").accesskit_node().is_disabled());
        {
            let s = h.state_mut();
            s.scene.set_slot_inherit(ThingId(2), SlotId::Look);
            s.resolved = resolve(&s.scene, &s.kit.registry);
        }
        h.run();
        h.get_by_label("Delete").click();
        h.run();
        assert_eq!(h.state().scene.library.modules.len(), 4);
        assert_eq!(h.state().lib.selected, None);
    }

    #[test]
    fn new_module_buttons_add_one_per_slot_kind() {
        let mut h = harness();
        h.get_by_label("+ New Look").click();
        h.run();
        h.get_by_label("Classic").click();
        h.run();
        let s = h.state();
        assert_eq!(s.scene.library.modules.len(), 6);
        let new = s.scene.library.modules.values().find(|m| m.name == "Classic").expect("named after its kind");
        assert_eq!(s.lib.selected, Some(new.id));
        assert_eq!(new.slot, SlotId::Look);
    }

    #[test]
    fn double_clicking_a_module_renames_it() {
        let mut h = harness();
        h.get_by_label_contains("Plain look").click();
        h.step();
        h.get_by_label_contains("Plain look").click();
        h.run();
        assert_eq!(h.state().lib.renaming, Some(ModuleId(0)));
        h.event(egui::Event::Text("Soft look".into()));
        h.key_press(Key::Enter);
        h.run();
        assert_eq!(h.state().scene.library.modules[&ModuleId(0)].name, "Soft look");
        assert_eq!(h.state().lib.renaming, None);
        // The scene's defaults still link it.
        assert!(
            matches!(h.state().scene.things[&ThingId::ROOT].slots[&SlotId::Look], SlotLink::Linked { module, .. } if module == ModuleId(0))
        );
    }

    #[test]
    fn a_closed_window_does_nothing() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_scene(&kit.registry);
        let resolved = resolve(&scene, &kit.registry);
        let mut lib = LibraryWindow::default();
        let ctx = egui::Context::default();
        let out = show(&ctx, &mut lib, &mut scene, &kit, &resolved);
        assert!(!out.edited && out.select.is_none());
    }

    #[test]
    fn the_library_shows_my_presets_and_adds_one_to_the_scene() {
        let dir = std::env::temp_dir().join(format!("bpe-library-presets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_scene(&kit.registry);
        let id = scene.library.modules.values().find(|m| m.name == "Ball look").expect("the ball look").id;
        scene.rename_module(id, "Neon").unwrap();
        crate::presets::save_in(&dir, &scene.library.modules[&id]).unwrap();
        let mut h = harness();
        h.state_mut().lib.presets_dir = Some(dir.clone());
        h.state_mut().lib.look_again();
        h.run();
        assert!(h.query_by_label("My presets").is_some());
        assert!(h.query_by_label("Neon").is_some());
        let before = h.state().scene.library.modules.len();
        // The presets sit under every slot, so the list scrolls down to them.
        h.get_by_label("Add to scene").scroll_to_me();
        h.run();
        h.get_by_label("Add to scene").click();
        h.run();
        let s = h.state();
        assert_eq!(s.scene.library.modules.len(), before + 1);
        let added = s.scene.library.modules.values().find(|m| m.name == "Neon").expect("added from the preset");
        assert_eq!(s.lib.selected, Some(added.id));
        // Back up the list, which scrolled down to the presets.
        h.get_by_label_contains("Plain look").scroll_to_me();
        h.run();
        h.get_by_label_contains("Plain look").click();
        h.run();
        h.get_by_label("Save as preset").click();
        h.run();
        let listed = crate::presets::list_in(&dir);
        assert_eq!(listed.len(), 2);
        assert!(listed.iter().any(|p| p.module.name == "Plain look"));
        assert_eq!(h.query_all_by_label("Add to scene").count(), 2, "the list is read again after saving");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_preset_name_never_runs_under_its_buttons() {
        let dir = std::env::temp_dir().join(format!("bpe-library-preset-name-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let kit = bpe_kit::kit();
        let scene = bpe_kit::new_scene(&kit.registry);
        let mut module = scene.library.modules.values().find(|m| m.name == "Ball look").expect("the ball look").clone();
        module.name = "Long glowing neon look".into();
        crate::presets::save_in(&dir, &module).unwrap();
        let mut h = harness();
        h.state_mut().lib.presets_dir = Some(dir.clone());
        h.state_mut().lib.look_again();
        h.run();
        h.get_by_label("Add to scene").scroll_to_me();
        h.run();
        let name = h.get_by_label("Long glowing neon look").rect();
        for button in ["Add to scene", "Delete"] {
            let button = h.get_by_label(button).rect();
            assert!(!name.intersects(button), "the name {name:?} runs under {button:?}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn deleting_a_preset_asks_first() {
        let dir = std::env::temp_dir().join(format!("bpe-library-preset-delete-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let kit = bpe_kit::kit();
        let scene = bpe_kit::new_scene(&kit.registry);
        let module = scene.library.modules.values().find(|m| m.name == "Ball look").unwrap().clone();
        let file = crate::presets::save_in(&dir, &module).unwrap();
        let mut h = harness();
        h.state_mut().lib.presets_dir = Some(dir.clone());
        h.state_mut().lib.look_again();
        h.run();
        h.get_by_label("Delete").scroll_to_me();
        h.run();
        h.get_by_label("Delete").click();
        h.run();
        assert!(file.is_file(), "one click does not delete the file");
        h.get_by_label("Keep").click();
        h.run();
        assert!(file.is_file());
        assert!(h.query_by_label("Delete for good").is_none());
        h.get_by_label("Delete").click();
        h.run();
        h.get_by_label("Delete for good").click();
        h.run();
        assert!(!file.exists(), "the second click deletes it");
        assert!(h.query_by_label("Add to scene").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}

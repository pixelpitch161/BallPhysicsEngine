//! The inspector: the selected thing's header, how it hangs in the tree, its kind's settings and
//! one card per slot. A shared module's card edits this thing's own overrides; an inherited card
//! is read-only until the thing takes its own link.

use crate::selection::Selection;
use crate::theme;
use crate::widgets::{GridCx, TargetChoices, settings_grid, settings_grid_marked};
use bpe_core::{
    Address, Attach, Depth, ModuleId, ModuleKind, Owner, Resolved, Scene, SlotId, SlotLink, ThingId, Transform, Value,
};
use bpe_kit::Kit;
use eframe::egui::{self, DragValue, RichText, Ui};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub struct Inspector {
    /// The depth each card is open at: the kind's settings (`None`) and each slot.
    pub depth: BTreeMap<(ThingId, Option<SlotId>), Depth>,
    /// Set when "Open in Library" was clicked; the app opens the Library window on it.
    pub open_library: Option<ModuleId>,
}

/// A change the panel asks for, applied once the drawing is done.
enum Edit {
    Name(String),
    Tags(BTreeSet<String>),
    Enabled(bool),
    Transform(Transform),
    Parent(ThingId),
    Attach(Attach),
    /// One of the kind's own settings, for every selected thing of the kind.
    SetKind(&'static str, Value),
    /// One of a slot's settings, for every selected thing whose slot holds the same module kind.
    SetSlot(SlotId, &'static str, Value),
    ResetOverride(SlotId, &'static str),
    PushOverride(SlotId, &'static str),
    Link(SlotId, ModuleId),
    NewModule {
        slot: SlotId,
        kind: &'static str,
        copy_from: Option<ModuleId>,
    },
    MakeUnique(SlotId),
    PushAll(SlotId),
    ResetAll(SlotId),
    Inherit(SlotId),
    Off(SlotId),
    AddSlot(SlotId),
}

/// Where an edit to a slot's settings goes for one thing.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Write {
    /// The thing is the module's only user with no changes of its own: edit the module.
    Module(ModuleId),
    /// The module is shared, or the thing already has changes: edit this thing's overrides.
    Override(ThingId, SlotId),
    /// The slot comes from a thing above: read-only until the thing links the module itself.
    Inherited { from: ThingId, module: ModuleId },
}

/// The write policy: where this thing's edits to `slot` land, or `None` when the slot is off or
/// nothing fills it.
pub(crate) fn write_target(scene: &Scene, resolved: &Resolved, id: ThingId, slot: SlotId) -> Option<Write> {
    let thing = scene.things.get(&id)?;
    match thing.slots.get(&slot)? {
        SlotLink::Off => None,
        SlotLink::Linked { module, overrides } => {
            if overrides.is_empty() && scene.users_of(*module).len() <= 1 {
                Some(Write::Module(*module))
            } else {
                Some(Write::Override(id, slot))
            }
        }
        SlotLink::Inherit => {
            let r = resolved.things.get(&id)?.slots.get(&slot)?;
            Some(Write::Inherited { from: r.from, module: r.module })
        }
    }
}

/// Draws the inspector for the selection's primary thing and applies its edits, to every
/// selected thing where that makes sense. True when the scene changed.
pub fn show(
    ui: &mut Ui,
    inspector: &mut Inspector,
    scene: &mut Scene,
    kit: &Kit,
    resolved: &Resolved,
    selection: &Selection,
) -> bool {
    let reg = &kit.registry;
    let Some((id, thing)) = selection.primary.and_then(|id| scene.things.get(&id).map(|t| (id, t.clone()))) else {
        ui.add_space(8.0);
        ui.label(RichText::new("Select something in the tree to see its settings.").color(theme::MUTED));
        return false;
    };
    let Some(kind) = reg.thing(&thing.kind) else {
        ui.label(RichText::new(format!("Its kind \"{}\" isn't installed.", thing.kind)).color(theme::WARNING));
        return false;
    };
    let selected = selection.all(scene);
    let choices = TargetChoices::from_scene(scene, reg);
    let mut edits = Vec::new();

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        if selected.len() > 1 {
            ui.label(RichText::new(format!("Editing {} things", selected.len())).color(theme::MUTED).small())
                .on_hover_text("Settings changed here apply to every selected thing that has them.");
        }
        // ---------- header ----------
        egui::Grid::new(("header", id)).num_columns(2).spacing([12.0, 6.0]).min_col_width(96.0).show(ui, |ui| {
            let name_label = ui.label("Name");
            let mut name = thing.name.clone();
            let edit = egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY);
            if ui.add(edit).labelled_by(name_label.id).changed() {
                edits.push(Edit::Name(name));
            }
            ui.end_row();
            ui.label("Kind");
            ui.label(RichText::new(kind.name()).color(theme::MUTED)).on_hover_text(kind.help());
            ui.end_row();
            if id != ThingId::ROOT {
                tree_rows(ui, scene, &thing, &mut edits);
                let shown = ui.label("Shown").on_hover_text("A hidden thing is left out of the video and the physics");
                let mut enabled = thing.enabled;
                if ui.checkbox(&mut enabled, "").labelled_by(shown.id).changed() {
                    edits.push(Edit::Enabled(enabled));
                }
                ui.end_row();
            }
            let tags_label = ui
                .label("Tags")
                .on_hover_text("Words that rules and other things can pick this out by, separated by spaces");
            let mut tags = thing.tags.iter().cloned().collect::<Vec<_>>().join(" ");
            let tag_edit = ui
                .add(egui::TextEdit::singleline(&mut tags).desired_width(f32::INFINITY).hint_text("none"))
                .labelled_by(tags_label.id);
            if tag_edit.lost_focus() {
                let set: BTreeSet<String> = tags.split_whitespace().map(str::to_owned).collect();
                if set != thing.tags {
                    edits.push(Edit::Tags(set));
                }
            }
            ui.end_row();
        });
        ui.add_space(6.0);

        // ---------- the kind's own settings ----------
        card(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("{} settings", kind.name())).strong());
                ui.add_space(8.0);
                depth_switch(ui, inspector.depth.entry((id, None)).or_insert(Depth::Simple));
            });
            let depth = inspector.depth.get(&(id, None)).copied().unwrap_or(Depth::Simple);
            let get = |key: &str| {
                let a = Address { owner: Owner::Thing(id), slot: None, key: key.into() };
                scene.value_at(reg, &a).expect("the kind's own settings all resolve")
            };
            let cx = GridCx { choices: &choices, overridden: &|_| false, inherited: &|_| None };
            for (key, value) in settings_grid_marked(ui, ("kind", id), kind.settings(), depth, get, &cx).changes {
                edits.push(Edit::SetKind(key, value));
            }
        });

        // ---------- one card per slot ----------
        let rthing = resolved.things.get(&id);
        for slot in SlotId::ALL {
            let Some(link) = thing.slots.get(&slot) else { continue };
            let rslot = rthing.and_then(|r| r.slots.get(&slot));
            ui.add_space(6.0);
            card(ui, |ui| {
                let state = match link {
                    SlotLink::Linked { module, .. } => match scene.library.modules.get(module) {
                        Some(m) => {
                            let users = scene.users_of(*module).len();
                            if users > 1 { format!("{} · shared ×{users}", m.name) } else { m.name.clone() }
                        }
                        None => format!("Missing module (id {})", module.0),
                    },
                    SlotLink::Inherit => match rslot {
                        Some(r) => format!("from {}", scene.things.get(&r.from).map_or("?", |t| t.name.as_str())),
                        None => "Inherit (nothing above)".to_owned(),
                    },
                    SlotLink::Off => "Off".to_owned(),
                };
                let shown_module = match link {
                    SlotLink::Linked { module, .. } => Some(*module),
                    SlotLink::Inherit => rslot.map(|r| r.module),
                    SlotLink::Off => None,
                };
                let overrides = match link {
                    SlotLink::Linked { overrides, .. } => Some(overrides),
                    _ => None,
                };
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(slot.name()).strong());
                    // The picker: a button that reads the slot's state and drops down the choices.
                    ui.menu_button(state, |ui| {
                        for m in scene.library.modules.values().filter(|m| m.slot == slot) {
                            let current = matches!(link, SlotLink::Linked { module, .. } if *module == m.id);
                            if ui.selectable_label(current, &m.name).clicked() {
                                edits.push(Edit::Link(slot, m.id));
                                ui.close();
                            }
                        }
                        ui.separator();
                        // One kind: a plain button. Several: a submenu naming each kind. A new
                        // module copies the shown one's settings when it is of the same kind.
                        let kinds: Vec<&dyn ModuleKind> = reg.modules_for(slot).collect();
                        let shown_kind =
                            shown_module.and_then(|m| scene.library.modules.get(&m)).map(|m| m.kind.as_str());
                        let copy_for = |kind: &str| shown_module.filter(|_| shown_kind == Some(kind));
                        if let [only] = kinds[..] {
                            if ui.button(format!("New {} module…", slot.name())).clicked() {
                                edits.push(Edit::NewModule { slot, kind: only.id(), copy_from: copy_for(only.id()) });
                                ui.close();
                            }
                        } else if !kinds.is_empty() {
                            ui.menu_button(format!("New {} module", slot.name()), |ui| {
                                ui.set_min_width(180.0);
                                for kind in &kinds {
                                    if ui.button(kind.name()).on_hover_text(kind.help()).clicked() {
                                        edits.push(Edit::NewModule {
                                            slot,
                                            kind: kind.id(),
                                            copy_from: copy_for(kind.id()),
                                        });
                                        ui.close();
                                    }
                                }
                            });
                        }
                        if let SlotLink::Linked { .. } = link {
                            if ui
                                .button("Make unique")
                                .on_hover_text("This thing gets its own copy of the module, with its changes baked in")
                                .clicked()
                            {
                                edits.push(Edit::MakeUnique(slot));
                                ui.close();
                            }
                            if overrides.is_some_and(|o| !o.is_empty()) {
                                if ui
                                    .button("Push all to module")
                                    .on_hover_text("Every thing using the module gets this thing's changes")
                                    .clicked()
                                {
                                    edits.push(Edit::PushAll(slot));
                                    ui.close();
                                }
                                if ui
                                    .button("Reset all")
                                    .on_hover_text("Drop this thing's changes and follow the module")
                                    .clicked()
                                {
                                    edits.push(Edit::ResetAll(slot));
                                    ui.close();
                                }
                            }
                        }
                        if let Some(m) = shown_module
                            && ui.button("Open in Library").clicked()
                        {
                            inspector.open_library = Some(m);
                            ui.close();
                        }
                        ui.separator();
                        if ui.selectable_label(matches!(link, SlotLink::Inherit), "Inherit").clicked() {
                            edits.push(Edit::Inherit(slot));
                            ui.close();
                        }
                        if ui.selectable_label(matches!(link, SlotLink::Off), "Off").clicked() {
                            edits.push(Edit::Off(slot));
                            ui.close();
                        }
                    })
                    .response
                    .on_hover_text("Which module fills this slot");
                    ui.add_space(8.0);
                    depth_switch(ui, inspector.depth.entry((id, Some(slot))).or_insert(Depth::Simple));
                });
                let problems: Vec<String> = resolved
                    .problems
                    .iter()
                    .filter(|p| p.thing == id && p.slot == Some(slot))
                    .map(crate::tree::problem_words)
                    .collect();
                if !problems.is_empty() {
                    ui.label(RichText::new(problems.join("\n")).small().color(theme::ERROR));
                }
                let Some(module) = shown_module else { return };
                let (Some(m), Some(write)) =
                    (scene.library.modules.get(&module), write_target(scene, resolved, id, slot))
                else {
                    return;
                };
                let Some(mkind) = reg.module(&m.kind) else { return };
                let depth = inspector.depth.get(&(id, Some(slot))).copied().unwrap_or(Depth::Simple);
                ui.add_space(4.0);
                match write {
                    Write::Module(module) => {
                        let get = |key: &str| {
                            let a = Address { owner: Owner::Module(module), slot: None, key: key.into() };
                            scene.value_at(reg, &a).expect("a module's settings all resolve")
                        };
                        let cx = GridCx { choices: &choices, overridden: &|_| false, inherited: &|_| None };
                        let out = settings_grid_marked(ui, ("slot", id, slot), mkind.settings(), depth, get, &cx);
                        edits.extend(out.changes.into_iter().map(|(key, value)| Edit::SetSlot(slot, key, value)));
                    }
                    Write::Override(..) => {
                        let get = |key: &str| {
                            let a = Address { owner: Owner::Thing(id), slot: Some(slot), key: key.into() };
                            scene.value_at(reg, &a).expect("a linked slot's settings all resolve")
                        };
                        let overridden = |key: &str| overrides.is_some_and(|o| o.get(key).is_some());
                        let inherited = |key: &str| {
                            let a = Address { owner: Owner::Module(module), slot: None, key: key.into() };
                            scene.value_at(reg, &a)
                        };
                        let cx = GridCx { choices: &choices, overridden: &overridden, inherited: &inherited };
                        let out = settings_grid_marked(ui, ("slot", id, slot), mkind.settings(), depth, get, &cx);
                        edits.extend(out.changes.into_iter().map(|(key, value)| Edit::SetSlot(slot, key, value)));
                        edits.extend(out.resets.into_iter().map(|key| Edit::ResetOverride(slot, key)));
                        edits.extend(out.pushes.into_iter().map(|key| Edit::PushOverride(slot, key)));
                    }
                    Write::Inherited { module, .. } => {
                        if ui
                            .button("Change here")
                            .on_hover_text("Link the module to this thing, so changes here are its own")
                            .clicked()
                        {
                            edits.push(Edit::Link(slot, module));
                        }
                        let get = |key: &str| {
                            rslot.and_then(|r| r.params.get(key).cloned()).unwrap_or_else(|| {
                                let a = Address { owner: Owner::Module(module), slot: None, key: key.into() };
                                scene.value_at(reg, &a).expect("a module's settings all resolve")
                            })
                        };
                        ui.add_enabled_ui(false, |ui| {
                            settings_grid(ui, ("slot", id, slot), mkind.settings(), depth, get);
                        });
                    }
                }
            });
        }

        // ---------- + Add slot ----------
        ui.add_space(8.0);
        let missing: Vec<SlotId> = kind
            .slots()
            .iter()
            .copied()
            .filter(|s| !thing.slots.contains_key(s) && reg.modules_for(*s).next().is_some())
            .collect();
        ui.add_enabled_ui(!missing.is_empty(), |ui| {
            ui.menu_button("+ Add slot", |ui| {
                for slot in &missing {
                    if ui.button(slot.name()).clicked() {
                        edits.push(Edit::AddSlot(*slot));
                        ui.close();
                    }
                }
            })
            .response
            .on_disabled_hover_text("Every slot this kind can use is already shown");
        });
    });

    let mut changed = false;
    for edit in edits {
        let ok = match edit {
            Edit::Name(name) => set_thing(scene, id, |t| t.name = name),
            Edit::Tags(tags) => set_thing(scene, id, |t| t.tags = tags),
            Edit::Enabled(enabled) => set_thing(scene, id, |t| t.enabled = enabled),
            Edit::Transform(t) => set_thing(scene, id, |thing| thing.transform = t),
            Edit::Parent(parent) => {
                let attach = scene.things.get(&id).map(|t| t.attach).unwrap_or_default();
                crate::tree::apply_drop(scene, id, parent, crate::tree::Drop::Into(attach)).is_ok()
            }
            Edit::Attach(attach) => crate::tree::reattach(scene, id, attach).is_ok(),
            Edit::SetKind(key, value) => {
                let mut any = false;
                let same_kind: Vec<ThingId> = selected
                    .iter()
                    .copied()
                    .filter(|t| scene.things.get(t).is_some_and(|t| t.kind == thing.kind))
                    .collect();
                for t in &same_kind {
                    let a = Address { owner: Owner::Thing(*t), slot: None, key: key.into() };
                    any |= scene.set_at(reg, &a, value.clone()).is_ok();
                }
                any
            }
            Edit::SetSlot(slot, key, value) => {
                let mkind = resolved.things.get(&id).and_then(|r| r.slots.get(&slot)).map(|r| r.kind.clone());
                let mut any = false;
                for t in &selected {
                    let same =
                        resolved.things.get(t).and_then(|r| r.slots.get(&slot)).map(|r| &r.kind) == mkind.as_ref();
                    if !same {
                        continue;
                    }
                    any |= write_slot(scene, resolved, reg, *t, slot, key, value.clone());
                }
                any
            }
            Edit::ResetOverride(slot, key) => scene.reset_override(id, slot, key).is_ok(),
            Edit::PushOverride(slot, key) => {
                let value = match scene.things.get(&id).and_then(|t| t.slots.get(&slot)) {
                    Some(SlotLink::Linked { module, overrides }) => overrides.get(key).cloned().map(|v| (*module, v)),
                    _ => None,
                };
                match value {
                    Some((module, v)) => {
                        let a = Address { owner: Owner::Module(module), slot: None, key: key.into() };
                        scene.set_at(reg, &a, v).is_ok() && scene.reset_override(id, slot, key).is_ok()
                    }
                    None => false,
                }
            }
            Edit::Link(slot, module) => scene.link(reg, id, slot, module).is_ok(),
            Edit::NewModule { slot, kind, copy_from } => {
                let name = format!("{} {}", thing.name, slot.name());
                match scene.add_module(reg, kind, &name) {
                    Ok(module) => {
                        if let Some(params) =
                            copy_from.and_then(|c| scene.library.modules.get(&c)).map(|m| m.params.clone())
                            && let Some(m) = scene.library.modules.get_mut(&module)
                        {
                            m.params = params;
                        }
                        scene.link(reg, id, slot, module).is_ok()
                    }
                    Err(_) => false,
                }
            }
            Edit::MakeUnique(slot) => scene.make_unique(id, slot).is_ok(),
            Edit::PushAll(slot) => scene.push_to_module(id, slot).is_ok(),
            Edit::ResetAll(slot) => {
                let keys: Vec<String> = match scene.things.get(&id).and_then(|t| t.slots.get(&slot)) {
                    Some(SlotLink::Linked { overrides, .. }) => overrides.iter().map(|(k, _)| k.clone()).collect(),
                    _ => Vec::new(),
                };
                let mut any = false;
                for key in keys {
                    any |= scene.reset_override(id, slot, &key).is_ok();
                }
                any
            }
            Edit::Inherit(slot) => {
                scene.set_slot_inherit(id, slot);
                true
            }
            Edit::Off(slot) => {
                scene.set_slot_off(id, slot);
                true
            }
            Edit::AddSlot(slot) => scene.add_slot(reg, id, slot).is_ok(),
        };
        changed |= ok;
    }
    changed
}

/// Writes one slot setting for one thing, where its write policy says: the module, the thing's
/// own override, or, for an inherited slot, a fresh link to the module and then an override.
fn write_slot(
    scene: &mut Scene,
    resolved: &Resolved,
    reg: &bpe_core::Registry,
    id: ThingId,
    slot: SlotId,
    key: &str,
    value: Value,
) -> bool {
    match write_target(scene, resolved, id, slot) {
        Some(Write::Module(module)) => {
            let a = Address { owner: Owner::Module(module), slot: None, key: key.into() };
            scene.set_at(reg, &a, value).is_ok()
        }
        Some(Write::Override(..)) => {
            let a = Address { owner: Owner::Thing(id), slot: Some(slot), key: key.into() };
            scene.set_at(reg, &a, value).is_ok()
        }
        Some(Write::Inherited { module, .. }) => {
            if scene.link(reg, id, slot, module).is_err() {
                return false;
            }
            let a = Address { owner: Owner::Thing(id), slot: Some(slot), key: key.into() };
            scene.set_at(reg, &a, value).is_ok()
        }
        None => false,
    }
}

/// The header rows about the thing's place in the tree: its parent, how it hangs off it, and
/// where it sits, as a position of its own or an offset along for the ride.
fn tree_rows(ui: &mut Ui, scene: &Scene, thing: &bpe_core::Thing, edits: &mut Vec<Edit>) {
    let id = thing.id;
    let parent = thing.parent.unwrap_or(ThingId::ROOT);
    let parent_name = scene.things.get(&parent).map(|t| t.name.clone()).unwrap_or_default();
    ui.label("Parent").on_hover_text("The thing this one sits under in the tree");
    let under = scene.descendants(id);
    ui.menu_button(parent_name, |ui| {
        for other in scene.order.iter().filter(|o| **o != id && !under.contains(o)) {
            let Some(name) = scene.things.get(other).map(|t| t.name.as_str()) else { continue };
            if ui.selectable_label(*other == parent, name).clicked() {
                edits.push(Edit::Parent(*other));
                ui.close();
            }
        }
    })
    .response
    .on_hover_text("Move it under another thing");
    ui.end_row();

    let rides = thing.rides();
    ui.label("Attach").on_hover_text("Grouped things keep their own place; a rider follows its parent");
    ui.horizontal(|ui| {
        if ui.selectable_label(!rides, "Grouped").clicked() && rides {
            edits.push(Edit::Attach(Attach::Group));
        }
        let ride = ui.add_enabled(parent != ThingId::ROOT, egui::Button::selectable(rides, "Rides on it"));
        if ride.on_disabled_hover_text("Scene defaults don't move, so there is nothing to ride").clicked() && !rides {
            edits.push(Edit::Attach(Attach::Ride { offset: glam::Vec2::ZERO, turn_with_parent: true, item: None }));
        }
    });
    ui.end_row();

    let mut t = thing.transform;
    match thing.attach {
        Attach::Ride { offset, turn_with_parent, item } => {
            let mut o = offset;
            let label = ui.label("Offset").on_hover_text("Where it sits from its parent, in video pixels");
            let moved = ui
                .horizontal(|ui| {
                    let x = ui.add(DragValue::new(&mut o.x).speed(1.0).prefix("x ").suffix(" px"));
                    let y = ui.add(DragValue::new(&mut o.y).speed(1.0).prefix("y ").suffix(" px"));
                    x.labelled_by(label.id).changed() || y.labelled_by(label.id).changed()
                })
                .inner;
            ui.end_row();
            let label = ui.label("Turns with it").on_hover_text("Swing around as the parent turns");
            let mut turns = turn_with_parent;
            let turned = ui.checkbox(&mut turns, "").labelled_by(label.id).changed();
            ui.end_row();
            let mut picked = item;
            let mut item_changed = false;
            if thing.parent.and_then(|p| scene.things.get(&p)).is_some_and(|p| p.kind == "balls") {
                let label = ui.label("Item").on_hover_text("Ride one ball of the parent: its number, or -1 for all");
                let mut n: i64 = item.map_or(-1, |i| i as i64);
                if ui.add(DragValue::new(&mut n).range(-1..=9999)).labelled_by(label.id).changed() {
                    picked = (n >= 0).then_some(n as u32);
                    item_changed = true;
                }
                ui.end_row();
            }
            if moved || turned || item_changed {
                edits.push(Edit::Attach(Attach::Ride { offset: o, turn_with_parent: turns, item: picked }));
            }
        }
        Attach::Group => {
            let position = ui.label("Position").on_hover_text("Where it sits, in video pixels from the centre");
            let moved = ui
                .horizontal(|ui| {
                    let x = ui.add(DragValue::new(&mut t.position.x).speed(1.0).prefix("x ").suffix(" px"));
                    let y = ui.add(DragValue::new(&mut t.position.y).speed(1.0).prefix("y ").suffix(" px"));
                    x.labelled_by(position.id).changed() || y.labelled_by(position.id).changed()
                })
                .inner;
            ui.end_row();
            if moved {
                edits.push(Edit::Transform(t));
            }
        }
    }
    let rotation = ui.label("Rotation").on_hover_text("How far it is turned, clockwise");
    let mut degrees = t.rotation.to_degrees();
    let turned = ui.add(DragValue::new(&mut degrees).speed(1.0).suffix("°")).labelled_by(rotation.id).changed();
    if turned {
        t.rotation = degrees.to_radians();
        edits.push(Edit::Transform(t));
    }
    ui.end_row();
}

fn set_thing(scene: &mut Scene, id: ThingId, f: impl FnOnce(&mut bpe_core::Thing)) -> bool {
    match scene.things.get_mut(&id) {
        Some(t) => {
            f(t);
            true
        }
        None => false,
    }
}

/// Simple | More | Nodes, the last one waiting for a later stage.
pub(crate) fn depth_switch(ui: &mut Ui, depth: &mut Depth) {
    ui.selectable_value(depth, Depth::Simple, "Simple");
    ui.selectable_value(depth, Depth::More, "More");
    ui.add_enabled(false, egui::Button::selectable(false, "Nodes"))
        .on_disabled_hover_text("Nodes come in a later stage");
}

fn card(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) {
    egui::Frame::new().fill(theme::CARD).corner_radius(8).inner_margin(10).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add_contents(ui);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Resolved, SlotId, SlotLink, Value, resolve};
    use eframe::egui::{self, Modifiers, PointerButton, accesskit::Role};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };
    use glam::Vec2;

    struct State {
        inspector: Inspector,
        scene: Scene,
        kit: Kit,
        resolved: Resolved,
        selection: Selection,
    }

    fn harness(selected: Option<ThingId>) -> Harness<'static, State> {
        let kit = bpe_kit::kit();
        let scene = bpe_kit::new_scene(&kit.registry);
        let resolved = resolve(&scene, &kit.registry);
        let mut selection = Selection::default();
        if let Some(id) = selected {
            selection.set(id);
        }
        let state = State { inspector: Inspector::default(), scene, kit, resolved, selection };
        let mut h = Harness::builder()
            .with_size(egui::vec2(420.0, 1600.0))
            .with_step_dt(1.0 / 60.0)
            .with_max_steps(120)
            .allow_missing_glyphs()
            .with_context_setup(crate::theme::apply)
            .build_ui_state(
                |ui, s: &mut State| {
                    if show(ui, &mut s.inspector, &mut s.scene, &s.kit, &s.resolved, &s.selection) {
                        s.resolved = resolve(&s.scene, &s.kit.registry);
                    }
                },
                state,
            );
        h.run();
        h
    }

    /// The slider a setting's name labels.
    fn slider_rect(h: &Harness<'_, State>, label: &str) -> egui::Rect {
        h.get_by_role_and_label(Role::Slider, label).rect()
    }

    /// Presses on the slider and lets go at `fraction` of its width.
    fn drag_slider(h: &mut Harness<'_, State>, rect: egui::Rect, fraction: f32) {
        let start = rect.center();
        let end = egui::pos2(rect.left() + rect.width() * fraction, rect.center().y);
        h.hover_at(start);
        h.event(egui::Event::PointerButton {
            pos: start,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        h.step();
        h.event(egui::Event::PointerMoved(end));
        h.step();
        h.event(egui::Event::PointerButton {
            pos: end,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
        h.run();
    }

    fn ball_look(s: &State) -> (ModuleId, bpe_core::Params) {
        match &s.scene.things[&ThingId(2)].slots[&SlotId::Look] {
            SlotLink::Linked { module, overrides } => (*module, overrides.clone()),
            other => panic!("the ball's look is linked: {other:?}"),
        }
    }

    #[test]
    fn more_shows_more_rows_than_simple() {
        let mut h = harness(Some(ThingId(2)));
        let simple = h.get_all_by_role(Role::Slider).count();
        assert!(simple > 0);
        // Every card has its own switch; the first belongs to the kind's settings.
        h.get_all_by_label("More").next().expect("a More switch").click();
        h.run();
        assert!(h.get_all_by_role(Role::Slider).count() > simple);
        assert_eq!(h.state().inspector.depth.get(&(ThingId(2), None)), Some(&Depth::More));
    }

    #[test]
    fn editing_a_slider_writes_through_set_at() {
        let mut h = harness(Some(ThingId(2)));
        let rect = slider_rect(&h, "Size");
        drag_slider(&mut h, rect, 0.9);
        let s = h.state();
        let v = s.scene.things[&ThingId(2)].params.get("radius").cloned();
        assert!(matches!(v, Some(Value::Number(r)) if r > 100.0), "{v:?}");
    }

    #[test]
    fn the_module_picker_can_switch_a_slot_off_and_back_to_inherit() {
        let mut h = harness(Some(ThingId(2)));
        h.get_by_label("Ball look").click();
        h.run();
        h.get_by_label("Off").click();
        h.run();
        assert_eq!(h.state().scene.things[&ThingId(2)].slots[&SlotId::Look], SlotLink::Off);
        h.get_by_label("Off").click();
        h.run();
        h.get_by_label("Inherit").click();
        h.run();
        assert_eq!(h.state().scene.things[&ThingId(2)].slots[&SlotId::Look], SlotLink::Inherit);
        // Look, Motion, Bounce and Sound now all come from the scene's defaults.
        assert_eq!(h.get_all_by_label_contains("from Scene defaults").count(), 4);
    }

    #[test]
    fn a_new_module_copies_the_current_one() {
        let mut h = harness(Some(ThingId(1)));
        // The arena's first card is its Look, inherited from the scene's defaults.
        h.get_all_by_label_contains("from Scene defaults").next().expect("an inherited slot").click();
        h.run();
        h.get_by_label("New Look module").click();
        h.run();
        h.get_by_label("Classic").click();
        h.run();
        let s = h.state();
        let SlotLink::Linked { module, .. } = &s.scene.things[&ThingId(1)].slots[&SlotId::Look] else { panic!() };
        assert_eq!(s.scene.library.modules[module].name, "Arena Look");
        assert_eq!(s.scene.library.modules[module].kind, "look.classic");
        assert_eq!(s.scene.library.modules[module].params, s.scene.library.modules[&bpe_core::ModuleId(0)].params);
    }

    #[test]
    fn a_new_module_is_picked_by_kind_when_a_slot_has_several() {
        let mut h = harness(Some(ThingId(2)));
        h.get_by_label("+ Add slot").click();
        h.run();
        h.get_by_label("Particles").click();
        h.run();
        h.get_by_label("Inherit (nothing above)").click();
        h.run();
        // Particles has one kind: a plain button that makes it straight away.
        h.get_by_label("New Particles module…").click();
        h.run();
        let s = h.state();
        let SlotLink::Linked { module, .. } = &s.scene.things[&ThingId(2)].slots[&SlotId::Particles] else { panic!() };
        assert_eq!(s.scene.library.modules[module].kind, "particles.burst");
        assert_eq!(s.scene.library.modules[module].name, "Ball Particles");
        // The ball's Look has two kinds: the submenu names them, and a different kind starts
        // from its own defaults rather than copying the classic look's settings.
        h.get_by_label("Ball look").click();
        h.run();
        h.get_by_label("New Look module").click();
        h.run();
        h.get_by_label("Styled").click();
        h.run();
        let s = h.state();
        let SlotLink::Linked { module, .. } = &s.scene.things[&ThingId(2)].slots[&SlotId::Look] else { panic!() };
        let m = &s.scene.library.modules[module];
        assert_eq!(m.kind, "look.styled");
        assert!(m.params.get("color").is_none(), "{:?}", m.params);
    }

    #[test]
    fn add_slot_lists_only_slots_the_kind_lacks_and_adds_them_inheriting() {
        let mut h = harness(Some(ThingId(2)));
        // Shader has module kinds and isn't shown on a new ball, so there is something to add.
        assert!(!h.get_by_label("+ Add slot").accesskit_node().is_disabled());
        h.state_mut().scene.things.get_mut(&ThingId(2)).unwrap().slots.remove(&SlotId::Motion);
        h.run();
        h.get_by_label("+ Add slot").click();
        h.run();
        assert!(h.query_by_label("Trail").is_some());
        assert!(h.query_by_label("Shader").is_some());
        h.get_by_label("Motion").click();
        h.run();
        assert_eq!(h.state().scene.things[&ThingId(2)].slots.get(&SlotId::Motion), Some(&SlotLink::Inherit));
    }

    #[test]
    fn the_write_policy_picks_module_override_or_inherited() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_scene(&kit.registry);
        let resolved = resolve(&scene, &kit.registry);
        let look = match &scene.things[&ThingId(2)].slots[&SlotId::Look] {
            SlotLink::Linked { module, .. } => *module,
            _ => unreachable!(),
        };
        assert_eq!(write_target(&scene, &resolved, ThingId(2), SlotId::Look), Some(Write::Module(look)));
        assert_eq!(
            write_target(&scene, &resolved, ThingId(2), SlotId::Motion),
            Some(Write::Inherited { from: ThingId::ROOT, module: ModuleId(1) })
        );
        scene.link(&kit.registry, ThingId(1), SlotId::Look, look).unwrap();
        assert_eq!(
            write_target(&scene, &resolved, ThingId(2), SlotId::Look),
            Some(Write::Override(ThingId(2), SlotId::Look))
        );
        // Alone again but with a change of its own, the thing keeps editing its overrides.
        scene.set_slot_inherit(ThingId(1), SlotId::Look);
        scene.set_override(&kit.registry, ThingId(2), SlotId::Look, "glow", Value::Number(1.5)).unwrap();
        assert_eq!(
            write_target(&scene, &resolved, ThingId(2), SlotId::Look),
            Some(Write::Override(ThingId(2), SlotId::Look))
        );
        scene.set_slot_off(ThingId(2), SlotId::Look);
        assert_eq!(write_target(&scene, &resolved, ThingId(2), SlotId::Look), None);
        assert_eq!(write_target(&scene, &resolved, ThingId(2), SlotId::Trail), None);
    }

    #[test]
    fn a_sole_user_edits_its_module_and_a_shared_module_gets_an_override() {
        let mut h = harness(Some(ThingId(2)));
        // "Ball look" is the ball's alone: the Glow slider writes into the module.
        let rect = slider_rect(&h, "Glow");
        drag_slider(&mut h, rect, 0.9);
        {
            let s = h.state();
            let (module, overrides) = ball_look(s);
            assert!(overrides.is_empty());
            let glow = s.scene.library.modules[&module].params.get("glow").cloned();
            assert!(matches!(glow, Some(Value::Number(g)) if g > 1.0), "{glow:?}");
        }
        assert!(h.query_by_label("Changed here").is_none());
        // Share it with the arena: now the ball's edits are its own, with a dot.
        {
            let s = h.state_mut();
            let (module, _) = ball_look(s);
            s.scene.link(&s.kit.registry, ThingId(1), SlotId::Look, module).unwrap();
            s.resolved = resolve(&s.scene, &s.kit.registry);
        }
        h.run();
        assert!(h.query_by_label_contains("shared ×2").is_some());
        let rect = slider_rect(&h, "Glow");
        drag_slider(&mut h, rect, 0.1);
        {
            let s = h.state();
            let (_, overrides) = ball_look(s);
            assert!(matches!(overrides.get("glow"), Some(Value::Number(g)) if *g < 0.5), "{overrides:?}");
        }
        assert!(h.query_by_label("Changed here").is_some());
        // Reset all in the picker drops the change again.
        h.get_by_label_contains("shared ×2").click();
        h.run();
        h.get_by_label("Reset all").click();
        h.run();
        assert!(ball_look(h.state()).1.is_empty());
        assert!(h.query_by_label("Changed here").is_none());
    }

    #[test]
    fn push_all_writes_the_overrides_into_the_module() {
        let mut h = harness(Some(ThingId(2)));
        {
            let s = h.state_mut();
            let (module, _) = ball_look(s);
            s.scene.link(&s.kit.registry, ThingId(1), SlotId::Look, module).unwrap();
            s.scene.set_override(&s.kit.registry, ThingId(2), SlotId::Look, "glow", Value::Number(1.7)).unwrap();
            s.resolved = resolve(&s.scene, &s.kit.registry);
        }
        h.run();
        h.get_by_label_contains("shared ×2").click();
        h.run();
        h.get_by_label("Push all to module").click();
        h.run();
        let s = h.state();
        let (module, overrides) = ball_look(s);
        assert!(overrides.is_empty());
        assert_eq!(s.scene.library.modules[&module].params.get("glow"), Some(&Value::Number(1.7)));
    }

    #[test]
    fn an_inherited_card_is_read_only_until_changed_here() {
        let mut h = harness(Some(ThingId(2)));
        // Motion is inherited from Scene defaults.
        assert!(h.get_by_role_and_label(Role::Slider, "Air drag").accesskit_node().is_disabled());
        // The first inherited card is Motion.
        h.get_all_by_label("Change here").next().expect("a Change here button").click();
        h.run();
        let s = h.state();
        assert!(matches!(s.scene.things[&ThingId(2)].slots[&SlotId::Motion], SlotLink::Linked { .. }));
        assert_eq!(s.scene.users_of(bpe_core::ModuleId(1)), vec![ThingId::ROOT, ThingId(2)]);
        assert!(!h.get_by_role_and_label(Role::Slider, "Air drag").accesskit_node().is_disabled());
    }

    #[test]
    fn make_unique_is_in_the_picker() {
        let mut h = harness(Some(ThingId(2)));
        h.get_by_label("Ball look").click();
        h.run();
        h.get_by_label("Make unique").click();
        h.run();
        let s = h.state();
        assert_eq!(s.scene.library.modules.len(), 6);
        assert!(s.scene.library.modules.values().any(|m| m.name == "Ball look (Ball)"));
    }

    #[test]
    fn open_in_library_asks_the_app_to_open_the_module() {
        let mut h = harness(Some(ThingId(2)));
        h.get_by_label("Ball look").click();
        h.run();
        h.get_by_label("Open in Library").click();
        h.run();
        assert_eq!(h.state().inspector.open_library, Some(ball_look(h.state()).0));
    }

    #[test]
    fn a_mixed_selection_only_edits_things_that_have_the_setting() {
        let mut h = harness(Some(ThingId(2)));
        {
            let s = h.state_mut();
            let c = s.scene.add_thing(&s.kit.registry, "balls", ThingId::ROOT).unwrap();
            s.selection.extra.extend([ThingId(1), c]);
            s.resolved = resolve(&s.scene, &s.kit.registry);
        }
        h.run();
        assert!(h.query_by_label("Editing 3 things").is_some());
        let rect = slider_rect(&h, "Size");
        drag_slider(&mut h, rect, 0.9);
        let s = h.state();
        let big = |id| matches!(s.scene.things[&id].params.get("radius"), Some(Value::Number(r)) if *r > 100.0);
        assert!(big(ThingId(2)) && big(ThingId(3)));
        assert!(s.scene.things[&ThingId(1)].params.get("radius").is_none(), "the arena's Radius is another setting");
    }

    #[test]
    fn a_slot_edit_reaches_every_selected_thing_through_its_own_policy() {
        let mut h = harness(Some(ThingId(2)));
        {
            let s = h.state_mut();
            // The arena inherits its look from the scene; the ball has its own module.
            s.selection.extra.insert(ThingId(1));
            s.resolved = resolve(&s.scene, &s.kit.registry);
        }
        h.run();
        let rect = slider_rect(&h, "Glow");
        drag_slider(&mut h, rect, 0.9);
        let s = h.state();
        let (module, overrides) = ball_look(s);
        assert!(overrides.is_empty(), "the ball is its module's only user");
        assert!(matches!(s.scene.library.modules[&module].params.get("glow"), Some(Value::Number(g)) if *g > 1.0));
        // The arena took its own link to the scene's look and changed glow on top of it.
        let SlotLink::Linked { module: arena_module, overrides } = &s.scene.things[&ThingId(1)].slots[&SlotId::Look]
        else {
            panic!("the arena links the look it inherited")
        };
        assert_eq!(*arena_module, ModuleId(0));
        assert!(matches!(overrides.get("glow"), Some(Value::Number(g)) if *g > 1.0), "{overrides:?}");
        assert!(
            s.scene.library.modules[&ModuleId(0)].params.get("glow").is_none(),
            "the scene's own look is untouched"
        );
    }

    #[test]
    fn the_parent_and_attach_rows_reparent_and_ride() {
        let mut h = harness(Some(ThingId(2)));
        // The Parent picker shows the current parent.
        h.get_by_label("Scene defaults").click();
        h.run();
        h.get_by_label("Arena").click();
        h.run();
        assert_eq!(h.state().scene.things[&ThingId(2)].parent, Some(ThingId(1)));
        h.get_by_label("Rides on it").click();
        h.run();
        let s = h.state();
        assert!(
            matches!(s.scene.things[&ThingId(2)].attach, Attach::Ride { offset, turn_with_parent: true, .. } if offset == Vec2::new(0.0, -200.0)),
            "{:?}",
            s.scene.things[&ThingId(2)].attach
        );
        assert!(h.query_all_by_label("Offset").next().is_some() && h.query_all_by_label("Position").next().is_none());
        assert!(h.query_all_by_label("Item").next().is_none(), "an arena has no balls to ride");
        h.get_by_label("Grouped").click();
        h.run();
        let s = h.state();
        assert_eq!(s.scene.things[&ThingId(2)].attach, Attach::Group);
        assert_eq!(s.scene.things[&ThingId(2)].transform.position, Vec2::new(0.0, -200.0));
    }

    #[test]
    fn a_target_setting_has_a_picker() {
        let mut h = harness(Some(ThingId(2)));
        // The bounce card's "Which balls" target is a More setting.
        {
            let s = h.state_mut();
            s.inspector.depth.insert((ThingId(2), Some(SlotId::Bounce)), Depth::More);
            // Give the ball its own bounce so the card is editable.
            let m = s.scene.add_module(&s.kit.registry, "bounce.basic", "Own bounce").unwrap();
            s.scene.link(&s.kit.registry, ThingId(2), SlotId::Bounce, m).unwrap();
            s.resolved = resolve(&s.scene, &s.kit.registry);
        }
        h.run();
        // The picker is a button named by its row, reading "Anything" until something is picked.
        h.get_by_role_and_label(Role::Button, "Which balls").click();
        h.run();
        h.get_by_role_and_label(Role::Button, "Kind").click();
        h.run();
        h.get_by_label("Arena").click();
        h.run();
        let s = h.state();
        let m = s.scene.library.modules.values().find(|m| m.name == "Own bounce").unwrap();
        assert_eq!(m.params.get("balls_to_hit"), Some(&Value::Target(bpe_core::Target::Kind("arena".into()))));
    }
}

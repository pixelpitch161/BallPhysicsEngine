//! The inspector: the selected thing's header, how it hangs in the tree, its kind's settings and
//! one card per slot. A shared module's card edits this thing's own overrides; an inherited card
//! is read-only until the thing takes its own link.

use crate::dice::dice_button;
use crate::selection::Selection;
use crate::theme;
use crate::widgets::{
    GridCx, KeyMark, TargetChoices, fold_toggle, grid_label, label_width, settings_grid, settings_grid_marked,
    settings_like_grid, widget_cell,
};
use bpe_core::{
    Address, Attach, Color, FOLDED_SECTIONS, ModuleId, ModuleKind, Owner, PulseSource, Resolved, SAME_TIME, Scene,
    SettingDef, SlotId, SlotLink, ThingId, Transform, Value, ValueType, section_rank,
};
use bpe_kit::Kit;
use eframe::egui::{self, DragValue, RichText, Ui};
use std::collections::BTreeSet;

#[derive(Default)]
pub struct Inspector {
    /// What is folded shut, by thing: its kind's settings card (`None`) and each slot's card
    /// (the group `""`), the root's cards by name, and groups of settings inside them by their
    /// heading.
    pub collapsed: BTreeSet<(ThingId, Option<SlotId>, &'static str)>,
    /// The quiet sections ([`FOLDED_SECTIONS`](bpe_core::FOLDED_SECTIONS)) opened, which
    /// otherwise stay folded.
    pub opened: BTreeSet<(ThingId, Option<SlotId>, &'static str)>,
    /// A keyed setting was edited this frame, so the key landed at the playhead; the studio
    /// pauses so a drag doesn't leave a key on every frame.
    pub keyed_edit: bool,
    /// Set when "Open in Library" was clicked; the app opens the Library window on it.
    pub open_library: Option<ModuleId>,
    /// The card the jump bar was clicked on, scrolled to when its header is next drawn.
    pub jump_to: Option<Card>,
    /// Each card header's top in the last frame, relative to the scroll area's top.
    pub card_tops: Vec<(Card, f32)>,
    /// The card last jumped to from the bar. The bar marks it until the next scroll, even when
    /// it is too short to reach the top.
    pub jumped: Option<(ThingId, Card)>,
}

/// One of the inspector's cards, as the jump bar names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Card {
    /// The kind's own settings, on every thing but the root.
    Thing,
    Placement,
    /// One of the root's cards of its own settings, by name: one of [`bpe_kit::ROOT_CARDS`].
    Section(&'static str),
    Slot(SlotId),
}

impl Card {
    /// The key that folds the card shut in [`Inspector::collapsed`].
    fn fold_key(self, id: ThingId) -> (ThingId, Option<SlotId>, &'static str) {
        match self {
            Card::Thing => (id, None, ""),
            Card::Placement => (id, None, "placement"),
            Card::Section(name) => (id, None, name),
            Card::Slot(slot) => (id, Some(slot), ""),
        }
    }
}

/// A change the panel asks for, applied once the drawing is done.
enum Edit {
    Name(String),
    Tags(BTreeSet<String>),
    Enabled(bool),
    /// Drawn on the screen or in the world; `None` is the kind's own choice.
    Screen(Option<bool>),
    Transform(Transform),
    Attach(Attach),
    /// One of the kind's own settings, for every selected thing of the kind.
    SetOwn(&'static str, Value),
    /// The module in a slot becomes another kind, once for each module the selected things show
    /// there with the same kind.
    SetKind(SlotId, &'static str),
    /// One of a slot's settings, for every selected thing whose slot holds the same module kind.
    SetSlot(SlotId, &'static str, Value),
    ResetOverride(SlotId, &'static str),
    PushOverride(SlotId, &'static str),
    /// Add a key at the playhead, or remove the one there.
    ToggleKey(Address),
    /// Pulse with a source, or stop pulsing.
    Pulse(Address, Option<PulseSource>),
    Link(SlotId, ModuleId),
    NewModule {
        slot: SlotId,
        kind: &'static str,
        copy_from: Option<ModuleId>,
    },
    MakeUnique(SlotId),
    PushAll(SlotId),
    ResetAll(SlotId),
    /// Follow the folder above instead of a module of its own.
    Follow(SlotId),
    /// Give a followed slot its own copy of the folder's module.
    MakeOwn(SlotId),
    Off(SlotId),
    AddSlot(SlotId),
    /// Lock (true) or unlock a setting against the dice.
    Lock(Address, bool),
    /// Roll the dice on one card: the kind's own settings (`None`) or one slot.
    RollCard(Option<SlotId>),
    /// Roll the dice on one of the root's cards: its own settings in that group.
    RollGroup(&'static str),
}

/// Where an edit to a slot's settings goes for one thing.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Write {
    /// The thing is the module's only user with no changes of its own: edit the module.
    Module(ModuleId),
    /// The module is shared, or the thing already has changes: edit this thing's overrides.
    Override(ThingId, SlotId),
    /// The slot follows a folder above: read-only until the thing makes its own copy.
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

/// The modules a type switch on `slot` changes: the one `id` links there and, for each other
/// selected thing, the module it links there when that is of the same kind.
fn type_targets(scene: &Scene, selected: &[ThingId], id: ThingId, slot: SlotId) -> BTreeSet<ModuleId> {
    let shown = |t: &ThingId| match scene.things.get(t).and_then(|t| t.slots.get(&slot)) {
        Some(SlotLink::Linked { module, .. }) => Some(*module),
        _ => None,
    };
    let mkind = shown(&id).and_then(|m| scene.library.modules.get(&m)).map(|m| m.kind.clone());
    selected
        .iter()
        .filter_map(shown)
        .filter(|m| scene.library.modules.get(m).map(|m| &m.kind) == mkind.as_ref())
        .collect()
}

/// Draws the inspector for the selection's primary thing and applies its edits, to every
/// selected thing where that makes sense. Values are shown as they are at `time` (the
/// playhead), and an edit to a keyed setting writes a key there. True when the scene changed.
pub fn show(
    ui: &mut Ui,
    inspector: &mut Inspector,
    scene: &mut Scene,
    kit: &Kit,
    resolved: &Resolved,
    selection: &Selection,
    time: f64,
) -> bool {
    // Nothing inside may widen the panel: labels wrap at one width, the same in every card.
    let panel = ui.available_width();
    ui.set_max_width(panel);
    let lw = label_width(panel);
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

    let marked = jump_bar(ui, inspector, id, kind, &thing);
    inspector.card_tops.clear();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        if selected.len() > 1 {
            ui.label(RichText::new(format!("Editing {} things", selected.len())).color(theme::MUTED).small())
                .on_hover_text("Settings changed here apply to every selected thing that has them.");
        }
        // ---------- header ----------
        // Row 1: the name, the kind and the eye; row 2: the tags.
        // Indented like a card's insides, so its columns line up with the cards' settings.
        let indent = egui::Frame::new().inner_margin(egui::Margin::symmetric(CARD_MARGIN, 0));
        indent.show(ui, |ui| {
            settings_like_grid(("header", id)).show(ui, |ui| {
                let name_label = grid_label(ui, lw, "Name");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Hiding the root would hide the whole scene; its card already says what it is.
                    if id != ThingId::ROOT {
                        if eye_toggle(ui, thing.enabled) {
                            edits.push(Edit::Enabled(!thing.enabled));
                        }
                        ui.label(RichText::new(kind.name()).small().color(theme::MUTED)).on_hover_text(kind.help());
                    }
                    let mut name = thing.name.clone();
                    let edit = egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY);
                    if ui.add(edit).labelled_by(name_label.id).changed() {
                        edits.push(Edit::Name(name));
                    }
                });
                ui.end_row();
                let tags_label = grid_label(ui, lw, "Tags").on_hover_text(
                    "Words that bounce targets and the camera can pick this out by, separated by spaces",
                );
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
            })
        });
        ui.add_space(6.0);

        // ---------- the kind's own settings ----------
        let own = |key: &str| Address { owner: Owner::Thing(id), slot: None, key: key.into() };
        let applies = |d: &SettingDef| d.applies(Some(&thing.kind), &|k| scene.value_at_time(reg, &own(k), time));
        let cx = OwnCx { scene, kit, choices: &choices, id, thing: &thing, time, label_width: lw, applies: &applies };
        if id == ThingId::ROOT {
            // The root's settings are one card per group.
            for (i, name) in bpe_kit::ROOT_CARDS.into_iter().enumerate() {
                if i > 0 {
                    ui.add_space(6.0);
                }
                root_card(ui, inspector, &cx, name, &mut edits);
            }
        } else {
            card(ui, |ui| {
                // The kind's name alone: "Balls", "Arena".
                let title = kind.name().to_owned();
                let header = ui.horizontal_wrapped(|ui| {
                    let open = fold_arrow(ui, &mut inspector.collapsed, Card::Thing.fold_key(id), &title);
                    ui.label(RichText::new(&title).strong());
                    ui.add_space(8.0);
                    if dice_button(ui, "Randomize these settings").clicked() {
                        edits.push(Edit::RollCard(None));
                    }
                    open
                });
                card_header(ui, inspector, Card::Thing, &header.response);
                if header.inner {
                    own_grid(ui, inspector, &cx, ("kind", id), kind.settings(), &mut edits);
                }
            });
        }

        // ---------- placement ----------
        // The camera's place is its own Position setting, which can be keyframed.
        if id != ThingId::ROOT && kind.id() != "camera" {
            ui.add_space(6.0);
            card(ui, |ui| {
                let header = ui.horizontal(|ui| {
                    let open = fold_arrow(ui, &mut inspector.collapsed, (id, None, "placement"), "Placement");
                    ui.label(RichText::new("Placement").strong());
                    open
                });
                card_header(ui, inspector, Card::Placement, &header.response);
                if header.inner {
                    placement_rows(ui, scene, kind, &thing, lw, &mut edits);
                }
            });
        }

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
                        Some(r) => {
                            format!("Follow folder: {}", scene.things.get(&r.from).map_or("?", |t| t.name.as_str()))
                        }
                        None => "Nothing (pick a module)".to_owned(),
                    },
                    SlotLink::Off => "Removed".to_owned(),
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
                let header = ui.horizontal_wrapped(|ui| {
                    let title = kind.slot_label(slot);
                    let open = fold_arrow(ui, &mut inspector.collapsed, (id, Some(slot), ""), title);
                    ui.label(RichText::new(title).strong());
                    // The Type menu: what kind of module fills the slot, when more than one fits.
                    let fitting: Vec<&dyn ModuleKind> = reg.kinds_fitting(slot, kind).collect();
                    let shown = shown_module.and_then(|m| scene.library.modules.get(&m));
                    if let Some(m) = shown
                        && let Some(current) = reg.module(&m.kind)
                        && fitting.len() > 1
                    {
                        let inherited = matches!(link, SlotLink::Inherit);
                        ui.add_space(4.0);
                        let label = ui.label(RichText::new("Type").small().color(theme::MUTED));
                        ui.add_enabled_ui(!inherited, |ui| {
                            ui.menu_button(current.name(), |ui| {
                                ui.set_min_width(180.0);
                                let users = scene.users_of(m.id).len();
                                if users > 1 {
                                    ui.label(
                                        RichText::new(format!("Changes all {users} things using {}", m.name))
                                            .color(theme::MUTED),
                                    );
                                }
                                // Only kinds every thing using the changed modules takes.
                                let targets = type_targets(scene, &selected, id, slot);
                                let offered = fitting.iter().filter(|k| {
                                    k.id() == current.id()
                                        || targets.iter().all(|t| scene.kind_suits_users(reg, *t, k.id()))
                                });
                                for k in offered {
                                    if ui
                                        .selectable_label(k.id() == current.id(), k.name())
                                        .on_hover_text(k.help())
                                        .clicked()
                                    {
                                        edits.push(Edit::SetKind(slot, k.id()));
                                        ui.close();
                                    }
                                }
                            })
                            .response
                            .labelled_by(label.id)
                            .on_hover_text("What kind of module fills this slot")
                            .on_disabled_hover_text("Make own first: this slot follows the folder above");
                        });
                    }
                    // The picker: a button that reads the slot's state and drops down the choices.
                    ui.menu_button(state, |ui| {
                        for m in
                            scene.library.modules.values().filter(|m| m.slot == slot && kind.accepts(slot, &m.kind))
                        {
                            let current = matches!(link, SlotLink::Linked { module, .. } if *module == m.id);
                            if ui.selectable_label(current, &m.name).clicked() {
                                edits.push(Edit::Link(slot, m.id));
                                ui.close();
                            }
                        }
                        ui.separator();
                        // One kind: a plain button. Several: a submenu naming each kind. A new
                        // module copies the shown one's settings, all of them when it is of the
                        // same kind, else the ones both kinds have (a look's colour and glow).
                        let kinds = &fitting;
                        if let [only] = kinds[..] {
                            if ui.button(format!("New {title} module…")).clicked() {
                                edits.push(Edit::NewModule { slot, kind: only.id(), copy_from: shown_module });
                                ui.close();
                            }
                        } else if !kinds.is_empty() {
                            ui.menu_button(format!("New {title} module"), |ui| {
                                ui.set_min_width(180.0);
                                for kind in kinds {
                                    if ui.button(kind.name()).on_hover_text(kind.help()).clicked() {
                                        edits.push(Edit::NewModule { slot, kind: kind.id(), copy_from: shown_module });
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
                        if folder_above(scene, reg, id, slot).is_some()
                            && ui
                                .selectable_label(matches!(link, SlotLink::Inherit), "Follow folder")
                                .on_hover_text("Use whatever the folder above uses")
                                .clicked()
                        {
                            edits.push(Edit::Follow(slot));
                            ui.close();
                        }
                        if ui
                            .selectable_label(matches!(link, SlotLink::Off), "Remove")
                            .on_hover_text("Nothing fills this slot")
                            .clicked()
                        {
                            edits.push(Edit::Off(slot));
                            ui.close();
                        }
                    })
                    .response
                    .on_hover_text("Which module fills this slot");
                    ui.add_space(8.0);
                    let editable =
                        matches!(write_target(scene, resolved, id, slot), Some(Write::Module(_) | Write::Override(..)));
                    if editable {
                        ui.add_space(8.0);
                        if dice_button(ui, "Randomize these settings").clicked() {
                            edits.push(Edit::RollCard(Some(slot)));
                        }
                    }
                    open
                });
                card_header(ui, inspector, Card::Slot(slot), &header.response);
                let open = header.inner;
                let problems: Vec<String> = resolved
                    .problems
                    .iter()
                    .filter(|p| p.thing == id && p.slot == Some(slot))
                    .map(crate::tree::problem_words)
                    .collect();
                if !problems.is_empty() {
                    ui.label(RichText::new(problems.join("\n")).small().color(theme::ERROR));
                }
                if !open {
                    return;
                }
                let Some(module) = shown_module else { return };
                let (Some(m), Some(write)) =
                    (scene.library.modules.get(&module), write_target(scene, resolved, id, slot))
                else {
                    return;
                };
                let Some(mkind) = reg.module(&m.kind) else { return };
                let physics = matches!(slot, SlotId::Motion | SlotId::Bounce);
                let of_module = |key: &str| Address { owner: Owner::Module(module), slot: None, key: key.into() };
                let of_thing = |key: &str| Address { owner: Owner::Thing(id), slot: Some(slot), key: key.into() };
                let mark = |a: &Address| {
                    let def = mkind.settings().iter().find(|d| d.key == a.key)?;
                    mark_for(scene, def, a, time, physics)
                };
                ui.add_space(4.0);
                let defs = mkind.settings();
                debug_assert!(
                    unknown_groups(defs).is_empty(),
                    "{} has groups that are not sections: {:?}",
                    m.kind,
                    unknown_groups(defs)
                );
                match write {
                    Write::Module(_) => {
                        let get = |key: &str| {
                            scene.value_at_time(reg, &of_module(key), time).expect("a module's settings all resolve")
                        };
                        let marks = |key: &str| mark(&of_module(key));
                        let locked = |key: &str| scene.is_locked(&of_module(key));
                        let applies = |d: &SettingDef| {
                            d.applies(Some(&thing.kind), &|k| scene.value_at_time(reg, &of_module(k), time))
                        };
                        let shut = folded_groups(inspector, (id, Some(slot)), defs);
                        let folded = |group: &str| shut.contains(group);
                        let cx = GridCx {
                            choices: &choices,
                            overridden: &|_| false,
                            inherited: &|_| None,
                            marks: &marks,
                            locked: &locked,
                            applies: &applies,
                            folded: Some(&folded),
                            label_width: lw,
                        };
                        let out = settings_grid_marked(ui, ("slot", id, slot), defs, get, &cx);
                        fold_groups(inspector, (id, Some(slot)), &out.folds);
                        edits.extend(out.changes.into_iter().map(|(key, value)| Edit::SetSlot(slot, key, value)));
                        edits.extend(out.toggled_key.into_iter().map(|key| Edit::ToggleKey(of_module(key))));
                        edits.extend(out.pulse.into_iter().map(|(key, source)| Edit::Pulse(of_module(key), source)));
                        edits.extend(out.locks.into_iter().map(|(key, lock)| Edit::Lock(of_module(key), lock)));
                    }
                    Write::Override(..) => {
                        let get = |key: &str| {
                            scene
                                .value_at_time(reg, &of_thing(key), time)
                                .expect("a linked slot's settings all resolve")
                        };
                        let overridden = |key: &str| overrides.is_some_and(|o| o.get(key).is_some());
                        let inherited = |key: &str| scene.value_at(reg, &of_module(key));
                        let marks = |key: &str| mark(&of_thing(key));
                        // A row shows the module's value until this thing changes it, so a lock
                        // goes on what the row shows: the module's setting, else this change.
                        let locked = |key: &str| {
                            scene.is_locked(&of_thing(key)) || (!overridden(key) && scene.is_locked(&of_module(key)))
                        };
                        let applies = |d: &SettingDef| {
                            d.applies(Some(&thing.kind), &|k| scene.value_at_time(reg, &of_thing(k), time))
                        };
                        let shut = folded_groups(inspector, (id, Some(slot)), defs);
                        let folded = |group: &str| shut.contains(group);
                        let cx = GridCx {
                            choices: &choices,
                            overridden: &overridden,
                            inherited: &inherited,
                            marks: &marks,
                            locked: &locked,
                            applies: &applies,
                            folded: Some(&folded),
                            label_width: lw,
                        };
                        let out = settings_grid_marked(ui, ("slot", id, slot), defs, get, &cx);
                        fold_groups(inspector, (id, Some(slot)), &out.folds);
                        edits.extend(out.changes.into_iter().map(|(key, value)| Edit::SetSlot(slot, key, value)));
                        edits.extend(out.resets.into_iter().map(|key| Edit::ResetOverride(slot, key)));
                        edits.extend(out.pushes.into_iter().map(|key| Edit::PushOverride(slot, key)));
                        edits.extend(out.toggled_key.into_iter().map(|key| Edit::ToggleKey(of_thing(key))));
                        edits.extend(out.pulse.into_iter().map(|(key, source)| Edit::Pulse(of_thing(key), source)));
                        for (key, lock) in out.locks {
                            match (lock, overridden(key)) {
                                (true, true) => edits.push(Edit::Lock(of_thing(key), true)),
                                (true, false) => edits.push(Edit::Lock(of_module(key), true)),
                                (false, overridden) => {
                                    edits.push(Edit::Lock(of_thing(key), false));
                                    if !overridden {
                                        edits.push(Edit::Lock(of_module(key), false));
                                    }
                                }
                            }
                        }
                    }
                    Write::Inherited { module, .. } => {
                        if make_own(ui).clicked() {
                            edits.push(Edit::MakeOwn(slot));
                        }
                        let get = |key: &str| {
                            rslot.and_then(|r| r.params.get(key).cloned()).unwrap_or_else(|| {
                                let a = Address { owner: Owner::Module(module), slot: None, key: key.into() };
                                scene.value_at(reg, &a).expect("a module's settings all resolve")
                            })
                        };
                        let applies = |d: &SettingDef| d.applies(Some(&thing.kind), &|k| Some(get(k)));
                        ui.add_enabled_ui(false, |ui| {
                            settings_grid(ui, ("slot", id, slot), defs, &applies, lw, get);
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
            .filter(|s| !thing.slots.contains_key(s) && reg.modules_for(*s).any(|m| kind.accepts(*s, m.id())))
            .collect();
        ui.add_enabled_ui(!missing.is_empty(), |ui| {
            ui.menu_button("+ Add slot", |ui| {
                for slot in &missing {
                    if ui.button(kind.slot_label(*slot)).clicked() {
                        edits.push(Edit::AddSlot(*slot));
                        ui.close();
                    }
                }
            })
            .response
            .on_disabled_hover_text("Every slot this kind can use is already shown");
        });
    });
    // A jump to a card that wasn't drawn (it went away) is dropped.
    inspector.jump_to = None;
    // The bar marked the card at the top as of the last frame; once scrolling has moved
    // another card there, draw again so the bar catches up.
    if marked_card(inspector, id) != marked {
        ui.ctx().request_repaint();
    }

    let mut changed = false;
    for edit in edits {
        // A folder's followers keep what they used when its slot changes under them.
        let reslotted = matches!(edit, Edit::Link(..) | Edit::NewModule { .. } | Edit::Follow(_) | Edit::Off(_));
        let before = (reslotted && !scene.children(id).is_empty()).then(|| bpe_core::resolve(scene, reg));
        let ok = match edit {
            Edit::Name(name) => {
                scene.set_thing_name(id, &name);
                true
            }
            Edit::Tags(tags) => set_thing(scene, id, |t| t.tags = tags),
            Edit::Enabled(enabled) => set_thing(scene, id, |t| t.enabled = enabled),
            Edit::Screen(screen) => set_thing(scene, id, |t| t.screen = screen),
            Edit::Transform(t) => set_thing(scene, id, |thing| thing.transform = t),
            Edit::Attach(attach) => crate::tree::reattach(scene, id, attach).is_ok(),
            Edit::SetOwn(key, value) => {
                let mut any = false;
                let same_kind: Vec<ThingId> = selected
                    .iter()
                    .copied()
                    .filter(|t| scene.things.get(t).is_some_and(|t| t.kind == thing.kind))
                    .collect();
                for t in &same_kind {
                    let a = Address { owner: Owner::Thing(*t), slot: None, key: key.into() };
                    any |= set_or_key(scene, reg, &a, value.clone(), time, &mut inspector.keyed_edit);
                }
                any
            }
            Edit::SetKind(slot, kind) => {
                let mut any = false;
                for m in type_targets(scene, &selected, id, slot) {
                    any |= scene.set_module_kind(reg, m, kind).is_ok();
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
                    any |=
                        write_slot(scene, resolved, reg, *t, slot, key, value.clone(), time, &mut inspector.keyed_edit);
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
            Edit::ToggleKey(address) => toggle_key(scene, reg, &address, time),
            Edit::Pulse(address, Some(source)) => match scene.def_at(reg, &address) {
                Some(def) => scene.set_pulse(reg, &address, source, pulse_amount(def), 0.25).is_ok(),
                None => false,
            },
            Edit::Pulse(address, None) => scene.remove_pulse(&address),
            Edit::Link(slot, module) => scene.link(reg, id, slot, module).is_ok(),
            Edit::NewModule { slot, kind, copy_from } => {
                let name = format!("{} {}", thing.name, slot.name());
                match scene.add_module(reg, kind, &name) {
                    Ok(module) => {
                        // The current module's settings carry over: all of them to the same kind,
                        // the ones both kinds have to another.
                        if let Some(params) = copy_from
                            .and_then(|c| scene.library.modules.get(&c))
                            .map(|m| if m.kind == kind { m.params.clone() } else { reg.carry_over(&m.params, kind) })
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
            Edit::Follow(slot) => {
                scene.set_slot_inherit(id, slot);
                true
            }
            Edit::MakeOwn(slot) => scene.make_own(reg, id, slot).is_ok(),
            Edit::Off(slot) => {
                scene.set_slot_off(id, slot);
                true
            }
            Edit::AddSlot(slot) => bpe_kit::fill_slot(scene, reg, id, slot).is_ok(),
            Edit::Lock(address, lock) => scene.set_locked(address, lock),
            Edit::RollCard(slot) => {
                use bpe_kit::random::{Roll, Scope, fresh_seed, roll};
                let scope = Scope::Card { thing: id, slot };
                !roll(scene, reg, &scope, Roll { seed: fresh_seed(), amount: 1.0 }).changed.is_empty()
            }
            Edit::RollGroup(group) => {
                use bpe_kit::random::{Roll, Scope, fresh_seed, roll};
                let scope = Scope::Group { thing: id, group };
                !roll(scene, reg, &scope, Roll { seed: fresh_seed(), amount: 1.0 }).changed.is_empty()
            }
        };
        if let Some(before) = before.filter(|_| ok) {
            bpe_kit::keep_followers(scene, reg, &before);
        }
        changed |= ok;
    }
    changed
}

/// The folder above `id` whose `slot` it would follow, if any fills it.
fn folder_above(scene: &Scene, reg: &bpe_core::Registry, id: ThingId, slot: SlotId) -> Option<ThingId> {
    let mut at = scene.things.get(&id)?.parent;
    while let Some(p) = at {
        let t = scene.things.get(&p)?;
        if reg.thing(&t.kind).is_some_and(|k| k.passes_down()) {
            return bpe_core::resolve_slot(scene, reg, p, slot, &mut Vec::new()).map(|r| r.from);
        }
        at = t.parent;
    }
    None
}

/// Writes one slot setting for one thing, where its write policy says: the module or the thing's
/// own override. A followed slot is read-only (Make own first), so nothing is written.
#[allow(clippy::too_many_arguments)]
fn write_slot(
    scene: &mut Scene,
    resolved: &Resolved,
    reg: &bpe_core::Registry,
    id: ThingId,
    slot: SlotId,
    key: &str,
    value: Value,
    time: f64,
    keyed: &mut bool,
) -> bool {
    match write_target(scene, resolved, id, slot) {
        Some(Write::Module(module)) => {
            let a = Address { owner: Owner::Module(module), slot: None, key: key.into() };
            set_or_key(scene, reg, &a, value, time, keyed)
        }
        Some(Write::Override(..)) => {
            let a = Address { owner: Owner::Thing(id), slot: Some(slot), key: key.into() };
            set_or_key(scene, reg, &a, value, time, keyed)
        }
        Some(Write::Inherited { .. }) | None => false,
    }
}

/// Writes `value` to `a`: as a key at `time` when the setting has keys (and notes so in
/// `keyed`), else as its plain value.
fn set_or_key(
    scene: &mut Scene,
    reg: &bpe_core::Registry,
    a: &Address,
    value: Value,
    time: f64,
    keyed: &mut bool,
) -> bool {
    if scene.keyed(a) {
        *keyed = true;
        scene.set_key(reg, a, time, value).is_ok()
    } else {
        scene.set_at(reg, a, value).is_ok()
    }
}

/// The Placement card's rows: where the thing sits, as a position of its own or an offset along
/// for the ride, how it is turned, how it hangs off its parent, and whether it sticks to the
/// screen.
fn placement_rows(
    ui: &mut Ui,
    scene: &Scene,
    kind: &dyn bpe_core::ThingKind,
    thing: &bpe_core::Thing,
    lw: f32,
    edits: &mut Vec<Edit>,
) {
    let right = ui.available_rect_before_wrap().right();
    settings_like_grid(("placement", thing.id)).show(ui, |ui| {
        position_rows(ui, scene, thing, lw, right, edits);
        if kind.can_stick_to_screen() {
            let label = grid_label(ui, lw, "Stick to screen")
                .on_hover_text("Stay put on the screen while the camera moves, like a title");
            let mut on = thing.sticks_to_screen(kind);
            if ui.checkbox(&mut on, "").labelled_by(label.id).changed() {
                edits.push(Edit::Screen((on != kind.sticks_to_screen()).then_some(on)));
            }
            ui.end_row();
        }
    });
}

/// Where the thing sits and how it hangs off its parent: Position (for a grouped thing),
/// Rotation, Attach, and a rider's Offset, Turns with it and Item.
fn position_rows(ui: &mut Ui, scene: &Scene, thing: &bpe_core::Thing, lw: f32, right: f32, edits: &mut Vec<Edit>) {
    let mut t = thing.transform;
    if let Attach::Group = thing.attach {
        let position = grid_label(ui, lw, "Position")
            .on_hover_text("Where it sits in the world. At Zoom 1 the shot is 1080 across");
        // In a narrow panel y goes under x.
        let moved = widget_cell(ui, right, |ui| {
            ui.horizontal_wrapped(|ui| {
                let x = ui.add(DragValue::new(&mut t.position.x).speed(1.0).prefix("x ").suffix(" px"));
                let y = ui.add(DragValue::new(&mut t.position.y).speed(1.0).prefix("y ").suffix(" px"));
                x.labelled_by(position.id).changed() || y.labelled_by(position.id).changed()
            })
            .inner
        });
        ui.end_row();
        if moved {
            edits.push(Edit::Transform(t));
        }
    }
    let rotation = grid_label(ui, lw, "Rotation").on_hover_text("How far it is turned, clockwise");
    let mut degrees = t.rotation.to_degrees();
    let turned = ui.add(DragValue::new(&mut degrees).speed(1.0).suffix("°")).labelled_by(rotation.id).changed();
    if turned {
        t.rotation = degrees.to_radians();
        edits.push(Edit::Transform(t));
    }
    ui.end_row();

    let parent = thing.parent.unwrap_or(ThingId::ROOT);
    let rides = thing.rides();
    grid_label(ui, lw, "Attach").on_hover_text("Grouped things keep their own place; a rider follows its parent");
    widget_cell(ui, right, |ui| {
        ui.horizontal_wrapped(|ui| {
            if ui.selectable_label(!rides, "Grouped").clicked() && rides {
                edits.push(Edit::Attach(Attach::Group));
            }
            let ride = ui.add_enabled(parent != ThingId::ROOT, egui::Button::selectable(rides, "Rides on it"));
            let why = "Scene settings don't move, so there is nothing to ride";
            if ride.on_disabled_hover_text(why).clicked() && !rides {
                let ride = Attach::Ride { offset: glam::Vec2::ZERO, turn_with_parent: true, item: None };
                edits.push(Edit::Attach(ride));
            }
        });
    });
    ui.end_row();

    if let Attach::Ride { offset, turn_with_parent, item } = thing.attach {
        let mut o = offset;
        let label = grid_label(ui, lw, "Offset").on_hover_text("Where it sits from its parent, in video pixels");
        let moved = widget_cell(ui, right, |ui| {
            ui.horizontal_wrapped(|ui| {
                let x = ui.add(DragValue::new(&mut o.x).speed(1.0).prefix("x ").suffix(" px"));
                let y = ui.add(DragValue::new(&mut o.y).speed(1.0).prefix("y ").suffix(" px"));
                x.labelled_by(label.id).changed() || y.labelled_by(label.id).changed()
            })
            .inner
        });
        ui.end_row();
        let label = grid_label(ui, lw, "Turns with it").on_hover_text("Swing around as the parent turns");
        let mut turns = turn_with_parent;
        let turned = ui.checkbox(&mut turns, "").labelled_by(label.id).changed();
        ui.end_row();
        let mut picked = item;
        let mut item_changed = false;
        if thing.parent.and_then(|p| scene.things.get(&p)).is_some_and(|p| p.kind == "balls") {
            let label =
                grid_label(ui, lw, "Item").on_hover_text("Ride one ball of the parent: its number, or -1 for all");
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
}

/// The header's eye: open while the thing is shown. True when it was clicked.
fn eye_toggle(ui: &mut Ui, shown: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(18.0, 16.0), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, shown, "Shown"));
    let ink = if response.hovered() { theme::TEXT } else { theme::MUTED };
    crate::tree::paint_eye(ui.painter(), rect, shown, ink);
    let help = if shown {
        "Shown. A hidden thing is left out of the video and the physics."
    } else {
        "Hidden: left out of the video and the physics. Click to show it."
    };
    response.on_hover_text(help).clicked()
}

/// What a setting's diamond shows, or `None` for a setting that can't be keyed or pulsed.
fn mark_for(scene: &Scene, def: &SettingDef, a: &Address, time: f64, physics: bool) -> Option<KeyMark> {
    // Hits are the only pulse, and a setting that moves the physics can't follow them; one that
    // pulses anyway keeps its menu so the pulse can be taken off.
    let can_pulse = def.flags.pulse && (!physics || scene.pulses.contains_key(a));
    if !def.flags.keyframe && !can_pulse {
        return None;
    }
    Some(KeyMark {
        can_key: def.flags.keyframe,
        can_pulse,
        keyed: scene.keyed(a),
        on_key: scene.tracks.get(a).and_then(|t| t.key_near(time, SAME_TIME)).is_some(),
        pulsed: scene.pulses.contains_key(a),
        physics,
    })
}

/// Removes the key at the playhead, or puts one there holding the setting's current value.
fn toggle_key(scene: &mut Scene, reg: &bpe_core::Registry, a: &Address, time: f64) -> bool {
    if let Some(index) = scene.tracks.get(a).and_then(|t| t.key_near(time, SAME_TIME)) {
        return scene.remove_key(a, index);
    }
    let Some(value) = scene.value_at_time(reg, a, time) else { return false };
    scene.set_key(reg, a, time, value).is_ok()
}

/// A pulse's starting amount: a quarter of the setting's range, as its kind of value.
pub(crate) fn pulse_amount(def: &SettingDef) -> Value {
    let quarter = def.range.map_or(1.0, |(min, max)| (max - min) / 4.0);
    match (&def.ty, &def.default) {
        (ValueType::Int, _) => Value::Int(quarter.round().max(1.0) as i64),
        (ValueType::Vec2, Value::Vec2(v)) if v != &[0.0, 0.0] => Value::Vec2([v[0] / 4.0, v[1] / 4.0]),
        (ValueType::Vec2, _) => Value::Vec2([0.0, 100.0]),
        (ValueType::Color, _) => Value::Color(Color::WHITE),
        _ => Value::Number(quarter),
    }
}

/// What drawing a thing's own settings reads.
struct OwnCx<'a> {
    scene: &'a Scene,
    kit: &'a Kit,
    choices: &'a TargetChoices,
    id: ThingId,
    thing: &'a bpe_core::Thing,
    time: f64,
    label_width: f32,
    /// Whether a setting does anything here; rows that don't are left out.
    applies: &'a dyn Fn(&SettingDef) -> bool,
}

/// A grid of some of a thing's own settings, with their keys, pulses and locks, and folding
/// groups. Its edits are pushed to `edits`.
fn own_grid(
    ui: &mut Ui,
    inspector: &mut Inspector,
    cx: &OwnCx,
    grid: impl std::hash::Hash + std::fmt::Debug,
    defs: &[SettingDef],
    edits: &mut Vec<Edit>,
) {
    let (scene, reg, id, time) = (cx.scene, &cx.kit.registry, cx.id, cx.time);
    let own = |key: &str| Address { owner: Owner::Thing(id), slot: None, key: key.into() };
    let get = |key: &str| scene.value_at_time(reg, &own(key), time).expect("the kind's own settings all resolve");
    let sim = cx.kit.sims.thing(&cx.thing.kind);
    let marks = |key: &str| {
        let def = defs.iter().find(|d| d.key == key)?;
        let physics = sim.is_some_and(|s| s.affects_physics(key));
        mark_for(scene, def, &own(key), time, physics)
    };
    let locked = |key: &str| scene.is_locked(&own(key));
    let folded = |group: &str| is_folded(inspector, (id, None), group);
    let grid_cx = GridCx {
        choices: cx.choices,
        overridden: &|_| false,
        inherited: &|_| None,
        marks: &marks,
        locked: &locked,
        applies: cx.applies,
        folded: Some(&folded),
        label_width: cx.label_width,
    };
    let out = settings_grid_marked(ui, grid, defs, get, &grid_cx);
    fold_groups(inspector, (id, None), &out.folds);
    edits.extend(out.changes.into_iter().map(|(key, value)| Edit::SetOwn(key, value)));
    edits.extend(out.toggled_key.into_iter().map(|key| Edit::ToggleKey(own(key))));
    edits.extend(out.pulse.into_iter().map(|(key, source)| Edit::Pulse(own(key), source)));
    edits.extend(out.locks.into_iter().map(|(key, lock)| Edit::Lock(own(key), lock)));
}

/// One of the root's cards: the root's own settings whose group is `name`, under a title that
/// folds them away, with dice when any of them can be rolled.
fn root_card(ui: &mut Ui, inspector: &mut Inspector, cx: &OwnCx, name: &'static str, edits: &mut Vec<Edit>) {
    let Some(kind) = cx.kit.registry.thing(&cx.thing.kind) else { return };
    // Blanked, so no heading inside repeats the card's title.
    let defs: Vec<SettingDef> =
        kind.settings().iter().filter(|d| d.group == name).map(|d| SettingDef { group: "", ..d.clone() }).collect();
    let card_kind = Card::Section(name);
    card(ui, |ui| {
        let header = ui.horizontal_wrapped(|ui| {
            let open = fold_arrow(ui, &mut inspector.collapsed, card_kind.fold_key(cx.id), name);
            ui.label(RichText::new(name).strong());
            ui.add_space(8.0);
            if defs.iter().any(|d| d.flags.random) && dice_button(ui, "Randomize these settings").clicked() {
                edits.push(Edit::RollGroup(name));
            }
            open
        });
        card_header(ui, inspector, card_kind, &header.response);
        if header.inner {
            own_grid(ui, inspector, cx, ("kind", cx.id, name), &defs, edits);
        }
    });
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

/// The arrow at the left of a card's title that folds the card's settings away and back. True
/// while the card is open.
pub(crate) fn fold_arrow<K: Ord + Copy>(ui: &mut Ui, collapsed: &mut BTreeSet<K>, key: K, title: &str) -> bool {
    let open = !collapsed.contains(&key);
    if fold_toggle(ui, open, title) {
        if open {
            collapsed.insert(key);
        } else {
            collapsed.remove(&key);
        }
        return !open;
    }
    open
}

/// Whether a section stays folded until opened: one of [`FOLDED_SECTIONS`], or a paint's own
/// variation, which ranks with Variation.
fn quiet(group: &str) -> bool {
    section_rank(group).is_some_and(|rank| FOLDED_SECTIONS.iter().any(|f| section_rank(f) == Some(rank)))
}

/// Whether a group of a card's settings is folded shut: a quiet section unless opened, any
/// other group when folded.
fn is_folded(inspector: &Inspector, (id, slot): (ThingId, Option<SlotId>), group: &str) -> bool {
    let has = |set: &BTreeSet<(ThingId, Option<SlotId>, &'static str)>| {
        set.iter().any(|&(i, s, g)| i == id && s == slot && g == group)
    };
    if quiet(group) { !has(&inspector.opened) } else { has(&inspector.collapsed) }
}

/// The groups of a card's settings that are folded shut, as [`is_folded`] says.
fn folded_groups(
    inspector: &Inspector,
    card: (ThingId, Option<SlotId>),
    defs: &[SettingDef],
) -> BTreeSet<&'static str> {
    defs.iter().map(|d| d.group).filter(|g| !g.is_empty() && is_folded(inspector, card, g)).collect()
}

/// Folds shut or opens the groups of a card's settings, as their headings asked.
fn fold_groups(inspector: &mut Inspector, (id, slot): (ThingId, Option<SlotId>), folds: &[(&'static str, bool)]) {
    for &(group, shut) in folds {
        let key = (id, slot, group);
        match (quiet(group), shut) {
            (true, true) => inspector.opened.remove(&key),
            (true, false) => inspector.opened.insert(key),
            (false, true) => inspector.collapsed.insert(key),
            (false, false) => inspector.collapsed.remove(&key),
        };
    }
}

/// The groups of a module's settings that are not sections, for the debug check that every
/// group is one. Every slot's modules should have none.
fn unknown_groups(defs: &[SettingDef]) -> Vec<&'static str> {
    defs.iter().map(|d| d.group).filter(|g| !g.is_empty() && section_rank(g).is_none()).collect()
}

/// The button an inherited card offers to link the module to the thing itself.
fn make_own(ui: &mut Ui) -> egui::Response {
    ui.button("Make own").on_hover_text("Give this thing its own copy, so changes here are its own")
}

/// The row of buttons above the inspector that jump to its cards: the thing's own card (the
/// root's six cards), then one per slot it has. The card last clicked is selected until the
/// next scroll, then the card whose header has scrolled to the top (or just past). A click
/// unfolds the card and scrolls it to the top when it's next drawn. Returns the selected card.
fn jump_bar(
    ui: &mut Ui,
    inspector: &mut Inspector,
    id: ThingId,
    kind: &dyn bpe_core::ThingKind,
    thing: &bpe_core::Thing,
) -> Card {
    let scrolled = ui.input(|i| i.smooth_scroll_delta.y != 0.0) && ui.rect_contains_pointer(ui.max_rect());
    if scrolled {
        inspector.jumped = None;
    }
    let at_top = marked_card(inspector, id);
    // Placement has no button: it sits just under the thing's own card. The root's own
    // settings are six cards, each with its own button.
    let own: Vec<(Card, &str)> = if id == ThingId::ROOT {
        bpe_kit::ROOT_CARDS.into_iter().map(|name| (Card::Section(name), name)).collect()
    } else {
        vec![(Card::Thing, "Thing")]
    };
    let cards = own.into_iter().chain(
        SlotId::ALL.into_iter().filter(|s| thing.slots.contains_key(s)).map(|s| (Card::Slot(s), kind.slot_label(s))),
    );
    ui.horizontal_wrapped(|ui| {
        for (card, label) in cards {
            if ui.selectable_label(card == at_top, label).clicked() {
                inspector.collapsed.remove(&card.fold_key(id));
                inspector.jump_to = Some(card);
                inspector.jumped = Some((id, card));
            }
        }
    });
    ui.add_space(4.0);
    at_top
}

/// The card the jump bar marks on `id`: the one last jumped to, else the one at the top.
fn marked_card(inspector: &Inspector, id: ThingId) -> Card {
    match inspector.jumped {
        Some((thing, card)) if thing == id => card,
        _ => card_at_top(&inspector.card_tops),
    }
}

/// The last card whose header is at or above the scroll area's top, or the first card drawn
/// when none is. Placement counts as part of the thing's own card.
fn card_at_top(tops: &[(Card, f32)]) -> Card {
    let first = tops.first().map_or(Card::Thing, |(c, _)| *c);
    match tops.iter().rfind(|(_, top)| *top <= 1.0).map_or(first, |(c, _)| *c) {
        Card::Placement => Card::Thing,
        card => card,
    }
}

/// Notes where a card's header was drawn, and scrolls it to the top when the jump bar asked to.
fn card_header(ui: &Ui, inspector: &mut Inspector, card: Card, header: &egui::Response) {
    inspector.card_tops.push((card, header.rect.top() - ui.clip_rect().top()));
    if inspector.jump_to == Some(card) {
        // egui leaves the item spacing above a rect scrolled to the top; shifting the rect down
        // by that much puts the header's top right at the scroll area's top.
        let gap = ui.spacing().item_spacing.y;
        ui.scroll_to_rect(header.rect.translate(egui::vec2(0.0, gap)), Some(egui::Align::TOP));
        inspector.jump_to = None;
    }
}

/// The space inside a card's edge.
const CARD_MARGIN: i8 = 10;

fn card(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) {
    egui::Frame::new().fill(theme::CARD).corner_radius(8).inner_margin(CARD_MARGIN).show(ui, |ui| {
        // As wide as the scroll area shows, less the margin: a card above that couldn't fit
        // stretches the scroll area's insides, and the cards under it mustn't follow.
        let shown = ui.clip_rect().right() - f32::from(CARD_MARGIN) - ui.max_rect().left();
        ui.set_width(ui.available_width().min(shown).max(0.0));
        add_contents(ui);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Resolved, Settings, SlotId, SlotLink, Value, resolve};
    use bpe_kit::resolve_scene;
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
        time: f64,
    }

    fn harness(selected: Option<ThingId>) -> Harness<'static, State> {
        harness_of(bpe_kit::new_scene(&bpe_kit::registry()), selected)
    }

    fn harness_of(scene: Scene, selected: Option<ThingId>) -> Harness<'static, State> {
        harness_sized(scene, selected, 4000.0)
    }

    fn harness_sized(scene: Scene, selected: Option<ThingId>, height: f32) -> Harness<'static, State> {
        let kit = bpe_kit::kit();
        let resolved = resolve_scene(&scene, &kit.registry);
        let mut selection = Selection::default();
        if let Some(id) = selected {
            selection.set(id);
        }
        let state = State { inspector: Inspector::default(), scene, kit, resolved, selection, time: 0.0 };
        let mut h = Harness::builder()
            .with_size(egui::vec2(420.0, height))
            .with_step_dt(1.0 / 60.0)
            .with_max_steps(120)
            .allow_missing_glyphs()
            .with_context_setup(crate::theme::apply)
            .build_ui_state(
                |ui, s: &mut State| {
                    if show(ui, &mut s.inspector, &mut s.scene, &s.kit, &s.resolved, &s.selection, s.time) {
                        s.resolved = resolve_scene(&s.scene, &s.kit.registry);
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
    fn a_balls_colours_are_one_list() {
        let mut h = harness(Some(ThingId(2)));
        let (module, _) = ball_look(h.state());
        assert!(h.query_by_label("Colour").is_none(), "no separate Colour row");
        h.get_by_label("Add a colour").click();
        h.run();
        let m = &h.state().scene.library.modules[&module];
        assert!(matches!(m.params.get("colours"), Some(Value::Gradient(g)) if g.stops.len() == 2), "{:?}", m.params);
    }

    #[test]
    fn a_palette_from_the_menu_sets_the_colours() {
        let mut h = harness(Some(ThingId(2)));
        let (module, _) = ball_look(h.state());
        h.get_by_label("Colours palette").click();
        h.run();
        h.get_by_label("Ocean").click();
        h.run();
        let ocean = bpe_kit::paint::palettes::built_in().into_iter().find(|p| p.name == "Ocean").unwrap().colours;
        let m = &h.state().scene.library.modules[&module];
        assert_eq!(m.params.get("colours"), Some(&Value::Gradient(ocean)), "{:?}", m.params);
    }

    #[test]
    fn keyed_gradient_edit_writes_a_key() {
        let mut h = harness(Some(ThingId(2)));
        let (module, _) = ball_look(h.state());
        let colours = Address { owner: Owner::Module(module), slot: None, key: "colours".into() };
        let red_blue = bpe_core::Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)]);
        h.state_mut()
            .scene
            .set_key(&bpe_kit::kit().registry, &colours, 0.0, Value::Gradient(red_blue.clone()))
            .unwrap();
        h.state_mut().time = 1.0;
        h.run();
        h.get_by_label("Colours bar").click();
        h.run();
        let s = h.state();
        let keys = &s.scene.tracks[&colours].keys;
        assert_eq!(keys.len(), 2, "{keys:?}");
        assert_eq!(keys[0].value, Value::Gradient(red_blue), "the first key is left alone");
        assert_eq!(keys[1].time, 1.0);
        let Value::Gradient(g) = &keys[1].value else { panic!("{:?}", keys[1].value) };
        assert_eq!(g.stops.len(), 3, "the key holds the whole new gradient: {g:?}");
        assert!((g.stops[1].at - 0.5).abs() < 0.01, "{g:?}");
        assert!(s.inspector.keyed_edit);
    }

    #[test]
    fn a_paint_folds_under_its_heading() {
        let mut h = harness(Some(ThingId(2)));
        assert!(h.query_by_label("Add a colour").is_some());
        h.get_by_label("Hide Across the group").click();
        h.run();
        assert!(h.query_by_label("Add a colour").is_none(), "the paint's rows fold away");
        assert!(h.query_by_role_and_label(Role::Label, "Glow").is_some(), "rows outside the group stay");
        h.get_by_label("Show Across the group").click();
        h.run();
        assert!(h.query_by_label("Add a colour").is_some());
    }

    #[test]
    fn a_setting_can_be_locked_from_its_menu_and_shows_a_padlock() {
        let mut h = harness(Some(ThingId(2)));
        let size = Address { owner: Owner::Thing(ThingId(2)), slot: None, key: "radius".into() };
        assert!(h.query_by_label("Locked when randomizing").is_none());
        h.get_by_role_and_label(Role::Label, "Size").click_secondary();
        h.run();
        h.get_by_label("Lock when randomizing").click();
        h.run();
        assert!(h.state().scene.is_locked(&size));
        assert!(h.query_by_label("Locked when randomizing").is_some(), "a padlock shows on the row");
        h.get_by_role_and_label(Role::Label, "Size").click_secondary();
        h.run();
        assert!(h.query_by_label("Lock when randomizing").is_none());
        h.get_by_label("Unlock").click();
        h.run();
        assert!(!h.state().scene.is_locked(&size));
        assert!(h.query_by_label("Locked when randomizing").is_none());
        // A module card's row locks the module's setting, since the ball is its only user.
        h.get_by_role_and_label(Role::Label, "Glow").click_secondary();
        h.run();
        h.get_by_label("Lock when randomizing").click();
        h.run();
        let (module, _) = ball_look(h.state());
        let glow = Address { owner: Owner::Module(module), slot: None, key: "glow".into() };
        assert!(h.state().scene.is_locked(&glow));
    }

    #[test]
    fn the_card_die_rolls_only_that_card() {
        let mut h = harness(Some(ThingId(2)));
        let (module, _) = ball_look(h.state());
        let look_before = h.state().scene.library.modules[&module].params.clone();
        assert_eq!(h.state().scene.things[&ThingId(2)].params.get("radius"), None, "the size starts at its default");
        // The ball has a die on its own settings and on each of its own cards.
        assert_eq!(h.get_all_by_label("Randomize these settings").count(), 4);
        h.get_all_by_label("Randomize these settings").next().expect("the kind's own die").click();
        h.run();
        let s = h.state();
        assert!(s.scene.things[&ThingId(2)].params.get("radius").is_some(), "the size rolled");
        assert_eq!(s.scene.library.modules[&module].params, look_before, "the look card was not rolled");
        h.get_all_by_label("Randomize these settings").nth(1).expect("the look card's die").click();
        h.run();
        assert_ne!(h.state().scene.library.modules[&module].params, look_before, "the look module rolled");
    }

    #[test]
    fn every_setting_that_applies_shows_and_a_card_folds_away() {
        let mut h = harness(Some(ThingId(2)));
        // A single ball: its own Launch speed shows, the block's rows and columns don't.
        assert!(h.query_by_role_and_label(Role::Label, "Launch speed").is_some());
        assert!(h.query_by_role_and_label(Role::Label, "Columns").is_none(), "a single ball has no columns");
        assert!(h.query_by_role_and_label(Role::Label, "Friction").is_some(), "settings once under More show too");
        let before = h.get_all_by_role(Role::Slider).count();
        h.get_by_label("Hide Balls").click();
        h.run();
        assert!(h.query_by_role_and_label(Role::Label, "Launch speed").is_none());
        assert!(h.get_all_by_role(Role::Slider).count() < before);
        assert!(h.state().inspector.collapsed.contains(&(ThingId(2), None, "")));
        h.get_by_label("Show Balls").click();
        h.run();
        assert!(h.query_by_role_and_label(Role::Label, "Launch speed").is_some());
    }

    #[test]
    fn an_arena_shows_only_what_does_something_for_it() {
        let h = harness(Some(ThingId(1)));
        // The arena's bounce only matters when it takes over; ball-only settings never show.
        assert!(h.query_by_role_and_label(Role::Label, "Takes over").is_some());
        for ball_only in ["Bounciness", "Grow per hit", "Hits other balls", "Pops after", "Flash"] {
            assert!(h.query_by_role_and_label(Role::Label, ball_only).is_none(), "{ball_only} shows on the arena");
        }
        // A circle has a radius but no width or corners.
        assert!(h.query_by_role_and_label(Role::Label, "Radius").is_some());
        assert!(h.query_by_role_and_label(Role::Label, "Corner radius").is_none());
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
    fn clicking_a_diamond_adds_a_key_at_the_playhead_and_again_removes_it() {
        let mut h = harness(Some(ThingId(2)));
        h.state_mut().time = 2.0;
        h.run();
        h.get_by_label("Keyframe Size").click();
        h.run();
        let size = Address { owner: Owner::Thing(ThingId(2)), slot: None, key: "radius".into() };
        let keys = &h.state().scene.tracks[&size].keys;
        assert_eq!(keys.len(), 1);
        assert_eq!((keys[0].time, &keys[0].value), (2.0, &Value::Number(24.0)));
        h.get_by_label("Keyframe Size").click();
        h.run();
        assert!(!h.state().scene.tracks.contains_key(&size));
        // Size can't pulse, so its diamond has no menu; the scene's bloom can.
        h.get_by_label("Keyframe Size").click_secondary();
        h.run();
        assert!(h.query_by_label("Pulse with hits").is_none());
        h.state_mut().selection.set(ThingId::ROOT);
        h.run();
        h.get_by_label("Keyframe Bloom").click_secondary();
        h.run();
        h.get_by_label("Pulse with hits").click();
        h.run();
        let bloom = Address { owner: Owner::Thing(ThingId::ROOT), slot: None, key: "bloom".into() };
        let pulse = &h.state().scene.pulses[&bloom];
        assert_eq!((pulse.source, &pulse.amount), (PulseSource::Hits, &Value::Number(0.25)));
        let gravity = bpe_kit::SceneSettings::defs().iter().find(|d| d.key == "gravity").unwrap();
        assert_eq!(pulse_amount(gravity), Value::Vec2([0.0, 125.0]), "gravity");
        h.get_by_label("Keyframe Bloom").click_secondary();
        h.run();
        h.get_by_label("No pulse").click();
        h.run();
        assert!(h.state().scene.pulses.is_empty());
    }

    #[test]
    fn editing_a_keyed_slider_writes_a_key_not_the_base_value() {
        let mut h = harness(Some(ThingId(2)));
        let size = Address { owner: Owner::Thing(ThingId(2)), slot: None, key: "radius".into() };
        h.state_mut().scene.set_key(&bpe_kit::kit().registry, &size, 0.0, Value::Number(40.0)).unwrap();
        h.state_mut().time = 3.0;
        h.run();
        let rect = slider_rect(&h, "Size");
        drag_slider(&mut h, rect, 0.9);
        let s = h.state();
        assert_eq!(s.scene.things[&ThingId(2)].params.get("radius"), None, "the plain value is left alone");
        let keys = &s.scene.tracks[&size].keys;
        assert_eq!(keys.len(), 2, "{keys:?}");
        assert_eq!(keys[1].time, 3.0);
        assert!(matches!(keys[1].value, Value::Number(r) if r > 100.0), "{:?}", keys[1].value);
        assert!(s.inspector.keyed_edit, "the studio pauses on this, so the drag keys one moment");
    }

    #[test]
    fn the_module_picker_can_remove_a_slot_and_pick_a_module_again() {
        let mut h = harness(Some(ThingId(2)));
        h.get_by_label("Ball look").click();
        h.run();
        h.get_by_label("Remove").click();
        h.run();
        assert_eq!(h.state().scene.things[&ThingId(2)].slots[&SlotId::Look], SlotLink::Off);
        h.get_by_label("Removed").click();
        h.run();
        h.get_by_label("Ball look").click();
        h.run();
        let look = linked(&h.state().scene, ThingId(2), SlotId::Look);
        assert_eq!(h.state().scene.library.modules[&look].name, "Ball look");
    }

    #[test]
    fn a_new_module_copies_the_current_one() {
        let mut h = harness(Some(ThingId(1)));
        let starter = linked(&h.state().scene, ThingId(1), SlotId::Look);
        h.get_by_label("Arena look").click();
        h.run();
        // One Look kind fits an arena, so the menu makes the module straight away.
        h.get_by_label("New Look module…").click();
        h.run();
        let s = h.state();
        let module = linked(&s.scene, ThingId(1), SlotId::Look);
        assert_ne!(module, starter);
        assert_eq!(s.scene.library.modules[&module].name, "Arena Look");
        assert_eq!(s.scene.library.modules[&module].kind, "look.basic");
        assert_eq!(s.scene.library.modules[&module].params, s.scene.library.modules[&starter].params);
    }

    #[test]
    fn a_new_module_for_an_added_slot_is_made_from_its_picker() {
        let mut h = harness(Some(ThingId(2)));
        h.get_by_label("+ Add slot").click();
        h.run();
        h.get_by_label("Particles").click();
        h.run();
        // Added, the slot gets the ball's own starter; Particles has one kind, so its picker has
        // a plain button that makes another straight away.
        h.get_by_label("Ball particles").click();
        h.run();
        h.get_by_label("New Particles module…").click();
        h.run();
        let s = h.state();
        let SlotLink::Linked { module, .. } = &s.scene.things[&ThingId(2)].slots[&SlotId::Particles] else { panic!() };
        assert_eq!(s.scene.library.modules[module].kind, "particles.burst");
        assert_eq!(s.scene.library.modules[module].name, "Ball Particles");
    }

    #[test]
    fn add_slot_lists_only_slots_the_kind_lacks_and_adds_a_starter() {
        let mut h = harness(Some(ThingId(2)));
        // Trail, Particles and Shader have module kinds and aren't shown on a new ball, so there
        // is something to add.
        assert!(!h.get_by_label("+ Add slot").accesskit_node().is_disabled());
        h.get_by_label("+ Add slot").click();
        h.run();
        assert!(h.query_by_label("Particles").is_some());
        h.state_mut().scene.things.get_mut(&ThingId(2)).unwrap().slots.remove(&SlotId::Motion);
        h.run();
        h.get_by_label("+ Add slot").click();
        h.run();
        assert!(h.query_by_label("Trail").is_some());
        assert!(h.query_by_label("Shader").is_some());
        // The menu's Motion, not the look's Motion heading.
        h.get_all_by_label("Motion").last().expect("the Motion slot").click();
        h.run();
        let s = h.state();
        let motion = linked(&s.scene, ThingId(2), SlotId::Motion);
        assert_eq!(s.scene.library.modules[&motion].owner, Some(ThingId(2)));
        assert_eq!(s.scene.library.modules[&motion].kind, "motion.basic");
    }

    #[test]
    fn the_new_module_menu_of_a_one_kind_slot_makes_the_module_at_once() {
        let mut h = harness(Some(ThingId(2)));
        h.get_by_label("Ball look").click();
        h.run();
        assert!(h.query_by_label("New Look module…").is_some());
        h.key_press(egui::Key::Escape);
        h.run();
        // Bounce has one kind, so its menu keeps the plain button that makes the module at once.
        h.get_by_label("Ball bounce").click();
        h.run();
        h.get_by_label("New Bounce module…").click();
        h.run();
        let s = h.state();
        let SlotLink::Linked { module, .. } = &s.scene.things[&ThingId(2)].slots[&SlotId::Bounce] else { panic!() };
        assert_eq!(s.scene.library.modules[module].kind, "bounce.basic");
        assert_eq!(s.scene.library.modules[module].name, "Ball Bounce");
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
        let motion = linked(&scene, ThingId(2), SlotId::Motion);
        assert_eq!(write_target(&scene, &resolved, ThingId(2), SlotId::Motion), Some(Write::Module(motion)));
        let (followed, folder, folder_look) = ball_in_a_folder();
        let followed_resolved = resolve(&followed, &kit.registry);
        assert_eq!(
            write_target(&followed, &followed_resolved, ThingId(2), SlotId::Look),
            Some(Write::Inherited { from: folder, module: folder_look })
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

    /// The module `id` links in `slot`.
    fn linked(scene: &Scene, id: ThingId, slot: SlotId) -> ModuleId {
        match &scene.things[&id].slots[&slot] {
            SlotLink::Linked { module, .. } => *module,
            other => panic!("{slot:?} is linked: {other:?}"),
        }
    }

    /// A new scene with the ball in a folder whose own look the ball follows.
    fn ball_in_a_folder() -> (Scene, ThingId, ModuleId) {
        let reg = bpe_kit::registry();
        let mut scene = bpe_kit::new_scene(&reg);
        let folder = bpe_kit::add_thing(&mut scene, &reg, "folder", ThingId::ROOT).unwrap();
        let look = scene.add_module(&reg, "look.basic", "Folder look").unwrap();
        scene.library.modules.get_mut(&look).unwrap().params.set("glow", Value::Number(1.25));
        scene.link(&reg, folder, SlotId::Look, look).unwrap();
        scene.set_parent(ThingId(2), folder, bpe_core::Attach::Group).unwrap();
        scene.set_slot_inherit(ThingId(2), SlotId::Look);
        (scene, folder, look)
    }

    #[test]
    fn a_new_balls_card_edits_straight_away() {
        let mut h = harness(Some(ThingId(2)));
        assert!(h.query_by_label("Change here").is_none());
        assert!(h.query_by_label("Make own").is_none());
        let motion = linked(&h.state().scene, ThingId(2), SlotId::Motion);
        assert_eq!(h.state().scene.library.modules[&motion].name, "Ball motion");
        let before = h.state().scene.library.modules[&motion].params.clone();
        let rect = slider_rect(&h, "Air drag");
        drag_slider(&mut h, rect, 0.9);
        let s = h.state();
        assert_ne!(s.scene.library.modules[&motion].params, before, "the ball's own motion changed");
        assert_eq!(linked(&s.scene, ThingId(2), SlotId::Motion), motion);
    }

    #[test]
    fn typing_a_new_name_renames_the_things_starters() {
        let mut h = harness(Some(ThingId(2)));
        let field = h.get_by_label("Name");
        field.click();
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.event(egui::Event::Text("Hero".into()));
        h.run();
        let s = h.state();
        assert_eq!(s.scene.things[&ThingId(2)].name, "Hero");
        let look = linked(&s.scene, ThingId(2), SlotId::Look);
        assert_eq!(s.scene.library.modules[&look].name, "Hero look");
    }

    #[test]
    fn follow_folder_is_offered_only_inside_a_filled_folder() {
        let mut h = harness(Some(ThingId(2)));
        h.get_by_label("Ball look").click();
        h.run();
        assert!(h.query_by_label("Follow folder").is_none(), "no folder above the ball");
        assert!(h.query_by_label("Inherit").is_none());
        assert!(h.query_by_label("Remove").is_some());
        let (mut scene, folder, _) = ball_in_a_folder();
        let reg = bpe_kit::registry();
        let own = scene.make_own(&reg, ThingId(2), SlotId::Look).unwrap();
        let mut h = harness_of(scene, Some(ThingId(2)));
        h.get_by_label("Ball look").click();
        h.run();
        h.get_by_label("Follow folder").click();
        h.run();
        let s = h.state();
        assert_eq!(s.scene.things[&ThingId(2)].slots[&SlotId::Look], SlotLink::Inherit);
        assert!(s.scene.library.modules.contains_key(&own));
        let name = &s.scene.things[&folder].name;
        assert!(h.query_by_label(&format!("Follow folder: {name}")).is_some());
    }

    #[test]
    fn make_own_gives_a_followed_card_its_own_module() {
        let (scene, _, folder_look) = ball_in_a_folder();
        let mut h = harness_of(scene, Some(ThingId(2)));
        assert!(h.get_by_role_and_label(Role::Slider, "Glow").accesskit_node().is_disabled(), "followed is read-only");
        h.get_by_label("Make own").hover();
        h.run();
        h.get_by_label("Make own").click();
        h.run();
        let s = h.state();
        let own = linked(&s.scene, ThingId(2), SlotId::Look);
        assert_ne!(own, folder_look);
        let m = &s.scene.library.modules[&own];
        assert_eq!((m.name.as_str(), m.owner), ("Ball look", Some(ThingId(2))));
        assert_eq!(m.params, s.scene.library.modules[&folder_look].params);
        assert!(!h.get_by_role_and_label(Role::Slider, "Glow").accesskit_node().is_disabled());
    }

    #[test]
    fn removing_a_folders_look_leaves_its_followers_looking_the_same() {
        let (scene, folder, folder_look) = ball_in_a_folder();
        let mut h = harness_of(scene, Some(folder));
        h.get_by_label("Folder look").click();
        h.run();
        h.get_by_label("Remove").click();
        h.run();
        let s = h.state();
        assert_eq!(s.scene.things[&folder].slots[&SlotId::Look], SlotLink::Off);
        let own = linked(&s.scene, ThingId(2), SlotId::Look);
        assert_ne!(own, folder_look);
        assert_eq!(s.scene.library.modules[&own].params, s.scene.library.modules[&folder_look].params);
    }

    #[test]
    fn make_unique_is_in_the_picker() {
        let mut h = harness(Some(ThingId(2)));
        let before = h.state().scene.library.modules.len();
        h.get_by_label("Ball look").click();
        h.run();
        h.get_by_label("Make unique").click();
        h.run();
        let s = h.state();
        assert_eq!(s.scene.library.modules.len(), before + 1);
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
            let c = bpe_kit::add_thing(&mut s.scene, &s.kit.registry, "balls", ThingId::ROOT).unwrap();
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
            // The arena shares its look with a second arena; the ball has its own module.
            let arena_look = linked(&s.scene, ThingId(1), SlotId::Look);
            let other = bpe_kit::add_thing(&mut s.scene, &s.kit.registry, "arena", ThingId::ROOT).unwrap();
            s.scene.link(&s.kit.registry, other, SlotId::Look, arena_look).unwrap();
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
        // The arena shares its look, so it changed glow on top of it.
        let SlotLink::Linked { module: arena_module, overrides } = &s.scene.things[&ThingId(1)].slots[&SlotId::Look]
        else {
            panic!("the arena links its look")
        };
        assert!(matches!(overrides.get("glow"), Some(Value::Number(g)) if *g > 1.0), "{overrides:?}");
        assert!(s.scene.library.modules[arena_module].params.get("glow").is_none(), "the shared look is untouched");
    }

    #[test]
    fn the_attach_rows_ride_and_regroup() {
        // The tree moves things (Move into, or a drag); the Placement card hangs them off it.
        let mut scene = bpe_kit::new_scene(&bpe_kit::registry());
        scene.set_parent(ThingId(2), ThingId(1), Attach::Group).unwrap();
        let mut h = harness_of(scene, Some(ThingId(2)));
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
        // The bounce card's "Which balls" target shows once the balls hit each other.
        {
            let s = h.state_mut();
            // Give the ball its own bounce so the card is editable.
            let m = s.scene.add_module(&s.kit.registry, "bounce.basic", "Own bounce").unwrap();
            s.scene.link(&s.kit.registry, ThingId(2), SlotId::Bounce, m).unwrap();
            let hits = Address { owner: Owner::Module(m), slot: None, key: "hit_balls".into() };
            s.scene.set_at(&s.kit.registry, &hits, Value::Bool(true)).unwrap();
            s.resolved = resolve(&s.scene, &s.kit.registry);
        }
        h.run();
        // Other balls starts folded, marked as holding the change.
        h.get_by_label("Other balls (changed)").click();
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

    #[test]
    fn scene_settings_fits_the_panel() {
        // Long labels and units once pushed the number boxes past the edge, so the panel grew.
        let h = harness(Some(ThingId::ROOT));
        for n in h.get_all_by_role(Role::SpinButton) {
            assert!(n.rect().right() <= 420.0, "{:?} reaches {}", n.accesskit_node().label(), n.rect().right());
        }
    }

    #[test]
    fn target_settings_offer_the_scene_things_and_tags() {
        let reg = bpe_kit::registry();
        let mut scene = bpe_kit::new_scene(&reg);
        scene.things.get_mut(&ThingId(2)).unwrap().tags.insert("bouncers".into());
        let bounce = linked(&scene, ThingId(2), SlotId::Bounce);
        let special = Address { owner: Owner::Module(bounce), slot: None, key: "special".into() };
        scene.set_at(&reg, &special, Value::Bool(true)).unwrap();
        let mut h = harness_of(scene, Some(ThingId(2)));
        h.get_by_label("Show Against some things").click();
        h.run();
        h.get_by_role_and_label(Role::Button, "Those things").click();
        h.run();
        // The Target menu's Thing, not the jump bar's.
        h.get_all_by_label("Thing").last().expect("the Thing submenu").hover();
        h.run();
        assert!(h.query_by_label("Ball").is_some(), "the ball is offered by name");
        h.get_by_label("Tag").hover();
        h.run();
        h.get_by_label("bouncers").click();
        h.run();
        let target = h.state().scene.library.modules[&bounce].params.get("special_target").cloned();
        assert_eq!(target, Some(Value::Target(bpe_core::Target::Tag("bouncers".into()))));
    }

    /// The Colours row's name, after the Colours heading over it.
    fn colours_row<'h>(h: &'h Harness<'_, State>) -> egui_kittest::Node<'h> {
        h.get_all_by_role_and_label(Role::Label, "Colours").last().expect("a Colours row")
    }

    #[test]
    fn a_padlock_on_a_shared_card_holds_when_the_dice_roll() {
        use bpe_kit::random::{Roll, Scope, roll};
        let reg = bpe_kit::registry();
        let mut scene = bpe_kit::new_scene(&reg);
        let SlotLink::Linked { module, .. } = scene.things[&ThingId(2)].slots[&SlotId::Look].clone() else {
            panic!("the ball's look is linked");
        };
        // A second ball shares the look, so the first ball's look card edits its own changes.
        let other = bpe_kit::add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
        scene.link(&reg, other, SlotId::Look, module).unwrap();
        let mut h = harness_of(scene, Some(ThingId(2)));
        colours_row(&h).click_secondary();
        h.run();
        h.get_by_label("Lock when randomizing").click();
        h.run();
        assert!(h.query_by_label("Locked when randomizing").is_some(), "a padlock shows on the row");
        let colour = Address { owner: Owner::Module(module), slot: None, key: "colours".into() };
        let before = h.state().scene.value_at(&reg, &colour);
        // Neither a whole-scene roll nor this card's die moves the colour this ball shows.
        for (seed, scope) in [(7, Scope::Scene), (8, Scope::Card { thing: ThingId(2), slot: Some(SlotId::Look) })] {
            let mut scene = h.state().scene.clone();
            roll(&mut scene, &reg, &scope, Roll { seed, amount: 1.0 });
            let shown = Address { owner: Owner::Thing(ThingId(2)), slot: Some(SlotId::Look), key: "colours".into() };
            assert_eq!(scene.value_at(&reg, &shown), before, "{scope:?}");
        }
        // Unlocking from the same row lets it roll again.
        colours_row(&h).click_secondary();
        h.run();
        h.get_by_label("Unlock").click();
        h.run();
        assert!(h.query_by_label("Locked when randomizing").is_none());
        assert!(h.state().scene.locks.is_empty(), "{:?}", h.state().scene.locks);
    }

    #[test]
    fn look_card_shows_blending_and_variation() {
        let mut h = harness(Some(ThingId(2)));
        h.get_by_label("Ball look").click();
        h.run();
        assert!(h.query_all_by_label("Blending").next().is_some(), "a Blending group");
        h.get_all_by_label("Blending").next().unwrap().click();
        h.get_all_by_label("Variation").next().unwrap().click();
        h.run();
        assert!(h.query_all_by_label("Blend").next().is_some(), "the Blend setting");
        assert!(h.query_all_by_label("Blend amount").next().is_some(), "the Blend amount setting");
        assert!(h.query_all_by_label("Variation").next().is_some(), "a Variation group");
        assert!(h.query_all_by_label("Vary hue").next().is_some(), "the Vary hue setting");
    }

    #[test]
    fn variation_and_blending_start_folded_and_open_when_clicked() {
        let mut h = harness(Some(ThingId(2)));
        assert!(h.query_all_by_label("Variation").next().is_some(), "the Variation heading");
        assert!(h.query_all_by_label("Blending").next().is_some(), "the Blending heading");
        assert!(h.query_by_label("Vary hue").is_none(), "Variation starts folded");
        assert!(h.query_by_label("Blend amount").is_none(), "Blending starts folded");
        h.get_all_by_label("Variation").next().unwrap().click();
        h.run();
        assert!(h.query_all_by_label("Vary hue").next().is_some(), "a click opens it");
        assert!(h.query_by_label("Blend amount").is_none(), "Blending stays folded");
        // It stays open after looking at another thing and coming back.
        h.state_mut().selection.set(ThingId(1));
        h.run();
        h.state_mut().selection.set(ThingId(2));
        h.run();
        assert!(h.query_all_by_label("Vary hue").next().is_some());
        // And a click on the heading folds it again.
        h.get_all_by_label("Variation").next().unwrap().click();
        h.run();
        assert!(h.query_by_label("Vary hue").is_none());
    }

    #[test]
    fn a_folded_section_with_changes_shows_the_dot() {
        let mut h = harness(Some(ThingId(2)));
        assert!(h.query_all_by_label("Variation").next().is_some());
        assert!(h.query_by_label("Variation (changed)").is_none(), "nothing changed yet");
        assert!(h.query_by_label("Changes inside").is_none(), "no dot with defaults");
        let (module, _) = ball_look(h.state());
        h.state_mut().scene.library.modules.get_mut(&module).unwrap().params.set("vary_hue", Value::Number(20.0));
        h.run();
        assert!(h.query_by_label("Variation (changed)").is_some(), "the heading says it holds changes");
        assert!(h.query_by_label("Changes inside").is_some(), "the dot is drawn");
        // Open, the rows show for themselves, so the dot goes.
        h.get_by_label("Variation (changed)").click();
        h.run();
        assert!(h.query_by_label("Changes inside").is_none());
    }

    #[test]
    fn a_folded_section_with_a_keyframe_shows_the_dot() {
        let mut h = harness(Some(ThingId(2)));
        let (module, _) = ball_look(h.state());
        let wobble = Address { owner: Owner::Module(module), slot: None, key: "wobble".into() };
        let reg = bpe_kit::registry();
        let default = h.state().scene.value_at(&reg, &wobble).unwrap();
        h.state_mut().scene.set_key(&reg, &wobble, 0.0, default).unwrap();
        h.run();
        assert!(h.query_by_label("Variation (changed)").is_some(), "a key at the default still moves the video");
        assert!(h.query_by_label("Changes inside").is_some());
        assert!(h.query_by_label("Blending (changed)").is_none(), "only the section holding it");
    }

    #[test]
    fn a_folded_section_with_only_a_pulse_shows_the_dot() {
        let mut h = harness(Some(ThingId(2)));
        h.get_by_label("Hide Light").click();
        h.run();
        assert!(h.query_by_label("Light (changed)").is_none(), "Light holds no changes yet");
        let (module, _) = ball_look(h.state());
        let def = bpe_kit::modules::look::BasicLook::defs().iter().find(|d| d.key == "opacity").unwrap();
        assert!(def.flags.pulse && def.group == "Light");
        let address = Address { owner: Owner::Module(module), slot: None, key: def.key.into() };
        let reg = bpe_kit::registry();
        h.state_mut().scene.set_pulse(&reg, &address, bpe_core::PulseSource::Hits, pulse_amount(def), 0.25).unwrap();
        h.run();
        assert!(h.query_by_label("Light (changed)").is_some(), "a pulse moves the video");
        assert!(h.query_by_label("Changes inside").is_some());
    }

    #[test]
    fn the_header_eye_hides_and_shows_the_thing() {
        let mut h = harness(Some(ThingId(2)));
        assert!(h.state().scene.things[&ThingId(2)].enabled);
        h.get_by_label("Shown").click();
        h.run();
        assert!(!h.state().scene.things[&ThingId(2)].enabled, "the eye hides it");
        h.get_by_label("Shown").click();
        h.run();
        assert!(h.state().scene.things[&ThingId(2)].enabled, "and shows it again");
    }

    #[test]
    fn the_picture_card_shows_the_grade() {
        let reg = bpe_kit::registry();
        let mut h = harness_of(bpe_kit::scenes::graded_sunset(&reg), Some(ThingId::ROOT));
        // At the start the map amount is 0, yet the map still shows.
        h.run();
        let slider =
            |h: &Harness<'_, State>, label: &str| h.query_all_by_role_and_label(Role::Slider, label).count() > 0;
        assert!(h.query_by_role_and_label(Role::Label, "Picture").is_some(), "a Picture card");
        assert!(slider(&h, "Brightness"), "the Brightness setting");
        assert!(slider(&h, "Vignette"), "the Vignette setting");
        assert!(h.query_all_by_label("Gradient map").next().is_some(), "the Gradient map setting at t=0");
        // The card folds like the others.
        h.get_by_label("Hide Picture").click();
        h.run();
        assert!(!slider(&h, "Vignette"), "the grade's rows fold away");
        assert!(h.state().inspector.collapsed.contains(&(ThingId::ROOT, None, "Picture")));
        h.get_by_label("Show Picture").click();
        h.run();
        assert!(slider(&h, "Vignette"));
    }

    /// Opens the card's Type menu: the one Type button on the page.
    fn open_type_menu(h: &mut Harness<'_, State>) {
        h.get_by_role_and_label(Role::Button, "Type").click();
        h.run();
    }

    /// A new scene whose ball has its own effect in its Shader slot, with one of its number
    /// settings moved off its default, and that setting's key. The other effect kinds named
    /// come back too: those that lack the setting.
    fn ball_with_an_effect() -> (Scene, ModuleId, Address, Vec<&'static str>) {
        let reg = bpe_kit::registry();
        let mut scene = bpe_kit::new_scene(&reg);
        bpe_kit::fill_slot(&mut scene, &reg, ThingId(2), SlotId::Shader).unwrap();
        let effect = linked(&scene, ThingId(2), SlotId::Shader);
        let kind = reg.module(&scene.library.modules[&effect].kind).unwrap();
        let def = kind
            .settings()
            .iter()
            .find(|d| d.key != "blend" && matches!(d.default, Value::Number(_)))
            .expect("an effect with a number setting of its own");
        let (Value::Number(n), Some((low, high))) = (&def.default, def.range) else { unreachable!() };
        let a = Address { owner: Owner::Module(effect), slot: None, key: def.key.into() };
        scene.set_at(&reg, &a, Value::Number(if *n == low { high } else { low })).unwrap();
        let others = reg
            .modules_for(SlotId::Shader)
            .filter(|m| m.id() != kind.id() && m.settings().iter().all(|d| d.key != def.key))
            .map(|m| m.name())
            .collect();
        (scene, effect, a, others)
    }

    /// The name of `module`'s kind.
    fn kind_name(s: &State, module: ModuleId) -> &'static str {
        s.kit.registry.module(&s.scene.library.modules[&module].kind).unwrap().name()
    }

    #[test]
    fn an_effects_card_has_a_type_menu_and_a_balls_look_has_none() {
        let reg = bpe_kit::registry();
        let mut scene = bpe_kit::new_scene(&reg);
        // Only the ball's Look, Motion and Bounce: each has one kind.
        bpe_kit::fill_slot(&mut scene, &reg, ThingId::ROOT, SlotId::Shader).unwrap();
        let mut h = harness_of(scene, Some(ThingId(2)));
        assert!(h.query_by_label("Ball look").is_some(), "the look card shows its module");
        assert!(h.query_by_role_and_label(Role::Button, "Type").is_none(), "a ball's look has one kind");
        h.state_mut().selection.set(ThingId::ROOT);
        h.run();
        let root = reg.thing("scene").unwrap();
        let effects: Vec<&str> = reg.kinds_fitting(SlotId::Shader, root).map(|k| k.name()).collect();
        assert!(effects.len() > 1, "{effects:?}");
        let shader = linked(&h.state().scene, ThingId::ROOT, SlotId::Shader);
        let current = reg.module(&h.state().scene.library.modules[&shader].kind).unwrap().name();
        open_type_menu(&mut h);
        for name in &effects {
            assert!(h.query_all_by_label(name).next().is_some(), "{name} is offered");
        }
        let other = effects.iter().find(|n| **n != current).unwrap();
        h.get_all_by_label(other).last().unwrap().click();
        h.run();
        let s = h.state();
        assert_eq!(linked(&s.scene, ThingId::ROOT, SlotId::Shader), shader, "the same module, another kind");
        assert_eq!(reg.module(&s.scene.library.modules[&shader].kind).unwrap().name(), *other);
    }

    #[test]
    fn picking_a_type_switches_the_module_and_back_restores_it() {
        let (scene, effect, moved, others) = ball_with_an_effect();
        let reg = bpe_kit::registry();
        let value = scene.value_at(&reg, &moved);
        let mut h = harness_of(scene, Some(ThingId(2)));
        let first = kind_name(h.state(), effect);
        let other = *others.first().expect("an effect without the moved setting");
        open_type_menu(&mut h);
        h.get_all_by_label(other).last().unwrap().click();
        h.run();
        {
            let s = h.state();
            assert_eq!(linked(&s.scene, ThingId(2), SlotId::Shader), effect, "the slot keeps its module");
            assert_eq!(kind_name(s, effect), other);
            assert!(s.scene.library.modules[&effect].params.get(&moved.key).is_none());
        }
        open_type_menu(&mut h);
        h.get_all_by_label(first).last().unwrap().click();
        h.run();
        let s = h.state();
        assert_eq!(kind_name(s, effect), first);
        assert_eq!(s.scene.value_at(&s.kit.registry, &moved), value, "the moved setting is back");
    }

    #[test]
    fn undo_after_a_type_switch_restores_the_module() {
        let (scene, effect, _, others) = ball_with_an_effect();
        let before = scene.clone();
        let mut h = harness_of(scene, Some(ThingId(2)));
        let first = kind_name(h.state(), effect);
        open_type_menu(&mut h);
        h.get_all_by_label(others[0]).last().unwrap().click();
        h.run();
        assert_eq!(kind_name(h.state(), effect), others[0]);
        // The studio records any frame that changed the scene as an undo step.
        let mut history = crate::history::History::default();
        history.record(before.clone(), false);
        let s = h.state_mut();
        assert!(history.undo(&mut s.scene));
        assert_eq!(kind_name(s, effect), first);
        assert_eq!(s.scene, before);
        assert!(history.redo(&mut s.scene));
        assert_eq!(kind_name(s, effect), others[0]);
    }

    #[test]
    fn a_shared_modules_type_menu_says_how_many_change() {
        let (mut scene, effect, _, others) = ball_with_an_effect();
        let reg = bpe_kit::registry();
        let other = bpe_kit::add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
        scene.add_slot(&reg, other, SlotId::Shader).unwrap();
        scene.link(&reg, other, SlotId::Shader, effect).unwrap();
        let mut h = harness_of(scene, Some(ThingId(2)));
        open_type_menu(&mut h);
        let name = h.state().scene.library.modules[&effect].name.clone();
        assert!(h.query_by_label(&format!("Changes all 2 things using {name}")).is_some());
        h.get_all_by_label(others[0]).last().unwrap().click();
        h.run();
        let s = h.state();
        assert_eq!(kind_name(s, effect), others[0]);
        assert_eq!(linked(&s.scene, other, SlotId::Shader), effect, "both still share it");
    }

    #[test]
    fn a_type_switch_reaches_each_selected_module_once() {
        let (mut scene, mine, _, others) = ball_with_an_effect();
        let reg = bpe_kit::registry();
        let other = bpe_kit::add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
        let theirs = scene.add_module(&reg, &scene.library.modules[&mine].kind.clone(), "Their effect").unwrap();
        scene.add_slot(&reg, other, SlotId::Shader).unwrap();
        scene.link(&reg, other, SlotId::Shader, theirs).unwrap();
        let mut h = harness_of(scene, Some(ThingId(2)));
        h.state_mut().selection.toggle(other);
        h.run();
        open_type_menu(&mut h);
        h.get_all_by_label(others[0]).last().unwrap().click();
        h.run();
        let s = h.state();
        assert_eq!(kind_name(s, mine), others[0]);
        assert_eq!(kind_name(s, theirs), others[0]);
    }

    #[test]
    fn the_type_button_is_disabled_on_a_slot_that_follows_a_folder() {
        let reg = bpe_kit::registry();
        let mut scene = bpe_kit::new_scene(&reg);
        let folder = bpe_kit::add_thing(&mut scene, &reg, "folder", ThingId::ROOT).unwrap();
        let effect = scene.add_module(&reg, "shader.echo", "Folder effect").unwrap();
        scene.add_slot(&reg, folder, SlotId::Shader).unwrap();
        scene.link(&reg, folder, SlotId::Shader, effect).unwrap();
        scene.set_parent(ThingId(2), folder, bpe_core::Attach::Group).unwrap();
        scene.set_slot_inherit(ThingId(2), SlotId::Shader);
        let mut h = harness_of(scene, Some(ThingId(2)));
        let button = h.get_by_role_and_label(Role::Button, "Type");
        assert!(button.accesskit_node().is_disabled(), "an inherited slot's type is the folder's to change");
        let before = h.state().scene.clone();
        button.click();
        h.run();
        assert_eq!(h.state().scene, before);
    }

    #[test]
    fn the_header_is_name_kind_and_shown_then_tags() {
        let h = harness(Some(ThingId(2)));
        for label in ["Name", "Shown", "Tags"] {
            assert!(h.query_all_by_label(label).next().is_some(), "{label} is in the header");
        }
        for gone in ["Parent", "Kind"] {
            assert!(h.query_by_role_and_label(Role::Label, gone).is_none(), "{gone} is no longer a row");
        }
        // The Placement card's heading sits above its Position row.
        let heading = h.get_by_role_and_label(Role::Label, "Placement").rect();
        let position = h.get_by_role_and_label(Role::Label, "Position").rect();
        assert!(heading.top() < position.top(), "Position is under Placement");
        for row in ["Rotation", "Attach", "Stick to screen"] {
            let r = h.get_by_role_and_label(Role::Label, row).rect();
            assert!(heading.top() < r.top(), "{row} is under Placement");
        }
        // The kind's card is titled by the kind alone.
        assert!(h.query_by_label("Hide Balls").is_some());
        assert!(h.query_by_label_contains("Balls settings").is_none());
    }

    #[test]
    fn the_camera_and_scene_settings_have_no_placement_card() {
        let root = harness(Some(ThingId::ROOT));
        assert!(root.query_by_role_and_label(Role::Label, "Placement").is_none());
        assert!(root.query_by_role_and_label(Role::Label, "Shown").is_none(), "the root has no eye");
        assert!(root.query_by_label("Hide Video").is_some(), "the root's first card keeps its title");
        let reg = bpe_kit::registry();
        let mut scene = bpe_kit::new_scene(&reg);
        let camera = bpe_kit::add_thing(&mut scene, &reg, "camera", ThingId::ROOT).unwrap();
        let h = harness_of(scene, Some(camera));
        assert!(h.query_by_role_and_label(Role::Label, "Placement").is_none());
        assert!(h.query_by_role_and_label(Role::Label, "Stick to screen").is_none());
        assert!(h.query_all_by_label("Shown").next().is_some(), "the camera can still be hidden");
    }

    #[test]
    fn the_placement_card_folds_away() {
        let mut h = harness(Some(ThingId(2)));
        h.get_by_label("Hide Placement").click();
        h.run();
        assert!(h.query_by_role_and_label(Role::Label, "Rotation").is_none());
        assert!(h.state().inspector.collapsed.contains(&(ThingId(2), None, "placement")));
        h.get_by_label("Show Placement").click();
        h.run();
        assert!(h.query_by_role_and_label(Role::Label, "Rotation").is_some());
    }

    const BAR_LABELS: [&str; 12] = [
        "Thing",
        "Look",
        "Trail",
        "Motion",
        "Bounce",
        "Particles",
        "Shader",
        "Effects",
        "Video",
        "World",
        "Background",
        "Picture",
    ];

    /// The jump bar's buttons, in order, each with whether it is selected.
    fn jump_bar(h: &Harness<'_, State>) -> Vec<(String, bool)> {
        h.get_all_by_role(Role::Button)
            .filter_map(|n| {
                let node = n.accesskit_node();
                let label = node.label()?;
                let toggled = node.toggled()?;
                BAR_LABELS.contains(&label.as_str()).then(|| (label, toggled == egui::accesskit::Toggled::True))
            })
            .collect()
    }

    fn the_bar_button<'h>(h: &'h Harness<'_, State>, label: &'h str) -> egui_kittest::Node<'h> {
        h.get_all_by_role_and_label(Role::Button, label)
            .find(|n| n.accesskit_node().toggled().is_some())
            .expect("the bar's button")
    }

    fn a_ball_with_every_slot() -> Scene {
        let reg = bpe_kit::registry();
        let mut scene = bpe_kit::new_scene(&reg);
        for slot in [SlotId::Trail, SlotId::Particles, SlotId::Shader] {
            scene.add_slot(&reg, ThingId(2), slot).unwrap();
        }
        scene
    }

    #[test]
    fn slider_columns_line_up_across_cards() {
        let mut h = harness(Some(ThingId(2)));
        h.get_by_label("+ Add slot").click();
        h.run();
        h.get_by_label("Trail").click();
        h.run();
        let glow = slider_rect(&h, "Glow");
        let length = slider_rect(&h, "Length");
        assert!(
            (glow.left() - length.left()).abs() <= 1.0,
            "Glow starts at {}, Length at {}",
            glow.left(),
            length.left()
        );
    }

    #[test]
    fn the_header_and_placement_line_up_with_the_settings() {
        let h = harness(Some(ThingId(2)));
        let label = |l: &str| h.get_by_role_and_label(Role::Label, l).rect().left();
        let size = label("Size");
        for row in ["Name", "Tags", "Position", "Rotation"] {
            assert!((label(row) - size).abs() <= 1.0, "{row}'s label starts at {}, Size's at {size}", label(row));
        }
        let slider = slider_rect(&h, "Size").left();
        let name = h.get_by_role_and_label(Role::TextInput, "Name").rect().left();
        let position = h.get_all_by_role_and_label(Role::SpinButton, "Position").next().unwrap().rect().left();
        let rotation = h.get_by_role_and_label(Role::SpinButton, "Rotation").rect().left();
        for (row, x) in [("Name", name), ("Position", position), ("Rotation", rotation)] {
            assert!((x - slider).abs() <= 1.0, "{row}'s field starts at {x}, Size's slider at {slider}");
        }
    }

    #[test]
    fn the_jump_bar_lists_thing_then_the_slots_in_order() {
        let h = harness_of(a_ball_with_every_slot(), Some(ThingId(2)));
        let labels: Vec<String> = jump_bar(&h).into_iter().map(|(l, _)| l).collect();
        assert_eq!(labels, ["Thing", "Look", "Trail", "Motion", "Bounce", "Particles", "Shader"]);
        assert_eq!(jump_bar(&h)[0], ("Thing".to_owned(), true), "at the top, Thing is selected");
    }

    #[test]
    fn the_jump_bar_unfolds_and_scrolls_to_trail() {
        let mut h = harness_sized(a_ball_with_every_slot(), Some(ThingId(2)), 600.0);
        h.state_mut().inspector.collapsed.insert((ThingId(2), Some(SlotId::Trail), ""));
        h.run();
        assert!(h.query_by_label("Show Trail").is_some(), "the Trail card starts folded");
        the_bar_button(&h, "Trail").click();
        h.run();
        let s = h.state();
        let open = !s.inspector.collapsed.contains(&(ThingId(2), Some(SlotId::Trail), ""));
        assert!(open, "the Trail card is open");
        assert!(s.inspector.jump_to.is_none());
        let tops = &s.inspector.card_tops;
        let top = tops.iter().find(|(c, _)| *c == Card::Slot(SlotId::Trail)).expect("Trail' top").1;
        assert!(top.abs() <= 2.0, "Trail' header is at the top of the scroll area, at {top}");
        // The header's own rect sits just under the bar, where the scroll area starts.
        let header = h.get_by_label("Hide Trail").rect();
        let bar = the_bar_button(&h, "Thing").rect();
        assert!(header.top() > bar.bottom() && header.top() < bar.bottom() + 60.0, "{header:?} under {bar:?}");
        let selected: Vec<String> = jump_bar(&h).into_iter().filter(|(_, on)| *on).map(|(l, _)| l).collect();
        assert_eq!(selected, ["Trail"], "tops {:?} bar {:?}", h.state().inspector.card_tops, jump_bar(&h));
    }

    /// Scene settings with every slot it can have, so its cards run well past one screen.
    fn a_root_with_every_slot() -> Scene {
        let reg = bpe_kit::registry();
        let mut scene = bpe_kit::new_scene(&reg);
        scene.add_slot(&reg, ThingId::ROOT, SlotId::Shader).unwrap();
        scene
    }

    /// The top of a root card's strong title.
    fn heading_top(h: &Harness<'_, State>, name: &str) -> f32 {
        h.get_by_role_and_label(Role::Label, name).rect().top()
    }

    #[test]
    fn scene_settings_shows_a_card_per_group() {
        let h = harness(Some(ThingId::ROOT));
        let mut last = f32::NEG_INFINITY;
        for name in bpe_kit::ROOT_CARDS {
            assert!(h.query_by_label(&format!("Hide {name}")).is_some(), "{name} has a fold arrow");
            let top = heading_top(&h, name);
            assert!(top > last, "{name} comes after the card before it");
            last = top;
        }
        let tops: Vec<Card> = h.state().inspector.card_tops.iter().map(|(c, _)| *c).collect();
        assert_eq!(tops, bpe_kit::ROOT_CARDS.map(Card::Section), "the cards in order, and nothing else");
        assert!(h.query_by_label("Hide Scene settings").is_none(), "no single Scene settings card");
    }

    #[test]
    fn the_root_jump_bar_lists_the_cards_then_the_slots() {
        let h = harness(Some(ThingId::ROOT));
        let labels: Vec<String> = jump_bar(&h).into_iter().map(|(l, _)| l).collect();
        assert_eq!(labels, bpe_kit::ROOT_CARDS);
        assert_eq!(jump_bar(&h)[0], ("Video".to_owned(), true), "at the top, Video is selected");
        assert!(h.query_by_role_and_label(Role::Button, "Thing").is_none(), "no Thing button on the root");
        assert!(h.query_by_role_and_label(Role::Button, "Scene settings").is_none());
        // Its slots follow, in the order their cards are drawn.
        let h = harness_of(a_root_with_every_slot(), Some(ThingId::ROOT));
        let labels: Vec<String> = jump_bar(&h).into_iter().map(|(l, _)| l).collect();
        let mut want: Vec<&str> = bpe_kit::ROOT_CARDS.to_vec();
        want.push("Effects");
        assert_eq!(labels, want);
    }

    #[test]
    fn jumping_to_background_scrolls_it_to_the_top() {
        let mut h = harness_sized(a_root_with_every_slot(), Some(ThingId::ROOT), 600.0);
        the_bar_button(&h, "Background").click();
        h.run();
        let s = h.state();
        let tops = &s.inspector.card_tops;
        let top = tops.iter().find(|(c, _)| *c == Card::Section("Background")).expect("Background's top").1;
        assert!(top.abs() <= 1.0, "Background's header is at the top of the scroll area, at {top}");
        assert_eq!(selected_in_bar(&h), ["Background"]);
    }

    #[test]
    fn a_root_card_with_no_shown_rows_still_has_its_title() {
        let kit = bpe_kit::kit();
        let scene = bpe_kit::new_scene(&kit.registry);
        let resolved = resolve_scene(&scene, &kit.registry);
        let state =
            State { inspector: Inspector::default(), scene, kit, resolved, selection: Selection::default(), time: 0.0 };
        let mut h = Harness::builder()
            .with_size(egui::vec2(420.0, 800.0))
            .allow_missing_glyphs()
            .with_context_setup(crate::theme::apply)
            .build_ui_state(
                |ui, s: &mut State| {
                    let choices = TargetChoices::from_scene(&s.scene, &s.kit.registry);
                    let thing = s.scene.things[&ThingId::ROOT].clone();
                    let cx = OwnCx {
                        scene: &s.scene,
                        kit: &s.kit,
                        choices: &choices,
                        id: ThingId::ROOT,
                        thing: &thing,
                        time: s.time,
                        label_width: label_width(420.0),
                        applies: &|_| false,
                    };
                    let mut edits = Vec::new();
                    root_card(ui, &mut s.inspector, &cx, "World", &mut edits);
                },
                state,
            );
        h.run();
        assert!(h.query_by_label("Hide World").is_some(), "the card keeps its fold arrow");
        assert!(h.query_by_role_and_label(Role::Label, "World").is_some(), "and its title");
        assert!(h.query_by_role_and_label(Role::Label, "Gravity").is_none(), "no rows");
        assert!(h.query_by_role_and_label(Role::Label, "Seed").is_none(), "no rows");
        assert!(h.query_all_by_role(Role::Slider).next().is_none(), "no grid row is drawn");
    }

    #[test]
    fn background_card_dice_rolls_only_the_background() {
        let mut h = harness(Some(ThingId::ROOT));
        let headings: Vec<(&str, f32)> = bpe_kit::ROOT_CARDS.iter().map(|n| (*n, heading_top(&h, n))).collect();
        // Which card each dice button sits in: the last heading above it.
        let cards: Vec<&str> = h
            .get_all_by_role_and_label(Role::Button, "Randomize these settings")
            .map(|n| {
                let at = n.rect().center().y;
                headings.iter().rfind(|(_, top)| *top <= at + 12.0).expect("under a card").0
            })
            .collect();
        // The background's paint rolls its colours, so Background has dice too.
        assert_eq!(cards, ["Background", "Picture"], "only cards with something random have dice");
        let reg = bpe_kit::registry();
        let values = |s: &State| -> Vec<(&str, Value)> {
            bpe_kit::SceneSettings::defs()
                .iter()
                .map(|d| {
                    let a = Address { owner: Owner::Thing(ThingId::ROOT), slot: None, key: d.key.into() };
                    (d.key, s.scene.value_at(&reg, &a).unwrap())
                })
                .collect()
        };
        let before = values(h.state());
        let background = cards.iter().position(|c| *c == "Background").unwrap();
        h.get_all_by_role_and_label(Role::Button, "Randomize these settings").nth(background).unwrap().click();
        h.run();
        let after = values(h.state());
        let changed: Vec<&str> = before.iter().zip(&after).filter(|(b, a)| b != a).map(|(b, _)| b.0).collect();
        let groups: Vec<&str> = changed
            .iter()
            .map(|k| bpe_kit::SceneSettings::defs().iter().find(|d| d.key == *k).unwrap().group)
            .collect();
        assert!(!groups.is_empty() && groups.iter().all(|g| *g == "Background"), "{changed:?}");
    }

    fn selected_in_bar(h: &Harness<'_, State>) -> Vec<String> {
        jump_bar(h).into_iter().filter(|(_, on)| *on).map(|(l, _)| l).collect()
    }

    fn wheel(h: &mut Harness<'_, State>, dy: f32) {
        h.hover_at(egui::pos2(200.0, 400.0));
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, dy),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
            modifiers: Modifiers::NONE,
        });
        h.run();
    }

    #[test]
    fn a_short_last_card_stays_marked_after_jumping_to_it() {
        let mut h = harness_sized(a_ball_with_every_slot(), Some(ThingId(2)), 600.0);
        the_bar_button(&h, "Shader").click();
        h.run();
        let tops = &h.state().inspector.card_tops;
        let shader = tops.iter().find(|(c, _)| *c == Card::Slot(SlotId::Shader)).expect("Shader's top").1;
        assert!(shader > 1.0, "Shader is too short to reach the top, at {shader}");
        assert_eq!(selected_in_bar(&h), ["Shader"], "the card jumped to stays marked");
        // A scroll hands the mark back to the card at the top.
        wheel(&mut h, 40.0);
        assert_ne!(selected_in_bar(&h), ["Shader"]);
        assert!(h.state().inspector.jumped.is_none());
    }

    #[test]
    fn the_jump_bar_marks_the_card_at_the_top_while_scrolling() {
        let mut h = harness_sized(a_ball_with_every_slot(), Some(ThingId(2)), 600.0);
        // Scroll down until Look's header has passed the top but Trail's hasn't.
        for _ in 0..200 {
            let tops = &h.state().inspector.card_tops;
            let top_of = |card| tops.iter().find(|(c, _)| *c == card).map(|(_, t)| *t);
            if top_of(Card::Slot(SlotId::Look)).is_some_and(|t| t < -1.0) {
                break;
            }
            h.hover_at(egui::pos2(200.0, 400.0));
            h.event(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -40.0),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Unknown,
                modifiers: Modifiers::NONE,
            });
            h.run();
        }
        let tops = &h.state().inspector.card_tops;
        let look = tops.iter().find(|(c, _)| *c == Card::Slot(SlotId::Look)).unwrap().1;
        let trail = tops.iter().find(|(c, _)| *c == Card::Slot(SlotId::Trail)).unwrap().1;
        assert!(look < -1.0 && trail > 1.0, "scrolled past Look's header but not Trail's: {look}, {trail}");
        let selected: Vec<String> = jump_bar(&h).into_iter().filter(|(_, on)| *on).map(|(l, _)| l).collect();
        assert_eq!(selected, ["Look"]);
    }
}

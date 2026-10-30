//! The inspector: the selected thing's header, how it hangs in the tree, its kind's settings and
//! one card per slot. A shared module's card edits this thing's own overrides; an inherited card
//! is read-only until the thing takes its own link.

use crate::chords::{self, Audition, ChordClip, ChordUi};
use crate::dice::dice_button;
use crate::instruments::InstrumentEdit;
use crate::nodes::cards;
use crate::selection::Selection;
use crate::theme;
use crate::widgets::{
    BUILT_IN_BASE, COLOUR, COLOURS, ColoursEdit, GridCx, KeyMark, ListChange, MAKE_UNIQUE, MAKE_UNIQUE_INSTRUMENT,
    NEW_PATCH, NEW_PROGRESSION, STARTER_BASE, TargetChoices, plays_built_in, settings_grid, settings_grid_marked,
};
use bpe_core::{
    Address, Attach, Chord, Color, Depth, InstrumentId, InstrumentRef, InstrumentSource, ModuleGraph, ModuleId,
    ModuleKind, Owner, Progression, ProgressionId, ProgressionRef, PulseSource, Resolved, SAME_TIME, Scene, SettingDef,
    SettingValue, SlotId, SlotLink, ThingId, Transform, Value, ValueType,
};
use bpe_kit::Kit;
use bpe_kit::graphs::{bound_keys, module_graph};
use eframe::egui::{self, DragValue, RichText, Ui};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub struct Inspector {
    /// The cards folded shut, by thing: its kind's settings (`None`) and each slot.
    pub collapsed: BTreeSet<(ThingId, Option<SlotId>)>,
    /// A keyed setting was edited this frame, so the key landed at the playhead; the studio
    /// pauses so a drag doesn't leave a key on every frame.
    pub keyed_edit: bool,
    /// Set when "Open in Library" was clicked; the app opens the Library window on it.
    pub open_library: Option<ModuleId>,
    /// A card's Nodes switch was pressed: the node panel should open on this slot.
    pub nodes_request: Option<(ThingId, SlotId)>,
    /// What the node panel shows, so that card's switch shows Nodes as chosen.
    pub nodes_open: Option<(ThingId, SlotId)>,
    /// Each progression's chord editor: the picked chord, the presets list.
    pub chord_ui: BTreeMap<ProgressionId, ChordUi>,
    /// Chords asked to be heard this frame; the app plays them.
    pub clip: Option<ChordClip>,
    /// A card's "Edit patch" was pressed: the node panel should open on this patch.
    pub patch_request: Option<InstrumentId>,
}

/// A card's Nodes button.
pub struct NodesSwitch {
    pub enabled: bool,
    pub open: bool,
    /// Why it can't be pressed, when it can't.
    pub hover: &'static str,
}

/// Why a slot's Nodes switch is off.
fn no_nodes_yet(slot: SlotId) -> &'static str {
    match slot {
        SlotId::Look => "Look graphs can't be edited as nodes yet",
        SlotId::Trail => "Trail graphs can't be edited as nodes yet",
        SlotId::Particles => "Particle graphs can't be edited as nodes yet",
        SlotId::Shader => "Shader graphs can't be edited as nodes yet",
        _ => "Pick a module first",
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
    Parent(ThingId),
    Attach(Attach),
    /// One of the kind's own settings, for every selected thing of the kind.
    SetKind(&'static str, Value),
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
    Inherit(SlotId),
    Off(SlotId),
    AddSlot(SlotId),
    /// Throw a module's graph edits away and go back to its settings.
    Revert(ModuleId),
    /// Lock (true) or unlock a setting against the dice.
    Lock(Address, bool),
    /// Roll the dice on one card: the kind's own settings (`None`) or one slot.
    RollCard(Option<SlotId>),
    /// A card edited the module's graph.
    Graph(ModuleId, ModuleGraph),
    /// A chord editor changed a progression in the library.
    Progression(ProgressionId, Progression),
    /// An instrument section changed an instrument in the library.
    Instrument(InstrumentId, bpe_core::Instrument),
    /// A module's own value for one of its instrument's macros, or `None` to follow it again.
    Macro(ModuleId, String, Option<f32>),
    /// The look's colour list changed: its Colour and Colours settings, at these addresses.
    Colours {
        colour: Address,
        colours: Address,
        edit: ColoursEdit,
    },
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
    let bpm = bpe_kit::music::root_settings(resolved).bpm;
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
                // The camera's place is its own Position setting, which can be keyframed.
                if kind.id() != "camera" {
                    tree_rows(ui, scene, &thing, &mut edits);
                }
                if !matches!(kind.id(), "liquid" | "swarm" | "camera") {
                    let label = ui
                        .label("Stick to screen")
                        .on_hover_text("Stay put on the screen while the camera moves, like a caption");
                    let mut on = thing.sticks_to_screen(kind);
                    if ui.checkbox(&mut on, "").labelled_by(label.id).changed() {
                        edits.push(Edit::Screen((on != kind.sticks_to_screen()).then_some(on)));
                    }
                    ui.end_row();
                }
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
            let title = format!("{} settings", kind.name());
            let open = ui
                .horizontal_wrapped(|ui| {
                    let open = fold_arrow(ui, &mut inspector.collapsed, (id, None), &title);
                    ui.label(RichText::new(&title).strong());
                    ui.add_space(8.0);
                    if dice_button(ui, "Randomize these settings").clicked() {
                        edits.push(Edit::RollCard(None));
                    }
                    open
                })
                .inner;
            if !open {
                return;
            }
            let own = |key: &str| Address { owner: Owner::Thing(id), slot: None, key: key.into() };
            let get =
                |key: &str| scene.value_at_time(reg, &own(key), time).expect("the kind's own settings all resolve");
            let applies = |d: &SettingDef| d.applies(Some(&thing.kind), &|k| scene.value_at_time(reg, &own(k), time));
            let sim = kit.sims.thing(&thing.kind);
            let marks = |key: &str| {
                let def = kind.settings().iter().find(|d| d.key == key)?;
                let physics = sim.is_some_and(|s| s.affects_physics(key));
                mark_for(scene, def, &own(key), time, physics)
            };
            let locked = |key: &str| scene.is_locked(&own(key));
            let own_choices = TargetChoices { hide_scenes: id == ThingId::ROOT, ..choices.clone() };
            let cx = GridCx {
                choices: &own_choices,
                overridden: &|_| false,
                inherited: &|_| None,
                marks: &marks,
                locked: &locked,
                applies: &applies,
            };
            let out = settings_grid_marked(ui, ("kind", id), kind.settings(), get, &cx);
            if let Some(heard) = chord_editor(ui, &mut inspector.chord_ui, scene, kind.settings(), &get, &mut edits) {
                let sound = resolved.things.get(&ThingId::ROOT).and_then(|r| r.slots.get(&SlotId::Sound));
                let module = sound.map(|r| r.module);
                inspector.clip =
                    Some(chord_clip(heard, bpm, scene, module, &|k| sound.and_then(|r| r.params.get(k).cloned())));
            }
            edits.extend(out.changes.into_iter().map(|(key, value)| Edit::SetKind(key, value)));
            edits.extend(out.toggled_key.into_iter().map(|key| Edit::ToggleKey(own(key))));
            edits.extend(out.pulse.into_iter().map(|(key, source)| Edit::Pulse(own(key), source)));
            edits.extend(out.locks.into_iter().map(|(key, lock)| Edit::Lock(own(key), lock)));
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
                let open = ui
                    .horizontal_wrapped(|ui| {
                        let open = fold_arrow(ui, &mut inspector.collapsed, (id, Some(slot)), slot.name());
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
                            // module copies the shown one's settings, all of them when it is of the
                            // same kind, else the ones both kinds have (a look's colour and glow).
                            let kinds: Vec<&dyn ModuleKind> = reg.modules_for(slot).collect();
                            if let [only] = kinds[..] {
                                if ui.button(format!("New {} module…", slot.name())).clicked() {
                                    edits.push(Edit::NewModule { slot, kind: only.id(), copy_from: shown_module });
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
                                                copy_from: shown_module,
                                            });
                                            ui.close();
                                        }
                                    }
                                });
                            }
                            if let SlotLink::Linked { .. } = link {
                                if ui
                                    .button("Make unique")
                                    .on_hover_text(
                                        "This thing gets its own copy of the module, with its changes baked in",
                                    )
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
                        let nodes = NodesSwitch {
                            enabled: bpe_kit::role_of(slot).is_some() && shown_module.is_some(),
                            open: inspector.nodes_open == Some((id, slot)),
                            hover: no_nodes_yet(slot),
                        };
                        if nodes_button(ui, nodes) {
                            inspector.nodes_request = Some((id, slot));
                        }
                        let editable = matches!(
                            write_target(scene, resolved, id, slot),
                            Some(Write::Module(_) | Write::Override(..))
                        );
                        if editable && slot != SlotId::Rules {
                            ui.add_space(8.0);
                            if dice_button(ui, "Randomize these settings").clicked() {
                                edits.push(Edit::RollCard(Some(slot)));
                            }
                        }
                        open
                    })
                    .inner;
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
                // Every setting shows; the depth only matters to graph cards.
                let depth = Depth::More;
                let inherited = matches!(write, Write::Inherited { .. });
                let physics = matches!(slot, SlotId::Motion | SlotId::Bounce);
                let of_module = |key: &str| Address { owner: Owner::Module(module), slot: None, key: key.into() };
                let of_thing = |key: &str| Address { owner: Owner::Thing(id), slot: Some(slot), key: key.into() };
                let mark = |a: &Address| {
                    let def = mkind.settings().iter().find(|d| d.key == a.key)?;
                    mark_for(scene, def, a, time, physics)
                };
                ui.add_space(4.0);
                if slot == SlotId::Rules {
                    // Rules are their graph: sentences on the card, blocks in the panel. The
                    // graph is the module's, so an inherited card only reads it.
                    let Some(mut g) = m.graph.clone().or_else(|| module_graph(mkind, m)) else { return };
                    if inherited && change_here(ui).clicked() {
                        edits.push(Edit::Link(slot, module));
                    }
                    let out = ui.add_enabled_ui(!inherited, |ui| {
                        cards::rules_card(ui, ("rules", id, slot), &mut g, scene, depth)
                    });
                    if out.inner.changed && !inherited {
                        g.custom = true;
                        edits.push(Edit::Graph(module, g));
                    }
                    if out.inner.open_nodes {
                        inspector.nodes_request = Some((id, slot));
                    }
                    return;
                }
                // A module edited as nodes shows its graph's setting blocks; the settings the
                // graph doesn't bind, such as the instrument, stay native.
                let custom = match (m.graph.as_ref().filter(|g| g.custom), bpe_kit::role_of(slot)) {
                    (Some(g), Some(role)) => Some((g.clone(), role)),
                    _ => None,
                };
                let bound = match custom {
                    Some(_) => mkind.template().map(|t| bound_keys(&t)).unwrap_or_default(),
                    None => Default::default(),
                };
                let unbound: Vec<SettingDef> =
                    mkind.settings().iter().filter(|d| !bound.contains(d.key)).cloned().collect();
                let defs: &[SettingDef] = if custom.is_some() { &unbound } else { mkind.settings() };
                if let Some((mut g, role)) = custom {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Edited as nodes").small().color(theme::ACCENT_TEXT)).on_hover_text(
                            "This module runs its graph. These are the graph's own settings; open Nodes to see it.",
                        );
                        if !inherited
                            && ui
                                .small_button("Revert to template")
                                .on_hover_text("Throw the graph edits away and go back to the module's settings")
                                .clicked()
                        {
                            edits.push(Edit::Revert(module));
                        }
                    });
                    let catalog = bpe_vm::catalog(role);
                    let changed = ui
                        .add_enabled_ui(!inherited, |ui| {
                            cards::graph_settings(ui, ("graph", id, slot), &mut g, catalog, depth, scene)
                        })
                        .inner;
                    if changed && !inherited {
                        edits.push(Edit::Graph(module, g));
                    }
                }
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
                        let cx = GridCx {
                            choices: &choices,
                            overridden: &|_| false,
                            inherited: &|_| None,
                            marks: &marks,
                            locked: &locked,
                            applies: &applies,
                        };
                        let out = settings_grid_marked(ui, ("slot", id, slot), defs, get, &cx);
                        if let Some(heard) = chord_editor(ui, &mut inspector.chord_ui, scene, defs, &get, &mut edits) {
                            inspector.clip = Some(chord_clip(heard, bpm, scene, Some(module), &|k| Some(get(k))));
                        }
                        instrument_section(ui, scene, defs, &get, module, &mut edits, &mut inspector.patch_request);
                        if let Some(edit) = out.colours {
                            edits.push(Edit::Colours { colour: of_module(COLOUR), colours: of_module(COLOURS), edit });
                        }
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
                        let cx = GridCx {
                            choices: &choices,
                            overridden: &overridden,
                            inherited: &inherited,
                            marks: &marks,
                            locked: &locked,
                            applies: &applies,
                        };
                        let out = settings_grid_marked(ui, ("slot", id, slot), defs, get, &cx);
                        if let Some(heard) = chord_editor(ui, &mut inspector.chord_ui, scene, defs, &get, &mut edits) {
                            inspector.clip = Some(chord_clip(heard, bpm, scene, Some(module), &|k| Some(get(k))));
                        }
                        instrument_section(ui, scene, defs, &get, module, &mut edits, &mut inspector.patch_request);
                        if let Some(edit) = out.colours {
                            edits.push(Edit::Colours { colour: of_thing(COLOUR), colours: of_thing(COLOURS), edit });
                        }
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
                        if change_here(ui).clicked() {
                            edits.push(Edit::Link(slot, module));
                        }
                        let get = |key: &str| {
                            rslot.and_then(|r| r.params.get(key).cloned()).unwrap_or_else(|| {
                                let a = Address { owner: Owner::Module(module), slot: None, key: key.into() };
                                scene.value_at(reg, &a).expect("a module's settings all resolve")
                            })
                        };
                        let applies = |d: &SettingDef| d.applies(Some(&thing.kind), &|k| Some(get(k)));
                        ui.add_enabled_ui(false, |ui| {
                            settings_grid(ui, ("slot", id, slot), defs, &applies, get);
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
        let edit = make_picked_progression(scene, reg, id, time, edit);
        let edit = make_picked_instrument(scene, reg, id, time, edit);
        let ok = match edit {
            Edit::Name(name) => set_thing(scene, id, |t| t.name = name),
            Edit::Progression(pid, p) => scene.set_progression(pid, p).is_ok(),
            Edit::Instrument(iid, i) => scene.set_instrument(iid, i).is_ok(),
            Edit::Macro(module, key, value) => scene.set_macro(module, &key, value).is_ok(),
            Edit::Tags(tags) => set_thing(scene, id, |t| t.tags = tags),
            Edit::Enabled(enabled) => set_thing(scene, id, |t| t.enabled = enabled),
            Edit::Screen(screen) => set_thing(scene, id, |t| t.screen = screen),
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
                    any |= set_or_key(scene, reg, &a, value.clone(), time, &mut inspector.keyed_edit);
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
            Edit::Inherit(slot) => {
                scene.set_slot_inherit(id, slot);
                true
            }
            Edit::Off(slot) => {
                scene.set_slot_off(id, slot);
                true
            }
            Edit::AddSlot(slot) => scene.add_slot(reg, id, slot).is_ok(),
            Edit::Graph(module, g) => match scene.library.modules.get_mut(&module) {
                Some(m) => {
                    m.graph = Some(g);
                    if let Some(kind) = reg.module(&m.kind)
                        && let Some(graph) = m.graph.as_ref().map(|g| g.graph.clone())
                    {
                        crate::nodes::panel::sync_params(&graph, m, kind);
                    }
                    true
                }
                None => false,
            },
            Edit::Revert(module) => match scene.library.modules.get_mut(&module) {
                Some(m) => match reg.module(&m.kind) {
                    Some(kind) => {
                        crate::nodes::panel::revert_to_template(m, kind);
                        true
                    }
                    None => false,
                },
                None => false,
            },
            Edit::Lock(address, lock) => scene.set_locked(address, lock),
            Edit::Colours { colour, colours, edit } => {
                edit_colours(scene, reg, &colour, &colours, edit, time, &mut inspector.keyed_edit)
            }
            Edit::RollCard(slot) => {
                use bpe_kit::random::{Roll, Scope, fresh_seed, roll};
                let scope = Scope::Card { thing: id, slot };
                !roll(scene, reg, &scope, Roll { seed: fresh_seed(), amount: 1.0 }).changed.is_empty()
            }
        };
        changed |= ok;
    }
    changed
}

/// Writes one slot setting for one thing, where its write policy says: the module, the thing's
/// own override, or, for an inherited slot, a fresh link to the module and then an override.
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
        Some(Write::Inherited { module, .. }) => {
            if scene.link(reg, id, slot, module).is_err() {
                return false;
            }
            let a = Address { owner: Owner::Thing(id), slot: Some(slot), key: key.into() };
            set_or_key(scene, reg, &a, value, time, keyed)
        }
        None => false,
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
            let position =
                ui.label("Position").on_hover_text("Where it sits in the world. At Zoom 1 the shot is 1080 across");
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

/// What a setting's diamond shows, or `None` for a setting that can't be keyed or pulsed.
fn mark_for(scene: &Scene, def: &SettingDef, a: &Address, time: f64, physics: bool) -> Option<KeyMark> {
    if !def.flags.keyframe && !def.flags.pulse {
        return None;
    }
    Some(KeyMark {
        can_key: def.flags.keyframe,
        can_pulse: def.flags.pulse,
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

/// Under a card with a Chords setting: the chord editor for the progression it picks, or a line
/// saying it follows Scene defaults' chords.
fn chord_editor(
    ui: &mut Ui,
    chord_ui: &mut BTreeMap<ProgressionId, ChordUi>,
    scene: &Scene,
    defs: &[SettingDef],
    get: &dyn Fn(&str) -> Value,
    edits: &mut Vec<Edit>,
) -> Option<Vec<Chord>> {
    let mut heard = None;
    for def in defs.iter().filter(|d| d.ty == ValueType::Progression) {
        let picked = ProgressionRef::from_value(&get(def.key)).and_then(|r| r.0);
        let Some((pid, p)) = picked.and_then(|pid| scene.library.progressions.get(&pid).map(|p| (pid, p))) else {
            ui.label(
                RichText::new("Plays Scene defaults' chords. Pick Make unique to give it its own.")
                    .small()
                    .color(theme::MUTED),
            );
            continue;
        };
        let mut p = p.clone();
        let out = ui.push_id(("chords", pid), |ui| chords::show(ui, &mut p, chord_ui.entry(pid).or_default())).inner;
        heard = match out.audition {
            Some(Audition::Chord(c)) => Some(vec![Chord { beats: 4, ..c }]),
            Some(Audition::All) => Some(p.chords.clone()),
            None => heard,
        };
        if out.changed {
            edits.push(Edit::Progression(pid, p));
        }
    }
    heard
}

/// Under a card with an Instrument setting: the section for the instrument it picks, or a line
/// saying it plays Scene defaults' one.
fn instrument_section(
    ui: &mut Ui,
    scene: &Scene,
    defs: &[SettingDef],
    get: &dyn Fn(&str) -> Value,
    module: ModuleId,
    edits: &mut Vec<Edit>,
    patch_request: &mut Option<InstrumentId>,
) {
    for def in defs.iter().filter(|d| d.ty == ValueType::Instrument) {
        let picked = InstrumentRef::from_value(&get(def.key)).and_then(|r| r.0);
        let Some(iid) = picked.filter(|i| scene.library.instruments.contains_key(i)) else {
            ui.label(
                RichText::new("Plays Scene defaults' instrument. Pick Make unique to shape its own.")
                    .small()
                    .color(theme::MUTED),
            );
            continue;
        };
        for edit in crate::instruments::show(ui, scene, module, iid) {
            match edit {
                InstrumentEdit::Set(i, instrument) => edits.push(Edit::Instrument(i, instrument)),
                InstrumentEdit::Macro(key, value) => edits.push(Edit::Macro(module, key, value)),
                InstrumentEdit::EditPatch(i) => *patch_request = Some(i),
            }
        }
    }
}

/// A clip of `chords` on the scene's tempo, through the instrument a sound module's `get` reads.
fn chord_clip(
    chords: Vec<Chord>,
    bpm: f32,
    scene: &Scene,
    module: Option<ModuleId>,
    get: &dyn Fn(&str) -> Option<Value>,
) -> ChordClip {
    let id = get("instrument").and_then(|v| InstrumentRef::from_value(&v)).and_then(|r| r.0);
    ChordClip {
        chords,
        instrument: crate::instruments::choice(scene, id, module),
        seconds_per_beat: 60.0 / if bpm > 0.0 { bpm as f64 } else { 120.0 },
    }
}

/// A Chords row's "New progression" or "Make unique" becomes a pick of a progression made for
/// it: a starter, or a copy of the one the row shows (Scene defaults' when it follows that).
fn make_picked_progression(scene: &mut Scene, reg: &bpe_core::Registry, id: ThingId, time: f64, edit: Edit) -> Edit {
    let (slot, key, value) = match &edit {
        Edit::SetKind(key, value) => (None, *key, value),
        Edit::SetSlot(slot, key, value) => (Some(*slot), *key, value),
        _ => return edit,
    };
    let Value::Int(pick @ (NEW_PROGRESSION | MAKE_UNIQUE)) = *value else { return edit };
    let made =
        if pick == NEW_PROGRESSION {
            Some(scene.add_progression(bpe_core::Progression {
                name: "New chords".into(),
                ..bpe_core::Progression::starter()
            }))
        } else {
            let here = Address { owner: Owner::Thing(id), slot, key: key.into() };
            let root = Address { owner: Owner::Thing(ThingId::ROOT), slot: None, key: "chords".into() };
            let shown = |a: &Address| {
                scene.value_at_time(reg, a, time).and_then(|v| ProgressionRef::from_value(&v)).and_then(|r| r.0)
            };
            shown(&here).or_else(|| shown(&root)).and_then(|p| scene.copy_progression(p).ok())
        };
    let value = made.map_or(Value::Int(-1), |p| ProgressionRef(Some(p)).to_value());
    match slot {
        None => Edit::SetKind(key, value),
        Some(slot) => Edit::SetSlot(slot, key, value),
    }
}

/// An Instrument row's built-in, starter, "New patch" or "Make unique" pick becomes a pick of a
/// library instrument made for it: a built-in is shared with whatever already plays it.
fn make_picked_instrument(scene: &mut Scene, reg: &bpe_core::Registry, id: ThingId, time: f64, edit: Edit) -> Edit {
    let (slot, key, value) = match &edit {
        Edit::SetKind(key, value) => (None, *key, value),
        Edit::SetSlot(slot, key, value) => (Some(*slot), *key, value),
        _ => return edit,
    };
    let is_instrument = |d: &SettingDef| d.key == key && d.ty == ValueType::Instrument;
    let defs = match slot {
        None => scene.things.get(&id).and_then(|t| reg.thing(&t.kind)).map(|k| k.settings()),
        Some(slot) => {
            let a = Address { owner: Owner::Thing(id), slot: Some(slot), key: key.into() };
            scene.def_at(reg, &a).map(std::slice::from_ref)
        }
    };
    if !defs.is_some_and(|d| d.iter().any(is_instrument)) {
        return edit;
    }
    let Value::Int(pick) = *value else { return edit };
    let starters = || bpe_synth::starters();
    let made = match pick {
        NEW_PATCH => Some(scene.add_instrument(bpe_core::Instrument {
            name: "New patch".into(),
            source: InstrumentSource::Patch { graph: bpe_synth::blank_patch() },
            voices: Default::default(),
        })),
        MAKE_UNIQUE_INSTRUMENT => {
            let here = Address { owner: Owner::Thing(id), slot, key: key.into() };
            let shown =
                scene.value_at_time(reg, &here, time).and_then(|v| InstrumentRef::from_value(&v)).and_then(|r| r.0);
            shown
                .filter(|i| scene.library.instruments.contains_key(i))
                .or_else(|| crate::instruments::scene_instrument(scene))
                .and_then(|i| scene.copy_instrument(i).ok())
        }
        p if p <= STARTER_BASE => {
            let all = starters();
            all.into_iter().nth((STARTER_BASE - p) as usize).map(|(name, graph, voices)| {
                scene.add_instrument(bpe_core::Instrument {
                    name: name.into(),
                    source: InstrumentSource::Patch { graph },
                    voices,
                })
            })
        }
        p if p <= BUILT_IN_BASE => bpe_mix::Instrument::ALL.get((BUILT_IN_BASE - p) as usize).map(|&which| {
            let found = scene.library.instruments.iter().find(|(_, i)| plays_built_in(i, which)).map(|(i, _)| *i);
            found.unwrap_or_else(|| scene.add_instrument(bpe_kit::built_in_instrument(which)))
        }),
        _ => return edit,
    };
    let value = made.map_or(Value::Int(-1), |i| InstrumentRef(Some(i)).to_value());
    match slot {
        None => Edit::SetKind(key, value),
        Some(slot) => Edit::SetSlot(slot, key, value),
    }
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

/// The Nodes button: it opens the node panel on the card's module; true when it was pressed.
pub(crate) fn nodes_button(ui: &mut Ui, nodes: NodesSwitch) -> bool {
    ui.add_enabled(nodes.enabled, egui::Button::selectable(nodes.open, "Nodes"))
        .on_hover_text("Edit this module as blocks and wires, in the panel under the preview")
        .on_disabled_hover_text(nodes.hover)
        .clicked()
}

/// The arrow at the left of a card's title that folds the card's settings away and back. True
/// while the card is open.
pub(crate) fn fold_arrow<K: Ord + Copy>(ui: &mut Ui, collapsed: &mut BTreeSet<K>, key: K, title: &str) -> bool {
    let open = !collapsed.contains(&key);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
    let name = if open { format!("Hide {title}") } else { format!("Show {title}") };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name.clone()));
    let response = response.on_hover_text(if open { "Hide these settings" } else { "Show these settings" });
    // The collapsing header's own triangle, pointing down when open.
    let mut icon = response.clone();
    icon.rect = rect;
    egui::collapsing_header::paint_default_icon(ui, if open { 1.0 } else { 0.0 }, &icon);
    if response.clicked() {
        if open {
            collapsed.insert(key);
        } else {
            collapsed.remove(&key);
        }
        return !open;
    }
    open
}

/// Writes a change to a look's colour list. One colour lives in Colour, several in Colours; when
/// the list grows from one or shrinks to one, the keys move across too, and when it gains or
/// loses a colour every key gains or loses it, so the keys stay in step with the list. Changing
/// a colour keys the whole list at the playhead once it has keys.
fn edit_colours(
    scene: &mut Scene,
    reg: &bpe_core::Registry,
    colour: &Address,
    colours: &Address,
    edit: ColoursEdit,
    time: f64,
    keyed: &mut bool,
) -> bool {
    let ColoursEdit { list, change } = edit;
    let Some(&first) = list.first() else { return false };
    let was_many = matches!(scene.value_at_time(reg, colours, time), Some(Value::Colors(l)) if !l.is_empty());
    let reshape = |l: &mut Vec<Color>| match change {
        ListChange::Add => l.push(*list.last().expect("the list has a colour")),
        ListChange::Remove(i) if i < l.len() => _ = l.remove(i),
        _ => {}
    };
    match (was_many, list.len() > 1) {
        (false, false) => set_or_key(scene, reg, colour, Value::Color(first), time, keyed),
        (false, true) => {
            // From one colour to several: Colour's keys become the list's, first colour first.
            if let Some(mut track) = scene.tracks.remove(colour) {
                for key in &mut track.keys {
                    if let Value::Color(c) = key.value {
                        let mut l = vec![c];
                        l.extend_from_slice(&list[1..]);
                        key.value = Value::Colors(l);
                    }
                }
                track.range = None;
                scene.tracks.insert(colours.clone(), track);
            }
            scene.set_at(reg, colours, Value::Colors(list)).is_ok()
        }
        (true, false) => {
            // Down to one colour: the list's keys become Colour's, keeping what is left.
            if let Some(mut track) = scene.tracks.remove(colours) {
                for key in &mut track.keys {
                    if let Value::Colors(l) = &key.value {
                        let mut l = l.clone();
                        reshape(&mut l);
                        key.value = Value::Color(l.first().copied().unwrap_or(first));
                    }
                }
                scene.tracks.insert(colour.clone(), track);
            }
            let one = scene.set_at(reg, colour, Value::Color(first)).is_ok();
            one && scene.set_at(reg, colours, Value::Colors(Vec::new())).is_ok()
        }
        (true, true) => {
            if let ListChange::Set(_) = change {
                return set_or_key(scene, reg, colours, Value::Colors(list), time, keyed);
            }
            if let Some(track) = scene.tracks.get_mut(colours) {
                for key in &mut track.keys {
                    if let Value::Colors(l) = &mut key.value {
                        reshape(l);
                    }
                }
            }
            scene.set_at(reg, colours, Value::Colors(list)).is_ok()
        }
    }
}

/// The button an inherited card offers to link the module to the thing itself.
fn change_here(ui: &mut Ui) -> egui::Response {
    ui.button("Change here").on_hover_text("Link the module to this thing, so changes here are its own")
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
    use bpe_core::{Resolved, Settings, SlotId, SlotLink, Value, resolve};
    use bpe_kit::graphs::module_graph;
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

    #[test]
    fn the_switch_says_why_a_slot_has_no_nodes() {
        assert_eq!(no_nodes_yet(SlotId::Look), "Look graphs can't be edited as nodes yet");
        assert_eq!(no_nodes_yet(SlotId::Shader), "Shader graphs can't be edited as nodes yet");
        assert_eq!(no_nodes_yet(SlotId::Motion), "Pick a module first");
    }

    fn harness(selected: Option<ThingId>) -> Harness<'static, State> {
        harness_of(bpe_kit::new_scene(&bpe_kit::registry()), selected)
    }

    fn harness_of(scene: Scene, selected: Option<ThingId>) -> Harness<'static, State> {
        let kit = bpe_kit::kit();
        let resolved = resolve_scene(&scene, &kit.registry);
        let mut selection = Selection::default();
        if let Some(id) = selected {
            selection.set(id);
        }
        let state = State { inspector: Inspector::default(), scene, kit, resolved, selection, time: 0.0 };
        let mut h = Harness::builder()
            .with_size(egui::vec2(420.0, 2600.0))
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
    fn a_balls_colour_and_colours_are_one_list() {
        let mut h = harness(Some(ThingId(2)));
        let (module, _) = ball_look(h.state());
        assert!(h.query_by_label("Colour").is_none(), "no separate Colour row");
        assert_eq!(h.get_all_by_label("Colours").count(), 1);
        h.get_by_label("Add a colour").click();
        h.run();
        let m = &h.state().scene.library.modules[&module];
        assert!(matches!(m.params.get("gradient"), Some(Value::Colors(l)) if l.len() == 2), "{:?}", m.params);
    }

    #[test]
    fn colour_keys_follow_the_list_as_it_grows_and_shrinks() {
        let kit = bpe_kit::kit();
        let reg = &kit.registry;
        let mut scene = bpe_kit::new_scene(reg);
        let SlotLink::Linked { module, .. } = scene.things[&ThingId(2)].slots[&SlotId::Look].clone() else {
            panic!("the ball's look is linked")
        };
        let at = |key: &str| Address { owner: Owner::Module(module), slot: None, key: key.into() };
        let (red, blue, green) = (Color::rgb(255, 0, 0), Color::rgb(0, 0, 255), Color::rgb(0, 255, 0));
        scene.set_key(reg, &at(COLOUR), 0.0, Value::Color(red)).unwrap();
        scene.set_key(reg, &at(COLOUR), 2.0, Value::Color(blue)).unwrap();
        let mut keyed = false;
        // A second colour: Colour's keys move to the list, each gaining the new colour.
        let grow = ColoursEdit { list: vec![red, green], change: ListChange::Add };
        assert!(edit_colours(&mut scene, reg, &at(COLOUR), &at(COLOURS), grow, 0.0, &mut keyed));
        assert!(!scene.keyed(&at(COLOUR)));
        let keys: Vec<Value> = scene.tracks[&at(COLOURS)].keys.iter().map(|k| k.value.clone()).collect();
        assert_eq!(keys, [Value::Colors(vec![red, green]), Value::Colors(vec![blue, green])]);
        // Halfway between keys each colour is halfway too.
        let Some(Value::Colors(mid)) = scene.value_at_time(reg, &at(COLOURS), 1.0) else { panic!() };
        assert_eq!(mid[1], green);
        assert_ne!(mid[0], red);
        // Changing the second colour at 2 s keys the whole list there.
        let set = ColoursEdit { list: vec![blue, red], change: ListChange::Set(1) };
        assert!(edit_colours(&mut scene, reg, &at(COLOUR), &at(COLOURS), set, 2.0, &mut keyed));
        assert!(keyed);
        assert_eq!(scene.tracks[&at(COLOURS)].keys[1].value, Value::Colors(vec![blue, red]));
        // Back to one colour: the keys return to Colour, keeping the first colour left.
        let shrink = ColoursEdit { list: vec![red], change: ListChange::Remove(0) };
        assert!(edit_colours(&mut scene, reg, &at(COLOUR), &at(COLOURS), shrink, 0.0, &mut keyed));
        assert!(!scene.keyed(&at(COLOURS)));
        let keys: Vec<Value> = scene.tracks[&at(COLOUR)].keys.iter().map(|k| k.value.clone()).collect();
        assert_eq!(keys, [Value::Color(green), Value::Color(red)]);
        assert_eq!(scene.value_at(reg, &at(COLOURS)), Some(Value::Colors(Vec::new())));
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
        // The ball has a die on its own settings and on its look; its inherited cards have none.
        assert_eq!(h.get_all_by_label("Randomize these settings").count(), 2);
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
        h.get_by_label("Hide Balls settings").click();
        h.run();
        assert!(h.query_by_role_and_label(Role::Label, "Launch speed").is_none());
        assert!(h.get_all_by_role(Role::Slider).count() < before);
        assert!(h.state().inspector.collapsed.contains(&(ThingId(2), None)));
        h.get_by_label("Show Balls settings").click();
        h.run();
        assert!(h.query_by_role_and_label(Role::Label, "Launch speed").is_some());
    }

    #[test]
    fn an_arena_shows_only_what_does_something_for_it() {
        let h = harness(Some(ThingId(1)));
        // The arena's bounce only matters when it takes over; ball-only settings never show.
        assert!(h.query_by_role_and_label(Role::Label, "Takes over").is_some());
        for ball_only in ["Bounciness", "Grow per hit", "Hits other balls", "Pops after", "Flash", "Colours follow"] {
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
        assert!(h.query_by_label("Pulse with beat").is_none());
        h.state_mut().selection.set(ThingId::ROOT);
        h.run();
        h.get_by_label("Keyframe Bloom").click_secondary();
        h.run();
        h.get_by_label("Pulse with beat").click();
        h.run();
        let bloom = Address { owner: Owner::Thing(ThingId::ROOT), slot: None, key: "bloom".into() };
        let pulse = &h.state().scene.pulses[&bloom];
        assert_eq!((pulse.source, &pulse.amount), (PulseSource::Beat, &Value::Number(0.25)));
        assert_eq!(pulse_amount(&bpe_kit::SceneSettings::defs()[1]), Value::Vec2([0.0, 125.0]), "gravity");
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
        // The ball's Look has two kinds: the submenu names them, and a different kind keeps only
        // the settings both kinds share, such as the colour.
        let classic = s.scene.library.modules.values().find(|m| m.name == "Ball look").unwrap().params.clone();
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
        assert_eq!(m.params.get("color"), classic.get("color"), "{:?}", m.params);
        assert_eq!(m.params, s.kit.registry.carry_over(&classic, "look.styled"));
    }

    #[test]
    fn add_slot_lists_only_slots_the_kind_lacks_and_adds_them_inheriting() {
        let mut h = harness(Some(ThingId(2)));
        // Trail, Shader and Rules have module kinds and aren't shown on a new ball, so there is
        // something to add.
        assert!(!h.get_by_label("+ Add slot").accesskit_node().is_disabled());
        h.get_by_label("+ Add slot").click();
        h.run();
        assert!(h.query_by_label("Rules").is_some());
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
    fn the_new_module_menu_lists_every_kind_of_the_slot() {
        let mut h = harness(Some(ThingId(2)));
        h.get_by_label("Ball look").click();
        h.run();
        h.get_by_label("New Look module").click();
        h.run();
        for name in ["Classic", "Liquid surface", "Glowing dots"] {
            assert!(h.query_by_label(name).is_some(), "{name} is offered");
        }
        h.get_by_label("Glowing dots").click();
        h.run();
        let s = h.state();
        let SlotLink::Linked { module, .. } = &s.scene.things[&ThingId(2)].slots[&SlotId::Look] else { panic!() };
        assert_eq!(s.scene.library.modules[module].kind, "look.swarm");
        assert_eq!(s.scene.library.modules[module].name, "Ball Look");
        // Bounce has one kind, so its menu keeps the plain button that makes the module at once.
        // The ball's Motion, Bounce and Sound are inherited; Bounce's picker is the second.
        h.get_all_by_label_contains("from Scene defaults").nth(1).expect("the bounce picker").click();
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

    fn chords_combos(h: &Harness<'_, State>) -> Vec<egui::Rect> {
        h.query_all_by_role_and_label(Role::ComboBox, "Chords").map(|n| n.rect()).collect()
    }

    #[test]
    fn a_chords_setting_has_a_progression_picker() {
        let h = harness(Some(ThingId::ROOT));
        assert_eq!(chords_combos(&h).len(), 2, "Scene defaults' own and its sound module's");
        let shown: Vec<String> = h
            .query_all_by_role_and_label(Role::ComboBox, "Chords")
            .map(|n| n.accesskit_node().value().unwrap_or_default().to_owned())
            .collect();
        assert_eq!(shown, ["Starter", "Scene's (Starter)"], "Scene defaults' own, then the module following it");
    }

    #[test]
    fn scenes_is_offered_on_modules_not_on_scene_defaults() {
        let mut h = harness(Some(ThingId::ROOT));
        h.query_all_by_role_and_label(Role::ComboBox, "Chords").next().unwrap().click();
        h.run();
        assert!(h.query_by_label("Scene's").is_none());
        assert!(h.query_by_label("New progression").is_some());
        h.key_press(egui::Key::Escape);
        h.run();
        h.query_all_by_role_and_label(Role::ComboBox, "Chords").nth(1).unwrap().click();
        h.run();
        assert!(h.query_by_label("Scene's").is_some());
    }

    #[test]
    fn scene_defaults_fits_the_panel() {
        // Long labels and units once pushed the number boxes past the edge, so the panel grew.
        let h = harness(Some(ThingId::ROOT));
        for n in h.get_all_by_role(Role::SpinButton) {
            assert!(n.rect().right() <= 420.0, "{:?} reaches {}", n.accesskit_node().label(), n.rect().right());
        }
    }

    #[test]
    fn the_chord_editor_edits_the_picked_progression() {
        let mut h = harness(Some(ThingId::ROOT));
        assert_eq!(h.query_all_by_label("Chord lane").count(), 1, "only Scene defaults picks its own");
        assert!(h.query_by_label_contains("Plays Scene defaults' chords").is_some(), "its sound module follows it");
        let node = h.get_by_label("IV\nF");
        node.scroll_to_me();
        h.run();
        h.get_by_label("IV\nF").click();
        h.run();
        let p = &h.state().scene.library.progressions[&ProgressionId(0)];
        assert_eq!(p.chords.len(), 7);
        assert_eq!(p.chords[6].root, 5);
        // The new chord is heard for a bar through Scene defaults' sound, on its tempo.
        let clip = h.state().inspector.clip.as_ref().expect("a chord to hear");
        assert_eq!(clip.chords, [bpe_core::Chord { beats: 4, ..p.chords[6] }]);
        assert_eq!(clip.seconds_per_beat, 0.5, "120 bpm when the scene has no tempo");
    }

    /// The module Scene defaults' sound links.
    fn root_sound(s: &Scene) -> ModuleId {
        let SlotLink::Linked { module, .. } = &s.things[&ThingId::ROOT].slots[&SlotId::Sound] else {
            panic!("Scene defaults links a sound")
        };
        *module
    }

    fn open_instrument_menu(h: &mut Harness<'_, State>) {
        let combo = h.get_by_role_and_label(Role::ComboBox, "Instrument");
        combo.scroll_to_me();
        h.run();
        h.get_by_role_and_label(Role::ComboBox, "Instrument").click();
        h.run();
    }

    #[test]
    fn a_sound_card_has_an_instrument_picker_with_built_ins_first() {
        let mut h = harness(Some(ThingId::ROOT));
        let combo = h.get_by_role_and_label(Role::ComboBox, "Instrument");
        assert_eq!(combo.accesskit_node().value().as_deref(), Some("Pluck"));
        open_instrument_menu(&mut h);
        let y = |label: &str| h.get_by_label(label).rect().top();
        assert!(h.query_by_label("Scene's").is_some(), "a module can follow Scene defaults");
        assert!(y("Built-in") < y("Marimba"));
        assert!(y("Marimba") < y("New patch"), "built-ins come before the library and new ones");
        assert!(y("New patch") < y("Starter patches"));
        assert!(h.query_by_label(bpe_synth::starters()[0].0).is_some());
        assert!(h.query_by_label("Make unique").is_some());
    }

    #[test]
    fn picking_a_starter_adds_it_to_the_library_and_links_it() {
        let mut h = harness(Some(ThingId::ROOT));
        open_instrument_menu(&mut h);
        let name = bpe_synth::starters()[0].0;
        h.get_by_label(name).scroll_to_me();
        h.run();
        h.get_by_label(name).click();
        h.run();
        let s = h.state();
        assert_eq!(s.scene.library.instruments.len(), 2);
        let (id, i) = s.scene.library.instruments.iter().find(|(_, i)| i.name == name).expect("the starter is added");
        assert!(matches!(i.source, InstrumentSource::Patch { .. }));
        let m = root_sound(&s.scene);
        assert_eq!(s.scene.library.modules[&m].params.get("instrument"), Some(&InstrumentRef(Some(*id)).to_value()));
        // The section now shows the patch, its macros and its voices.
        h.get_by_label("Edit patch");
        h.get_by_role_and_label(Role::Slider, "Voice limit");
        // Picking a built-in shares the library's Pluck rather than adding another.
        open_instrument_menu(&mut h);
        // The first Pluck is the built-in; the library's own Pluck follows further down.
        h.query_all_by_label("Pluck").next().unwrap().scroll_to_me();
        h.run();
        h.query_all_by_label("Pluck").next().unwrap().click();
        h.run();
        let s = h.state();
        assert_eq!(s.scene.library.instruments.len(), 2);
        assert_eq!(
            s.scene.library.modules[&root_sound(&s.scene)].params.get("instrument"),
            Some(&InstrumentRef(Some(InstrumentId(0))).to_value())
        );
    }

    #[test]
    fn make_unique_copies_the_instrument() {
        let mut h = harness(Some(ThingId::ROOT));
        open_instrument_menu(&mut h);
        // The menu scrolls: Make unique is at its foot.
        h.get_by_label("Make unique").scroll_to_me();
        h.run();
        h.get_by_label("Make unique").click();
        h.run();
        let s = h.state();
        assert_eq!(s.scene.library.instruments.len(), 2);
        let copy = s.scene.library.instruments.iter().find(|(_, i)| i.name == "Pluck copy").expect("a copy").0;
        assert_eq!(
            s.scene.library.modules[&root_sound(&s.scene)].params.get("instrument"),
            Some(&InstrumentRef(Some(*copy)).to_value())
        );
    }

    #[test]
    fn a_macro_override_is_marked_and_resets() {
        let reg = bpe_kit::registry();
        let mut scene = bpe_kit::new_scene(&reg);
        let (name, graph, voices) = bpe_synth::starters().into_iter().next().unwrap();
        let knob = bpe_synth::macros(&graph).into_iter().next().expect("a starter has macros");
        let id = scene.add_instrument(bpe_core::Instrument {
            name: name.into(),
            source: InstrumentSource::Patch { graph },
            voices,
        });
        let m = root_sound(&scene);
        scene.library.modules.get_mut(&m).unwrap().params.set("instrument", InstrumentRef(Some(id)).to_value());
        let mid = (knob.min + knob.max) / 2.0;
        scene.set_macro(m, &knob.key(), Some(mid)).unwrap();
        let mut h = harness_of(scene, Some(ThingId::ROOT));
        let slider = h.get_by_role_and_label(Role::Slider, &knob.label);
        assert_eq!(slider.accesskit_node().numeric_value(), Some(mid as f64));
        assert!(h.query_by_label("Changed here").is_some(), "the override is marked");
        let label = h.get_by_role_and_label(Role::Label, &knob.label);
        label.scroll_to_me();
        h.run();
        h.get_by_role_and_label(Role::Label, &knob.label).click_secondary();
        h.run();
        h.get_by_label("Reset to instrument").click();
        h.run();
        assert!(h.state().scene.library.modules[&m].macros.is_empty());
        assert!(h.query_by_label("Changed here").is_none());
    }

    #[test]
    fn a_patch_section_fits_the_panel() {
        let mut h = harness(Some(ThingId::ROOT));
        open_instrument_menu(&mut h);
        let name = bpe_synth::starters()[0].0;
        h.get_by_label(name).scroll_to_me();
        h.run();
        h.get_by_label(name).click();
        h.run();
        h.get_by_role_and_label(Role::Slider, "Note length");
        for n in h.get_all_by_role(Role::SpinButton) {
            assert!(n.rect().right() <= 420.0, "{:?} reaches {}", n.accesskit_node().label(), n.rect().right());
        }
    }

    #[test]
    fn pluck_knobs_show_for_the_built_in_pluck() {
        let h = harness(Some(ThingId::ROOT));
        let decay = h.get_by_role_and_label(Role::Slider, "Decay");
        assert_eq!(decay.accesskit_node().numeric_value(), Some(0.9f32 as f64));
        let brightness = h.get_by_role_and_label(Role::Slider, "Brightness");
        assert_eq!(brightness.accesskit_node().numeric_value(), Some(0.5));
        assert!(h.query_by_label("Voice limit").is_none(), "a built-in plays without voices");
    }

    #[test]
    fn make_unique_gives_the_module_its_own_progression() {
        let mut h = harness(Some(ThingId(2)));
        let m = {
            let s = h.state_mut();
            let m = s.scene.add_module(&s.kit.registry, "sound.notes", "Own sound").unwrap();
            s.scene.link(&s.kit.registry, ThingId(2), SlotId::Sound, m).unwrap();
            s.resolved = resolve(&s.scene, &s.kit.registry);
            m
        };
        h.run();
        h.query_all_by_role_and_label(Role::ComboBox, "Chords").next().unwrap().click();
        h.run();
        h.get_by_label("Make unique").click();
        h.run();
        let s = h.state();
        assert_eq!(s.scene.library.progressions.len(), 2);
        let copy = s.scene.library.progressions.iter().find(|(_, p)| p.name == "Starter copy").unwrap().0;
        assert_eq!(s.scene.library.modules[&m].params.get("chords"), Some(&Value::Int(copy.0 as i64)));
    }

    /// Stores the module's template as its own graph, so it runs through the compiled path.
    fn make_custom(scene: &mut Scene, module: ModuleId) {
        let reg = bpe_kit::registry();
        let m = &scene.library.modules[&module];
        let mut g = module_graph(reg.module(&m.kind).unwrap(), m).expect("a template");
        g.custom = true;
        scene.library.modules.get_mut(&module).unwrap().graph = Some(g);
    }

    #[test]
    fn a_rules_card_lists_sentences_and_adds_a_recipe() {
        let reg = bpe_kit::registry();
        let mut scene = bpe_kit::new_scene(&reg);
        scene.add_slot(&reg, ThingId(2), SlotId::Rules).unwrap();
        let rules = scene.add_module(&reg, "rules.graph", "Ball rules").unwrap();
        scene.link(&reg, ThingId(2), SlotId::Rules, rules).unwrap();
        let mut h = harness_of(scene, Some(ThingId(2)));
        h.get_by_label("No rules yet.");
        h.get_by_label("Add rule").click();
        h.run();
        h.get_by_label("Grow on hit").click();
        h.run();
        assert!(h.query_by_label("No rules yet.").is_none());
        h.get_by_label_contains("When hit › Grow");
        // The recipe's number is right there on the card, named with its block.
        let by = h
            .get_all_by_role(Role::Slider)
            .find(|n| n.accesskit_node().label().is_some_and(|l| l.ends_with("By (px)")))
            .expect("the recipe's slider");
        assert_eq!(by.accesskit_node().numeric_value(), Some(2.0));
        let s = h.state();
        let g = s.scene.library.modules[&rules].graph.as_ref().expect("the recipe is stored");
        assert!(g.custom);
        assert_eq!(g.graph.nodes.len(), 2);
        assert_eq!(bpe_vm::sentences(&g.graph).len(), 1);
    }

    #[test]
    fn a_custom_module_shows_its_setting_blocks_as_sliders() {
        let mut scene = bpe_kit::new_scene(&bpe_kit::registry());
        make_custom(&mut scene, ModuleId(2));
        // The ball inherits the bounce from the scene, so its card shows the blocks read-only
        // until "Change here", like any inherited card.
        let h = harness_of(scene.clone(), Some(ThingId(2)));
        h.get_by_label("Edited as nodes");
        assert!(h.get_by_role_and_label(Role::Slider, "Bounciness").accesskit_node().is_disabled());
        // The scene links the module: its card edits the graph.
        let mut h = harness_of(scene, Some(ThingId::ROOT));
        h.get_by_label("Edited as nodes");
        let before = {
            let s = h.state();
            bpe_runtime::fingerprint(&s.scene, &s.resolved, &s.kit.sims)
        };
        let rect = slider_rect(&h, "Bounciness");
        drag_slider(&mut h, rect, 0.95);
        let s = h.state();
        let g = s.scene.library.modules[&ModuleId(2)].graph.as_ref().unwrap();
        let block =
            g.graph.nodes.iter().find(|n| n.params.get("key") == Some(&bpe_graph::Param::Text("bounciness".into())));
        let value = block.and_then(|n| n.params.get("value")).and_then(bpe_graph::Param::number).unwrap();
        assert!(value > 1.0, "the setting block holds the slider's value: {value}");
        // The module's own setting follows, so the card and the graph agree.
        let v = s.scene.library.modules[&ModuleId(2)].params.get("bounciness").cloned();
        assert!(matches!(v, Some(Value::Number(b)) if (b - value as f64).abs() < 1e-5), "{v:?}");
        assert_ne!(bpe_runtime::fingerprint(&s.scene, &s.resolved, &s.kit.sims), before, "the physics changed");
    }

    #[test]
    fn a_custom_sound_module_keeps_its_instrument_picker() {
        let mut scene = bpe_kit::new_scene(&bpe_kit::registry());
        make_custom(&mut scene, ModuleId(3));
        let h = harness_of(scene, Some(ThingId(2)));
        h.get_by_label("Instrument");
        // Bound settings come from the graph: the volume is a slider, not shown twice.
        assert_eq!(h.get_all_by_role_and_label(Role::Slider, "Volume").count(), 1);
    }

    #[test]
    fn target_settings_offer_the_scene_things_and_tags() {
        let reg = bpe_kit::registry();
        let mut scene = bpe_kit::new_scene(&reg);
        scene.things.get_mut(&ThingId(2)).unwrap().tags.insert("bouncers".into());
        let emitter = scene.add_thing(&reg, "emitter", ThingId::ROOT).unwrap();
        let mut h = harness_of(scene, Some(emitter));
        h.get_by_role_and_label(Role::Button, "Target").click();
        h.run();
        h.get_by_label("Thing").hover();
        h.run();
        assert!(h.query_by_label("Ball").is_some(), "the ball is offered by name");
        h.get_by_label("Tag").hover();
        h.run();
        h.get_by_label("bouncers").click();
        h.run();
        let target = h.state().scene.things[&emitter].params.get("target").cloned();
        assert_eq!(target, Some(Value::Target(bpe_core::Target::Tag("bouncers".into()))));
    }

    #[test]
    fn a_new_look_of_another_kind_keeps_the_colour_and_glow() {
        let reg = bpe_kit::registry();
        let mut scene = bpe_kit::new_scene(&reg);
        let SlotLink::Linked { module, .. } = scene.things[&ThingId(2)].slots[&SlotId::Look].clone() else {
            panic!("the ball's look is linked");
        };
        let colour = Value::Color(bpe_core::Color { r: 12, g: 200, b: 90, a: 255 });
        let set = |scene: &mut Scene, key: &str, v: Value| {
            let a = Address { owner: Owner::Module(module), slot: None, key: key.into() };
            scene.set_at(&reg, &a, v).unwrap();
        };
        set(&mut scene, "color", colour.clone());
        set(&mut scene, "glow", Value::Number(0.7));
        let mut h = harness_of(scene, Some(ThingId(2)));
        h.get_by_label("Ball look").click();
        h.run();
        h.get_by_label("New Look module").hover();
        h.run();
        h.get_by_label("Styled").click();
        h.run();
        let (new, _) = ball_look(h.state());
        assert_ne!(new, module);
        let styled = &h.state().scene.library.modules[&new];
        assert_eq!(styled.kind, "look.styled");
        assert_eq!(styled.params.get("color"), Some(&colour), "the colour comes along");
        assert_eq!(styled.params.get("glow"), Some(&Value::Number(0.7)), "and the glow");
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
        let other = scene.add_thing(&reg, "balls", ThingId::ROOT).unwrap();
        scene.link(&reg, other, SlotId::Look, module).unwrap();
        let mut h = harness_of(scene, Some(ThingId(2)));
        h.get_by_role_and_label(Role::Label, "Colours").click_secondary();
        h.run();
        h.get_by_label("Lock when randomizing").click();
        h.run();
        assert!(h.query_by_label("Locked when randomizing").is_some(), "a padlock shows on the row");
        let colour = Address { owner: Owner::Module(module), slot: None, key: "color".into() };
        let before = h.state().scene.value_at(&reg, &colour);
        // Neither a whole-scene roll nor this card's die moves the colour this ball shows.
        for (seed, scope) in [(7, Scope::Scene), (8, Scope::Card { thing: ThingId(2), slot: Some(SlotId::Look) })] {
            let mut scene = h.state().scene.clone();
            roll(&mut scene, &reg, &scope, Roll { seed, amount: 1.0 });
            let shown = Address { owner: Owner::Thing(ThingId(2)), slot: Some(SlotId::Look), key: "color".into() };
            assert_eq!(scene.value_at(&reg, &shown), before, "{scope:?}");
        }
        // Unlocking from the same row lets it roll again.
        h.get_by_role_and_label(Role::Label, "Colours").click_secondary();
        h.run();
        h.get_by_label("Unlock").click();
        h.run();
        assert!(h.query_by_label("Locked when randomizing").is_none());
        assert!(h.state().scene.locks.is_empty(), "{:?}", h.state().scene.locks);
    }
}

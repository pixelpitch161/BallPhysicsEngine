//! The inspector: the selected thing's header, its kind's settings and one card per slot.

use crate::theme;
use crate::widgets::settings_grid;
use bpe_core::{
    Address, Depth, ModuleId, ModuleKind, Owner, Resolved, Scene, SlotId, SlotLink, ThingId, Transform, Value,
};
use bpe_kit::Kit;
use eframe::egui::{self, DragValue, RichText, Ui};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub struct Inspector {
    /// The depth each card is open at: the kind's settings (`None`) and each slot.
    pub depth: BTreeMap<(ThingId, Option<SlotId>), Depth>,
}

/// A change the panel asks for, applied once the drawing is done.
enum Edit {
    Name(String),
    Tags(BTreeSet<String>),
    Enabled(bool),
    Transform(Transform),
    Set(Address, Value),
    Link(SlotId, ModuleId),
    NewModule { slot: SlotId, kind: &'static str, copy_from: Option<ModuleId> },
    Inherit(SlotId),
    Off(SlotId),
    AddSlot(SlotId),
}

/// Draws the inspector for `selected` and applies its edits. True when the scene changed.
pub fn show(
    ui: &mut Ui,
    inspector: &mut Inspector,
    scene: &mut Scene,
    kit: &Kit,
    resolved: &Resolved,
    selected: Option<ThingId>,
) -> bool {
    let reg = &kit.registry;
    let Some((id, thing)) = selected.and_then(|id| scene.things.get(&id).map(|t| (id, t.clone()))) else {
        ui.add_space(8.0);
        ui.label(RichText::new("Select something in the tree to see its settings.").color(theme::MUTED));
        return false;
    };
    let Some(kind) = reg.thing(&thing.kind) else {
        ui.label(RichText::new(format!("Its kind \"{}\" isn't installed.", thing.kind)).color(theme::WARNING));
        return false;
    };
    let mut edits = Vec::new();

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
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
                let mut t = thing.transform;
                let position = ui.label("Position").on_hover_text("Where it sits, in video pixels from the centre");
                let moved = ui
                    .horizontal(|ui| {
                        let x = ui.add(DragValue::new(&mut t.position.x).speed(1.0).prefix("x ").suffix(" px"));
                        let y = ui.add(DragValue::new(&mut t.position.y).speed(1.0).prefix("y ").suffix(" px"));
                        x.labelled_by(position.id).changed() || y.labelled_by(position.id).changed()
                    })
                    .inner;
                ui.end_row();
                let rotation = ui.label("Rotation").on_hover_text("How far it is turned, clockwise");
                let mut degrees = t.rotation.to_degrees();
                let turned =
                    ui.add(DragValue::new(&mut degrees).speed(1.0).suffix("°")).labelled_by(rotation.id).changed();
                if turned {
                    t.rotation = degrees.to_radians();
                }
                ui.end_row();
                if moved || turned {
                    edits.push(Edit::Transform(t));
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
            for (key, value) in settings_grid(ui, ("kind", id), kind.settings(), depth, get) {
                edits.push(Edit::Set(Address { owner: Owner::Thing(id), slot: None, key: key.into() }, value));
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
                if let Some(module) = shown_module
                    && let Some(m) = scene.library.modules.get(&module)
                    && let Some(mkind) = reg.module(&m.kind)
                {
                    let depth = inspector.depth.get(&(id, Some(slot))).copied().unwrap_or(Depth::Simple);
                    let get = |key: &str| {
                        let a = Address { owner: Owner::Module(module), slot: None, key: key.into() };
                        scene.value_at(reg, &a).expect("a module's settings all resolve")
                    };
                    ui.add_space(4.0);
                    for (key, value) in settings_grid(ui, ("slot", id, slot), mkind.settings(), depth, get) {
                        edits.push(Edit::Set(
                            Address { owner: Owner::Module(module), slot: None, key: key.into() },
                            value,
                        ));
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
            Edit::Set(address, value) => scene.set_at(reg, &address, value).is_ok(),
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
fn depth_switch(ui: &mut Ui, depth: &mut Depth) {
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

    struct State {
        inspector: Inspector,
        scene: Scene,
        kit: Kit,
        resolved: Resolved,
        selected: Option<ThingId>,
    }

    fn harness(selected: Option<ThingId>) -> Harness<'static, State> {
        let kit = bpe_kit::kit();
        let scene = bpe_kit::new_scene(&kit.registry);
        let resolved = resolve(&scene, &kit.registry);
        let state = State { inspector: Inspector::default(), scene, kit, resolved, selected };
        let mut h = Harness::builder()
            .with_size(egui::vec2(420.0, 1600.0))
            .with_step_dt(1.0 / 60.0)
            .with_max_steps(120)
            .allow_missing_glyphs()
            .with_context_setup(crate::theme::apply)
            .build_ui_state(
                |ui, s: &mut State| {
                    if show(ui, &mut s.inspector, &mut s.scene, &s.kit, &s.resolved, s.selected) {
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
}

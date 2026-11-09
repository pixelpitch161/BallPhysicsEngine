//! One card per slot the thing has: Motion and Bounce as one Physics card, Look cards with the
//! Look editor, Effects, a Particles thing's forces and looks, Lighting with its scenes, and
//! plain cards as a grid of settings.

use super::*;

/// The thing's slot cards, in [`SlotId::ALL`]'s order; Motion and Bounce show as one Physics
/// card, where the first of them would be.
pub(super) fn cards(
    ui: &mut Ui,
    inspector: &mut Inspector,
    cx: &OwnCx,
    kind: &dyn bpe_core::ThingKind,
    edits: &mut Vec<Edit>,
) {
    let (scene, kit, resolved, id, thing, time, lw) =
        (cx.scene, cx.kit, cx.resolved, cx.id, cx.thing, cx.time, cx.label_width);
    let reg = &kit.registry;
    let physics = physics_slots(thing);
    for slot in SlotId::ALL.into_iter().filter(|s| s.in_card_list()) {
        if in_physics(&thing.kind, slot) {
            if physics.first() == Some(&slot) {
                ui.add_space(6.0);
                physics_card(ui, inspector, cx, kind, resolved, &physics, edits);
            }
            continue;
        }
        let Some(card_now) = thing.slots.get(&slot) else { continue };
        ui.add_space(6.0);
        card(ui, |ui| {
            let title = kind.slot_label(slot);
            let header = ui.horizontal_wrapped(|ui| {
                let open = fold_arrow(ui, &mut inspector.collapsed, (id, Some(slot), ""), title);
                ui.label(RichText::new(title).font(style::semibold(13.0)));
                folded_summary(ui, open, card_now.params.iter().count());
                // The Type menu: what kind of card fills the slot, when more than one fits.
                let fitting: Vec<&dyn ModuleKind> = reg.kinds_fitting(slot, kind).collect();
                if let Some(current) = reg.module(&card_now.kind)
                    && fitting.len() > 1
                {
                    ui.add_space(4.0);
                    let label = ui.label(RichText::new("Type").small().color(style::MUTED));
                    ui.menu_button(current.name(), |ui| {
                        ui.set_min_width(180.0);
                        for k in &fitting {
                            if ui.selectable_label(k.id() == current.id(), k.name()).on_hover_text(k.help()).clicked() {
                                edits.push(Edit::SetKind(slot, k.id()));
                                ui.close();
                            }
                        }
                    })
                    .response
                    .labelled_by(label.id)
                    .on_hover_text("What kind of settings fill this card");
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let remove = egui::Button::new(RichText::new("Remove").small()).frame(false);
                    let remove = ui.add(remove).on_hover_text(format!("Take {title} off this thing"));
                    remove
                        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Remove {title}")));
                    if remove.clicked() {
                        edits.push(Edit::RemoveSlot(slot));
                    }
                });
                open
            });
            card_header(ui, inspector, Card::Slot(slot), &header.response);
            let problems: Vec<String> = resolved
                .problems
                .iter()
                .filter(|p| p.thing == id && p.slot == Some(slot))
                .map(crate::tree::problem_words)
                .collect();
            if !problems.is_empty() {
                ui.label(RichText::new(problems.join("\n")).small().color(style::TEXT));
            }
            if !header.inner {
                return;
            }
            if card_now.kind == bpe_kit::modules::effects::EFFECTS {
                ui.add_space(4.0);
                let look =
                    crate::look_editor::LookCx { scene, kit, choices: cx.choices, id, slot, time, label_width: lw };
                crate::look_editor::effects::show(ui, inspector, &look, edits);
                return;
            }
            // A Particles thing's forces and draw layers.
            let particles = [bpe_kit::things::particles::forces::CARD, bpe_kit::things::particles::look::CARD];
            if particles.contains(&card_now.kind.as_str()) {
                ui.add_space(4.0);
                let look =
                    crate::look_editor::LookCx { scene, kit, choices: cx.choices, id, slot, time, label_width: lw };
                if slot == SlotId::Motion {
                    crate::look_editor::particles::show_forces(ui, inspector, &look, edits);
                } else {
                    crate::look_editor::particles::show_look(ui, inspector, &look, edits);
                }
                return;
            }
            if card_now.kind == bpe_kit::modules::look::shape::CARD {
                ui.add_space(4.0);
                surface_section(ui, inspector, cx, slot, false, edits);
                let look =
                    crate::look_editor::LookCx { scene, kit, choices: cx.choices, id, slot, time, label_width: lw };
                crate::look_editor::show(ui, inspector, &look, edits);
                return;
            }
            if card_now.kind == bpe_kit::modules::look::layers::CARD {
                ui.add_space(4.0);
                if has_ball_look(kind.id()) {
                    look_mode_switch(ui, inspector, cx, edits);
                    ui.add_space(4.0);
                }
                surface_section(ui, inspector, cx, slot, has_ball_look(kind.id()), edits);
                let look =
                    crate::look_editor::LookCx { scene, kit, choices: cx.choices, id, slot, time, label_width: lw };
                if has_ball_look(kind.id()) && group_mode(inspector, id) {
                    crate::look_editor::group::show(ui, inspector, &look, edits);
                } else {
                    crate::look_editor::show(ui, inspector, &look, edits);
                }
                return;
            }
            slot_grid(ui, inspector, cx, slot, &card_now.kind, edits);
            if card_now.kind == bpe_kit::modules::lighting::LIGHTING
                && !is_folded(inspector, (id, Some(slot)), "Scenes")
            {
                crate::light_scenes_panel::show(ui, inspector, scene, reg, time, edits);
            }
            if card_now.kind == bpe_kit::modules::lighting::LIGHTING {
                ui.add_space(6.0);
                let button = ui.button("Preview final frame").on_hover_text(
                    "Draws the frame at the playhead exactly as the exported video will show it (baked when \
                     On export is Bake, at the export window's size), and says how long the whole video \
                     would take.",
                );
                if button.clicked() {
                    inspector.final_frame_wanted = true;
                }
            }
        });
    }
}

/// Motion and Bounce as one card, Physics: its header folds, counts and removes both, and each
/// card's settings grid shows inside it, Motion's first. The scene still keeps the two cards.
pub(super) fn physics_card(
    ui: &mut Ui,
    inspector: &mut Inspector,
    cx: &OwnCx,
    kind: &dyn bpe_core::ThingKind,
    resolved: &Resolved,
    slots: &[SlotId],
    edits: &mut Vec<Edit>,
) {
    let (reg, id) = (&cx.kit.registry, cx.id);
    card(ui, |ui| {
        let header = ui.horizontal_wrapped(|ui| {
            let open = fold_arrow(ui, &mut inspector.collapsed, Card::Physics.fold_key(id), "Physics");
            ui.label(RichText::new("Physics").font(style::semibold(13.0)));
            let changed = slots.iter().filter_map(|s| cx.thing.slots.get(s)).map(|c| c.params.iter().count()).sum();
            folded_summary(ui, open, changed);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let remove = egui::Button::new(RichText::new("Remove").small()).frame(false);
                let remove = ui.add(remove).on_hover_text("Take Physics off this thing");
                remove.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Remove Physics"));
                if remove.clicked() {
                    edits.extend(slots.iter().map(|s| Edit::RemoveSlot(*s)));
                }
            });
            open
        });
        card_header(ui, inspector, Card::Physics, &header.response);
        let problems: Vec<String> = resolved
            .problems
            .iter()
            .filter(|p| p.thing == id && p.slot.is_some_and(|s| slots.contains(&s)))
            .map(crate::tree::problem_words)
            .collect();
        if !problems.is_empty() {
            ui.label(RichText::new(problems.join("\n")).small().color(style::TEXT));
        }
        if !header.inner {
            return;
        }
        for &slot in slots {
            let Some(card_now) = cx.thing.slots.get(&slot) else { continue };
            // Each slot has one kind today; a second would need a Type menu here.
            debug_assert!(reg.kinds_fitting(slot, kind).count() <= 1, "{slot:?} needs a Type menu in Physics");
            slot_grid(ui, inspector, cx, slot, &card_now.kind, edits);
        }
    });
}

/// A plain card's settings: the card in `slot`, of kind `card_kind`, as a grid with folding
/// groups. Its edits are pushed to `edits`.
pub(super) fn slot_grid(
    ui: &mut Ui,
    inspector: &mut Inspector,
    cx: &OwnCx,
    slot: SlotId,
    card_kind: &str,
    edits: &mut Vec<Edit>,
) {
    let Some(ckind) = cx.kit.registry.module(card_kind) else { return };
    let defs = ckind.settings();
    debug_assert!(
        unknown_groups(defs).is_empty(),
        "{} has groups that are not sections: {:?}",
        card_kind,
        unknown_groups(defs)
    );
    slot_grid_of(ui, inspector, cx, slot, ("slot", cx.id, slot), defs, edits);
}

/// The grid of `defs`, settings of the thing's card in `slot`.
pub(super) fn slot_grid_of(
    ui: &mut Ui,
    inspector: &mut Inspector,
    cx: &OwnCx,
    slot: SlotId,
    grid_id: impl std::hash::Hash + std::fmt::Debug,
    defs: &[SettingDef],
    edits: &mut Vec<Edit>,
) {
    let (scene, reg, id, time) = (cx.scene, &cx.kit.registry, cx.id, cx.time);
    ui.add_space(4.0);
    let at = |key: &str| Address::on(id, slot, key);
    let get = |key: &str| scene.value_at_time(reg, &at(key), time).expect("a card's settings all resolve");
    let marks = |key: &str| {
        let def = crate::widgets::def_in(defs, key)?;
        Some(mark_for(scene, def, &at(key)))
    };
    let applies = |d: &SettingDef| d.applies(Some(&cx.thing.kind), &|k| scene.value_at_time(reg, &at(k), time));
    let shut = folded_groups(inspector, (id, Some(slot)), defs);
    let folded = |group: &str| shut.contains(group);
    let follow_menu = crate::follow::menu(scene);
    let follow_now = |key: &str| crate::follow::now(scene, &at(key));
    let follow_light = |key: &str| crate::follow::can_light(scene, reg, &at(key));
    let follow = crate::follow::FollowCx { menu: &follow_menu, now: &follow_now, light: &follow_light };
    let vary = crate::vary::VaryCx::of(scene, reg, time, id, &at);
    let grid = GridCx {
        choices: cx.choices,
        marks: &marks,
        applies: &applies,
        folded: Some(&folded),
        label_width: cx.label_width,
        track: style::colour_of(scene, id),
        follow: Some(&follow),
        vary: vary.as_ref(),
        offers: None,
    };
    let out = settings_grid(ui, grid_id, defs, get, &grid);
    fold_groups(inspector, (id, Some(slot)), &out.folds);
    edits.extend(out.changes.into_iter().map(|(key, value)| Edit::SetSlot(slot, key.into(), value)));
    edits.extend(out.new_keys.into_iter().map(|key| Edit::NewKey(Some(slot), key.into())));
    edits.extend(out.cleared.into_iter().map(|key| Edit::ClearKeys(Some(slot), key.into())));
    edits.extend(out.follows.into_iter().map(|(key, source)| Edit::Follow(Some(slot), key.into(), source)));
    edits.extend(out.varies.into_iter().map(|(key, v)| crate::vary::edit(Some(slot), key, v)));
}

/// A Look card's Surface section (lighting part 2a): a chip per Surface, each a painted ball,
/// which sets the numbers under it in the same edit, then those numbers (folded until opened;
/// Bulge only on balls).
pub(super) fn surface_section(
    ui: &mut Ui,
    inspector: &mut Inspector,
    cx: &OwnCx,
    slot: SlotId,
    ball: bool,
    edits: &mut Vec<Edit>,
) {
    use bpe_core::Choice;
    use bpe_kit::modules::look::surface::{self, SurfaceKind, preset_values};
    const GROUP: &str = "Surface";
    let (scene, reg, id, time) = (cx.scene, &cx.kit.registry, cx.id, cx.time);
    let at = |key: &str| Address::on(id, slot, key);
    let get = |key: &str| scene.value_at_time(reg, &at(key), time).expect("a Look card's Surface settings resolve");
    let now = match get("surface") {
        Value::Choice(name) => SurfaceKind::from_name(&name).unwrap_or_default(),
        _ => SurfaceKind::default(),
    };
    let open = !is_folded(inspector, (id, Some(slot)), GROUP);
    ui.horizontal(|ui| {
        if fold_toggle(ui, open, GROUP) {
            fold_groups(inspector, (id, Some(slot)), &[(GROUP, open)]);
        }
        ui.label(RichText::new(GROUP).font(style::semibold(12.0)));
    });
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        for (i, name) in SurfaceKind::NAMES.iter().enumerate() {
            let kind = SurfaceKind::from_name(name).expect("one of the names");
            if surface_chip(ui, i, &crate::widgets::pretty(name), kind == now).clicked() && kind != now {
                edits.push(Edit::SetSlot(slot, "surface".into(), Value::Choice((*name).into())));
                for (key, value) in preset_values(kind) {
                    edits.push(Edit::SetSlot(slot, key.into(), value));
                }
            }
        }
    });
    if !scene.things.get(&ThingId::ROOT).is_some_and(|r| r.slots.contains_key(&SlotId::Lighting)) {
        ui.label(RichText::new("Surfaces show once the scene has lighting (add a light).").small().color(style::MUTED));
    }
    if !open {
        return;
    }
    let defs: Vec<SettingDef> = surface::defs()
        .iter()
        .filter(|d| d.key != "surface" && (ball || d.key != "bulge"))
        .map(|d| SettingDef { group: "", ..d.clone() })
        .collect();
    let marks = |key: &str| {
        let def = crate::widgets::def_in(&defs, key)?;
        Some(mark_for(scene, def, &at(key)))
    };
    let applies = |d: &SettingDef| d.applies(Some(&cx.thing.kind), &|k| scene.value_at_time(reg, &at(k), time));
    let follow_menu = crate::follow::menu(scene);
    let follow_now = |key: &str| crate::follow::now(scene, &at(key));
    let follow_light = |key: &str| crate::follow::can_light(scene, reg, &at(key));
    let follow = crate::follow::FollowCx { menu: &follow_menu, now: &follow_now, light: &follow_light };
    let grid = GridCx {
        choices: cx.choices,
        marks: &marks,
        applies: &applies,
        folded: None,
        label_width: cx.label_width,
        track: style::colour_of(scene, id),
        follow: Some(&follow),
        vary: None,
        offers: None,
    };
    let out = settings_grid(ui, ("surface", id, slot), &defs, get, &grid);
    edits.extend(out.changes.into_iter().map(|(key, value)| Edit::SetSlot(slot, key.into(), value)));
    edits.extend(out.new_keys.into_iter().map(|key| Edit::NewKey(Some(slot), key.into())));
    edits.extend(out.cleared.into_iter().map(|key| Edit::ClearKeys(Some(slot), key.into())));
    edits.extend(out.follows.into_iter().map(|(key, source)| Edit::Follow(Some(slot), key.into(), source)));
    ui.add_space(4.0);
}

/// One Surface chip: Surface number `kind` painted as a little ball, then its name; outlined in
/// the text colour when picked.
pub(super) fn surface_chip(ui: &mut Ui, kind: usize, name: &str, on: bool) -> egui::Response {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let text = ui.painter().layout_no_wrap(name.to_owned(), font, if on { style::TEXT_STRONG } else { style::TEXT });
    let size = egui::vec2(26.0 + text.size().x + 8.0, 24.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    // Named as a surface: a Lava surface beside a Lava effect.
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, format!("{name} surface")));
    let response = response.on_hover_text(format!("Make it {}", name.to_lowercase()));
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, style::RADIUS, style::HOVER);
    }
    let edge = if on { style::TEXT } else { style::LINE };
    painter.rect_stroke(rect, style::RADIUS, egui::Stroke::new(1.0, edge), egui::StrokeKind::Inside);
    style::surface_swatch(painter, egui::pos2(rect.left() + 13.0, rect.center().y), 7.5, kind);
    painter.galley(egui::pos2(rect.left() + 26.0, rect.center().y - text.size().y / 2.0), text, style::TEXT);
    response
}

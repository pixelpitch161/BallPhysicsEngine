//! One card per slot the thing has, in order: Motion and Bounce as one Physics card
//! (`physics.rs`), Look cards with the Look editor and their Surface section (`surface.rs`),
//! Effects, a Particles thing's forces and looks, Lighting with its scenes, and plain cards as a
//! grid of settings.

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
    let (resolved, id, thing) = (cx.resolved, cx.id, cx.thing);
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
            let open = slot_header(ui, inspector, cx, kind, slot, card_now, edits);
            let problems: Vec<String> = resolved
                .problems
                .iter()
                .filter(|p| p.thing == id && p.slot == Some(slot))
                .map(crate::tree::problem_words)
                .collect();
            if !problems.is_empty() {
                ui.label(RichText::new(problems.join("\n")).small().color(style::TEXT));
            }
            if open {
                slot_body(ui, inspector, cx, kind, slot, &card_now.kind, edits);
            }
        });
    }
}

/// A slot card's header: its fold arrow, title and count, the Type menu (when more than one kind
/// of card fits the slot) and Remove. Says whether the card is open.
fn slot_header(
    ui: &mut Ui,
    inspector: &mut Inspector,
    cx: &OwnCx,
    kind: &dyn bpe_core::ThingKind,
    slot: SlotId,
    card_now: &bpe_core::Slot,
    edits: &mut Vec<Edit>,
) -> bool {
    let (reg, id) = (&cx.kit.registry, cx.id);
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
            remove.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Remove {title}")));
            if remove.clicked() {
                edits.push(Edit::RemoveSlot(slot));
            }
        });
        open
    });
    card_header(ui, inspector, Card::Slot(slot), &header.response);
    header.inner
}

/// An open slot card's settings: the Look editor for Look cards, the Effects stack, a Particles
/// thing's forces and looks, and a grid of settings for the rest (Lighting adds its scenes).
fn slot_body(
    ui: &mut Ui,
    inspector: &mut Inspector,
    cx: &OwnCx,
    kind: &dyn bpe_core::ThingKind,
    slot: SlotId,
    card_kind: &str,
    edits: &mut Vec<Edit>,
) {
    let (scene, kit, id, time, lw) = (cx.scene, cx.kit, cx.id, cx.time, cx.label_width);
    let look = crate::look_editor::LookCx { scene, kit, choices: cx.choices, id, slot, time, label_width: lw };
    if card_kind == bpe_kit::modules::effects::EFFECTS {
        ui.add_space(4.0);
        crate::look_editor::effects::show(ui, inspector, &look, edits);
        return;
    }
    // A Particles thing's forces and draw layers.
    let particles = [bpe_kit::things::particles::forces::CARD, bpe_kit::things::particles::look::CARD];
    if particles.contains(&card_kind) {
        ui.add_space(4.0);
        if slot == SlotId::Motion {
            crate::look_editor::particles::show_forces(ui, inspector, &look, edits);
        } else {
            crate::look_editor::particles::show_look(ui, inspector, &look, edits);
        }
        return;
    }
    if card_kind == bpe_kit::modules::look::shape::CARD {
        ui.add_space(4.0);
        surface_section(ui, inspector, cx, slot, false, edits);
        crate::look_editor::show(ui, inspector, &look, edits);
        return;
    }
    if card_kind == bpe_kit::modules::look::layers::CARD {
        ui.add_space(4.0);
        if has_ball_look(kind.id()) {
            look_mode_switch(ui, inspector, cx, edits);
            ui.add_space(4.0);
        }
        surface_section(ui, inspector, cx, slot, has_ball_look(kind.id()), edits);
        if has_ball_look(kind.id()) && group_mode(inspector, id) {
            crate::look_editor::group::show(ui, inspector, &look, edits);
        } else {
            crate::look_editor::show(ui, inspector, &look, edits);
        }
        return;
    }
    slot_grid(ui, inspector, cx, slot, card_kind, edits);
    if card_kind == bpe_kit::modules::lighting::LIGHTING && !is_folded(inspector, (id, Some(slot)), "Scenes") {
        crate::light_scenes_panel::show(ui, inspector, scene, &kit.registry, time, edits);
    }
    if card_kind == bpe_kit::modules::lighting::LIGHTING {
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

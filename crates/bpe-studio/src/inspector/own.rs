//! The thing's own settings: the kind's card (the root's one card per group, the Emitter card,
//! a Balls thing's Ball / Group switch), and the grids of a thing's own settings.

use super::*;

/// The kind's own card: the root's settings as one card per group, the Emitter card, or the
/// kind's settings (none for a Balls thing with a layered Look, whose own settings show by its
/// Look card's Ball / Group switch, under Placement and in Group mode).
pub(super) fn kind_card(
    ui: &mut Ui,
    inspector: &mut Inspector,
    cx: &OwnCx,
    kind: &dyn bpe_core::ThingKind,
    edits: &mut Vec<Edit>,
) {
    let (scene, kit, id, thing, time, lw) = (cx.scene, cx.kit, cx.id, cx.thing, cx.time, cx.label_width);
    if id == ThingId::ROOT {
        // The root's settings are one card per group.
        for (i, name) in bpe_kit::ROOT_CARDS.into_iter().enumerate() {
            if i > 0 {
                ui.add_space(6.0);
            }
            root_card(ui, inspector, cx, name, edits);
        }
    } else if kind.id() == EMITTER {
        card(ui, |ui| {
            let title = kind.name().to_owned();
            let header = ui.horizontal_wrapped(|ui| {
                let open = fold_arrow(ui, &mut inspector.collapsed, Card::Thing.fold_key(id), &title);
                ui.label(RichText::new(&title).font(style::semibold(13.0)));
                folded_summary(ui, open, thing.params.iter().count());
                open
            });
            card_header(ui, inspector, Card::Thing, &header.response);
            if header.inner {
                emitter_card::show(ui, inspector, cx, edits);
            }
        });
    } else if kind.id() == BALLS && layered_look(thing) {
        // A Balls thing's own settings show elsewhere: Ball size by the Look card's Ball /
        // Group switch, Launch speed under Placement, the group's in Group mode.
    } else {
        card(ui, |ui| {
            // The kind's name alone: "Balls", "Folder". A shape's outline is under its Look's
            // Body, so its own card holds the rest.
            let title = match kind.id() {
                SHAPE => "Shape settings".to_owned(),
                bpe_kit::things::text::KIND => "Words".to_owned(),
                _ => kind.name().to_owned(),
            };
            let header = ui.horizontal_wrapped(|ui| {
                let open = fold_arrow(ui, &mut inspector.collapsed, Card::Thing.fold_key(id), &title);
                ui.label(RichText::new(&title).font(style::semibold(13.0)));
                folded_summary(ui, open, thing.params.iter().count());
                open
            });
            card_header(ui, inspector, Card::Thing, &header.response);
            if header.inner {
                if kind.id() == BALLS {
                    // With no layered Look card, the Ball / Group switch (and Ball size) and
                    // Group mode show here; the other settings are under Placement and in
                    // Group mode.
                    look_mode_switch(ui, inspector, cx, edits);
                    if group_mode(inspector, id) {
                        let look = crate::look_editor::LookCx {
                            scene,
                            kit,
                            choices: cx.choices,
                            id,
                            slot: SlotId::Look,
                            time,
                            label_width: lw,
                        };
                        ui.add_space(6.0);
                        crate::look_editor::group::show(ui, inspector, &look, edits);
                    }
                } else if kind.id() == SHAPE {
                    // The outline's settings show under the Look's Body.
                    let defs: Vec<SettingDef> = kind
                        .settings()
                        .iter()
                        .filter(|d| !bpe_kit::BODY_HEADINGS.contains(&d.group))
                        .cloned()
                        .collect();
                    own_grid(ui, inspector, cx, ("kind", id), &defs, edits);
                } else {
                    own_grid(ui, inspector, cx, ("kind", id), kind.settings(), edits);
                }
            }
        });
    }
}

/// The Ball / Group switch of a Balls thing's look, with its Ball size under it (shown in both
/// modes).
pub(super) fn look_mode_switch(ui: &mut Ui, inspector: &mut Inspector, cx: &OwnCx, edits: &mut Vec<Edit>) {
    use crate::look_editor::LookMode;
    let (id, lw) = (cx.id, cx.label_width);
    let mut mode = inspector.look_mode.get(&id).copied().unwrap_or_default();
    settings_like_grid(("look mode", id)).show(ui, |ui| {
        let label = grid_label(ui, lw, "Edit")
            .on_hover_text("One ball's look, or the whole group: where the balls start and their colours");
        ui.horizontal(|ui| {
            crate::look_editor::ball_view::segmented(
                ui,
                &mut mode,
                &[
                    (LookMode::Ball, "Ball", "One ball's look, layer by layer"),
                    (LookMode::Group, "Group", "The whole group: where the balls start and the colours across them"),
                ],
            );
        })
        .response
        .labelled_by(label.id);
        ui.end_row();
    });
    inspector.look_mode.insert(id, mode);
    if let Some(kind) = cx.kit.registry.thing(&cx.thing.kind) {
        let size: Vec<SettingDef> = kind
            .settings()
            .iter()
            .filter(|d| d.key == BALL_SIZE)
            .map(|d| SettingDef { label: "Ball size", ..d.clone() })
            .collect();
        own_grid(ui, inspector, cx, ("ball size", id), &size, edits);
    }
}

/// Whether `thing`'s Look card is a layered one (the Ball Look editor).
pub(super) fn layered_look(thing: &bpe_core::Thing) -> bool {
    thing.slots.get(&SlotId::Look).is_some_and(|c| c.kind == bpe_kit::modules::look::layers::CARD)
}

/// A Balls thing's size setting, shown beside its Ball / Group switch.
pub(super) const BALL_SIZE: &str = "radius";

/// Whether thing `id`'s look shows across its group.
pub(super) fn group_mode(inspector: &Inspector, id: ThingId) -> bool {
    inspector.look_mode.get(&id) == Some(&crate::look_editor::LookMode::Group)
}

/// A grid of some of a thing's own settings, with folding groups. Its edits are pushed to
/// `edits`.
pub(super) fn own_grid(
    ui: &mut Ui,
    inspector: &mut Inspector,
    cx: &OwnCx,
    grid: impl std::hash::Hash + std::fmt::Debug,
    defs: &[SettingDef],
    edits: &mut Vec<Edit>,
) {
    let (scene, reg, id, time) = (cx.scene, &cx.kit.registry, cx.id, cx.time);
    let own = |key: &str| Address { thing: id, slot: None, key: key.into() };
    let get = |key: &str| scene.value_at_time(reg, &own(key), time).expect("the kind's own settings all resolve");
    let marks = |key: &str| {
        let def = crate::widgets::def_in(defs, key)?;
        Some(mark_for(scene, def, &own(key)))
    };
    let folded = |group: &str| is_folded(inspector, (id, None), group);
    let follow_menu = crate::follow::menu(scene);
    let follow_now = |key: &str| crate::follow::now(scene, &own(key));
    let follow_light = |key: &str| crate::follow::can_light(scene, reg, &own(key));
    let follow = crate::follow::FollowCx { menu: &follow_menu, now: &follow_now, light: &follow_light };
    let vary = crate::vary::VaryCx::of(scene, reg, time, id, &own);
    // The kind leaves out choices that make no sense where the thing is (riding a shape's balls).
    let kind = reg.thing(&cx.thing.kind);
    let offers = |d: &SettingDef, choice: &str| kind.is_none_or(|k| k.offers(scene, id, d.key, choice));
    let grid_cx = GridCx {
        choices: cx.choices,
        marks: &marks,
        applies: cx.applies,
        folded: Some(&folded),
        label_width: cx.label_width,
        track: style::colour_of(scene, id),
        follow: Some(&follow),
        vary: vary.as_ref(),
        offers: Some(&offers),
    };
    let out = settings_grid(ui, grid, defs, get, &grid_cx);
    fold_groups(inspector, (id, None), &out.folds);
    edits.extend(out.changes.into_iter().map(|(key, value)| Edit::SetOwn(key.into(), value)));
    edits.extend(out.new_keys.into_iter().map(|key| Edit::NewKey(None, key.into())));
    edits.extend(out.cleared.into_iter().map(|key| Edit::ClearKeys(None, key.into())));
    edits.extend(out.follows.into_iter().map(|(key, source)| Edit::Follow(None, key.into(), source)));
    edits.extend(out.varies.into_iter().map(|(key, v)| crate::vary::edit(None, key, v)));
}

/// One of the root's cards: the root's own settings whose group is `name`, under a title that
/// folds them away.
pub(super) fn root_card(ui: &mut Ui, inspector: &mut Inspector, cx: &OwnCx, name: &'static str, edits: &mut Vec<Edit>) {
    let Some(kind) = cx.kit.registry.thing(&cx.thing.kind) else { return };
    // Blanked, so no heading inside repeats the card's title.
    let defs: Vec<SettingDef> =
        kind.settings().iter().filter(|d| d.group == name).map(|d| SettingDef { group: "", ..d.clone() }).collect();
    let card_kind = Card::Section(name);
    card(ui, |ui| {
        let header = ui.horizontal_wrapped(|ui| {
            let open = fold_arrow(ui, &mut inspector.collapsed, card_kind.fold_key(cx.id), name);
            ui.label(RichText::new(name).font(style::semibold(13.0)));
            open
        });
        card_header(ui, inspector, card_kind, &header.response);
        if header.inner {
            own_grid(ui, inspector, cx, ("kind", cx.id, name), &defs, edits);
        }
    });
}

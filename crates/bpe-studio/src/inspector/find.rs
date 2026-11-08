//! The inspector's Find a setting box (editor workflow, W5): what is typed there shows, in place
//! of the cards, every setting of the selected thing whose name (or section) holds it: its own,
//! each card's and each layer's, under the card (and layer) it is in. Each row edits, keys and
//! follows as it does in its card.

use super::*;

/// The box's label (and hint).
pub(super) const FIND: &str = "Find a setting";

/// Whether `def` is found by `query` (already lower case): every word of it is in the setting's
/// name or its section.
pub(super) fn matches(def: &SettingDef, query: &str) -> bool {
    let words = format!("{} {}", def.label, def.group).to_lowercase();
    let mut typed = query.split_whitespace().peekable();
    typed.peek().is_some() && typed.all(|w| words.contains(w))
}

/// The defs `query` finds that apply now, without their sections (so no heading folds one away).
fn found<'a>(
    defs: impl IntoIterator<Item = &'a SettingDef>,
    query: &str,
    applies: &dyn Fn(&SettingDef) -> bool,
) -> Vec<SettingDef> {
    defs.into_iter()
        .filter(|d| matches(d, query) && applies(d))
        .map(|d| SettingDef { group: "", ..d.clone() })
        .collect()
}

/// The box itself, above the cards. Escape empties it.
pub(super) fn find_box(ui: &mut Ui, inspector: &mut Inspector) {
    ui.horizontal(|ui| {
        ui.add_space(CARD_MARGIN as f32);
        let clear = !inspector.find.is_empty();
        let width = (ui.available_width() - if clear { 56.0 } else { 8.0 }).max(60.0);
        let label = ui.label(RichText::new("Find").small().color(style::MUTED));
        let edit = egui::TextEdit::singleline(&mut inspector.find).hint_text(FIND).desired_width(width - 32.0);
        let response = ui.add(edit).labelled_by(label.id).on_hover_text(
            "Type part of a setting's name to see every setting of this thing that has it, whichever card it is on",
        );
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, FIND));
        // egui lets go of the box on Escape before the box sees it, so it has just lost focus.
        if (response.has_focus() || response.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            inspector.find.clear();
        }
        if clear && ui.small_button("Clear").on_hover_text("Show the cards again").clicked() {
            inspector.find.clear();
        }
    });
    ui.add_space(4.0);
}

/// Everything `query` finds on the thing, card by card; or a word that nothing matched.
pub(super) fn results(
    ui: &mut Ui,
    inspector: &mut Inspector,
    cx: &OwnCx,
    kind: &dyn bpe_core::ThingKind,
    query: &str,
    edits: &mut Vec<Edit>,
) {
    let query = query.trim().to_lowercase();
    let (scene, reg, id, time, thing) = (cx.scene, &cx.kit.registry, cx.id, cx.time, cx.thing);
    let mut any = false;
    let mut heading = |ui: &mut Ui, title: &str| {
        if any {
            ui.add_space(6.0);
        }
        any = true;
        ui.label(RichText::new(title).font(style::semibold(13.0)));
    };

    let own = found(kind.settings(), &query, cx.applies);
    if !own.is_empty() {
        card(ui, |ui| {
            heading(ui, if id == ThingId::ROOT { "Scene settings" } else { kind.name() });
            own_grid(ui, inspector, cx, ("find own", id), &own, edits);
        });
    }

    for slot in SlotId::ALL.into_iter().filter(|s| s.in_card_list()) {
        let Some(card_now) = thing.slots.get(&slot) else { continue };
        let title = kind.slot_label(slot);
        if let Some(module) = reg.module(&card_now.kind) {
            let at = |k: &str| Address::on(id, slot, k);
            let applies = |d: &SettingDef| d.applies(Some(&thing.kind), &|k| scene.value_at_time(reg, &at(k), time));
            let defs = found(module.settings(), &query, &applies);
            if !defs.is_empty() {
                ui.add_space(6.0);
                card(ui, |ui| {
                    heading(ui, title);
                    slot_grid_of(ui, inspector, cx, slot, ("find slot", id, slot), &defs, edits);
                });
            }
        }
        for layer in &card_now.layers {
            let Some(lkind) = reg.layer(&layer.kind) else { continue };
            let at = |k: &str| Address::on(id, slot, &bpe_core::layered_key(layer.id, k));
            let applies = |d: &SettingDef| d.applies(Some(&thing.kind), &|k| scene.value_at_time(reg, &at(k), time));
            let defs = found(lkind.settings().iter().chain(lkind.light_defs()), &query, &applies);
            if defs.is_empty() {
                continue;
            }
            ui.add_space(6.0);
            card(ui, |ui| {
                heading(ui, &format!("{title} \u{00b7} {}", layer.name));
                let look = crate::look_editor::LookCx {
                    scene,
                    kit: cx.kit,
                    choices: cx.choices,
                    id,
                    slot,
                    time,
                    label_width: cx.label_width,
                };
                crate::look_editor::layer_settings(ui, inspector, &look, layer.id, &defs, edits);
            });
        }
    }

    if !any {
        ui.add_space(8.0);
        ui.label(RichText::new(format!("No settings match \u{201c}{}\u{201d}.", query)).color(style::MUTED));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_setting_is_found_by_part_of_its_name_or_its_section() {
        let any = <bpe_kit::BallsSettings as bpe_core::Settings>::defs()[0].clone();
        let def = SettingDef { label: "Glow strength", group: "Light", ..any };
        assert!(matches(&def, "glow"));
        assert!(matches(&def, "light"));
        assert!(!matches(&def, "size"));
        assert!(!matches(&def, ""), "an empty box finds nothing");
        assert!(matches(&def, "light glow"), "every word, in any order");
        assert!(!matches(&def, "glow size"));
    }
}

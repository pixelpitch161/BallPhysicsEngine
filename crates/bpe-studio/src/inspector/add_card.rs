//! "+ Add card": a card for each slot the thing's kind can have and it doesn't yet.

use super::*;

/// The "+ Add card" menu. Motion and Bounce are one entry, Physics, at the first of them; it adds
/// every Physics card the kind uses, so each selected thing gets whichever it lacks.
pub(super) fn menu(ui: &mut Ui, cx: &OwnCx, kind: &dyn bpe_core::ThingKind, edits: &mut Vec<Edit>) {
    let (thing, reg) = (cx.thing, &cx.kit.registry);
    ui.add_space(8.0);
    let missing: Vec<SlotId> = kind
        .slots()
        .iter()
        .copied()
        // Sound is turned on from the Sound tab, not added as a card.
        .filter(|s| s.in_card_list())
        .filter(|s| !thing.slots.contains_key(s) && reg.kinds_fitting(*s, kind).next().is_some())
        .collect();
    let mut entries: Vec<(&str, Vec<SlotId>, String)> = Vec::new();
    for &slot in &missing {
        if !in_physics(kind.id(), slot) {
            let label = kind.slot_label(slot);
            entries.push((label, vec![slot], format!("Adds {label} to this thing")));
        } else if !entries.iter().any(|(label, ..)| *label == "Physics") {
            let all: Vec<SlotId> = PHYSICS
                .into_iter()
                .filter(|s| kind.slots().contains(s) && reg.kinds_fitting(*s, kind).next().is_some())
                .collect();
            let lacks: Vec<&str> =
                missing.iter().filter(|s| PHYSICS.contains(s)).map(|s| kind.slot_label(*s)).collect();
            let help = format!("Adds {} to this thing's Physics", lacks.join(" and "));
            entries.push(("Physics", all, help));
        }
    }
    ui.add_enabled_ui(!missing.is_empty(), |ui| {
        ui.menu_button("+ Add card", |ui| {
            for (label, slots, help) in &entries {
                if ui.button(*label).on_hover_text(help).clicked() {
                    edits.extend(slots.iter().map(|s| Edit::AddSlot(*s)));
                    ui.close();
                }
            }
        })
        .response
        .on_disabled_hover_text("Every card this kind can use is already shown");
    });
}

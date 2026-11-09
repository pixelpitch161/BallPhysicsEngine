//! The Physics card: Motion and Bounce shown as one card.

use super::*;

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

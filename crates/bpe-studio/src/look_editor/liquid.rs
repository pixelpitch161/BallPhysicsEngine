//! A Liquid thing's two cards with lists (liquid part 1): its **Fill** (the settings, then its
//! Pours) and its **Meets** (the settings, then its rules for other liquids). Each list has an add
//! menu, drag to reorder, the row menus, and under it the picked one's settings.

use bpe_core::{LayerId, LayerSection};
use eframe::egui::{RichText, Ui};

use super::{LookCx, layer_list, list_header, picked_settings};
use crate::inspector::{Edit, Inspector};
use crate::style;

/// The picked layer of the card: the one the user picked, or the top one.
fn picked(inspector: &Inspector, cx: &LookCx, card: &bpe_core::Slot) -> Option<LayerId> {
    let chosen = inspector.selected_layer.get(&cx.key()).copied().filter(|l| card.layer(*l).is_some());
    chosen.or_else(|| layer_list::top_level(card).last().map(|l| l.id))
}

/// The card's list, the top one first, the picked one's settings under it; `empty` when there
/// are none.
fn list_of(
    ui: &mut Ui,
    inspector: &mut Inspector,
    cx: &LookCx,
    (title, add, empty): (&str, &str, &str),
    edits: &mut Vec<Edit>,
) {
    let Some(card) = cx.card() else { return };
    ui.add_space(6.0);
    list_header(ui, cx, card, title, add, LayerSection::Force, edits);
    let Some(picked) = picked(inspector, cx, card) else {
        ui.label(RichText::new(empty).small().color(style::FAINT));
        return;
    };
    let tops: Vec<&bpe_core::Layer> = layer_list::top_level(card).collect();
    let mut rows = Vec::new();
    for layer in tops.iter().rev() {
        rows.push((layer.id, layer_list::piece(ui, inspector, cx, card, layer, picked, edits)));
    }
    layer_list::section_drop_target(ui, cx, card, &rows, LayerSection::Force, edits);
    picked_settings(ui, inspector, cx, card, picked, false, edits);
}

/// The Fill card's Pours and forces (Heat, Drain, Current, Magnet), under its settings.
pub fn show_pours(ui: &mut Ui, inspector: &mut Inspector, cx: &LookCx, edits: &mut Vec<Edit>) {
    let words = (
        "Pours and forces",
        "+ Add pour or force",
        "None yet. Add a pour to pour liquid in from a spout, or heat, a drain, a current or a magnet.",
    );
    list_of(ui, inspector, cx, words, edits);
}

/// The Meets card's rules for other liquids, under its settings.
pub fn show_others(ui: &mut Ui, inspector: &mut Inspector, cx: &LookCx, edits: &mut Vec<Edit>) {
    let words = (
        "Other liquids",
        "+ Add other liquid",
        "None yet: it meets other liquids with its own Look's mixing. Add one to layer or mix differently.",
    );
    list_of(ui, inspector, cx, words, edits);
}

#[cfg(test)]
mod tests {
    use crate::inspector::Inspector;
    use crate::selection::Selection;
    use bpe_core::{Resolved, Scene, SlotId, ThingId, resolve};
    use bpe_kit::Kit;
    use bpe_kit::things::liquid::{KIND, OTHER, POUR};
    use eframe::egui;
    use egui_kittest::{Harness, kittest::Queryable};

    struct State {
        inspector: Inspector,
        scene: Scene,
        kit: Kit,
        resolved: Resolved,
        selection: Selection,
    }

    /// The blank project with a new Liquid thing, selected.
    fn harness() -> (Harness<'static, State>, ThingId) {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_studio_scene(&kit.registry);
        let id = bpe_kit::add_thing(&mut scene, &kit.registry, KIND, ThingId::ROOT).unwrap();
        let resolved = resolve(&scene, &kit.registry);
        let mut selection = Selection::default();
        selection.set(id);
        let state = State { inspector: Inspector::default(), scene, kit, resolved, selection };
        let mut h = Harness::builder()
            .with_size(egui::vec2(420.0, 4000.0))
            .with_step_dt(1.0 / 60.0)
            .with_max_steps(120)
            .allow_missing_glyphs()
            .with_context_setup(crate::style::apply)
            .build_ui_state(
                |ui, s: &mut State| {
                    let changed = crate::inspector::show(
                        ui,
                        &mut s.inspector,
                        &mut s.scene,
                        &s.kit,
                        &s.resolved,
                        &s.selection,
                        0.0,
                    );
                    if changed {
                        s.resolved = resolve(&s.scene, &s.kit.registry);
                    }
                },
                state,
            );
        h.run();
        (h, id)
    }

    fn kinds(h: &Harness<'_, State>, id: ThingId, slot: SlotId) -> Vec<String> {
        h.state().scene.things[&id].slots[&slot].layers.iter().map(|l| l.kind.clone()).collect()
    }

    #[test]
    fn a_liquid_shows_its_fill_meets_and_look_and_adds_pours_and_rules() {
        let (mut h, id) = harness();
        assert!(h.query_by_label("Physics").is_none(), "a Liquid thing has no Physics card");
        for title in ["Fill", "Meets", "Look", "Balls push it", "Mix style"] {
            assert!(h.query_all_by_label(title).next().is_some(), "{title}");
        }
        h.get_by_label("+ Add pour or force").click();
        h.run();
        for name in ["Heat", "Drain", "Current", "Magnet"] {
            assert!(h.query_all_by_label(name).next().is_some(), "the menu offers {name}");
        }
        h.get_by_label("Pour").click();
        h.run();
        assert_eq!(kinds(&h, id, SlotId::Motion), [POUR]);
        // The picked pour's settings show under the list.
        assert!(h.query_all_by_label("Aim").next().is_some());
        h.get_by_label("+ Add other liquid").click();
        h.run();
        h.get_by_label("Other liquid").click();
        h.run();
        assert_eq!(kinds(&h, id, SlotId::Bounce), [OTHER]);
        assert_eq!(kinds(&h, id, SlotId::Motion), [POUR], "the pours are as they were");
    }
}

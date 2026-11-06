//! A Particles thing's two layered cards: its **Motion** (a list of forces, the top one applied
//! last) and its **Look** (a list of draw layers with their sub-layers, the top one drawn last),
//! each with an add menu, drag to reorder, the row menus, and under the list the picked layer's
//! settings. No ball view and no reactions: the particles are seen in the preview.

use bpe_core::{LayerId, LayerSection};
use eframe::egui::{RichText, Ui};

use super::{LookCx, layer_list, list_header, picked_settings};
use crate::inspector::{Edit, Inspector};
use crate::style;

/// The picked layer of the card: the one the user picked, or the top one.
fn picked(inspector: &Inspector, card: &bpe_core::Slot, id: bpe_core::ThingId) -> Option<LayerId> {
    let chosen = inspector.selected_layer.get(&id).copied().filter(|l| card.layer(*l).is_some());
    chosen.or_else(|| layer_list::top_level(card).last().map(|l| l.id))
}

/// The Motion card: its forces.
pub fn show_forces(ui: &mut Ui, inspector: &mut Inspector, cx: &LookCx, edits: &mut Vec<Edit>) {
    let Some(card) = cx.card() else { return };
    list_header(ui, cx, card, "Forces", "+ Add force", LayerSection::Force, edits);
    let Some(picked) = picked_on(inspector, cx, card) else {
        let words = "None yet: the particles fly straight on. Add a force to pull, blow or swirl them.";
        ui.label(RichText::new(words).small().color(style::FAINT));
        return;
    };
    list(ui, inspector, cx, card, picked, LayerSection::Force, edits);
    picked_settings(ui, inspector, cx, card, picked, false, edits);
}

/// The Look card: its draw layers.
pub fn show_look(ui: &mut Ui, inspector: &mut Inspector, cx: &LookCx, edits: &mut Vec<Edit>) {
    let Some(card) = cx.card() else { return };
    list_header(ui, cx, card, "Layers", "+ Add layer", LayerSection::Drawn, edits);
    let Some(picked) = picked_on(inspector, cx, card) else {
        let words = "None yet: the particles are there but nothing draws them. Add Sprites or Streaks.";
        ui.label(RichText::new(words).small().color(style::FAINT));
        return;
    };
    list(ui, inspector, cx, card, picked, LayerSection::Drawn, edits);
    picked_settings(ui, inspector, cx, card, picked, false, edits);
}

/// The layer picked on this card: each card keeps its own pick ([`super::pick_key`]).
fn picked_on(inspector: &Inspector, cx: &LookCx, card: &bpe_core::Slot) -> Option<LayerId> {
    picked(inspector, card, cx.key())
}

/// The card's list of `section` layers, the top one first, each with its sub-layers.
fn list(
    ui: &mut Ui,
    inspector: &mut Inspector,
    cx: &LookCx,
    card: &bpe_core::Slot,
    picked: LayerId,
    section: LayerSection,
    edits: &mut Vec<Edit>,
) {
    let tops: Vec<&bpe_core::Layer> = layer_list::top_level(card).collect();
    let mut rows = Vec::new();
    for layer in tops.iter().rev() {
        rows.push((layer.id, layer_list::piece(ui, inspector, cx, card, layer, picked, edits)));
    }
    layer_list::section_drop_target(ui, cx, card, &rows, section, edits);
}

#[cfg(test)]
mod tests {
    use crate::inspector::Inspector;
    use crate::selection::Selection;
    use bpe_core::{Resolved, Scene, SlotId, ThingId, resolve};
    use bpe_kit::Kit;
    use bpe_kit::things::particles::forces::{DRAG, GRAVITY};
    use bpe_kit::things::particles::look::{SPRITES, STREAKS, SUB_GLOW};
    use bpe_kit::things::particles::looks::{Look, add_particles};
    use eframe::egui;
    use egui_kittest::{Harness, kittest::Queryable};

    struct State {
        inspector: Inspector,
        scene: Scene,
        kit: Kit,
        resolved: Resolved,
        selection: Selection,
    }

    /// The blank project with an empty Particles thing, selected.
    fn harness() -> (Harness<'static, State>, ThingId) {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_studio_scene(&kit.registry);
        let id = add_particles(&mut scene, &kit.registry, Look::Empty, ThingId::ROOT).unwrap();
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
    fn the_forces_list_adds_forces_without_touching_the_look() {
        let (mut h, id) = harness();
        assert!(h.query_by_label("Physics").is_none(), "a Particles thing has no Physics card");
        assert!(h.query_by_label("Meets things").is_some());
        let look_pick = h.state().inspector.selected_layer.get(&id).copied();
        h.get_by_label("+ Add force").click();
        h.run();
        h.get_by_label("Gravity").click();
        h.run();
        h.get_by_label("+ Add force").click();
        h.run();
        h.get_by_label("Drag").click();
        h.run();
        assert_eq!(kinds(&h, id, SlotId::Motion), [GRAVITY, DRAG]);
        assert_eq!(kinds(&h, id, SlotId::Look), [SPRITES], "the look is as it was");
        assert_eq!(h.state().inspector.selected_layer.get(&id).copied(), look_pick, "the Look's pick stays");
        // The forces' pick is the Motion card's own: the new Drag.
        let key = super::super::pick_key(id, SlotId::Motion);
        let drag = h.state().scene.things[&id].slots[&SlotId::Motion].layers[1].id;
        assert_eq!(h.state().inspector.selected_layer.get(&key), Some(&drag));
    }

    #[test]
    fn the_look_list_adds_a_streaks_layer_and_a_glow_on_it() {
        let (mut h, id) = harness();
        h.get_by_label("+ Add layer").click();
        h.run();
        h.get_by_label("Streaks").click();
        h.run();
        assert_eq!(kinds(&h, id, SlotId::Look), [SPRITES, STREAKS]);
        let streaks = h.state().scene.things[&id].slots[&SlotId::Look].layers[1].id;
        // The glow goes on through the core edit the "+ Sub-layer" menu makes.
        let s = h.state_mut();
        s.scene.add_sub_layer(&s.kit.registry, id, SlotId::Look, streaks, SUB_GLOW).unwrap();
        s.resolved = resolve(&s.scene, &s.kit.registry);
        h.run();
        let card = &h.state().scene.things[&id].slots[&SlotId::Look];
        assert_eq!(card.layers.iter().filter(|l| l.parent == Some(streaks)).count(), 1);
        assert!(h.query_by_label("Glow").is_some(), "the glow is listed under its layer");
    }
}

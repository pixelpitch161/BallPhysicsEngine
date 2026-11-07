//! A Particles thing's two layered cards: its **Motion** (a list of forces, the top one applied
//! last) and its **Look** (a list of draw layers with their sub-layers, the top one drawn last),
//! each with an add menu, drag to reorder, the row menus, and under the list the picked layer's
//! settings. No ball view and no reactions: the particles are seen in the preview.

use bpe_core::{Choice, LayerId, LayerSection, Settings, SlotId, Value};
use bpe_kit::things::particles::look::{
    ENGINE_KEY, Engine, FLAME, LookCardSettings, PILES, SMOKE, SUB_FLASH, SUB_HUE, SUB_RECOLOUR, WEB,
};
use bpe_kit::things::particles::more_forces::FLOCK;
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
    let look = cx.scene.things.get(&cx.id).and_then(|t| t.slots.get(&SlotId::Look));
    let swarm = look.is_some_and(|l| LookCardSettings::from_params(&l.params).engine == Engine::HugeSwarm);
    if swarm && card.layer(picked).is_some_and(|l| l.kind == FLOCK) {
        let words = "Not in Huge swarm: a swarm doesn't flock. Switch the Look's Engine to Exact to use it.";
        ui.label(RichText::new(words).small().color(style::PROBLEM));
    }
    picked_settings(ui, inspector, cx, card, picked, false, edits);
}

/// What a Huge swarm leaves out, under the Engine chips.
const SWARM_LEAVES_OUT: &str = "Huge swarm moves up to 4,000,000 particles on the graphics card, for decoration. It \
    doesn't draw Soft smoke, Flame, Web lines, Piles, Trails, Marks that stay or Liquid layers, Fine detail, the \
    Experimental fold, or the Recolour, Flash on hit and Colour on hit sub-layers. It doesn't Flock, push the balls \
    back (two-way), feed other particles or play their moments; \
    Stick bounces off, Settle and Pile up stop, Pop vanishes; Rise doesn't heat the air, and a word's, picture's or \
    grid's crowd isn't refilled.";

/// The Look card's Engine chips: Exact or Huge swarm.
fn engine_row(ui: &mut Ui, card: &bpe_core::Slot, edits: &mut Vec<Edit>) -> Engine {
    let engine = LookCardSettings::from_params(&card.params).engine;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Engine").color(style::MUTED));
        let choices = [
            (Engine::Exact, "Exact", "Moved on the processor: exactly the same on every computer, up to 1,000,000."),
            (Engine::HugeSwarm, "Huge swarm", "Moved on the graphics card: up to 4,000,000, for decoration."),
        ];
        for (choice, text, help) in choices {
            if crate::gradient_tabs::chip(ui, text, engine == choice, format!("Engine {text}"), help)
                && engine != choice
            {
                edits.push(Edit::SetSlot(SlotId::Look, ENGINE_KEY.into(), Value::Choice(choice.name().into())));
            }
        }
    });
    if engine == Engine::HugeSwarm {
        ui.label(RichText::new(SWARM_LEAVES_OUT).small().color(style::FAINT));
    }
    engine
}

/// Whether a Huge swarm draws a layer of `kind` (a draw layer or a sub-layer).
fn swarm_draws(kind: &str) -> bool {
    !matches!(kind, SMOKE | FLAME | WEB | PILES | SUB_RECOLOUR | SUB_FLASH | SUB_HUE)
}

/// The Look card: its Engine, then its draw layers.
pub fn show_look(ui: &mut Ui, inspector: &mut Inspector, cx: &LookCx, edits: &mut Vec<Edit>) {
    let Some(card) = cx.card() else { return };
    let engine = engine_row(ui, card, edits);
    list_header(ui, cx, card, "Layers", "+ Add layer", LayerSection::Drawn, edits);
    let Some(picked) = picked_on(inspector, cx, card) else {
        let words = "None yet: the particles are there but nothing draws them. Add Sprites or Streaks.";
        ui.label(RichText::new(words).small().color(style::FAINT));
        return;
    };
    list(ui, inspector, cx, card, picked, LayerSection::Drawn, edits);
    let left_out = card.layer(picked).is_some_and(|l| !swarm_draws(&l.kind));
    if engine == Engine::HugeSwarm && left_out {
        let words = "Not drawn in Huge swarm: switch the Engine to Exact to see this layer.";
        ui.label(RichText::new(words).small().color(style::PROBLEM));
    }
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
    use bpe_core::Settings;
    use bpe_core::{Resolved, Scene, SlotId, ThingId, resolve};
    use bpe_kit::Kit;
    use bpe_kit::things::particles::forces::{DRAG, GRAVITY};
    use bpe_kit::things::particles::look::{SPRITES, STREAKS, SUB_GLOW};
    use bpe_kit::things::particles::looks::{Look, add_particles};
    use bpe_kit::things::particles::more_forces::{FLOCK, PULL, SPRING};
    use bpe_kit::things::particles::{BornFrom, ParticlesSettings, WordFrom};
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
        assert!(h.query_all_by_label("Meets things").next().is_some());
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
    fn the_forces_list_offers_part_3_s_forces() {
        let (mut h, id) = harness();
        for name in ["Pull to", "Spring home", "Flock"] {
            h.get_by_label("+ Add force").click();
            h.run();
            h.get_by_label(name).click();
            h.run();
        }
        assert_eq!(kinds(&h, id, SlotId::Motion), [PULL, SPRING, FLOCK]);
        assert!(h.query_all_by_label("Toward").next().is_none(), "the Flock is picked, not the Pull");
    }

    #[test]
    fn a_picture_source_has_a_png_picker_and_a_grid_its_rows() {
        let (mut h, id) = harness();
        let set = |h: &mut Harness<'static, State>, s: ParticlesSettings| {
            let st = h.state_mut();
            st.scene.things.get_mut(&id).unwrap().params = s.to_params();
            st.resolved = resolve(&st.scene, &st.kit.registry);
            h.run();
        };
        let picture = ParticlesSettings {
            from: BornFrom::AWordOrPicture,
            word_from: WordFrom::Picture,
            ..ParticlesSettings::default()
        };
        set(&mut h, picture);
        assert!(h.query_all_by_label("Browse Picture").next().is_some());
        assert!(h.query_all_by_label("Text").next().is_none());
        set(&mut h, ParticlesSettings { from: BornFrom::AGrid, ..ParticlesSettings::default() });
        assert!(h.query_all_by_label("Covers").next().is_some());
        assert!(h.query_all_by_label("Spacing").next().is_some());
        assert!(h.query_all_by_label("Browse Picture").next().is_none());
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

    #[test]
    fn following_the_air_gives_the_scene_its_air_card() {
        let (mut h, id) = harness();
        assert!(!h.state().scene.things[&ThingId::ROOT].slots.contains_key(&SlotId::Air));
        h.get_by_label("+ Add force").click();
        h.run();
        h.get_by_label("Follow the air").click();
        h.run();
        assert_eq!(kinds(&h, id, SlotId::Motion), [bpe_kit::things::particles::forces::AIR]);
        assert_eq!(h.state().scene.things[&ThingId::ROOT].slots[&SlotId::Air].kind, bpe_kit::modules::air::AIR);
        // The scene settings show it, with its wind.
        h.state_mut().selection.set(ThingId::ROOT);
        h.run();
        assert!(h.query_all_by_label("Air").next().is_some());
        assert!(h.query_all_by_label("Calms down").next().is_some());
    }

    #[test]
    fn the_look_list_offers_the_part_6_layers() {
        use bpe_kit::things::particles::liquid::LIQUID;
        use bpe_kit::things::particles::marks::MARKS;
        use bpe_kit::things::particles::trail::TRAILS;
        let (mut h, id) = harness();
        for (name, kind) in [("Trails", TRAILS), ("Marks that stay", MARKS), ("Liquid", LIQUID)] {
            h.get_by_label("+ Add layer").click();
            h.run();
            h.get_by_label(name).click();
            h.run();
            assert_eq!(kinds(&h, id, SlotId::Look).last().map(String::as_str), Some(kind), "{name} added");
        }
        // The picked layer is the Liquid: its own settings, the Experimental fold, no colour split.
        assert!(h.query_all_by_label("Blob size").next().is_some(), "the Liquid's settings show");
        assert!(h.query_all_by_label("Experimental").next().is_some(), "with the Experimental fold");
        assert!(h.query_all_by_label("Colour split").next().is_none(), "a liquid has no colour split");
    }

    #[test]
    fn the_engine_chips_switch_to_a_huge_swarm_and_say_what_it_leaves_out() {
        use bpe_core::Settings;
        use bpe_kit::things::particles::look::{Engine, LookCardSettings};
        let (mut h, id) = harness();
        let engine = |h: &Harness<'_, State>| {
            LookCardSettings::from_params(&h.state().scene.things[&id].slots[&SlotId::Look].params).engine
        };
        assert_eq!(engine(&h), Engine::Exact);
        assert!(h.query_by_label_contains("doesn't draw Soft smoke").is_none(), "nothing left out in Exact");
        h.get_by_label("Engine Huge swarm").click();
        h.run();
        assert_eq!(engine(&h), Engine::HugeSwarm);
        assert!(h.query_by_label_contains("doesn't draw Soft smoke").is_some());
        h.get_by_label("Engine Exact").click();
        h.run();
        assert_eq!(engine(&h), Engine::Exact);
        assert!(h.query_by_label_contains("doesn't draw Soft smoke").is_none());
    }
}

use bpe_core::SlotId;
use bpe_kit::modules::look::layers as l;
use eframe::egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::*;
use crate::app::Studio;
use crate::inspector::InspectorTab;
use crate::sound_tab::tests::{BALL, SHAPE, studio_harness};

/// The studio's inspector on thing `id`'s Looks tab.
fn on_looks(id: ThingId) -> Harness<'static, (Studio, f32)> {
    let mut h = studio_harness(id, false);
    h.get_by_label("Looks").click();
    h.run();
    h
}

fn tile(h: &Harness<'_, (Studio, f32)>, name: &str) -> bool {
    h.query_all_by_role_and_label(Role::Button, name).next().is_some()
}

fn click_tile(h: &mut Harness<'_, (Studio, f32)>, name: &str) {
    h.query_all_by_role_and_label(Role::Button, name).next().unwrap_or_else(|| panic!("no tile {name}")).click();
    h.run();
}

fn kinds(h: &Harness<'_, (Studio, f32)>, id: ThingId) -> Vec<String> {
    h.state().0.scene.card(id, SlotId::Look).unwrap().layers.iter().map(|x| x.kind.clone()).collect()
}

#[test]
fn a_ball_has_a_looks_tab_with_the_ball_looks() {
    let h = on_looks(BALL);
    assert_eq!(h.state().0.panels.inspector.tab.get(&BALL), Some(&InspectorTab::Looks));
    assert!(h.state().0.panels.inspector.wide_open(&h.state().0.scene, Some(BALL)), "the inspector is wide");
    for look in ready_looks::all().iter().filter(|x| x.wears == Wears::Balls) {
        assert!(tile(&h, look.name), "{} has a tile", look.name);
    }
    assert!(!tile(&h, "Neon tube"), "no shape looks on a ball");
    assert!(h.query_by_label("Use this look").is_some());
}

#[test]
fn use_this_look_puts_the_picked_look_on_and_undo_takes_it_off() {
    let mut h = on_looks(BALL);
    let before = h.state().0.scene.card(BALL, SlotId::Look).cloned();
    click_tile(&mut h, "Chrome");
    assert_eq!(h.state().0.scene.card(BALL, SlotId::Look).cloned(), before, "picking alone changes nothing");
    h.get_by_label("Use this look").click();
    h.run();
    assert!(kinds(&h, BALL).iter().any(|k| k == l::SHINE), "it wears Chrome: {:?}", kinds(&h, BALL));
    let name = h.state().0.scene.things[&BALL].name.clone();
    assert!(h.query_by_label(&format!("{name} wears Chrome. Ctrl+Z undoes it.")).is_some());
    h.state_mut().0.undo();
    h.run();
    assert_eq!(h.state().0.scene.card(BALL, SlotId::Look).cloned(), before, "one undo takes it off");
}

#[test]
fn add_its_layers_on_top_keeps_the_body() {
    let mut h = on_looks(BALL);
    let before = kinds(&h, BALL);
    click_tile(&mut h, "Neon ring");
    h.get_by_label("Add its layers on top").click();
    h.run();
    let after = kinds(&h, BALL);
    assert_eq!(after.len(), before.len() + 2, "{after:?}");
    assert_eq!(after[0], l::BODY, "the Body is still the ball's own");
    // The old layers stay in their order; the new ones go in where their sections are.
    let mut rest = after.iter();
    assert!(before.iter().all(|k| rest.any(|a| a == k)), "{before:?} in {after:?}");
}

#[test]
fn a_family_and_the_find_box_narrow_the_tiles() {
    let mut h = on_looks(BALL);
    h.get_by_label("Fire  6").click();
    h.run();
    assert!(tile(&h, "Lava"));
    assert!(!tile(&h, "Chrome"));
    h.get_by_label("All  30").click();
    h.run();
    h.get_by_label("Find a look").click();
    h.run();
    h.get_by_label("Find a look").type_text("neon");
    h.run();
    assert!(tile(&h, "Neon ring"));
    assert!(!tile(&h, "Chrome"));
}

#[test]
fn a_shape_has_settings_looks_and_sound() {
    let mut h = studio_harness(SHAPE, false);
    for tab in ["Settings", "Looks", "Sound"] {
        assert!(h.query_by_label(tab).is_some(), "{tab}");
    }
    h.get_by_label("Looks").click();
    h.run();
    assert!(tile(&h, "Neon tube"));
    assert!(!tile(&h, "Chrome"), "no ball looks on a shape");
    // A plain shape has no colours of its own to keep.
    h.get_by_label("Keep mine").click();
    h.run();
    assert_eq!(h.state().0.panels.inspector.looks.colours, Colours::Look);
    click_tile(&mut h, "Neon tube");
    h.get_by_label("Use this look").click();
    h.run();
    let glow = bpe_kit::modules::look::shape::GLOW;
    assert!(kinds(&h, SHAPE).iter().any(|k| k == glow), "{:?}", kinds(&h, SHAPE));
}

#[test]
fn keep_mine_puts_the_ball_s_own_colours_in() {
    let mut h = on_looks(BALL);
    let pink = bpe_core::Gradient::solid(bpe_core::Color::rgb(0xff, 0x00, 0x88));
    {
        let scene = &mut h.state_mut().0.scene;
        let body = scene.card(BALL, SlotId::Look).unwrap().layers[0].id;
        let key = bpe_core::layered_key(body, "colours");
        scene.card_mut(BALL, SlotId::Look).unwrap().params.set(&key, bpe_core::Value::Gradient(pink.clone()));
    }
    h.run();
    h.get_by_label("Keep mine").click();
    h.run();
    click_tile(&mut h, "Neon ring");
    h.get_by_label("Use this look").click();
    h.run();
    let scene = &h.state().0.scene;
    let card = scene.card(BALL, SlotId::Look).unwrap();
    let outline = card.layers.iter().find(|x| x.kind == l::OUTLINE).unwrap().id;
    let got = card.params.get(&bpe_core::layered_key(outline, "colour")).cloned();
    assert_eq!(got, Some(bpe_core::Value::Gradient(pink)));
}

#[test]
fn keep_mine_is_greyed_for_a_look_with_colours_of_its_own() {
    let mut h = on_looks(BALL);
    click_tile(&mut h, "Chrome");
    h.get_by_label("Keep mine").click();
    h.run();
    assert_eq!(h.state().0.panels.inspector.looks.colours, Colours::Look, "Chrome has colours of its own");
    click_tile(&mut h, "Neon ring");
    h.get_by_label("Keep mine").click();
    h.run();
    assert_eq!(h.state().0.panels.inspector.looks.colours, Colours::Mine);
}

#[test]
fn each_press_is_an_undo_step_of_its_own() {
    let mut h = on_looks(BALL);
    let before = kinds(&h, BALL).len();
    click_tile(&mut h, "Neon ring");
    h.get_by_label("Add its layers on top").click();
    h.run();
    h.get_by_label("Add its layers on top").click();
    h.run();
    assert_eq!(kinds(&h, BALL).len(), before + 4);
    h.state_mut().0.undo();
    h.run();
    assert_eq!(kinds(&h, BALL).len(), before + 2, "one undo takes back one press");
}

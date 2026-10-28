//! Drives the Text tab with simulated input.

use crate::inspector::{self, Inspector, Tab};
use bpe_scene::{Caption, CaptionStyle, Scene};
use eframe::egui;
use egui_kittest::{Harness, kittest::Queryable};

struct State {
    inspector: Inspector,
    scene: Scene,
}

fn harness(scene: Scene) -> Harness<'static, State> {
    let state = State { inspector: Inspector { tab: Tab::Text, ..Inspector::default() }, scene };
    let mut harness = Harness::builder()
        .with_size(egui::vec2(460.0, 2400.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(240)
        .allow_missing_glyphs()
        .with_context_setup(crate::theme::apply)
        .build_ui_state(|ui, state: &mut State| inspector::show(ui, &mut state.inspector, &mut state.scene), state);
    harness.run();
    harness
}

fn with_caption(text: &str, style: CaptionStyle) -> Scene {
    Scene { captions: vec![Caption::new(text, style)], ..Scene::default() }
}

#[test]
fn adding_a_caption_from_a_template() {
    let mut harness = harness(Scene::default());
    harness.get_by_label_contains("No captions yet");
    harness.get_by_label("+ Add caption").click();
    harness.run();
    harness.get_by_label("Ring {escapes}/{rings}").click();
    harness.run();
    let captions = &harness.state().scene.captions;
    assert_eq!(captions.len(), 1);
    assert_eq!(captions[0].text, "Ring {escapes}/{rings}");
    assert_eq!(captions[0].style, CaptionStyle::Counter);
}

#[test]
fn the_token_picker_adds_live_numbers() {
    let mut harness = harness(with_caption("Balls:", CaptionStyle::Counter));
    harness.get_by_label("{ }").click();
    harness.run();
    harness.get_by_label("{balls}").click();
    harness.run();
    assert_eq!(harness.state().scene.captions[0].text, "Balls: {balls}");
}

#[test]
fn picking_a_style_moves_the_caption() {
    let mut harness = harness(with_caption("Hi", CaptionStyle::Hook));
    let before = harness.state().scene.captions[0].position;
    harness.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Style").click();
    harness.run();
    harness.get_by_label("Subtitle").click();
    harness.run();
    let caption = &harness.state().scene.captions[0];
    assert_eq!(caption.style, CaptionStyle::Subtitle);
    assert_ne!(caption.position, before);
    assert_eq!(caption.text, "Hi");
}

#[test]
fn showing_until_a_time_instead_of_the_end() {
    let mut harness = harness(with_caption("Hi", CaptionStyle::Hook));
    assert_eq!(harness.state().scene.captions[0].show_to, 0.0);
    harness.get_by_label("the end").click();
    harness.run();
    assert!(harness.state().scene.captions[0].show_to > 0.0);
    harness.get_by_label("the end").click();
    harness.run();
    assert_eq!(harness.state().scene.captions[0].show_to, 0.0);
}

#[test]
fn removing_and_duplicating_captions() {
    let mut harness = harness(with_caption("Hi", CaptionStyle::Hook));
    harness.get_by_label("···").click();
    harness.run();
    harness.get_by_label("Duplicate").click();
    harness.run();
    assert_eq!(harness.state().scene.captions.len(), 2);
    harness.get_all_by_label("···").next().unwrap().click();
    harness.run();
    harness.get_by_label("Remove").click();
    harness.run();
    assert_eq!(harness.state().scene.captions.len(), 1);
}

#[test]
fn the_dice_adds_or_restyles_captions() {
    let mut harness = harness(Scene::escape_rings());
    harness.get_by_label("Roll random caption settings").click();
    harness.run();
    let state = harness.state();
    assert!(!state.scene.captions.is_empty());
    assert!(state.inspector.rolled.as_deref().is_some_and(|s| s.contains("caption")));
    assert_eq!(Scene { captions: Vec::new(), ..state.scene.clone() }, Scene::escape_rings());
}

#[test]
fn reset_this_tab_removes_every_caption() {
    let mut harness = harness(with_caption("Hi", CaptionStyle::Hook));
    harness.get_by_label("Reset this tab").click();
    harness.run();
    assert!(harness.state().scene.captions.is_empty());
}

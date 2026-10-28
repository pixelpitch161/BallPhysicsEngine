//! Drives the settings panel with simulated mouse input to check the reset shortcuts.

use crate::inspector::{self, Inspector, Tab};
use bpe_scene::{
    ArenaMotion, BounceRules, ChordStep, Color, Container, Effect, Instrument, Look, Physics, Scene, Sound, Spawner,
    Vec2,
};
use eframe::egui::{self, Modifiers, PointerButton};
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};

struct State {
    inspector: Inspector,
    scene: Scene,
}

fn harness(tab: Tab, scene: Scene) -> Harness<'static, State> {
    let state = State { inspector: Inspector { tab, ..Inspector::default() }, scene };
    let mut harness = Harness::builder()
        .with_size(egui::vec2(460.0, 2400.0))
        // Real frame times, so two quick clicks count as a double-click.
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(240)
        // The test fonts lack the colour picker's copy icon.
        .allow_missing_glyphs()
        .with_context_setup(crate::theme::apply)
        .build_ui_state(|ui, state: &mut State| inspector::show(ui, &mut state.inspector, &mut state.scene), state);
    harness.run();
    harness
}

fn edited_physics() -> Scene {
    let mut scene = Scene::default();
    scene.physics.gravity = Vec2::new(250.0, 900.0);
    scene.physics.restitution = 0.5;
    scene
}

fn double_click(harness: &mut Harness<'_, State>, label: &str) {
    harness.get_by_label(label).click();
    harness.step();
    harness.get_by_label(label).click();
    harness.run();
}

fn alt_click(harness: &mut Harness<'_, State>, label: &str) {
    harness.event(egui::Event::ModifiersChanged(Modifiers::ALT));
    harness.step();
    harness.get_by_label(label).click();
    harness.step();
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    harness.run();
}

/// The slider in the same grid row as a setting's name.
fn slider_rect(harness: &Harness<'_, State>, label: &str) -> egui::Rect {
    let name = harness.get_by_label(label).rect();
    harness
        .get_all_by_role(egui::accesskit::Role::Slider)
        .map(|node| node.rect())
        .find(|rect| rect.y_range().contains(name.center().y) && rect.left() > name.left())
        .unwrap_or_else(|| panic!("no slider beside {label}"))
}

fn click_at(harness: &mut Harness<'_, State>, pos: egui::Pos2) {
    harness.hover_at(pos);
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
}

#[test]
fn double_clicking_a_name_resets_it() {
    let mut harness = harness(Tab::Arena, edited_physics());
    double_click(&mut harness, "Gravity");
    assert_eq!(harness.state().scene.physics.gravity.y, Physics::default().gravity.y);
    // Only that setting.
    assert_eq!(harness.state().scene.physics.gravity.x, 250.0);
    assert_eq!(harness.state().scene.physics.restitution, 0.5);
}

#[test]
fn a_single_click_on_a_name_changes_nothing() {
    let mut harness = harness(Tab::Arena, edited_physics());
    harness.get_by_label("Gravity").click();
    harness.run();
    assert_eq!(harness.state().scene.physics.gravity.y, 900.0);
}

#[test]
fn two_slow_clicks_on_a_name_change_nothing() {
    let mut harness = harness(Tab::Arena, edited_physics());
    harness.get_by_label("Gravity").click();
    harness.run_steps(30);
    harness.get_by_label("Gravity").click();
    harness.run();
    assert_eq!(harness.state().scene.physics.gravity.y, 900.0);
}

#[test]
fn alt_clicking_a_name_resets_it() {
    let mut harness = harness(Tab::Arena, edited_physics());
    alt_click(&mut harness, "Bounciness");
    assert_eq!(harness.state().scene.physics.restitution, Physics::default().restitution);
    assert_eq!(harness.state().scene.physics.gravity.y, 900.0);
}

#[test]
fn double_clicking_a_slider_resets_it() {
    let mut harness = harness(Tab::Arena, edited_physics());
    let pos = slider_rect(&harness, "Gravity").left_center() + egui::vec2(20.0, 0.0);
    click_at(&mut harness, pos);
    harness.step();
    click_at(&mut harness, pos);
    harness.run();
    assert_eq!(harness.state().scene.physics.gravity.y, Physics::default().gravity.y);
}

#[test]
fn alt_clicking_a_slider_resets_it() {
    let mut harness = harness(Tab::Arena, edited_physics());
    let pos = slider_rect(&harness, "Gravity").left_center() + egui::vec2(20.0, 0.0);
    harness.event(egui::Event::ModifiersChanged(Modifiers::ALT));
    harness.step();
    click_at(&mut harness, pos);
    harness.step();
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    harness.run();
    assert_eq!(harness.state().scene.physics.gravity.y, Physics::default().gravity.y);
}

#[test]
fn a_plain_slider_click_still_moves_the_value() {
    let mut harness = harness(Tab::Arena, edited_physics());
    let pos = slider_rect(&harness, "Gravity").left_center() + egui::vec2(20.0, 0.0);
    click_at(&mut harness, pos);
    harness.run();
    let gravity = harness.state().scene.physics.gravity.y;
    assert!(gravity < 0.0, "a click near the left end should pick a strong upward gravity, got {gravity}");
}

#[test]
fn right_clicking_a_name_offers_reset() {
    let mut harness = harness(Tab::Arena, edited_physics());
    harness.get_by_label("Air drag").click_secondary();
    harness.run();
    // Air drag is already at its default, so the menu item is there but greyed out.
    assert!(harness.get_by_label("Reset to default").accesskit_node().is_disabled());
    harness.get_by_label("Bounciness").click_secondary();
    harness.run();
    harness.get_by_label("Reset to default").click();
    harness.run();
    assert_eq!(harness.state().scene.physics.restitution, Physics::default().restitution);
    assert!(harness.query_by_label("Reset to default").is_none(), "the menu closes after resetting");
}

#[test]
fn double_clicking_a_number_box_types_instead_of_resetting() {
    let mut harness = harness(Tab::Arena, edited_physics());
    let rect = slider_rect(&harness, "Gravity");
    // The number box sits just right of the slider track.
    let pos = rect.right_center() + egui::vec2(30.0, 0.0);
    click_at(&mut harness, pos);
    harness.step();
    click_at(&mut harness, pos);
    harness.run();
    assert_eq!(harness.state().scene.physics.gravity.y, 900.0);
    assert!(harness.ctx.text_edit_focused(), "the number box should be ready to type into");
}

#[test]
fn alt_clicking_a_number_box_resets_it_without_typing() {
    let mut harness = harness(Tab::Arena, edited_physics());
    let pos = slider_rect(&harness, "Gravity").right_center() + egui::vec2(30.0, 0.0);
    harness.event(egui::Event::ModifiersChanged(Modifiers::ALT));
    harness.step();
    click_at(&mut harness, pos);
    harness.step();
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    harness.run();
    assert_eq!(harness.state().scene.physics.gravity.y, Physics::default().gravity.y);
    assert!(!harness.ctx.text_edit_focused());
}

fn launch(scene: &Scene) -> Vec2 {
    match &scene.spawners[0] {
        Spawner::Grid { velocity, .. } | Spawner::Single { velocity, .. } | Spawner::Stream { velocity, .. } => {
            *velocity
        }
    }
}

fn single_ball(velocity: Vec2) -> Scene {
    let spawners =
        vec![Spawner::Single { position: Vec2::new(40.0, -30.0), velocity, radius: 33.0, color: Color::WHITE }];
    Scene { spawners, ..Scene::default() }
}

#[test]
fn launch_speed_resets_without_turning_the_ball() {
    // Default for a single ball is (300, -200).
    let mut harness = harness(Tab::Balls, single_ball(Vec2::new(0.0, 1000.0)));
    double_click(&mut harness, "Launch speed");
    let v = launch(&harness.state().scene);
    assert!((v.length() - Vec2::new(300.0, -200.0).length()).abs() < 0.01, "{v}");
    assert!(v.x.abs() < 0.01 && v.y > 0.0, "still straight down: {v}");
}

#[test]
fn direction_resets_without_changing_speed() {
    let mut harness = harness(Tab::Balls, single_ball(Vec2::new(0.0, 1000.0)));
    double_click(&mut harness, "Direction");
    let v = launch(&harness.state().scene);
    assert!((v.length() - 1000.0).abs() < 0.01, "{v}");
    let default = Vec2::new(300.0, -200.0).normalize();
    assert!((v.normalize() - default).length() < 0.001, "{v}");
}

#[test]
fn start_position_resets_to_the_middle() {
    let mut harness = harness(Tab::Balls, single_ball(Vec2::new(300.0, -200.0)));
    harness.get_by_label("More settings").click();
    harness.run();
    // Straight away: egui counts a double-click soon after another click as a triple-click.
    double_click(&mut harness, "Start position");
    match &harness.state().scene.spawners[0] {
        Spawner::Single { position, radius, .. } => {
            assert_eq!(*position, Vec2::ZERO);
            assert_eq!(*radius, 33.0, "other settings stay");
        }
        _ => unreachable!(),
    }
}

#[test]
fn alt_clicking_a_colour_resets_it_and_leaves_the_picker_shut() {
    let mut scene = single_ball(Vec2::new(300.0, -200.0));
    if let Spawner::Single { color, .. } = &mut scene.spawners[0] {
        *color = Color::rgb(1, 2, 3);
    }
    let mut harness = harness(Tab::Balls, scene);
    let name = harness.get_by_label("Colour").rect();
    let swatch = harness
        .get_all_by_role(egui::accesskit::Role::ColorWell)
        .map(|node| node.rect())
        .find(|rect| rect.y_range().contains(name.center().y) && rect.left() > name.right())
        .expect("colour swatch");
    harness.event(egui::Event::ModifiersChanged(Modifiers::ALT));
    harness.step();
    click_at(&mut harness, swatch.center());
    harness.step();
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    harness.run();
    match &harness.state().scene.spawners[0] {
        Spawner::Single { color, .. } => assert_eq!(*color, Color::rgb(0xff, 0x5c, 0xa8)),
        _ => unreachable!(),
    }
    assert!(!egui::Popup::is_any_open(&harness.ctx), "colour picker should stay closed");

    // A plain click still opens the picker, once it can't count as a double-click.
    harness.run_steps(30);
    click_at(&mut harness, swatch.center());
    harness.run();
    assert!(egui::Popup::is_any_open(&harness.ctx), "colour picker should open on a normal click");
}

#[test]
fn reset_this_tab_resets_the_look() {
    let mut scene = Scene::default();
    scene.look.trail_seconds = 1.5;
    scene.look.exposure = 2.0;
    let mut harness = harness(Tab::Look, scene);
    harness.get_by_label("Reset this tab").click();
    harness.run();
    assert_eq!(harness.state().scene.look, Look::default());
    assert!(harness.get_by_label("Reset this tab").accesskit_node().is_disabled(), "nothing left to reset");
}

#[test]
fn reset_this_tab_resets_bounces() {
    let mut scene = Scene::default();
    scene.rules.speed_gain = 1.3;
    scene.rules.recolor = vec![Color::WHITE];
    let mut harness = harness(Tab::Bounces, scene);
    harness.get_by_label("Reset this tab").click();
    harness.run();
    assert_eq!(harness.state().scene.rules, BounceRules::default());
}

#[test]
fn reset_this_tab_keeps_the_arena_shape() {
    let mut scene = edited_physics();
    scene.container = Container::Rect {
        center: Vec2::new(5.0, 5.0),
        size: Vec2::new(300.0, 700.0),
        thickness: 9.0,
        color: Color::WHITE,
    };
    let mut harness = harness(Tab::Arena, scene);
    harness.get_by_label("Reset this tab").click();
    harness.run();
    let state = harness.state();
    assert_eq!(state.scene.physics, Physics::default());
    match state.scene.container {
        Container::Rect { center, size, thickness, .. } => {
            assert_eq!((center, size, thickness), (Vec2::ZERO, Vec2::new(1000.0, 1000.0), 3.0));
        }
        _ => panic!("the box should stay a box"),
    }
}

#[test]
fn reset_this_tab_keeps_chords_and_wav_file() {
    let mut scene = Scene::default();
    let chords = vec![ChordStep { chord: "F#m7".into(), seconds: 2.0 }];
    scene.sound = Sound {
        progression: chords.clone(),
        seed: 42,
        low_note: 30,
        stereo_width: 0.0,
        instrument: Instrument::Sample { path: "C:/sounds/bell.wav".into(), root_note: 72 },
        ..Sound::default()
    };
    let mut harness = harness(Tab::Sound, scene);
    harness.get_by_label("Reset this tab").click();
    harness.run();
    let sound = &harness.state().scene.sound;
    assert_eq!(sound.progression, chords);
    assert_eq!(sound.seed, 42);
    assert_eq!(sound.instrument, Instrument::Sample { path: "C:/sounds/bell.wav".into(), root_note: 60 });
    assert_eq!((sound.low_note, sound.stereo_width), (Sound::default().low_note, Sound::default().stereo_width));
}

#[test]
fn reset_this_tab_works_while_sound_is_off() {
    let mut scene = Scene::default();
    scene.sound.enabled = false;
    let mut harness = harness(Tab::Sound, scene);
    harness.get_by_label("Reset this tab").click();
    harness.run();
    assert!(harness.state().scene.sound.enabled);
}

#[test]
fn opening_a_tab_changes_nothing() {
    for tab in [Tab::Balls, Tab::Arena, Tab::Bounces, Tab::Look, Tab::Effects, Tab::Sound] {
        let harness = harness(tab, Scene::default());
        assert!(harness.state().scene == Scene::default(), "showing a tab must not edit the scene");
    }
    // Effect sliders the scene leaves at their defaults must not get written in just by showing them.
    let scene =
        Scene { effects: vec![Effect::new("echo"), Effect::new("mirror"), Effect::new("pixels")], ..Scene::default() };
    let harness = harness(Tab::Effects, scene.clone());
    assert!(harness.state().scene == scene, "showing the effects must not edit them");
}

fn with_effects(effects: Vec<Effect>) -> Scene {
    Scene { effects, ..Scene::default() }
}

#[test]
fn double_clicking_an_effect_setting_resets_it() {
    let mut effect = Effect::new("kaleidoscope").with("slices", 12.0).with("spin", 0.5);
    effect.amount = 0.3;
    let mut harness = harness(Tab::Effects, with_effects(vec![effect]));
    double_click(&mut harness, "Slices");
    double_click(&mut harness, "Amount");
    let effect = &harness.state().scene.effects[0];
    assert_eq!(effect.params["slices"], 6.0);
    assert_eq!(effect.params["spin"], 0.5, "other sliders keep their values");
    assert_eq!(effect.amount, 1.0);
}

#[test]
fn alt_clicking_an_effect_switch_resets_it() {
    let mut harness = harness(Tab::Effects, with_effects(vec![Effect::new("mirror").with("left_right", 0.0)]));
    alt_click(&mut harness, "Left right");
    assert_eq!(harness.state().scene.effects[0].params["left_right"], 1.0);
}

#[test]
fn effects_are_added_from_the_menu_and_from_looks() {
    let mut harness = harness(Tab::Effects, Scene::default());
    harness.get_by_label("+ Add effect").click();
    harness.run();
    harness.get_by_label("Kaleidoscope").click();
    harness.run();
    assert_eq!(harness.state().scene.effects, vec![Effect::new("kaleidoscope")]);

    harness.get_by_label("Start from a look").click();
    harness.run();
    harness.get_by_label("Retro TV").click();
    harness.run();
    let shaders: Vec<_> = harness.state().scene.effects.iter().map(|e| e.shader.as_str()).collect();
    assert_eq!(shaders, ["pixels", "film", "glitch"]);
}

#[test]
fn reset_this_tab_removes_the_effects() {
    let mut harness = harness(Tab::Effects, with_effects(vec![Effect::new("film"), Effect::new("glitch")]));
    harness.get_by_label("Reset this tab").click();
    harness.run();
    assert!(harness.state().scene.effects.is_empty());
}

#[test]
fn a_missing_effect_is_explained_not_dropped() {
    let harness = harness(Tab::Effects, with_effects(vec![Effect::new("not_installed")]));
    assert!(harness.query_by_label_contains("There is no effect called").is_some());
    assert_eq!(harness.state().scene.effects.len(), 1);
}

/// Drags a slider by `dx` pixels, optionally holding Shift, and returns the new value.
fn drag_gravity(shift: bool, dx: f32) -> f32 {
    let mut harness = harness(Tab::Arena, Scene::default());
    let rect = slider_rect(&harness, "Gravity");
    // Start on the handle, so pressing doesn't jump the value.
    let range = 6000.0;
    let start_x = rect.left() + rect.width() * ((Physics::default().gravity.y + 3000.0) / range);
    let start = egui::pos2(start_x, rect.center().y);
    let before = harness.state().scene.physics.gravity.y;
    if shift {
        harness.event(egui::Event::ModifiersChanged(Modifiers::SHIFT));
    }
    harness.hover_at(start);
    harness.step();
    harness.drag_at(start);
    harness.step();
    for i in 1..=10 {
        harness.hover_at(start + egui::vec2(dx * i as f32 / 10.0, 0.0));
        harness.step();
    }
    harness.drop_at(start + egui::vec2(dx, 0.0));
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    harness.run();
    harness.state().scene.physics.gravity.y - before
}

#[test]
fn shift_drag_moves_a_slider_a_tenth_as_far() {
    let normal = drag_gravity(false, 60.0);
    let fine = drag_gravity(true, 60.0);
    assert!(normal > 100.0, "normal drag moved {normal}");
    let ratio = fine / normal;
    assert!((0.05..0.15).contains(&ratio), "fine drag moved {fine} vs {normal}");
}

#[test]
fn double_clicking_a_colour_resets_it_and_leaves_the_picker_shut() {
    let mut scene = Scene::default();
    scene.look.background_top = Color::rgb(200, 10, 10);
    let mut harness = harness(Tab::Look, scene);
    let name = harness.get_by_label("Top colour").rect();
    let swatch = harness
        .get_all_by_role(egui::accesskit::Role::ColorWell)
        .map(|node| node.rect())
        .find(|rect| rect.y_range().contains(name.center().y))
        .expect("colour swatch");
    click_at(&mut harness, swatch.center());
    harness.step();
    click_at(&mut harness, swatch.center());
    harness.run();
    assert_eq!(harness.state().scene.look.background_top, Look::default().background_top);
    assert!(!egui::Popup::is_any_open(&harness.ctx), "colour picker should be closed");
}

#[test]
fn double_clicking_a_colour_name_resets_it() {
    let mut scene = Scene::default();
    scene.look.background_bottom = Color::rgb(200, 10, 10);
    let mut harness = harness(Tab::Look, scene);
    double_click(&mut harness, "Bottom colour");
    assert_eq!(harness.state().scene.look.background_bottom, Look::default().background_bottom);
}

#[test]
fn whole_number_sliders_reset() {
    let mut scene = Scene::default();
    if let Spawner::Grid { columns, .. } = &mut scene.spawners[0] {
        *columns = 50;
    }
    let mut harness = harness(Tab::Balls, scene);
    double_click(&mut harness, "Balls across");
    match &harness.state().scene.spawners[0] {
        Spawner::Grid { columns, .. } => assert_eq!(*columns, 10),
        _ => unreachable!(),
    }
}

#[test]
fn opening_a_tab_changes_no_preset() {
    for preset in Scene::presets() {
        for tab in [Tab::Balls, Tab::Arena, Tab::Bounces, Tab::Look, Tab::Sound] {
            let harness = harness(tab, preset.scene.clone());
            assert!(harness.state().scene == preset.scene, "showing a tab must not edit {}", preset.name);
        }
    }
}

#[test]
fn reset_this_tab_keeps_the_polygon_and_pegs() {
    let mut scene = Scene::plinko();
    scene.container = Container::Polygon {
        center: Vec2::new(5.0, 5.0),
        radius: 300.0,
        sides: 5,
        rotation: 30.0,
        thickness: 9.0,
        color: Color::WHITE,
    };
    scene.arena_motion.spin = 90.0;
    // Just the pegs, so the whole tab fits in the test window.
    scene.obstacles.truncate(1);
    let pegs = scene.obstacles.clone();
    let mut harness = harness(Tab::Arena, scene);
    harness.get_by_label("Reset this tab").click();
    harness.run();
    let state = harness.state();
    assert_eq!(state.scene.arena_motion, ArenaMotion::default());
    assert_eq!(state.scene.obstacles, pegs);
    assert!(matches!(state.scene.container, Container::Polygon { center: Vec2::ZERO, sides: 6, .. }));
}

#[test]
fn picking_escape_rings_keeps_the_arena_position() {
    let mut scene = Scene::default();
    *scene.container.center_mut() = Vec2::new(0.0, 100.0);
    let mut harness = harness(Tab::Arena, scene);
    harness.get_by_label("Escape rings").click();
    harness.run();
    let container = &harness.state().scene.container;
    assert!(matches!(container, Container::Rings { .. }));
    assert_eq!(container.center(), Vec2::new(0.0, 100.0));
    // The ring settings show up straight away.
    harness.get_by_label("Gap size");
}

#[test]
fn turning_off_the_speed_limit_hides_top_speed() {
    let mut harness = harness(Tab::Bounces, Scene::default());
    assert!(harness.query_by_label("Top speed").is_some());
    harness.get_by_label("Cap the top speed").click();
    harness.run();
    assert!(!harness.state().scene.rules.limit_speed);
    assert!(harness.query_by_label("Top speed").is_none());
    harness.get_by_label("Cap the top speed").click();
    harness.run();
    assert!(harness.state().scene.rules.limit_speed);
}

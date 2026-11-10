//! The Scenes window as a person uses it: the toolbar button, filters, Find a scene, picking, opening and closing.

use super::*;
use crate::app::{Pending, Studio};
use eframe::egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

/// The studio's toolbar with the Scenes window over it, drawing no pictures.
fn studio_harness(studio: Studio) -> Harness<'static, Studio> {
    let mut studio = studio;
    studio.panels.scenes.pictures = Pictures::none();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1440.0, 960.0))
        .with_step_dt(0.05)
        .allow_missing_glyphs()
        .with_context_setup(style::apply)
        .build_ui_state(
            |ui, s: &mut Studio| {
                egui::Panel::top("toolbar").show(ui, |ui| s.toolbar(ui));
                let ctx = ui.ctx().clone();
                s.take_toolbar_click(&ctx);
                s.scenes_window(&ctx);
            },
            studio,
        );
    h.run();
    h
}

fn opened(studio: Studio) -> Harness<'static, Studio> {
    let mut h = studio_harness(studio);
    h.get_by_label("Scenes").click();
    h.run();
    assert!(h.state().panels.scenes.open, "the Scenes button opens the window");
    h
}

fn has(h: &Harness<'_, Studio>, label: &str) -> bool {
    h.query_all_by_label(label).next().is_some()
}

fn tile(h: &Harness<'_, Studio>, name: &str) -> bool {
    h.query_all_by_role_and_label(Role::Button, &format!("Scene {name}")).next().is_some()
}

fn click(h: &mut Harness<'_, Studio>, label: &str) {
    h.query_all_by_label(label).next().unwrap_or_else(|| panic!("nothing called {label}")).click();
    h.run();
}

#[test]
fn scenes_button_opens_the_window() {
    let h = opened(Studio::headless());
    for e in library::all() {
        assert!(tile(&h, e.info.name), "{} has a tile", e.info.name);
    }
    assert!(has(&h, "Find a scene") && has(&h, "Open scene") && has(&h, "Close scenes"));
    let count = format!("{} scenes, from simple starters to full spectacles", library::all().len());
    assert!(has(&h, &count));
}

#[test]
fn level_filter_narrows_the_tiles_and_counts_show() {
    let mut h = opened(Studio::headless());
    let starters = library::all().iter().filter(|e| e.info.level == Level::Starter).count();
    click(&mut h, "Level Spectacles");
    let spectacles = library::all().iter().filter(|e| e.info.level == Level::Spectacle).count();
    assert_eq!(library::all().iter().filter(|e| tile(&h, e.info.name)).count(), spectacles);
    click(&mut h, "Level Starters");
    assert_eq!(library::all().iter().filter(|e| tile(&h, e.info.name)).count(), starters);
    click(&mut h, "Kind Hypnotic");
    assert!(tile(&h, "String art") && !tile(&h, "First bounce"), "a kind narrows too");
    click(&mut h, "Kind Hypnotic");
    assert!(tile(&h, "First bounce"), "clicking the kind that is on turns it off");
    click(&mut h, "Level All");
    assert!(library::all().iter().all(|e| tile(&h, e.info.name)));
}

#[test]
fn find_box_filters_and_clear_brings_them_back() {
    let mut h = opened(Studio::headless());
    h.get_by_label("Find a scene").click();
    h.run();
    h.get_by_label("Find a scene").type_text("zebra");
    h.run();
    assert!(has(&h, "No scenes match."));
    assert!(!tile(&h, "First bounce"));
    click(&mut h, "Clear the filters");
    assert!(library::all().iter().all(|e| tile(&h, e.info.name)));
    assert_eq!(h.state().panels.scenes.filter, Filter::default(), "every filter is cleared");
    h.get_by_label("Find a scene").click();
    h.run();
    h.get_by_label("Find a scene").type_text("string");
    h.run();
    assert!(tile(&h, "String art") && !tile(&h, "First bounce"));
    assert!(has(&h, "Showing 1 scene for \"string\"."));
}

#[test]
fn clicking_a_tile_shows_its_info() {
    let mut h = opened(Studio::headless());
    let entry = library::find("grow-every-bounce").unwrap();
    click(&mut h, "Scene Grow every bounce");
    assert_eq!(h.state().panels.scenes.picked, Some("grow-every-bounce"));
    assert!(has(&h, entry.info.about), "its description shows");
    assert!(has(&h, "Good things to change first") && has(&h, entry.info.change_first));
    assert!(has(&h, &facts(&entry.info)));
    assert!(has(&h, &format!("Uses: {}", entry.info.uses.join(", "))));
}

#[test]
fn open_scene_replaces_the_project() {
    let mut h = opened(Studio::headless());
    click(&mut h, "Scene Ball in a circle");
    click(&mut h, "Open scene");
    let studio = h.state();
    assert!(!studio.panels.scenes.open, "the window closes");
    assert_eq!(studio.scene.name, "Ball in a circle");
    assert_eq!(studio.path, None, "a new project with no file yet");
    assert!(!studio.unsaved());
    assert_eq!(studio.status, "Opened the built-in scene Ball in a circle. Save it to keep your changes.");
}

#[test]
fn cancel_and_escape_close_and_restore_playing() {
    let mut studio = Studio::headless();
    studio.set_playing(true);
    let mut h = opened(studio);
    assert!(!h.state().playing, "the project pauses while the window is open");
    click(&mut h, "Cancel");
    assert!(!h.state().panels.scenes.open);
    assert!(h.state().playing, "and plays again after");

    click(&mut h, "Scenes");
    assert!(h.state().panels.scenes.open && !h.state().playing);
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(!h.state().panels.scenes.open, "Escape closes it");
    assert!(h.state().playing);

    let mut h = opened(Studio::headless());
    click(&mut h, "Close scenes");
    assert!(!h.state().panels.scenes.open);
    assert!(!h.state().playing, "a project that wasn't playing stays still");
}

#[test]
fn with_no_device_tiles_show_swatches_and_the_preview_says_so() {
    let h = opened(Studio::headless());
    let window = &h.state().panels.scenes;
    assert!(window.pictures.device_missing());
    assert!(library::all().iter().all(|e| window.pictures.poster(e.info.id).is_none()));
    assert!(has(&h, "No preview on this computer"));
}

#[test]
fn closing_mid_preview_stops_the_sound_and_drawing() {
    let mut h = opened(Studio::headless());
    h.state_mut().panels.scenes.sound.play_recording(library::find("first-bounce").unwrap());
    assert!(h.state().panels.scenes.sound.has_job());
    click(&mut h, "Close scenes");
    let window = &h.state().panels.scenes;
    assert!(!window.sound.has_job(), "the preview's sound is dropped");
    assert_eq!(window.sound.previewing(), None);
    assert!(!window.pictures.busy());
}

#[test]
fn opening_a_built_in_scene_makes_an_unsaved_free_new_project() {
    let mut studio = Studio::headless();
    assert!(studio.open_built_in("ball-in-a-circle"));
    assert_eq!(studio.path, None);
    assert_eq!(studio.scene.name, "Ball in a circle");
    assert!(!studio.unsaved());
    let names: Vec<_> = studio.scene.things.values().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"Ring") && names.contains(&"Ball"), "{names:?}");
    assert!(!studio.history.can_undo(), "opening is not an undo step");
}

#[test]
fn opening_with_unsaved_work_asks_first() {
    let mut studio = Studio::headless();
    studio.scene.name = "My work".into();
    assert!(studio.unsaved());
    let ctx = egui::Context::default();
    studio.guard(Pending::OpenBuiltIn("first-bounce"), &ctx);
    assert!(matches!(studio.confirm, Some(Pending::OpenBuiltIn("first-bounce"))));
    assert_eq!(studio.scene.name, "My work", "nothing is opened until the question is answered");
}

#[test]
fn an_unknown_built_in_id_keeps_the_scene() {
    let mut studio = Studio::headless();
    let before = studio.scene.clone();
    assert!(!studio.open_built_in("no-such-scene"));
    assert_eq!(studio.scene, before);
}

#[test]
fn the_words_under_the_tiles_read_right() {
    let none = Filter::default();
    assert_eq!(showing(&none, 8), "Showing all 8 scenes.");
    let words = Filter { words: " escape ".into(), ..Default::default() };
    assert_eq!(showing(&words, 5), "Showing 5 scenes for \"escape\".");
    let level = Filter { level: Some(Level::Starter), ..Default::default() };
    assert_eq!(showing(&level, 1), "Showing 1 scene.");
    assert_eq!(minutes(72.4), "1:12");
    let info = &library::find("first-bounce").unwrap().info;
    assert_eq!(facts(info), "Starter · Bounce and drop · 12 s · one simple sound");
}

#[test]
fn escape_in_the_find_box_only_leaves_it() {
    let mut h = opened(Studio::headless());
    h.get_by_label("Find a scene").click();
    h.run();
    h.get_by_label("Find a scene").type_text("ring");
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.state().panels.scenes.open, "the first Escape leaves the box");
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(!h.state().panels.scenes.open, "the next closes the window");
}

#[test]
fn keys_meant_for_the_editor_dont_reach_it_while_the_window_is_open() {
    let ctx = egui::Context::default();
    let mut window = ScenesWindow { pictures: Pictures::none(), ..Default::default() };
    window.open(false, true);
    let key = |key| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    let input = egui::RawInput {
        events: vec![key(egui::Key::Delete), key(egui::Key::F), key(egui::Key::A)],
        ..Default::default()
    };
    let mut seen = (true, true, false);
    let mut out = ctx.run_ui(input, |ui| {
        window.take_keys(ui.ctx());
        seen =
            ui.input(|i| (i.key_pressed(egui::Key::Delete), i.key_pressed(egui::Key::F), i.key_pressed(egui::Key::A)));
    });
    out.textures_delta.clear();
    assert_eq!(seen, (false, false, true), "Delete and F are taken; other keys still reach the window");
}

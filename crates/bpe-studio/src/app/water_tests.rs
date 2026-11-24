//! The editor holding a Water scene (Water mode part 1): the ball engine stays empty, and what a
//! Water scene has not got yet is hidden or switched off with a sentence saying why.

use super::*;
use bpe_core::{SlotId, ThingId, Value};
use bpe_kit::things::water::{POUR, StartingLook, new_water_scene};

use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};

fn harness<'a>(
    studio: Studio,
    size: egui::Vec2,
    draw: impl FnMut(&mut egui::Ui, &mut Studio) + 'a,
) -> Harness<'a, Studio> {
    Harness::builder()
        .with_size(size)
        .allow_missing_glyphs()
        .with_context_setup(style::apply)
        .build_ui_state(draw, studio)
}

fn has(h: &Harness<Studio>, label: &str) -> bool {
    h.query_all_by_label(label).next().is_some()
}

fn disabled(h: &Harness<Studio>, label: &str) -> bool {
    h.query_all_by_label(label).next().unwrap_or_else(|| panic!("no {label}")).accesskit_node().is_disabled()
}

#[test]
fn a_water_scene_starts_no_ball_jobs_and_keeps_no_mixer() {
    let studio = Studio::headless_water();
    assert!(studio.water());
    assert!(studio.scene.card(ThingId::ROOT, SlotId::Mixer).is_none());
    assert!(studio.aim_job.is_none() && studio.particle_job.is_none());
    assert!(studio.follow.is_none() && studio.light_probe.is_none());
    assert!(studio.playback.world().bodies.is_empty());
    assert!(studio.resolved.problems.is_empty(), "{:?}", studio.resolved.problems);
    assert!(!Studio::headless().water(), "the test scene is a Balls scene");
}

#[test]
fn editing_a_water_scene_restarts_nothing_and_undo_works() {
    let mut studio = Studio::headless_water();
    studio.sound = Some(SoundJob::recording());
    let kit = studio.kit.clone();
    let (print, heard, particles) = (studio.fingerprint, studio.sound_fingerprint, studio.particle_print);
    let before = studio.scene.clone();
    let pour = bpe_kit::add_thing(&mut studio.scene, &kit.registry, POUR, ThingId::ROOT).unwrap();
    studio.after_edit(before.clone());
    // A setting of the scene's own that restarts a Balls scene's physics: nothing to restart here.
    let again = studio.scene.clone();
    studio.scene.card_mut(ThingId::ROOT, SlotId::Water).unwrap().params.set("screens_wide", Value::Int(2));
    studio.scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("tempo", Value::Number(97.0));
    studio.after_edit(again);
    assert_eq!((studio.fingerprint, studio.sound_fingerprint, studio.particle_print), (print, heard, particles));
    assert!(studio.sound.as_ref().unwrap().recorded().is_empty(), "the sound is left alone");
    assert!(studio.resolved.things.contains_key(&pour), "the scene itself is resolved as it is");
    assert!(studio.unsaved());
    while studio.undo() {}
    assert_eq!(studio.scene, before);
    assert!(!studio.resolved.things.contains_key(&pour));
}

#[test]
fn opening_a_water_scene_stops_the_ball_jobs_and_a_balls_scene_starts_them_again() {
    use bpe_kit::things::particles::looks::{Look, add_particles};
    let mut studio = Studio::headless();
    let kit = studio.kit.clone();
    let before = studio.scene.clone();
    add_particles(&mut studio.scene, &kit.registry, Look::DustClouds, ThingId::ROOT).unwrap();
    studio.after_edit(before);
    // Opened as a file is: with the camera every held scene gets.
    let balls = studio.scene.clone();
    studio.replace_scene(balls, None);
    assert!(studio.particle_job.is_some() && !studio.playback.world().bodies.is_empty());
    let balls = studio.scene.clone();
    let (print, particles) = (studio.fingerprint, studio.particle_print);
    studio.sound = Some(SoundJob::recording());
    studio.set_playing(true);

    studio.replace_scene(new_water_scene(&kit.registry, StartingLook::Plain, ""), None);
    assert!(studio.water() && !studio.playing);
    assert!(studio.aim_job.is_none() && studio.particle_job.is_none(), "no ball job is left running");
    assert!(studio.playback.world().bodies.is_empty());
    let sent = studio.sound.as_ref().unwrap().recorded();
    assert_eq!(sent.last(), Some(&"rebuild"), "the sound starts over on nothing, so no note rings on: {sent:?}");
    assert_ne!(studio.fingerprint, print);

    studio.replace_scene(balls, None);
    assert!(!studio.water());
    assert!(studio.particle_job.is_some(), "the particles play again");
    assert!(!studio.playback.world().bodies.is_empty(), "and so do the balls");
    assert_eq!((studio.fingerprint, studio.particle_print), (print, particles));
}

#[test]
fn a_water_scene_cannot_reach_grade_mode_or_a_video() {
    use crate::grade_mode::EditorMode;
    let mut studio = Studio::headless_water();
    studio.mode = EditorMode::Grade;
    studio.keep_out_of_grade();
    assert_eq!(studio.mode, EditorMode::Build);
    assert_eq!(studio.status, "Grading a Water scene arrives with its light and looks.");
    // Adding the open scene to a video is refused, and a new video from it starts empty.
    let scene = studio.scene.clone();
    assert!(crate::video_mode::scenes::keep(&mut studio, scene).is_none());
    assert_eq!(studio.status, "Water scenes can't go in a video yet.");
    assert!(studio.video.video.scenes.is_empty());
    studio.new_video();
    assert!(studio.video.video.scenes.is_empty() && studio.video.video.clips.is_empty());
    assert_eq!(studio.status, "Started a new video. Water scenes can't go in a video yet.");
    // A Balls scene is as it was.
    let mut balls = Studio::headless();
    balls.mode = EditorMode::Grade;
    balls.keep_out_of_grade();
    assert_eq!(balls.mode, EditorMode::Grade);
    balls.new_video();
    assert_eq!(balls.video.video.scenes.len(), 1);
}

#[test]
fn a_water_scenes_toolbar_says_so_and_greys_export_and_grade() {
    let size = egui::vec2(1500.0, 80.0);
    let mut h = harness(Studio::headless_water(), size, |ui, s| s.toolbar(ui));
    h.run();
    assert!(has(&h, "Water scene"));
    assert!(disabled(&h, "Export video…") && disabled(&h, "Grade"));
    assert!(!disabled(&h, "Build") && !disabled(&h, "Video"));
    // In Video mode the video can still be exported.
    h.state_mut().mode = crate::grade_mode::EditorMode::Video;
    h.run();
    assert!(!disabled(&h, "Export video…"));
    let mut h = harness(Studio::headless(), size, |ui, s| s.toolbar(ui));
    h.run();
    assert!(!has(&h, "Water scene"));
    assert!(!disabled(&h, "Export video…") && !disabled(&h, "Grade"));
}

#[test]
fn a_water_scene_has_no_lights_tab_no_mixer_tab_and_no_rules_view() {
    let side = |ui: &mut egui::Ui, s: &mut Studio| {
        let before = s.scene.clone();
        s.left_panel(ui, &before);
        egui::CentralPanel::default().show(ui, |_| {});
    };
    let mut water = Studio::headless_water();
    // The last Balls scene was left on the Mixer tab and the Rules view: both are kept for it.
    water.left = LeftPanel::Mixer;
    water.bottom = settings::BottomView::Rules;
    assert_eq!(water.bottom_view(), settings::BottomView::Timeline);
    let mut h = harness(water, egui::vec2(900.0, 500.0), side);
    h.run();
    assert!(!has(&h, "Lights") && !has(&h, "Mixer") && !has(&h, "Things"));
    assert!(has(&h, "Course"), "the tree shows, whichever tab was last open");
    assert_eq!(h.state().left, LeftPanel::Mixer);
    let mut h = harness(Studio::headless_water(), egui::vec2(900.0, 80.0), |ui, s| crate::transport::controls(ui, s));
    h.run();
    assert!(!has(&h, "Rules") && !has(&h, "Timeline"));

    let mut h = harness(Studio::headless(), egui::vec2(900.0, 500.0), side);
    h.run();
    assert!(has(&h, "Lights") && has(&h, "Mixer") && has(&h, "Things"));
    let mut h = harness(Studio::headless(), egui::vec2(900.0, 80.0), |ui, s| crate::transport::controls(ui, s));
    h.run();
    assert!(has(&h, "Rules") && has(&h, "Timeline"));
}

#[test]
fn pasting_ball_things_into_a_water_scene_says_why_not() {
    let mut balls = Studio::headless();
    let ball = balls.scene.order.iter().copied().find(|id| balls.scene.things[id].kind == "balls").unwrap();
    assert!(balls.copy_things(&[ball]));
    let mut water = Studio::headless_water();
    water.clipboard = balls.clipboard.clone();
    let before = water.scene.clone();
    assert!(!water.paste_things(None));
    assert_eq!(water.status, "Nothing to paste: those things don't go in a Water scene.");
    assert_eq!(water.scene, before);
    // And a Water thing into a Balls scene.
    let kit = water.kit.clone();
    let pour = bpe_kit::add_thing(&mut water.scene, &kit.registry, POUR, ThingId::ROOT).unwrap();
    assert!(water.copy_things(&[pour]));
    balls.clipboard = water.clipboard.clone();
    assert!(!balls.paste_things(None));
    assert_eq!(balls.status, "Nothing to paste: those things don't go in a Balls scene.");
    // Within a Water scene it pastes as usual.
    assert!(water.paste_things(None));
    assert_eq!(water.scene.things.values().filter(|t| t.kind == POUR).count(), 2);
}

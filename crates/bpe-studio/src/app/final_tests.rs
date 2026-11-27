//! A Water scene's final in the editor (Water mode part 8): the Draft | Final switch, Make final
//! and the Final lane, the status line's words, the Export window's buttons, slow motion on the
//! playhead and the Finals menu.

use super::*;
use crate::export_window::Subject;
use crate::water_mode::final_job::{FinalJob, WaterView};
use bpe_core::{SlotId, Stroke, Strokes, ThingId, Value};
use bpe_kit::things::water::finals::Finals;
use bpe_kit::things::water::make::MakeProgress;
use bpe_kit::things::water::{LIQUID, POUR, course_of};
use std::sync::atomic::AtomicBool;

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

fn set(studio: &mut Studio, id: ThingId, slot: SlotId, key: &str, v: Value) {
    studio.scene.things.get_mut(&id).unwrap().slots.get_mut(&slot).unwrap().params.set(key, v);
}

/// A Water scene a quarter second long at Coarse final detail: a box carved in the rock with a
/// fill at its foot, its finals in a folder of the test's own.
fn small(name: &str) -> Studio {
    let mut studio = Studio::headless_water();
    let dir = std::env::temp_dir().join(format!("bpe-studio-final-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    studio.water_final.finals = Finals::at(dir);
    let before = studio.scene.clone();
    let kit = studio.kit.clone();
    let reg = &kit.registry;
    let root = &mut studio.scene.things.get_mut(&ThingId::ROOT).unwrap().params;
    root.set("duration", Value::Number(0.25));
    set(&mut studio, ThingId::ROOT, SlotId::Water, "final_detail", Value::Choice("coarse".into()));
    let course = course_of(&studio.scene).unwrap();
    let hole = Stroke { points: vec![[0.0, 0.0]], size: 200.0, strength: 1.0, square: true, ..Default::default() };
    set(&mut studio, course, SlotId::Form, "carved", Value::Strokes(Strokes { list: vec![hole] }));
    bpe_kit::add_thing(&mut studio.scene, reg, LIQUID, ThingId::ROOT).unwrap();
    let fill = bpe_kit::add_thing(&mut studio.scene, reg, POUR, ThingId::ROOT).unwrap();
    studio.scene.things.get_mut(&fill).unwrap().transform.position = glam::Vec2::new(0.0, 120.0);
    set(&mut studio, fill, SlotId::Main, "kind", Value::Choice("fill_at_the_start".into()));
    set(&mut studio, fill, SlotId::Main, "fill_size", Value::Vec2([160.0, 160.0]));
    studio.after_edit(before);
    find(&mut studio);
    studio
}

/// Finds the scene's final now.
fn find(studio: &mut Studio) {
    studio.water_final.stale = true;
    let draft = studio.water_draft.setup.clone();
    studio.water_final.find(studio.revision, &studio.scene, &studio.resolved, draft.as_deref());
}

/// Makes the scene's final on the test's graphics device.
fn make(studio: &mut Studio) {
    let (device, queue) = bpe_gfx::test_device().expect("this test makes a final on the graphics card");
    let gpu = bpe_water::WaterGpu::new(&device, &queue);
    let make = studio.water_final.found.as_ref().unwrap().make.clone();
    make.make(&gpu, &studio.water_final.finals, &MakeProgress::default(), &AtomicBool::new(false)).unwrap();
    find(studio);
}

/// The Final lane's name for screen readers and tests.
fn lane(h: &Harness<Studio>) -> String {
    let node = h.query_all_by_label_contains("Final lane").next().expect("the Final lane");
    node.accesskit_node().label().unwrap_or_default()
}

#[test]
fn the_toolbar_switches_the_view_and_export_is_on() {
    let size = egui::vec2(1700.0, 80.0);
    let mut h = harness(small("switch"), size, |ui, s| s.toolbar(ui));
    h.run();
    assert!(!disabled(&h, "Export video…"), "a Water scene exports now");
    assert_eq!(h.state().water_final.view, WaterView::Draft);
    h.get_by_label("Final").click();
    h.run();
    assert_eq!(h.state().water_final.view, WaterView::Final);
    h.get_by_label("Draft").click();
    h.run();
    assert_eq!(h.state().water_final.view, WaterView::Draft);
    assert!(!disabled(&h, "Make final"));
    assert!(has(&h, "Finals"));
}

#[test]
fn make_final_starts_a_make_and_the_lane_shows_it() {
    let size = egui::vec2(1200.0, 300.0);
    let mut h = harness(small("lane"), size, crate::timeline::show);
    h.run();
    assert_eq!(lane(&h), "Final lane, not made yet");
    let print = h.state().water_final.found.as_ref().unwrap().make.print();
    let (job, _ended) = FinalJob::pretend(print, 0, 6, 15);
    h.state_mut().water_final.job = Some(job);
    h.run();
    assert_eq!(lane(&h), "Final lane, being made, at 0:00.10");
    let words = h.state().water_final.words().unwrap().0;
    assert_eq!(words, "· Making the final: 40 %");
    // A make of the scene before an edit is not this scene's.
    h.state_mut().water_final.job = Some(FinalJob::pretend(print ^ 1, 0, 6, 15).0);
    h.run();
    assert_eq!(lane(&h), "Final lane, the scene as it was being made");
    let words = h.state().water_final.words().unwrap().0;
    assert_eq!(words, "· Making the final of the scene as it was: 40 %");
    // (The toolbar offers to stop it instead.)
    let mut t = harness(Studio::headless_water(), egui::vec2(1700.0, 80.0), |ui, s| s.toolbar(ui));
    t.state_mut().water_final.job = Some(FinalJob::pretend(0, 0, 1, 2).0);
    t.run();
    assert!(has(&t, "Stop making") && !has(&t, "Make final"));
}

#[test]
fn before_any_make_the_status_line_estimates_it() {
    let studio = small("estimate");
    let (words, problem) = studio.water_final.words().unwrap();
    assert!(!problem);
    assert!(words.starts_with("· Final: about ") && words.ends_with("(estimate)"), "{words}");
    let found = studio.water_final.found.as_ref().unwrap();
    assert!(found.drops_about > 100, "a fill of 160 px at 3 px: {}", found.drops_about);
}

#[test]
fn a_made_final_shows_on_the_lane_and_an_edit_puts_it_out_of_date_from_its_change() {
    let mut studio = small("made");
    make(&mut studio);
    let words = studio.water_final.words().unwrap().0;
    assert!(words.starts_with("· Final: ") && words.contains("0.25 s of 0.25 made"), "{words}");
    assert!(studio.water_final.found.as_ref().unwrap().status.complete());
    let size = egui::vec2(1200.0, 300.0);
    let mut h = harness(studio, size, crate::timeline::show);
    h.run();
    assert_eq!(lane(&h), "Final lane, made to 0:00.25");
    // A look edit changes nothing the final holds.
    let s = h.state_mut();
    let before = s.scene.clone();
    let liquid = s.scene.things.iter().find(|(_, t)| t.kind == LIQUID).map(|(id, _)| *id).unwrap();
    set(s, liquid, SlotId::Look, "colour", Value::Color(bpe_core::Color::rgb(200, 10, 10)));
    s.after_edit(before);
    find(s);
    assert!(s.water_final.found.as_ref().unwrap().status.complete(), "a colour is no physics");
    // A take right up to three frames, of the scene as it was: the rest is hatched.
    let status = &mut s.water_final.found.as_mut().unwrap().status;
    (status.own, status.good) = (false, 3);
    h.run();
    assert_eq!(lane(&h), "Final lane, made to 0:00.05, out of date from 0:00.05");
    // A tap added: the take was right up to three frames before it starts, which is before its
    // first restart point, so it is no help and the final is made again from the start.
    let s = h.state_mut();
    let before = s.scene.clone();
    let kit = s.kit.clone();
    let tap = bpe_kit::add_thing(&mut s.scene, &kit.registry, POUR, ThingId::ROOT).unwrap();
    s.scene.things.get_mut(&tap).unwrap().transform.position = glam::Vec2::new(-60.0, -150.0);
    set(s, tap, SlotId::Main, "starts", Value::Number(0.1));
    s.after_edit(before);
    find(s);
    let status = &s.water_final.found.as_ref().unwrap().status;
    assert!(status.take.is_none() && status.good == 0, "{status:?}");
}

#[test]
fn a_stopped_make_says_so_and_exports_nothing() {
    let mut studio = small("stopped");
    let (device, queue) = bpe_gfx::test_device().expect("this test makes a final on the graphics card");
    let gpu = bpe_water::WaterGpu::new(&device, &queue);
    let make = studio.water_final.found.as_ref().unwrap().make.clone();
    // (A make that ended with what it made, as a stopped one does.)
    let head = make.make(&gpu, &studio.water_final.finals, &MakeProgress::default(), &AtomicBool::new(false)).unwrap();
    let (job, ended) = FinalJob::pretend(make.print(), 0, 3, 15);
    job.stop();
    ended.send(Ok(head)).unwrap();
    studio.water_final.job = Some(job);
    studio.water_final.export_after = true;
    studio.follow_water_final(&egui::Context::default());
    assert!(studio.water_final.job.is_none());
    assert_eq!(studio.status, "Stopped making the final. What was made is kept.");
    assert!(!studio.panels.export.export_final, "a stopped make exports nothing");
}

#[test]
fn the_export_window_offers_make_final_and_export_until_the_final_is_made() {
    let draw = |ui: &mut egui::Ui, s: &mut Studio| {
        let ctx = ui.ctx().clone();
        s.follow_water_final(&ctx);
        s.panels.export.open = true;
        let (settings, kit) = (s.settings(), s.kit.clone());
        let subject = Subject::Scene { scene: &s.scene, settings: &settings };
        s.panels.export.show_subject(&ctx, &subject, None, &kit);
    };
    let mut h = harness(small("export"), egui::vec2(800.0, 900.0), draw);
    h.run();
    if !has(&h, "Make final and export") {
        // (No ffmpeg here: the buttons are there but greyed.)
        panic!("no Make final and export");
    }
    assert!(has(&h, "Export the draft") && !has(&h, "Export"));
    assert!(has(&h, "0.25 s at 60 fps · 15 frames"));
    h.get_by_label("Make final and export").click();
    h.run();
    assert!(h.state().water_final.job.is_some() && h.state().water_final.export_after, "the make for the export");
    h.state_mut().water_final.stop_make();
    h.state_mut().water_final.job = None;
    make(h.state_mut());
    h.run();
    assert!(has(&h, "Export") && !has(&h, "Make final and export"));
}

#[test]
fn slow_motion_moves_the_playhead_at_its_speed() {
    let mut studio = small("slow");
    let before = studio.scene.clone();
    set(&mut studio, ThingId::ROOT, SlotId::Water, "speed", Value::Number(0.25));
    studio.after_edit(before);
    find(&mut studio);
    assert_eq!(studio.water_speed(0.1), 0.25);
    let found = studio.water_final.found.as_ref().unwrap();
    assert!((found.video_seconds - 1.0).abs() < 1e-9, "a quarter second of water plays for a second");
    assert_eq!(found.slow, vec![(0, 15)], "every frame keeps more moments");
    studio.set_playing(true);
    studio.last_tick = Some(Instant::now() - std::time::Duration::from_millis(200));
    studio.tick();
    assert!((studio.time - 0.05).abs() < 0.01, "{}", studio.time);
}

#[test]
fn the_finals_menu_clears_this_scenes_final() {
    let mut studio = small("clear");
    make(&mut studio);
    let take = studio.water_final.finals.take_dir(studio.water_final.found.as_ref().unwrap().make.print());
    assert!(take.exists());
    let mut h = harness(studio, egui::vec2(1700.0, 300.0), |ui, s| {
        let ctx = ui.ctx().clone();
        s.toolbar(ui);
        s.follow_water_final(&ctx);
    });
    h.run();
    assert!(disabled(&h, "Make final"), "the final is made");
    h.get_by_label("Finals").click();
    h.run();
    assert!(h.query_all_by_label_contains("Clear every final (").next().is_some());
    h.get_by_label("Clear this scene's final").click();
    h.run();
    assert!(!take.exists());
    assert_eq!(h.state().status, "Cleared this scene's final.");
    assert!(h.state().water_final.found.as_ref().unwrap().status.take.is_none());
}

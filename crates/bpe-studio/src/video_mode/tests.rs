//! Video mode's checks (part 2 plan, Tasks 2 to 6): the session and its files, arranging by
//! dragging in the lanes, the inspectors, keys, the question before losing work, autosave and
//! recovery, the Export window and the still frame.

use super::*;
use crate::grade_mode::EditorMode;
use crate::style;
use bpe_core::video::{Snap, TransitionKind};
use bpe_core::{ThingId, Value};
use eframe::egui::{Event, Modifiers, PointerButton, Pos2};
use egui_kittest::{Harness, kittest::Queryable};
use std::path::{Path, PathBuf};
use std::time::Instant;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ball-studio-video-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A studio whose Build scene is 4 s long, and a video (120 BPM, 4/4: a bar is 2 s) playing it as
/// clip A (0 to 4 s) and clip B (4 to 8 s), in Video mode, with nothing to undo yet.
fn studio() -> (Studio, ClipId, ClipId) {
    let mut studio = Studio::headless();
    studio.scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("duration", Value::Number(4.0));
    studio.scene.name = "Ring Drop".into();
    studio.saved = studio.scene.clone();
    let reg = &studio.kit.registry;
    let mut video = Video::new();
    let kept = arrange::keep_scene(&mut video, studio.scene.clone(), "Ring Drop");
    let a = arrange::add_clip(&mut video, reg, kept, 0.0).unwrap();
    let b = arrange::add_at_end(&mut video, reg, kept).unwrap();
    studio.replace_video(video, None);
    studio.mode = EditorMode::Video;
    (studio, a, b)
}

fn harness(studio: Studio) -> Harness<'static, Studio> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(60)
        .allow_missing_glyphs()
        .with_context_setup(style::apply)
        .build_ui_state(|ui, studio: &mut Studio| studio.video_frame_for_tests(ui), studio);
    h.run();
    h
}

/// Where video time `t` is in the Scenes lane.
fn at(h: &Harness<'_, Studio>, t: f64) -> Pos2 {
    let session = &h.state().video;
    let lane = session.view.lane.expect("the lane was drawn");
    Pos2::new(session.view.x_of(lane.left(), t), lane.center().y)
}

fn press(h: &mut Harness<'_, Studio>, pos: Pos2, pressed: bool, modifiers: Modifiers) {
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers });
}

/// Drags in the lane from video time `from` to `to`.
fn drag(h: &mut Harness<'_, Studio>, from: f64, to: f64, modifiers: Modifiers) {
    let (a, b) = (at(h, from), at(h, to));
    h.input_mut().events.push(Event::ModifiersChanged(modifiers));
    h.input_mut().events.push(Event::PointerMoved(a));
    h.step();
    press(h, a, true, modifiers);
    h.step();
    for i in 1..=8 {
        let p = a + (b - a) * (i as f32 / 8.0);
        h.input_mut().events.push(Event::PointerMoved(p));
        h.step();
    }
    press(h, b, false, modifiers);
    h.step();
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
    h.run();
}

fn click(h: &mut Harness<'_, Studio>, t: f64) {
    let p = at(h, t);
    h.input_mut().events.push(Event::PointerMoved(p));
    h.step();
    press(h, p, true, Modifiers::NONE);
    h.step();
    press(h, p, false, Modifiers::NONE);
    h.run();
}

fn clip_of(studio: &Studio, id: ClipId) -> Clip {
    arrange::clip(&studio.video.video, id).unwrap().clone()
}

#[test]
fn a_fresh_video_is_not_unsaved() {
    let studio = Studio::headless();
    assert!(!studio.video_unsaved());
    assert_eq!(studio.video.name(), "Untitled video");
}

#[test]
fn new_video_keeps_the_open_scene_as_its_first_clip() {
    let mut studio = Studio::headless();
    studio.scene.name = "Ring Drop".into();
    studio.new_video();
    assert_eq!(studio.mode, EditorMode::Video);
    assert_eq!(studio.video.video.clips.len(), 1);
    assert_eq!(studio.video.video.scenes.values().next().unwrap().name, "Ring Drop");
    assert!(studio.video_unsaved(), "a new video with a clip is not saved yet");
    assert!(studio.window_title().starts_with("Untitled video •"));
}

#[test]
fn save_then_open_gives_the_same_video() {
    let dir = temp_dir("save-open");
    let (mut studio, _, _) = studio();
    let path = dir.join("Drop Tape.ballvideo");
    assert!(studio.save_video_to(path.clone()));
    assert!(!studio.video_unsaved());
    assert_eq!(studio.video.name(), "Drop Tape");
    let saved = studio.video.video.clone();
    let mut other = Studio::headless();
    assert!(other.open_video_path(&path));
    assert_eq!(other.mode, EditorMode::Video);
    assert_eq!(other.video.video, saved);
    assert!(!other.video_unsaved());
    // Saving again keeps the version before.
    assert!(studio.save_video_to(path.clone()));
    assert!(dir.join("Drop Tape (backup).ballvideo").exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_broken_video_file_is_a_file_problem() {
    let dir = temp_dir("broken");
    let path = dir.join("Broken.ballvideo");
    std::fs::write(&path, "not a video").unwrap();
    let mut studio = Studio::headless();
    assert!(!studio.open_video_path(&path));
    let problem = studio.problem.as_ref().expect("said why");
    assert_eq!(problem.kind, crate::app::ProblemKind::File);
    assert!(problem.text.contains("Broken.ballvideo"), "{}", problem.text);
    assert_eq!(studio.mode, EditorMode::Build, "the video stays as it was");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn undo_and_redo_walk_the_video_steps() {
    let (mut studio, a, _) = studio();
    let kit = studio.kit.clone();
    studio.video.change(false, |v| arrange::move_clip(v, &kit.registry, a, 10.0));
    studio.video.change(false, |v| v.settings.tempo = 90.0);
    assert!(studio.video_undo());
    assert_eq!(studio.video.video.settings.tempo, 120.0);
    assert!(studio.video_undo());
    assert_eq!(clip_of(&studio, a).start, 0.0);
    assert!(!studio.video_undo());
    assert!(studio.video_redo());
    assert_ne!(clip_of(&studio, a).start, 0.0);
}

#[test]
fn the_toolbar_switches_to_video_mode() {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1500.0, 80.0))
        .allow_missing_glyphs()
        .with_context_setup(style::apply)
        .build_ui_state(|ui, s: &mut Studio| s.toolbar(ui), Studio::headless());
    h.run();
    h.get_by_label("Video").click();
    h.run();
    assert_eq!(h.state().mode, EditorMode::Video);
    assert!(h.query_by_label("Save this video (Ctrl+S)").is_none() || h.query_by_label("Untitled video").is_some());
}

#[test]
fn dragging_a_clip_moves_it_by_whole_bars_and_is_one_undo_step() {
    let (studio, _, b) = studio();
    let mut h = harness(studio);
    drag(&mut h, 5.0, 7.3, Modifiers::NONE);
    assert_eq!(clip_of(h.state(), b).start, 6.0, "4 + 2.3 lands on the bar at 6 s");
    assert!(h.state().video.history.can_undo());
    assert!(h.state_mut().video_undo());
    assert_eq!(clip_of(h.state(), b).start, 4.0);
    assert!(!h.state().video.history.can_undo(), "the whole drag was one step");
}

#[test]
fn alt_drags_free() {
    let (studio, _, b) = studio();
    let mut h = harness(studio);
    let (from, to) = (at(&h, 5.0), at(&h, 5.37));
    let lane = h.state().video.view.lane.unwrap();
    let view = h.state().video.view;
    let moved = view.time_of(lane.left(), to.x) - view.time_of(lane.left(), from.x);
    drag(&mut h, 5.0, 5.37, Modifiers::ALT);
    let start = clip_of(h.state(), b).start;
    let frame = 1.0 / 60.0;
    assert!((start - (4.0 + moved)).abs() <= frame + 1e-9, "free: {start} vs {}", 4.0 + moved);
    assert!((start * 60.0 - (start * 60.0).round()).abs() < 1e-6, "still a whole frame");
}

#[test]
fn dragging_the_edges_trims_and_the_left_edge_keeps_the_end_picture() {
    let (mut studio, _, b) = studio();
    studio.video.video.settings.snap = Snap::Beat;
    let mut h = harness(studio);
    drag(&mut h, 8.0, 6.6, Modifiers::NONE);
    assert_eq!(clip_of(h.state(), b).end(), 6.5, "the right edge lands on the beat at 6.5 s");
    drag(&mut h, 4.0, 5.1, Modifiers::NONE);
    let c = clip_of(h.state(), b);
    assert_eq!((c.start, c.offset, c.end()), (5.0, 1.0, 6.5), "start and offset move together");
}

#[test]
fn overlapping_makes_a_transition_and_clicking_it_picks_it() {
    let (mut studio, _, b) = studio();
    studio.video.video.settings.snap = Snap::Beat;
    let mut h = harness(studio);
    drag(&mut h, 6.0, 5.0, Modifiers::NONE);
    assert_eq!(clip_of(h.state(), b).start, 3.0);
    assert_eq!(h.state().video.video.overlap(1), 1.0);
    assert_eq!(clip_of(h.state(), b).transition_in.kind, TransitionKind::Crossfade);
    click(&mut h, 3.5);
    assert_eq!(h.state().video.pick(), VideoPick::Transition(b));
    h.get_by_label("Zoom through").click();
    h.run();
    assert_eq!(clip_of(h.state(), b).transition_in.kind, TransitionKind::ZoomThrough);
    // An empty part of the lane picks the video.
    click(&mut h, 8.2);
    assert_eq!(h.state().video.pick(), VideoPick::Video);
}

#[test]
fn the_video_inspector_changes_the_frame_rate_as_one_step() {
    let (studio, _, _) = studio();
    let mut h = harness(studio);
    h.get_by_label("30").click();
    h.run();
    assert_eq!(h.state().video.video.settings.fps, 30);
    h.query_all_by_label("Off").next().unwrap().click();
    h.run();
    assert_eq!(h.state().video.video.settings.snap, Snap::Off);
    assert!(h.state_mut().video_undo());
    assert!(h.state_mut().video_undo());
    assert_eq!(h.state().video.video.settings.fps, 60);
}

#[test]
fn delete_copy_paste_duplicate_a_clip() {
    let (studio, _, b) = studio();
    let mut h = harness(studio);
    click(&mut h, 6.0);
    assert_eq!(h.state().video.pick(), VideoPick::Clip(b));
    h.input_mut().events.push(Event::Copy);
    h.run();
    assert!(h.state().video.clip_board.is_some());
    h.state_mut().video.seek(10.0);
    h.input_mut().events.push(Event::Paste(String::new()));
    h.run();
    assert_eq!(h.state().video.video.clips.len(), 3);
    assert_eq!(h.state().video.video.clips[2].start, 10.0);
    h.input_mut().events.push(Event::Key {
        key: egui::Key::D,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::COMMAND,
    });
    h.run();
    assert_eq!(h.state().video.video.clips.len(), 4);
    h.input_mut().events.push(Event::Key {
        key: egui::Key::Delete,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
    assert_eq!(h.state().video.video.clips.len(), 3);
}

#[test]
fn ctrl_z_in_video_mode_undoes_the_video_not_the_scene() {
    let (mut studio, a, _) = studio();
    let before = studio.scene.clone();
    studio.scene.name = "Edited".into();
    studio.after_edit(before);
    assert!(studio.history.can_undo());
    let kit = studio.kit.clone();
    studio.video.change(false, |v| arrange::move_clip(v, &kit.registry, a, 12.0));
    let mut h = harness(studio);
    h.input_mut().events.push(Event::Key {
        key: egui::Key::Z,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::COMMAND,
    });
    h.run();
    assert_eq!(clip_of(h.state(), a).start, 0.0, "the video's step is undone");
    assert_eq!(h.state().scene.name, "Edited", "the scene is left alone");
    assert!(h.state().history.can_undo());
}

#[test]
fn closing_asks_about_the_scene_then_the_video() {
    for cancel_second in [false, true] {
        let (mut studio, a, _) = studio();
        studio.scene.name = "Changed".into();
        let kit = studio.kit.clone();
        studio.video.change(false, |v| arrange::move_clip(v, &kit.registry, a, 12.0));
        let mut h = harness(studio);
        let ctx = h.ctx.clone();
        h.state_mut().guard(crate::app::Pending::Close, &ctx);
        h.run();
        assert!(h.query_by_label("Save changes to \"Changed\"?").is_some(), "the scene first");
        h.get_by_label("Don't save").click();
        h.run();
        assert!(h.query_by_label("Save changes to \"Untitled video\"?").is_some(), "then the video");
        assert!(!h.state().closing);
        if cancel_second {
            h.get_by_label("Cancel").click();
            h.run();
            assert!(h.state().confirm.is_none() && !h.state().closing, "Cancel stops the close");
            assert!(!h.state().close_scene_answered);
        } else {
            h.get_by_label("Don't save").click();
            h.run();
            assert!(h.state().closing);
        }
    }
}

#[test]
fn a_video_file_opens_in_video_mode_from_open_and_from_a_drop() {
    let dir = temp_dir("drop");
    let (mut studio, _, _) = studio();
    let path = dir.join("Tape.ballvideo");
    assert!(studio.save_video_to(path.clone()));
    let mut other = Studio::headless();
    let ctx = egui::Context::default();
    other.ask_open_path(path.clone(), &ctx);
    assert_eq!(other.mode, EditorMode::Video);
    assert_eq!(other.video.name(), "Tape");
    assert_eq!(crate::drops::what("Tape.ballvideo"), crate::drops::Dropped::Video);
    // With unsaved video work, opening asks first.
    let kit = other.kit.clone();
    other.video.change(false, |v| v.settings.tempo = 100.0);
    other.ask_open_path(path, &ctx);
    assert!(matches!(other.confirm, Some(crate::app::Pending::OpenVideoPath(_))));
    assert!(other.asking_about_video());
    drop(kit);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_unsaved_video_is_autosaved_and_offered_back() {
    let dir = temp_dir("recover").join("recovery");
    {
        let (mut studio, a, _) = studio();
        studio.start_recovery(&dir, None);
        let kit = studio.kit.clone();
        studio.video.change(false, |v| arrange::move_clip(v, &kit.registry, a, 12.0));
        studio.autosave(Instant::now());
        let files = studio.recovery.as_ref().unwrap().files().clone();
        studio.recovery.as_ref().unwrap().flush();
        assert!(files.video.exists(), "the video's copy is written");
        assert!(!files.scene.exists(), "the scene is saved, so it has no copy");
        // The editor goes away without a clean exit.
        drop(studio);
    }
    let mut next = Studio::headless();
    next.start_recovery(&dir, None);
    let offer = next.offer.as_ref().expect("the video is offered back");
    assert!(offer.video.is_some());
    next.restore();
    assert_eq!(next.mode, EditorMode::Video);
    assert_eq!(next.video.video.clips.len(), 2);
    assert!(next.video_unsaved(), "restored work is still unsaved");
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
}

#[test]
fn the_export_window_exports_the_video_without_stems_or_midi() {
    let (studio, _, _) = studio();
    let video = studio.video.video.clone();
    let kit = studio.kit.clone();
    let mut window = crate::export_window::ExportWindow::default();
    window.open = true;
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 900.0))
        .allow_missing_glyphs()
        .with_context_setup(style::apply)
        .build_ui(move |ui| {
            let subject = crate::export_window::Subject::Video { video: &video, name: "Drop Tape".into() };
            window.show_subject(ui.ctx(), &subject, Some(Path::new("/videos")), &kit);
        });
    h.run();
    assert!(h.query_by_label("Drop Tape.mp4").is_some(), "named after the video");
    assert!(h.query_by_label("Also save stems").is_none());
    assert!(h.query_by_label("Also save MIDI").is_none());
    assert!(h.query_by_label("Stems, MIDI and baked lighting aren't made for a video yet.").is_some());
    assert!(h.query_by_label("8 s at 60 fps · 480 frames").is_some());
}

#[test]
fn the_still_frame_comes_back() {
    let (mut studio, _, _) = studio();
    studio.video.still.enabled = true;
    let ctx = egui::Context::default();
    let started = Instant::now();
    let kit = studio.kit.clone();
    let session = &mut studio.video;
    session.seek(1.0);
    while !session.still.has_frame() && started.elapsed() < std::time::Duration::from_secs(90) {
        let mut out = ctx.run_ui(Default::default(), |_| {
            let frame = session.frame();
            session.still.ask(&session.video, session.revision, frame, &kit, &ctx);
            session.still.take(&ctx);
        });
        out.textures_delta.clear();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    if !session.still.has_frame() {
        // Without a graphics card the still frame can't be drawn; that passes only when skipping was asked for.
        assert!(bpe_gfx::test_device().is_err(), "no still frame came back with a graphics device");
    }
}

#[test]
fn keeping_the_build_scene_lists_it_and_add_to_the_end_plays_it_last() {
    let (mut studio, _, b) = studio();
    let mut scene = studio.scene.clone();
    scene.name = "Second".into();
    let kept = scenes::keep(&mut studio, scene);
    assert_eq!(studio.video.pick(), VideoPick::Scene(kept));
    assert_eq!(studio.video.video.scenes[&kept].name, "Second");
    let mut h = harness(studio);
    assert!(h.query_all_by_label("Scene Second").next().is_some(), "the kept scene is listed");
    h.get_by_label("Add to the end").click();
    h.run();
    let VideoPick::Clip(c) = h.state().video.pick() else { panic!("the new clip is picked") };
    let (last, before) = (clip_of(h.state(), c), clip_of(h.state(), b));
    assert_eq!(last.scene, kept);
    assert_eq!(last.start, before.start + before.length);
    assert!(h.state_mut().video_undo());
    assert_eq!(h.state().video.video.clips.len(), 2);
}

#[test]
fn a_new_frame_rate_keeps_every_clip_on_whole_frames() {
    let (mut studio, a, _) = studio();
    studio.video.change(false, |v| v.clips[0].start = 0.01);
    let mut h = harness(studio);
    h.get_by_label("24").click();
    h.run();
    assert_eq!(h.state().video.video.settings.fps, 24);
    let start = clip_of(h.state(), a).start;
    assert!((start * 24.0 - (start * 24.0).round()).abs() < 1e-9, "start {start} is on a frame");
}

// ---------- part 3: playing, the Scene sound lane, Edit scene ----------

use std::sync::Arc;
use std::time::Duration;

fn key(h: &mut Harness<'_, Studio>, key: egui::Key) {
    h.input_mut().events.push(Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
}

/// Ticks the playing `n` times, a quarter second apart from `t0` (no speakers: the wall clock).
fn ticks(studio: &mut Studio, t0: Instant, from: u32, n: u32) {
    for i in from..from + n {
        studio.video_tick(t0 + Duration::from_millis(250 * u64::from(i)));
    }
}

#[test]
fn space_plays_and_stops_the_video() {
    let (studio, _, _) = studio();
    let mut h = harness(studio);
    key(&mut h, egui::Key::Space);
    assert!(h.state().video.playing.on, "Space plays");
    let studio = h.state_mut();
    let t0 = Instant::now();
    ticks(studio, t0, 0, 3);
    assert!(!studio.video.playing.starting, "without a picture or speakers it starts at once");
    assert!((studio.video.time - 0.5).abs() < 0.05, "the playhead follows the clock: {}", studio.video.time);
    key(&mut h, egui::Key::Space);
    let session = &h.state().video;
    assert!(!session.playing.on, "Space again stops");
    let frames = session.time * f64::from(session.video.settings.fps);
    assert!((frames - frames.round()).abs() < 1e-6, "stopped on a whole frame");
    assert!(h.query_by_label("Play").is_some(), "the header's button says Play again");
}

#[test]
fn playing_stops_at_the_end_or_loops() {
    let (mut studio, _, _) = studio();
    studio.video_play();
    let t0 = Instant::now();
    ticks(&mut studio, t0, 0, 40);
    assert!(!studio.video.playing.on, "it stops at the end");
    assert_eq!(studio.video.time, studio.video.video.end());
    studio.video.playing.looping = true;
    studio.video_play();
    assert_eq!(studio.video.time, 0.0, "Play at the end starts from the start");
    ticks(&mut studio, t0, 100, 34);
    assert!(studio.video.playing.on, "with Loop it plays on");
    assert!(studio.video.time < 2.0, "from the start again: {}", studio.video.time);
}

#[test]
fn a_jump_while_playing_starts_again_there() {
    let (mut studio, _, _) = studio();
    studio.video_play();
    let t0 = Instant::now();
    ticks(&mut studio, t0, 0, 3);
    studio.video.seek(5.0);
    ticks(&mut studio, t0, 3, 1);
    assert!(studio.video.playing.on);
    assert!((studio.video.time - 5.0).abs() < 1e-9, "it went to the jump: {}", studio.video.time);
    ticks(&mut studio, t0, 4, 2);
    assert!((studio.video.time - 5.5).abs() < 1e-6, "and plays on from there: {}", studio.video.time);
}

#[test]
fn switching_to_build_stops_the_video() {
    let (mut studio, _, _) = studio();
    studio.seen_mode = EditorMode::Video;
    studio.video_play();
    studio.mode = EditorMode::Build;
    studio.follow_mode();
    assert!(!studio.video.playing.on);
}

#[test]
fn a_take_is_made_once_the_video_rests() {
    let (mut studio, a, _) = studio();
    let t0 = Instant::now();
    studio.video_tick(t0);
    assert!(studio.video.playing.take.is_none(), "not while the video may still be changing");
    studio.video_tick(t0 + Duration::from_millis(500));
    let first = studio.video.playing.take.clone().expect("a take once it rested");
    studio.video.change(false, |v| v.clips.iter_mut().find(|c| c.id == a).unwrap().level_db = -6.0);
    studio.video_tick(t0 + Duration::from_millis(600));
    assert!(Arc::ptr_eq(&first, studio.video.playing.take.as_ref().unwrap()), "not straight after an edit");
    studio.video_tick(t0 + Duration::from_millis(1_100));
    assert!(!Arc::ptr_eq(&first, studio.video.playing.take.as_ref().unwrap()), "a new take for the edit");
}

#[test]
fn the_picture_plays_on_the_card() {
    let (mut studio, _, _) = studio();
    studio.video.still.enabled = true;
    let ctx = egui::Context::default();
    studio.repaint = Some(ctx.clone());
    studio.video_play();
    let started = Instant::now();
    while (studio.video.playing.starting || studio.video.playing.shown.is_none_or(|f| f < 10))
        && started.elapsed() < Duration::from_secs(120)
    {
        let mut out = ctx.run_ui(Default::default(), |_| studio.video_tick(Instant::now()));
        out.textures_delta.clear();
        std::thread::sleep(Duration::from_millis(10));
    }
    if studio.video.playing.shown.is_none() {
        assert!(bpe_gfx::test_device().is_err(), "no frame was played with a graphics device");
        return;
    }
    let shown = studio.video.playing.shown.unwrap();
    assert!(!studio.video.playing.starting, "it got ready");
    assert!(shown <= studio.video.frame(), "the frame on show is never ahead of the playhead");
    studio.video_stop();
    assert_eq!(studio.video.frame(), shown, "stopping keeps the frame that was on show");
}

#[test]
fn hits_land_where_the_clip_plays() {
    let (mut studio, a, b) = studio();
    let kit = studio.kit.clone();
    let session = &mut studio.video;
    session.hits.enabled = true;
    session.hits.ask(&session.video, session.revision, &kit);
    session.hits.wait(&session.video);
    let (ha, hb) = (&session.hits.clips[&a], &session.hits.clips[&b]);
    assert!(!ha.is_empty(), "the ball hits something");
    assert_eq!(ha.len(), hb.len(), "the same scene from the start hits as often");
    for (x, y) in ha.iter().zip(hb) {
        assert!((y - x - 4.0).abs() < 1e-3, "clip B's hits are clip A's 4 s later ({x}, {y})");
    }
    assert!(ha.iter().all(|&h| (0.0..4.0).contains(&h)), "only inside the clip");
}

/// A studio editing clip A's scene, its Build scene "Other" (saved, at `other.ron`) parked.
fn editing() -> (Studio, ClipId) {
    let (mut studio, a, _) = studio();
    studio.scene.name = "Other".into();
    studio.path = Some(PathBuf::from("other.ron"));
    studio.saved = studio.scene.clone();
    studio.seen_mode = EditorMode::Video;
    assert!(studio.edit_clip_scene(a));
    (studio, a)
}

fn length_of(studio: &Studio) -> Value {
    studio.scene.things[&ThingId::ROOT].params.get("duration").cloned().unwrap()
}

#[test]
fn double_click_edits_the_clip_scene_and_back_puts_it_in() {
    let (studio, a, _) = studio();
    let mut h = harness(studio);
    let p = at(&h, 1.0);
    h.input_mut().events.push(Event::PointerMoved(p));
    h.step();
    for _ in 0..2 {
        press(&mut h, p, true, Modifiers::NONE);
        h.step();
        press(&mut h, p, false, Modifiers::NONE);
        h.step();
    }
    let studio = h.state_mut();
    assert!(studio.editing_clip(), "a double-click edits the clip's scene");
    assert_eq!(studio.mode, EditorMode::Build);
    assert_eq!(studio.scene.name, "Ring Drop");
    assert!(studio.path.is_none(), "not a scene file of its own");
    drop(h);
    let (mut studio, a2) = editing();
    assert_eq!(a, a2);
    studio.scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("duration", Value::Number(3.0));
    assert!(studio.video_unsaved(), "the edit is the video's");
    studio.back_to_video();
    assert!(!studio.editing_clip());
    assert_eq!(studio.mode, EditorMode::Video);
    assert_eq!(studio.scene.name, "Other", "the parked scene is back");
    assert_eq!(studio.path, Some(PathBuf::from("other.ron")));
    assert!(!studio.unsaved(), "and as saved as it was");
    let kept = clip_of(&studio, a).scene;
    let duration = studio.video.video.scenes[&kept].scene.things[&ThingId::ROOT].params.get("duration").cloned();
    assert_eq!(duration, Some(Value::Number(3.0)), "the edit is in the video");
    assert!(clip_of(&studio, a).length <= 3.0, "the clip fits its shorter scene");
    assert!(studio.video_undo(), "one undo step");
    let duration = studio.video.video.scenes[&kept].scene.things[&ThingId::ROOT].params.get("duration").cloned();
    assert_eq!(duration, Some(Value::Number(4.0)), "undo takes it back");
}

#[test]
fn saving_while_editing_saves_the_video() {
    let dir = temp_dir("edit-save");
    let (mut studio, a) = editing();
    let path = dir.join("Drop Tape.ballvideo");
    assert!(studio.save_video_to(path.clone()));
    studio.scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("duration", Value::Number(3.0));
    assert!(studio.save_work(false));
    assert!(studio.editing_clip(), "still editing");
    assert!(!studio.video_unsaved());
    let text = std::fs::read_to_string(&path).unwrap();
    let saved = bpe_kit::video::open_video(&text, &studio.kit.registry).unwrap();
    let kept = clip_of(&studio, a).scene;
    let duration = saved.scenes[&kept].scene.things[&ThingId::ROOT].params.get("duration").cloned();
    assert_eq!(duration, Some(Value::Number(3.0)), "the file has the edit");
    assert_eq!(length_of(&studio), Value::Number(3.0));
}

#[test]
fn opening_a_scene_while_editing_keeps_the_edit_in_the_video() {
    let (mut studio, a) = editing();
    studio.scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("duration", Value::Number(3.0));
    let ctx = egui::Context::default();
    studio.guard(crate::app::Pending::New, &ctx);
    assert!(!studio.editing_clip());
    assert_eq!(studio.mode, EditorMode::Build, "the new scene shows in Build mode");
    assert_ne!(studio.scene.name, "Other", "a new scene replaced the parked one");
    let kept = clip_of(&studio, a).scene;
    let duration = studio.video.video.scenes[&kept].scene.things[&ThingId::ROOT].params.get("duration").cloned();
    assert_eq!(duration, Some(Value::Number(3.0)));
}

#[test]
fn the_video_button_goes_back_to_video() {
    let (studio, _) = editing();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .allow_missing_glyphs()
        .with_context_setup(style::apply)
        .build_ui_state(|ui, studio: &mut Studio| studio.edit_frame_for_tests(ui), studio);
    h.run();
    assert!(h.query_by_label("Editing Ring Drop in the video Untitled video.").is_some(), "the bar says so");
    h.get_by_label("Video").click();
    h.run();
    assert!(!h.state().editing_clip(), "the Video button went back");
    assert_eq!(h.state().mode, EditorMode::Video);
    assert_eq!(h.state().scene.name, "Other");
}

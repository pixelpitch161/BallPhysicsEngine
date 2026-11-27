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
    let mut h = harness(Studio::headless_water(), egui::vec2(900.0, 80.0), crate::transport::controls);
    h.run();
    assert!(!has(&h, "Rules") && !has(&h, "Timeline"));

    let mut h = harness(Studio::headless(), egui::vec2(900.0, 500.0), side);
    h.run();
    assert!(has(&h, "Lights") && has(&h, "Mixer") && has(&h, "Things"));
    let mut h = harness(Studio::headless(), egui::vec2(900.0, 80.0), crate::transport::controls);
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

#[test]
fn new_asks_what_kind_then_about_unsaved_work_and_cancel_keeps_the_scene() {
    use crate::water_mode::{ChooserOutcome, NewScene};
    let ctx = egui::Context::default();
    let mut studio = Studio::headless();
    let before = studio.scene.clone();
    studio.scene.name = "Mine".into();
    studio.after_edit(before);
    assert!(studio.unsaved());
    let kept = studio.scene.clone();
    studio.set_playing(true);

    studio.ask_new_scene();
    assert!(studio.chooser.as_ref().is_some_and(|c| !c.at_start) && !studio.playing);
    assert_eq!(studio.scene, kept, "the open scene stays until the chooser is answered");
    studio.answer_chooser(ChooserOutcome::Create(NewScene::water(StartingLook::Molten)), &ctx);
    assert!(studio.chooser.is_none());
    assert!(matches!(studio.confirm, Some(Pending::New(_))), "unsaved work is asked about first");
    assert_eq!(studio.scene, kept);
    // Cancel on that question: the scene is kept exactly, and the chooser is gone.
    studio.confirm = None;
    assert_eq!(studio.scene, kept);
    assert!(studio.chooser.is_none() && studio.unsaved() && studio.history.can_undo());

    // Again, and this time Don't save.
    studio.ask_new_scene();
    studio.answer_chooser(ChooserOutcome::Create(NewScene::water(StartingLook::Molten)), &ctx);
    let action = studio.confirm.take().expect("asked");
    studio.perform(action, &ctx);
    assert!(studio.water() && !studio.unsaved() && !studio.history.can_undo());
    assert_eq!((studio.scene.name.as_str(), studio.path.as_ref()), ("Water course", None));
    assert_eq!(studio.status, "Started a new Water scene.");
    let water = &studio.resolved.things[&ThingId::ROOT].slots[&SlotId::Water];
    let world = bpe_kit::modules::water::WaterWorld::from_params(&water.params);
    assert_eq!(world.look, StartingLook::Molten, "the look picked is remembered");

    // Cancel on the chooser itself changes nothing.
    studio.ask_new_scene();
    studio.answer_chooser(ChooserOutcome::Cancel, &ctx);
    assert!(studio.chooser.is_none() && studio.water() && studio.confirm.is_none());

    // Nothing unsaved: a Balls scene starts at once, under the name typed.
    studio.ask_new_scene();
    let named = NewScene { name: " Bouncy ".into(), ..NewScene::balls() };
    studio.answer_chooser(ChooserOutcome::Create(named), &ctx);
    assert!(!studio.water() && studio.confirm.is_none() && studio.chooser.is_none());
    assert_eq!(studio.scene.name, "Bouncy");
    assert!(studio.scene.card(ThingId::ROOT, SlotId::Mixer).is_some(), "the blank Balls project, as before");
    assert_eq!(studio.status, "Started a new scene.");
}

#[test]
fn the_chooser_goes_when_a_scene_opens_and_blocks_no_headless_start() {
    use crate::water_mode::Chooser;
    assert!(Studio::blank().chooser.is_none() && Studio::headless().chooser.is_none());
    let mut studio = Studio::blank();
    studio.chooser = Some(Chooser::at_start());
    let scene = bpe_kit::new_scene(&studio.kit.registry);
    studio.replace_scene(scene, None);
    assert!(studio.chooser.is_none(), "a scene dropped on the window answers it");
}

#[test]
fn the_chooser_shows_as_a_dialog_and_creates_a_water_scene() {
    let mut studio = Studio::blank();
    studio.chooser = Some(crate::water_mode::Chooser::at_start());
    let mut h = harness(studio, egui::vec2(1100.0, 800.0), |ui, s| {
        let ctx = ui.ctx().clone();
        s.chooser_dialog(&ctx);
        s.confirm_dialog(&ctx);
    });
    h.run();
    assert!(has(&h, "New scene") && has(&h, "Balls") && !has(&h, "Cancel"));
    for label in ["Water", "Next", "Glass lab", "Create Water scene"] {
        h.query_all_by_label(label).next().unwrap_or_else(|| panic!("no {label}")).click();
        h.run();
    }
    assert!(h.state().water() && h.state().chooser.is_none());
    assert!(!has(&h, "New scene"), "the dialog is gone");
    // While the question about unsaved work is up, the chooser waits behind it.
    h.state_mut().chooser = Some(crate::water_mode::Chooser::for_new());
    h.state_mut().confirm = Some(Pending::Close);
    h.run();
    assert!(!has(&h, "New scene") && has(&h, "Don't save"));
}

/// The first bytes of a PNG: enough for the drop to take it for a picture.
fn png() -> std::sync::Arc<[u8]> {
    vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A].into()
}

/// A tenth of a second of silence as a WAV file.
fn wav() -> std::sync::Arc<[u8]> {
    let samples = vec![0u8; 4800 * 2];
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((36 + samples.len() as u32).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes()); // PCM
    bytes.extend(1u16.to_le_bytes()); // mono
    bytes.extend(48_000u32.to_le_bytes());
    bytes.extend(96_000u32.to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend((samples.len() as u32).to_le_bytes());
    bytes.extend(samples);
    bytes.into()
}

fn font() -> std::sync::Arc<[u8]> {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts/text");
    let first = std::fs::read_dir(folder).unwrap().filter_map(Result::ok).map(|e| e.path());
    let font = first.filter(|p| p.extension().is_some_and(|e| e == "ttf" || e == "otf")).min().expect("a font");
    std::fs::read(font).unwrap().into()
}

#[test]
fn a_picture_song_or_font_dropped_on_a_water_scene_is_let_go_with_a_sentence() {
    let mut studio = Studio::headless_water();
    let before = studio.scene.clone();
    studio.drop_picture("dot.png", png());
    assert_eq!(studio.status, NO_WATER_DROPS);
    studio.status.clear();
    studio.drop_song("song.wav", wav());
    assert_eq!(studio.status, NO_WATER_DROPS);
    studio.status.clear();
    studio.drop_font("font.ttf", font());
    assert_eq!(studio.status, NO_WATER_DROPS);
    assert!(studio.scene == before, "nothing is kept in the scene");
    assert!(!studio.separate_edit && !studio.unsaved(), "and it is no edit");

    // A Balls scene takes all three as before.
    let mut studio = Studio::headless();
    studio.drop_picture("dot.png", png());
    studio.drop_song("song.wav", wav());
    studio.drop_font("font.ttf", font());
    let scene = &studio.scene;
    assert_eq!((scene.pictures.len(), scene.audio.len(), scene.fonts.len()), (1, 1, 1));
}

#[test]
fn a_picture_dropped_behind_the_start_chooser_is_let_go() {
    let mut studio = Studio::headless();
    studio.chooser = Some(crate::water_mode::Chooser::at_start());
    let before = studio.scene.clone();
    studio.drop_picture("dot.png", png());
    studio.drop_song("song.wav", wav());
    studio.drop_font("font.ttf", font());
    assert!(studio.scene == before, "the blank project behind the chooser is not touched");
    assert_eq!(studio.status, DROP_AFTER_CHOOSING);
}

#[test]
fn a_physics_edit_restarts_the_draft_and_a_look_edit_does_not() {
    let mut studio = Studio::headless_water();
    let kit = studio.kit.clone();
    let liquid =
        bpe_kit::add_thing(&mut studio.scene, &kit.registry, bpe_kit::things::water::LIQUID, ThingId::ROOT).unwrap();
    studio.scene_changed();
    let print = |studio: &Studio| studio.water_draft.setup.as_ref().expect("a Water scene has its water").print;
    let before = print(&studio);
    let set = |studio: &mut Studio, slot, key: &str, v| {
        studio.scene.things.get_mut(&liquid).unwrap().slots.get_mut(&slot).unwrap().params.set(key, v)
    };
    set(&mut studio, SlotId::Look, "colour", Value::Color(bpe_core::Color::rgb(200, 10, 10)));
    studio.scene_changed();
    assert_eq!(print(&studio), before, "a new colour plays the same water");
    set(&mut studio, SlotId::Feel, "thickness", Value::Number(0.6));
    studio.scene_changed();
    assert_ne!(print(&studio), before, "a thicker liquid starts the water over");
}

#[test]
fn a_piece_edit_restarts_the_draft_and_a_colour_edit_does_not() {
    use bpe_kit::things::water::PIECE;
    let mut studio = Studio::headless_water();
    let kit = studio.kit.clone();
    let piece = bpe_kit::add_thing(&mut studio.scene, &kit.registry, PIECE, ThingId::ROOT).unwrap();
    studio.scene_changed();
    let print = |studio: &Studio| studio.water_draft.setup.as_ref().expect("a Water scene has its water").print;
    let set = |studio: &mut Studio, slot, key: &str, v| {
        studio.scene.things.get_mut(&piece).unwrap().slots.get_mut(&slot).unwrap().params.set(key, v);
        studio.scene_changed();
    };
    let before = print(&studio);
    set(&mut studio, SlotId::Material, "colour", Value::Color(bpe_core::Color::rgb(200, 10, 10)));
    assert_eq!(print(&studio), before, "a new colour plays the same water");
    set(&mut studio, SlotId::Moves, "moves", Value::Choice("keyframed".into()));
    assert_eq!(print(&studio), before, "a piece that will move (a later part) plays the same water");
    set(&mut studio, SlotId::Form, "size", Value::Vec2([120.0, 40.0]));
    let sized = print(&studio);
    assert_ne!(sized, before, "a new size starts the water over");
    studio.scene.things.get_mut(&piece).unwrap().transform.position.x += 30.0;
    studio.scene_changed();
    assert_ne!(print(&studio), sized, "so does a move");
}

#[test]
fn a_balls_scene_has_no_draft() {
    let mut studio = Studio::headless_water();
    assert!(studio.water_draft.setup.is_some());
    studio.new_scene(crate::water_mode::NewScene::balls());
    assert!(studio.water_draft.setup.is_none() && studio.water_draft.job.is_none());
    assert!(Studio::headless().water_draft.setup.is_none());
}

#[test]
fn the_status_line_counts_the_draft_drops() {
    assert_eq!(crate::app::water_readout(52_300, 8.0, false), "· Water: 52,300 drops (draft 8 px)");
    assert_eq!(
        crate::app::water_readout(500_000, 12.0, true),
        "· Water: 500,000 drops (draft 12 px): the most a draft holds, so the pours have stopped"
    );
}

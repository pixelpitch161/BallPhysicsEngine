use super::*;
use bpe_core::{Address, SlotId, ThingId, Value};
use bpe_kit::BallsSettings;
use std::path::Path;
use std::time::Duration;

use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};

#[test]
fn no_device_falls_back_to_the_wall_clock() {
    let mut studio = Studio::headless();
    assert!(studio.sound.is_none(), "no speakers in tests");
    studio.set_playing(true);
    studio.tick();
    std::thread::sleep(Duration::from_millis(30));
    studio.tick();
    assert!(studio.time > 0.02, "the wall clock moves the playhead: {}", studio.time);
}

#[test]
fn an_air_edit_restarts_only_the_particles_and_the_toolbar_shows_the_air() {
    use bpe_kit::things::particles::looks::{Look, add_particles};
    let mut studio = Studio::headless();
    let kit = studio.kit.clone();
    let before = studio.scene.clone();
    add_particles(&mut studio.scene, &kit.registry, Look::DustClouds, ThingId::ROOT).unwrap();
    studio.after_edit(before);
    assert!(studio.particle_job.is_some());
    studio.sound = Some(SoundJob::recording());
    let (print, physics) = (studio.particle_print, studio.fingerprint);
    let before = studio.scene.clone();
    studio.scene.card_mut(ThingId::ROOT, SlotId::Air).unwrap().params.set("wind_speed", Value::Number(300.0));
    studio.after_edit(before);
    assert_eq!(studio.fingerprint, physics, "the balls play on");
    assert_ne!(studio.particle_print, print, "the particles start again");
    let sent = studio.sound.as_ref().unwrap().recorded();
    assert!(!sent.contains(&"rebuild"), "the sound plays on: {sent:?}");
    let mut h = Harness::builder()
        .with_size(egui::vec2(1500.0, 80.0))
        .allow_missing_glyphs()
        .with_context_setup(style::apply)
        .build_ui_state(|ui, s: &mut Studio| s.toolbar(ui), studio);
    h.run();
    assert!(!h.state().show_air);
    h.get_by_label("Air").click();
    h.run();
    assert!(h.state().show_air);
}

#[test]
fn a_physics_edit_rebuilds_and_a_tempo_edit_patches() {
    let mut studio = Studio::headless();
    studio.sound = Some(SoundJob::recording());
    studio.set_playing(true);
    let ball = studio.scene.order.iter().copied().find(|id| studio.scene.things[id].kind == "balls").unwrap();
    let before = studio.scene.clone();
    studio.scene.things.get_mut(&ball).unwrap().transform.position.x += 40.0;
    studio.after_edit(before);
    let before = studio.scene.clone();
    studio.scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("tempo", Value::Number(97.0));
    studio.after_edit(before);
    studio.set_playing(false);
    let sent = studio.sound.as_ref().unwrap().recorded();
    assert_eq!(sent, ["seek", "play", "rebuild", "seek", "patch", "pause"]);
}

#[test]
fn a_trail_length_edit_keeps_the_sound_playing() {
    let mut studio = Studio::headless();
    let kit = studio.kit.clone();
    let before = studio.scene.clone();
    let trail =
        studio.scene.add_layer(&kit.registry, ThingId(2), SlotId::Look, bpe_kit::modules::look::layers::TRAIL).unwrap();
    studio.after_edit(before);
    studio.sound = Some(SoundJob::recording());
    studio.set_playing(true);
    let physics = studio.fingerprint;
    let before = studio.scene.clone();
    // Longer when fast stretches its half-second Length to 3.5 s.
    let faster = Address::on(ThingId(2), SlotId::Look, &bpe_core::layered_key(trail, "faster"));
    studio.scene.set_at(&kit.registry, &faster, Value::Number(2.0)).unwrap();
    studio.after_edit(before);
    assert_ne!(studio.fingerprint, physics, "the editor keeps a longer history for the trail");
    let sent = studio.sound.as_ref().unwrap().recorded();
    assert_eq!(sent, ["seek", "play"], "the sound plays on, its settings untouched");
}

#[test]
fn only_a_sound_edit_builds_the_sound_again() {
    let mut studio = Studio::headless();
    let kit = studio.kit.clone();
    let shape = studio.scene.order.iter().copied().find(|id| studio.scene.things[id].kind == "shape").unwrap();
    let before = studio.scene.clone();
    studio
        .scene
        .things
        .get_mut(&shape)
        .unwrap()
        .slots
        .insert(SlotId::Sound, bpe_kit::sound::starter_voice(&kit.registry));
    studio.after_edit(before);
    studio.sound = Some(SoundJob::recording());
    // A picture drag, frame after frame: nothing for the sound.
    for k in 1..6 {
        let before = studio.scene.clone();
        let bloom = Address::own(ThingId::ROOT, "bloom");
        studio.scene.set_at(&kit.registry, &bloom, Value::Number(k as f64 * 0.1)).unwrap();
        studio.after_edit(before);
    }
    assert!(studio.sound.as_ref().unwrap().recorded().is_empty(), "a picture edit leaves the sound alone");
    // A sound knob does.
    let before = studio.scene.clone();
    let gain = Address::on(shape, SlotId::Sound, "a_gain");
    studio.scene.set_at(&kit.registry, &gain, Value::Number(-9.0)).unwrap();
    studio.after_edit(before);
    assert_eq!(studio.sound.as_ref().unwrap().recorded(), ["patch"]);
}

#[test]
fn a_key_clicked_while_stopped_sounds_at_the_playhead() {
    let mut studio = Studio::headless();
    studio.sound = Some(SoundJob::recording());
    let click = |studio: &mut Studio| {
        studio.panels.inspector.auditions.push((ThingId(1), 60));
        studio.send_auditions();
    };
    // At the start the sound is where the playhead is.
    click(&mut studio);
    // Scrubbing tells it nothing; the click takes it there first.
    studio.seek(3.0);
    studio.seek(5.0);
    click(&mut studio);
    click(&mut studio);
    // A rebuild starts the sound at 0 again.
    let before = studio.scene.clone();
    studio.scene.things.get_mut(&ThingId(2)).unwrap().transform.position.x += 40.0;
    studio.after_edit(before);
    click(&mut studio);
    let sent = studio.sound.as_ref().unwrap().recorded();
    assert_eq!(sent, ["audition", "seek", "audition", "audition", "rebuild", "seek", "audition"]);
}

#[test]
fn another_scene_forgets_the_export_file_chosen() {
    let mut studio = Studio::headless();
    studio.panels.export.path = Some(PathBuf::from("/videos/A.mp4"));
    studio.new_scene();
    assert_eq!(studio.panels.export.path, None, "exporting the new scene doesn't overwrite A.mp4");
    studio.panels.export.path = Some(PathBuf::from("/videos/A.mp4"));
    studio.replace_scene(bpe_kit::new_scene(&studio.kit.registry), Some(PathBuf::from("/scenes/B.json")));
    assert_eq!(studio.panels.export.path, None);
}

#[test]
fn seeking_while_playing_tells_the_sound() {
    let mut studio = Studio::headless();
    studio.sound = Some(SoundJob::recording());
    studio.seek(1.0);
    studio.set_playing(true);
    studio.seek(2.0);
    let sent = studio.sound.as_ref().unwrap().recorded();
    assert_eq!(sent, ["seek", "play", "seek"], "a seek while stopped waits for play");
    assert_eq!(studio.sound.as_ref().unwrap().clock(), 2.0, "the clock holds at the seek");
}

/// The studio's left panel beside an empty middle; each frame's change is recorded as the
/// editor does. The state keeps the panel's width as drawn.
fn left_harness() -> Harness<'static, (Studio, f32)> {
    let studio = Studio::headless();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(120)
        .allow_missing_glyphs()
        .with_context_setup(style::apply)
        .build_ui_state(
            |ui, (studio, width): &mut (Studio, f32)| {
                let before = studio.scene.clone();
                studio.recorded = false;
                *width = studio.left_panel(ui, &before);
                egui::CentralPanel::default().show(ui, |_| {});
                if !std::mem::take(&mut studio.recorded) {
                    studio.after_edit(before);
                }
            },
            (studio, 0.0),
        );
    h.run();
    h
}

#[test]
fn switching_to_mixer_shows_tempo_and_master() {
    let mut h = left_harness();
    assert!(h.query_by_label("Tempo").is_none());
    h.get_by_label("Mixer").click();
    h.run();
    assert_eq!(h.state().0.left, LeftPanel::Mixer);
    for name in ["Tempo", "Beats in a bar", "Beat note", "Master", "Input", "Output", "Release", "Whole clip"] {
        assert!(h.query_by_label(name).is_some(), "{name} is shown");
    }
    assert!((h.state().1 - style::TREE_W).abs() < 1.0, "the Mixer opens as wide as the tree: {}", h.state().1);
}

#[test]
fn turning_the_ceiling_knob_edits_the_mixer_card_and_undoes() {
    let mut h = left_harness();
    h.get_by_label("Mixer").click();
    h.run();
    let dial = h
        .query_all_by_label("Ceiling")
        .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Slider)
        .unwrap()
        .rect()
        .center();
    h.hover_at(dial);
    h.run();
    let press = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    h.event(press(dial, true));
    h.step();
    for k in 1..=4 {
        h.event(egui::Event::PointerMoved(dial + egui::vec2(0.0, 50.0 * k as f32 / 4.0)));
        h.step();
    }
    h.event(press(dial + egui::vec2(0.0, 50.0), false));
    h.run();
    let ceiling =
        |s: &Studio| s.scene.card(ThingId::ROOT, SlotId::Mixer).unwrap().params.get("master_ceiling").cloned();
    // 50 px down of a 200 px turn over 12 dB.
    assert_eq!(ceiling(&h.state().0), Some(Value::Number(-4.0)));
    assert!(h.state_mut().0.undo());
    assert_eq!(ceiling(&h.state().0), None, "back to the -1 dBTP default");
}

#[test]
fn master_output_takes_keyframes_but_ceiling_doesnt() {
    let mut h = left_harness();
    h.get_by_label("Mixer").click();
    h.run();
    let knob = |h: &Harness<'_, (Studio, f32)>, label: &str| {
        h.query_all_by_label(label)
            .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Slider)
            .unwrap()
            .click_secondary()
    };
    knob(&h, "Output");
    h.run();
    assert!(h.query_by_label("Copy Value").is_some(), "the rest of the value menu is there");
    h.get_by_label("Create Keyframe").click();
    h.run();
    let a = Address::on(ThingId::ROOT, SlotId::Mixer, "master_output");
    assert!(h.state().0.scene.keyed(&a), "Output is keyed");
    h.key_press(egui::Key::Escape);
    h.run();
    knob(&h, "Ceiling");
    h.run();
    assert!(h.get_by_label("Create Keyframe").accesskit_node().is_disabled(), "Ceiling takes no keys");
}

/// Drags the left panel's edge from where it is by `dx`.
fn drag_left_edge(h: &mut Harness<'_, (Studio, f32)>, dx: f32) {
    // Just outside the panel: inside it, its scroll bar is on top of the edge.
    let edge = egui::pos2(h.state().1 + 2.0, 450.0);
    crate::mixer::strips::tests::drag(h, edge, edge + egui::vec2(dx, 0.0));
}

#[test]
fn the_mixer_opens_at_the_trees_width() {
    let mut h = left_harness();
    drag_left_edge(&mut h, -200.0);
    let tree = h.state().1;
    // (The drag starts past egui's drag threshold, a few pixels short.)
    assert!((tree - (style::TREE_W - 200.0)).abs() < 10.0, "the tree was dragged narrower: {tree}");
    h.get_by_label("Mixer").click();
    h.run();
    assert!((h.state().1 - tree).abs() < 0.5, "the Mixer opens at the tree's {tree}: {}", h.state().1);
    // Dragged wider while open, it stays so until it is switched away.
    drag_left_edge(&mut h, 300.0);
    let wide = h.state().1;
    assert!(wide > tree + 250.0, "the Mixer was dragged wider: {wide}");
    h.get_by_label("Things").click();
    h.run();
    assert_eq!(h.state().0.left, LeftPanel::Things);
    assert!((h.state().1 - tree).abs() < 0.5, "{} back to {tree}", h.state().1);
    // Opened again, it is the tree's width once more.
    h.get_by_label("Mixer").click();
    h.run();
    assert!((h.state().1 - tree).abs() < 0.5, "{} opens at {tree} again", h.state().1);
}

#[test]
fn a_narrow_mixer_still_draws_its_strips() {
    let mut h = left_harness();
    drag_left_edge(&mut h, -400.0);
    assert!((h.state().1 - 180.0).abs() < 1.0, "the tree at its narrowest: {}", h.state().1);
    h.get_by_label("Mixer").click();
    h.run();
    assert!((h.state().1 - 180.0).abs() < 1.0, "the Mixer as narrow: {}", h.state().1);
    for name in ["Bus 1 fader", "Bus 3 fader", "Ceiling", "Pan"] {
        assert!(h.query_all_by_label(name).next().is_some(), "{name} is drawn");
    }
    // The strips keep their order, side by side at their narrowest (48 px and a 6 px gap),
    // scrolling sideways past the edge.
    let x = |h: &Harness<'_, (Studio, f32)>, name: &str| h.query_all_by_label(name).next().unwrap().rect().left();
    let step = x(&h, "Bus 2 fader") - x(&h, "Bus 1 fader");
    assert!((step - 54.0).abs() < 1.0, "{step}");
    assert!(x(&h, "Bus 3 fader") - x(&h, "Bus 2 fader") == step);
    // A little wider, all four fit.
    h.get_by_label("Things").click();
    h.run();
    drag_left_edge(&mut h, 120.0);
    h.get_by_label("Mixer").click();
    h.run();
    let width = h.state().1;
    assert!((280.0..320.0).contains(&width), "{width}");
    for name in ["Ceiling", "Release"] {
        let knob = h.query_all_by_label(name).find(|n| n.accesskit_node().role() == egui::accesskit::Role::Slider);
        let r = knob.unwrap().rect();
        assert!(r.right() <= width, "{name} at {r:?} is inside the {width} px Mixer");
    }
}

/// The studio's inspector beside an empty middle, in a wide window. The state keeps the
/// inspector's width as drawn.
fn inspector_harness(selected: ThingId) -> Harness<'static, (Studio, f32)> {
    let mut studio = Studio::headless();
    studio.selection.set(selected);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_max_steps(120)
        .allow_missing_glyphs()
        .with_context_setup(style::apply)
        .build_ui_state(
            |ui, (studio, width): &mut (Studio, f32)| {
                *width = studio.inspector_panel(ui);
                egui::CentralPanel::default().show(ui, |_| {});
            },
            (studio, 0.0),
        );
    h.run();
    h
}

#[test]
fn the_inspector_width_holds_when_selecting_scene_settings() {
    let mut h = inspector_harness(ThingId(2));
    let ball = h.state().1;
    h.state_mut().0.selection.set(ThingId::ROOT);
    h.run();
    let scene = h.state().1;
    assert!((ball - scene).abs() <= 1.0, "a ball's inspector is {ball} wide, Scene settings' {scene}");
}

#[test]
fn a_squeezed_window_does_not_change_the_saved_width() {
    let mut h = inspector_harness(ThingId(2));
    h.set_size(egui::vec2(380.0, 900.0));
    h.run();
    assert!(h.state().1 < 370.0, "the window squeezes the panel to {}", h.state().1);
    assert_eq!(h.state().0.inspector_width, 370.0);
}

#[test]
fn dragging_the_inspector_edge_saves_its_width() {
    let mut h = inspector_harness(ThingId(2));
    let edge = egui::pos2(1400.0 - 370.0, 450.0);
    let to = egui::pos2(1400.0 - 500.0, 450.0);
    h.hover_at(edge);
    h.step();
    h.event(egui::Event::PointerButton {
        pos: edge,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.step();
    h.event(egui::Event::PointerMoved(to));
    h.step();
    assert_eq!(h.state().0.inspector_width, 370.0, "nothing is saved while the button is down");
    h.event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    let (saved, shown) = (h.state().0.inspector_width, h.state().1);
    // The drag starts a few pixels in, past egui's drag threshold, so it lands near 500.
    assert!(saved > 480.0 && saved == shown, "the dragged width {shown} is saved, got {saved}");
}

/// Where the toolbar's save status ends and Export starts, for a scene saved as `name` in
/// a window `width` wide.
fn status_and_export(name: &str, width: f32) -> (egui::Rect, egui::Rect) {
    let mut studio = Studio::headless();
    studio.path = Some(PathBuf::from(format!("/scenes/{name}")));
    let mut h = Harness::builder()
        .with_size(egui::vec2(width, 80.0))
        .with_max_steps(20)
        .allow_missing_glyphs()
        .with_context_setup(style::apply)
        .build_ui_state(|ui, studio: &mut Studio| studio.toolbar(ui), studio);
    h.run();
    let saved = |n: &egui_kittest::Node<'_>| {
        n.accesskit_node().role() == egui::accesskit::Role::Label
            && n.accesskit_node().value().is_some_and(|v| v.starts_with("Saved as"))
    };
    let status = h.root().children_recursive().find(saved).expect("the save status").rect();
    let export = h.get_by_label("Export video…").rect();
    (status, export)
}

#[test]
fn a_long_save_status_does_not_reach_export() {
    let long = format!("{}.ron", &"a long scene name ".repeat(7)[..116]);
    assert_eq!(long.len(), 120);
    // The app's narrowest window, the screenshots' width, and a wide one.
    for width in [1000.0, 1440.0, 1600.0] {
        for name in ["blend-modes.ron", long.as_str()] {
            let (status, export) = status_and_export(name, width);
            // On Export's line it ends before Export; at a narrow width it may go to the next.
            let beside = status.top() < export.bottom();
            assert!(
                !beside || status.right() < export.left(),
                "at {width}, {name}: the status {status:?} runs into Export {export:?}"
            );
            if width >= 1440.0 {
                assert!(beside, "at {width} the toolbar is one line: {status:?}, Export {export:?}");
            }
        }
    }
}

#[test]
fn the_narrowest_inspector_keeps_its_value_boxes_inside() {
    let mut studio = Studio::headless();
    studio.selection.set(ThingId(2));
    studio.inspector_width = *settings::INSPECTOR_RANGE.start();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 2400.0))
        .with_max_steps(120)
        .allow_missing_glyphs()
        .with_context_setup(style::apply)
        .build_ui_state(
            |ui, (studio, width): &mut (Studio, f32)| {
                *width = studio.inspector_panel(ui);
                egui::CentralPanel::default().show(ui, |_| {});
            },
            (studio, 0.0),
        );
    h.run();
    let panel = h.state().1;
    assert!((panel - 320.0).abs() <= 1.0, "the panel is at its minimum: {panel}");
    let glow = h.get_all_by_role_and_label(egui::accesskit::Role::Slider, "Size").next().unwrap().rect();
    // The number box sits right of the slider, level with it.
    let value = h
        .root()
        .children_recursive()
        .filter(|n| n.accesskit_node().role() == egui::accesskit::Role::SpinButton)
        .map(|n| n.rect())
        .find(|r| r.left() > glow.right() - 1.0 && (r.center().y - glow.center().y).abs() < 4.0)
        .expect("Size's number box");
    assert!(value.right() <= 1000.0 - 10.0, "Size's number box ends at {}, past the panel", value.right());
    // An x and y pair that can't fit side by side goes onto two lines, inside its row.
    let speed: Vec<egui::Rect> =
        h.get_all_by_role_and_label(egui::accesskit::Role::SpinButton, "Launch speed").map(|n| n.rect()).collect();
    assert_eq!(speed.len(), 2);
    let labels: Vec<egui::Rect> = h
        .root()
        .children_recursive()
        .filter(|n| n.accesskit_node().role() == egui::accesskit::Role::Label)
        .map(|n| n.rect())
        .collect();
    for r in &speed {
        assert!(r.right() <= 1000.0 - 10.0, "Launch speed {r:?} runs past the panel");
        assert!(!labels.iter().any(|l| l.intersects(r.shrink(1.0))), "Launch speed {r:?} runs into another row");
    }
    // Every slider row's number box fits, and so do the colours' buttons.
    for button in ["Palette", "Smooth"] {
        for node in h.query_all_by_label(button) {
            assert!(node.rect().right() <= 1000.0 - 10.0, "{button} ends at {}", node.rect().right());
        }
    }
}

#[test]
fn opening_an_old_scene_reports_it_and_keeps_the_current_one() {
    let mut studio = Studio::headless();
    let old = concat!(env!("CARGO_MANIFEST_DIR"), "/../bpe-core/tests/data/old-editor-scene.ron");
    studio.open_path(Path::new(old));
    let problem = studio.problem.clone().expect("a failed open is a problem, not a grey status");
    assert_eq!(
        problem,
        Problem { kind: ProblemKind::File, text: "This scene was made with the old Ball Studio".into() }
    );
    assert_eq!(studio.scene.things.len(), 3);
    studio.undo();
    assert!(studio.problem.is_some(), "a later message doesn't hide it");
}

#[test]
fn problems_stay_until_their_own_source_clears_them() {
    let mut studio = Studio::headless();
    studio.report(ProblemKind::File, "Couldn't save");
    studio.clear_problem(ProblemKind::Speakers);
    assert_eq!(
        studio.problem.as_ref().map(|p| p.kind),
        Some(ProblemKind::File),
        "the speakers can't clear a save failure"
    );
    studio.report(ProblemKind::Speakers, "The speakers stopped working.");
    studio.clear_problem(ProblemKind::Speakers);
    assert_eq!(studio.problem, None);
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ball-studio-app-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn session_file(studio: &Studio) -> PathBuf {
    let session = studio.recovery.as_ref().expect("autosaving");
    session.flush();
    session.files().scene.clone()
}

#[test]
fn unsaved_work_is_autosaved_and_the_copy_goes_once_it_is_saved() {
    let dir = temp_dir("autosave");
    let mut studio = Studio::blank();
    studio.start_recovery(&dir.join("recovery"), None);
    let now = Instant::now();
    studio.autosave(now);
    assert!(!session_file(&studio).exists(), "nothing unsaved, nothing copied");
    let before = studio.scene.clone();
    studio.scene.name = "Changed".into();
    studio.after_edit(before);
    assert_eq!(studio.autosave(now), None, "everything copied: nothing to wake for");
    assert!(session_file(&studio).exists(), "the first change is copied at once");
    let first = std::fs::read_to_string(session_file(&studio)).unwrap();
    let before = studio.scene.clone();
    studio.scene.name = "Changed again".into();
    studio.after_edit(before);
    let wait = studio.autosave(now + Duration::from_secs(5)).expect("wakes for the next copy");
    assert!(wait <= recovery::AUTOSAVE_EVERY - Duration::from_secs(5), "{wait:?}");
    assert_eq!(std::fs::read_to_string(session_file(&studio)).unwrap(), first, "not again within the minute");
    studio.autosave(now + recovery::AUTOSAVE_EVERY);
    assert!(std::fs::read_to_string(session_file(&studio)).unwrap().contains("Changed again"), "a minute later");
    let crash = studio.crash_slot.lock().unwrap().as_ref().map(|c| c.scene.name.clone());
    assert_eq!(crash.as_deref(), Some("Changed again"), "a crash would save the latest");
    studio.path = Some(dir.join("Mine.ron"));
    assert!(studio.save(false));
    studio.autosave(now + recovery::AUTOSAVE_EVERY * 2);
    assert!(!session_file(&studio).exists(), "saved, so the copy goes");
    assert!(studio.crash_slot.lock().unwrap().is_none());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_copy_written_by_a_caught_panic_still_goes_when_the_work_is_saved() {
    let dir = temp_dir("caught");
    let mut studio = Studio::blank();
    studio.start_recovery(&dir.join("recovery"), None);
    studio.path = Some(dir.join("Mine.ron"));
    assert!(studio.save(false));
    let now = Instant::now();
    studio.scene.name = "Edited".into();
    studio.autosaved_at = Some(now); // within the minute: the autosave hasn't written it
    studio.autosave(now);
    let files = studio.recovery.as_ref().unwrap().files().clone();
    assert!(recovery::crash_save(&studio.crash_slot, &files), "a panic (caught, say) writes the copy");
    assert!(session_file(&studio).exists());
    assert!(studio.save(false));
    studio.autosave(now);
    assert!(!session_file(&studio).exists(), "saved, so it goes too");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_clean_exit_keeps_the_copy_only_when_work_was_not_saved_or_thrown_away() {
    use eframe::App;
    for (closing, kept) in [(false, true), (true, false)] {
        let dir = temp_dir(if kept { "exit-kept" } else { "exit-gone" });
        let mut studio = Studio::blank();
        studio.start_recovery(&dir, None);
        studio.scene.name = "Unsaved".into();
        studio.autosave(Instant::now());
        let file = session_file(&studio);
        // `closing` is set by "Don't save"; without it, the editor was closed some other way.
        studio.closing = closing;
        studio.on_exit();
        assert_eq!(file.exists(), kept, "closing: {closing}");
        assert!(studio.crash_slot.lock().unwrap().is_none(), "a panic on the way out writes nothing");
        assert_eq!(recovery::orphans(&dir).len(), usize::from(kept));
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn restoring_with_the_dialog_or_discarding_it() {
    for restore in [true, false] {
        let dir = temp_dir(if restore { "dialog-restore" } else { "dialog-discard" });
        let mut crashed = Studio::blank();
        crashed.start_recovery(&dir, None);
        crashed.scene.name = "Left behind".into();
        crashed.autosave(Instant::now());
        session_file(&crashed);
        drop(crashed.recovery.take());
        let mut studio = Studio::blank();
        studio.start_recovery(&dir, None);
        let mut h = Harness::builder()
            .with_size(egui::vec2(1000.0, 600.0))
            .with_max_steps(20)
            .allow_missing_glyphs()
            .with_context_setup(style::apply)
            .build_ui_state(
                |ui, studio: &mut Studio| {
                    egui::CentralPanel::default().show(ui, |_| {});
                    let ctx = ui.ctx().clone();
                    studio.restore_dialog(&ctx);
                },
                studio,
            );
        h.run();
        assert!(h.query_by_label("Ball Studio closed unexpectedly").is_some());
        h.get_by_label(if restore { "Restore" } else { "Discard" }).click();
        h.run();
        assert!(h.state().offer.is_none());
        assert_eq!(h.state().scene.name == "Left behind", restore);
        assert!(h.query_by_label("Ball Studio closed unexpectedly").is_none());
        assert_eq!(recovery::orphans(&dir), vec![], "the old copy is gone either way");
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn an_older_copy_than_its_file_warns_and_leads_with_discard() {
    let dir = temp_dir("older");
    let file = dir.join("Mine.ron");
    let mut crashed = Studio::blank();
    crashed.path = Some(file.clone());
    assert!(crashed.save(false));
    crashed.start_recovery(&dir.join("recovery"), None);
    crashed.scene.name = "Older work".into();
    crashed.autosave(Instant::now());
    let copy = session_file(&crashed);
    drop(crashed.recovery.take());
    let earlier = std::time::SystemTime::now() - Duration::from_secs(3600);
    std::fs::File::options().write(true).open(&copy).unwrap().set_modified(earlier).unwrap();
    let mut studio = Studio::blank();
    studio.start_recovery(&dir.join("recovery"), None);
    assert!(studio.offer.as_ref().unwrap().file_newer, "the file was saved after the copy");
    assert_eq!(studio.offer.as_ref().unwrap().when, "1 hour ago");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_autosave_failure_is_told_and_cleared_when_a_later_one_works() {
    let dir = temp_dir("autosave-fail");
    let mut studio = Studio::blank();
    studio.start_recovery(&dir, None);
    let blocker = crate::files::sibling(&studio.recovery.as_ref().unwrap().files().path, ".tmp");
    std::fs::create_dir_all(&blocker).unwrap();
    studio.scene.name = "A".into();
    let now = Instant::now();
    studio.autosave(now);
    session_file(&studio);
    studio.autosave(now);
    assert_eq!(studio.problem.as_ref().map(|p| p.kind), Some(ProblemKind::Autosave));
    std::fs::remove_dir_all(&blocker).unwrap();
    studio.scene.name = "B".into();
    studio.autosave(now + recovery::AUTOSAVE_EVERY);
    session_file(&studio);
    studio.autosave(now + recovery::AUTOSAVE_EVERY);
    assert_eq!(studio.problem, None, "it works again");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn work_left_by_a_crash_is_offered_back_under_its_own_file_and_still_unsaved() {
    let dir = temp_dir("restore");
    let recovery_dir = dir.join("recovery");
    let file = dir.join("Mine.ron");
    let mut crashed = Studio::blank();
    assert!({
        crashed.path = Some(file.clone());
        crashed.save(false)
    });
    crashed.start_recovery(&recovery_dir, None);
    crashed.scene.name = "Unsaved work".into();
    crashed.autosave(Instant::now());
    session_file(&crashed);
    // The editor is gone without a clean exit: its lock goes with it.
    drop(crashed.recovery.take());

    let mut studio = Studio::blank();
    studio.start_recovery(&recovery_dir, None);
    let offer = studio.offer.as_ref().expect("offered back");
    assert_eq!(offer.name, "Unsaved work");
    assert_eq!(offer.when, "less than a minute ago");
    studio.restore();
    assert_eq!(studio.scene.name, "Unsaved work");
    assert_eq!(studio.path, Some(file.clone()));
    assert!(studio.unsaved(), "the file still holds the old version");
    assert!(studio.status.starts_with("Restored your work"), "{}", studio.status);
    studio.autosave(Instant::now());
    let own = session_file(&studio);
    assert!(own.exists(), "into this editor's own autosave at once");
    assert_eq!(recovery::orphans(&recovery_dir), vec![], "the old copy is gone, ours is held");

    let mut next = Studio::blank();
    next.start_recovery(&dir.join("elsewhere"), None);
    assert!(next.offer.is_none(), "nothing left behind there");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn autosaved_work_that_cannot_be_read_is_set_aside_and_reported() {
    let dir = temp_dir("broken");
    std::fs::write(dir.join("session-1-1.ron"), "not a scene").unwrap();
    let mut studio = Studio::blank();
    studio.start_recovery(&dir, None);
    assert!(studio.offer.is_none());
    assert_eq!(studio.problem.as_ref().map(|p| p.kind), Some(ProblemKind::Recovery));
    assert!(dir.join("session-1-1.ron.broken").is_file(), "kept, not offered again");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_save_keeps_the_version_before_as_a_backup() {
    let dir = temp_dir("backup");
    let file = dir.join("Mine.ron");
    let mut studio = Studio::blank();
    studio.path = Some(file.clone());
    studio.scene.name = "First".into();
    assert!(studio.save(false));
    studio.scene.name = "Second".into();
    assert!(studio.save(false));
    let backup = crate::files::backup_path(&file);
    assert!(std::fs::read_to_string(backup).unwrap().contains("First"));
    assert!(std::fs::read_to_string(&file).unwrap().contains("Second"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_graphics_card_complaint_is_a_problem_not_a_crash() {
    let mut studio = Studio::headless();
    {
        let mut errors = studio.gpu_errors.lock().unwrap();
        errors.first = Some("Validation Error\n\nCaused by:\n  bad buffer".into());
        errors.count = 3;
    }
    studio.tell_gpu_errors();
    let problem = studio.problem.clone().unwrap();
    assert_eq!(problem.kind, ProblemKind::Graphics);
    assert_eq!(problem.text, "The graphics card reported a problem (3 times): Validation Error");
    studio.gpu_errors.lock().unwrap().count = 5;
    studio.tell_gpu_errors();
    assert_eq!(
        studio.problem.as_ref().unwrap().text,
        "The graphics card reported a problem (5 times): Validation Error",
        "counts up in place"
    );
    studio.problem = None;
    studio.gpu_errors.lock().unwrap().count = 9;
    studio.tell_gpu_errors();
    assert_eq!(studio.problem, None, "a dismissed complaint stays dismissed however often it repeats");
    studio.report(ProblemKind::File, "Couldn't save");
    studio.gpu_errors.lock().unwrap().count = 12;
    studio.tell_gpu_errors();
    assert_eq!(studio.problem.as_ref().unwrap().kind, ProblemKind::File, "never hides another problem");
    studio.gpu_errors.lock().unwrap().lost = true;
    studio.tell_gpu_errors();
    assert!(studio.problem.unwrap().text.starts_with("The graphics card stopped responding"));
}

#[test]
fn an_export_ending_is_told_in_the_status_line() {
    let mut studio = Studio::headless();
    let sender = studio.panels.export.pretend_busy(1, 4);
    sender.send(Err(anyhow::anyhow!("ffmpeg failed: Permission denied"))).unwrap();
    let settings = studio.settings();
    let ctx = egui::Context::default();
    studio.panels.export.show(&ctx, &studio.scene, &settings, None, &studio.kit);
    studio.tell_export_news();
    let problem = studio.problem.clone().unwrap();
    assert_eq!(problem.kind, ProblemKind::Export);
    assert!(problem.text.contains("open in a player"), "{}", problem.text);
}

fn dialogs_harness(studio: Studio) -> Harness<'static, Studio> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 600.0))
        .with_max_steps(20)
        .allow_missing_glyphs()
        .with_context_setup(style::apply)
        .build_ui_state(
            |ui, studio: &mut Studio| {
                egui::Panel::bottom("status").show(ui, |ui| studio.status_line(ui));
                egui::CentralPanel::default().show(ui, |_| {});
                let ctx = ui.ctx().clone();
                studio.stop_export_dialog(&ctx);
            },
            studio,
        );
    h.run();
    h
}

#[test]
fn a_problem_shows_in_the_status_line_until_dismissed() {
    let mut studio = Studio::headless();
    studio.report(ProblemKind::File, "Couldn't save Mine.ron: access denied");
    let mut h = dialogs_harness(studio);
    assert!(h.query_by_label("Couldn't save Mine.ron: access denied").is_some());
    h.get_by_label("Dismiss").click();
    h.run();
    assert_eq!(h.state().problem, None);
    assert!(h.query_by_label("Dismiss").is_none());
}

#[test]
fn closing_while_exporting_asks_first_and_can_keep_exporting() {
    let mut studio = Studio::headless();
    let sender = studio.panels.export.pretend_busy(2, 5);
    studio.ask_stop_export = true;
    let mut h = dialogs_harness(studio);
    assert!(h.query_by_label("A video is still exporting (40%). Stop it and close?").is_some());
    assert!(h.query_by_label("· Exporting 40%").is_some(), "the status line shows the percentage");
    h.get_by_label("Keep exporting").click();
    h.run();
    assert!(!h.state().ask_stop_export);
    assert!(h.state().panels.export.busy(), "still exporting");
    h.state_mut().ask_stop_export = true;
    h.run();
    drop(sender);
    h.get_by_label("Stop and close").click();
    h.run();
    assert!(!h.state().panels.export.busy(), "stopped");
    assert!(h.state().closing, "nothing unsaved, so it closes");
}

#[test]
fn a_held_physics_edit_keeps_the_run_until_let_go_or_rested() {
    let mut studio = Studio::headless();
    let physics = studio.fingerprint;
    let nudge = |studio: &mut Studio| {
        let before = studio.scene.clone();
        studio.scene.things.get_mut(&ThingId(2)).unwrap().transform.position.x += 40.0;
        studio.after_edit(before);
    };
    studio.hold_runs = true;
    let revision = studio.revision;
    nudge(&mut studio);
    assert_eq!(studio.fingerprint, physics, "held: the old run plays on");
    assert!(studio.runs_due.is_some());
    assert!(studio.revision > revision, "the picture is drawn again with the new settings");
    assert!(studio.settle_runs(true).is_some(), "still held and just changed: it waits");
    assert_eq!(studio.fingerprint, physics);
    assert!(studio.settle_runs(false).is_none(), "let go");
    assert_ne!(studio.fingerprint, physics, "the run starts over");
    assert!(studio.runs_due.is_none());
    // Held again, then rested on.
    studio.hold_runs = true;
    let physics = studio.fingerprint;
    nudge(&mut studio);
    assert_eq!(studio.fingerprint, physics);
    studio.runs_due = Some(Instant::now() - RUNS_REST);
    assert!(studio.settle_runs(true).is_none());
    assert_ne!(studio.fingerprint, physics, "a rest starts it over while still held");
    assert!(studio.hold_runs, "and the drag is still held");
}

#[test]
fn a_held_edit_that_adds_or_removes_a_thing_never_waits() {
    let mut studio = Studio::headless();
    studio.hold_runs = true;
    let before = studio.scene.clone();
    bpe_kit::add_thing(&mut studio.scene, &studio.kit.registry, "balls", ThingId::ROOT).unwrap();
    let physics = studio.fingerprint;
    studio.after_edit(before);
    assert!(studio.runs_due.is_none());
    assert_ne!(studio.fingerprint, physics, "the runs have every thing the scene has");
}

#[test]
fn a_held_look_edit_waits_for_nothing() {
    let mut studio = Studio::headless();
    studio.hold_runs = true;
    let before = studio.scene.clone();
    let look = Address::on(ThingId(2), SlotId::Look, "2/strength");
    studio.scene.set_at(&studio.kit.registry, &look, Value::Number(1.0)).unwrap();
    studio.after_edit(before);
    assert!(studio.runs_due.is_none(), "a look edit changes no run");
}

#[test]
fn only_a_quiet_playing_frame_skips_looking_for_an_edit() {
    assert!(quiet_frame(true, false, false, false));
    assert!(!quiet_frame(false, false, false, false), "paused frames always look");
    assert!(!quiet_frame(true, true, false, false), "a click or key may edit");
    assert!(!quiet_frame(true, false, true, false));
    assert!(!quiet_frame(true, false, false, true));
}

#[test]
fn a_look_edit_keeps_the_playback_and_a_gravity_edit_restarts_it() {
    let mut studio = Studio::headless();
    studio.seek(2.0);
    studio.catch_up();
    let before = studio.scene.clone();
    // The test ball's Glow layer's Strength.
    let look = Address::on(ThingId(2), SlotId::Look, "2/strength");
    studio.scene.set_at(&studio.kit.registry, &look, Value::Number(1.0)).unwrap();
    studio.after_edit(before);
    assert!(studio.playback.time() >= 2.0);
    // Quick edits merge into one undo step; the frame loop ends the step once the pointer
    // is up and the edits have stopped. The next edit is a step of its own.
    studio.history.end_merging();
    let before = studio.scene.clone();
    let gravity = Address { thing: ThingId::ROOT, slot: None, key: "gravity".into() };
    studio.scene.set_at(&studio.kit.registry, &gravity, Value::Vec2([0.0, 10.0])).unwrap();
    studio.after_edit(before);
    assert_eq!(studio.playback.time(), 0.0);
    assert_eq!(studio.history.undo.len(), 2);
}

#[test]
fn a_brush_stroke_or_handle_drag_is_its_own_undo_step_however_quick() {
    let mut studio = Studio::headless();
    let look = Address::on(ThingId(2), SlotId::Look, "2/strength");
    let edit = |studio: &mut Studio, v: f64, own: bool| {
        let before = studio.scene.clone();
        studio.scene.set_at(&studio.kit.registry, &look, Value::Number(v)).unwrap();
        studio.panels.inspector.own_step = own;
        studio.after_edit(before);
    };
    edit(&mut studio, 0.5, false);
    edit(&mut studio, 0.6, false);
    assert_eq!(studio.history.undo.len(), 1, "quick edits merge");
    edit(&mut studio, 0.7, true);
    assert_eq!(studio.history.undo.len(), 2, "a stroke starts its own step");
    edit(&mut studio, 0.8, false);
    assert_eq!(studio.history.undo.len(), 2, "and the drag after it joins it");
}

#[test]
fn the_editor_starts_on_the_blank_project_and_new_makes_another() {
    let studio = Studio::blank();
    // A new thing's colour is picked at random, so compare the two with colours left out.
    let uncoloured = |mut scene: Scene| {
        scene.things.values_mut().for_each(|t| t.colour = None);
        scene
    };
    let blank = bpe_kit::new_studio_scene(&studio.kit.registry);
    assert_eq!(uncoloured(studio.scene.clone()), uncoloured(blank));
    assert!(!studio.unsaved());
    let mut studio = Studio::headless();
    studio.new_scene();
    let kinds: Vec<&str> = studio.scene.order.iter().map(|id| studio.scene.things[id].kind.as_str()).collect();
    assert_eq!(kinds, ["scene", "camera"], "New is blank too, with just the camera");
}

#[test]
fn new_scene_and_undo_keep_the_saved_state_honest() {
    let mut studio = Studio::headless();
    assert!(!studio.unsaved());
    let before = studio.scene.clone();
    studio.scene.name = "Changed".into();
    studio.after_edit(before);
    assert!(studio.unsaved());
    assert!(studio.undo());
    assert!(!studio.unsaved());
    assert!(studio.redo());
    assert_eq!(studio.scene.name, "Changed");
    studio.new_scene();
    assert_eq!(studio.scene.name, "New scene");
    assert!(!studio.unsaved());
    assert!(studio.history.undo.is_empty());
}

#[test]
fn scrubbing_a_huge_scene_returns_within_its_budget() {
    let mut studio = Studio::headless();
    let before = studio.scene.clone();
    // As many balls as a group may have.
    let mut block = BallsSettings { radius: 1.5, ..BallsSettings::default() };
    block.block(70, 70, 4.0);
    studio.scene.things.get_mut(&ThingId(2)).unwrap().params = block.to_params();
    studio.after_edit(before);
    let started = Instant::now();
    studio.seek(20.0);
    assert!(started.elapsed() < Duration::from_millis(500), "a scrub blocked for {:?}", started.elapsed());
    assert_eq!(studio.time, 20.0);
    assert!(studio.playback.time() < 20.0, "the preview catches the rest up a budget per frame");
}

/// The editor's panels as `ui` lays them out, with its keyboard shortcuts before and after
/// them and each frame's change recorded as it records them.
fn editor_harness(studio: Studio) -> Harness<'static, Studio> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(120)
        .allow_missing_glyphs()
        .with_context_setup(style::apply)
        .build_ui_state(
            |ui, studio: &mut Studio| {
                let ctx = ui.ctx().clone();
                let before = studio.scene.clone();
                studio.recorded = false;
                studio.shortcuts(&ctx);
                egui::Panel::bottom("timeline").show(ui, |ui| timeline::show(ui, studio));
                studio.left_panel(ui, &before);
                egui::CentralPanel::default().show(ui, |_| {});
                studio.late_shortcuts(&ctx);
                studio.take_drops(&ctx);
                studio.record_frame(before);
            },
            studio,
        );
    h.run();
    h
}

fn thing_named(studio: &Studio, name: &str) -> Option<ThingId> {
    studio.scene.things.values().find(|t| t.name == name).map(|t| t.id)
}

#[test]
fn ctrl_d_duplicates_the_selection_as_its_own_undo_step() {
    let mut studio = Studio::headless();
    studio.selection.set(ThingId(2));
    let mut h = editor_harness(studio);
    h.key_press_modifiers(Modifiers::COMMAND, Key::D);
    h.run();
    let copy = thing_named(h.state(), "Ball copy").expect("a copy");
    assert_eq!(h.state().selection.primary, Some(copy), "the copy is selected");
    assert!(h.state_mut().undo());
    assert!(thing_named(h.state(), "Ball copy").is_none());
}

#[test]
fn copied_things_paste_into_a_new_scene() {
    let mut studio = Studio::headless();
    studio.selection.set(ThingId(2));
    let mut h = editor_harness(studio);
    h.key_press_modifiers(Modifiers::COMMAND, Key::C);
    h.run();
    assert_eq!(h.state().status, "Copied Ball.");
    h.state_mut().new_scene();
    h.run();
    assert!(thing_named(h.state(), "Ball").is_none());
    h.key_press_modifiers(Modifiers::COMMAND, Key::V);
    h.run();
    let ball = thing_named(h.state(), "Ball").expect("pasted");
    assert_eq!(h.state().selection.primary, Some(ball));
    assert_eq!(h.state().scene.things[&ball].parent, Some(ThingId::ROOT));
    assert!(h.state().status.starts_with("Pasted Ball."), "{}", h.state().status);
    assert!(h.state().unsaved());
    // The row menu pastes too, beside the row picked.
    let rows = h.state().scene.things.len();
    h.get_by_label("Ball").click_secondary();
    h.run();
    h.get_by_label("Paste Ball Ctrl+V").click();
    h.run();
    assert_eq!(h.state().scene.things.len(), rows + 1);
    assert!(thing_named(h.state(), "Ball copy").is_some());
}

/// A file dropped in a test: its name and bytes.
#[derive(Debug)]
struct TestDrop(PathBuf, Vec<u8>);

impl egui::DroppedFile for TestDrop {
    fn path(&self) -> &Path {
        &self.0
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        Ok(self.1.clone())
    }
}

fn drop_file(h: &mut Harness<'_, Studio>, name: &str, bytes: &[u8]) {
    let file = Arc::new(TestDrop(PathBuf::from(name), bytes.to_vec()));
    h.input_mut().dropped_files.push(file);
    h.run();
}

#[test]
fn a_dropped_png_becomes_a_particles_picture_kept_in_the_scene() {
    let mut h = editor_harness(Studio::headless());
    let png = b"\x89PNG\r\n\x1a\nrest";
    drop_file(&mut h, "Logo.png", png);
    let s = h.state();
    assert_eq!(&*s.scene.pictures["Logo"].bytes, &png[..]);
    let id = s.selection.primary.expect("a new Particles thing, selected");
    let thing = &s.scene.things[&id];
    assert_eq!(thing.kind, "particles");
    assert_eq!(thing.params.get("picture"), Some(&Value::Text("Logo".into())));
    assert_eq!(thing.params.get("word_from"), Some(&Value::Choice("picture".into())));
    let things = s.scene.things.len();
    // Dropped again with it selected, the same thing shows it: no second thing, no second copy.
    drop_file(&mut h, "Logo.png", png);
    assert_eq!(h.state().scene.things.len(), things);
    assert_eq!(h.state().scene.pictures.len(), 1);
    assert!(h.state_mut().undo(), "a drop is an undo step");
}

#[test]
fn a_dropped_font_is_kept_and_sets_the_selected_text_in_it() {
    let mut studio = Studio::headless();
    let kit = studio.kit.clone();
    let text = bpe_kit::add_thing(&mut studio.scene, &kit.registry, bpe_kit::things::text::KIND, ThingId::ROOT);
    let text = text.unwrap();
    studio.selection.set(text);
    let mut h = editor_harness(studio);
    let bytes = std::fs::read(bpe_kit::text::fonts::repo_fonts_dir().join("BebasNeue.ttf")).unwrap();
    drop_file(&mut h, "BebasNeue.ttf", &bytes);
    assert!(h.state().scene.fonts.contains_key("Bebas Neue"));
    assert_eq!(h.state().scene.things[&text].params.get("font"), Some(&Value::Text("Bebas Neue".into())));
}

#[test]
fn a_dropped_file_ball_studio_cant_use_is_reported() {
    let mut h = editor_harness(Studio::headless());
    let before = h.state().scene.clone();
    drop_file(&mut h, "notes.txt", b"hello");
    assert_eq!(h.state().scene, before);
    let problem = h.state().problem.as_ref().map(|p| (p.kind, p.text.clone()));
    assert_eq!(problem, Some((ProblemKind::File, crate::drops::cant_open("notes.txt"))));
    drop_file(&mut h, "broken.wav", b"RIFF");
    assert_eq!(h.state().scene, before, "a sound file that can't be read is refused");
    assert!(h.state().problem.as_ref().is_some_and(|p| p.text.contains("broken.wav")));
    drop_file(&mut h, "fake.png", b"GIF89a");
    assert_eq!(h.state().scene, before, "a picture that isn't a PNG is refused");
    assert!(h.state().problem.as_ref().is_some_and(|p| p.text.contains("fake.png")));
}

#[test]
fn a_dropped_song_becomes_the_backing_track() {
    let mut h = editor_harness(Studio::headless());
    let kit = h.state().kit.clone();
    drop_file(&mut h, "My song.wav", &bpe_sound::audio::sine_wav(220.0, 0.2, 44_100, 2));
    assert!(h.state().scene.audio.contains_key("My song"));
    assert_eq!(crate::mixer::backing::backing_name(&h.state().scene, &kit), "My song");
    assert!(h.state().status.contains("backing track"), "{}", h.state().status);
}

#[test]
fn a_scene_with_only_its_camera_counts_as_empty() {
    let studio = Studio::headless();
    assert!(!crate::preview::is_empty_scene(&studio.scene), "the headless scene has a ball");
    let kit = bpe_kit::kit();
    let scene = bpe_kit::new_studio_scene(&kit.registry);
    assert!(crate::preview::is_empty_scene(&scene), "the blank project the editor opens to is");
}

#[test]
fn delete_works_from_the_mixer_tab() {
    let mut studio = Studio::headless();
    studio.selection.set(ThingId(2));
    studio.left = LeftPanel::Mixer;
    let mut h = editor_harness(studio);
    h.key_press(Key::Delete);
    h.run();
    assert!(!h.state().scene.things.contains_key(&ThingId(2)));
    assert!(h.state_mut().undo(), "its own undo step");
    assert!(h.state().scene.things.contains_key(&ThingId(2)));
}

#[test]
fn arrows_step_a_frame_or_a_beat_and_end_goes_to_the_end() {
    let mut studio = Studio::headless();
    studio.seek(1.0);
    let mut h = editor_harness(studio);
    h.key_press(Key::ArrowRight);
    h.run();
    assert!((h.state().time - (1.0 + 1.0 / 60.0)).abs() < 1e-9, "{}", h.state().time);
    h.key_press(Key::ArrowLeft);
    h.key_press(Key::ArrowLeft);
    h.run();
    assert!((h.state().time - (1.0 - 1.0 / 60.0)).abs() < 1e-9, "{}", h.state().time);
    // A beat is half a second at 120 BPM: from the beat it is nearest.
    h.key_press_modifiers(Modifiers::SHIFT, Key::ArrowRight);
    h.run();
    assert!((h.state().time - 1.5).abs() < 1e-9, "{}", h.state().time);
    h.key_press(Key::End);
    h.run();
    assert_eq!(h.state().time, 20.0);
    h.key_press(Key::Home);
    h.run();
    assert_eq!(h.state().time, 0.0);
}

#[test]
fn ctrl_c_and_ctrl_v_copy_the_timelines_keys_when_some_are_picked() {
    let mut studio = Studio::headless();
    let size = Address::own(ThingId(2), "radius");
    studio.scene.set_key(&studio.kit.registry, &size, 2.0, Value::Number(60.0)).unwrap();
    studio.selection.set(ThingId(2));
    let mut h = editor_harness(studio);
    h.get_by_label("Key at 0:02.00 of Ball · Size").click();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::C);
    h.run();
    assert!(matches!(h.state().clipboard, crate::things::Clipboard::Keys(_)));
    h.state_mut().seek(6.0);
    h.key_press_modifiers(Modifiers::COMMAND, Key::V);
    h.run();
    let times: Vec<f64> = h.state().scene.tracks[&size].keys.iter().map(|k| k.time).collect();
    assert_eq!(times, [2.0, 6.0]);
    // Picking a thing in the tree lets go of the keys: Ctrl+C copies the thing.
    h.get_by_label("Ball").click();
    h.run();
    assert!(h.state().panels.timeline.selected.is_empty());
    h.key_press_modifiers(Modifiers::COMMAND, Key::C);
    h.run();
    assert!(matches!(h.state().clipboard, crate::things::Clipboard::Things(_)));
}

#[test]
fn the_window_title_names_the_scene_and_marks_unsaved_changes() {
    let mut studio = Studio::headless();
    studio.scene.name = "Drop".into();
    studio.saved = studio.scene.clone();
    assert_eq!(studio.window_title(), "Drop – Ball Studio");
    // Each edit moves the revision on, as recording a frame's edit does.
    studio.scene.name = "Drop 2".into();
    studio.revision += 1;
    assert_eq!(studio.window_title(), "Drop 2 • – Ball Studio");
    studio.scene.name = "  ".into();
    studio.revision += 1;
    assert_eq!(studio.window_title(), "Untitled • – Ball Studio");
}

#[test]
fn d_switches_build_and_grade_and_grade_mode_keys_compare() {
    use crate::grade_mode::{Compare, EditorMode};
    let mut h = Harness::builder()
        .with_size(egui::vec2(400.0, 200.0))
        .allow_missing_glyphs()
        .with_context_setup(style::apply)
        .build_ui_state(|ui, s: &mut Studio| s.shortcuts(&ui.ctx().clone()), Studio::headless());
    h.run();
    assert_eq!(h.state().mode, EditorMode::Build);
    // In Build mode \ and Y do nothing to the grade view.
    h.key_press(egui::Key::Backslash);
    h.run();
    assert_eq!(h.state().grade.compare, Compare::After);
    h.key_press(egui::Key::D);
    h.run();
    assert_eq!(h.state().mode, EditorMode::Grade);
    h.key_press(egui::Key::Backslash);
    h.run();
    assert_eq!(h.state().grade.compare, Compare::Before);
    h.key_press(egui::Key::Y);
    h.run();
    assert_eq!(h.state().grade.compare, Compare::Split);
    // J shows the clipping (and again hides it); Esc stops the targeted tool.
    h.key_press(egui::Key::J);
    h.run();
    assert!(h.state().grade.clipping);
    assert_eq!(h.state().grade.clip_view(), bpe_gfx::grade::ClipView::Marks);
    h.key_press(egui::Key::J);
    h.run();
    assert!(!h.state().grade.clipping);
    h.state_mut().grade.targeted = Some(bpe_core::LayerId(2));
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(h.state().grade.targeted, None);
    h.key_press(egui::Key::D);
    h.run();
    assert_eq!(h.state().mode, EditorMode::Build);
}

#[test]
fn the_toolbar_switches_to_grade_mode() {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1500.0, 80.0))
        .allow_missing_glyphs()
        .with_context_setup(style::apply)
        .build_ui_state(|ui, s: &mut Studio| s.toolbar(ui), Studio::headless());
    h.run();
    h.get_by_label("Grade").click();
    h.run();
    assert_eq!(h.state().mode, crate::grade_mode::EditorMode::Grade);
    h.get_by_label("Build").click();
    h.run();
    assert_eq!(h.state().mode, crate::grade_mode::EditorMode::Build);
}

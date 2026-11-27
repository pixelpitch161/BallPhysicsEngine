//! The Water stage shell: the view, the rail, placing and moving.

use super::course_view::CourseView;
use super::rail::Tool;
use super::stage;
use super::stage::rooms_for_tests;
use crate::app::{Centre, Studio};
use crate::style;
use bpe_kit::things::water;
use eframe::egui::{self, Pos2, Rect, accesskit::Role, vec2};
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use glam::Vec2;

const SIZE: egui::Vec2 = egui::Vec2::new(700.0, 900.0);

/// The stage alone, recording each frame's edit as the editor's frame does.
pub(super) fn harness<'a>(studio: Studio) -> Harness<'a, Studio> {
    let mut h = Harness::builder()
        .with_size(SIZE)
        .with_step_dt(1.0 / 60.0)
        .allow_missing_glyphs()
        .with_context_setup(style::apply)
        .build_ui_state(
            |ui, studio: &mut Studio| {
                let before = studio.scene.clone();
                egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| stage(ui, studio));
                studio.record_frame(before);
            },
            studio,
        );
    h.run();
    h
}

pub(super) fn water_studio() -> Studio {
    let mut studio = Studio::headless_water();
    studio.grid.snap = false;
    studio
}

pub(super) fn view(h: &Harness<Studio>) -> CourseView {
    h.state().water_stage.view.expect("the stage was drawn")
}

pub(super) fn of_kind(studio: &Studio, kind: &str) -> Vec<bpe_core::ThingId> {
    studio.scene.order.iter().copied().filter(|id| studio.scene.things[id].kind == kind).collect()
}

pub(super) fn button(h: &mut Harness<Studio>, at: Pos2, pressed: bool) {
    let button = egui::PointerButton::Primary;
    h.event(egui::Event::PointerButton { pos: at, button, pressed, modifiers: egui::Modifiers::NONE });
    h.step();
}

pub(super) fn click_at(h: &mut Harness<Studio>, at: Pos2) {
    h.hover_at(at);
    h.step();
    button(h, at, true);
    button(h, at, false);
    h.run();
}

/// Presses at `from`, moves to `to` in a few steps and lets go.
pub(super) fn drag(h: &mut Harness<Studio>, from: Pos2, to: Pos2) {
    h.hover_at(from);
    h.step();
    button(h, from, true);
    for k in 1..=8 {
        h.event(egui::Event::PointerMoved(from + (to - from) * (k as f32 / 8.0)));
        h.step();
    }
    button(h, to, false);
    h.run();
}

#[test]
fn the_view_fits_the_course_and_maps_both_ways() {
    let room = Rect::from_min_size(Pos2::ZERO, vec2(832.0, 1032.0));
    let v = CourseView::fit(room, Vec2::new(1080.0, 1920.0));
    assert!((v.rect.height() - 1000.0).abs() < 0.01 && (v.rect.width() - 562.5).abs() < 0.01, "{:?}", v.rect);
    assert_eq!(v.to_screen(Vec2::ZERO), v.rect.center());
    assert_eq!(v.rect.center(), room.center());
    let p = Vec2::new(-300.0, 500.0);
    assert!((v.to_world(v.to_screen(p)) - p).length() < 1e-3);
    // A wide course (two screens wide, one tall) in the same room is held by its width instead.
    let wide = CourseView::fit(room, Vec2::new(2160.0, 1920.0));
    assert!((wide.rect.width() - 800.0).abs() < 0.01 && wide.rect.height() < 1000.0, "{:?}", wide.rect);
    assert!(wide.holds(wide.to_screen(Vec2::new(1079.0, 0.0))) && !wide.holds(wide.to_screen(Vec2::new(1100.0, 0.0))));
}

#[test]
fn the_shot_view_fits_the_shot_and_maps_both_ways() {
    let room = Rect::from_min_size(Pos2::ZERO, vec2(832.0, 1032.0));
    let course = Vec2::new(2160.0, 1920.0);
    // A half-size shot (Zoom 2) round a point right of the middle.
    let (centre, shot) = (Vec2::new(400.0, -200.0), Vec2::new(540.0, 960.0));
    let v = CourseView::fit_shot(room, course, centre, shot);
    let seen = v.seen_rect();
    assert!((seen.height() - 1000.0).abs() < 0.01 && (seen.width() - 562.5).abs() < 0.01, "{seen:?}");
    assert!((seen.center() - room.center()).length() < 1e-3);
    assert!((v.to_screen(centre) - room.center()).length() < 1e-3);
    assert_eq!(v.seen(), (centre - shot / 2.0, shot));
    let p = Vec2::new(500.0, 100.0);
    assert!((v.to_world(v.to_screen(p)) - p).length() < 1e-3);
    // Only the shot takes clicks, and drawing stays inside it.
    assert!(v.holds(v.to_screen(centre)) && !v.holds(v.to_screen(Vec2::new(-400.0, 0.0))));
    assert!(v.clip(0.0) == seen);
    // The whole course shows all of it.
    let whole = CourseView::fit(room, course);
    assert_eq!((whole.seen(), whole.seen_rect()), ((-course / 2.0, course), whole.rect));
}

#[test]
fn the_shot_view_shows_the_finish_and_the_whole_course_none() {
    use super::stage::Seen;
    use bpe_core::{SlotId, ThingId};
    use bpe_kit::modules::water::SCREEN;
    let mut studio = water_studio();
    let kit = studio.kit.clone();
    studio.scene.add_slot(&kit.registry, ThingId::ROOT, SlotId::Shader, bpe_kit::modules::effects::EFFECTS).unwrap();
    studio.scene.add_layer(&kit.registry, ThingId::ROOT, SlotId::Shader, "picture.mirror").unwrap();
    studio.scene_changed();
    let mut h = harness(studio);
    h.run();
    let course = h.state().water_stage.view.expect("drawn");
    assert!(course.shot.is_none(), "the whole course to start with");
    h.state_mut().water_stage.seen = Seen::Shot;
    h.run();
    let studio = h.state();
    assert_eq!(studio.water_stage.seen, Seen::Shot);
    let shot = studio.water_stage.view.expect("drawn");
    let (lo, size) = shot.seen();
    assert_eq!((lo, size), (-SCREEN / 2.0, SCREEN), "the camera's shot: one screen round the middle");
    assert_eq!(super::stage::look_now(studio, &shot).finish.effects.len(), 1, "Mirror on the shot");
    assert!(super::stage::look_now(studio, &course).finish.effects.is_empty(), "none on the whole course");
}

#[test]
fn in_the_shot_view_select_takes_clicks_only_on_the_shot() {
    use super::stage::Seen;
    use bpe_core::{Address, SlotId, ThingId, Value};
    let mut studio = water_studio();
    let kit = studio.kit.clone();
    // Two screens wide: the camera's shot is the middle screen, and the course reaches past it.
    let wide = Address::on(ThingId::ROOT, SlotId::Water, "screens_wide");
    studio.scene.set_at(&kit.registry, &wide, Value::Int(2)).unwrap();
    let beside = bpe_kit::add_thing(&mut studio.scene, &kit.registry, water::POUR, ThingId::ROOT).unwrap();
    studio.scene.things.get_mut(&beside).unwrap().transform.position = Vec2::new(600.0, 0.0);
    let inside = bpe_kit::add_thing(&mut studio.scene, &kit.registry, water::POUR, ThingId::ROOT).unwrap();
    studio.scene.things.get_mut(&inside).unwrap().transform.position = Vec2::new(200.0, 0.0);
    studio.scene_changed();
    studio.water_stage.seen = Seen::Shot;
    let mut h = harness(studio);
    h.run();
    let v = view(&h);
    let (room, at) = (rooms_for_tests(Rect::from_min_size(Pos2::ZERO, SIZE)).2, v.to_screen(Vec2::new(600.0, 0.0)));
    assert!(room.contains(at) && !v.seen_rect().contains(at), "the Pour beside the shot is in the room, off the shot");
    click_at(&mut h, at);
    assert_eq!(h.state().selection.primary, None, "a marker beside the shot isn't picked");
    click_at(&mut h, v.to_screen(Vec2::new(200.0, 0.0)));
    assert_eq!(h.state().selection.primary, Some(inside), "one on the shot is");
}

#[test]
fn the_stage_follows_the_courses_size_on_the_water_card() {
    let mut studio = water_studio();
    let before = studio.scene.clone();
    let wide = bpe_core::Address::on(bpe_core::ThingId::ROOT, bpe_core::SlotId::Water, "screens_wide");
    let kit = studio.kit.clone();
    studio.scene.set_at(&kit.registry, &wide, bpe_core::Value::Int(2)).unwrap();
    studio.after_edit(before);
    let h = harness(studio);
    assert_eq!(view(&h).size, Vec2::new(2160.0, 1920.0));
    let (_, _, room) = rooms_for_tests(Rect::from_min_size(Pos2::ZERO, SIZE));
    assert!(room.contains_rect(view(&h).rect), "the whole course is in the room");
}

#[test]
fn the_rail_has_the_nine_tools_and_only_the_built_ones_work() {
    let mut h = harness(water_studio());
    let mut last = f32::MIN;
    for tool in Tool::ALL {
        let node = h.get_by_role_and_label(Role::Button, tool.name());
        assert!(node.rect().min.y > last, "{} is below the tool before it", tool.name());
        last = node.rect().min.y;
        assert_eq!(node.accesskit_node().is_disabled(), !tool.ready(), "{}", tool.name());
    }
    let disabled: Vec<&str> = Tool::ALL.into_iter().filter(|t| !t.ready()).map(Tool::name).collect();
    assert_eq!(disabled, ["Camera path"]);
    // The strip says what the tool in hand does.
    assert!(h.query_by_label(Tool::Select.hint()).is_some());
    h.get_by_role_and_label(Role::Button, "Pour").click();
    h.run();
    assert_eq!(h.state().water_stage.tool, Tool::Pour);
    assert!(h.query_by_label(Tool::Pour.hint()).is_some());
    // A tool that is not built yet stays out of hand.
    h.get_by_role_and_label(Role::Button, "Camera path").click();
    h.run();
    assert_eq!(h.state().water_stage.tool, Tool::Pour);
}

#[test]
fn the_pour_tool_adds_a_pour_where_the_stage_is_clicked_and_select_moves_it() {
    let mut h = harness(water_studio());
    h.state_mut().water_stage.tool = Tool::Pour;
    h.run();
    let v = view(&h);
    let at = Vec2::new(120.0, -400.0);
    click_at(&mut h, v.to_screen(at));
    let pours = of_kind(h.state(), water::POUR);
    assert_eq!(pours.len(), 1, "one Pour");
    let pour = pours[0];
    let place = |h: &Harness<Studio>| h.state().scene.things[&pour].transform.position;
    assert!((place(&h) - at).length() < 0.6, "at the click: {}", place(&h));
    assert_eq!(h.state().selection.primary, Some(pour), "the new Pour is selected");
    assert_eq!(h.state().water_stage.tool, Tool::Select, "back to Select");
    let placed = place(&h);

    // Select drags it by its marker.
    let from = v.to_screen(placed);
    drag(&mut h, from, from + vec2(50.0, 30.0));
    let moved = place(&h) - placed;
    let want = Vec2::new(50.0, 30.0) / v.scale();
    assert!((moved - want).length() < 1.0, "moved {moved}, wanted {want}");
    // One undo puts it back where it was placed; the next takes the Pour away.
    assert!(h.state_mut().undo());
    assert_eq!(place(&h), placed, "the drag is one undo step");
    assert!(h.state_mut().undo());
    assert!(of_kind(h.state(), water::POUR).is_empty(), "placing is its own undo step");
}

#[test]
fn a_click_outside_the_course_adds_nothing_and_a_click_on_nothing_lets_go() {
    let mut h = harness(water_studio());
    h.state_mut().water_stage.tool = Tool::Drain;
    h.run();
    let v = view(&h);
    // In the room beside the course.
    let beside = Pos2::new(v.rect.left() - 8.0, v.rect.center().y);
    click_at(&mut h, beside);
    assert!(of_kind(h.state(), water::DRAIN).is_empty());
    assert_eq!(h.state().water_stage.tool, Tool::Drain, "the tool stays in hand");
    click_at(&mut h, v.rect.center());
    let drain = of_kind(h.state(), water::DRAIN)[0];
    assert_eq!(h.state().selection.primary, Some(drain));
    // With Select, a click on the empty course lets go of it, and a click on its marker picks it.
    click_at(&mut h, v.rect.center() + vec2(0.0, 200.0));
    assert_eq!(h.state().selection.primary, None);
    click_at(&mut h, v.rect.center() + vec2(3.0, -2.0));
    assert_eq!(h.state().selection.primary, Some(drain));
    assert_eq!(of_kind(h.state(), water::DRAIN).len(), 1, "Select adds nothing");
}

#[test]
fn a_drag_lands_on_the_grid_when_snap_is_on() {
    let mut studio = Studio::headless_water();
    studio.grid.snap = true;
    let spacing = studio.grid.spacing;
    let mut h = harness(studio);
    h.state_mut().water_stage.tool = Tool::Lamp;
    h.run();
    let v = view(&h);
    click_at(&mut h, v.to_screen(Vec2::new(spacing * 2.3, spacing * -3.4)));
    let lamp = of_kind(h.state(), water::LAMP)[0];
    assert_eq!(h.state().scene.things[&lamp].transform.position, Vec2::new(spacing * 2.0, spacing * -3.0));
}

#[test]
fn the_stage_shows_in_a_water_scene_and_the_ball_preview_does_not() {
    assert_eq!(Studio::headless_water().centre_shows(), Centre::Water);
    assert_eq!(Studio::headless().centre_shows(), Centre::Preview);
    let mut video = Studio::headless_water();
    video.mode = crate::grade_mode::EditorMode::Video;
    assert_eq!(video.centre_shows(), Centre::Video);
    // Opening another scene puts the tool back to Select.
    let mut studio = Studio::headless_water();
    studio.water_stage.tool = Tool::Force;
    let scene = bpe_kit::new_scene(&studio.kit.registry);
    studio.replace_scene(scene, None);
    assert_eq!(studio.water_stage.tool, Tool::Select);
}

#[test]
fn a_course_with_nothing_carved_says_how_to_start() {
    let mut h = harness(water_studio());
    assert!(h.query_by_label(stage::ALL_ROCK).is_some(), "the course is all rock");
    // Words over the course don't stop a click placing a pour.
    h.state_mut().water_stage.tool = Tool::Pour;
    h.run();
    let middle = view(&h).rect.center();
    click_at(&mut h, middle);
    assert_eq!(of_kind(h.state(), water::POUR).len(), 1);
    let course = water::course_of(&h.state().scene).unwrap();
    let dot = bpe_core::Stroke { points: vec![[0.0, 0.0]], size: 50.0, strength: 1.0, ..Default::default() };
    let card = h.state_mut().scene.things.get_mut(&course).unwrap().slots.get_mut(&bpe_core::SlotId::Form).unwrap();
    card.params.set("carved", bpe_core::Value::Strokes(bpe_core::Strokes { list: vec![dot] }));
    h.run();
    assert!(h.query_by_label(stage::ALL_ROCK).is_none(), "something is carved");
}

fn carving(studio: &Studio) -> Vec<bpe_core::Stroke> {
    water::carving_of(&studio.scene).list
}

fn carve_studio<'a>() -> Harness<'a, Studio> {
    let mut h = harness(water_studio());
    h.get_by_role_and_label(Role::Button, "Carve").click();
    h.run();
    assert_eq!(h.state().water_stage.tool, Tool::Carve);
    h
}

#[test]
fn carve_is_ready_on_the_rail() {
    assert!(Tool::Carve.ready());
    let h = carve_studio();
    assert!(h.query_by_label("Brush size").is_some() && h.query_by_label("Hardness").is_some());
}

#[test]
fn carving_paints_one_stroke_as_one_undo_step() {
    let mut h = carve_studio();
    let v = view(&h);
    let (from, to) = (v.to_screen(Vec2::new(-200.0, 0.0)), v.to_screen(Vec2::new(200.0, 100.0)));
    drag(&mut h, from, to);
    let strokes = carving(h.state());
    assert_eq!(strokes.len(), 1);
    let s = &strokes[0];
    assert!(Vec2::from(s.points[0]).distance(v.to_world(from)) < 1.0, "{:?}", s.points[0]);
    assert!(s.points.len() > 2, "{:?}", s.points);
    assert_eq!((s.size, s.erase, s.square, s.softness), (60.0, false, false, 0.0));
    assert!(h.query_by_label(stage::ALL_ROCK).is_none());
    assert!(h.state_mut().undo());
    h.run();
    assert!(carving(h.state()).is_empty());
    assert!(h.state_mut().redo());
    assert_eq!(carving(h.state()).len(), 1);
}

#[test]
fn an_undo_while_painting_is_kept_when_the_stroke_is_let_go() {
    let mut h = carve_studio();
    let v = view(&h);
    drag(&mut h, v.to_screen(Vec2::new(-200.0, 0.0)), v.to_screen(Vec2::new(200.0, 0.0)));
    assert_eq!(carving(h.state()).len(), 1);
    let (from, to) = (v.to_screen(Vec2::new(-200.0, 300.0)), v.to_screen(Vec2::new(200.0, 300.0)));
    h.hover_at(from);
    h.step();
    button(&mut h, from, true);
    h.event(egui::Event::PointerMoved(from + (to - from) * 0.5));
    h.step();
    assert!(h.state_mut().undo(), "the first stroke is undone mid-stroke");
    h.event(egui::Event::PointerMoved(to));
    h.step();
    button(&mut h, to, false);
    h.run();
    let strokes = carving(h.state());
    assert_eq!(strokes.len(), 1, "only the stroke just painted");
    assert!((strokes[0].points[0][1] - 300.0).abs() < 1.0, "{:?}", strokes[0].points);
}

#[test]
fn alt_fills_and_the_strip_sets_the_brush() {
    let mut h = carve_studio();
    h.get_by_role_and_label(Role::Button, "Fill").click();
    h.get_by_role_and_label(Role::Button, "Square").click();
    h.run();
    h.state_mut().water_stage.brush.size = 40.0;
    h.run();
    let v = view(&h);
    drag(&mut h, v.to_screen(Vec2::new(0.0, 0.0)), v.to_screen(Vec2::new(100.0, 0.0)));
    let s = carving(h.state()).pop().unwrap();
    assert_eq!((s.erase, s.square, s.size), (true, true, 20.0));
    // Alt does the other one.
    let (from, to) = (v.to_screen(Vec2::new(0.0, 200.0)), v.to_screen(Vec2::new(100.0, 200.0)));
    h.hover_at(from);
    h.step();
    let alt = egui::Modifiers::ALT;
    let button = egui::PointerButton::Primary;
    h.event(egui::Event::ModifiersChanged(alt));
    h.event(egui::Event::PointerButton { pos: from, button, pressed: true, modifiers: alt });
    h.step();
    for k in 1..=6 {
        h.event(egui::Event::PointerMoved(from + (to - from) * (k as f32 / 6.0)));
        h.step();
    }
    h.event(egui::Event::PointerButton { pos: to, button, pressed: false, modifiers: alt });
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    h.run();
    let s = carving(h.state()).pop().unwrap();
    assert!(!s.erase, "Alt with Fill on digs");
}

#[test]
fn a_straight_line_keeps_two_points() {
    let mut h = carve_studio();
    h.get_by_role_and_label(Role::Button, "Straight").click();
    h.run();
    let v = view(&h);
    let (from, to) = (v.to_screen(Vec2::new(-300.0, -300.0)), v.to_screen(Vec2::new(250.0, 400.0)));
    drag(&mut h, from, to);
    let s = carving(h.state()).pop().unwrap();
    assert_eq!(s.points.len(), 2, "{:?}", s.points);
    assert!(Vec2::from(s.points[1]).distance(v.to_world(to)) < 1.0);
}

#[test]
fn smooth_lags_behind_the_pointer() {
    let mut h = carve_studio();
    h.get_by_role_and_label(Role::Button, "Smooth").click();
    h.run();
    let v = view(&h);
    let (from, to) = (v.to_screen(Vec2::new(-300.0, 0.0)), v.to_screen(Vec2::new(300.0, 0.0)));
    drag(&mut h, from, to);
    let s = carving(h.state()).pop().unwrap();
    let end = Vec2::from(*s.points.last().unwrap());
    let string = 30.0;
    let gap = end.distance(v.to_world(to));
    assert!(gap > 1.0 && gap <= string + 0.5, "{gap}");
}

#[test]
fn escape_drops_the_stroke() {
    let mut h = carve_studio();
    let v = view(&h);
    let (from, to) = (v.to_screen(Vec2::new(0.0, 0.0)), v.to_screen(Vec2::new(200.0, 0.0)));
    h.hover_at(from);
    h.step();
    button(&mut h, from, true);
    for k in 1..=4 {
        h.event(egui::Event::PointerMoved(from + (to - from) * (k as f32 / 4.0)));
        h.step();
    }
    assert!(super::carve_tool::stroke_so_far(h.state()).is_some());
    h.key_press(egui::Key::Escape);
    h.step();
    button(&mut h, to, false);
    h.run();
    assert!(carving(h.state()).is_empty());
    assert!(!h.state_mut().undo(), "nothing to undo");
}

#[test]
fn a_course_without_a_carving_card_gets_one() {
    let mut studio = water_studio();
    let course = water::course_of(&studio.scene).unwrap();
    studio.scene.things.get_mut(&course).unwrap().slots.remove(&bpe_core::SlotId::Form);
    studio.scene_changed();
    let mut h = harness(studio);
    h.state_mut().water_stage.tool = Tool::Carve;
    h.run();
    let v = view(&h);
    drag(&mut h, v.to_screen(Vec2::new(0.0, 0.0)), v.to_screen(Vec2::new(100.0, 0.0)));
    assert_eq!(carving(h.state()).len(), 1);
    assert!(h.state_mut().undo());
    h.run();
    let back = &h.state().scene.things[&course];
    assert!(!back.slots.contains_key(&bpe_core::SlotId::Form), "one undo takes the card and the stroke away");
}

#[test]
fn placing_a_pour_without_a_liquid_adds_water() {
    let mut h = harness(water_studio());
    h.state_mut().water_stage.tool = Tool::Pour;
    h.run();
    let v = view(&h);
    click_at(&mut h, v.to_screen(Vec2::new(0.0, -300.0)));
    let liquids = of_kind(h.state(), water::LIQUID);
    assert_eq!((of_kind(h.state(), water::POUR).len(), liquids.len()), (1, 1));
    assert_eq!(h.state().scene.things[&liquids[0]].name, "Water");
    assert!(h.state().water_draft.setup.as_ref().unwrap().pours[0].liquid == Some(0), "it pours the Water");
    // A second pour brings no second Liquid.
    h.state_mut().water_stage.tool = Tool::Pour;
    h.run();
    click_at(&mut h, v.to_screen(Vec2::new(100.0, -300.0)));
    assert_eq!((of_kind(h.state(), water::POUR).len(), of_kind(h.state(), water::LIQUID).len()), (2, 1));
    // One undo takes the first pour and its Liquid away together.
    assert!(h.state_mut().undo());
    assert!(h.state_mut().undo());
    h.run();
    assert!(of_kind(h.state(), water::POUR).is_empty() && of_kind(h.state(), water::LIQUID).is_empty());
}

#[test]
fn a_picked_pour_says_what_it_cannot_do_yet() {
    use bpe_kit::things::water::draft::LATER_KINDS;
    let mut h = harness(water_studio());
    h.state_mut().water_stage.tool = Tool::Pour;
    h.run();
    let v = view(&h);
    click_at(&mut h, v.to_screen(Vec2::new(0.0, -300.0)));
    let pour = of_kind(h.state(), water::POUR)[0];
    assert!(h.query_by_label(LATER_KINDS).is_none(), "a tap pours");
    let studio = h.state_mut();
    let card = studio.scene.things.get_mut(&pour).unwrap().slots.get_mut(&bpe_core::SlotId::Main).unwrap();
    card.params.set("kind", bpe_core::Value::Choice("rain".into()));
    studio.scene_changed();
    studio.selection.set(pour);
    h.run();
    assert!(h.query_by_label(LATER_KINDS).is_some());
}

#[test]
fn use_a_look_sets_the_scene_to_it_as_one_undo_step() {
    use bpe_core::{Settings, SlotId, ThingId};
    use bpe_kit::modules::water::{ReadyLook, WaterLook};
    let mut studio = water_studio();
    let kit = studio.kit.clone();
    let liquid = bpe_kit::add_thing(&mut studio.scene, &kit.registry, water::LIQUID, ThingId::ROOT).unwrap();
    studio.scene_changed();
    let mut h = harness(studio);
    let before = h.state().scene.clone();
    h.get_by_label("Use a look…").click();
    h.run();
    for look in ReadyLook::ALL {
        assert!(h.query_all_by_label(look.title()).next().is_some(), "{} is in the menu", look.title());
    }
    assert!(h.query_all_by_label("Keep my colours").next().is_some());
    assert!(h.query_all_by_label("Keep my lamps").next().is_some());
    h.get_by_label("Lamp-lit rock").click();
    h.run();
    let studio = h.state();
    assert_eq!(studio.status, "Using the Lamp-lit rock look.");
    let look = WaterLook::from_params(&studio.resolved.things[&liquid].slots[&SlotId::Look].params);
    assert_eq!(look, ReadyLook::LampLitRock.row().liquid);
    assert_eq!(of_kind(studio, water::LAMP).len(), 2, "its two lamps");
    assert!(h.state_mut().undo());
    h.run();
    assert_eq!(h.state().scene, before, "one undo puts it all back");
}

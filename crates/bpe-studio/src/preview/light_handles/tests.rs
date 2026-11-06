use super::*;
use crate::preview::{apply_drag, begin_drag, hit_test};
use bpe_runtime::CameraView;
use eframe::egui::{Rect, vec2};

fn view() -> View {
    View {
        rect: Rect::from_min_size(Pos2::ZERO, vec2(270.0, 480.0)),
        scale: 0.25,
        camera: CameraView::default(),
        world: false,
    }
}

/// The headless scene (a circle shape 1, a ball 2 at (0, -200)) with a light of `kind` at the
/// root, selected, at (0, 300).
fn with_light(kind: LightType) -> (Studio, ThingId) {
    let mut studio = Studio::headless();
    let reg = &studio.kit.clone().registry;
    let before = studio.scene.clone();
    let id = bpe_kit::things::light::add_light(&mut studio.scene, reg, kind, ThingId::ROOT).unwrap();
    studio.scene.things.get_mut(&id).unwrap().params.set("position", Value::Vec2([0.0, 300.0]));
    studio.after_edit(before);
    studio.selection.set(id);
    (studio, id)
}

/// Drags `handle` from `from` to `to` (world pixels) as the preview does; true when keyed.
fn drag(studio: &mut Studio, handle: Handle, from: Vec2, to: Vec2) -> bool {
    let v = view();
    let d = begin_drag(studio, &v, handle, v.to_screen(from));
    let world = studio.playback.world().clone();
    let time = studio.sim_time();
    let reg = studio.kit.clone();
    apply_drag(&mut studio.scene, &reg.registry, &world, &d, to, None, time)
}

fn get(studio: &Studio, id: ThingId) -> LightSettings {
    LightSettings::from_params(&studio.scene.things[&id].params)
}

#[test]
fn the_selected_lights_lamp_is_found_and_dragging_it_moves_position() {
    let (mut studio, id) = with_light(LightType::Point);
    let v = view();
    let lamp = v.to_screen(Vec2::new(0.0, 300.0));
    assert_eq!(hit_test(&studio, &v, lamp), Some(Handle::Light(id, LightPart::Lamp(0))));
    drag(&mut studio, Handle::Light(id, LightPart::Lamp(0)), Vec2::new(0.0, 300.0), Vec2::new(120.0, 250.0));
    assert_eq!(get(&studio, id).position, Vec2::new(120.0, 250.0));
}

#[test]
fn an_unselected_light_shows_no_handles_but_the_lights_tab_shows_every_lamp() {
    let (mut studio, id) = with_light(LightType::Point);
    let v = view();
    let lamp = v.to_screen(Vec2::new(0.0, 300.0));
    studio.selection.clear();
    assert_eq!(hit_test(&studio, &v, lamp), None);
    studio.left = LeftPanel::Lights;
    assert_eq!(hit_test(&studio, &v, lamp), Some(Handle::Light(id, LightPart::Lamp(0))));
}

#[test]
fn the_reach_ring_sets_reach() {
    let (mut studio, id) = with_light(LightType::Point);
    let v = view();
    let reach = get(&studio, id).reach;
    let on_ring = v.to_screen(Vec2::new(reach, 300.0));
    assert_eq!(hit_test(&studio, &v, on_ring), Some(Handle::Light(id, LightPart::Reach)));
    drag(&mut studio, Handle::Light(id, LightPart::Reach), Vec2::new(reach, 300.0), Vec2::new(0.0, 300.0 + 450.0));
    assert_eq!(get(&studio, id).reach, 450.0);
}

#[test]
fn a_spots_cone_edge_sets_its_width_and_its_aim_its_direction() {
    let (mut studio, id) = with_light(LightType::Spot);
    // Points down (90°): an edge dragged to 30° off the aim makes a 60° cone.
    let to = Vec2::new(0.0, 300.0) + Vec2::from_angle(120f32.to_radians()) * 200.0;
    drag(&mut studio, Handle::Light(id, LightPart::Cone(true)), Vec2::new(0.0, 400.0), to);
    assert!((get(&studio, id).cone - 60.0).abs() < 0.11, "{}", get(&studio, id).cone);
    drag(&mut studio, Handle::Light(id, LightPart::Aim), Vec2::new(0.0, 400.0), Vec2::new(-100.0, 300.0));
    assert!((get(&studio, id).direction.abs() - 180.0).abs() < 0.11, "{}", get(&studio, id).direction);
}

#[test]
fn a_spinning_lights_aim_takes_off_how_far_it_has_spun() {
    let (mut studio, id) = with_light(LightType::Spot);
    let before = studio.scene.clone();
    let params = &mut studio.scene.things.get_mut(&id).unwrap().params;
    params.set("points", Value::Choice("spins".into()));
    params.set("spin", Value::Number(10.0));
    studio.after_edit(before);
    studio.seek(2.0);
    // Spun 20° by 2 s: aiming at 0° stores -20°.
    drag(&mut studio, Handle::Light(id, LightPart::Aim), Vec2::new(0.0, 400.0), Vec2::new(100.0, 300.0));
    assert!((get(&studio, id).direction + 20.0).abs() < 0.11, "{}", get(&studio, id).direction);
}

#[test]
fn dragging_a_strips_end_keeps_the_other_end() {
    let (mut studio, id) = with_light(LightType::Strip);
    // A 400 px strip pointing down from (0, 100) to (0, 500): drag its far end to (300, 100).
    drag(&mut studio, Handle::Light(id, LightPart::End(1)), Vec2::new(0.0, 500.0), Vec2::new(300.0, 100.0));
    let s = get(&studio, id);
    assert_eq!(s.length, 300.0);
    assert!(s.direction.abs() < 0.11, "{}", s.direction);
    assert_eq!(s.position, Vec2::new(150.0, 100.0), "the end at (0, 100) stays");
}

#[test]
fn an_area_lights_corner_sets_width_and_length() {
    let (mut studio, id) = with_light(LightType::Area);
    // Pointing down: width runs along y, length along x.
    drag(&mut studio, Handle::Light(id, LightPart::Corner), Vec2::new(0.0, 300.0), Vec2::new(-80.0, 450.0));
    let s = get(&studio, id);
    assert_eq!((s.width, s.length), (300.0, 160.0));
}

#[test]
fn a_keyed_position_gets_a_key_at_the_playhead() {
    let (mut studio, id) = with_light(LightType::Point);
    let reg = studio.kit.clone();
    let a = Address::own(id, "position");
    studio.scene.set_key(&reg.registry, &a, 0.0, Value::Vec2([0.0, 300.0])).unwrap();
    studio.seek(1.0);
    let keyed = drag(&mut studio, Handle::Light(id, LightPart::Lamp(0)), Vec2::new(0.0, 300.0), Vec2::new(50.0, 300.0));
    assert!(keyed);
    assert_eq!(studio.scene.value_at_time(&reg.registry, &a, 1.0), Some(Value::Vec2([50.0, 300.0])));
    assert_eq!(studio.scene.value_at_time(&reg.registry, &a, 0.0), Some(Value::Vec2([0.0, 300.0])));
}

#[test]
fn a_rider_is_grabbed_before_its_ball_and_moves_by_its_offset() {
    let mut studio = Studio::headless();
    let reg = studio.kit.clone();
    let before = studio.scene.clone();
    let id = bpe_kit::things::light::add_light(&mut studio.scene, &reg.registry, LightType::Point, ThingId(2)).unwrap();
    studio.after_edit(before);
    studio.selection.set(id);
    let v = view();
    let ball = v.to_screen(Vec2::new(0.0, -200.0));
    assert_eq!(hit_test(&studio, &v, ball), Some(Handle::Light(id, LightPart::Lamp(0))));
    drag(&mut studio, Handle::Light(id, LightPart::Lamp(0)), Vec2::new(0.0, -200.0), Vec2::new(30.0, -240.0));
    assert_eq!(get(&studio, id).position, Vec2::new(30.0, -40.0), "the offset from the ball it rides");
}

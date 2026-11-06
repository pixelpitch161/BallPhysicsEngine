//! Lighting part 6: rigs play their patterns on their lights, and lighting scenes stand in for
//! the lights' settings. Checked on the lights a frame hands the renderer (no GPU needed), then
//! once in pictures.
//!
//! `BPE_SAVE_RENDERS=1` saves the pictures into `target/pictures/show-*.png`.

use bpe_core::{Address, Color, Registry, Scene, Settings, SlotId, ThingId, Value};
use bpe_gfx::{Frame, LightGpu, build_frame};
use bpe_kit::modules::light_scenes::{self, ScenesPlay};
use bpe_kit::things::light::{LightSettings, LightType, add_light};
use bpe_kit::things::rig::{Across, Pattern, RigColours, RigSettings, StepsOn, add_rig};
use bpe_kit::{kit, scenes};
use bpe_runtime::Playback;

fn reg() -> Registry {
    kit().registry
}

fn frame_of(scene: &Scene, t: f64) -> Frame {
    let kit = kit();
    let resolved = bpe_core::resolve(scene, &kit.registry);
    let mut playback = Playback::new(scene, &resolved, kit.sims.clone());
    playback.seek(t);
    let mut frame = Frame::default();
    build_frame(scene, &resolved, playback.world(), playback.log(), t, &kit.draws, &mut frame);
    frame
}

fn lights_at(scene: &Scene, t: f64) -> Vec<LightGpu> {
    frame_of(scene, t).light.expect("a lit frame").lights
}

/// The brightness each light shines with (its colour's green channel × strength: the starter
/// spots are warm white).
fn strengths(lights: &[LightGpu]) -> Vec<f32> {
    lights.iter().map(|l| l.p1.iter().copied().fold(0.0, f32::max)).collect()
}

fn set_rig(scene: &mut Scene, rig: ThingId, f: impl FnOnce(&mut RigSettings)) {
    let mut r = RigSettings::from_params(&scene.things[&rig].params);
    f(&mut r);
    scene.things.get_mut(&rig).unwrap().params = r.to_params();
}

/// A scene with one rig of four spots and nothing else lit.
fn rig_scene() -> (Scene, ThingId) {
    let reg = reg();
    let mut scene = bpe_kit::new_scene(&reg);
    let rig = add_rig(&mut scene, &reg, ThingId::ROOT).unwrap();
    set_rig(&mut scene, rig, |r| {
        r.off_level = 0.0;
        r.tail = 0.0;
    });
    (scene, rig)
}

/// Which of the lights shine (brighter than a tenth of the brightest possible).
fn lit(lights: &[LightGpu]) -> Vec<bool> {
    strengths(lights).iter().map(|&s| s > 0.1).collect()
}

#[test]
fn a_new_rig_has_four_spots_and_the_lighting_card() {
    let (scene, rig) = rig_scene();
    let kids = scene.children(rig);
    assert_eq!(kids.len(), 4);
    assert!(kids.iter().all(|k| scene.things[k].kind == "light"));
    assert!(scene.things[&ThingId::ROOT].slots.contains_key(&SlotId::Lighting));
    assert!(bpe_core::resolve(&scene, &reg()).problems.is_empty());
}

#[test]
fn a_chase_on_the_beat_moves_one_light_along_each_beat() {
    let (scene, _) = rig_scene();
    // 120 BPM: a beat every half second.
    assert_eq!(lit(&lights_at(&scene, 0.1)), [true, false, false, false]);
    assert_eq!(lit(&lights_at(&scene, 0.6)), [false, true, false, false]);
    assert_eq!(lit(&lights_at(&scene, 1.6)), [false, false, false, true]);
    assert_eq!(lit(&lights_at(&scene, 2.1)), [true, false, false, false]);
}

#[test]
fn a_light_outside_the_rig_is_left_alone_and_one_inside_joins_the_chase() {
    let (mut scene, rig) = rig_scene();
    let reg = reg();
    let lone = add_light(&mut scene, &reg, LightType::Point, ThingId::ROOT).unwrap();
    let lights = lights_at(&scene, 0.6);
    let lone_at = lights.len() - 1;
    let own = LightSettings::from_params(&scene.things[&lone].params);
    assert!((strengths(&lights)[lone_at] - own.strength).abs() < 1e-4, "unchanged by the rig");
    // A fifth light in the rig: the chase now goes round five.
    add_light(&mut scene, &reg, LightType::Spot, rig).unwrap();
    let at = |t: f64| lit(&lights_at(&scene, t));
    assert_eq!(at(2.1)[..4], [false, false, false, false], "beat 4 is the fifth light's");
}

#[test]
fn colours_from_the_list_colour_the_lit_light() {
    let (mut scene, rig) = rig_scene();
    set_rig(&mut scene, rig, |r| {
        r.colours_from = RigColours::TheList;
        r.colours = bpe_core::Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)]);
    });
    let first = &lights_at(&scene, 0.1)[0];
    assert!(first.p1[0] > 0.5 && first.p1[2] < 1e-4, "red on step 0: {:?}", first.p1);
    let second = &lights_at(&scene, 0.6)[1];
    assert!(second.p1[2] > 0.5 && second.p1[0] < 1e-4, "blue on step 1: {:?}", second.p1);
}

#[test]
fn a_sweep_turns_the_spots() {
    let (mut scene, rig) = rig_scene();
    set_rig(&mut scene, rig, |r| {
        r.pattern = Pattern::Sweep;
        r.sweep_angle = 60.0;
        r.offset = 0.0;
    });
    // Half way through the first beat the swing is at its end: 90° down + 30°.
    let l = &lights_at(&scene, 0.25)[0];
    let angle = l.p3[1].atan2(l.p3[0]).to_degrees();
    assert!((angle - 120.0).abs() < 0.5, "{angle}");
    let back = &lights_at(&scene, 0.75)[0];
    assert!((back.p3[1].atan2(back.p3[0]).to_degrees() - 60.0).abs() < 0.5);
}

#[test]
fn nested_rigs_multiply_their_brightness() {
    let (mut scene, outer) = rig_scene();
    let reg = reg();
    set_rig(&mut scene, outer, |r| {
        r.pattern = Pattern::Still;
        r.brightness = 0.5;
    });
    let inner = bpe_kit::add_thing(&mut scene, &reg, "rig", outer).unwrap();
    set_rig(&mut scene, inner, |r| {
        r.pattern = Pattern::Still;
        r.brightness = 0.5;
    });
    let lamp = add_light(&mut scene, &reg, LightType::Point, inner).unwrap();
    let own = LightSettings::from_params(&scene.things[&lamp].params).strength;
    let lights = lights_at(&scene, 0.0);
    let last = strengths(&lights)[lights.len() - 1];
    assert!((last - own * 0.25).abs() < 1e-3, "{last} vs {own} × ¼");
}

#[test]
fn every_lamp_chases_ball_to_ball() {
    let reg = reg();
    let mut scene = scenes::lit_room(&reg);
    let ball = scene.things.values().find(|t| t.kind == "balls").unwrap().id;
    let mut balls = bpe_kit::BallsSettings::from_params(&scene.things[&ball].params);
    balls.ring(4, 200.0);
    scene.things.get_mut(&ball).unwrap().params = balls.to_params();
    let rig = bpe_kit::add_thing(&mut scene, &reg, "rig", ball).unwrap();
    set_rig(&mut scene, rig, |r| {
        r.across = Across::EveryLamp;
        r.off_level = 0.0;
        r.tail = 0.0;
    });
    let lamp = add_light(&mut scene, &reg, LightType::Point, rig).unwrap();
    assert_eq!(scene.things[&lamp].params.get("position"), Some(&Value::Vec2([0.0, 0.0])), "rides the balls");
    // The room's light first, then one lamp per ball.
    let on = |t: f64, scene: &Scene| lit(&lights_at(scene, t))[1..].to_vec();
    assert_eq!(on(0.1, &scene), [true, false, false, false]);
    assert_eq!(on(1.1, &scene), [false, false, true, false]);
    // Lights: all four lamps are one fixture, so a one-light chase keeps them lit.
    set_rig(&mut scene, rig, |r| r.across = Across::Lights);
    assert_eq!(on(1.1, &scene), [true, true, true, true]);
}

#[test]
fn a_strobe_on_hits_flashes_after_each_heard_hit() {
    let reg = reg();
    let mut scene = scenes::lit_room(&reg);
    let rig = add_rig(&mut scene, &reg, ThingId::ROOT).unwrap();
    set_rig(&mut scene, rig, |r| {
        r.pattern = Pattern::Strobe;
        r.steps_on = StepsOn::Hits;
        r.step_lasts = 0.2;
        r.flash_length = 0.5;
        r.tail = 0.0;
        r.off_level = 0.0;
        r.least_gap = 0.0;
    });
    let kit = kit();
    let resolved = bpe_core::resolve(&scene, &kit.registry);
    let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
    playback.seek(6.0);
    let rate = playback.world().steps_per_second;
    let first = playback.log().events().iter().find(|e| e.kind == bpe_runtime::EventKind::Hit).expect("the ball lands");
    let hit = first.time(rate);
    let spots = |t: f64| strengths(&lights_at(&scene, t))[1..].to_vec();
    assert!(spots(hit - 0.05).iter().all(|&s| s == 0.0), "dark before the first hit");
    assert!(spots(hit + 0.05).iter().all(|&s| s > 0.5), "flashing just after it");
}

fn show_key(scene: &mut Scene, t: f64, n: i64) {
    let a = Address::on(ThingId::ROOT, SlotId::Lighting, light_scenes::SHOW_SCENE);
    scene.set_key(&reg(), &a, t, Value::Int(n)).unwrap();
}

fn set_card(scene: &mut Scene, key: &str, v: Value) {
    let a = Address::on(ThingId::ROOT, SlotId::Lighting, key);
    scene.set_at(&reg(), &a, v).unwrap();
}

#[test]
fn a_scene_on_the_timeline_stands_in_for_the_lights_settings_and_crossfades() {
    let reg = reg();
    let mut scene = scenes::lit_room(&reg);
    let light = scene.things.values().find(|t| t.kind == "light").unwrap().id;
    let strength = Address::own(light, "strength");
    scene.set_at(&reg, &strength, Value::Number(1.0)).unwrap();
    light_scenes::add_scene(&mut scene, &reg, 0.0).unwrap();
    scene.set_at(&reg, &strength, Value::Number(3.0)).unwrap();
    scene.set_at(&reg, &Address::own(light, "colour"), Value::Color(Color::rgb(0, 0, 255))).unwrap();
    light_scenes::add_scene(&mut scene, &reg, 0.0).unwrap();
    scene.set_at(&reg, &strength, Value::Number(2.0)).unwrap();
    let max = |t: f64, s: &Scene| strengths(&lights_at(s, t))[0];
    assert!((max(1.0, &scene) - 2.0).abs() < 0.05, "Off: the light's own");
    set_card(&mut scene, "scenes_play", Value::Choice("on_the_timeline".into()));
    set_card(&mut scene, "scene_fade", Value::Number(1.0));
    show_key(&mut scene, 0.0, 1);
    show_key(&mut scene, 2.0, 2);
    let warm = lights_at(&scene, 1.0)[0].p1;
    assert!((warm[0] - 1.0).abs() < 0.01, "scene 1's strength and warm colour: {warm:?}");
    let half = lights_at(&scene, 2.5)[0].p1;
    let blue = lights_at(&scene, 3.5)[0].p1;
    assert!(blue[0] < 1e-4 && (blue[2] - 3.0).abs() < 0.01, "scene 2: blue at 3: {blue:?}");
    assert!(half[2] > warm[2] && half[2] < blue[2] && half[0] > 0.1, "half way between: {half:?}");
    // Removing scene 1 makes its key show the light's own settings.
    light_scenes::remove_scene(&mut scene, &reg, 0).unwrap();
    assert!((max(1.0, &scene) - 2.0).abs() < 0.05);
}

#[test]
fn scenes_in_turn_change_every_few_bars_and_on_hits() {
    let reg = reg();
    let mut scene = scenes::lit_room(&reg);
    let light = scene.things.values().find(|t| t.kind == "light").unwrap().id;
    let strength = Address::own(light, "strength");
    for s in [1.0, 2.0, 3.0] {
        scene.set_at(&reg, &strength, Value::Number(s)).unwrap();
        light_scenes::add_scene(&mut scene, &reg, 0.0).unwrap();
    }
    set_card(&mut scene, "scenes_play", Value::Choice(bpe_core::Choice::name(ScenesPlay::InTurn).into()));
    set_card(&mut scene, "scene_every", Value::Number(1.0));
    set_card(&mut scene, "scene_fade", Value::Number(0.0));
    let max = |t: f64, s: &Scene| strengths(&lights_at(s, t))[0];
    // 120 BPM in 4: two-second bars.
    let seen: Vec<f32> = [0.5, 2.5, 4.5, 6.5].iter().map(|&t| max(t, &scene)).collect();
    assert_eq!(seen.iter().map(|s| s.round()).collect::<Vec<_>>(), [1.0, 2.0, 3.0, 1.0], "{seen:?}");
    set_card(&mut scene, "scenes_play", Value::Choice("on_hits".into()));
    assert!((max(0.0, &scene) - 1.0).abs() < 0.05, "scene 1 before any hit");
    assert!(max(6.0, &scene) > 1.5, "a later scene once the ball has hit");
}

#[test]
fn the_light_show_scene_resolves_and_lights_up() {
    let reg = reg();
    let scene = scenes::light_show(&reg);
    assert!(bpe_core::resolve(&scene, &reg).problems.is_empty());
    assert_eq!(light_scenes::scenes_of(&scene).len(), 3);
    let back = bpe_core::save::load(&bpe_core::save::save(&scene), &reg).unwrap();
    assert_eq!(light_scenes::scenes_of(&back), light_scenes::scenes_of(&scene), "scenes survive saving");
    let lights = lights_at(&scene, 1.0);
    assert_eq!(lights.len(), 1 + 4 + 12, "the fill, four spots, a lamp per ball");
}

/// Writes the light show scene to `BPE_DEMO_SCENE` (for the review video), when set.
#[test]
fn write_the_demo_scene() {
    let Some(path) = std::env::var_os("BPE_DEMO_SCENE") else { return };
    std::fs::write(path, bpe_core::save::save(&scenes::light_show(&reg()))).unwrap();
}

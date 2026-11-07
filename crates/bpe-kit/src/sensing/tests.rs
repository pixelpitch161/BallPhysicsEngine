//! The light probe and what reads it: Follow light, the Light and Shadow spreads and Moths.

use bpe_core::{Address, LightSource, PulseSource, Scene, SlotId, ThingId, Value, resolve};
use bpe_runtime::{LightAt, Log, World, level_of};
use glam::Vec2;

use super::*;
use crate::testing::{BALLS, SHAPE, scene_with_balls};
use crate::things::light::{LightType, add_light};

/// A lit room: the test scene's ball at the middle (standing still, no gravity), a Point light
/// `light_at`, and ambient `ambient`.
fn lit(arrangement: &str, light_at: Vec2, ambient: f64) -> (Scene, ThingId, crate::Kit) {
    let (mut scene, _, kit) = scene_with_balls(arrangement, |p| p.set("velocity", Value::Vec2([0.0, 0.0])));
    scene.card_mut(BALLS, SlotId::Motion).unwrap().params.set("gravity_scale", Value::Number(0.0));
    let light = add_light(&mut scene, &kit.registry, LightType::Point, ThingId::ROOT).unwrap();
    scene.things.get_mut(&light).unwrap().params.set("position", Value::Vec2([light_at.x, light_at.y]));
    scene.card_mut(ThingId::ROOT, SlotId::Lighting).unwrap().params.set("ambient", Value::Number(ambient));
    (scene, light, kit)
}

fn world_of(scene: &Scene, kit: &crate::Kit) -> World {
    World::build(scene, &resolve(scene, &kit.registry), &kit.sims)
}

fn probe_of(scene: &Scene, kit: &crate::Kit) -> std::sync::Arc<dyn LightAt> {
    probe(scene, &resolve(scene, &kit.registry), None)
}

/// Puts ball rows where `at` says (the rest far away, out of the light), standing still.
fn place(world: &mut World, at: &[Vec2]) {
    let b = world.bodies.get_mut(&BALLS).unwrap();
    for row in 0..b.len() {
        b.position[row] = at.get(row).copied().unwrap_or(Vec2::new(4000.0 + 80.0 * row as f32, 4000.0));
        b.velocity[row] = Vec2::ZERO;
    }
}

#[test]
fn a_lamp_lights_a_ball_more_the_nearer_it_is() {
    let (scene, light, kit) = lit("single", Vec2::new(0.0, -300.0), 0.25);
    let mut world = world_of(&scene, &kit);
    let probe = probe_of(&scene, &kit);
    let log = Log::default();
    place(&mut world, &[Vec2::ZERO]);
    let near = probe.level(&world, &log, BALLS, &LightSource::On, 0.0);
    place(&mut world, &[Vec2::new(0.0, 300.0)]);
    let far = probe.level(&world, &log, BALLS, &LightSource::On, 0.0);
    place(&mut world, &[Vec2::new(0.0, 2000.0)]);
    let dark = probe.level(&world, &log, BALLS, &LightSource::On, 0.0);
    assert!(near > far && far > dark, "{near} {far} {dark}");
    assert!((dark - level_of(0.25)).abs() < 1e-4, "out of reach: ambient alone, {dark}");
    assert_eq!(probe.level(&world, &log, BALLS, &LightSource::Brightest, 0.0), 0.0);
    assert_eq!(probe.level(&world, &log, light, &LightSource::Strength, 0.0), 1.0, "it shines as set");
    assert_eq!(probe.level(&world, &log, BALLS, &LightSource::Strength, 0.0), 0.0, "not a light");
}

#[test]
fn a_ball_between_the_lamp_and_another_shadows_it() {
    let (mut scene, _, kit) = lit("ring", Vec2::new(0.0, -400.0), 0.25);
    let mut world = world_of(&scene, &kit);
    let probe = probe_of(&scene, &kit);
    let log = Log::default();
    // Ball 0 under ball 1 (in its shadow), ball 2 off to the side.
    place(&mut world, &[Vec2::ZERO, Vec2::new(0.0, -150.0), Vec2::new(250.0, 0.0)]);
    let hidden = probe.ball(&world, &log, BALLS, 0, 0.0);
    let blocker = probe.ball(&world, &log, BALLS, 1, 0.0);
    let open = probe.ball(&world, &log, BALLS, 2, 0.0);
    assert!(hidden.shadow > 0.9, "{hidden:?}");
    assert!(blocker.shadow < 0.05 && open.shadow < 0.05, "{blocker:?} {open:?}");
    assert!(hidden.light < open.light, "{hidden:?} {open:?}");
    let in_shadow = probe.level(&world, &log, BALLS, &LightSource::InShadow, 0.0);
    assert!(in_shadow > 0.0 && in_shadow < 0.2, "one ball of 24: {in_shadow}");
    // Shadows Off: nothing is in shadow.
    let card = scene.card_mut(ThingId::ROOT, SlotId::Lighting).unwrap();
    card.params.set("shadow_style", Value::Choice("off".into()));
    let probe = probe_of(&scene, &kit);
    assert!(probe.ball(&world, &log, BALLS, 0, 0.0).shadow < 1e-6);
}

/// Glass and light part 1: a see-through ball lets its share of the light through, in its colour
/// when Coloured light through glass is on.
#[test]
fn a_see_through_ball_lets_its_share_through_in_its_colour() {
    let (mut scene, _, kit) = lit("ring", Vec2::new(0.0, -400.0), 0.0);
    let mut world = world_of(&scene, &kit);
    let log = Log::default();
    place(&mut world, &[Vec2::ZERO, Vec2::new(0.0, -150.0)]);
    let body = scene.things[&BALLS].slots[&SlotId::Look].layers[0].id;
    let card = scene.card_mut(BALLS, SlotId::Look).unwrap();
    let red = bpe_core::Gradient::solid(bpe_core::Color::rgb(255, 0, 0));
    card.params.set(&bpe_core::layered_key(body, "colours"), Value::Gradient(red));
    let solid = probe_of(&scene, &kit).ball(&world, &log, BALLS, 0, 0.0).shadow;
    assert!(solid > 0.9, "solid: {solid}");
    scene.card_mut(BALLS, SlotId::Look).unwrap().params.set("see_through", Value::Number(0.6));
    let red = probe_of(&scene, &kit).ball(&world, &log, BALLS, 0, 0.0).shadow;
    let want = 1.0 - 0.6 * probe::luminance([1.0, 0.0, 0.0]);
    assert!((red - want).abs() < 0.03, "red glass: {red} against {want}");
    scene.card_mut(ThingId::ROOT, SlotId::Lighting).unwrap().params.set("glass_colour", Value::Bool(false));
    let white = probe_of(&scene, &kit).ball(&world, &log, BALLS, 0, 0.0).shadow;
    assert!((white - 0.4).abs() < 0.03, "colour off: {white}");
}

#[test]
fn a_crowd_shadows_those_behind_it() {
    // A row of 24 balls in a line pointing at the lamp: every ball but the nearest is shadowed.
    let (scene, _, kit) = lit("ring", Vec2::new(-400.0, 0.0), 0.25);
    let mut world = world_of(&scene, &kit);
    let probe = probe_of(&scene, &kit);
    let log = Log::default();
    let at: Vec<Vec2> = (0..24).map(|i| Vec2::new(-300.0 + i as f32 * 30.0, 0.0)).collect();
    place(&mut world, &at);
    let shadows: Vec<f32> = (0..24).map(|r| probe.ball(&world, &log, BALLS, r, 0.0).shadow).collect();
    assert!(shadows[0] < 0.05, "{shadows:?}");
    assert!(shadows[1..20].iter().all(|s| *s > 0.9), "{shadows:?}");
}

#[test]
fn a_glowing_ball_lights_its_neighbour_but_not_itself() {
    let (mut scene, light, kit) = lit("ring", Vec2::new(0.0, 0.0), 0.25);
    // Only the ball's own light: the lamp is out of everyone's reach.
    scene.things.get_mut(&light).unwrap().params.set("reach", Value::Number(1.0));
    let body = scene.card(BALLS, SlotId::Look).unwrap().layers[0].id;
    let added = scene.add_sub_layer(
        &kit.registry,
        BALLS,
        SlotId::Look,
        body,
        crate::modules::look::layers::gives_light::SUB_LIGHT,
    );
    assert!(added.is_ok(), "{added:?}");
    let mut world = world_of(&scene, &kit);
    let probe = probe_of(&scene, &kit);
    let log = Log::default();
    place(&mut world, &[Vec2::new(-600.0, 2000.0)]);
    let alone = probe.ball(&world, &log, BALLS, 0, 0.0);
    assert!((alone.light - level_of(0.25)).abs() < 1e-4, "its own glow doesn't fall on it: {alone:?}");
    place(&mut world, &[Vec2::ZERO, Vec2::new(120.0, 0.0)]);
    let beside = probe.ball(&world, &log, BALLS, 0, 0.0);
    assert!(beside.light > alone.light + 0.05, "{beside:?} {alone:?}");
}

#[test]
fn its_strength_follows_a_keyed_fade() {
    let (mut scene, light, kit) = lit("single", Vec2::new(0.0, -300.0), 0.25);
    let strength = Address::own(light, "strength");
    scene.set_key(&kit.registry, &strength, 0.0, Value::Number(0.0)).unwrap();
    scene.set_key(&kit.registry, &strength, 2.0, Value::Number(1.5)).unwrap();
    let world = world_of(&scene, &kit);
    let probe = probe_of(&scene, &kit);
    let log = Log::default();
    let at = |t| probe.level(&world, &log, light, &LightSource::Strength, t);
    assert_eq!(at(0.0), 0.0);
    assert!(at(1.0) > 0.1 && at(1.0) < 0.9, "{}", at(1.0));
    assert!((at(2.0) - 1.0).abs() < 1e-4);
}

#[test]
fn a_look_setting_follows_the_light_on_its_balls() {
    let (mut scene, _, kit) = lit("single", Vec2::new(0.0, -200.0), 0.25);
    let body = scene.card(BALLS, SlotId::Look).unwrap().layers[0].id;
    let a = Address::on(BALLS, SlotId::Look, &bpe_core::layered_key(body, "opacity"));
    assert!(can_follow_light(&scene, &kit.registry, &a));
    let source = PulseSource::Light { thing: BALLS, source: LightSource::On };
    scene.set_pulse(&kit.registry, &a, source, Value::Number(-1.0), 0.25).unwrap();
    assert!(uses_light(&scene));
    let resolved = resolve(&scene, &kit.registry);
    let world = World::build(&scene, &resolved, &kit.sims);
    assert!(world.light().is_none(), "a look following light leaves the physics alone");
    let probe = probe(&scene, &resolved, None);
    let log = Log::default();
    let levels = bpe_runtime::LightLevelsAt { light: probe.as_ref(), world: &world, log: &log };
    let animation = bpe_core::Animation::new(&scene, &resolved);
    let mut now = resolved.clone();
    animation.apply(&mut now, 0.0, &bpe_core::Inputs { light: Some(&levels), ..bpe_core::Inputs::NONE });
    let opacity = |r: &bpe_core::Resolved| match r.things[&BALLS].slots[&SlotId::Look].params.get(&a.key) {
        Some(Value::Number(x)) => *x,
        other => panic!("{other:?}"),
    };
    let lit_level = probe.level(&world, &log, BALLS, &LightSource::On, 0.0) as f64;
    assert!(lit_level > 0.3);
    assert!((opacity(&now) - (opacity(&resolved) - lit_level).max(0.0)).abs() < 1e-3);
}

#[test]
fn what_the_lighting_reads_never_follows_light() {
    let (scene, light, kit) = lit("single", Vec2::ZERO, 0.25);
    let reg = &kit.registry;
    assert!(!can_follow_light(&scene, reg, &Address::own(light, "strength")));
    assert!(!can_follow_light(&scene, reg, &Address::on(ThingId::ROOT, SlotId::Lighting, "ambient")));
    assert!(!can_follow_light(&scene, reg, &Address::own(ThingId::ROOT, "bg_glow")));
    let body = scene.card(BALLS, SlotId::Look).unwrap().layers[0].id;
    let lit_key = Address::on(BALLS, SlotId::Look, &bpe_core::layered_key(body, "lit"));
    assert!(!can_follow_light(&scene, reg, &lit_key));
    let surface = Address::on(BALLS, SlotId::Look, "roughness");
    assert!(!can_follow_light(&scene, reg, &surface), "how a look takes light is the lighting's");
    assert!(can_follow_light(&scene, reg, &Address::own(BALLS, "radius")));
    // Without a Lighting card nothing follows light.
    let (plain, _, kit) = scene_with_balls("single", |_| {});
    assert!(!can_follow_light(&plain, &kit.registry, &Address::own(BALLS, "radius")));
    assert!(!uses_light(&plain));
    let _ = SHAPE;
}

#[test]
fn a_layer_that_gives_off_light_is_the_lightings_too() {
    let (mut scene, _, kit) = lit("single", Vec2::ZERO, 0.25);
    let reg = &kit.registry;
    let body = scene.card(BALLS, SlotId::Look).unwrap().layers[0].id;
    let colour = Address::on(BALLS, SlotId::Look, &bpe_core::layered_key(body, "colour"));
    assert!(can_follow_light(&scene, reg, &colour));
    scene.add_sub_layer(reg, BALLS, SlotId::Look, body, SUB_LIGHT).unwrap();
    assert!(!can_follow_light(&scene, reg, &colour), "its light takes the Body's colour");
}

#[test]
fn the_probes_fingerprint_hears_the_tempo() {
    let (mut scene, _, _) = lit("single", Vec2::ZERO, 0.25);
    let before = fingerprint(&scene);
    scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("tempo", Value::Number(97.0));
    assert_ne!(fingerprint(&scene), before, "rigs keep time by the tempo");
}

#[test]
fn copies_of_a_world_never_share_the_probes_moments() {
    let (scene, _, kit) = lit("single", Vec2::new(0.0, -200.0), 0.0);
    let probe = probe_of(&scene, &kit);
    let mut a = world_of(&scene, &kit);
    let mut b = a.clone();
    place(&mut a, &[Vec2::ZERO]);
    place(&mut b, &[Vec2::new(0.0, 2000.0)]);
    let log = Log::default();
    let near = probe.ball(&a, &log, BALLS, 0, 0.0).light;
    let far = probe.ball(&b, &log, BALLS, 0, 0.0).light;
    assert!(near > far + 0.1, "the same step of two copies: {near} and {far}");
}

/// Runs `scene` for `seconds` and gives back where its first ball ends.
fn ends_at(scene: &Scene, kit: &crate::Kit, seconds: f64) -> Vec2 {
    let resolved = resolve(scene, &kit.registry);
    let mut world = World::build(scene, &resolved, &kit.sims);
    world.advance_to(seconds, &kit.sims, &mut Log::default());
    world.bodies[&BALLS].position[0]
}

#[test]
fn moths_are_drawn_to_the_light_and_light_pressure_pushes() {
    let lamp = Vec2::new(250.0, 0.0);
    let (mut scene, _, kit) = lit("single", lamp, 0.25);
    let mut start = scene_with_balls("single", |_| {}).0;
    let _ = &mut start;
    let still = ends_at(&scene, &kit, 0.5);
    let motion = |scene: &mut Scene, moths: &str| {
        scene.card_mut(BALLS, SlotId::Motion).unwrap().params.set("moths", Value::Choice(moths.into()));
    };
    motion(&mut scene, "drawn_to_light");
    assert!(uses_light(&scene));
    let resolved = resolve(&scene, &kit.registry);
    assert!(World::build(&scene, &resolved, &kit.sims).light().is_some(), "moths need the probe");
    let drawn = ends_at(&scene, &kit, 0.5);
    motion(&mut scene, "pushed_by_light");
    let pushed = ends_at(&scene, &kit, 0.5);
    let gap = |p: Vec2| p.distance(lamp);
    assert!(gap(drawn) < gap(still) - 20.0, "{drawn} {still}");
    assert!(gap(pushed) > gap(still) + 20.0, "{pushed} {still}");
    // The same run again lands on the same spot (deterministic).
    motion(&mut scene, "drawn_to_light");
    assert_eq!(ends_at(&scene, &kit, 0.5), drawn);
}

#[test]
fn a_physics_setting_follows_the_light_on_a_ball() {
    let (mut scene, _, kit) = lit("single", Vec2::new(0.0, -200.0), 0.25);
    let wind = Address::on(BALLS, SlotId::Motion, "wind");
    let source = PulseSource::Light { thing: BALLS, source: LightSource::On };
    scene.set_pulse(&kit.registry, &wind, source, Value::Vec2([400.0, 0.0]), 0.25).unwrap();
    let resolved = resolve(&scene, &kit.registry);
    let world = World::build(&scene, &resolved, &kit.sims);
    assert!(world.light().is_some());
    let moved = ends_at(&scene, &kit, 0.5);
    assert!(moved.x > 20.0, "the lit ball is blown sideways: {moved}");
    // The fingerprint moves with what the probe reads (the light's place).
    let before = bpe_runtime::fingerprint(&scene, &resolved, &kit.sims);
    let light = scene.things.values().find(|t| t.kind == "light").unwrap().id;
    scene.things.get_mut(&light).unwrap().params.set("position", Value::Vec2([0.0, 300.0]));
    assert_ne!(bpe_runtime::fingerprint(&scene, &resolve(&scene, &kit.registry), &kit.sims), before);
}

#[test]
fn light_spreads_vary_motion_ball_by_ball() {
    // Gravity × from 0 (in the dark) to 4 (lit), by each ball's light: lit balls fall faster.
    let (mut scene, _, kit) = lit("ring", Vec2::new(-300.0, 0.0), 0.0);
    let p = &mut scene.card_mut(BALLS, SlotId::Motion).unwrap().params;
    p.set("gravity_scale~to", Value::Number(4.0));
    p.set("gravity_scale~by", Value::Choice("light".into()));
    scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, 400.0]));
    let resolved = resolve(&scene, &kit.registry);
    let mut world = World::build(&scene, &resolved, &kit.sims);
    assert!(world.light().is_some());
    let mut log = Log::default();
    world.advance_to(0.3, &kit.sims, &mut log);
    let b = &world.bodies[&BALLS];
    // Balls nearer the lamp (left) fell further than those on the far side.
    let (mut near, mut far) = ((0.0, 0), (0.0, 0));
    for row in 0..b.len() {
        let fell = b.velocity[row].y;
        if b.position[row].x < 0.0 {
            near = (near.0 + fell, near.1 + 1);
        } else {
            far = (far.0 + fell, far.1 + 1);
        }
    }
    assert!(near.0 / near.1 as f32 > far.0 / far.1 as f32 + 10.0, "{near:?} {far:?}");
}

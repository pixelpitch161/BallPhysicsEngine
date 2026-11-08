//! Balls in the one wind (W13): a Motion card's Follow the air carries a ball with the Air card's
//! air, the shapes that block it shelter balls, and the physics steps the air only while some ball
//! feels it.

use bpe_core::{Address, Scene, Settings, SlotId, ThingId, Value, Who};
use bpe_runtime::{Log, World, air_felt, fingerprint};
use glam::Vec2;

use crate::modules::air::AirSettings;
use crate::testing::{BALLS, SHAPE};
use crate::{Kit, ShapeFill, ShapeSettings, kit, new_scene};

fn n(x: f64) -> Value {
    Value::Number(x)
}

/// The new scene with a still ball (no gravity) following the air with grip `follow`, and an Air
/// card blowing 300 px/s to the right with no gusts, then `f`'s changes to it.
fn windy(follow: f64, f: impl FnOnce(&mut AirSettings)) -> (Scene, Kit) {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    crate::things::particles::air::ensure_air_card(&mut scene, &kit.registry).unwrap();
    let card = scene.card_mut(ThingId::ROOT, SlotId::Air).unwrap();
    let mut a = AirSettings::from_params(&card.params);
    a.wind_speed = 300.0;
    a.wind_direction = 0.0;
    a.gusts = 0.0;
    // The ball doesn't stir the air it is carried by (that would hold the air back round it).
    a.stir = 0.0;
    f(&mut a);
    card.params = a.to_params();
    let motion = scene.card_mut(BALLS, SlotId::Motion).unwrap();
    motion.params.set("gravity_scale", n(0.0));
    motion.params.set("follow_air", n(follow));
    scene.things.get_mut(&BALLS).unwrap().params.set("velocity", Value::Vec2([0.0, 0.0]));
    (scene, kit)
}

fn run(scene: &Scene, kit: &Kit, seconds: f64) -> World {
    let resolved = bpe_core::resolve(scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    let mut world = World::build(scene, &resolved, &kit.sims);
    world.advance_to(seconds, &kit.sims, &mut Log::default());
    world
}

fn velocity(world: &World) -> Vec2 {
    world.bodies[&BALLS].velocity[0]
}

#[test]
fn a_ball_that_follows_the_air_is_carried_by_the_wind() {
    let (scene, kit) = windy(5.0, |a| a.shapes_block = false);
    assert!(air_felt(&scene, &kit.sims));
    let world = run(&scene, &kit, 1.0);
    let v = velocity(&world);
    // v = 300 (1 - e^-5t): about 298 px/s after a second.
    assert!((v.x - 298.0).abs() < 8.0 && v.y.abs() < 5.0, "carried right at about the wind's speed: {v}");
    // The air is awake and stepped with the physics.
    let air = crate::air::air_of(&world).expect("the world has the air");
    assert!(air.awake());
    assert_eq!(air.step, 120);
    // Stirring the air, the ball holds back the air round it a little, and still goes with it.
    let (stirring, kit) = windy(5.0, |a| {
        a.shapes_block = false;
        a.stir = 1.0;
    });
    let v = velocity(&run(&stirring, &kit, 1.0));
    assert!(v.x > 150.0, "{v}");
}

#[test]
fn a_ball_that_does_not_follow_the_air_stays_put_and_the_air_sleeps() {
    let (scene, kit) = windy(0.0, |a| a.shapes_block = false);
    assert!(!air_felt(&scene, &kit.sims));
    let world = run(&scene, &kit, 1.0);
    assert_eq!(velocity(&world), Vec2::ZERO, "the Air card alone moves no ball");
    assert!(crate::air::air_of(&world).is_none_or(|a| !a.awake()), "nothing feels the air: it isn't stepped");
}

#[test]
fn a_solid_shape_upwind_shelters_the_ball() {
    let speed = |blocks: bool| {
        let (mut scene, kit) = windy(5.0, |a| a.shapes_block = blocks);
        let shape = scene.things.get_mut(&SHAPE).unwrap();
        shape.params = ShapeSettings { fill: ShapeFill::Filled, ..ShapeSettings::circle(90.0, 3.0) }.to_params();
        shape.transform.position = Vec2::new(-150.0, -200.0);
        velocity(&run(&scene, &kit, 0.5)).x
    };
    let open = speed(false);
    let sheltered = speed(true);
    assert!(open > 200.0, "out in the open it is carried: {open}");
    assert!(sheltered < open * 0.8, "in the shape's lee it is slowed: {sheltered} against {open}");
}

#[test]
fn the_air_is_felt_when_follow_the_air_is_set_varied_keyed_or_ruled() {
    let kit = kit();
    let base = new_scene(&kit.registry);
    let felt = |f: &dyn Fn(&mut Scene)| {
        let mut scene = base.clone();
        f(&mut scene);
        air_felt(&scene, &kit.sims)
    };
    let set = |s: &mut Scene, k: &str, v: Value| s.card_mut(BALLS, SlotId::Motion).unwrap().params.set(k, v);
    assert!(!felt(&|_| {}), "a new scene feels no air");
    assert!(felt(&|s| set(s, "follow_air", n(2.0))));
    assert!(
        !felt(&|s| {
            set(s, "follow_air", n(2.0));
            s.things.get_mut(&BALLS).unwrap().enabled = false;
        }),
        "a switched-off thing feels nothing"
    );
    assert!(felt(&|s| set(s, "follow_air~to", n(3.0))), "varied across the balls from 0");
    assert!(felt(&|s| {
        let a = Address::on(BALLS, SlotId::Motion, "follow_air");
        s.set_key(&kit.registry, &a, 2.0, n(4.0)).unwrap();
    }));
    let ruled = |s: &mut Scene, key: &str| {
        let rule = s.add_rule("Gust");
        let when = s.add_block(&kit.registry, rule, "when.time", None).unwrap();
        let change = s.add_block(&kit.registry, rule, "do.change", Some(when)).unwrap();
        for (k, v) in [
            ("who", Value::Who(Who::Thing { thing: BALLS, ball: None })),
            ("card", Value::Choice("motion".into())),
            ("setting", Value::Text(key.into())),
            ("to_number", n(3.0)),
        ] {
            s.set_block_param(&kit.registry, rule, change, k, v).unwrap();
        }
    };
    assert!(felt(&|s| ruled(s, "follow_air")), "a rule turns it up later");
    assert!(!felt(&|s| ruled(s, "drag")), "a rule on another setting doesn't count");
}

#[test]
fn air_card_edits_restart_the_physics_only_while_a_ball_feels_the_air() {
    let changes = |follow: f64| {
        let (scene, kit) = windy(follow, |_| {});
        let mut gustier = scene.clone();
        gustier.card_mut(ThingId::ROOT, SlotId::Air).unwrap().params.set("wind_speed", n(600.0));
        let print = |s: &Scene| fingerprint(s, &bpe_core::resolve(s, &kit.registry), &kit.sims);
        print(&scene) != print(&gustier)
    };
    assert!(!changes(0.0), "no ball follows the air: the physics is the same");
    assert!(changes(2.0), "a ball follows it: the physics restarts");
}

#[test]
fn the_air_counts_how_far_its_wind_has_carried_the_haze() {
    let (scene, kit) = windy(1.0, |a| a.shapes_block = false);
    let world = run(&scene, &kit, 1.0);
    let drift = crate::air::air_of(&world).unwrap().drift;
    assert!((drift - Vec2::new(300.0, 0.0)).length() < 1.0, "a second of 300 px/s to the right: {drift}");
}

#[test]
fn haze_that_moves_with_the_air_restarts_the_playback_on_air_edits() {
    use crate::modules::lighting::{HazeMoves, LIGHTING, LightingSettings};
    let (mut scene, kit) = windy(0.0, |_| {});
    let reg = &kit.registry;
    scene.add_slot(reg, ThingId::ROOT, SlotId::Lighting, LIGHTING).unwrap();
    let changes = |scene: &Scene| {
        let mut gustier = scene.clone();
        gustier.card_mut(ThingId::ROOT, SlotId::Air).unwrap().params.set("wind_speed", n(600.0));
        let print = |s: &Scene| fingerprint(s, &bpe_core::resolve(s, reg), &kit.sims);
        print(scene) != print(&gustier)
    };
    assert!(!changes(&scene), "still haze reads no air");
    let card = scene.card_mut(ThingId::ROOT, SlotId::Lighting).unwrap();
    let with_air = LightingSettings { haze: 1.0, haze_moves: HazeMoves::WithTheAir, ..Default::default() };
    card.params = with_air.to_params();
    assert!(changes(&scene), "haze with the air drifts as the playback's air carried it");
    assert!(!air_felt(&scene, &kit.sims), "and still no ball feels the air");
}

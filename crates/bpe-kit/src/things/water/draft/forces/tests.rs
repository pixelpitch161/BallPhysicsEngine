use super::*;
use crate::things::water::draft::WaterSetup;
use crate::things::water::new_water_scene;
use bpe_core::{Address, Registry, Value};

fn add(scene: &mut Scene, reg: &Registry, kind: &str, at: Vec2) -> ThingId {
    let id = crate::add_thing(scene, reg, kind, ThingId::ROOT).unwrap();
    scene.things.get_mut(&id).unwrap().transform.position = at;
    id
}

fn set(scene: &mut Scene, id: ThingId, key: &str, v: Value) {
    scene.things.get_mut(&id).unwrap().slots.get_mut(&SlotId::Main).unwrap().params.set(key, v);
}

fn setup(scene: &Scene, reg: &Registry) -> WaterSetup {
    WaterSetup::of(scene, &bpe_core::resolve(scene, reg))
}

/// A Water scene with one Liquid and a Force of `kind` at (30, 40).
fn one_force(reg: &Registry, kind: &str) -> (Scene, ThingId) {
    let mut scene = new_water_scene(reg, "");
    add(&mut scene, reg, LIQUID, Vec2::ZERO);
    let force = add(&mut scene, reg, FORCE, Vec2::new(30.0, 40.0));
    set(&mut scene, force, "kind", Value::Choice(kind.into()));
    (scene, force)
}

#[test]
fn a_force_reads_its_card_each_frame() {
    let reg = crate::registry();
    let (mut scene, force) = one_force(&reg, "magnet");
    set(&mut scene, force, "reach", Value::Number(250.0));
    set(&mut scene, force, "spikes", Value::Int(7));
    let strength = Address { thing: force, slot: Some(SlotId::Main), key: "strength".into() };
    scene.set_key(&reg, &strength, 0.0, Value::Number(0.0)).unwrap();
    scene.set_key(&reg, &strength, 1.0, Value::Number(2.0)).unwrap();
    let s = setup(&scene, &reg);
    assert_eq!(s.forces.len(), 1);
    let pull = |frame: u32| match s.inputs_at(frame).forces[..] {
        [Force { push: Push::Magnet { pull, .. }, .. }] => pull,
        ref other => panic!("{other:?}"),
    };
    assert!((pull(60) - 2.0 * FORCE_UNIT / PSI_SLOPE_MOST).abs() < 1e-2, "{}", pull(60));
    assert!((pull(30) - pull(60) / 2.0).abs() < 1e-2, "half way: {} of {}", pull(30), pull(60));
    let f = s.inputs_at(60).forces[0];
    assert_eq!((f.place, f.reach, f.liquids), (Vec2::new(30.0, 40.0), 250.0, 1));
    let Push::Magnet { spikes, depth, aim, spread, .. } = f.push else { unreachable!() };
    assert_eq!(spikes, 7);
    assert!((depth - 0.75 * SPIKE_DEPTH).abs() < 1e-6, "Spike size 0.75 by default: {depth}");
    assert!((aim + std::f32::consts::FRAC_PI_2).abs() < 1e-6 && (spread - std::f32::consts::PI).abs() < 1e-6);
    // No spikes, no valleys.
    set(&mut scene, force, "spikes", Value::Int(0));
    let Push::Magnet { depth, .. } = setup(&scene, &reg).inputs_at(60).forces[0].push else { unreachable!() };
    assert_eq!(depth, 0.0);
}

#[test]
fn a_force_moves_the_liquids_it_names() {
    let reg = crate::registry();
    let mut scene = new_water_scene(&reg, "");
    let liquids: Vec<ThingId> = (0..3).map(|_| add(&mut scene, &reg, LIQUID, Vec2::ZERO)).collect();
    scene.things.get_mut(&liquids[2]).unwrap().tags.insert("iron".into());
    let force = add(&mut scene, &reg, FORCE, Vec2::ZERO);
    let mask = |scene: &Scene| setup(scene, &reg).forces.first().map(|f| f.liquids);
    assert_eq!(mask(&scene), Some(0b111), "Any Liquid by default");
    set(&mut scene, force, "liquid", Value::Target(Target::Thing(liquids[1])));
    assert_eq!(mask(&scene), Some(0b010));
    set(&mut scene, force, "liquid", Value::Target(Target::Tag("iron".into())));
    assert_eq!(mask(&scene), Some(0b100));
    // A Liquid that is off moves nothing, and the stage says so.
    scene.things.get_mut(&liquids[2]).unwrap().enabled = false;
    assert_eq!(mask(&scene), None);
    let s = setup(&scene, &reg);
    assert_eq!((s.note_for(force), s.inputs_at(0).forces.len()), (Some(NO_LIQUID_TO_MOVE), 0));
}

#[test]
fn wind_and_whirlpool_units() {
    let reg = crate::registry();
    let (mut scene, force) = one_force(&reg, "wind");
    set(&mut scene, force, "aim", Value::Number(90.0));
    let f = setup(&scene, &reg).inputs_at(0).forces[0];
    let Push::Wind { velocity, rate } = f.push else { panic!("{f:?}") };
    assert!((velocity - Vec2::new(0.0, WIND_SPEED)).length() < 0.1, "straight down: {velocity}");
    assert_eq!((rate, f.reach), (WIND_RATE, 0.0));
    let (mut scene, force) = one_force(&reg, "whirlpool");
    set(&mut scene, force, "strength", Value::Number(-1.0));
    let f = setup(&scene, &reg).inputs_at(0).forces[0];
    assert_eq!(f.push, Push::Whirlpool { speed: -SPIN_SPEED, rate: SPIN_RATE }, "the other way round");
    // Half the strength: half the speed, drawn half as fast.
    set(&mut scene, force, "strength", Value::Number(0.5));
    let f = setup(&scene, &reg).inputs_at(0).forces[0];
    assert_eq!(f.push, Push::Whirlpool { speed: SPIN_SPEED / 2.0, rate: SPIN_RATE / 2.0 });
}

/// Strength 0 is no force at all to the engine, whatever the kind.
#[test]
fn strength_zero_moves_nothing() {
    let reg = crate::registry();
    for kind in ["magnet", "whirlpool", "wind"] {
        let (mut scene, force) = one_force(&reg, kind);
        set(&mut scene, force, "strength", Value::Number(0.0));
        let forces = setup(&scene, &reg).inputs_at(0).forces;
        assert!(forces.iter().all(|f| !f.is_felt()), "{kind}: {forces:?}");
    }
}

#[test]
fn gusts_vary_the_wind_smoothly() {
    let mut last = gust(0.0);
    for frame in 1..36_000 {
        let g = gust(f64::from(frame) / 60.0);
        assert!(g.abs() <= 1.0, "{g} at frame {frame}");
        assert!((g - last).abs() < 0.1, "a jump at frame {frame}");
        last = g;
    }
    let reg = crate::registry();
    let (mut scene, force) = one_force(&reg, "wind");
    let speed = |scene: &Scene, frame: u32| match setup(scene, &reg).inputs_at(frame).forces[0].push {
        Push::Wind { velocity, .. } => velocity.length(),
        other => panic!("{other:?}"),
    };
    assert!((0..200).all(|f| (speed(&scene, f) - WIND_SPEED).abs() < 0.01), "Gusts 0: a steady wind");
    set(&mut scene, force, "gusts", Value::Number(1.0));
    let speeds: Vec<f32> = (0..200).map(|f| speed(&scene, f * 6)).collect();
    let (lo, hi) = speeds.iter().fold((f32::MAX, 0.0f32), |(lo, hi), s| (lo.min(*s), hi.max(*s)));
    assert!(lo < 0.7 * WIND_SPEED && hi > 1.3 * WIND_SPEED, "gusts come and go: {lo} to {hi}");
}

#[test]
fn heaters_say_they_come_later() {
    let reg = crate::registry();
    for kind in ["heater", "cooler"] {
        let (scene, force) = one_force(&reg, kind);
        let s = setup(&scene, &reg);
        assert!(s.forces.is_empty() && s.inputs_at(0).forces.is_empty());
        assert_eq!(s.note_for(force), Some(HEAT_LATER));
    }
}

#[test]
fn seventeen_forces_say_too_many() {
    let reg = crate::registry();
    let mut scene = new_water_scene(&reg, "");
    add(&mut scene, &reg, LIQUID, Vec2::ZERO);
    let forces: Vec<ThingId> = (0..17).map(|k| add(&mut scene, &reg, FORCE, Vec2::new(k as f32, 0.0))).collect();
    let s = setup(&scene, &reg);
    assert_eq!((s.forces.len(), s.inputs_at(0).forces.len()), (MOST_FORCES, MOST_FORCES));
    assert_eq!(s.note_for(forces[16]), Some(TOO_MANY_FORCES));
    assert_eq!(s.note_for(forces[15]), None);
}

#[test]
fn a_spinning_drain_is_a_whirlpool() {
    let reg = crate::registry();
    let mut scene = new_water_scene(&reg, "");
    add(&mut scene, &reg, LIQUID, Vec2::ZERO);
    add(&mut scene, &reg, LIQUID, Vec2::ZERO);
    let drain = add(&mut scene, &reg, DRAIN, Vec2::new(-20.0, 300.0));
    assert!(setup(&scene, &reg).forces.is_empty(), "Whirlpool 0: no spin");
    set(&mut scene, drain, "whirlpool", Value::Number(2.0));
    set(&mut scene, drain, "radius", Value::Number(50.0));
    let f = setup(&scene, &reg).inputs_at(0).forces[0];
    let want = Force {
        push: Push::Whirlpool { speed: 2.0 * SPIN_SPEED, rate: SPIN_RATE },
        place: Vec2::new(-20.0, 300.0),
        reach: 100.0,
        liquids: 0b11,
    };
    assert_eq!(f, want, "every liquid spins, out to twice its Radius");
    // A Leak doesn't spin; a Whirlpool keyed up from 0 spins from its key.
    set(&mut scene, drain, "kind", Value::Choice("leak".into()));
    assert!(setup(&scene, &reg).forces.is_empty());
    set(&mut scene, drain, "kind", Value::Choice("drain".into()));
    set(&mut scene, drain, "whirlpool", Value::Number(0.0));
    let spin = Address { thing: drain, slot: Some(SlotId::Main), key: "whirlpool".into() };
    scene.set_key(&reg, &spin, 1.0, Value::Number(0.0)).unwrap();
    scene.set_key(&reg, &spin, 2.0, Value::Number(1.0)).unwrap();
    let s = setup(&scene, &reg);
    assert_eq!((s.forces.len(), s.inputs_at(30).forces.len(), s.inputs_at(120).forces.len()), (1, 0, 1));
}

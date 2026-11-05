//! The modulators at audio rate: LFO, Hit and Distance, each against hand-worked values.

use bpe_core::{Scene, Settings, SlotId, ThingId, Value};
use bpe_kit::sound::{
    DistanceSettings, Follows, HitPick, HitSettings, LfoSettings, LfoShape, LfoStart, Measure, NoteFeel, NoteLength,
};
use bpe_kit::{Kit, kit, new_scene, resolve_scene};
use bpe_runtime::{Party, Playback};
use bpe_sound::mods::{
    DistanceMod, DistanceParams, HitMod, HitValues, LfoMod, ModCx, Modulator, NoteCx, hit_values, lfo_rate_hz,
};
use bpe_sound::{BLOCK, SAMPLE_RATE, TempoMap};
use glam::Vec2;

fn shape_of(scene: &Scene) -> ThingId {
    scene.order.iter().copied().find(|id| scene.things[id].kind == "shape").unwrap()
}

fn playback(scene: &Scene, kit: &Kit) -> Playback {
    let resolved = resolve_scene(scene, &kit.registry);
    Playback::new(scene, &resolved, kit.sims.clone())
}

/// Runs `m` from block `from` for `blocks` blocks, returning its value after each.
fn run(m: &mut dyn Modulator, pb: &Playback, shape: ThingId, from: u64, blocks: u64) -> Vec<f32> {
    let tempo = TempoMap::default();
    (from..from + blocks)
        .map(|b| {
            let cx = ModCx { start_sample: b * BLOCK as u64, tempo: &tempo, world: pb.world(), shape };
            m.next_block(&cx);
            m.value()
        })
        .collect()
}

#[test]
fn lfo_tempo_quarter_at_120_is_2hz() {
    let tempo = TempoMap::new(120.0, 4, 4);
    let s = LfoSettings { sync: true, rate_note: NoteLength::N4, ..LfoSettings::default() };
    assert!((lfo_rate_hz(&s, s.rate_hz as f64, &tempo) - 2.0).abs() < 1e-9);
    let dotted = LfoSettings { rate_feel: NoteFeel::Dotted, ..s.clone() };
    assert!((lfo_rate_hz(&dotted, 2.0, &tempo) - 2.0 / 1.5).abs() < 1e-9);
    let bar = LfoSettings { rate_note: NoteLength::Bar1, ..s.clone() };
    assert!((lfo_rate_hz(&bar, 2.0, &TempoMap::new(120.0, 3, 4)) - 1.0 / 1.5).abs() < 1e-9, "a bar of 3/4");
    let hz = LfoSettings { sync: false, ..s };
    assert_eq!(lfo_rate_hz(&hz, 3.5, &tempo), 3.5, "Hz is the knob as it is now");
}

#[test]
fn free_lfo_phase_matches_after_seek() {
    let kit = kit();
    let scene = new_scene(&kit.registry);
    let shape = shape_of(&scene);
    let pb = playback(&scene, &kit);
    let s = LfoSettings { shape: LfoShape::Triangle, rate_hz: 3.3, phase: 40.0, ..LfoSettings::default() };
    let tempo = TempoMap::default();
    let mut whole = LfoMod::new(9);
    whole.set(&s, &tempo, s.rate_hz as f64);
    let all = run(&mut whole, &pb, shape, 0, 400);
    let mut seeked = LfoMod::new(9);
    seeked.set(&s, &tempo, s.rate_hz as f64);
    seeked.reset();
    let after = run(&mut seeked, &pb, shape, 250, 150);
    for (i, (a, b)) in all[250..].iter().zip(&after).enumerate() {
        assert!((a - b).abs() < 1e-5, "block {}: {a} against {b}", 250 + i);
    }
    // The value is the closed form at the block's end.
    let t = 400.0 * BLOCK as f64 / SAMPLE_RATE as f64;
    let want = bpe_dsp::lfo::shape_at(bpe_dsp::lfo::LfoWave::Triangle, 3.3 * t + 40.0 / 360.0, 9);
    assert!((all[399] - want).abs() < 1e-5, "{} against {want}", all[399]);
}

#[test]
fn random_lfo_is_deterministic() {
    let kit = kit();
    let scene = new_scene(&kit.registry);
    let shape = shape_of(&scene);
    let pb = playback(&scene, &kit);
    let s = LfoSettings { shape: LfoShape::Random, rate_hz: 10.0, ..LfoSettings::default() };
    let tempo = TempoMap::default();
    let go = || {
        let mut m = LfoMod::new(3);
        m.set(&s, &tempo, 10.0);
        run(&mut m, &pb, shape, 0, 750)
    };
    let a = go();
    assert_eq!(a, go(), "the same every time");
    let mut distinct: Vec<u32> = a.iter().map(|v| v.to_bits()).collect();
    distinct.dedup();
    assert!(distinct.len() >= 9, "a new value each cycle (10 a second for 1 s): {}", distinct.len());
    // Held within a cycle: 75 blocks a cycle.
    assert_eq!(a[1], a[60]);
}

#[test]
fn restart_lfo_starts_from_phase_on_each_hit_and_fades_in() {
    let kit = kit();
    let scene = new_scene(&kit.registry);
    let shape = shape_of(&scene);
    let pb = playback(&scene, &kit);
    let s = LfoSettings {
        shape: LfoShape::Saw,
        rate_hz: 1.0,
        start: LfoStart::Restart,
        delay: 0.1,
        fade_in: 0.2,
        ..LfoSettings::default()
    };
    let tempo = TempoMap::default();
    let mut m = LfoMod::new(1);
    m.set(&s, &tempo, 1.0);
    run(&mut m, &pb, shape, 0, 100);
    let hit = HitValues::default();
    m.note_on(&NoteCx { sample: 100 * BLOCK as u64, hit: &hit, ball: None });
    let v = run(&mut m, &pb, shape, 100, 400);
    assert_eq!(v[0], 0.0, "inside the delay");
    // 0.2 s after the hit (half way through the fade), the saw is at 0.2.
    let b = (0.2 * SAMPLE_RATE as f64 / BLOCK as f64) as usize - 1;
    let t = (b + 1) as f64 * BLOCK as f64 / SAMPLE_RATE as f64;
    let want = (t as f32) * ((t as f32 - 0.1) / 0.2);
    assert!((v[b] - want).abs() < 1e-4, "{} against {want}", v[b]);
}

#[test]
fn hit_place_is_quarter_at_the_second_corner_of_a_square() {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let shape = shape_of(&scene);
    let thing = scene.things.get_mut(&shape).unwrap();
    thing.params.set("start", Value::Choice("polygon".into()));
    thing.params.set("sides", Value::Int(4));
    let pb = playback(&scene, &kit);
    let wall = pb.world().walls[&shape].outline.clone();
    // The shape's own corners, as By side counts them: the top left first, then clockwise.
    let size = wall.reach();
    let corners: Vec<Vec2> =
        bpe_core::ShapePath::even(4).points.iter().map(|&p| wall.center() + Vec2::from(p) * size).collect();
    assert_eq!(corners.len(), 4);
    assert!(corners[0].x < wall.center().x && corners[0].y < wall.center().y, "the top left: {corners:?}");
    let place = |p: Vec2| bpe_kit::things::shape::place_on_edge(pb.world(), shape, p).unwrap();
    assert!(place(corners[0]).0.abs() < 1e-4 || (place(corners[0]).0 - 1.0).abs() < 1e-4);
    let (p1, side, sides) = place(corners[1] + (corners[2] - corners[1]) * 1e-4);
    assert!((p1 - 0.25).abs() < 1e-3, "{p1}");
    assert_eq!((side, sides), (1, 4));
    let mid = (corners[2] + corners[3]) * 0.5;
    assert!((place(mid).0 - 0.625).abs() < 1e-3);
    // A point off the line counts at its nearest point.
    let centre = wall.center();
    let off = mid + (mid - centre).normalize() * 30.0;
    assert!((place(off).0 - 0.625).abs() < 1e-3);
    // Hit values read the same.
    let ev = bpe_runtime::Event {
        step: 1,
        kind: bpe_runtime::EventKind::Hit,
        a: Party { thing: ThingId(99), item: 0 },
        b: Some(Party { thing: shape, item: 0 }),
        point: mid,
        normal: Vec2::Y,
        speed: 750.0,
        at: 0.0,
    };
    let hv = hit_values(pb.world(), shape, &ev, 5);
    assert!((hv.place - 0.625).abs() < 1e-3);
    assert!((hv.side - 2.0 / 3.0).abs() < 1e-5, "side 2 of 0..3");
    assert!((hv.strength - 0.5).abs() < 1e-6);
    assert_eq!(hv.number, 0.0, "no group: ball 0");
}

#[test]
fn hit_low_high_maps_and_clamps() {
    let kit = kit();
    let scene = new_scene(&kit.registry);
    let shape = shape_of(&scene);
    let pb = playback(&scene, &kit);
    let s = HitSettings { pick: HitPick::Strength, low: 0.2, high: 0.6, ..HitSettings::default() };
    let mut m = HitMod::default();
    m.set(&s);
    let at = |m: &mut HitMod, strength: f32| {
        let hv = HitValues { strength, ..HitValues::default() };
        m.note_on(&NoteCx { sample: 0, hit: &hv, ball: None });
        run(m, &pb, shape, 0, 1)[0]
    };
    assert!((at(&mut m, 0.4) - 0.5).abs() < 1e-6);
    assert_eq!(at(&mut m, 0.1), 0.0, "under Low is nothing");
    assert_eq!(at(&mut m, 0.9), 1.0, "over High is full");
    // A curve bends the middle but keeps the ends.
    let bent = HitSettings { curve: 100.0, ..s.clone() };
    m.set(&bent);
    let v = at(&mut m, 0.4);
    assert!(v > 0.5 && v < 1.0, "{v}");
    assert_eq!(at(&mut m, 0.6), 1.0);
    // Each pick reads its own number.
    for (pick, hv, want) in [
        (HitPick::Place, HitValues { place: 0.3, ..Default::default() }, 0.3),
        (HitPick::Side, HitValues { side: 0.7, ..Default::default() }, 0.7),
        (HitPick::BallNumber, HitValues { number: 0.9, ..Default::default() }, 0.9),
        (HitPick::Random, HitValues { random: 0.15, ..Default::default() }, 0.15),
    ] {
        m.set(&HitSettings { pick, ..HitSettings::default() });
        m.note_on(&NoteCx { sample: 0, hit: &hv, ball: None });
        let v = run(&mut m, &pb, shape, 0, 1)[0];
        assert!((v - want).abs() < 1e-6, "{pick:?}: {v}");
    }
    let _ = HitSettings::from_params(&bpe_core::Params::default());
    let _ = SlotId::Sound;
}

/// The test scene's world (a ball in a hollow circle of radius 508 round the middle) with `n`
/// more balls, and the shape and the balls' thing.
fn world_with(kit: &Kit, n: usize) -> (bpe_runtime::World, ThingId, ThingId) {
    let scene = new_scene(&kit.registry);
    let shape = shape_of(&scene);
    let ball = scene.order.iter().copied().find(|id| scene.things[id].kind == "balls").unwrap();
    let resolved = resolve_scene(&scene, &kit.registry);
    let mut world = bpe_runtime::World::build(&scene, &resolved, &kit.sims);
    let b = world.bodies.get_mut(&ball).unwrap();
    for _ in 0..n {
        let (p, v, r) = (b.position[0], b.velocity[0], b.radius[0]);
        b.position.push(p);
        b.previous.push(p);
        b.start.push(p);
        b.spawn.push(p);
        b.velocity.push(v);
        b.radius.push(r);
        b.alive.push(true);
    }
    (world, shape, ball)
}

fn put(world: &mut bpe_runtime::World, ball: ThingId, i: usize, p: Vec2) {
    let b = world.bodies.get_mut(&ball).unwrap();
    b.position[i] = p;
    b.previous[i] = p;
}

fn step_value(m: &mut DistanceMod, world: &bpe_runtime::World, shape: ThingId, block: u64) -> f32 {
    let tempo = TempoMap::default();
    let cx = ModCx { start_sample: block * BLOCK as u64, tempo: &tempo, world, shape };
    m.next_block(&cx);
    m.value()
}

fn distance(s: DistanceSettings) -> DistanceMod {
    let mut m = DistanceMod::new(std::sync::Arc::new(s.heard_from.clone()));
    m.set_params(DistanceParams::of(&s));
    m
}

#[test]
fn distance_is_one_inside_near_zero_past_far() {
    let kit = kit();
    let (mut world, shape, ball) = world_with(&kit, 0);
    let mut m = distance(DistanceSettings { near: 50.0, far: 250.0, smooth: 0.0, ..DistanceSettings::default() });
    // A scripted path out from the middle.
    let mut seen = Vec::new();
    for (b, x) in [0.0f32, 40.0, 50.0, 150.0, 250.0, 400.0].into_iter().enumerate() {
        put(&mut world, ball, 0, Vec2::new(x, 0.0));
        seen.push(step_value(&mut m, &world, shape, b as u64));
    }
    assert_eq!(&seen[..3], &[1.0, 1.0, 1.0], "at Near or closer: full");
    assert!((seen[3] - 0.5).abs() < 1e-5, "half way: half (a smooth S is 0.5 in the middle): {}", seen[3]);
    assert_eq!(&seen[4..], &[0.0, 0.0], "at Far or beyond: nothing");
    // Invert flips it; Curve bends the middle only.
    let mut inv =
        distance(DistanceSettings { near: 50.0, far: 250.0, smooth: 0.0, invert: true, ..Default::default() });
    put(&mut world, ball, 0, Vec2::new(0.0, 0.0));
    assert_eq!(step_value(&mut inv, &world, shape, 0), 0.0);
    let mut bent =
        distance(DistanceSettings { near: 50.0, far: 250.0, smooth: 0.0, curve: 80.0, ..Default::default() });
    put(&mut world, ball, 0, Vec2::new(150.0, 0.0));
    assert!(step_value(&mut bent, &world, shape, 0) > 0.6);
    // Smooth follows over time: a jump from far to near takes blocks.
    let mut smooth = distance(DistanceSettings { near: 50.0, far: 250.0, smooth: 100.0, ..Default::default() });
    put(&mut world, ball, 0, Vec2::new(400.0, 0.0));
    assert_eq!(step_value(&mut smooth, &world, shape, 0), 0.0);
    put(&mut world, ball, 0, Vec2::new(0.0, 0.0));
    let first = step_value(&mut smooth, &world, shape, 1);
    assert!(first > 0.0 && first < 0.1, "{first}");
}

#[test]
fn edge_measures_to_the_outline_inside_and_out() {
    let kit = kit();
    let (mut world, shape, ball) = world_with(&kit, 0);
    let s = DistanceSettings { measure: Measure::Edge, near: 0.0, far: 100.0, smooth: 0.0, ..Default::default() };
    let mut m = distance(s);
    // The circle's line is 508 out.
    put(&mut world, ball, 0, Vec2::new(458.0, 0.0));
    let inside = step_value(&mut m, &world, shape, 0);
    put(&mut world, ball, 0, Vec2::new(0.0, 558.0));
    let outside = step_value(&mut m, &world, shape, 1);
    assert!((inside - 0.5).abs() < 1e-3 && (outside - 0.5).abs() < 1e-3, "50 px either side: {inside} {outside}");
    put(&mut world, ball, 0, Vec2::new(0.0, 0.0));
    assert_eq!(step_value(&mut m, &world, shape, 2), 0.0, "the middle is 508 from the edge");
}

#[test]
fn how_many_counts_balls_in_range() {
    let kit = kit();
    let (mut world, shape, ball) = world_with(&kit, 4);
    let s = DistanceSettings { follows: Follows::HowMany, far: 100.0, full_at: 4, smooth: 0.0, ..Default::default() };
    let mut m = distance(s);
    for i in 0..5 {
        put(&mut world, ball, i, Vec2::new(300.0, 0.0));
    }
    assert_eq!(step_value(&mut m, &world, shape, 0), 0.0);
    put(&mut world, ball, 1, Vec2::new(20.0, 0.0));
    put(&mut world, ball, 3, Vec2::new(0.0, 99.0));
    assert!((step_value(&mut m, &world, shape, 1) - 0.5).abs() < 1e-6, "two of four");
    for i in 0..5 {
        put(&mut world, ball, i, Vec2::ZERO);
    }
    assert_eq!(step_value(&mut m, &world, shape, 2), 1.0, "five is still full");
    // Average and Nearest.
    put(&mut world, ball, 0, Vec2::new(200.0, 0.0));
    let mut avg =
        distance(DistanceSettings { follows: Follows::Average, far: 100.0, smooth: 0.0, ..Default::default() });
    let mut near = distance(DistanceSettings { far: 100.0, smooth: 0.0, ..Default::default() });
    // Average of 200, 0, 0, 0, 0 is 40.
    let want = 1.0 - {
        let t = 0.4f32;
        t * t * (3.0 - 2.0 * t)
    };
    assert!((step_value(&mut avg, &world, shape, 0) - want).abs() < 1e-5);
    assert_eq!(step_value(&mut near, &world, shape, 0), 1.0);
}

#[test]
fn each_notes_ball_holds_after_the_ball_pops() {
    let kit = kit();
    let (mut world, shape, ball) = world_with(&kit, 1);
    let s = DistanceSettings { follows: Follows::EachNotesBall, far: 200.0, smooth: 0.0, ..Default::default() };
    let mut m = distance(s);
    put(&mut world, ball, 0, Vec2::new(0.0, 0.0));
    put(&mut world, ball, 1, Vec2::new(100.0, 0.0));
    let hv = HitValues::default();
    m.note_on(&NoteCx { sample: 0, hit: &hv, ball: Some(Party { thing: ball, item: 1 }) });
    assert!((step_value(&mut m, &world, shape, 0) - 0.5).abs() < 1e-5, "its own ball, not the nearest");
    put(&mut world, ball, 1, Vec2::new(150.0, 0.0));
    let before = step_value(&mut m, &world, shape, 1);
    // The ball pops: the value stays where it was.
    world.bodies.get_mut(&ball).unwrap().alive[1] = false;
    put(&mut world, ball, 1, Vec2::new(0.0, 0.0));
    for b in 2..10 {
        assert_eq!(step_value(&mut m, &world, shape, b), before);
    }
    // A ball that was never there holds 0.
    let mut lost = distance(DistanceSettings { follows: Follows::EachNotesBall, smooth: 0.0, ..Default::default() });
    lost.note_on(&NoteCx { sample: 0, hit: &hv, ball: Some(Party { thing: ball, item: 40 }) });
    assert_eq!(step_value(&mut lost, &world, shape, 0), 0.0);
}

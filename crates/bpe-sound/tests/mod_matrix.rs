//! Every modulator reaching every knob: LFOs on LFOs, loops a block late, per-note envelopes on
//! a filter, an envelope times a Distance on a gain, and a golden for a patch using them all.

use bpe_core::{Address, LayerId, ModRoute, ROUTES_KEY, Scene, Settings, SlotId, ThingId, Value, layered_key};
use bpe_kit::sound::{DISTANCE, FILTER, HIT, LFO, VoiceSettings, Wave, starter_voice};
use bpe_kit::{Kit, kit, new_scene, resolve_scene};
use bpe_runtime::{Event, EventKind, Party, World};
use bpe_sound::mods::{LfoMod, LfoParams, ModCx, Modulator};
use bpe_sound::patch::shape_voice;
use bpe_sound::voices::knobs;
use bpe_sound::{BLOCK, BlockCx, BlockHit, Engine, ShapeVoice, TempoMap, VoiceSource};
use glam::Vec2;

fn shape_of(scene: &Scene) -> ThingId {
    scene.order.iter().copied().find(|id| scene.things[id].kind == "shape").unwrap()
}

fn ball_of(scene: &Scene) -> ThingId {
    scene.order.iter().copied().find(|id| scene.things[id].kind == "balls").unwrap()
}

/// The test scene with the starter sound (a held sine) on its shape.
fn voiced(kit: &Kit) -> Scene {
    let mut scene = new_scene(&kit.registry);
    let shape = shape_of(&scene);
    let mut card = starter_voice(&kit.registry);
    let mut v = VoiceSettings::from_params(&card.params);
    v.a.wave = Wave::Sine;
    let routes = v.routes.clone();
    let mut params = v.to_params();
    params.set(ROUTES_KEY, Value::Routes(routes));
    params.set(&layered_key(LayerId(0), "sustain"), Value::Number(100.0));
    params.set(&layered_key(LayerId(0), "gate_ms"), Value::Number(50_000.0));
    card.params = params;
    scene.things.get_mut(&shape).unwrap().slots.insert(SlotId::Sound, card);
    scene
}

/// The starter's layer called `name`, switched on, with these settings.
fn turn_on(kit: &Kit, scene: &mut Scene, name: &str, set: &[(&str, Value)]) -> LayerId {
    let shape = shape_of(scene);
    let id = scene.card(shape, SlotId::Sound).unwrap().layers.iter().find(|l| l.name == name).unwrap().id;
    scene.set_layer_enabled(&kit.registry, shape, SlotId::Sound, id, true).unwrap();
    set_all(kit, scene, id, set);
    id
}

fn set_all(kit: &Kit, scene: &mut Scene, id: LayerId, set: &[(&str, Value)]) {
    let shape = shape_of(scene);
    for (k, v) in set {
        let a = Address::on(shape, SlotId::Sound, &layered_key(id, k));
        scene.set_at(&kit.registry, &a, v.clone()).unwrap();
    }
}

fn add_layer(kit: &Kit, scene: &mut Scene, kind: &str, set: &[(&str, Value)]) -> LayerId {
    let shape = shape_of(scene);
    let lane = (kind == FILTER).then_some(0);
    let id = scene.add_layer_in(&kit.registry, shape, SlotId::Sound, kind, lane).unwrap();
    set_all(kit, scene, id, set);
    id
}

fn route(kit: &Kit, scene: &mut Scene, source: LayerId, target: &str, amount: f32) {
    let shape = shape_of(scene);
    let card = scene.card_mut(shape, SlotId::Sound).unwrap();
    let mut routes = match card.params.get(ROUTES_KEY) {
        Some(Value::Routes(r)) => r.clone(),
        _ => Vec::new(),
    };
    routes.push(ModRoute { source, target: target.into(), amount, bipolar: false, invert: false });
    card.params.set(ROUTES_KEY, Value::Routes(routes));
    let _ = kit;
}

fn num(v: f64) -> Value {
    Value::Number(v)
}

fn choice(s: &str) -> Value {
    Value::Choice(s.into())
}

fn voice(scene: &Scene, kit: &Kit) -> ShapeVoice {
    let resolved = resolve_scene(scene, &kit.registry);
    let shape = shape_of(scene);
    let card = &resolved.things[&shape].slots[&SlotId::Sound];
    shape_voice(scene, shape, card, 0, 1080.0).expect("its sound is on")
}

fn world(scene: &Scene, kit: &Kit) -> World {
    let resolved = resolve_scene(scene, &kit.registry);
    World::build(scene, &resolved, &kit.sims)
}

fn hit(ball: ThingId, item: u32, shape: ThingId, offset: u32) -> BlockHit {
    BlockHit {
        offset,
        event: Event {
            step: 1,
            kind: EventKind::Hit,
            a: Party { thing: ball, item },
            b: Some(Party { thing: shape, item: 0 }),
            point: Vec2::new(508.0, 0.0),
            normal: Vec2::X,
            speed: 1500.0,
            at: 0.0,
        },
    }
}

/// Runs `v` for `blocks` blocks from block `from`, with `hits` (by block); after each block
/// `each` sees the voice. Returns the sound (left only).
fn run_with(
    v: &mut ShapeVoice,
    w: &World,
    from: u64,
    blocks: u64,
    hits: &[(u64, BlockHit)],
    mut each: impl FnMut(&ShapeVoice),
) -> Vec<f32> {
    let tempo = TempoMap::default();
    let mut out = Vec::new();
    let mut theirs = Vec::new();
    for b in from..from + blocks {
        theirs.clear();
        theirs.extend(hits.iter().filter(|(at, _)| *at == b).map(|(_, h)| h.clone()));
        let cx = BlockCx {
            start_sample: b * BLOCK as u64,
            time: (b * BLOCK as u64) as f64 / 48_000.0,
            tempo: &tempo,
            world: w,
        };
        let (mut l, mut r) = ([0.0; BLOCK], [0.0; BLOCK]);
        v.on_block(&cx, &theirs, &mut l, &mut r);
        out.extend(l);
        each(v);
    }
    out
}

fn run(v: &mut ShapeVoice, w: &World, from: u64, blocks: u64, hits: &[(u64, BlockHit)]) -> Vec<f32> {
    run_with(v, w, from, blocks, hits, |_| {})
}

/// How many times `x` crosses 0.5 going up.
fn rises(x: &[f32]) -> usize {
    x.windows(2).filter(|w| w[0] < 0.5 && w[1] >= 0.5).count()
}

#[test]
fn lfo2_on_lfo1_rate_changes_its_speed() {
    let kit = kit();
    let mut plain = voiced(&kit);
    let lfo1 = turn_on(&kit, &mut plain, "LFO 1", &[("rate_hz", num(1.0))]);
    route(&kit, &mut plain, lfo1, "a_cents", 0.1);
    let mut moved = plain.clone();
    // LFO 2: a square held high for 50 s, pushing LFO 1's rate up by a tenth of 0.01..40 Hz.
    let lfo2 = turn_on(&kit, &mut moved, "LFO 2", &[("shape", choice("square")), ("rate_hz", num(0.01))]);
    route(&kit, &mut moved, lfo2, &layered_key(lfo1, "rate_hz"), 0.1);
    let count = |scene: &Scene| {
        let w = world(scene, &kit);
        let mut v = voice(scene, &kit);
        let mut values = Vec::new();
        run_with(&mut v, &w, 0, 1500, &[], |v| values.push(v.mod_value(lfo1).unwrap()));
        rises(&values)
    };
    let (slow, fast) = (count(&plain), count(&moved));
    assert!((1..=2).contains(&slow), "1 Hz for 2 s: {slow}");
    assert!((9..=11).contains(&fast), "about 5 Hz for 2 s: {fast}");
}

#[test]
fn a_mod_loop_runs_one_block_late() {
    let kit = kit();
    let mut scene = voiced(&kit);
    let lfo1 = turn_on(&kit, &mut scene, "LFO 1", &[("rate_hz", num(1.0)), ("shape", choice("triangle"))]);
    let lfo2 = turn_on(&kit, &mut scene, "LFO 2", &[("rate_hz", num(2.0)), ("shape", choice("saw"))]);
    route(&kit, &mut scene, lfo1, "a_cents", 0.5);
    route(&kit, &mut scene, lfo2, &layered_key(lfo1, "rate_hz"), 0.05);
    route(&kit, &mut scene, lfo1, &layered_key(lfo2, "rate_hz"), 0.05);
    let w = world(&scene, &kit);
    let mut v = voice(&scene, &kit);
    let mut got = Vec::new();
    run_with(&mut v, &w, 0, 400, &[], |v| got.push((v.mod_value(lfo1).unwrap(), v.mod_value(lfo2).unwrap())));
    // By hand: LFO 1 (first in the card) hears LFO 2 as it was a block ago; LFO 2 hears LFO 1 now.
    let (s1, s2) =
        (bpe_sound::mods::lfo::seed_of(shape_of(&scene), lfo1), bpe_sound::mods::lfo::seed_of(shape_of(&scene), lfo2));
    let (mut a, mut b) = (LfoMod::new(s1), LfoMod::new(s2));
    let span = 40.0 - 0.01;
    let tempo = TempoMap::default();
    let mut vb = 0.0f32;
    for (blk, (ga, gb)) in got.iter().enumerate() {
        let cx = ModCx { start_sample: blk as u64 * BLOCK as u64, tempo: &tempo, world: &w, shape: shape_of(&scene) };
        let ra = (1.0 + 0.05 * vb as f64 * span).clamp(0.01, 40.0);
        a.set_params(LfoParams { rate_hz: ra, wave: bpe_dsp::lfo::LfoWave::Triangle, ..LfoParams::default() });
        a.next_block(&cx);
        let va = a.value();
        let rb = (2.0 + 0.05 * va as f64 * span).clamp(0.01, 40.0);
        b.set_params(LfoParams { rate_hz: rb, wave: bpe_dsp::lfo::LfoWave::Saw, ..LfoParams::default() });
        b.next_block(&cx);
        vb = b.value();
        assert!((va - ga).abs() < 1e-4 && (vb - gb).abs() < 1e-4, "block {blk}: ({ga}, {gb}) against ({va}, {vb})");
    }
}

#[test]
fn per_note_adsr_on_filter_cutoff_is_independent_per_voice() {
    let kit = kit();
    let mut scene = voiced(&kit);
    let filter = add_layer(&kit, &mut scene, FILTER, &[("cutoff", num(200.0))]);
    let adsr2 = turn_on(
        &kit,
        &mut scene,
        "ADSR 2",
        &[("attack", num(100.0)), ("attack_curve", num(0.0)), ("sustain", num(100.0)), ("gate_ms", num(10_000.0))],
    );
    route(&kit, &mut scene, adsr2, &layered_key(filter, "cutoff"), 0.5);
    let (shape, ball) = (shape_of(&scene), ball_of(&scene));
    let w = world(&scene, &kit);
    let mut v = voice(&scene, &kit);
    v.watch(knobs::chain(0, knobs::CUTOFF));
    // One note, then a second 50 ms later.
    run(&mut v, &w, 0, 38, &[(0, hit(ball, 0, shape, 0))]);
    run(&mut v, &w, 38, 1, &[(38, hit(ball, 1, shape, 0))]);
    let mut seen = v.watched();
    seen.sort_by(|a, b| b.1.total_cmp(&a.1));
    assert_eq!(seen.len(), 2);
    // The first note's envelope is half way up, the second's just starting.
    assert!(seen[0].1 > 4000.0, "{seen:?}");
    assert!(seen[1].1 < 1000.0, "{seen:?}");
    run(&mut v, &w, 39, 200, &[]);
    let late = v.watched();
    assert!(
        late.iter().all(|(_, c)| (*c - (200.0 + 0.5 * (20_000.0 - 20.0))).abs() < 1.0),
        "both at the top: {late:?}"
    );
}

#[test]
fn adsr_times_distance_on_gain_multiplies() {
    let kit = kit();
    let mut plain = voiced(&kit);
    let mut both = plain.clone();
    let shape = shape_of(&plain);
    // Distance 1 on Osc A's gain (and B's), its ball held where it reads one half.
    let d = turn_on(&kit, &mut both, "Distance 1", &[("near", num(0.0)), ("far", num(400.0)), ("smooth", num(0.0))]);
    route(&kit, &mut both, d, "a_gain", 1.0);
    let measure = |scene: &Scene| {
        let mut w = world(scene, &kit);
        let ball = ball_of(scene);
        let b = w.bodies.get_mut(&ball).unwrap();
        b.position[0] = Vec2::new(200.0, 0.0);
        let mut v = voice(scene, &kit);
        let out = run(&mut v, &w, 0, 400, &[(0, hit(ball, 0, shape, 0))]);
        out[20_000..25_000].iter().fold(0.0f32, |m, x| m.max(x.abs()))
    };
    let (alone, times) = (measure(&plain), measure(&both));
    assert!(alone > 0.1);
    assert!((times / alone - 0.5).abs() < 0.01, "{times} against {alone}");
    plain.name.clear();
}

#[test]
fn modulators_patch_golden() {
    let kit = kit();
    let mut scene = voiced(&kit);
    let filter = add_layer(&kit, &mut scene, FILTER, &[("cutoff", num(900.0)), ("reso", num(40.0))]);
    let lfo1 = turn_on(&kit, &mut scene, "LFO 1", &[("rate_hz", num(3.0))]);
    let lfo2 = turn_on(&kit, &mut scene, "LFO 2", &[("shape", choice("smooth_random")), ("start", choice("per_note"))]);
    let d = turn_on(&kit, &mut scene, "Distance 1", &[("far", num(600.0))]);
    let h = add_layer(&kit, &mut scene, HIT, &[("pick", choice("place"))]);
    route(&kit, &mut scene, lfo1, "a_cents", 0.2);
    route(&kit, &mut scene, lfo2, &layered_key(filter, "cutoff"), 0.3);
    route(&kit, &mut scene, d, "a_gain", 1.0);
    route(&kit, &mut scene, h, "out_pan", 0.5);
    let _ = (DISTANCE, LFO);
    let resolved = resolve_scene(&scene, &kit.registry);
    let mut e = Engine::new(&scene, &resolved, &kit);
    let mut out = Vec::new();
    let mut block = [0.0f32; BLOCK * 2];
    for _ in 0..(3.0 * 48_000.0 / BLOCK as f64) as usize {
        e.render(&mut block);
        out.extend_from_slice(&block);
    }
    assert!(out.iter().any(|v| v.abs() > 0.01), "it plays");
    assert!(out.iter().all(|v| v.is_finite()));
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for v in &out {
        hash ^= v.to_bits() as u64;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    let line = format!("modulators {hash:016x}\n");
    let file = if cfg!(debug_assertions) { "tests/golden-sound-mods-debug.txt" } else { "tests/golden-sound-mods.txt" };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file);
    if std::env::var_os("BLESS_GOLDEN").is_some() {
        std::fs::write(&path, &line).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).expect("the golden file is missing; run with BLESS_GOLDEN=1");
    assert_eq!(line, expected, "the modulators patch sounds different");
}

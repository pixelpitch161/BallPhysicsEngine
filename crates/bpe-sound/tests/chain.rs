//! Oscillator layer chains in a shape's voice: order, solo, loops, fallbacks, bypass, speed and
//! a golden.

use bpe_core::{Address, LayerId, Scene, Settings, SlotId, ThingId, Value, layered_key};
use bpe_kit::sound::{DISTORTION, FILTER, MODULATION, VoiceSettings, Wave, starter_voice};
use bpe_kit::{Kit, kit, new_scene, resolve_scene};
use bpe_runtime::{Event, EventKind, Party, Playback, World};
use bpe_sound::chain_graph::loops_of;
use bpe_sound::patch::shape_voice;
use bpe_sound::{BLOCK, BlockCx, BlockHit, Engine, ShapeVoice, TempoMap, VoiceSource};
use glam::Vec2;
use std::time::Instant;

fn shape_of(scene: &Scene) -> ThingId {
    scene.order.iter().copied().find(|id| scene.things[id].kind == "shape").unwrap()
}

fn ball_of(scene: &Scene) -> ThingId {
    scene.order.iter().copied().find(|id| scene.things[id].kind == "balls").unwrap()
}

/// The test scene with the starter sound on its shape (a held note: sustain 100 %), its card's
/// settings changed by `edit`.
fn voiced(kit: &Kit, edit: impl FnOnce(&mut VoiceSettings)) -> Scene {
    let mut scene = new_scene(&kit.registry);
    let shape = shape_of(&scene);
    let mut card = starter_voice(&kit.registry);
    let mut v = VoiceSettings::from_params(&card.params);
    v.a.wave = Wave::Saw;
    edit(&mut v);
    let mut params = v.to_params();
    params.set(&layered_key(LayerId(0), "sustain"), Value::Number(100.0));
    params.set(&layered_key(LayerId(0), "gate_ms"), Value::Number(50_000.0));
    card.params = params;
    scene.things.get_mut(&shape).unwrap().slots.insert(SlotId::Sound, card);
    scene
}

/// Adds a chain layer of `kind` to oscillator `lane` with these settings.
fn add(kit: &Kit, scene: &mut Scene, lane: u8, kind: &str, set: &[(&str, Value)]) -> LayerId {
    let shape = shape_of(scene);
    let id = scene.add_layer_in(&kit.registry, shape, SlotId::Sound, kind, Some(lane)).unwrap();
    for (k, v) in set {
        let a = Address::on(shape, SlotId::Sound, &layered_key(id, k));
        scene.set_at(&kit.registry, &a, v.clone()).unwrap();
    }
    id
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

fn hit(ball: ThingId, item: u32, shape: ThingId, offset: u32) -> BlockHit {
    BlockHit {
        offset,
        event: Event {
            step: 1,
            kind: EventKind::Hit,
            a: Party { thing: ball, item },
            b: Some(Party { thing: shape, item: 0 }),
            point: Vec2::ZERO,
            normal: Vec2::Y,
            speed: 1500.0,
            at: 0.0,
        },
    }
}

/// Runs `v` for `blocks` blocks from block `from` with `hits` (by block); its sound, left only.
fn run(v: &mut ShapeVoice, w: &World, from: u64, blocks: u64, hits: &[(u64, BlockHit)]) -> Vec<f32> {
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
            playing: true,
        };
        let (mut l, mut r) = ([0.0; BLOCK], [0.0; BLOCK]);
        v.on_block(&cx, &theirs, &mut l, &mut r);
        out.extend(l);
    }
    out
}

/// One note played on `scene`'s shape for `blocks` blocks.
fn play(scene: &Scene, kit: &Kit, blocks: u64) -> Vec<f32> {
    let (shape, ball) = (shape_of(scene), ball_of(scene));
    let resolved = resolve_scene(scene, &kit.registry);
    let p = Playback::new(scene, &resolved, kit.sims.clone());
    let mut v = voice(scene, kit);
    run(&mut v, p.world(), 0, blocks, &[(0, hit(ball, 0, shape, 3))])
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn card(scene: &Scene) -> &bpe_core::Slot {
    scene.card(shape_of(scene), SlotId::Sound).unwrap()
}

#[test]
fn layers_run_top_to_bottom() {
    let kit = kit();
    let filter = [("cutoff", num(400.0)), ("slope", choice("db24"))];
    let dist = [("kind", choice("hard_clip")), ("drive", num(30.0))];
    let mut a = voiced(&kit, |_| {});
    add(&kit, &mut a, 0, FILTER, &filter);
    add(&kit, &mut a, 0, DISTORTION, &dist);
    let mut b = voiced(&kit, |_| {});
    add(&kit, &mut b, 0, DISTORTION, &dist);
    add(&kit, &mut b, 0, FILTER, &filter);
    let (x, y) = (play(&a, &kit, 200), play(&b, &kit, 200));
    let diff: Vec<f32> = x.iter().zip(&y).map(|(p, q)| p - q).collect();
    assert!(rms(&x) > 0.05 && rms(&y) > 0.01, "both sound: {} {}", rms(&x), rms(&y));
    assert!(rms(&diff) > 0.2 * rms(&x), "the order is heard: {} vs {}", rms(&diff), rms(&x));
    // Filtering last leaves the clipped sound dark; clipping last brings the highs back.
    let edge = |s: &[f32]| s.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>();
    assert!(edge(&y) < edge(&x), "filter last is darker");
}

#[test]
fn solo_cuts_the_chain_at_its_layer() {
    let kit = kit();
    let mut soloed = voiced(&kit, |_| {});
    add(&kit, &mut soloed, 0, FILTER, &[("cutoff", num(300.0)), ("solo", Value::Bool(true))]);
    add(&kit, &mut soloed, 0, DISTORTION, &[("drive", num(36.0))]);
    let mut cut = voiced(&kit, |_| {});
    add(&kit, &mut cut, 0, FILTER, &[("cutoff", num(300.0))]);
    let heard = play(&soloed, &kit, 100);
    assert!(rms(&heard) > 0.01);
    assert_eq!(heard, play(&cut, &kit, 100), "a soloed layer plays the chain down to it");
}

/// A's FM reads B after B's filter; B's FM, above that filter, reads A's FM: a loop.
fn looping(kit: &Kit) -> (Scene, LayerId) {
    let mut s = voiced(kit, |v| {
        v.b.on = true;
        v.b.wave = Wave::Sine;
    });
    let a_fm = add(kit, &mut s, 0, MODULATION, &[("amount", num(100.0))]);
    let b_fm = add(kit, &mut s, 1, MODULATION, &[("amount", num(100.0)), ("source", Value::Text(a_fm.0.to_string()))]);
    let b_filter = add(kit, &mut s, 1, FILTER, &[("reso", num(90.0)), ("cutoff", num(900.0))]);
    let shape = shape_of(&s);
    let src = Address::on(shape, SlotId::Sound, &layered_key(a_fm, "source"));
    s.set_at(&kit.registry, &src, Value::Text(b_filter.0.to_string())).unwrap();
    (s, b_fm)
}

#[test]
fn a_loop_renders_finite() {
    let kit = kit();
    let (scene, _) = looping(&kit);
    let out = play(&scene, &kit, 10 * 48_000 / BLOCK as u64);
    assert!(out.iter().all(|v| v.is_finite() && v.abs() <= 4.0), "finite and bounded");
    assert!(rms(&out[48_000..]) > 0.01, "it sounds");
}

#[test]
fn loop_is_reported() {
    let kit = kit();
    let (scene, b_fm) = looping(&kit);
    assert_eq!(loops_of(card(&scene)), vec![b_fm], "B's FM, which closes the circle, reads A's a sample late");
    let resolved = resolve_scene(&scene, &kit.registry);
    let shape = shape_of(&scene);
    let v = shape_voice(&scene, shape, &resolved.things[&shape].slots[&SlotId::Sound], 0, 1080.0).unwrap();
    assert!(v.spec().chain.per_sample);
    assert_eq!(v.spec().chain.loops, vec![b_fm]);
}

#[test]
fn deleted_source_falls_back() {
    let kit = kit();
    let mut s = voiced(&kit, |v| v.b.on = true);
    let b_filter = add(&kit, &mut s, 1, FILTER, &[("cutoff", num(500.0))]);
    let fm =
        add(&kit, &mut s, 0, MODULATION, &[("amount", num(60.0)), ("source", Value::Text(b_filter.0.to_string()))]);
    let shape = shape_of(&s);
    s.remove_layer(&kit.registry, shape, SlotId::Sound, b_filter).unwrap();
    let fell_back = play(&s, &kit, 100);
    assert!(fell_back.iter().all(|v| v.is_finite()));
    // The same as pointing it at its own oscillator.
    let a = Address::on(shape, SlotId::Sound, &layered_key(fm, "source"));
    s.set_at(&kit.registry, &a, Value::Text("a".into())).unwrap();
    assert_eq!(fell_back, play(&s, &kit, 100));
}

#[test]
fn mix_zero_bypasses() {
    let kit = kit();
    let plain = play(&voiced(&kit, |_| {}), &kit, 100);
    assert!(rms(&plain) > 0.01);
    let mut f = voiced(&kit, |_| {});
    add(&kit, &mut f, 0, FILTER, &[("cutoff", num(200.0)), ("mix", num(0.0))]);
    add(
        &kit,
        &mut f,
        0,
        MODULATION,
        &[("kind", choice("ring")), ("source", Value::Text("a".into())), ("mix", num(0.0))],
    );
    assert_eq!(play(&f, &kit, 100), plain, "Mix 0 lets the sound through untouched");
    // A distortion at Mix 0 passes the sound through, held back by its oversampling.
    let mut d = voiced(&kit, |_| {});
    add(&kit, &mut d, 0, DISTORTION, &[("drive", num(40.0)), ("mix", num(0.0))]);
    let late = bpe_dsp::Shaper::new(bpe_dsp::ShaperType::SoftClip, 4).latency();
    let heard = play(&d, &kit, 100);
    let worst = (late..heard.len()).map(|i| (heard[i] - plain[i - late]).abs()).fold(0.0f32, f32::max);
    assert!(worst < 1e-6, "{worst}");
}

#[test]
fn an_off_oscillator_still_modulates() {
    let kit = kit();
    let plain = play(&voiced(&kit, |_| {}), &kit, 100);
    // Osc B is off: it isn't heard, but A's FM (from Osc B, the default) still hears it.
    let mut s = voiced(&kit, |v| v.b.wave = Wave::Sine);
    add(&kit, &mut s, 0, MODULATION, &[("amount", num(80.0))]);
    let fm = play(&s, &kit, 100);
    let diff: Vec<f32> = fm.iter().zip(&plain).map(|(a, b)| a - b).collect();
    assert!(rms(&diff) > 0.1 * rms(&plain), "B moves A: {} vs {}", rms(&diff), rms(&plain));
    // With nothing tapping it, B makes no difference while off.
    let mut quiet = voiced(&kit, |v| v.b.wave = Wave::Sine);
    add(&kit, &mut quiet, 0, MODULATION, &[("amount", num(80.0)), ("source", Value::Text("a".into()))]);
    let b_on = Address::on(shape_of(&quiet), SlotId::Sound, "b_on");
    assert_eq!(quiet.value_at_time(&kit.registry, &b_on, 0.0), Some(Value::Bool(false)));
}

#[test]
fn an_off_oscillator_keeps_its_sub_as_a_source() {
    let kit = kit();
    // Osc B is off but rings A; its square sub must be part of what rings A.
    let with = |sub: bpe_kit::sound::SubWave| {
        let mut s = voiced(&kit, |v| {
            v.b.wave = Wave::Sine;
            v.b.sub = sub;
            v.b.sub_level = 0.0;
        });
        add(&kit, &mut s, 0, MODULATION, &[("kind", choice("ring")), ("amount", num(100.0))]);
        play(&s, &kit, 60)
    };
    let off = with(bpe_kit::sound::SubWave::Off);
    let square = with(bpe_kit::sound::SubWave::Square);
    let diff: Vec<f32> = off.iter().zip(&square).map(|(a, b)| a - b).collect();
    assert!(rms(&diff) > 0.05 * rms(&off), "B's sub is heard through the ring: {}", rms(&diff));
}

#[test]
fn a_patch_change_keeps_a_ringing_filter() {
    let kit = kit();
    let mut s = voiced(&kit, |_| {});
    let f = add(&kit, &mut s, 0, FILTER, &[("cutoff", num(800.0)), ("reso", num(50.0))]);
    let (shape, ball) = (shape_of(&s), ball_of(&s));
    let resolved = resolve_scene(&s, &kit.registry);
    let p = Playback::new(&s, &resolved, kit.sims.clone());
    let mut v = voice(&s, &kit);
    let before = run(&mut v, p.world(), 0, 50, &[(0, hit(ball, 0, shape, 0))]);
    // Turn the cutoff while it plays: the new voice takes over the old one's memories.
    let a = Address::on(shape, SlotId::Sound, &layered_key(f, "cutoff"));
    s.set_at(&kit.registry, &a, num(900.0)).unwrap();
    let mut next = voice(&s, &kit);
    next.adopt(&mut v);
    let after = run(&mut next, p.world(), 50, 5, &[]);
    let jump = (after[0] - before[before.len() - 1]).abs();
    let step = before.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
    assert!(jump <= 2.0 * step, "no click: {jump} against steps of {step}");
}

#[test]
#[cfg_attr(debug_assertions, ignore = "timing needs a release build")]
fn twenty_layers_16_voices_stay_fast() {
    // Ruled bar (decisions log): 0.5x real time for all 16 voices (each voice's 20 layers run
    // about 11x real time on a 2.1 GHz server core). Seven 4x distortions a voice set the pace.
    let kit = kit();
    let mut s = voiced(&kit, |v| v.b.on = true);
    for i in 0..20 {
        match i % 3 {
            0 => {
                add(&kit, &mut s, 0, FILTER, &[("slope", choice("db24")), ("cutoff", num(3000.0)), ("reso", num(30.0))])
            }
            1 => add(&kit, &mut s, 0, DISTORTION, &[("drive", num(6.0))]),
            _ => add(&kit, &mut s, 0, MODULATION, &[("amount", num(20.0))]),
        };
    }
    let (shape, ball) = (shape_of(&s), ball_of(&s));
    let resolved = resolve_scene(&s, &kit.registry);
    let p = Playback::new(&s, &resolved, kit.sims.clone());
    let mut v = voice(&s, &kit);
    let hits: Vec<(u64, BlockHit)> = (0..16).map(|i| (0, hit(ball, i, shape, 0))).collect();
    run(&mut v, p.world(), 0, 10, &hits);
    assert_eq!(v.sounding(), 16);
    let t = Instant::now();
    run(&mut v, p.world(), 10, 750, &[]);
    let factor = 1.0 / t.elapsed().as_secs_f64();
    println!("20 layers x 16 voices: {factor:.1}x real time");
    assert!(factor >= 0.5, "only {factor:.1}x real time");
}

#[test]
#[cfg_attr(debug_assertions, ignore = "timing needs a release build")]
fn heavy_patch_with_a_filter_and_a_distortion_each_stays_fast() {
    // The spec's heavy patch: 16 voices, 16 unison on both oscillators, a filter and a
    // distortion on each. Ruled bar (decisions log): 1.5x real time on a 2.1 GHz server core;
    // the 32 distortions at 4x take most of it (the patch without layers runs 12x).
    let kit = kit();
    let mut s = voiced(&kit, |v| {
        v.a.unison = 16;
        v.b.on = true;
        v.b.unison = 16;
    });
    for lane in 0..2 {
        add(&kit, &mut s, lane, FILTER, &[("slope", choice("db24")), ("reso", num(40.0))]);
        add(&kit, &mut s, lane, DISTORTION, &[("kind", choice("tube"))]);
    }
    let (shape, ball) = (shape_of(&s), ball_of(&s));
    let resolved = resolve_scene(&s, &kit.registry);
    let p = Playback::new(&s, &resolved, kit.sims.clone());
    let mut v = voice(&s, &kit);
    let hits: Vec<(u64, BlockHit)> = (0..16).map(|i| (0, hit(ball, i, shape, 0))).collect();
    run(&mut v, p.world(), 0, 10, &hits);
    let t = Instant::now();
    run(&mut v, p.world(), 10, 750, &[]);
    let factor = 1.0 / t.elapsed().as_secs_f64();
    println!("heavy patch with layers: {factor:.1}x real time");
    assert!(factor >= 1.5, "only {factor:.1}x real time");
}

#[test]
fn chain_patch_golden() {
    let kit = kit();
    let mut s = voiced(&kit, |v| {
        v.b.on = true;
        v.b.wave = Wave::Square;
        v.b.level = -12.0;
    });
    add(&kit, &mut s, 0, MODULATION, &[("amount", num(40.0))]);
    add(&kit, &mut s, 0, FILTER, &[("kind", choice("ladder")), ("cutoff", num(1200.0)), ("reso", num(60.0))]);
    add(&kit, &mut s, 1, DISTORTION, &[("kind", choice("wavefold")), ("drive", num(9.0))]);
    add(&kit, &mut s, 1, FILTER, &[("kind", choice("comb")), ("key_track", num(100.0)), ("reso", num(70.0))]);
    let resolved = resolve_scene(&s, &kit.registry);
    let render = || {
        let mut e = Engine::new(&s, &resolved, &kit);
        let mut out = Vec::new();
        let mut block = [0.0f32; BLOCK * 2];
        for _ in 0..(2 * 48_000 / BLOCK) {
            e.render(&mut block);
            out.extend_from_slice(&block);
        }
        out
    };
    let out = render();
    assert!(out.iter().any(|v| v.abs() > 0.01), "the hits play");
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for v in &out {
        h ^= v.to_bits() as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    let line = format!("chain {h:016x}\n");
    let file =
        if cfg!(debug_assertions) { "tests/golden-sound-chain-debug.txt" } else { "tests/golden-sound-chain.txt" };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file);
    if std::env::var_os("BLESS_GOLDEN").is_some() {
        std::fs::write(&path, &line).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).expect("the golden file is missing; run with BLESS_GOLDEN=1");
    assert_eq!(line, expected, "the chain patch sounds different");
    assert_eq!(render(), out, "the same every time");
}

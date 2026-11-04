//! A shape's voice: hits on the test scene's shape play its starter patch, and the voice keeps
//! to its limits, filters and fades.

use bpe_core::{LayerId, Params, Scene, Settings, SlotId, Target, ThingId, layered_key};
use bpe_kit::sound::{AdsrSettings, GateMode, PlayMode, VoiceSettings, Wave, starter_voice};
use bpe_kit::{Kit, kit, new_scene, resolve_scene};
use bpe_runtime::{Event, EventKind, Party, Playback, World};
use bpe_sound::patch::shape_voice;
use bpe_sound::{BLOCK, BlockCx, BlockHit, Engine, ShapeVoice, SoundPatch, TempoMap, VoiceSource, note_hz};
use glam::Vec2;
use std::time::Instant;

fn shape_of(scene: &Scene) -> ThingId {
    scene.order.iter().copied().find(|id| scene.things[id].kind == "shape").unwrap()
}

fn ball_of(scene: &Scene) -> ThingId {
    scene.order.iter().copied().find(|id| scene.things[id].kind == "balls").unwrap()
}

/// The test scene (a ball dropping into a hollow circle) with the starter sound on its shape,
/// changed by `edit` (the card's settings and ADSR 1's).
fn voiced(kit: &Kit, edit: impl FnOnce(&mut VoiceSettings, &mut AdsrSettings)) -> Scene {
    let mut scene = new_scene(&kit.registry);
    let shape = shape_of(&scene);
    let mut card = starter_voice(&kit.registry);
    let mut v = VoiceSettings::from_params(&card.params);
    let mut a = AdsrSettings::default();
    edit(&mut v, &mut a);
    let mut params: Params = v.to_params();
    for (k, val) in a.to_params().iter() {
        params.set(&layered_key(LayerId(0), k), val.clone());
    }
    card.params = params;
    scene.things.get_mut(&shape).unwrap().slots.insert(SlotId::Sound, card);
    scene
}

fn voice(scene: &Scene, kit: &Kit) -> ShapeVoice {
    let resolved = resolve_scene(scene, &kit.registry);
    let shape = shape_of(scene);
    let card = &resolved.things[&shape].slots[&SlotId::Sound];
    shape_voice(scene, shape, card, 0, 1080.0).expect("its sound is on")
}

fn world(scene: &Scene, kit: &Kit) -> Playback {
    let resolved = resolve_scene(scene, &kit.registry);
    Playback::new(scene, &resolved, kit.sims.clone())
}

fn hit(ball: ThingId, item: u32, shape: ThingId, offset: u32, speed: f32) -> BlockHit {
    BlockHit {
        offset,
        event: Event {
            step: 1,
            kind: EventKind::Hit,
            a: Party { thing: ball, item },
            b: Some(Party { thing: shape, item: 0 }),
            point: Vec2::ZERO,
            normal: Vec2::Y,
            speed,
            at: 0.0,
        },
    }
}

/// Runs `v` for `blocks` blocks from block `from`, with `hits` (by block), and returns its
/// sound (left only).
fn run(v: &mut ShapeVoice, w: &World, from: u64, blocks: u64, hits: &[(u64, BlockHit)]) -> Vec<f32> {
    let tempo = TempoMap::default();
    let mut out = Vec::new();
    let mut theirs = Vec::new();
    for b in from..from + blocks {
        theirs.clear();
        theirs.extend(hits.iter().filter(|(at, _)| *at == b).map(|(_, h)| h.clone()));
        let cx = BlockCx { start_sample: b * BLOCK as u64, time: (b * BLOCK as u64) as f64 / 48_000.0, tempo: &tempo, world: w };
        let (mut l, mut r) = ([0.0; BLOCK], [0.0; BLOCK]);
        v.on_block(&cx, &theirs, &mut l, &mut r);
        out.extend(l);
    }
    out
}

fn render(scene: &Scene, kit: &Kit, seconds: f64) -> Vec<f32> {
    let resolved = resolve_scene(scene, &kit.registry);
    let mut e = Engine::new(scene, &resolved, kit);
    let blocks = (seconds * 48_000.0 / BLOCK as f64).ceil() as usize;
    let mut out = Vec::with_capacity(blocks * BLOCK * 2);
    let mut block = [0.0f32; BLOCK * 2];
    for _ in 0..blocks {
        e.render(&mut block);
        out.extend_from_slice(&block);
    }
    out
}

/// The strongest frequency in `x`, read finely from a zero-padded FFT.
fn peak_hz(x: &[f32]) -> f64 {
    let n = 1 << 17;
    let (mut re, mut im) = (vec![0.0f64; n], vec![0.0f64; n]);
    for (i, v) in x.iter().enumerate() {
        let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / x.len() as f64).cos();
        re[i] = *v as f64 * w;
    }
    bpe_dsp::tables::fft(&mut re, &mut im, false);
    let k = (1..n / 2).max_by(|&a, &b| (re[a].hypot(im[a])).total_cmp(&re[b].hypot(im[b]))).unwrap();
    k as f64 * 48_000.0 / n as f64
}

#[test]
fn starter_patch_renders_its_golden() {
    let kit = kit();
    let scene = voiced(&kit, |_, _| {});
    let out = render(&scene, &kit, 2.0);
    assert!(out.iter().any(|v| v.abs() > 0.01), "the ball's hits play");
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for v in &out {
        h ^= v.to_bits() as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    let line = format!("starter {h:016x}\n");
    let file = if cfg!(debug_assertions) { "tests/golden-sound-debug.txt" } else { "tests/golden-sound.txt" };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file);
    if std::env::var_os("BLESS_GOLDEN").is_some() {
        std::fs::write(&path, &line).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).expect("the golden file is missing; run with BLESS_GOLDEN=1");
    assert_eq!(line, expected, "the starter patch sounds different");
    assert_eq!(render(&scene, &kit, 2.0), out, "the same every time");
}

#[test]
fn a_hit_plays_c5() {
    let kit = kit();
    let scene = voiced(&kit, |_, _| {});
    let shape = shape_of(&scene);
    let mut p = world(&scene, &kit);
    p.seek(5.0);
    let rate = p.world().steps_per_second as f64;
    let first = p
        .log()
        .events()
        .iter()
        .find(|e| e.kind == EventKind::Hit && e.b.is_some_and(|b| b.thing == shape))
        .expect("the ball hits the shape")
        .clone();
    let at = (((first.step - 1) as f64 + first.at as f64) * 48_000.0 / rate).round() as usize;
    let out = render(&scene, &kit, at as f64 / 48_000.0 + 0.2);
    let left: Vec<f32> = out.chunks(2).map(|f| f[0]).collect();
    assert!(left[..at].iter().all(|v| *v == 0.0), "silent before the hit");
    let hz = peak_hz(&left[at..at + 2_400]);
    assert!((hz / 261.626 - 1.0).abs() < 0.01, "plays {hz:.1} Hz, not C5's 261.6");
    assert!((note_hz(60.0) - 261.626).abs() < 0.01);
}

#[test]
fn a_burst_of_hits_stays_within_the_voice_limit() {
    let kit = kit();
    let scene = voiced(&kit, |v, a| {
        v.a.unison = 4;
        a.sustain = 100.0;
        a.gate_ms = 2_000.0;
    });
    let (shape, ball) = (shape_of(&scene), ball_of(&scene));
    let p = world(&scene, &kit);
    let mut v = voice(&scene, &kit);
    let hits: Vec<(u64, BlockHit)> = (0..200).map(|i| (0, hit(ball, i, shape, (i % 64) as u32, 1500.0))).collect();
    run(&mut v, p.world(), 0, 1, &hits);
    assert!(v.sounding() <= 16, "{} notes sound", v.sounding());
    assert_eq!(v.sounding(), 16);
    let t = Instant::now();
    run(&mut v, p.world(), 1, 1, &[]);
    let took = t.elapsed();
    if !cfg!(debug_assertions) {
        assert!(took.as_secs_f64() < 0.002, "a block took {took:?}");
    }
    let out = run(&mut v, p.world(), 2, 10, &[]);
    assert!(out.iter().all(|s| s.is_finite()));
}

#[test]
fn mono_glide_reaches_pitch_in_its_time() {
    let kit = kit();
    let scene = voiced(&kit, |v, a| {
        v.a.wave = Wave::Sine;
        v.play_mode = PlayMode::Mono;
        v.play_glide = 100.0;
        a.sustain = 100.0;
        a.gate_ms = 5_000.0;
    });
    let p = world(&scene, &kit);
    let mut v = voice(&scene, &kit);
    v.audition(60);
    run(&mut v, p.world(), 0, 150, &[]);
    v.audition(72);
    // Half way through the glide it is between the notes; past its time it is on the new one.
    let during = run(&mut v, p.world(), 150, 75, &[]);
    let after = run(&mut v, p.world(), 225, 75, &[]);
    assert_eq!(v.sounding(), 1, "one note at a time");
    let mid = peak_hz(&during[during.len() / 2 - 600..during.len() / 2 + 600]);
    assert!(mid > note_hz(61.0) as f64 && mid < note_hz(71.0) as f64, "{mid}");
    let end = peak_hz(&after[..2_400]);
    assert!((end / note_hz(72.0) as f64 - 1.0).abs() < 0.01, "{end}");
}

#[test]
fn retrigger_guard_drops_a_second_hit_within_30ms() {
    let kit = kit();
    let scene = voiced(&kit, |_, a| {
        a.sustain = 100.0;
        a.gate_ms = 5_000.0;
    });
    let (shape, ball) = (shape_of(&scene), ball_of(&scene));
    let p = world(&scene, &kit);
    let mut v = voice(&scene, &kit);
    // 10 ms apart: one note. 40 ms on: a second.
    let hits = [(0, hit(ball, 0, shape, 0, 1500.0)), (7, hit(ball, 0, shape, 32, 1500.0))];
    run(&mut v, p.world(), 0, 10, &hits);
    assert_eq!(v.sounding(), 1);
    run(&mut v, p.world(), 10, 30, &[(30, hit(ball, 0, shape, 0, 1500.0))]);
    assert_eq!(v.sounding(), 2);
    // Another ball isn't held back by the first one's guard.
    run(&mut v, p.world(), 40, 2, &[(40, hit(ball, 1, shape, 0, 1500.0))]);
    assert_eq!(v.sounding(), 3);
}

#[test]
fn min_strength_drops_soft_hits() {
    let kit = kit();
    let scene = voiced(&kit, |v, a| {
        v.play_min_strength = 50.0;
        a.sustain = 100.0;
        a.gate_ms = 5_000.0;
    });
    let (shape, ball) = (shape_of(&scene), ball_of(&scene));
    let p = world(&scene, &kit);
    let mut v = voice(&scene, &kit);
    run(&mut v, p.world(), 0, 2, &[(0, hit(ball, 0, shape, 0, 300.0))]);
    assert_eq!(v.sounding(), 0, "a fifth-strength hit is too soft");
    run(&mut v, p.world(), 2, 2, &[(2, hit(ball, 1, shape, 0, 1200.0))]);
    assert_eq!(v.sounding(), 1);
}

#[test]
fn heard_from_ignores_other_balls() {
    let kit = kit();
    let mut ball = ThingId(0);
    let mut scene = new_scene(&kit.registry);
    let found = ball_of(&scene);
    scene = {
        let mut s = voiced(&kit, |v, a| {
            v.play_heard_from = Target::Thing(found);
            a.sustain = 100.0;
            a.gate_ms = 5_000.0;
        });
        ball = ball_of(&s).max(ball);
        std::mem::swap(&mut s, &mut scene);
        scene
    };
    let shape = shape_of(&scene);
    let p = world(&scene, &kit);
    let mut v = voice(&scene, &kit);
    run(&mut v, p.world(), 0, 2, &[(0, hit(ThingId(999), 0, shape, 0, 1500.0))]);
    assert_eq!(v.sounding(), 0, "a ball it doesn't listen to");
    run(&mut v, p.world(), 2, 2, &[(2, hit(ball, 0, shape, 0, 1500.0))]);
    assert_eq!(v.sounding(), 1);
    // A hit on another thing plays nothing here.
    run(&mut v, p.world(), 4, 2, &[(4, hit(ball, 1, ThingId(998), 0, 1500.0))]);
    assert_eq!(v.sounding(), 1);
}

#[test]
fn until_next_hit_releases_on_the_next_hit() {
    let kit = kit();
    let scene = voiced(&kit, |_, a| {
        a.sustain = 100.0;
        a.gate_mode = GateMode::UntilNextHit;
    });
    let (shape, ball) = (shape_of(&scene), ball_of(&scene));
    let p = world(&scene, &kit);
    let mut v = voice(&scene, &kit);
    run(&mut v, p.world(), 0, 750, &[(0, hit(ball, 0, shape, 0, 1500.0))]);
    assert_eq!(v.sounding(), 1, "held a whole second, waiting for the next hit");
    run(&mut v, p.world(), 750, 2, &[(750, hit(ball, 1, shape, 0, 1500.0))]);
    assert_eq!(v.sounding(), 2, "the first is releasing");
    // Release is 200 ms.
    run(&mut v, p.world(), 752, 200, &[]);
    assert_eq!(v.sounding(), 1);
}

#[test]
fn fast_gain_change_does_not_click() {
    let kit = kit();
    let quiet = voiced(&kit, |v, a| {
        v.a.wave = Wave::Sine;
        v.a.gain = -60.0;
        a.sustain = 100.0;
        a.gate_ms = 5_000.0;
    });
    let loud = voiced(&kit, |v, a| {
        v.a.wave = Wave::Sine;
        v.a.gain = 12.0;
        a.sustain = 100.0;
        a.gate_ms = 5_000.0;
    });
    let (shape, ball) = (shape_of(&quiet), ball_of(&quiet));
    let p = world(&quiet, &kit);
    let mut old = voice(&quiet, &kit);
    let before = run(&mut old, p.world(), 0, 20, &[(0, hit(ball, 0, shape, 0, 1500.0))]);
    assert!(before[600..].iter().all(|v| v.abs() < 1e-6), "Gain at the bottom is silent");
    let mut new = voice(&loud, &kit);
    new.adopt(&mut old);
    let after = run(&mut new, p.world(), 20, 100, &[]);
    assert!(after.iter().fold(0.0f32, |m, v| m.max(v.abs())) > 3.0, "it gets loud");
    // A sine at 4x full scale steps at most this much between samples at C5.
    let own = 4.0 * (std::f32::consts::TAU * 261.63 / 48_000.0);
    let worst = after.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
    assert!(worst < own + 0.05, "a step of {worst} (the wave's own is {own})");
}

#[test]
fn osc_b_off_costs_nothing() {
    let kit = kit();
    let timed = |b_on: bool| {
        let scene = voiced(&kit, |v, a| {
            v.a.unison = 8;
            v.b.on = b_on;
            v.b.unison = 8;
            a.sustain = 100.0;
            a.gate_ms = 50_000.0;
        });
        let (shape, ball) = (shape_of(&scene), ball_of(&scene));
        let p = world(&scene, &kit);
        let mut v = voice(&scene, &kit);
        let hits: Vec<(u64, BlockHit)> = (0..8).map(|i| (0, hit(ball, i, shape, 0, 1500.0))).collect();
        run(&mut v, p.world(), 0, 50, &hits);
        let t = Instant::now();
        run(&mut v, p.world(), 50, 400, &[]);
        t.elapsed().as_secs_f64()
    };
    timed(false);
    let (off, on) = (timed(false), timed(true));
    assert!(off < 0.7 * on, "Osc B off took {off:.4} s, on {on:.4} s");
}

#[test]
fn removing_a_ringing_shape_fades_it_out() {
    let kit = kit();
    let scene = voiced(&kit, |_, a| {
        a.sustain = 100.0;
        a.gate_ms = 5_000.0;
    });
    let resolved = resolve_scene(&scene, &kit.registry);
    let mut e = Engine::new(&scene, &resolved, &kit);
    e.audition(shape_of(&scene), 60);
    let mut block = [0.0f32; BLOCK * 2];
    for _ in 0..40 {
        e.render(&mut block);
    }
    assert!(block.iter().any(|v| v.abs() > 0.01), "the note rings");
    let mut gone = scene.clone();
    gone.things.remove(&shape_of(&scene));
    gone.order.retain(|id| *id != shape_of(&scene));
    let gone_resolved = resolve_scene(&gone, &kit.registry);
    e.swap_patch(SoundPatch::build(&gone, &gone_resolved, &kit));
    assert!(e.has_leaving());
    let mut after = Vec::new();
    for _ in 0..30 {
        e.render(&mut block);
        after.extend(block.chunks(2).map(|f| f[0]));
    }
    // The master's lookahead holds back a few ms; within 10 ms (plus that) it has faded.
    let tail = &after[48 * 20..];
    assert!(tail.iter().all(|v| v.abs() < 1e-4), "still sounding {:?}", tail.iter().fold(0.0f32, |m, v| m.max(v.abs())));
    let worst = after.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
    assert!(worst < 0.1, "no click: {worst}");
    assert!(!e.has_leaving(), "dropped once silent");
}

#[test]
#[cfg_attr(debug_assertions, ignore = "timing needs a release build")]
fn heavy_patch_renders_8x_real_time() {
    let kit = kit();
    let scene = voiced(&kit, |v, a| {
        v.a.wave = Wave::Saw;
        v.a.unison = 16;
        v.b.on = true;
        v.b.unison = 16;
        a.sustain = 100.0;
        a.gate_ms = 50_000.0;
    });
    let (shape, ball) = (shape_of(&scene), ball_of(&scene));
    let p = world(&scene, &kit);
    let mut v = voice(&scene, &kit);
    let hits: Vec<(u64, BlockHit)> = (0..16).map(|i| (0, hit(ball, i, shape, 0, 1500.0))).collect();
    run(&mut v, p.world(), 0, 10, &hits);
    assert_eq!(v.sounding(), 16);
    let t = Instant::now();
    run(&mut v, p.world(), 10, 750, &[]);
    let factor = 1.0 / t.elapsed().as_secs_f64();
    println!("heavy patch: {factor:.1}x real time");
    assert!(factor >= 8.0, "only {factor:.1}x real time");
}



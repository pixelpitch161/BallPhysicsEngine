//! The engine end to end, with the test tone on a test scene's shape.

use bpe_core::{Scene, ThingId};
use bpe_kit::{Kit, kit, new_scene, new_studio_scene, resolve_scene};
use bpe_runtime::{EventKind, Playback};
use bpe_sound::{BLOCK, Engine};

/// The classic test scene (a ball dropping into a hollow circle), its shape tagged to blip.
fn toned(kit: &Kit, tag: &str) -> (Scene, ThingId) {
    let mut scene = new_scene(&kit.registry);
    let shape = scene.order.iter().copied().find(|id| scene.things[id].kind == "shape").unwrap();
    scene.things.get_mut(&shape).unwrap().tags.insert(tag.into());
    (scene, shape)
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

#[test]
fn rendering_twice_gives_the_same_samples() {
    let kit = kit();
    let (scene, _) = toned(&kit, "test-tone");
    let a = render(&scene, &kit, 5.0);
    let b = render(&scene, &kit, 5.0);
    assert!(a.iter().any(|&v| v != 0.0), "the ball hits the shape and it blips");
    assert!(a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()));
}

#[test]
fn a_hit_starts_on_its_exact_sample() {
    let kit = kit();
    let (scene, shape) = toned(&kit, "test-tone");
    let resolved = resolve_scene(&scene, &kit.registry);
    let mut p = Playback::new(&scene, &resolved, kit.sims.clone());
    p.seek(5.0);
    let rate = p.world().steps_per_second as f64;
    let hit = p
        .log()
        .events()
        .iter()
        .find(|e| e.kind == EventKind::Hit && e.b.is_some_and(|b| b.thing == shape))
        .expect("the ball hits the shape")
        .clone();
    let sps = 48_000.0 / rate;
    let expected = (((hit.step - 1) as f64 + hit.at as f64) * sps).round() as usize;
    let out = render(&scene, &kit, 5.0);
    let first = out.iter().position(|&v| v != 0.0).unwrap() / 2;
    assert_eq!(first, expected, "hit in step {} at {}", hit.step, hit.at);
    let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert!((peak - bpe_dsp::db_to_gain(-12.0)).abs() < 0.01, "a -12 dBFS blip: {peak}");
}

#[test]
fn silence_without_sound_cards() {
    let kit = kit();
    let scene = new_studio_scene(&kit.registry);
    assert!(render(&scene, &kit, 2.0).iter().all(|&v| v == 0.0));
    // A sounding shape's tag on nothing but the classic scene: still silent without the tag.
    let scene = new_scene(&kit.registry);
    assert!(render(&scene, &kit, 3.0).iter().all(|&v| v == 0.0));
}

#[test]
fn seek_with_pre_roll_matches_a_straight_render() {
    let kit = kit();
    let (scene, _) = toned(&kit, "test-tone-decay=2");
    let straight = render(&scene, &kit, 10.0);
    let resolved = resolve_scene(&scene, &kit.registry);
    let mut e = Engine::new(&scene, &resolved, &kit);
    // Somewhere else first, so the seek goes back as well as forward.
    let mut block = [0.0f32; BLOCK * 2];
    for _ in 0..2000 {
        e.render(&mut block);
    }
    e.seek(6.0);
    let from = e.sample_position() as usize;
    assert_eq!(from, 6 * 48_000, "6 s is a block boundary");
    let mut sought = Vec::new();
    while (e.sample_position() as usize) < straight.len() / 2 {
        e.render(&mut block);
        sought.extend_from_slice(&block);
    }
    let tail = &straight[from * 2..];
    assert!(tail.iter().any(|&v| v.abs() > 0.01), "there is sound after 6 s");
    let worst = tail.iter().zip(&sought).fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
    assert!(worst < bpe_dsp::db_to_gain(-120.0), "worst difference {} dB", bpe_dsp::gain_to_db(worst));
}

#[test]
fn seeking_back_to_the_start_matches_a_fresh_engine() {
    let kit = kit();
    let (scene, _) = toned(&kit, "test-tone");
    let fresh = render(&scene, &kit, 3.0);
    let resolved = resolve_scene(&scene, &kit.registry);
    let mut e = Engine::new(&scene, &resolved, &kit);
    let mut block = [0.0f32; BLOCK * 2];
    for _ in 0..1500 {
        e.render(&mut block);
    }
    e.seek(0.0);
    let mut again = Vec::new();
    while again.len() < fresh.len() {
        e.render(&mut block);
        again.extend_from_slice(&block);
    }
    assert!(fresh.iter().zip(&again).all(|(a, b)| a.to_bits() == b.to_bits()));
}

/// The scene with the Master's output turned down to `db`.
fn with_output(mut scene: Scene, kit: &Kit, db: f64) -> Scene {
    bpe_kit::ensure_mixer(&mut scene, &kit.registry);
    let a =
        bpe_core::Address { thing: ThingId::ROOT, slot: Some(bpe_core::SlotId::Mixer), key: "master_output".into() };
    scene.set_at(&kit.registry, &a, bpe_core::Value::Number(db)).unwrap();
    scene
}

#[test]
fn a_master_change_while_stopped_holds_after_a_seek() {
    let kit = kit();
    let (scene, _) = toned(&kit, "test-tone-decay=1");
    let quieter = with_output(scene.clone(), &kit, -6.0);
    let resolved = resolve_scene(&scene, &kit.registry);
    let quiet_resolved = resolve_scene(&quieter, &kit.registry);
    // Turned while stopped: nothing renders between the patch and the seek that Play sends.
    let mut a = Engine::new(&scene, &resolved, &kit);
    a.swap_patch(bpe_sound::SoundPatch::build(&quieter, &quiet_resolved, &kit));
    a.seek(1.0);
    let mut b = Engine::new(&quieter, &quiet_resolved, &kit);
    b.seek(1.0);
    let mut x = [0.0f32; BLOCK * 2];
    let mut y = [0.0f32; BLOCK * 2];
    let mut worst = 0.0f32;
    for _ in 0..2000 {
        a.render(&mut x);
        b.render(&mut y);
        worst = x.iter().zip(&y).fold(worst, |m, (p, q)| m.max((p - q).abs()));
    }
    assert!(worst < 1e-6, "the -6 dB output holds after Play: {worst}");
}

#[test]
fn a_silent_scene_runs_no_physics() {
    let kit = kit();
    let scene = new_scene(&kit.registry);
    let resolved = resolve_scene(&scene, &kit.registry);
    let mut e = Engine::new(&scene, &resolved, &kit);
    let mut block = [0.0f32; BLOCK * 2];
    for _ in 0..100 {
        e.render(&mut block);
    }
    e.seek(30.0);
    e.render(&mut block);
    assert_eq!(e.physics_step(), 0, "no sound, so the engine's own physics stays put");
    assert_eq!(e.sample_position(), 30 * 48_000 + BLOCK as u64);
}

#[test]
fn the_first_voice_of_a_silent_scene_picks_up_where_the_sound_is() {
    let kit = kit();
    let plain = new_scene(&kit.registry);
    let (toned_scene, _) = toned(&kit, "test-tone");
    let straight = render(&toned_scene, &kit, 4.0);
    let resolved = resolve_scene(&plain, &kit.registry);
    let mut e = Engine::new(&plain, &resolved, &kit);
    let mut block = [0.0f32; BLOCK * 2];
    for _ in 0..750 {
        e.render(&mut block);
    }
    let swapped = e.sample_position() as usize;
    let toned_resolved = resolve_scene(&toned_scene, &kit.registry);
    e.swap_patch(bpe_sound::SoundPatch::build(&toned_scene, &toned_resolved, &kit));
    let mut after = Vec::new();
    while (e.sample_position() as usize) < straight.len() / 2 {
        e.render(&mut block);
        after.extend_from_slice(&block);
    }
    // Past the master's delay and any blip that started before the swap.
    let skip = 400;
    let tail = &straight[(swapped + skip) * 2..];
    assert!(tail.iter().any(|&v| v != 0.0), "the ball hits after 1 s");
    assert!(tail.iter().zip(&after[skip * 2..]).all(|(a, b)| (a - b).abs() < 1e-6));
}

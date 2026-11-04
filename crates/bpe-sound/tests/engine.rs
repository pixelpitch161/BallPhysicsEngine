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

//! The lens effects of lighting part 5 (Lens flare, Lens streaks, Colour fringes) in pictures, on
//! the cloud's software GPU; each check compares pixels with each other.

use glam::Vec2;

use crate::effects::{BUILTINS, defaults};
use crate::{EffectCall, Frame, Renderer, Stage, test_device};

const W: u32 = 270;
const H: u32 = 480;

fn builtin(id: &str) -> &'static crate::effects::Builtin {
    BUILTINS.iter().find(|b| b.id == id).expect("a built-in")
}

/// The effect `id` at its defaults, with `set` changing its settings by key.
fn call(id: &str, set: &[(&str, f32)]) -> EffectCall {
    let b = builtin(id);
    let header = b.header();
    let mut params = defaults(header);
    let mut at = 0;
    for p in &header.params {
        if let Some((_, v)) = set.iter().find(|(k, _)| *k == p.key) {
            params[at] = *v;
        }
        at += p.floats();
    }
    EffectCall { params, ..EffectCall::new(7, b.wgsl, Stage::Picture) }
}

/// A black picture with one bright ball.
fn dot(at: Vec2, radius: f32, brightness: f32) -> Frame {
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    crate::pictures::plain_ball(&mut frame, at, radius, [brightness, brightness, brightness, 1.0], 0.0);
    frame
}

fn render(frame: &Frame) -> Option<Vec<u8>> {
    let Ok((device, queue)) = test_device() else {
        eprintln!("no GPU adapter available, skipping");
        return None;
    };
    let mut renderer = Renderer::new(&device, W, H);
    renderer.render(&device, &queue, frame);
    Some(renderer.read_output(&device, &queue).unwrap())
}

fn index(at: Vec2) -> (i32, i32) {
    (((at.x / 1080.0 + 0.5) * W as f32) as i32, ((at.y / 1920.0 + 0.5) * H as f32) as i32)
}

/// One channel (0 red .. 2 blue, 3 all three) summed over the 5 × 5 pixels round world point `at`.
fn around(pixels: &[u8], at: Vec2, channel: usize) -> u32 {
    let (cx, cy) = index(at);
    let mut sum = 0;
    for y in cy - 2..=cy + 2 {
        for x in cx - 2..=cx + 2 {
            if x < 0 || y < 0 || x >= W as i32 || y >= H as i32 {
                continue;
            }
            let i = (y as usize * W as usize + x as usize) * 4;
            sum += match channel {
                3 => pixels[i] as u32 + pixels[i + 1] as u32 + pixels[i + 2] as u32,
                c => pixels[i + c] as u32,
            };
        }
    }
    sum
}

#[test]
fn every_lens_effect_assembles_its_pre_pass() {
    for id in ["lens_flare", "lens_streaks"] {
        let b = builtin(id);
        assert!(b.header().lens.any(), "{id}");
        crate::effects::assemble_bright(b.wgsl).unwrap_or_else(|e| panic!("{id}: {e}"));
    }
    assert!(!builtin("fringes").header().lens.any());
}

#[test]
fn streaks_run_across_a_light_and_turn_with_their_setting() {
    let light = Vec2::new(-150.0, -300.0);
    let mut frame = dot(light, 14.0, 8.0);
    let Some(plain) = render(&frame) else { return };
    frame.effects.push(call("lens_streaks", &[]));
    let Some(across) = render(&frame) else { return };
    let side = |p: &[u8]| around(p, light + Vec2::new(200.0, 0.0), 3) + around(p, light - Vec2::new(200.0, 0.0), 3);
    let updown = |p: &[u8]| around(p, light + Vec2::new(0.0, 200.0), 3) + around(p, light - Vec2::new(0.0, 200.0), 3);
    assert_eq!(side(&plain), 0, "the picture is black but for the ball");
    assert!(side(&across) > 4 * updown(&across).max(10), "across: {} vs {}", side(&across), updown(&across));
    frame.effects[0] = call("lens_streaks", &[("ray_turn", 90.0)]);
    let Some(turned) = render(&frame) else { return };
    assert!(updown(&turned) > 4 * side(&turned).max(10), "turned: {} vs {}", updown(&turned), side(&turned));
}

#[test]
fn a_dim_picture_under_the_threshold_neither_flares_nor_streaks() {
    // The plain ball pipeline's own shine reaches a little over 1, so the threshold sits above it.
    let mut frame = dot(Vec2::new(-150.0, -300.0), 30.0, 0.3);
    let Some(plain) = render(&frame) else { return };
    frame.effects.push(call("lens_streaks", &[("threshold", 3.0)]));
    frame.effects.push(call("lens_flare", &[("threshold", 3.0)]));
    let Some(with) = render(&frame) else { return };
    // Passing through an effect may round a value by one step; nothing more may change.
    let worst = plain.iter().zip(&with).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
    let differ = plain.iter().zip(&with).filter(|(a, b)| a != b).count();
    assert!(worst <= 1 && differ < 64, "{differ} values differ, by up to {worst}");
}

#[test]
fn a_flare_throws_ghosts_across_the_middle() {
    let light = Vec2::new(-300.0, -500.0);
    let mut frame = dot(light, 14.0, 8.0);
    // Ghosts only: no halo or starburst to confuse the check.
    frame.effects.push(call("lens_flare", &[("halo", 0.0), ("star", 0.0)]));
    let Some(pixels) = render(&frame) else { return };
    // The fourth ghost (0.5 - 3 x 0.42 = -0.76 of the way out) sits across the middle.
    let ghost = light * -0.76;
    let mirror = Vec2::new(-ghost.x, ghost.y);
    assert!(around(&pixels, ghost, 3) > 2 * around(&pixels, mirror, 3).max(10), "{}", around(&pixels, ghost, 3));
}

#[test]
fn a_flare_rings_its_light_with_a_halo() {
    let light = Vec2::new(0.0, -300.0);
    let mut frame = dot(light, 14.0, 8.0);
    frame.effects.push(call("lens_flare", &[("ghosts", 0.0), ("star", 0.0)]));
    let Some(pixels) = render(&frame) else { return };
    // The ring is 0.22 of the picture's height (about 422 world px) from the light.
    let on = around(&pixels, light + Vec2::new(0.0, 422.0), 3);
    let off = around(&pixels, light + Vec2::new(0.0, 250.0), 3);
    assert!(on > 2 * off.max(10), "{on} vs {off}");
}

#[test]
fn colour_fringes_split_red_from_blue_toward_the_edge() {
    let mut frame = dot(Vec2::new(380.0, 0.0), 40.0, 1.0);
    frame.effects.push(call("fringes", &[("amount", 0.05), ("beat", 0.0)]));
    let Some(pixels) = render(&frame) else { return };
    // Red is drawn toward the middle and blue out toward the edge.
    let inner = Vec2::new(380.0 - 46.0, 0.0);
    let outer = Vec2::new(380.0 + 46.0, 0.0);
    assert!(around(&pixels, inner, 0) > around(&pixels, inner, 2) + 50, "inner edge reddish");
    assert!(around(&pixels, outer, 2) > around(&pixels, outer, 0) + 50, "outer edge bluish");
    // The middle of the lens doesn't move.
    let mut middle = dot(Vec2::ZERO, 40.0, 1.0);
    let Some(plain) = render(&middle) else { return };
    middle.effects.push(call("fringes", &[("amount", 0.05), ("beat", 0.0)]));
    let Some(split) = render(&middle) else { return };
    assert_eq!(around(&plain, Vec2::ZERO, 3), around(&split, Vec2::ZERO, 3));
}

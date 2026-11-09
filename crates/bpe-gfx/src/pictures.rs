//! Picture test helpers, and tests of stretched balls and trails, on the software GPU.

use crate::{Frame, LayerGpu, LayeredInstance, Renderer, test_device};
use glam::Vec2;

/// Renders `frame` at `w` by `h`, or `None` without a GPU.
pub(crate) fn render_sized(frame: &Frame, w: u32, h: u32) -> Option<Vec<u8>> {
    let Ok((device, queue)) = test_device() else {
        eprintln!("no GPU adapter available, skipping");
        return None;
    };
    let mut renderer = Renderer::new(&device, w, h);
    renderer.render(&device, &queue, frame);
    Some(renderer.read_output(&device, &queue).unwrap())
}

/// The pixel at a world position of a 1080 by 1920 frame drawn at `w` by `h`.
pub(crate) fn pixel_sized(pixels: &[u8], w: u32, h: u32, at: Vec2) -> [u8; 4] {
    let x = (((at.x / 1080.0 + 0.5) * w as f32) as usize).min(w as usize - 1);
    let y = (((at.y / 1920.0 + 0.5) * h as f32) as usize).min(h as usize - 1);
    let i = (y * w as usize + x) * 4;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

/// Saves a picture under target/pictures when BPE_SAVE_RENDERS is set.
pub(crate) fn save(name: &str, pixels: &[u8], w: u32, h: u32) {
    if std::env::var_os("BPE_SAVE_RENDERS").is_none() {
        return;
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pictures");
    std::fs::create_dir_all(&dir).unwrap();
    // A plain PPM, so the tests need no image crate.
    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    for p in pixels.chunks(4) {
        out.extend_from_slice(&p[..3]);
    }
    std::fs::write(dir.join(format!("{name}.ppm")), out).unwrap();
}

/// A plain ball on the layered path: one Body record of `color` (linear; alpha -1 adds its light)
/// at `center`, the thing code `code`, pushed into `frame` (its own records, so it works in a
/// blend layer's frame too).
pub(crate) fn plain_ball(frame: &mut Frame, center: Vec2, radius: f32, color: [f32; 4], code: f32) {
    let first = frame.look_layers.len() as u32;
    frame.look_layers.push(LayerGpu {
        head: [LayerGpu::BODY, bpe_core::Blend::Normal.shader_code(), 0, 0],
        amount: [1.0, 0.0, 0.0, 0.0],
        ..LayerGpu::ZERO
    });
    frame.layered.push(LayeredInstance {
        center: center.into(),
        radius,
        seed: 0.3,
        body: color,
        stretch: [1.0, 0.0, 1.0, 1.0],
        form: [0.0, 0.15, 0.0, 1.45],
        misc: [1.0, code, 0.0, 0.0],
        vary: crate::NO_VARY,
        range: [first, 1, 0, 0],
        hit: crate::NO_HIT,
        ..LayeredInstance::default()
    });
}

#[test]
fn every_ball_shape_draws_its_own_outline() {
    // The eight shapes (0 circle, 1 square, 2 triangle, 3 diamond, 4 hexagon, 5 star, 6 heart,
    // 7 donut) in a 2 by 4 grid, red on the dark background, 50 pixels round on the output.
    const W: u32 = 540;
    const H: u32 = 960;
    let centre =
        |k: usize| Vec2::new(if k.is_multiple_of(2) { -270.0 } else { 270.0 }, -720.0 + 480.0 * (k / 2) as f32);
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    for k in 0..8 {
        plain_ball(&mut frame, centre(k), 100.0, [1.0, 0.0, 0.0, 1.0], 0.0);
        frame.layered[k].form = [k as f32, 0.0, 0.0, 1.45];
    }
    let Some(pixels) = render_sized(&frame, W, H) else { return };
    save("ball-shapes", &pixels, W, H);
    let red = |at: Vec2| pixel_sized(&pixels, W, H, at)[0] > 128;
    let covered: Vec<usize> = (0..8)
        .map(|k| {
            let mut n = 0;
            for y in (-150..=150).step_by(2) {
                for x in (-150..=150).step_by(2) {
                    n += red(centre(k) + Vec2::new(x as f32, y as f32)) as usize;
                }
            }
            n
        })
        .collect();
    for k in 0..8 {
        assert!(covered[k] > 1000 && covered[k] < 151 * 151, "shape {k} covers {}: {covered:?}", covered[k]);
        for j in 0..k {
            assert!(covered[j].abs_diff(covered[k]) > 40, "shapes {j} and {k} look alike: {covered:?}");
        }
        assert_eq!(red(centre(k)), k != 7, "shape {k}: the middle (only the donut has a hole)");
    }
    let r = 100.0;
    assert!(!red(centre(0) + Vec2::splat(0.8 * r)) && red(centre(1) + Vec2::splat(0.75 * r)), "the square's corner");
    assert!(red(centre(7) + Vec2::new(0.72 * r, 0.0)), "the donut's ring");
    assert!(!red(centre(2) + Vec2::new(0.8 * r, -0.8 * r)), "the triangle narrows to its top corner");

    // An outline 8 pixels wide outside the square, in white.
    let mut outlined = Frame { bloom: 0.0, ..Frame::default() };
    plain_ball(&mut outlined, Vec2::ZERO, 100.0, [1.0, 0.0, 0.0, 1.0], 0.0);
    outlined.look_layers.push(LayerGpu {
        head: [LayerGpu::OUTLINE, bpe_core::Blend::Normal.shader_code(), 0, 0],
        amount: [1.0, 0.0, 0.0, 0.0],
        p0: [8.0, 2.0, 0.0, 0.0],
        colour: [1.0; 4],
        ..LayerGpu::ZERO
    });
    outlined.layered[0].form = [1.0, 0.0, 0.0, 1.45];
    outlined.layered[0].range[1] = 2;
    let Some(pixels) = render_sized(&outlined, W, H) else { return };
    save("ball-shape-outline", &pixels, W, H);
    // Just outside the square's side (0.84 radii), and inside it.
    let edge = pixel_sized(&pixels, W, H, Vec2::new(0.84 * r + 6.0, 0.0));
    let inside = pixel_sized(&pixels, W, H, Vec2::new(0.6 * r, 0.0));
    assert!(edge[0] > 200 && edge[1] > 200 && edge[2] > 200, "the outline is white: {edge:?}");
    assert!(inside[0] > 128 && inside[1] < 40, "the body is still red inside: {inside:?}");
}

#[test]
fn a_stretched_ball_reaches_further_along_its_motion() {
    const W: u32 = 270;
    const H: u32 = 480;
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    plain_ball(&mut frame, Vec2::ZERO, 80.0, [1.0, 0.1046, 0.3813, 1.0], 3.0);
    frame.layered[0].stretch = [1.0, 0.0, 1.6, 0.8];
    frame.layered[0].form[3] = 2.0;
    let Some(pixels) = render_sized(&frame, W, H) else { return };
    let along = pixel_sized(&pixels, W, H, Vec2::new(112.0, 0.0));
    let across = pixel_sized(&pixels, W, H, Vec2::new(0.0, 112.0));
    assert!(along[0] > 120, "{along:?}");
    assert!(across[0] < 40, "{across:?}");
    // An additive ball adds light and covers nothing: the output alpha stays opaque and the
    // colour shows.
    frame.layered[0].body = [1.0, 0.1, 0.4, -1.0];
    let pixels = render_sized(&frame, W, H).unwrap();
    assert!(pixel_sized(&pixels, W, H, Vec2::ZERO)[0] > 120);
}

#[test]
fn a_streak_fades_to_its_tail_and_sits_behind_the_balls() {
    use crate::push_streak;
    const W: u32 = 270;
    const H: u32 = 480;
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    let points: Vec<Vec2> = (0..=10).map(|i| Vec2::new(0.0, -40.0 * i as f32)).collect();
    push_streak(&mut frame.trails, &points, 24.0, 1.0, [1.0, 0.0, 0.0, -1.0], 4.0);
    plain_ball(&mut frame, Vec2::ZERO, 60.0, [0.0, 0.0, 1.0, 1.0], 0.0);
    let Some(pixels) = render_sized(&frame, W, H) else { return };
    save("streak", &pixels, W, H);
    let head = pixel_sized(&pixels, W, H, Vec2::new(0.0, -80.0));
    let tail = pixel_sized(&pixels, W, H, Vec2::new(0.0, -390.0));
    let beside = pixel_sized(&pixels, W, H, Vec2::new(60.0, -200.0));
    let ball = pixel_sized(&pixels, W, H, Vec2::ZERO);
    assert!(head[0] > 120, "the streak is red near its head: {head:?}");
    assert!(tail[0] < head[0] / 3, "and fades towards its tail: {tail:?} vs {head:?}");
    assert!(beside[0] < 20, "nothing beside it: {beside:?}");
    // Drawn four times smaller than the video, the edge still softens over an output pixel
    // instead of being cut off by the quad: just outside the half width there is still light.
    // A quarter of the way down the streak is 18 px wide (tapering from 24).
    let edge = pixel_sized(&pixels, W, H, Vec2::new(19.5, -100.0));
    let outside = pixel_sized(&pixels, W, H, Vec2::new(24.0, -100.0));
    assert!(edge[0] > 30 && edge[0] < 200, "a soft edge: {edge:?}");
    assert!(outside[0] < 20, "dark past the margin: {outside:?}");
    assert!(ball[2] > 100 && ball[0] < 80, "the ball covers the streak: {ball:?}");
    // A covering streak hides the background instead of adding to it.
    let mut covering = Frame { bloom: 0.0, ..Frame::default() };
    covering.background.row = covering.gradients.row(&bpe_core::Gradient::solid(bpe_core::Color::WHITE));
    push_streak(&mut covering.trails, &points, 24.0, 0.0, [0.0, 0.0, 0.0, 1.0], 1.0);
    let pixels = render_sized(&covering, W, H).unwrap();
    let head = pixel_sized(&pixels, W, H, Vec2::new(0.0, -20.0));
    assert!(head[0] < 120, "a black covering streak darkens a white background: {head:?}");
}

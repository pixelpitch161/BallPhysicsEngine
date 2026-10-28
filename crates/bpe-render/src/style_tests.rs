//! Picture tests for balls with their own style.

use super::*;
use bpe_scene::{BallFill, BallShape, BallStyle, Color, Container, Spawner, Vec2};
use bpe_sim::Playback;

const W: u32 = 540;
const H: u32 = 960;

/// A still scene: one ball per style, laid out in a grid, with no gravity, walls or bloom.
fn sheet(styles: &[BallStyle]) -> Scene {
    let mut scene = Scene::default();
    scene.physics.gravity = Vec2::ZERO;
    scene.container = Container::Circle { center: Vec2::ZERO, radius: 5000.0, thickness: 0.0, color: Color::WHITE };
    scene.look.bloom_strength = 0.0;
    scene.look.flash_strength = 0.0;
    scene.effects.clear();
    let columns = 4;
    scene.spawners = styles
        .iter()
        .enumerate()
        .map(|(i, style)| {
            let (x, y) = ((i % columns) as f32, (i / columns) as f32);
            Spawner::Single {
                position: Vec2::new(-405.0 + x * 270.0, -800.0 + y * 230.0),
                velocity: Vec2::ZERO,
                radius: 80.0,
                color: Color::rgb(0xff, 0x5c, 0xa8),
                depth: 0.0,
                depth_velocity: 0.0,
                style: Some(style.clone()),
            }
        })
        .collect();
    scene
}

fn render(scene: &Scene, time: f64) -> Option<Vec<u8>> {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return None;
    };
    let mut renderer = Renderer::new(&device, W, H);
    let mut playback = Playback::new(scene);
    playback.seek(time);
    let frame = FrameData::build(scene, &playback, renderer.pixel_size(scene));
    renderer.render(&device, &queue, scene, time, &frame);
    Some(renderer.read_output(&device, &queue).unwrap())
}

/// The pixel at a world position.
fn pixel(pixels: &[u8], at: Vec2) -> [u8; 4] {
    let x = ((at.x / 1080.0 + 0.5) * W as f32) as usize;
    let y = ((at.y / 1920.0 + 0.5) * H as f32) as usize;
    let i = (y * W as usize + x) * 4;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

#[test]
fn every_shape_and_fill_draws() {
    let shapes = [
        BallShape::Circle,
        BallShape::Square,
        BallShape::Triangle,
        BallShape::Diamond,
        BallShape::Hexagon,
        BallShape::Star,
        BallShape::Heart,
        BallShape::Donut,
    ];
    let fills = [
        BallFill::Solid,
        BallFill::Shiny,
        BallFill::Radial,
        BallFill::Linear,
        BallFill::Stripes,
        BallFill::Rings,
        BallFill::Dots,
        BallFill::Checker,
        BallFill::Halves,
        BallFill::Slices,
        BallFill::Swirl,
        BallFill::Face,
    ];
    let mut styles: Vec<BallStyle> = shapes
        .iter()
        .map(|&shape| BallStyle {
            shape,
            outline: 4.0,
            outline_color: Color::rgb(0x20, 0xe0, 0xff),
            ..Default::default()
        })
        .collect();
    styles.extend(fills.iter().map(|&fill| BallStyle {
        fill,
        second_color: Color::rgb(0x20, 0x30, 0xff),
        glow: 0.3,
        ..Default::default()
    }));
    let scene = sheet(&styles);
    let Some(pixels) = render(&scene, 0.5) else { return };
    gpu::tests::save("ball-styles", &pixels, W, H);
    // The middle of every ball but the donut shows its pink or its blue (or an eye or stripe).
    for (i, spawner) in scene.spawners.iter().enumerate() {
        let Spawner::Single { position, .. } = spawner else { unreachable!() };
        let p = pixel(&pixels, *position + Vec2::new(0.0, 30.0));
        let style = spawner.style().unwrap();
        if style.shape == BallShape::Donut {
            assert!(p[0] < 60, "donut {i} should have a hole: {p:?}");
            continue;
        }
        assert!(p[0] > 120 || p[2] > 120, "ball {i} ({:?}, {:?}) is missing: {p:?}", style.shape, style.fill);
    }
}

#[test]
fn every_preset_look_draws() {
    let styles: Vec<BallStyle> = BallStyle::presets().into_iter().map(|p| p.style).collect();
    let mut scene = sheet(&styles);
    // Moving, so trails, stretch and facing the motion are drawn too.
    for spawner in &mut scene.spawners {
        let Spawner::Single { velocity, .. } = spawner else { unreachable!() };
        *velocity = Vec2::new(900.0, 0.0);
    }
    let Some(pixels) = render(&scene, 0.1) else { return };
    gpu::tests::save("ball-style-presets", &pixels, W, H);
    assert!(pixels.chunks(4).filter(|p| p[0] > 120).count() > 1000);
}

#[test]
fn plain_balls_are_one_flat_colour() {
    let scene = sheet(&[BallStyle::default()]);
    let Some(pixels) = render(&scene, 0.0) else { return };
    let Spawner::Single { position, .. } = &scene.spawners[0] else { unreachable!() };
    let a = pixel(&pixels, *position);
    let b = pixel(&pixels, *position + Vec2::new(-50.0, -40.0));
    let c = pixel(&pixels, *position + Vec2::new(45.0, 50.0));
    // Equal, give or take the output's dithering.
    let close = |x: [u8; 4], y: [u8; 4]| x.iter().zip(y).all(|(&x, y)| x.abs_diff(y) <= 2);
    assert!(close(a, b) && close(a, c), "{a:?} {b:?} {c:?}");
    // Tone mapping keeps it close to the colour picked.
    assert!(a[0] > 200 && a[1] > 60 && a[1] < 130 && a[2] > 130, "{a:?}");
}

#[test]
fn styled_and_classic_balls_mix_in_one_scene() {
    let mut scene = Scene::default();
    let mut styled = scene.spawners[0].clone();
    *styled.style_mut() = Some(BallStyle { trail_seconds: 0.4, ..BallStyle::default() });
    scene.spawners.push(styled);
    let mut playback = Playback::new(&scene);
    playback.seek(2.0);
    let frame = FrameData::build(&scene, &playback, 1.0);
    let half = frame.balls.len() / 2;
    assert!(frame.balls[..half].iter().all(|b| b.form[1] < 0.0), "the first block keeps the classic look");
    assert!(frame.balls[half..].iter().all(|b| b.form[1] >= 0.0), "the second block has its own look");
    assert!(!frame.trails.is_empty() && !frame.styled_trails.is_empty());
}

#[test]
fn classic_trails_keep_their_length_when_a_style_wants_a_longer_one() {
    let scene = Scene::default();
    let mut playback = Playback::new(&scene);
    playback.seek(3.0);
    let before = FrameData::build(&scene, &playback, 1.0).trails;

    let mut longer = scene.clone();
    let mut styled = longer.spawners[0].clone();
    *styled.style_mut() = Some(BallStyle { trail_seconds: 2.0, ..BallStyle::default() });
    longer.spawners.push(styled);
    let mut playback = Playback::new(&longer);
    playback.seek(3.0);
    let after = FrameData::build(&longer, &playback, 1.0);
    assert_eq!(bytemuck::cast_slice::<_, u8>(&before), bytemuck::cast_slice::<_, u8>(&after.trails));
    assert!(after.styled_trails.len() > after.trails.len() * 3);
}

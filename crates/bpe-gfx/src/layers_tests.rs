//! Picture tests for balls drawn with a stack of layers (layers.wgsl), on the software GPU.

use crate::pictures::{pixel_sized, render_sized, save};
use crate::{BallInstance, Frame, LayerGpu, LayeredInstance, PaintGpu};
use bpe_core::{Blend, Color, Gradient};
use glam::Vec2;

const W: u32 = 540;
const H: u32 = 960;
const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const PINK: [f32; 4] = [1.0, 0.1046, 0.3813, 1.0];

/// One ball at `center` of `radius`, its body `body`, drawn with records `first..first+count`.
fn ball(center: Vec2, radius: f32, body: [f32; 4], first: u32, count: u32, reach: f32) -> LayeredInstance {
    LayeredInstance {
        center: center.into(),
        radius,
        seed: 0.3,
        body,
        stretch: [1.0, 0.0, 1.0, 1.0],
        form: [0.0, 0.15, 0.0, reach],
        misc: [1.0, 3.0, 0.0, 0.0],
        vary: crate::NO_VARY,
        range: [first, count, 0, 0],
    }
}

fn record(code: u32, blend: Blend) -> LayerGpu {
    LayerGpu { head: [code, blend.shader_code(), 0, 0], amount: [1.0, 0.0, 0.0, 0.0], ..LayerGpu::ZERO }
}

fn body() -> LayerGpu {
    record(LayerGpu::BODY, Blend::Normal)
}

fn outline(width: f32, sits: f32) -> LayerGpu {
    LayerGpu { p0: [width, sits, 0.0, 0.0], colour: [0.0, 0.0, 1.0, 1.0], ..record(LayerGpu::OUTLINE, Blend::Normal) }
}

fn shine() -> LayerGpu {
    LayerGpu { amount: [1.0, 1.0, 0.0, 0.0], p0: [-0.45, -0.55, 1.0, 0.0], ..record(LayerGpu::SHINE, Blend::Normal) }
}

fn glow(strength: f32) -> LayerGpu {
    LayerGpu {
        amount: [1.0, strength, 0.0, 0.0],
        p0: [0.8, 0.0, 0.0, 0.0],
        head: [LayerGpu::GLOW, 0, LayerGpu::FLAG, 0],
        ..LayerGpu::ZERO
    }
}

/// A frame with one ball of `radius` at the centre wearing `records`.
fn one(records: Vec<LayerGpu>, body_colour: [f32; 4], radius: f32, reach: f32) -> Frame {
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    frame.layered.push(ball(Vec2::ZERO, radius, body_colour, 0, records.len() as u32, reach));
    frame.look_layers = records;
    frame
}

fn at(pixels: &[u8], x: f32, y: f32) -> [u8; 4] {
    pixel_sized(pixels, W, H, Vec2::new(x, y))
}

#[test]
fn a_body_alone_is_a_white_circle_with_a_soft_edge() {
    // The edge runs through the middle of a pixel.
    let frame = one(vec![body()], WHITE, 201.0, 1.45);
    let Some(px) = render_sized(&frame, W, H) else { return };
    save("layers-white-circle", &px, W, H);
    let middle = at(&px, 0.0, 0.0);
    assert!(middle[..3].iter().all(|&c| c > 240) && middle[0] == middle[2], "white in the middle: {middle:?}");
    assert_eq!(&at(&px, 260.0, 0.0)[..3], &[0, 0, 0], "nothing outside");
    // Somewhere across the edge a pixel is part covered.
    let row: Vec<u8> = (190..210).map(|x| at(&px, x as f32, 0.0)[0]).collect();
    assert!(row.iter().any(|&v| v > 10 && v < 245), "{row:?}");
}

#[test]
fn an_outline_sits_inside_or_outside_the_edge() {
    let inside = one(vec![body(), outline(20.0, 0.0)], WHITE, 200.0, 1.45);
    let outside = one(vec![body(), outline(20.0, 2.0)], WHITE, 200.0, 1.6);
    let Some(a) = render_sized(&inside, W, H) else { return };
    let b = render_sized(&outside, W, H).unwrap();
    save("layers-outline-inside", &a, W, H);
    save("layers-outline-outside", &b, W, H);
    // Just inside the edge: blue for the inside ring, the white body for the outside one.
    let (ai, bi) = (at(&a, 190.0, 0.0), at(&b, 190.0, 0.0));
    assert!(ai[2] > 200 && ai[0] < 30, "{ai:?}");
    assert!(bi[0] > 240 && bi[1] > 240, "{bi:?}");
    // Just outside: nothing, or the outside ring.
    let (ao, bo) = (at(&a, 210.0, 0.0), at(&b, 210.0, 0.0));
    assert!(ao[2] < 10, "{ao:?}");
    assert!(bo[2] > 200 && bo[0] < 30, "{bo:?}");
}

/// A pattern of one flat grey from its own gradient, mixed over a pink body by `blend`.
fn grey_pattern(frame: &mut Frame, blend: Blend) -> LayerGpu {
    let row = frame.gradients.row(&Gradient::solid(Color::rgb(128, 128, 128)));
    LayerGpu { paint: PaintGpu { row: row as f32, ..PaintGpu::default() }, ..record(LayerGpu::PATTERN, blend) }
}

#[test]
fn a_pattern_layer_mixes_by_its_blend() {
    let middle = |blend: Blend| {
        let mut frame = one(vec![], PINK, 200.0, 1.45);
        let pattern = grey_pattern(&mut frame, blend);
        frame.look_layers = vec![body(), pattern];
        frame.layered[0].range[1] = 2;
        render_sized(&frame, W, H).map(|px| at(&px, 0.0, 0.0))
    };
    let Some(plain) = render_sized(&one(vec![body()], PINK, 200.0, 1.45), W, H).map(|px| at(&px, 0.0, 0.0)) else {
        return;
    };
    let (multiply, screen, normal) =
        (middle(Blend::Multiply).unwrap(), middle(Blend::Screen).unwrap(), middle(Blend::Normal).unwrap());
    let sum = |p: [u8; 4]| p[0] as u32 + p[1] as u32 + p[2] as u32;
    assert!(sum(multiply) < sum(plain), "multiply darkens: {multiply:?} vs {plain:?}");
    assert!(sum(screen) > sum(plain), "screen lightens: {screen:?} vs {plain:?}");
    assert!(normal[0].abs_diff(normal[1]) < 3 && normal[1].abs_diff(normal[2]) < 3, "normal covers: {normal:?}");
}

#[test]
fn shine_is_brightest_near_its_spot() {
    let frame = one(vec![body(), shine()], [0.3, 0.3, 0.3, 1.0], 200.0, 1.45);
    let Some(px) = render_sized(&frame, W, H) else { return };
    save("layers-shine", &px, W, H);
    let spot = at(&px, -50.0, -60.0)[0];
    for (x, y) in [(60.0, 60.0), (-120.0, 120.0), (120.0, -120.0), (0.0, 150.0)] {
        assert!(spot > at(&px, x, y)[0], "{spot} at the spot, {:?} at ({x}, {y})", at(&px, x, y));
    }
}

#[test]
fn glow_reaches_past_the_edge_and_strength_zero_shows_nothing() {
    let lit = one(vec![body(), glow(1.0)], PINK, 100.0, 1.45 + 2.2 * 0.8);
    let dark = one(vec![body(), glow(0.0)], PINK, 100.0, 1.45 + 2.2 * 0.8);
    let Some(a) = render_sized(&lit, W, H) else { return };
    let b = render_sized(&dark, W, H).unwrap();
    save("layers-glow", &a, W, H);
    assert!(at(&a, 130.0, 0.0)[0] > 40, "{:?}", at(&a, 130.0, 0.0));
    assert_eq!(&at(&b, 130.0, 0.0)[..3], &[0, 0, 0]);
}

#[test]
fn records_past_the_count_draw_nothing_and_the_same_frame_draws_the_same_bytes() {
    let mut longer = one(vec![body(), outline(20.0, 0.0)], WHITE, 200.0, 1.45);
    longer.layered[0].range[1] = 1;
    let alone = one(vec![body()], WHITE, 200.0, 1.45);
    let Some(a) = render_sized(&longer, W, H) else { return };
    let b = render_sized(&alone, W, H).unwrap();
    assert!(a == b, "only the body is drawn");
    let again = render_sized(&longer, W, H).unwrap();
    assert!(a == again, "drawing is repeatable");
}

#[test]
fn a_default_shine_on_a_flat_ball_looks_like_the_classic_shiny_ball() {
    for colour in [WHITE, PINK] {
        let mut classic = Frame { bloom: 0.0, ..Frame::default() };
        classic.balls.push(BallInstance {
            center: [0.0, 0.0],
            radius: 200.0,
            glow: 0.0,
            color: colour,
            code: [3.0, 0.3, 0.0, 0.0],
            paint: PaintGpu::default(),
        });
        let layered = one(vec![body(), shine()], colour, 200.0, 1.45);
        let Some(a) = render_sized(&classic, W, H) else { return };
        let b = render_sized(&layered, W, H).unwrap();
        save("layers-shine-classic", &a, W, H);
        save("layers-shine-layered", &b, W, H);
        let worst = a.iter().zip(&b).map(|(x, y)| x.abs_diff(*y)).max().unwrap();
        assert!(worst <= 2, "worst byte difference {worst} for {colour:?}");
    }
}

#[test]
fn layered_balls_draw_inside_a_layer_that_blends_its_things_with_each_other() {
    // In a Screen layer whose balls blend with each other: its own pipeline must build and draw.
    let mut frame = one(vec![body()], PINK, 100.0, 1.45);
    let list = std::mem::take(&mut frame.layered);
    let records = std::mem::take(&mut frame.look_layers);
    let layer = frame.layer_mut(Blend::Screen, 1.0, true);
    layer.layered = list;
    layer.look_layers = records;
    let Some(px) = render_sized(&frame, W, H) else { return };
    assert!(at(&px, 0.0, 0.0)[0] > 200, "{:?}", at(&px, 0.0, 0.0));
}

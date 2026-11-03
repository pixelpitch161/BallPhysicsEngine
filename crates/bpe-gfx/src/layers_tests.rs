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
fn a_glow_bigger_than_its_quad_fades_out_before_the_quad_s_edge() {
    // Size 4 would reach well past a quad of 3 radii: it fades to nothing at the quad's edge
    // instead of ending in a hard square.
    let mut big = glow(0.8);
    big.p0[0] = 4.0;
    let frame = one(vec![body(), big], WHITE, 40.0, 3.0);
    let Some(a) = render_sized(&frame, W, H) else { return };
    save("layers-glow-cut", &a, W, H);
    assert!(at(&a, 70.0, 0.0)[0] > 60, "the glow near the ball: {:?}", at(&a, 70.0, 0.0));
    for x in [120.0, 122.0, 123.0] {
        assert!(at(&a, x, 0.0)[0] < 12, "nearly nothing at the quad's edge ({x}): {:?}", at(&a, x, 0.0));
    }
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

/// A red pattern from its own gradient, covering the body, with `masks` mask records after it.
fn red_pattern(frame: &mut Frame, masks: u32) -> LayerGpu {
    let row = frame.gradients.row(&Gradient::solid(Color::rgb(255, 0, 0)));
    LayerGpu {
        head: [LayerGpu::PATTERN, 0, 0, masks],
        paint: PaintGpu { row: row as f32, ..PaintGpu::default() },
        ..record(LayerGpu::PATTERN, Blend::Normal)
    }
}

/// A mask record: `from` (0 layer below, 1 layer, 2 shape, 3 brush, 4 light, 5 dark), its shape,
/// angle (radians) and size, moving with the ball.
fn mask(from: f32, shape: f32, angle: f32, size: f32) -> LayerGpu {
    LayerGpu {
        head: [LayerGpu::MASK, 0, 0, 0],
        amount: [1.0, -1.0, 0.0, shape],
        p0: [from, 0.0, 0.05, 1.0],
        p1: [angle, size, 0.5, -1.0],
        ..LayerGpu::ZERO
    }
}

fn shading(kind: f32, p1: [f32; 4]) -> LayerGpu {
    LayerGpu { p0: [kind, 0.5, 0.0, 1.0], p1, colour: [0.0, 0.0, 0.0, 1.0], ..record(LayerGpu::SHADING, Blend::Normal) }
}

/// A frame with one white ball of radius 200 wearing `records`, built by `f`.
fn white(f: impl FnOnce(&mut Frame) -> Vec<LayerGpu>) -> Frame {
    let mut frame = one(vec![], WHITE, 200.0, 1.45);
    let records = f(&mut frame);
    frame.layered[0].range[1] = records.len() as u32;
    frame.look_layers = records;
    frame
}

#[test]
fn linear_shading_is_dark_at_its_dark_end_and_radial_shading_at_the_rim() {
    let linear = white(|_| vec![body(), shading(0.0, [0.7, 0.7, -0.7, -0.7])]);
    let Some(a) = render_sized(&linear, W, H) else { return };
    save("layers-shading-linear", &a, W, H);
    let (dark, light) = (at(&a, 120.0, 120.0)[0], at(&a, -120.0, -120.0)[0]);
    assert!(dark + 60 < light, "dark {dark}, light {light}");
    let radial = white(|_| vec![body(), shading(1.0, [0.0, 0.0, 0.6, -1.0])]);
    let b = render_sized(&radial, W, H).unwrap();
    save("layers-shading-radial", &b, W, H);
    let (middle, rim) = (at(&b, 0.0, 0.0)[0], at(&b, 185.0, 0.0)[0]);
    assert!(middle > 240 && rim + 60 < middle, "middle {middle}, rim {rim}");
}

#[test]
fn brush_shading_darkens_only_where_the_stroke_went() {
    let mut frame = white(|_| vec![body(), shading(2.0, [0.0, 0.0, 0.0, 0.0])]);
    // A band across the middle third of the tile, top to bottom.
    let n = crate::MASK_TILE;
    let tile: Vec<u8> = (0..n * n).map(|i| if (n / 3..2 * n / 3).contains(&(i % n)) { 255 } else { 0 }).collect();
    frame.mask_tiles.push((7, tile.into()));
    let Some(px) = render_sized(&frame, W, H) else { return };
    save("layers-shading-brush", &px, W, H);
    let (on, off) = (at(&px, 0.0, 100.0)[0], at(&px, 150.0, 0.0)[0]);
    assert!(off > 240 && on + 80 < off, "on the stroke {on}, off it {off}");
}

#[test]
fn a_half_mask_shows_a_pattern_on_one_side_and_invert_flips_it() {
    let half = |invert: bool| {
        white(|f| {
            let mut m = mask(2.0, 0.0, 0.0, 0.5);
            m.p0[1] = if invert { 1.0 } else { 0.0 };
            vec![body(), red_pattern(f, 1), m]
        })
    };
    let Some(a) = render_sized(&half(false), W, H) else { return };
    let b = render_sized(&half(true), W, H).unwrap();
    save("layers-mask-half", &a, W, H);
    let (top, bottom) = (at(&a, 0.0, -100.0), at(&a, 0.0, 100.0));
    assert!(top[0] > 240 && top[1] < 20, "the top half shows the pattern: {top:?}");
    assert!(bottom[1] > 240, "the bottom half shows the white body: {bottom:?}");
    let (top, bottom) = (at(&b, 0.0, -100.0), at(&b, 0.0, 100.0));
    assert!(top[1] > 240 && bottom[1] < 20, "inverted: {top:?} {bottom:?}");
}

#[test]
fn a_layer_below_mask_clips_a_pattern_to_an_outline() {
    let frame = white(|f| vec![body(), outline(40.0, 0.0), red_pattern(f, 1), mask(0.0, 0.0, 0.0, 0.0)]);
    let Some(px) = render_sized(&frame, W, H) else { return };
    save("layers-mask-clip", &px, W, H);
    let ring = at(&px, 180.0, 0.0);
    let middle = at(&px, 0.0, 0.0);
    assert!(ring[0] > 240 && ring[2] < 20, "red on the ring: {ring:?}");
    assert!(middle[1] > 240, "white inside it: {middle:?}");
    assert!(at(&px, 230.0, 0.0)[0] < 5, "nothing outside the ball");
}

#[test]
fn a_light_mask_over_a_shine_layer_shows_only_in_the_highlight() {
    let frame = {
        let mut frame = one(vec![], [0.2, 0.2, 0.2, 1.0], 200.0, 1.45);
        let mut light = mask(4.0, 0.0, 0.0, 0.0);
        light.p1[2] = 0.55;
        let mut s = shine();
        s.head[3] = 0;
        let p = red_pattern(&mut frame, 1);
        frame.look_layers = vec![body(), s, p, light];
        frame.layered[0].range[1] = 4;
        frame
    };
    let Some(px) = render_sized(&frame, W, H) else { return };
    save("layers-mask-light", &px, W, H);
    let spot = at(&px, -90.0, -110.0);
    let dim = at(&px, 120.0, 120.0);
    assert!(spot[0] > 200 && spot[1] < 40, "red in the highlight: {spot:?}");
    assert!(dim[0] < 120 && dim[0].abs_diff(dim[1]) < 10, "grey elsewhere: {dim:?}");
}

#[test]
fn two_masks_multiply_and_a_missing_target_hides_the_layer() {
    let both = white(|f| {
        vec![body(), red_pattern(f, 2), mask(2.0, 0.0, 0.0, 0.5), mask(2.0, 0.0, std::f32::consts::FRAC_PI_2, 0.5)]
    });
    let Some(px) = render_sized(&both, W, H) else { return };
    save("layers-mask-two", &px, W, H);
    // The top half and, turned a quarter, the right half: only the top right quarter is red.
    assert!(at(&px, 100.0, -100.0)[1] < 20, "{:?}", at(&px, 100.0, -100.0));
    for (x, y) in [(-100.0, -100.0), (100.0, 100.0), (-100.0, 100.0)] {
        assert!(at(&px, x, y)[1] > 240, "({x}, {y}): {:?}", at(&px, x, y));
    }
    let missing = white(|f| {
        let mut m = mask(1.0, 0.0, 0.0, 0.0);
        m.amount[1] = -1.0;
        vec![body(), red_pattern(f, 1), m]
    });
    let a = render_sized(&missing, W, H).unwrap();
    let plain = render_sized(&white(|_| vec![body()]), W, H).unwrap();
    assert!(a == plain, "a mask on a missing layer passes nothing");
    let again = render_sized(&both, W, H).unwrap();
    assert!(px == again, "drawing is repeatable");
}

#[test]
fn a_solo_layer_shows_its_masks_alone_in_white_and_black() {
    // A red ball under a red pattern with a half mask, shown solo: white top, black bottom.
    let frame = white(|f| {
        let mut p = red_pattern(f, 1);
        p.head[2] |= LayerGpu::SOLO;
        vec![body(), p, mask(2.0, 0.0, 0.0, 0.5)]
    });
    let Some(px) = render_sized(&frame, W, H) else { return };
    save("layers-mask-solo", &px, W, H);
    let (top, bottom) = (at(&px, 0.0, -100.0), at(&px, 0.0, 100.0));
    assert!(top.iter().take(3).all(|&c| c > 240), "white where the mask lets it show: {top:?}");
    assert!(bottom.iter().take(3).all(|&c| c < 15), "black where it hides: {bottom:?}");
}

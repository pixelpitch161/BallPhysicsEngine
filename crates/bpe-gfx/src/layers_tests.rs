//! Picture tests for balls drawn with a stack of layers (layers.wgsl), on the software GPU.

use crate::pictures::{pixel_sized, render_sized, save};
use crate::{BallInstance, BlendLayer, Frame, LayerGpu, LayeredInstance, PaintGpu, Place};
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
        group: [0.0; 4],
        place: [0.0; 4],
        hit: crate::NO_HIT,
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
fn a_shine_of_its_own_colour_tints_its_spot() {
    let white = one(vec![body(), shine()], [0.3, 0.3, 0.3, 1.0], 200.0, 1.45);
    let red_light = LayerGpu { head: [LayerGpu::SHINE, 0, LayerGpu::FLAG, 0], colour: [1.0, 0.0, 0.0, 1.0], ..shine() };
    let red = one(vec![body(), red_light], [0.3, 0.3, 0.3, 1.0], 200.0, 1.45);
    let Some(w) = render_sized(&white, W, H) else { return };
    let r = render_sized(&red, W, H).unwrap();
    let (ws, rs) = (at(&w, -50.0, -60.0), at(&r, -50.0, -60.0));
    assert!(ws[0].abs_diff(ws[2]) < 4, "a white spot: {ws:?}");
    assert!(rs[0] > rs[2] + 40, "a red spot: {rs:?}");
    assert!(rs[0].abs_diff(ws[0]) < 6, "as bright in red: {rs:?} {ws:?}");
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
        head: [LayerGpu::SUB_CUT, 0, 0, 0],
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
    let tile: Vec<u8> =
        (0..n * n).flat_map(|i| if (n / 3..2 * n / 3).contains(&(i % n)) { [255, 255] } else { [0, 0] }).collect();
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
fn a_layer_the_ball_doesn_t_wear_is_passed_over_by_a_layer_below_mask() {
    let unworn = |r: LayerGpu| LayerGpu { head: [r.head[0], r.head[1], r.head[2] | LayerGpu::UNWORN, r.head[3]], ..r };
    let frame = white(|f| {
        vec![body(), outline(40.0, 0.0), unworn(outline(120.0, 0.0)), red_pattern(f, 1), mask(0.0, 0.0, 0.0, 0.0)]
    });
    let Some(px) = render_sized(&frame, W, H) else { return };
    let ring = at(&px, 180.0, 0.0);
    assert!(ring[0] > 240 && ring[2] < 20, "red on the worn outline's ring: {ring:?}");
    assert!(at(&px, 0.0, 0.0)[1] > 240, "white inside it, the unworn outline drawing nothing");
    assert!(at(&px, 120.0, 0.0)[1] > 240, "nor its wider band");
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

/// A white ball with dark stripes (a striped pattern of black and white) and a blue outline,
/// its group colour blue, mixed in by `mode` (0 none) at full amount.
fn grouped_ball(frame: &mut Frame, mode: u32, group_record: bool, grouped_outline: bool) {
    let row = frame.gradients.row(&Gradient {
        stops: vec![
            bpe_core::Stop { at: 0.0, color: Color::rgb(0, 0, 0) },
            bpe_core::Stop { at: 0.5, color: Color::rgb(255, 255, 255) },
        ],
        transition: bpe_core::Transition::Steps,
    });
    let stripes = LayerGpu {
        paint: PaintGpu { row: row as f32, pattern: 1.0, scale: 4.0, repeat: 1.0, steps: 1.0, ..PaintGpu::default() },
        ..record(LayerGpu::PATTERN, Blend::Normal)
    };
    let mut ring = outline(20.0, 0.0);
    ring.colour = [1.0, 0.0, 0.0, 1.0];
    if grouped_outline {
        ring.head[2] |= LayerGpu::GROUPED;
    }
    let mut records = vec![body(), stripes, ring];
    if group_record {
        records.push(record(LayerGpu::GROUP, Blend::Normal));
    }
    frame.layered.push(ball(Vec2::ZERO, 200.0, WHITE, 0, records.len() as u32, 1.45));
    frame.layered[0].range[3] = mode;
    frame.layered[0].group = [0.0, 0.0, 1.0, 1.0];
    frame.look_layers = records;
}

/// The brightest and darkest red, green and blue sums inside the ring.
fn inside_extremes(px: &[u8]) -> ([u8; 4], [u8; 4]) {
    let mut samples: Vec<[u8; 4]> = Vec::new();
    for x in (-150..=150).step_by(6) {
        samples.push(at(px, x as f32, 0.0));
    }
    let sum = |p: &[u8; 4]| p[0] as u32 + p[1] as u32 + p[2] as u32;
    let lightest = *samples.iter().max_by_key(|p| sum(p)).unwrap();
    let darkest = *samples.iter().min_by_key(|p| sum(p)).unwrap();
    (lightest, darkest)
}

#[test]
fn every_layer_multiply_turns_white_parts_blue_and_keeps_dark_ones_dark() {
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    grouped_ball(&mut frame, Blend::Multiply.shader_code() + 1, true, false);
    let Some(px) = render_sized(&frame, W, H) else { return };
    save("layers-group-multiply", &px, W, H);
    let (light, dark) = inside_extremes(&px);
    assert!(light[2] > 200 && light[0] < 20 && light[1] < 20, "white went blue: {light:?}");
    assert!(dark[..3].iter().all(|&c| c < 20), "stripes stay dark: {dark:?}");
    let ring = at(&px, 192.0, 0.0);
    assert!(ring[..3].iter().all(|&c| c < 20), "a red ring times blue is dark: {ring:?}");
}

#[test]
fn choosing_the_outline_colours_only_the_outline() {
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    grouped_ball(&mut frame, 1, false, true);
    let Some(px) = render_sized(&frame, W, H) else { return };
    let ring = at(&px, 192.0, 0.0);
    assert!(ring[2] > 200 && ring[0] < 20, "the ring takes the group's blue: {ring:?}");
    let (light, _) = inside_extremes(&px);
    assert!(light[..3].iter().all(|&c| c > 230), "the body stays white: {light:?}");
}

#[test]
fn group_mode_none_changes_nothing() {
    let mut with = Frame { bloom: 0.0, ..Frame::default() };
    grouped_ball(&mut with, 0, true, true);
    let mut without = Frame { bloom: 0.0, ..Frame::default() };
    grouped_ball(&mut without, 0, false, false);
    without.layered[0].group = [0.0; 4];
    let Some(a) = render_sized(&with, W, H) else { return };
    let b = render_sized(&without, W, H).unwrap();
    assert!(a == b, "no group mode, no change");
}

/// What the bit of `range.z` that draws a split ball's upper part is.
const OVER: u32 = 0x1_0000;

#[test]
fn a_ball_split_by_its_blend_layer_draws_the_layers_under_it_and_over_it_apart() {
    let records = vec![body(), outline(20.0, 0.0)];
    let whole = one(records.clone(), WHITE, 200.0, 1.45);
    let mut under = whole.clone();
    under.layered[0].range[2] = 1;
    let mut over = whole.clone();
    over.layered[0].range[2] = 1 | OVER;
    let Some(w) = render_sized(&whole, W, H) else { return };
    let (u, o) = (render_sized(&under, W, H).unwrap(), render_sized(&over, W, H).unwrap());
    save("layers-split-under", &u, W, H);
    save("layers-split-over", &o, W, H);
    let ring = (-190.0, 0.0);
    assert!(at(&w, ring.0, ring.1)[2] > 200 && at(&w, ring.0, ring.1)[0] < 40, "the whole ball has its blue ring");
    // Under: the white body, left out where the solid ring over it covers it.
    assert!(at(&u, 0.0, 0.0)[..3].iter().all(|&c| c > 240), "under: the white body");
    assert!(at(&u, ring.0, ring.1)[..3].iter().all(|&c| c < 10), "under: nothing under the solid ring");
    assert!(at(&o, ring.0, ring.1)[2] > 200, "over: the ring");
    assert_eq!(&at(&o, 0.0, 0.0)[..3], &[0, 0, 0], "over: nothing where only the body is");
}

#[test]
fn a_split_ball_adds_up_to_the_whole_ball_at_its_soft_edge() {
    // An Inside outline over a Blend layer: the lower part mixed in, then the upper part over it,
    // match the ball drawn whole, its soft edge too: no ring of the body round the outline.
    let records = vec![body(), outline(20.0, 0.0)];
    let whole = one(records.clone(), WHITE, 201.0, 1.45);
    let mut under = one(records.clone(), WHITE, 201.0, 1.45);
    under.layered[0].range[2] = 1;
    let mut over = one(records, WHITE, 201.0, 1.45);
    over.layered[0].range[2] = 1 | OVER;
    let mut split = Frame { bloom: 0.0, ..Frame::default() };
    let layer = |place, frame| BlendLayer { blend: Blend::Normal, amount: 1.0, each: false, z: 0, place, frame };
    split.layers.push(layer(Place::Blend, under));
    split.layers.push(layer(Place::Over, over));
    let Some(w) = render_sized(&whole, W, H) else { return };
    let s = render_sized(&split, W, H).unwrap();
    save("layers-split-edge", &s, W, H);
    for k in 0..40 {
        let x = 190.0 + k as f32 * 0.5;
        let (a, b) = (at(&w, x, 0.0), at(&s, x, 0.0));
        let worst = (0..3).map(|i| a[i].abs_diff(b[i])).max().unwrap();
        assert!(worst <= 3, "at x {x}: whole {a:?}, split {b:?}");
    }
}

#[test]
fn a_split_ball_s_masks_above_the_blend_layer_read_the_same_layers_as_drawn_whole() {
    // Over a Blend layer, an outline and a pattern clipped to it by a layer-below mask: the lower
    // part must leave room only for the outline, not the whole ball.
    let build = |range_z: u32| {
        let mut frame = white(|f| vec![body(), outline(40.0, 0.0), red_pattern(f, 1), mask(0.0, 0.0, 0.0, 0.0)]);
        frame.layered[0].range[2] = range_z;
        frame
    };
    let whole = build(0);
    let mut split = Frame { bloom: 0.0, ..Frame::default() };
    // A layer's things paint from the frame's own gradients: the red one is the first there too.
    split.gradients.row(&Gradient::solid(Color::rgb(255, 0, 0)));
    let layer = |place, frame| BlendLayer { blend: Blend::Normal, amount: 1.0, each: false, z: 0, place, frame };
    split.layers.push(layer(Place::Blend, build(1)));
    split.layers.push(layer(Place::Over, build(1 | OVER)));
    let Some(w) = render_sized(&whole, W, H) else { return };
    let s = render_sized(&split, W, H).unwrap();
    save("layers-split-masked", &s, W, H);
    for (x, y) in [(0.0, 0.0), (100.0, 0.0), (180.0, 0.0), (0.0, -185.0), (199.0, 0.0)] {
        let (a, b) = (at(&w, x, y), at(&s, x, y));
        let worst = (0..3).map(|i| a[i].abs_diff(b[i])).max().unwrap();
        assert!(worst <= 3, "at ({x}, {y}): whole {a:?}, split {b:?}");
    }
}

/// A record's own gradient from `a` to `b`, straight at `angle` (pattern 1) or outward (2).
fn own(frame: &mut Frame, a: Color, b: Color, pattern: f32, angle: f32) -> PaintGpu {
    let row = frame.gradients.row(&Gradient::even(&[a, b]));
    PaintGpu { row: row as f32, pattern, angle, scale: 1.0, ..PaintGpu::default() }
}

#[test]
fn a_body_gradient_runs_across_each_ball_and_an_outline_s_round_one_across_its_ring() {
    let mut frame = one(vec![], WHITE, 200.0, 1.45);
    let mut b = body();
    b.paint = own(&mut frame, Color::rgb(255, 0, 0), Color::rgb(0, 0, 255), 1.0, 0.0);
    frame.look_layers = vec![b];
    frame.layered[0].range[1] = 1;
    let Some(px) = render_sized(&frame, W, H) else { return };
    save("layers-body-gradient", &px, W, H);
    let (left, right) = (at(&px, -150.0, 0.0), at(&px, 150.0, 0.0));
    assert!(left[0] > 200 && left[0] > left[2] + 100, "red on the left: {left:?}");
    assert!(right[2] > 200 && right[2] > right[0] + 100, "blue on the right: {right:?}");

    let mut frame = one(vec![], WHITE, 200.0, 1.45);
    let mut ring = outline(40.0, 0.0);
    ring.paint = own(&mut frame, Color::rgb(255, 0, 0), Color::rgb(0, 255, 0), 2.0, 0.0);
    frame.look_layers = vec![body(), ring];
    frame.layered[0].range[1] = 2;
    let px = render_sized(&frame, W, H).unwrap();
    let (inner, outer) = (at(&px, 0.0, -165.0), at(&px, 0.0, -197.0));
    assert!(inner[0] > inner[1], "red at the ring's inner edge: {inner:?}");
    assert!(outer[1] > outer[0], "green at its outer edge: {outer:?}");
}

#[test]
fn shine_materials_and_softness_light_the_ball_differently() {
    let lit = |material: f32, sharpness: f32| {
        let mut s = shine();
        s.p0[3] = material;
        s.p1[0] = sharpness;
        render_sized(&one(vec![body(), s], [0.3, 0.3, 0.3, 1.0], 200.0, 1.45), W, H)
    };
    let Some(glossy) = lit(0.0, 0.0) else { return };
    let mut seen = vec![glossy.clone()];
    for m in [1.0, 2.0, 3.0] {
        let px = lit(m, 0.0).unwrap();
        save(&format!("layers-shine-material-{m}"), &px, W, H);
        assert!(seen.iter().all(|p| *p != px), "material {m} looks like no other");
        seen.push(px);
    }
    // A sharper spot lights fewer pixels brightly, a softer one more.
    let bright = |px: &[u8]| px.chunks(4).filter(|p| p[0] > 200).count();
    let (sharp, soft) = (lit(0.0, 3.0).unwrap(), lit(0.0, -0.75).unwrap());
    assert!(bright(&sharp) < bright(&glossy) && bright(&glossy) < bright(&soft));
}

#[test]
fn a_higher_z_order_draws_over_and_a_lower_one_under() {
    let red = [1.0, 0.0, 0.0, 1.0];
    let with = |z: i32, place: Place| {
        let mut frame = one(vec![body()], WHITE, 200.0, 1.45);
        let raised = one(vec![body()], red, 120.0, 1.45);
        frame.layers.push(BlendLayer { blend: Blend::Normal, amount: 1.0, each: false, z, place, frame: raised });
        frame
    };
    let Some(over) = render_sized(&with(1, Place::Front), W, H) else { return };
    let under = render_sized(&with(-1, Place::Front), W, H).unwrap();
    let back = render_sized(&with(3, Place::Back), W, H).unwrap();
    let middle = |px: &[u8]| at(px, 0.0, 0.0);
    assert!(middle(&over)[0] > 240 && middle(&over)[1] < 10, "the raised red ball is on top: {:?}", middle(&over));
    assert!(middle(&under)[..3].iter().all(|&c| c > 240), "the lowered one is under the white: {:?}", middle(&under));
    assert!(middle(&back)[0] > 240 && middle(&back)[1] < 10, "a raised shape's layer draws over too");
}

#[test]
fn things_stuck_to_the_screen_draw_in_the_same_order_as_the_rest() {
    // A white ball stuck to the screen, and a smaller red one in a layer of the overlay: the
    // red one sits where its place and z-order say, as it would in the frame itself.
    let red = [1.0, 0.0, 0.0, 1.0];
    let with = |z: i32, place: Place| {
        let mut stuck = one(vec![body()], WHITE, 200.0, 1.45);
        let layer = one(vec![body()], red, 120.0, 1.45);
        stuck.layers.push(BlendLayer { blend: Blend::Normal, amount: 1.0, each: false, z, place, frame: layer });
        let mut frame = Frame { bloom: 0.0, ..Frame::default() };
        frame.overlay = Some(Box::new(stuck));
        frame
    };
    let middle = |z: i32, place: Place| render_sized(&with(z, place), W, H).map(|px| at(&px, 0.0, 0.0));
    let Some(blended) = middle(0, Place::Blend) else { return };
    let is_red = |c: [u8; 4]| c[0] > 240 && c[1] < 10;
    let is_white = |c: [u8; 4]| c[..3].iter().all(|&v| v > 240);
    assert!(is_red(blended), "a blended thing sits over the balls: {blended:?}");
    for (z, place) in [(0, Place::Behind), (0, Place::BehindOver), (-1, Place::Front), (-1, Place::Back)] {
        let c = middle(z, place).unwrap();
        assert!(is_white(c), "z {z} {place:?} draws under the stuck ball: {c:?}");
    }
    let raised = middle(1, Place::Front).unwrap();
    assert!(is_red(raised), "a raised one draws over: {raised:?}");
}

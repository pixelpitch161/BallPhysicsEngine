//! Picture tests for shapes (the paths pipeline), on the software GPU.

use crate::Frame;
use crate::pictures::{pixel_sized, render_sized, save};
use bpe_runtime::CameraView;
use glam::Vec2;

const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// A unit square's corners, clockwise from the top left.
fn square() -> Vec<Vec2> {
    vec![Vec2::new(-1.0, -1.0), Vec2::new(1.0, -1.0), Vec2::new(1.0, 1.0), Vec2::new(-1.0, 1.0)]
}

fn bright(p: [u8; 4]) -> bool {
    p[0] > 200 && p[1] > 200 && p[2] > 200
}

fn dark(p: [u8; 4]) -> bool {
    p[0] < 30 && p[1] < 30 && p[2] < 30
}

#[test]
fn hollow_and_filled_shapes_draw_white_on_black() {
    // Full size, so one world pixel is one picture pixel.
    const W: u32 = 1080;
    const H: u32 = 1920;
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    let hollow_at = Vec2::new(-250.0, -500.0);
    let solid_at = Vec2::new(250.0, -500.0);
    let ring_at = Vec2::new(0.0, 300.0);
    // A hollow square 200 across with a 21 px band (its edge runs through a pixel's middle), a filled one, and a hollow circle.
    frame.push_path(hollow_at, 0.0, 100.0, 10.5, Some(&square()), WHITE, 2.0);
    frame.push_path(solid_at, 0.3, 100.0, 0.0, Some(&square()), WHITE, 3.0);
    frame.push_path(ring_at, 0.0, 150.0, 6.0, None, WHITE, 4.0);
    assert_eq!(frame.path_points.len(), 8, "the circle names no points");
    let Some(pixels) = render_sized(&frame, W, H) else { return };
    save("paths", &pixels, W, H);
    let at = |p: Vec2| pixel_sized(&pixels, W, H, p);
    // The hollow square: its band's middle is white, inside and 3 px outside the band are black.
    assert!(bright(at(hollow_at + Vec2::new(100.0, 0.5))), "{:?}", at(hollow_at + Vec2::new(100.0, 0.5)));
    assert!(dark(at(hollow_at)), "inside the band");
    assert!(dark(at(hollow_at + Vec2::new(113.6, 0.5))), "outside the band");
    // Its edge pixel is in between.
    let edge = at(hollow_at + Vec2::new(110.0, 0.5));
    assert!(!bright(edge) && !dark(edge), "{edge:?}");
    // The filled square is white inside, black outside.
    assert!(bright(at(solid_at)) && bright(at(solid_at + Vec2::new(50.0, 20.0))));
    assert!(dark(at(solid_at + Vec2::new(160.0, 0.0))));
    // The hollow circle.
    assert!(bright(at(ring_at + Vec2::new(150.5, 0.5))));
    assert!(dark(at(ring_at)) && dark(at(ring_at + Vec2::new(160.0, 0.0))));
}

#[test]
fn a_self_crossing_outline_fills_even_odd() {
    const W: u32 = 1080;
    const H: u32 = 1920;
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    // A bow tie: two triangles meeting at the middle.
    let bow = [Vec2::new(-1.0, -1.0), Vec2::new(1.0, 1.0), Vec2::new(1.0, -1.0), Vec2::new(-1.0, 1.0)];
    frame.push_path(Vec2::ZERO, 0.0, 200.0, 0.0, Some(&bow), WHITE, 2.0);
    let Some(pixels) = render_sized(&frame, W, H) else { return };
    let at = |p: Vec2| pixel_sized(&pixels, W, H, p);
    assert!(bright(at(Vec2::new(150.0, 0.5))) && bright(at(Vec2::new(-150.0, 0.5))), "the two triangles");
    assert!(dark(at(Vec2::new(0.5, 150.0))) && dark(at(Vec2::new(0.5, -150.0))), "the gaps");
}

#[test]
fn culling_keeps_a_shape_whose_reach_touches_the_view() {
    let mut frame = Frame::default();
    frame.push_path(Vec2::new(5000.0, 0.0), 0.0, 4500.0, 4.0, None, WHITE, 2.0);
    frame.push_path(Vec2::new(9000.0, 0.0), 0.0, 100.0, 4.0, None, WHITE, 3.0);
    frame.camera = CameraView { center: Vec2::new(0.0, 1.0), ..CameraView::default() };
    frame.cull();
    assert_eq!(frame.paths.len(), 1, "the big one reaches into the view, the small far one doesn't");
    assert_eq!(frame.paths[0].c[0], 2.0);
}

// ---- Shapes with layers (shape_walk.wgsl) ----

use crate::{LayerGpu, PaintGpu, PathInstance};
use bpe_core::{Color, Gradient};

const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];

fn body_record() -> LayerGpu {
    LayerGpu { amount: [1.0, 0.0, 0.0, 0.0], colour: WHITE, ..LayerGpu::ZERO }
}

/// A Colour and gradient record of one colour.
fn paint_record(colour: [f32; 4]) -> LayerGpu {
    LayerGpu { head: [LayerGpu::SHAPE_PAINT, 0, 0, 0], amount: [1.0, 0.0, 0.0, 0.0], colour, ..LayerGpu::ZERO }
}

/// A gradient from red to blue, running by `runs`, across along x from -1 to 1.
fn red_to_blue(frame: &mut Frame, code: u32, runs: f32) -> LayerGpu {
    let row = frame.gradients.row(&Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)]));
    LayerGpu {
        head: [code, 0, 0, 0],
        amount: [1.0, 0.0, 0.0, 0.0],
        p0: [runs, 0.0, 0.0, 0.0],
        p1: [1.0, 0.0, -1.0, 1.0],
        paint: PaintGpu { row: row as f32, ..PaintGpu::default() },
        ..LayerGpu::ZERO
    }
}

/// Gives the frame's shape `i` the records `records`, reaching `past` pixels past its outline.
fn wear(frame: &mut Frame, i: usize, records: Vec<LayerGpu>, past: f32) {
    let first = frame.look_layers.len() as u32;
    frame.paths[i].layers = [first, records.len() as u32, PathInstance::NO_PROGRAM, 0];
    frame.paths[i].c[2] = past;
    frame.look_layers.extend(records);
}

fn reddish(p: [u8; 4]) -> bool {
    p[0] > 150 && p[0] as i32 > p[2] as i32 + 80 && p[1] < 60
}

fn bluish(p: [u8; 4]) -> bool {
    p[2] > 150 && p[2] as i32 > p[0] as i32 + 80 && p[1] < 60
}

/// A 400 × 200 rectangle's corners in units of 200 px.
fn wide() -> Vec<Vec2> {
    vec![Vec2::new(-1.0, -0.5), Vec2::new(1.0, -0.5), Vec2::new(1.0, 0.5), Vec2::new(-1.0, 0.5)]
}

#[test]
fn a_gradient_runs_across_a_shape_from_edge_to_edge_and_along_its_edge() {
    const W: u32 = 1080;
    const H: u32 = 1920;
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    let rect_at = Vec2::new(0.0, -500.0);
    let ring_at = Vec2::new(0.0, 400.0);
    frame.push_path(rect_at, 0.0, 200.0, 0.0, Some(&wide()), WHITE, 2.0);
    frame.push_path(ring_at, 0.0, 300.0, 10.0, None, WHITE, 3.0);
    let across = red_to_blue(&mut frame, LayerGpu::SHAPE_PAINT, 0.0);
    wear(&mut frame, 0, vec![body_record(), across], 0.0);
    let along = red_to_blue(&mut frame, LayerGpu::SHAPE_PAINT, 2.0);
    wear(&mut frame, 1, vec![body_record(), along], 0.0);
    let Some(pixels) = render_sized(&frame, W, H) else { return };
    save("shape-gradients", &pixels, W, H);
    let at = |p: Vec2| pixel_sized(&pixels, W, H, p);
    assert!(reddish(at(rect_at + Vec2::new(-190.0, 0.5))), "{:?}", at(rect_at + Vec2::new(-190.0, 0.5)));
    assert!(bluish(at(rect_at + Vec2::new(190.0, 0.5))), "{:?}", at(rect_at + Vec2::new(190.0, 0.5)));
    let middle = at(rect_at + Vec2::new(0.5, 0.5));
    assert!(middle[0] > 60 && middle[2] > 60, "purple in the middle: {middle:?}");
    assert!(dark(at(rect_at + Vec2::new(0.5, 110.0))), "nothing past the edge");
    // Along the edge: red at the top, purple at the bottom, blue towards the left (three quarters round).
    let top = at(ring_at + Vec2::new(4.0, -300.0));
    let left = at(ring_at + Vec2::new(-300.0, 0.5));
    assert!(reddish(top), "{top:?}");
    assert!(left[2] > left[0] + 40, "{left:?}");
    assert!(dark(at(ring_at)), "the ring's hole");
}

#[test]
fn an_outline_sits_on_the_edge_in_its_own_colour_or_the_colour_under_it() {
    const W: u32 = 1080;
    const H: u32 = 1920;
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    let own_at = Vec2::new(-250.0, -400.0);
    let same_at = Vec2::new(250.0, -400.0);
    frame.push_path(own_at, 0.0, 100.0, 0.0, Some(&square()), WHITE, 2.0);
    frame.push_path(same_at, 0.0, 100.0, 0.0, Some(&square()), WHITE, 3.0);
    // An own green line 10 px wide outside a red square.
    let outside = LayerGpu {
        head: [LayerGpu::OUTLINE, 0, 0, 0],
        amount: [1.0, 0.0, 0.0, 0.0],
        p0: [10.0, 2.0, 0.0, 0.0],
        colour: GREEN,
        ..LayerGpu::ZERO
    };
    wear(&mut frame, 0, vec![body_record(), paint_record(RED), outside], 10.0);
    // A line inside the edge taking the red under it, darker.
    let same = LayerGpu { head: [LayerGpu::OUTLINE, 0, LayerGpu::FLAG, 0], p0: [10.0, 0.0, 0.0, 0.0], ..outside };
    wear(&mut frame, 1, vec![body_record(), paint_record(RED), same], 0.0);
    let Some(pixels) = render_sized(&frame, W, H) else { return };
    save("shape-outlines", &pixels, W, H);
    let at = |p: Vec2| pixel_sized(&pixels, W, H, p);
    let line = at(own_at + Vec2::new(105.0, 0.5));
    assert!(line[1] > 200 && line[0] < 40, "the green line outside the edge: {line:?}");
    assert!(reddish(at(own_at)), "the red paint inside");
    assert!(dark(at(own_at + Vec2::new(125.0, 0.5))), "past the line");
    let darker = at(same_at + Vec2::new(95.0, 0.5));
    let inside = at(same_at);
    assert!(reddish(inside) && darker[1] < 30, "{inside:?} {darker:?}");
    assert!(darker[0] + 20 < inside[0] && darker[0] > 60, "a darker red line: {darker:?} vs {inside:?}");
}

#[test]
fn a_glow_lights_outside_the_edge_and_fades_out() {
    const W: u32 = 1080;
    const H: u32 = 1920;
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    frame.push_path(Vec2::ZERO, 0.0, 100.0, 0.0, None, WHITE, 2.0);
    let glow = LayerGpu {
        head: [LayerGpu::GLOW, 0, 0, 0],
        amount: [1.0, 1.0, 0.0, 0.0],
        p0: [30.0, 0.0, 0.0, 0.0],
        colour: RED,
        ..LayerGpu::ZERO
    };
    wear(&mut frame, 0, vec![body_record(), glow], 66.0);
    let Some(pixels) = render_sized(&frame, W, H) else { return };
    save("shape-glow", &pixels, W, H);
    let at = |p: Vec2| pixel_sized(&pixels, W, H, p);
    let near = at(Vec2::new(110.0, 0.5));
    assert!(near[0] > 100 && near[1] < 40, "red light just outside: {near:?}");
    assert!(bright(at(Vec2::new(0.5, 0.5))), "the white body inside");
    assert!(dark(at(Vec2::new(165.0, 0.5))), "gone by the quad's edge");
}

#[test]
fn a_body_alone_draws_the_same_band_as_the_plain_shape() {
    const W: u32 = 540;
    const H: u32 = 960;
    let mut plain = Frame { bloom: 0.0, ..Frame::default() };
    plain.push_path(Vec2::ZERO, 0.4, 200.0, 8.0, Some(&square()), WHITE, 2.0);
    let mut layered = plain.clone();
    wear(&mut layered, 0, vec![body_record()], 0.0);
    let Some(a) = render_sized(&plain, W, H) else { return };
    let b = render_sized(&layered, W, H).unwrap();
    let differs = a.iter().zip(&b).filter(|(x, y)| x.abs_diff(**y) > 2).count();
    assert!(differs < 20, "{differs} values differ");
}

#[test]
fn a_big_shape_s_glow_is_as_bright_on_its_sides_as_on_its_diagonals() {
    const W: u32 = 1080;
    const H: u32 = 1920;
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    frame.push_path(Vec2::ZERO, 0.0, 400.0, 0.0, None, WHITE, 2.0);
    let glow = LayerGpu {
        head: [LayerGpu::GLOW, 0, 0, 0],
        amount: [1.0, 1.0, 0.0, 0.0],
        p0: [30.0, 0.0, 0.0, 0.0],
        colour: RED,
        ..LayerGpu::ZERO
    };
    wear(&mut frame, 0, vec![body_record(), glow], 66.0);
    let Some(pixels) = render_sized(&frame, W, H) else { return };
    let at = |p: Vec2| pixel_sized(&pixels, W, H, p);
    let side = at(Vec2::new(415.0, 0.5));
    let diagonal = at(Vec2::from_angle(std::f32::consts::FRAC_PI_4) * 415.0);
    assert!(side[0] > 60, "{side:?}");
    assert!(side[0].abs_diff(diagonal[0]) < 12, "side {side:?} vs diagonal {diagonal:?}");
}

#[test]
fn a_shape_blended_behind_the_balls_never_tints_them() {
    use crate::{BlendLayer, LayeredInstance};
    use bpe_core::Blend;
    const W: u32 = 540;
    const H: u32 = 960;
    // A white ball in front, a black filled square multiplied in a layer.
    let ball = |frame: &mut Frame| {
        frame.look_layers.push(body_record());
        frame.layered.push(LayeredInstance {
            center: [0.0, 0.0],
            radius: 100.0,
            seed: 0.3,
            body: WHITE,
            stretch: [1.0, 0.0, 1.0, 1.0],
            form: [0.0, 0.15, 0.0, 1.45],
            misc: [1.0, 3.0, 0.0, 0.0],
            vary: crate::NO_VARY,
            range: [0, 1, 0, 0],
            group: [0.0; 4],
            place: [0.0; 4],
        });
    };
    let picture = |behind: bool| {
        let mut frame = Frame { bloom: 0.0, ..Frame::default() };
        ball(&mut frame);
        let mut layer = Frame::default();
        layer.push_path(Vec2::ZERO, 0.0, 300.0, 0.0, Some(&square()), [0.0, 0.0, 0.0, 1.0], 2.0);
        frame.layers.push(BlendLayer { blend: Blend::Multiply, amount: 1.0, each: false, behind, frame: layer });
        render_sized(&frame, W, H)
    };
    let Some(behind) = picture(true) else { return };
    let over = picture(false).unwrap();
    let at = |px: &[u8]| pixel_sized(px, W, H, Vec2::new(0.5, 0.5));
    assert!(bright(at(&behind)), "the ball stays white: {:?}", at(&behind));
    assert!(dark(at(&over)), "a layer over the balls darkens them: {:?}", at(&over));
}

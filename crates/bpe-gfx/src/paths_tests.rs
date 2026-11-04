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

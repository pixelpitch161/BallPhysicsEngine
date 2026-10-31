//! Picture tests for the scene's colour grade, on the software GPU.

use crate::paint_tests::gpu;
use crate::{BackgroundPaint, BallInstance, Frame, Grade, Renderer};
use bpe_core::{Color, Gradient};

const N: u32 = 64;
/// Linear light that encodes to sRGB 0.5.
const MID: f32 = 0.214_041;

/// A 64×64 world on a 64×64 picture, no bloom, its background running from `top` to `bottom`
/// (linear light) down the picture.
fn two_tone(top: f32, bottom: f32) -> Frame {
    let (a, b) = ([top, top, top, 1.0], [bottom, bottom, bottom, 1.0]);
    Frame {
        width: N,
        height: N,
        video: (N, N),
        bloom: 0.0,
        background: BackgroundPaint {
            row: 0,
            ends: Some([a, b]),
            mode: 1.0,
            place: [90.0, N as f32, 0.0, 0.0],
            ..BackgroundPaint::default()
        },
        ..Frame::default()
    }
}

fn flat(v: f32) -> Frame {
    let mut f = two_tone(v, v);
    f.background.mode = 0.0;
    f
}

fn graded(mut frame: Frame, grade: Grade) -> Frame {
    frame.grade = grade;
    frame
}

/// The frame drawn on the software GPU, or `None` without one.
fn draw(frame: &Frame) -> Option<Vec<u8>> {
    let (device, queue) = gpu()?;
    let mut renderer = Renderer::new(&device, N, N);
    renderer.render(&device, &queue, frame);
    Some(renderer.read_output(&device, &queue).unwrap())
}

fn at(pixels: &[u8], x: u32, y: u32) -> [i32; 3] {
    let i = ((y * N + x) * 4) as usize;
    [pixels[i] as i32, pixels[i + 1] as i32, pixels[i + 2] as i32]
}

const CENTRE: (u32, u32) = (32, 32);

fn centre(p: &[u8]) -> [i32; 3] {
    at(p, CENTRE.0, CENTRE.1)
}

#[test]
fn grade_at_defaults_is_byte_identical() {
    let mut frame = two_tone(0.02, 0.6);
    frame.balls.push(BallInstance {
        center: [5.0, -3.0],
        radius: 12.0,
        glow: 1.0,
        color: [0.9, 0.3, 0.1, 1.0],
        ..BallInstance::default()
    });
    frame.bloom = 0.4;
    let Some(plain) = draw(&frame) else { return };
    // Every value at its default, a map that only counts once its amount is above 0, and the
    // negative zero a keyframe passing through 0 can land on.
    let at_default = Grade {
        brightness: -0.0,
        contrast: 0.0,
        map: Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)]),
        ..Grade::default()
    };
    assert!(at_default.is_default());
    let again = draw(&graded(frame, at_default)).unwrap();
    assert!(plain == again, "the default grade changed the picture");
}

#[test]
fn brightness_up_brightens_and_contrast_spreads() {
    let Some(plain) = draw(&flat(MID)) else { return };
    let bright = draw(&graded(flat(MID), Grade { brightness: 0.5, ..Grade::default() })).unwrap();
    let (p, b) = (centre(&plain), centre(&bright));
    assert!(b[0] > p[0] + 40, "{p:?} -> {b:?}");

    let tones = two_tone(0.1, 0.5);
    let plain = draw(&tones).unwrap();
    let spread = draw(&graded(tones, Grade { contrast: 0.5, ..Grade::default() })).unwrap();
    let gap = |p: &[u8]| at(p, 32, 60)[0] - at(p, 32, 3)[0];
    assert!(gap(&spread) > gap(&plain) + 20, "{} -> {}", gap(&plain), gap(&spread));
}

#[test]
fn saturation_minus_one_is_grey() {
    let mut frame = flat(0.0);
    let c = [0.8, 0.1, 0.3, 1.0];
    frame.background.ends = Some([c, c]);
    let Some(grey) = draw(&graded(frame, Grade { saturation: -1.0, ..Grade::default() })) else { return };
    let p = centre(&grey);
    assert!((p[0] - p[1]).abs() <= 1 && (p[1] - p[2]).abs() <= 1, "{p:?}");
}

#[test]
fn warm_adds_red_takes_blue() {
    let Some(plain) = draw(&flat(MID)) else { return };
    let warm = draw(&graded(flat(MID), Grade { warmth: 1.0, ..Grade::default() })).unwrap();
    let (p, w) = (centre(&plain), centre(&warm));
    assert!(w[0] > p[0] + 20 && w[2] < p[2] - 20, "{p:?} -> {w:?}");
    assert!((w[1] - p[1]).abs() <= 1, "{p:?} -> {w:?}");
}

#[test]
fn tint_positive_is_magenta() {
    let Some(plain) = draw(&flat(MID)) else { return };
    let tinted = draw(&graded(flat(MID), Grade { tint: 1.0, ..Grade::default() })).unwrap();
    let (p, t) = (centre(&plain), centre(&tinted));
    assert!(t[1] < p[1] - 20, "{p:?} -> {t:?}");
    assert!((t[0] - p[0]).abs() <= 1 && (t[2] - p[2]).abs() <= 1, "{p:?} -> {t:?}");
}

#[test]
fn vignette_darkens_corners_not_centre() {
    let Some(plain) = draw(&flat(MID)) else { return };
    let v = draw(&graded(flat(MID), Grade { vignette: 1.0, ..Grade::default() })).unwrap();
    let (p, c) = (centre(&plain), centre(&v));
    assert!((0..3).all(|k| (p[k] - c[k]).abs() <= 1), "{p:?} -> {c:?}");
    let corner = at(&v, 0, 0);
    assert!(corner[0] < at(&plain, 0, 0)[0] - 40, "{corner:?}");
}

#[test]
fn vignette_size_edges_are_finite() {
    for size in [0.0, 1.0] {
        let Some(v) = draw(&graded(flat(MID), Grade { vignette: 1.0, vignette_size: size, ..Grade::default() })) else {
            return;
        };
        let c = centre(&v);
        // A NaN comes out of the 8-bit target as 0, so the centre stays lit and the corners
        // are no brighter than it.
        assert!(c[0] > 100, "size {size}: centre {c:?}");
        for (x, y) in [(0, 0), (N - 1, 0), (0, N - 1), (N - 1, N - 1)] {
            let p = at(&v, x, y);
            assert!(p[0] <= c[0] + 1, "size {size}: corner {p:?} brighter than centre {c:?}");
        }
    }
}

#[test]
fn gradient_map_full_maps_black_to_first_stop() {
    let grade = Grade {
        map: Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)]),
        map_amount: 1.0,
        ..Grade::default()
    };
    let Some(mapped) = draw(&graded(flat(0.0), grade)) else { return };
    let p = centre(&mapped);
    assert!(p[0] >= 253 && p[1] <= 1 && p[2] <= 1, "{p:?}");
}

#[test]
fn defaults_follow_the_ruling_and_ignore_an_unused_map() {
    let d = Grade::default();
    assert_eq!(
        (d.brightness, d.contrast, d.saturation, d.warmth, d.tint, d.vignette, d.vignette_size, d.map_amount),
        (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0)
    );
    assert_eq!(d.map, Gradient::even(&[Color::rgb(0, 0, 0), Color::WHITE]));
    assert!(d.is_default());
    let red = Gradient::solid(Color::rgb(255, 0, 0));
    assert!(Grade { map: red.clone(), ..Grade::default() }.is_default());
    assert!(!Grade { map: red, map_amount: 0.5, ..Grade::default() }.is_default());
    assert!(!Grade { vignette_size: 0.4, ..Grade::default() }.is_default());
}

#[test]
fn clearing_a_frame_resets_its_grade() {
    let mut frame = Frame { grade: Grade { warmth: 0.5, ..Grade::default() }, ..Frame::default() };
    frame.clear();
    assert!(frame.grade.is_default());
}

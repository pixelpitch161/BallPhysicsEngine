//! Picture tests for the colour grade, on the software GPU (`test_device`).

use crate::frame::{GradeBasic, GradeMask, GradePanel, GradeStack, GradeToneCurve};
use crate::{BackgroundPaint, Frame, Renderer};
use bpe_core::CurvePoint;

const N: u32 = 64;

/// A 64×64 world on a 64×64 picture, no bloom, its background running from `top` to `bottom`
/// (linear light, any brightness) down the picture.
fn two_tone(top: [f32; 3], bottom: [f32; 3]) -> Frame {
    let (a, b) = ([top[0], top[1], top[2], 1.0], [bottom[0], bottom[1], bottom[2], 1.0]);
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

fn flat(c: [f32; 3]) -> Frame {
    let mut f = two_tone(c, c);
    f.background.mode = 0.0;
    f
}

fn grey(v: f32) -> Frame {
    flat([v; 3])
}

fn panels(panels: Vec<GradePanel>) -> GradeStack {
    GradeStack { masks: vec![GradeMask { amount: 1.0, panels }], split: None }
}

fn basic(b: GradeBasic) -> GradeStack {
    panels(vec![GradePanel::Basic(b)])
}

fn graded(mut frame: Frame, grade: GradeStack) -> Frame {
    frame.grade = grade;
    frame
}

/// The frame drawn on the software GPU (`None` only when GPU tests are skipped on purpose).
fn draw(frame: &Frame) -> Option<Vec<u8>> {
    let (device, queue) = crate::test_device().ok()?;
    let mut renderer = Renderer::new(&device, N, N);
    renderer.render(&device, &queue, frame);
    Some(renderer.read_output(&device, &queue).unwrap())
}

fn at(pixels: &[u8], x: u32, y: u32) -> [i32; 3] {
    let i = ((y * N + x) * 4) as usize;
    [pixels[i] as i32, pixels[i + 1] as i32, pixels[i + 2] as i32]
}

fn centre(p: &[u8]) -> [i32; 3] {
    at(p, 32, 32)
}

/// sRGB byte to linear light.
fn linear(v: i32) -> f64 {
    crate::grade::maths::untone(v as f64 / 255.0)
}

fn close(a: [i32; 3], b: [i32; 3], by: i32) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() <= by)
}

#[test]
fn no_grade_and_a_grade_at_its_defaults_draw_the_same_bytes() {
    let mut frame = two_tone([0.02, 0.01, 0.0], [0.6, 0.7, 2.0]);
    crate::pictures::plain_ball(&mut frame, glam::Vec2::new(5.0, -3.0), 12.0, [0.9, 0.3, 0.1, 1.0], 0.0);
    frame.bloom = 0.4;
    let Some(plain) = draw(&frame) else { return };
    let defaults = panels(vec![
        GradePanel::Basic(GradeBasic { exposure: -0.0, ..GradeBasic::default() }),
        GradePanel::ToneCurve(GradeToneCurve::default()),
    ]);
    assert!(defaults.is_identity());
    let again = draw(&graded(frame, defaults)).unwrap();
    assert!(plain == again, "a grade at its defaults changed the picture");
}

#[test]
fn panels_that_do_almost_nothing_change_nothing_by_more_than_a_step() {
    // Run (not skipped), every working space visited: a hair of exposure, of contrast, of
    // saturation and a point curve a hair off the line.
    let mut frame = two_tone([0.0, 0.0, 0.0], [1.0, 0.8, 0.6]);
    crate::pictures::plain_ball(&mut frame, glam::Vec2::new(0.0, 0.0), 14.0, [0.1, 0.5, 0.9, 1.0], 0.0);
    let Some(plain) = draw(&frame) else { return };
    let tiny = panels(vec![
        GradePanel::Basic(GradeBasic { exposure: 1e-6, contrast: 1e-4, saturation: 1e-4, ..GradeBasic::default() }),
        GradePanel::ToneCurve(GradeToneCurve {
            rgb: vec![
                CurvePoint::new(0.0, 0.0, 0.0),
                CurvePoint::new(0.5, 0.5001, 0.0),
                CurvePoint::new(1.0, 1.0, 0.0),
            ],
            ..GradeToneCurve::default()
        }),
    ]);
    assert!(!tiny.is_identity());
    let graded = draw(&graded(frame, tiny)).unwrap();
    for y in 0..N {
        for x in 0..N {
            assert!(
                close(at(&plain, x, y), at(&graded, x, y), 1),
                "({x}, {y}): {:?} vs {:?}",
                at(&plain, x, y),
                at(&graded, x, y)
            );
        }
    }
}

#[test]
fn exposure_plus_one_doubles_linear_light() {
    let v = 0.18;
    let Some(plain) = draw(&grey(v)) else { return };
    let doubled = draw(&graded(grey(v), basic(GradeBasic { exposure: 1.0, ..GradeBasic::default() }))).unwrap();
    let want = draw(&grey(2.0 * v)).unwrap();
    assert!(close(centre(&doubled), centre(&want), 1), "{:?} vs {:?}", centre(&doubled), centre(&want));
    let ratio = linear(centre(&doubled)[1]) / linear(centre(&plain)[1]);
    assert!((ratio - 2.0).abs() < 0.05, "{ratio}");
}

#[test]
fn temp_warms_a_grey_and_keeps_its_brightness_and_tint_turns_it_magenta() {
    let v = 0.2;
    let Some(plain) = draw(&grey(v)) else { return };
    let warm = centre(&draw(&graded(grey(v), basic(GradeBasic { temp: 60.0, ..GradeBasic::default() }))).unwrap());
    assert!(warm[0] > warm[1] && warm[1] > warm[2], "{warm:?}");
    let luma = |c: [i32; 3]| 0.2126 * linear(c[0]) + 0.7152 * linear(c[1]) + 0.0722 * linear(c[2]);
    assert!((luma(warm) - luma(centre(&plain))).abs() < 0.006, "{} vs {}", luma(warm), luma(centre(&plain)));
    let magenta = centre(&draw(&graded(grey(v), basic(GradeBasic { tint: 60.0, ..GradeBasic::default() }))).unwrap());
    assert!(magenta[1] < magenta[0] && magenta[1] < magenta[2], "{magenta:?}");
}

#[test]
fn highlights_down_brings_a_glow_above_white_back_under_white() {
    let Some(plain) = draw(&grey(3.0)) else { return };
    assert_eq!(centre(&plain), [255; 3], "3.0 rolls off to white");
    let down = draw(&graded(grey(3.0), basic(GradeBasic { highlights: -100.0, ..GradeBasic::default() }))).unwrap();
    assert!(centre(&down).iter().all(|&c| c < 250), "{:?}", centre(&down));
    // The darks are left alone.
    let dark = draw(&graded(grey(0.05), basic(GradeBasic { highlights: -100.0, ..GradeBasic::default() }))).unwrap();
    assert!(close(centre(&dark), centre(&draw(&grey(0.05)).unwrap()), 1));
}

#[test]
fn saturation_down_greys_a_colour() {
    let red = flat([0.6, 0.1, 0.05]);
    let Some(grey) = draw(&graded(red, basic(GradeBasic { saturation: -100.0, ..GradeBasic::default() }))) else {
        return;
    };
    let c = centre(&grey);
    assert!((c[0] - c[1]).abs() <= 1 && (c[1] - c[2]).abs() <= 1, "{c:?}");
}

#[test]
fn panel_order_is_processing_order() {
    let red_up = GradeToneCurve {
        red: vec![CurvePoint::new(0.0, 0.0, 0.0), CurvePoint::new(0.5, 0.75, 0.0), CurvePoint::new(1.0, 1.0, 0.0)],
        ..GradeToneCurve::default()
    };
    let grey_out = GradeBasic { saturation: -100.0, ..GradeBasic::default() };
    let colour = flat([0.1, 0.3, 0.2]);
    let curve_last = panels(vec![GradePanel::Basic(grey_out.clone()), GradePanel::ToneCurve(red_up.clone())]);
    let curve_first = panels(vec![GradePanel::ToneCurve(red_up), GradePanel::Basic(grey_out)]);
    let Some(a) = draw(&graded(colour.clone(), curve_last)) else { return };
    let b = draw(&graded(colour, curve_first)).unwrap();
    let (a, b) = (centre(&a), centre(&b));
    assert!(a[0] > a[1] + 20, "greyed, then reddened: {a:?}");
    assert!((b[0] - b[1]).abs() <= 1, "reddened, then greyed: {b:?}");
}

#[test]
fn the_split_grades_only_right_of_the_line() {
    let stack = GradeStack { split: Some(0.5), ..basic(GradeBasic { exposure: 2.0, ..GradeBasic::default() }) };
    let Some(split) = draw(&graded(grey(0.05), stack)) else { return };
    let plain = draw(&grey(0.05)).unwrap();
    assert_eq!(at(&split, 10, 32), at(&plain, 10, 32));
    assert!(at(&split, 50, 32)[0] > at(&plain, 50, 32)[0] + 20);
}

#[test]
fn a_mask_at_half_amount_lands_half_way_in_linear_light() {
    let mut stack = basic(GradeBasic { exposure: 1.0, ..GradeBasic::default() });
    stack.masks[0].amount = 0.5;
    let Some(half) = draw(&graded(grey(0.2), stack)) else { return };
    let want = draw(&grey(0.3)).unwrap();
    assert!(close(centre(&half), centre(&want), 1), "{:?} vs {:?}", centre(&half), centre(&want));
}

#[test]
fn the_histogram_counts_every_pixel_of_a_flat_picture_by_its_colour() {
    let Ok((device, queue)) = crate::test_device() else { return };
    let mut renderer = Renderer::new(&device, N, N);
    renderer.render(&device, &queue, &flat([0.2, 0.05, 0.6]));
    let pixel = {
        let bytes = renderer.read_output(&device, &queue).unwrap();
        centre(&bytes)
    };
    renderer.request_histogram(&device, &queue);
    let _ = device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
    let bins = renderer.histogram(&device).expect("counted");
    let all = N * N;
    // The dither spreads a flat colour over the steps either side of it (the pixel read is one of
    // them).
    for ch in 0..3 {
        let v = pixel[ch] as usize;
        assert_eq!(bins[ch][v - 2..=v + 2].iter().sum::<u32>(), all, "channel {ch}: {pixel:?}");
        assert!(bins[ch].iter().max().unwrap() > &(all / 3), "most on one or two steps");
        assert_eq!(bins[ch].iter().sum::<u32>(), all);
    }
    assert_eq!(bins[3].iter().sum::<u32>(), all);
    // Asked again, it counts again.
    renderer.request_histogram(&device, &queue);
    let _ = device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
    assert!(renderer.histogram(&device).is_some());
}

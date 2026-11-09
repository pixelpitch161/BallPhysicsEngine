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
    GradeStack { masks: vec![GradeMask { amount: 1.0, panels, ..GradeMask::default() }], split: None }
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

#[test]
fn black_through_saturation_then_a_curve_that_lifts_blacks_is_lifted() {
    let lift = GradeToneCurve {
        rgb: vec![CurvePoint::new(0.0, 0.3, 0.0), CurvePoint::new(1.0, 1.0, 0.0)],
        ..GradeToneCurve::default()
    };
    let stack = panels(vec![
        GradePanel::Basic(GradeBasic { saturation: 40.0, vibrance: 30.0, ..GradeBasic::default() }),
        GradePanel::ToneCurve(lift),
    ]);
    let Some(lifted) = draw(&graded(grey(0.0), stack)) else { return };
    let c = centre(&lifted);
    // tone 0.3 is about 77 of 255.
    assert!(c.iter().all(|&v| (70..=84).contains(&v)), "{c:?}");
}

#[test]
fn a_long_stack_grows_the_program_s_buffer() {
    // Five curves each lifting a little: more words than the buffer starts with.
    let step = || GradeToneCurve { lights: 10.0, darks: 10.0, ..GradeToneCurve::default() };
    let one = panels(vec![GradePanel::ToneCurve(step())]);
    let five = panels((0..5).map(|_| GradePanel::ToneCurve(step())).collect());
    let Some(a) = draw(&graded(grey(0.1), one)) else { return };
    let (device, queue) = crate::test_device().unwrap();
    let mut renderer = Renderer::new(&device, N, N);
    renderer.render(&device, &queue, &graded(grey(0.1), panels(vec![])));
    renderer.render(&device, &queue, &graded(grey(0.1), five));
    let b = renderer.read_output(&device, &queue).unwrap();
    assert!(centre(&b)[0] > centre(&a)[0] + 10, "{:?} vs {:?}", centre(&b), centre(&a));
    assert!(!renderer.histogram_pending());
    renderer.request_histogram(&device, &queue);
    assert!(renderer.histogram_pending(), "asked for");
    let _ = device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
    assert!(renderer.histogram(&device).is_some());
    assert!(!renderer.histogram_pending(), "taken");
}

// ---------------------------------------------------------------------------------------------
// Part 2: the colour panels.

use crate::frame::{GradeCalibration, GradeColourGrading, GradeHsl, GradePick, GradePointColour, GradeWheel};
use crate::grade::maths;

/// The colours the colour panels are checked on (linear light).
const SWATCHES: [[f32; 3]; 9] = [
    // Brighter than white (the roll-off squeezes both sides alike) and a hue just under 360°.
    [1.8, 0.4, 0.1],
    [0.5, 0.05, 0.15],
    [0.6, 0.05, 0.03],
    [0.7, 0.35, 0.02],
    [0.05, 0.4, 0.08],
    [0.03, 0.2, 0.55],
    [0.3, 0.05, 0.4],
    [0.18, 0.18, 0.18],
    [0.02, 0.015, 0.01],
];

/// A colour panel worked out on the CPU, in its working space.
fn on_cpu(panel: &GradePanel, c: [f32; 3]) -> [f32; 3] {
    let c = c.map(|v| v as f64);
    let lab = |f: &dyn Fn([f64; 3]) -> [f64; 3]| maths::unoklab(f(maths::oklab(c)));
    let out = match panel {
        GradePanel::Hsl(h) => {
            let row = maths::hsl_row(h);
            lab(&|l| maths::hsl(l, &row))
        }
        GradePanel::PointColour(p) => {
            let picks: Vec<maths::PickWords> = p.picks.iter().filter_map(maths::pick_words).collect();
            lab(&|l| maths::point_colour(l, &picks))
        }
        GradePanel::ColourGrading(g) => {
            let w = maths::wheels_words(g);
            lab(&|l| maths::colour_wheels(l, &w))
        }
        GradePanel::Calibration(k) => {
            let m = if k.primaries_identity() { c } else { maths::apply_matrix(&maths::calibration_matrix(k), c) };
            maths::dark_tint(m, maths::dark_tint_gain(k.shadows_tint))
        }
        _ => unreachable!("only the colour panels"),
    };
    out.map(|v| v.max(0.0) as f32)
}

fn colour_panels() -> Vec<GradePanel> {
    let hsl = GradeHsl {
        hue: [40.0, -30.0, 20.0, 60.0, -50.0, 70.0, -20.0, 35.0],
        saturation: [-40.0, 30.0, 50.0, -60.0, 20.0, 80.0, -30.0, 10.0],
        luminance: [20.0, -30.0, 40.0, -50.0, 25.0, -60.0, 15.0, -10.0],
    };
    let wheel = |hue, saturation, luminance| GradeWheel { hue, saturation, luminance };
    vec![
        GradePanel::Hsl(hsl),
        GradePanel::PointColour(GradePointColour {
            picks: vec![
                GradePick { colour: [0.6, 0.05, 0.03], hue: 60.0, saturation: -30.0, luminance: 40.0, range: 70.0 },
                GradePick { colour: [0.03, 0.2, 0.55], hue: -40.0, saturation: 50.0, luminance: -20.0, range: 30.0 },
            ],
        }),
        GradePanel::ColourGrading(GradeColourGrading {
            shadows: wheel(210.0, 40.0, 20.0),
            midtones: wheel(30.0, 25.0, -10.0),
            highlights: wheel(50.0, 30.0, 15.0),
            global: wheel(300.0, 10.0, -20.0),
            blending: 70.0,
            balance: -30.0,
        }),
        GradePanel::Calibration(GradeCalibration {
            shadows_tint: 40.0,
            red_hue: 50.0,
            red_saturation: -30.0,
            green_hue: -40.0,
            green_saturation: 60.0,
            blue_hue: 30.0,
            blue_saturation: 40.0,
        }),
    ]
}

#[test]
fn the_colour_panels_on_the_gpu_match_their_maths() {
    if draw(&grey(0.5)).is_none() {
        return;
    }
    for panel in colour_panels() {
        for c in SWATCHES {
            let got = centre(&draw(&graded(flat(c), panels(vec![panel.clone()]))).unwrap());
            let want = centre(&draw(&flat(on_cpu(&panel, c))).unwrap());
            assert!(close(got, want, 2), "{panel:?} on {c:?}: GPU {got:?}, maths {want:?}");
        }
    }
}

#[test]
fn the_colour_panels_at_their_defaults_draw_the_same_bytes() {
    let mut frame = two_tone([0.02, 0.01, 0.0], [0.6, 0.7, 2.0]);
    crate::pictures::plain_ball(&mut frame, glam::Vec2::new(5.0, -3.0), 12.0, [0.9, 0.3, 0.1, 1.0], 0.0);
    let Some(plain) = draw(&frame) else { return };
    let defaults = panels(vec![
        GradePanel::Hsl(GradeHsl::default()),
        GradePanel::PointColour(GradePointColour { picks: vec![GradePick::default()] }),
        GradePanel::ColourGrading(GradeColourGrading { balance: 40.0, ..GradeColourGrading::default() }),
        GradePanel::Calibration(GradeCalibration::default()),
    ]);
    assert!(defaults.is_identity());
    assert!(draw(&graded(frame, defaults)).unwrap() == plain, "a colour panel at its defaults changed the picture");
}

#[test]
fn hsl_blue_saturation_down_greys_a_blue_patch_and_leaves_a_red_one() {
    let mut hsl = GradeHsl::default();
    hsl.saturation[5] = -100.0;
    let stack = panels(vec![GradePanel::Hsl(hsl)]);
    let Some(blue) = draw(&graded(flat([0.03, 0.08, 0.6]), stack.clone())) else { return };
    let b = centre(&blue);
    assert!((b[0] - b[1]).abs() <= 2 && (b[1] - b[2]).abs() <= 2, "blue greyed: {b:?}");
    let red = centre(&draw(&graded(flat([0.6, 0.04, 0.03]), stack)).unwrap());
    let plain = centre(&draw(&flat([0.6, 0.04, 0.03])).unwrap());
    assert!(close(red, plain, 1), "red kept: {red:?} vs {plain:?}");
}

#[test]
fn calibration_then_hsl_is_not_hsl_then_calibration() {
    let mut hsl = GradeHsl::default();
    hsl.saturation[0] = -100.0;
    let calibration = GradeCalibration { red_hue: 100.0, red_saturation: 100.0, ..GradeCalibration::default() };
    let colour = flat([0.5, 0.06, 0.04]);
    let a = panels(vec![GradePanel::Hsl(hsl.clone()), GradePanel::Calibration(calibration.clone())]);
    let b = panels(vec![GradePanel::Calibration(calibration), GradePanel::Hsl(hsl)]);
    let Some(a) = draw(&graded(colour.clone(), a)) else { return };
    let b = draw(&graded(colour, b)).unwrap();
    assert!(!close(centre(&a), centre(&b), 3), "{:?} vs {:?}", centre(&a), centre(&b));
}

// ---------------------------------------------------------------------------------------------
// Part 3: area steps (passes), Detail and Effects.

use crate::frame::{GradeDetail, GradeEffects};

/// A grey picture with a grey ball of `radius` in the middle.
fn ball_on(bg: f32, ball: f32, radius: f32) -> Frame {
    let mut f = grey(bg);
    crate::pictures::plain_ball(&mut f, glam::Vec2::ZERO, radius, [ball, ball, ball, 1.0], 0.0);
    f
}

fn sharpen(amount: f32) -> GradePanel {
    GradePanel::Detail(GradeDetail { amount, radius: 1.5, detail: 100.0 })
}

fn effects(e: GradeEffects) -> GradePanel {
    GradePanel::Effects(e)
}

/// The green channel along row `y`.
fn row(p: &[u8], y: u32) -> Vec<i32> {
    (0..N).map(|x| at(p, x, y)[1]).collect()
}

fn mean(p: &[u8]) -> f64 {
    p.chunks(4).map(|c| c[1] as f64).sum::<f64>() / (N * N) as f64
}

fn all_close(a: &[u8], b: &[u8], by: i32) -> bool {
    (0..N).all(|y| (0..N).all(|x| close(at(a, x, y), at(b, x, y), by)))
}

#[test]
fn detail_effects_and_basic_presence_at_their_defaults_draw_the_same_bytes() {
    let frame = ball_on(0.1, 0.6, 12.0);
    let Some(plain) = draw(&frame) else { return };
    let defaults = panels(vec![
        GradePanel::Basic(GradeBasic { texture: 0.0, clarity: -0.0, dehaze: 0.0, ..GradeBasic::default() }),
        GradePanel::Detail(GradeDetail { radius: 3.0, ..GradeDetail::default() }),
        effects(GradeEffects { vignette_highlights: 80.0, grain_size: 90.0, ..GradeEffects::default() }),
    ]);
    assert!(defaults.is_identity());
    assert!(plain == draw(&graded(frame, defaults)).unwrap());
}

#[test]
fn a_pass_split_leaves_point_steps_as_they_were() {
    // Sharpening a flat picture changes nothing, so the grade around it must come out the same
    // in three passes as in one.
    let frame = flat([0.3, 0.2, 0.1]);
    let one = panels(vec![GradePanel::Basic(GradeBasic { exposure: 1.0, saturation: -50.0, ..GradeBasic::default() })]);
    let three = panels(vec![
        GradePanel::Basic(GradeBasic { exposure: 1.0, ..GradeBasic::default() }),
        sharpen(150.0),
        GradePanel::Basic(GradeBasic { saturation: -50.0, ..GradeBasic::default() }),
        sharpen(150.0),
    ]);
    assert_eq!(crate::grade::compile(&three).segments.len(), 3);
    let Some(a) = draw(&graded(frame.clone(), one)) else { return };
    let b = draw(&graded(frame, three)).unwrap();
    assert!(all_close(&a, &b, 1), "{:?} vs {:?}", centre(&a), centre(&b));
}

#[test]
fn a_mask_at_half_amount_works_across_a_pass_split() {
    let mut stack =
        panels(vec![GradePanel::Basic(GradeBasic { exposure: 1.0, ..GradeBasic::default() }), sharpen(100.0)]);
    stack.masks[0].amount = 0.5;
    let Some(half) = draw(&graded(grey(0.2), stack)) else { return };
    let want = draw(&grey(0.3)).unwrap();
    assert!(close(centre(&half), centre(&want), 1), "{:?} vs {:?}", centre(&half), centre(&want));
}

#[test]
fn the_split_line_holds_with_area_steps() {
    let stack = GradeStack {
        split: Some(0.5),
        ..panels(vec![sharpen(100.0), GradePanel::Basic(GradeBasic { exposure: 2.0, ..GradeBasic::default() })])
    };
    let Some(split) = draw(&graded(grey(0.05), stack)) else { return };
    let plain = draw(&grey(0.05)).unwrap();
    assert!(close(at(&split, 10, 32), at(&plain, 10, 32), 1));
    assert!(at(&split, 50, 32)[0] > at(&plain, 50, 32)[0] + 20);
}

#[test]
fn sharpening_overshoots_both_sides_of_an_edge_and_leaves_flat_parts() {
    let frame = ball_on(0.05, 0.5, 14.0);
    let Some(plain) = draw(&frame) else { return };
    let sharp = draw(&graded(frame, panels(vec![sharpen(150.0)]))).unwrap();
    let (p, s) = (row(&plain, 32), row(&sharp, 32));
    let (pmax, pmin) = (*p.iter().max().unwrap(), *p.iter().min().unwrap());
    let (smax, smin) = (*s.iter().max().unwrap(), *s.iter().min().unwrap());
    assert!(smax > pmax + 4 && smin < pmin - 4, "plain {pmin}..{pmax}, sharpened {smin}..{smax}");
    assert!(close(at(&sharp, 2, 2), at(&plain, 2, 2), 1), "the background far off is untouched");
    assert!(close(centre(&sharp), centre(&plain), 2), "the ball's flat middle is nearly untouched");
}

#[test]
fn clarity_widens_or_narrows_the_difference_between_a_ball_and_its_surround() {
    let frame = ball_on(0.1, 0.3, 14.0);
    let clarity = |c: f32| panels(vec![GradePanel::Basic(GradeBasic { clarity: c, ..GradeBasic::default() })]);
    let Some(plain) = draw(&frame) else { return };
    let up = draw(&graded(frame.clone(), clarity(100.0))).unwrap();
    let down = draw(&graded(frame, clarity(-100.0))).unwrap();
    let gap = |p: &[u8]| centre(p)[1] - at(p, 3, 3)[1];
    assert!(gap(&up) > gap(&plain) + 6, "{} vs {}", gap(&up), gap(&plain));
    assert!(gap(&down) < gap(&plain) - 6, "{} vs {}", gap(&down), gap(&plain));
}

#[test]
fn texture_brings_out_or_smooths_small_detail() {
    let frame = ball_on(0.1, 0.4, 2.5);
    let texture = |t: f32| panels(vec![GradePanel::Basic(GradeBasic { texture: t, ..GradeBasic::default() })]);
    let Some(plain) = draw(&frame) else { return };
    let up = draw(&graded(frame.clone(), texture(100.0))).unwrap();
    let down = draw(&graded(frame, texture(-100.0))).unwrap();
    assert!(centre(&up)[1] > centre(&plain)[1] + 3, "{:?} vs {:?}", centre(&up), centre(&plain));
    assert!(centre(&down)[1] < centre(&plain)[1] - 3, "{:?} vs {:?}", centre(&down), centre(&plain));
}

#[test]
fn dehaze_clears_a_hazy_grey_keeps_white_and_minus_fogs_black() {
    let dehaze = |d: f32| panels(vec![GradePanel::Basic(GradeBasic { dehaze: d, ..GradeBasic::default() })]);
    let Some(plain) = draw(&grey(0.4)) else { return };
    let cleared = draw(&graded(grey(0.4), dehaze(100.0))).unwrap();
    assert!(centre(&cleared)[1] < centre(&plain)[1] - 20, "{:?}", centre(&cleared));
    let white = draw(&graded(grey(1.0), dehaze(100.0))).unwrap();
    assert_eq!(centre(&white), [255; 3]);
    let fogged = draw(&graded(grey(0.0), dehaze(-100.0))).unwrap();
    assert!(centre(&fogged)[1] > 100, "{:?}", centre(&fogged));
}

#[test]
fn the_vignette_darkens_or_lightens_the_corners_and_highlights_keep_bright_ones() {
    let vignette = |amount: f32, highlights: f32| {
        panels(vec![effects(GradeEffects {
            vignette_amount: amount,
            vignette_highlights: highlights,
            ..GradeEffects::default()
        })])
    };
    let Some(plain) = draw(&grey(0.3)) else { return };
    let dark = draw(&graded(grey(0.3), vignette(-100.0, 0.0))).unwrap();
    assert!(close(centre(&dark), centre(&plain), 1));
    assert!(at(&dark, 0, 0)[1] < 20, "{:?}", at(&dark, 0, 0));
    let light = draw(&graded(grey(0.3), vignette(100.0, 0.0))).unwrap();
    assert!(at(&light, 0, 0)[1] > 245, "{:?}", at(&light, 0, 0));
    // A bright picture: without Highlights its corners go dark, with them they stay.
    let bright = draw(&graded(grey(2.0), vignette(-100.0, 0.0))).unwrap();
    let kept = draw(&graded(grey(2.0), vignette(-100.0, 100.0))).unwrap();
    assert!(at(&bright, 0, 0)[1] < 100 && at(&kept, 0, 0)[1] == 255, "{:?} {:?}", at(&bright, 0, 0), at(&kept, 0, 0));
}

#[test]
fn grain_is_the_same_for_the_same_frame_and_new_for_another() {
    let grain = panels(vec![effects(GradeEffects { grain_amount: 60.0, ..GradeEffects::default() })]);
    let frame = graded(grey(0.2), grain);
    let Some(a) = draw(&frame) else { return };
    let b = draw(&frame).unwrap();
    assert!(a == b, "the same frame drew different grain");
    let later = draw(&Frame { time: 1.0 / 60.0, ..frame.clone() }).unwrap();
    assert!(a != later, "the next frame has the same grain");
    let plain = draw(&grey(0.2)).unwrap();
    let moved = (0..N * N).filter(|i| a[(*i * 4) as usize] != plain[(*i * 4) as usize]).count();
    assert!(moved > (N * N / 2) as usize, "only {moved} pixels have grain");
    assert!((mean(&a) - mean(&plain)).abs() < 2.0, "{} vs {}", mean(&a), mean(&plain));
}

#[test]
fn the_gradient_map_recolours_a_grey_by_its_brightness() {
    let map = GradeEffects {
        map: bpe_core::Gradient::even(&[bpe_core::Color::rgb(0, 0, 0), bpe_core::Color::rgb(255, 0, 0)]),
        map_amount: 100.0,
        ..GradeEffects::default()
    };
    let Some(red) = draw(&graded(grey(0.18), panels(vec![effects(map)]))) else { return };
    let c = centre(&red);
    assert!(c[0] > c[1] + 60 && c[1] <= 2 && c[2] <= 2, "{c:?}");
}

#[test]
fn two_area_passes_each_build_their_pyramid_from_their_own_input() {
    // Clarity does nothing to a flat picture if its pyramid is of the picture it grades: so on a
    // flat grey, Clarity, Exposure −2, Clarity must be Exposure −2 alone (a pyramid left over from
    // the brighter first pass would darken it further).
    let clarity = GradePanel::Basic(GradeBasic { clarity: 100.0, ..GradeBasic::default() });
    let darker = GradePanel::Basic(GradeBasic { exposure: -2.0, ..GradeBasic::default() });
    let twice = panels(vec![clarity.clone(), darker.clone(), clarity]);
    let p = crate::grade::compile(&twice);
    assert_eq!(p.segments.iter().filter(|s| s.pyramid).count(), 2);
    let Some(got) = draw(&graded(grey(0.3), twice)) else { return };
    let want = draw(&graded(grey(0.3), panels(vec![darker]))).unwrap();
    assert!(all_close(&got, &want, 1), "{:?} vs {:?}", centre(&got), centre(&want));
}

// ---------------------------------------------------------------------------------------------
// Part 4: masks.

use crate::frame::{
    GradeArea, GradeCombine, GradeLinear, GradeLuminanceRange, GradePart, GradePlace, GradeRadial, GradeShape,
};

fn radial(x: f32, y: f32, size: f32, feather: f32) -> GradeShape {
    GradeShape::Radial(GradeRadial {
        x,
        y,
        width: size,
        height: size,
        angle: 0.0,
        roundness: 0.0,
        feather,
        places: vec![GradePlace::SCREEN],
    })
}

fn add(shape: GradeShape) -> GradePart {
    GradePart { combine: GradeCombine::Add, invert: false, shape, id: 0 }
}

fn exposed() -> Vec<GradePanel> {
    vec![GradePanel::Basic(GradeBasic { exposure: 1.0, ..GradeBasic::default() })]
}

/// A mask of `parts` raising the exposure a stop at `amount`.
fn mask_of(parts: Vec<GradePart>, amount: f32) -> GradeStack {
    GradeStack {
        masks: vec![GradeMask {
            amount,
            area: Some(GradeArea { parts, invert: false }),
            panels: exposed(),
            ..GradeMask::default()
        }],
        split: None,
    }
}

/// Pixel `(x, y)`'s middle in screen units (the 64 × 64 video's middle at 0).
fn screen(x: u32, y: u32) -> [f64; 2] {
    [x as f64 + 0.5 - N as f64 / 2.0, y as f64 + 0.5 - N as f64 / 2.0]
}

/// The byte a grey of linear `v` shows as.
fn byte(v: f64) -> i32 {
    (crate::grade::maths::tone(crate::grade::maths::roll_off([v; 3])[0]) * 255.0).round() as i32
}

#[test]
fn a_radial_mask_grades_its_inside_as_the_maths_says() {
    let v = 0.18;
    let stack = mask_of(vec![add(radial(4.0, -2.0, 30.0, 60.0))], 1.0);
    let Some(p) = draw(&graded(grey(v as f32), stack.clone())) else { return };
    let area = stack.masks[0].area.clone().unwrap();
    for y in [30, 31] {
        for x in 0..N {
            let k = crate::grade::maths::area_cover(&area, [v; 3], screen(x, y), 1.0);
            let want = byte(v * (1.0 + k));
            assert!((at(&p, x, y)[1] - want).abs() <= 2, "({x}, {y}): {} vs {want} (coverage {k})", at(&p, x, y)[1]);
        }
    }
    assert!(close(at(&p, 1, 1), [byte(v); 3], 1), "outside untouched");
}

#[test]
fn a_linear_mask_is_half_way_on_its_middle_line() {
    let v = 0.18;
    let linear = GradeShape::Linear(GradeLinear {
        x: 0.0,
        y: 0.0,
        angle: 90.0,
        feather: 32.0,
        places: vec![GradePlace::SCREEN],
    });
    let Some(p) = draw(&graded(grey(v as f32), mask_of(vec![add(linear)], 1.0))) else { return };
    assert!(close(at(&p, 10, 2), [byte(2.0 * v); 3], 2), "full above: {:?}", at(&p, 10, 2));
    assert!(close(at(&p, 10, 61), [byte(v); 3], 2), "none below: {:?}", at(&p, 10, 61));
    let middle = at(&p, 10, 32)[1];
    assert!((middle - byte(1.5 * v)).abs() <= 3, "half-way: {middle}");
}

#[test]
fn add_subtract_and_intersect_cover_where_they_should_on_the_gpu() {
    let v = 0.18;
    let (lit, dark) = ([byte(2.0 * v); 3], [byte(v); 3]);
    let left = radial(-12.0, 0.0, 24.0, 0.0);
    let right = radial(12.0, 0.0, 24.0, 0.0);
    let middle = radial(0.0, 0.0, 40.0, 0.0);
    let with = |b: GradeShape, how| vec![add(middle.clone()), GradePart { combine: how, ..add(b) }];
    let Some(sum) = draw(&graded(grey(v as f32), mask_of(vec![add(left.clone()), add(right.clone())], 1.0))) else {
        return;
    };
    assert!(close(at(&sum, 20, 32), lit, 2) && close(at(&sum, 44, 32), lit, 2) && close(at(&sum, 32, 10), dark, 2));
    let sub = draw(&graded(grey(v as f32), mask_of(with(right.clone(), GradeCombine::Subtract), 1.0))).unwrap();
    assert!(close(at(&sum, 20, 32), lit, 2) && close(at(&sub, 44, 32), dark, 2), "{:?}", at(&sub, 44, 32));
    let both = draw(&graded(grey(v as f32), mask_of(with(right, GradeCombine::Intersect), 1.0))).unwrap();
    assert!(close(at(&both, 20, 32), dark, 2) && close(at(&both, 44, 32), lit, 2), "{:?}", at(&both, 20, 32));
}

#[test]
fn a_mask_at_amount_0_changes_nothing_and_its_overlay_shows_only_red() {
    let frame = ball_on(0.1, 0.6, 10.0);
    let Some(plain) = draw(&frame) else { return };
    let mut stack = mask_of(vec![add(radial(0.0, 0.0, 30.0, 0.0))], 0.0);
    assert!(stack.is_identity());
    assert!(plain == draw(&graded(frame.clone(), stack.clone())).unwrap());
    stack.masks[0].overlay = true;
    let shown = draw(&graded(frame, stack)).unwrap();
    let c = centre(&shown);
    assert!(c[0] > c[1] + 40 && c[0] > c[2] + 40, "red over the mask: {c:?}");
    assert!(close(at(&shown, 1, 1), at(&plain, 1, 1), 1), "untouched outside");
}

#[test]
fn a_luminance_range_brightens_only_the_bright_part_of_a_ramp() {
    let ramp = two_tone([0.01; 3], [0.8; 3]);
    let Some(plain) = draw(&ramp) else { return };
    let range = GradeShape::LuminanceRange(GradeLuminanceRange { low: 70.0, high: 100.0, smoothness: 10.0 });
    let p = draw(&graded(ramp, mask_of(vec![add(range)], 1.0))).unwrap();
    assert!(close(at(&p, 10, 2), at(&plain, 10, 2), 1), "dark top kept");
    assert!(at(&p, 10, 61)[1] > at(&plain, 10, 61)[1] + 20, "bright bottom raised");
}

#[test]
fn a_colour_range_takes_the_picked_colour_and_not_another() {
    let red = [0.6, 0.04, 0.03];
    let range = GradeShape::ColourRange(crate::frame::GradeColourRange { picks: vec![red], refine: 30.0 });
    let stack = GradeStack {
        masks: vec![GradeMask {
            amount: 1.0,
            area: Some(GradeArea { parts: vec![add(range)], invert: false }),
            panels: vec![GradePanel::Basic(GradeBasic { saturation: -100.0, ..GradeBasic::default() })],
            ..GradeMask::default()
        }],
        split: None,
    };
    let Some(r) = draw(&graded(flat(red), stack.clone())) else { return };
    let r = centre(&r);
    assert!((r[0] - r[1]).abs() <= 2, "red greyed: {r:?}");
    let blue = flat([0.03, 0.08, 0.6]);
    let b = draw(&graded(blue.clone(), stack)).unwrap();
    assert!(close(centre(&b), centre(&draw(&blue).unwrap()), 1), "blue kept");
}

#[test]
fn every_ball_copies_grade_round_each_place_and_not_between() {
    let v = 0.18;
    let places = [[-16.0, -16.0], [16.0, 0.0], [-8.0, 20.0]].map(|at| GradePlace { at, ..GradePlace::SCREEN });
    let shape = GradeShape::Radial(GradeRadial {
        x: 0.0,
        y: 0.0,
        width: 10.0,
        height: 10.0,
        angle: 0.0,
        roundness: 0.0,
        feather: 0.0,
        places: places.to_vec(),
    });
    let Some(p) = draw(&graded(grey(v as f32), mask_of(vec![add(shape)], 1.0))) else { return };
    for [x, y] in [[16, 16], [48, 32], [24, 52]] {
        assert!(close(at(&p, x, y), [byte(2.0 * v); 3], 2), "({x}, {y}) {:?}", at(&p, x, y));
    }
    assert!(close(at(&p, 32, 32), [byte(v); 3], 1));
}

#[test]
fn turned_and_scaled_copies_are_found_by_their_reach_on_the_gpu() {
    // A wide flat ellipse, turned a quarter and doubled at one place: a tall one 12 wide and 40 high.
    let v = 0.18;
    let tall = GradePlace { at: [12.0, 0.0], turn: std::f32::consts::FRAC_PI_2, scale: 2.0, world: false };
    let small = GradePlace { at: [-12.0, -20.0], ..GradePlace::SCREEN };
    let shape = GradeShape::Radial(GradeRadial {
        x: 0.0,
        y: 0.0,
        width: 40.0,
        height: 6.0,
        angle: 0.0,
        roundness: 0.0,
        feather: 0.0,
        places: vec![tall, small],
    });
    let area = GradeArea { parts: vec![add(shape.clone())], invert: false };
    let Some(p) = draw(&graded(grey(v as f32), mask_of(vec![add(shape)], 1.0))) else { return };
    let (lit, dark) = ([byte(2.0 * v); 3], [byte(v); 3]);
    assert!(close(at(&p, 44, 48), lit, 2), "far down the tall one: {:?}", at(&p, 44, 48));
    assert!(close(at(&p, 52, 32), dark, 2), "beside it: {:?}", at(&p, 52, 32));
    assert!(close(at(&p, 20, 12), lit, 2), "the small one: {:?}", at(&p, 20, 12));
    // The CPU twin agrees.
    let cpu = |x: f64, y: f64| crate::grade::maths::area_cover(&area, [v; 3], [x - 32.0, y - 32.0], 1.0);
    assert!(cpu(44.5, 48.5) > 0.99 && cpu(52.5, 32.5) < 0.01);
}

#[test]
fn an_inverted_part_covers_outside_on_the_gpu() {
    let v = 0.18;
    let part = GradePart { invert: true, ..add(radial(0.0, 0.0, 30.0, 0.0)) };
    let Some(p) = draw(&graded(grey(v as f32), mask_of(vec![part], 1.0))) else { return };
    assert!(close(centre(&p), [byte(v); 3], 2), "inside kept: {:?}", centre(&p));
    assert!(close(at(&p, 2, 2), [byte(2.0 * v); 3], 2), "outside graded: {:?}", at(&p, 2, 2));
}

#[test]
fn a_mask_with_an_area_step_inside_still_covers_only_its_part() {
    let frame = grey(0.18);
    let Some(plain) = draw(&frame) else { return };
    let mut stack = mask_of(vec![add(radial(0.0, 0.0, 30.0, 0.0))], 1.0);
    stack.masks[0].panels.push(GradePanel::Basic(GradeBasic { dehaze: 60.0, ..GradeBasic::default() }));
    stack.masks[0].panels.push(GradePanel::Basic(GradeBasic { exposure: 0.5, ..GradeBasic::default() }));
    assert!(crate::grade::compile(&stack).segments.len() == 2, "split by Dehaze");
    let p = draw(&graded(frame, stack)).unwrap();
    assert!(close(at(&p, 1, 1), at(&plain, 1, 1), 1), "outside kept across the split");
    assert!(centre(&p)[1] > centre(&plain)[1] + 20, "inside graded: {:?}", centre(&p));
}

#[test]
fn the_overlay_rides_through_a_pass_split() {
    let mut stack = mask_of(vec![add(radial(0.0, 0.0, 30.0, 0.0))], 0.0);
    stack.masks[0].overlay = true;
    // A Whole picture after the mask with an area step splits the program after the mask's end.
    stack.masks.push(GradeMask {
        panels: vec![GradePanel::Basic(GradeBasic { dehaze: 40.0, ..GradeBasic::default() })],
        ..GradeMask::default()
    });
    assert!(crate::grade::compile(&stack).segments.len() == 2);
    let Some(p) = draw(&graded(grey(0.18), stack)) else { return };
    let c = centre(&p);
    assert!(c[0] > c[1] + 40, "the overlay survives the split: {c:?}");
}

// ---------------------------------------------------------------------------------------------
// Part 5: Things masks and the Brush.

use crate::frame::{GradeBrush, GradeThings};

/// A Things part covering `things` (`None`: every thing), grown and faded by `expand` and
/// `feather`.
fn things(things: Option<Vec<u32>>, background: bool, expand: f32, feather: f32) -> GradeShape {
    GradeShape::Things(GradeThings { things, background, expand, feather })
}

/// A grey `v` picture with grey `v` balls (code, x, y, radius), drawn in order.
fn balls_on(v: f64, balls: &[(f32, f32, f32, f32)]) -> Frame {
    let v = v as f32;
    let mut f = grey(v);
    for &(code, x, y, r) in balls {
        crate::pictures::plain_ball(&mut f, glam::Vec2::new(x, y), r, [v, v, v, 1.0], code);
    }
    f
}

#[test]
fn a_things_mask_grades_the_chosen_ball_and_not_another_or_the_background() {
    let v = 0.18;
    let frame = balls_on(v, &[(1.0, -14.0, 0.0, 8.0), (2.0, 14.0, 0.0, 8.0)]);
    let Some(p) = draw(&graded(frame, mask_of(vec![add(things(Some(vec![1]), false, 0.0, 0.0))], 1.0))) else {
        return;
    };
    let (lit, dark) = ([byte(2.0 * v); 3], [byte(v); 3]);
    assert!(close(at(&p, 18, 32), lit, 2), "the chosen ball: {:?}", at(&p, 18, 32));
    assert!(close(at(&p, 46, 32), dark, 2), "the other ball: {:?}", at(&p, 46, 32));
    assert!(close(at(&p, 32, 32), dark, 2) && close(at(&p, 18, 12), dark, 2), "the background");
}

#[test]
fn a_thing_in_front_hides_the_chosen_ball_under_it() {
    let v = 0.18;
    // Ball 2 is drawn after ball 1, over its right side.
    let frame = balls_on(v, &[(1.0, 0.0, 0.0, 12.0), (2.0, 8.0, 0.0, 8.0)]);
    let Some(p) = draw(&graded(frame, mask_of(vec![add(things(Some(vec![1]), false, 0.0, 0.0))], 1.0))) else {
        return;
    };
    assert!(close(at(&p, 25, 32), [byte(2.0 * v); 3], 2), "what shows of it: {:?}", at(&p, 25, 32));
    assert!(close(at(&p, 40, 32), [byte(v); 3], 2), "the ball in front: {:?}", at(&p, 40, 32));
}

#[test]
fn expand_and_feather_reach_past_the_ball_as_the_maths_says() {
    let v = 0.18;
    let frame = balls_on(v, &[(1.0, 0.0, 0.0, 8.0)]);
    let (expand, feather) = (4.0, 8.0);
    let Some(p) = draw(&graded(frame, mask_of(vec![add(things(Some(vec![1]), false, expand, feather))], 1.0))) else {
        return;
    };
    // Along the row through its middle: the ball's edge is 8 px right of the middle.
    for x in 41..N {
        let centre_x = x as f64 + 0.5 - 32.0;
        // How far the pixel is from the nearest pixel at least half covered (the edge's pixel).
        let d = (centre_x - 7.5).max(0.0);
        let cover = maths::things_cover(0.0, d, expand as f64, feather as f64, 1.0, false);
        let want = byte(v * (1.0 + cover));
        let got = at(&p, x, 32)[1];
        assert!((got - want).abs() <= 3, "x {x}: {got} wants {want}");
    }
    assert!(close(at(&p, 42, 32), [byte(2.0 * v); 3], 2), "within Expand: full");
    assert!(close(at(&p, 60, 32), [byte(v); 3], 1), "past Expand and Feather: none");
}

#[test]
fn background_covers_where_no_thing_is_and_inverted_the_things() {
    let v = 0.18;
    let frame = balls_on(v, &[(1.0, -14.0, 0.0, 8.0), (2.0, 14.0, 0.0, 8.0)]);
    let (lit, dark) = ([byte(2.0 * v); 3], [byte(v); 3]);
    let Some(p) = draw(&graded(frame.clone(), mask_of(vec![add(things(None, true, 0.0, 0.0))], 1.0))) else {
        return;
    };
    assert!(close(at(&p, 18, 32), dark, 2) && close(at(&p, 46, 32), dark, 2), "no ball");
    assert!(close(at(&p, 32, 8), lit, 2), "the background");
    let inverted = GradePart { invert: true, ..add(things(None, true, 0.0, 0.0)) };
    let p = draw(&graded(frame, mask_of(vec![inverted], 1.0))).unwrap();
    assert!(close(at(&p, 18, 32), lit, 2) && close(at(&p, 46, 32), lit, 2) && close(at(&p, 32, 8), dark, 2));
}

#[test]
fn a_particles_things_own_sprites_are_in_its_silhouette() {
    let v = 0.18;
    let c = [v as f32, v as f32, v as f32, 1.0];
    let mut frame = grey(v as f32);
    let mut dot = crate::Particle::new(6, glam::Vec2::new(-12.0, 0.0), 0.0, 6.0, c);
    dot.shape[2] = 7;
    // The same sprite without a code (a glow or shadow) isn't.
    let other = crate::Particle::new(6, glam::Vec2::new(12.0, 0.0), 0.0, 6.0, c);
    frame.particles = vec![dot, other];
    let Some(p) = draw(&graded(frame, mask_of(vec![add(things(Some(vec![7]), false, 0.0, 0.0))], 1.0))) else {
        return;
    };
    assert!(close(at(&p, 20, 32), [byte(2.0 * v); 3], 3), "its sprite: {:?}", at(&p, 20, 32));
    assert!(close(at(&p, 44, 32), [byte(v); 3], 1), "no thing's sprite: {:?}", at(&p, 44, 32));
}

#[test]
fn a_grade_without_things_parts_draws_no_silhouettes_and_with_one_it_does() {
    let Ok((device, queue)) = crate::test_device() else { return };
    let mut renderer = Renderer::new(&device, N, N);
    let frame = balls_on(0.18, &[(1.0, 0.0, 0.0, 8.0)]);
    renderer.render(&device, &queue, &graded(frame.clone(), mask_of(vec![add(radial(0.0, 0.0, 30.0, 0.0))], 1.0)));
    assert!(renderer.silhouettes.is_none());
    // A Things part naming no thing covers nothing and draws none either.
    renderer.render(
        &device,
        &queue,
        &graded(frame.clone(), mask_of(vec![add(things(Some(vec![]), false, 0.0, 0.0))], 1.0)),
    );
    assert!(renderer.silhouettes.is_none());
    renderer.render(&device, &queue, &graded(frame, mask_of(vec![add(things(Some(vec![1]), false, 0.0, 0.0))], 1.0)));
    assert!(renderer.silhouettes.is_some());
}

/// A brush part of `strokes` at `places`.
fn brush(strokes: Vec<bpe_core::Stroke>, places: Vec<GradePlace>) -> GradeShape {
    GradeShape::Brush(GradeBrush { strokes: bpe_core::Strokes { list: strokes }, places })
}

fn stroke(points: Vec<[f32; 2]>, size: f32, softness: f32) -> bpe_core::Stroke {
    bpe_core::Stroke { points, size, softness, strength: 1.0, ..Default::default() }
}

#[test]
fn a_brush_stroke_on_the_gpu_matches_its_maths() {
    let v = 0.18;
    let shape = brush(vec![stroke(vec![[-16.0, 0.0], [16.0, 4.0]], 8.0, 0.6)], vec![GradePlace::SCREEN]);
    let area = GradeArea { parts: vec![add(shape.clone())], invert: false };
    let Some(p) = draw(&graded(grey(v as f32), mask_of(vec![add(shape)], 1.0))) else { return };
    for y in [26, 30, 32, 34, 38] {
        for x in 0..N {
            let cover = maths::area_cover(&area, [v; 3], [x as f64 + 0.5 - 32.0, y as f64 + 0.5 - 32.0], 1.0);
            let want = byte(v * (1.0 + cover));
            let got = at(&p, x, y)[1];
            assert!((got - want).abs() <= 2, "({x}, {y}): {got} wants {want}");
        }
    }
    assert!(close(at(&p, 32, 34), [byte(2.0 * v); 3], 2), "its middle is full");
    assert!(close(at(&p, 32, 10), [byte(v); 3], 1), "away from it nothing");
}

#[test]
fn erasing_and_density_change_what_a_brush_covers() {
    let v = 0.18;
    let paint = stroke(vec![[-20.0, 0.0], [20.0, 0.0]], 10.0, 0.0);
    let erase = bpe_core::Stroke { erase: true, ..stroke(vec![[0.0, -12.0], [0.0, 12.0]], 4.0, 0.0) };
    let shape = brush(vec![paint.clone(), erase], vec![GradePlace::SCREEN]);
    let Some(p) = draw(&graded(grey(v as f32), mask_of(vec![add(shape)], 1.0))) else { return };
    assert!(close(at(&p, 20, 32), [byte(2.0 * v); 3], 2), "painted");
    assert!(close(at(&p, 32, 32), [byte(v); 3], 2), "erased: {:?}", at(&p, 32, 32));
    let half = brush(vec![bpe_core::Stroke { density: 0.5, ..paint }], vec![GradePlace::SCREEN]);
    let p = draw(&graded(grey(v as f32), mask_of(vec![add(half)], 1.0))).unwrap();
    assert!(close(at(&p, 20, 32), [byte(1.5 * v); 3], 2), "half density: {:?}", at(&p, 20, 32));
}

#[test]
fn a_brush_on_every_ball_paints_round_each() {
    let v = 0.18;
    let dot = stroke(vec![[0.0, 0.0]], 5.0, 0.0);
    let places = [[-16.0, -16.0], [16.0, 8.0]].map(|at| GradePlace { at, ..GradePlace::SCREEN });
    let shape = brush(vec![dot], places.to_vec());
    let Some(p) = draw(&graded(grey(v as f32), mask_of(vec![add(shape)], 1.0))) else { return };
    assert!(close(at(&p, 16, 16), [byte(2.0 * v); 3], 2) && close(at(&p, 48, 40), [byte(2.0 * v); 3], 2));
    assert!(close(at(&p, 32, 32), [byte(v); 3], 1));
}

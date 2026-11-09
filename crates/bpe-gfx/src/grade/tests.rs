//! The grade's maths and its program, on the CPU.

use super::maths::*;
use super::*;
use crate::frame::{GradeBasic, GradeMask, GradePanel, GradeStack, GradeToneCurve, straight_curve};
use bpe_core::CurvePoint;

/// Tone values from below black to far above white.
fn tones() -> impl Iterator<Item = f64> {
    (-50..4000).map(|i| i as f64 * 0.0025)
}

fn rises(f: impl Fn(f64) -> f64, what: &str) {
    let mut last = f(-0.13);
    for p in tones() {
        let y = f(p);
        assert!(y.is_finite(), "{what} at {p}: {y}");
        assert!(y >= last - 1e-12, "{what} falls at {p}: {last} then {y}");
        last = y;
    }
}

#[test]
fn the_tone_space_is_srgb_carried_on_past_white() {
    for x in [0.0, 0.001, 0.18, 0.5, 1.0, 3.0, 64.0, 65504.0] {
        assert!((untone(tone(x)) - x).abs() <= 1e-9 * x.max(1.0), "{x}");
        assert!((untone(tone(-x)) + x).abs() <= 1e-9 * x.max(1.0), "{x} mirrored");
    }
    assert!((tone(1.0) - 1.0).abs() < 1e-12);
    // The slope on both sides of white is the same (the formula carries on).
    let slope = |a: f64, b: f64| (tone(b) - tone(a)) / (b - a);
    assert!((slope(0.999, 1.0) - slope(1.0, 1.001)).abs() < 1e-3);
    assert!(tone(65504.0).is_finite() && tone(65504.0) > 100.0);
}

#[test]
fn every_tone_slider_rises_at_its_ends_and_does_nothing_at_0() {
    type Slider = fn(f64, f64) -> f64;
    let sliders: [(&str, Slider); 5] =
        [("contrast", contrast), ("highlights", highlights), ("shadows", shadows), ("whites", whites), ("blacks", blacks)];
    for (name, f) in sliders {
        for amount in [-100.0, -50.0, 50.0, 100.0] {
            rises(|p| f(p, amount), &format!("{name} {amount}"));
        }
        for p in tones() {
            assert_eq!(f(p, 0.0), p, "{name} at 0 is the straight line");
        }
    }
    let every = GradeBasic {
        contrast: 100.0,
        highlights: -100.0,
        shadows: 100.0,
        whites: -100.0,
        blacks: -100.0,
        ..GradeBasic::default()
    };
    rises(|p| basic_tone(p, &every), "every slider at once");
}

#[test]
fn tone_sliders_go_the_way_lightroom_s_do() {
    let at = |f: fn(f64, f64) -> f64, p: f64, amount: f64| f(p, amount);
    // Highlights −100 brings a linear 3.0 (a hot glow) under white; it leaves the darks alone.
    assert!(at(highlights, tone(3.0), -100.0) < 1.0);
    assert_eq!(at(highlights, 0.3, -100.0), 0.3);
    assert!(at(highlights, 0.8, 100.0) > 0.8);
    // Shadows lifts or darkens the dark part, not the light part.
    assert!(at(shadows, 0.2, 100.0) > 0.3 && at(shadows, 0.2, -100.0) < 0.15);
    assert_eq!(at(shadows, 0.8, 100.0), 0.8);
    // Whites moves white and what is above it.
    assert!((0.7..0.85).contains(&at(whites, 1.0, -100.0)));
    assert!(at(whites, 2.0, -100.0) < 2.0 && at(whites, 1.0, 100.0) > 1.15);
    // Blacks lifts black or crushes it below black.
    assert!(at(blacks, 0.0, 100.0) > 0.05 && at(blacks, 0.0, -100.0) < 0.0);
    // Contrast keeps black, middle grey and white where they are and steepens the middle.
    for c in [-100.0, 100.0] {
        assert!(at(contrast, 0.0, c).abs() < 1e-12 && (at(contrast, grey(), c) - grey()).abs() < 1e-12);
        assert!((at(contrast, 1.0, c) - 1.0).abs() < 1e-12);
    }
    assert!(at(contrast, 0.3, 100.0) < 0.3 && at(contrast, 0.7, 100.0) > 0.7);
}

#[test]
fn a_monotone_cubic_goes_through_its_points_and_never_overshoots() {
    let points = [(0.0, 0.0), (0.25, 0.1), (0.5, 0.5), (0.75, 0.95), (1.0, 1.0)];
    let curve = Pchip::new(&points);
    for (x, y) in points {
        assert!((curve.at(x) - y).abs() < 1e-12, "through ({x}, {y})");
    }
    rises(|p| curve.at(p), "a rising curve");
    for i in 0..=1000 {
        let y = curve.at(i as f64 / 1000.0);
        assert!((0.0..=1.0).contains(&y), "no overshoot: {y}");
    }
    // The straight line, however many points sit on it.
    let line = Pchip::new(&[(0.0, 0.0), (0.3, 0.3), (0.31, 0.31), (1.0, 1.0)]);
    for p in tones() {
        assert!((line.at(p) - p).abs() < 1e-12, "{p}");
    }
}

#[test]
fn the_parametric_curve_is_straight_at_0_and_keeps_its_splits_in_order() {
    let t = GradeToneCurve::default();
    let curve = parametric(&t);
    for p in tones() {
        assert!((curve.at(p) - p).abs() < 1e-12, "{p}");
    }
    assert_eq!(splits([75.0, 25.0, 50.0]), [0.25, 0.5, 0.75]);
    let s = splits([50.0, 50.0, 50.0]);
    assert!(s[0] < s[1] && s[1] < s[2] && s[2] - s[0] >= 0.039, "{s:?}");
    for amounts in [[100.0; 4], [-100.0; 4], [100.0, -100.0, 100.0, -100.0]] {
        let t = GradeToneCurve {
            highlights: amounts[0],
            lights: amounts[1],
            darks: amounts[2],
            shadows: amounts[3],
            splits: [10.0, 12.0, 90.0],
            ..GradeToneCurve::default()
        };
        rises(|p| parametric(&t).at(p), &format!("parametric {amounts:?}"));
    }
    let lifted = GradeToneCurve { shadows: 100.0, ..GradeToneCurve::default() };
    assert!(parametric(&lifted).at(0.12) > 0.15);
    assert!((parametric(&lifted).at(0.9) - 0.9).abs() < 0.01, "the highlights barely move");
}

#[test]
fn point_curves_carry_on_past_white_with_their_end_slope() {
    let pts = |list: &[(f32, f32)]| list.iter().map(|&(x, y)| CurvePoint::new(x, y, 0.0)).collect::<Vec<_>>();
    let up = point_curve(&pts(&[(0.0, 0.0), (0.5, 0.6), (1.0, 1.0)]));
    assert!(up.at(2.0) > 1.0, "nothing above white is clipped");
    // A curve that isn't usable is the straight line.
    assert_eq!(point_curve(&pts(&[(0.2, 0.0), (1.0, 1.0)])).at(0.5), 0.5);
    let t = GradeToneCurve { red: pts(&[(0.0, 0.0), (0.5, 0.7), (1.0, 1.0)]), ..GradeToneCurve::default() };
    let [r, g, b] = ToneCurves::new(&t).at(0.5);
    assert!(r > 0.6 && (g - 0.5).abs() < 1e-12 && (b - 0.5).abs() < 1e-12);
}

/// Linear luminance.
fn luma(c: [f64; 3]) -> f64 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

#[test]
fn white_balance_keeps_a_grey_s_luminance_and_warms_or_cools_it() {
    let grey = [0.2; 3];
    let of = |temp: f32, tint: f32| apply_matrix(&basic_matrix(&GradeBasic { temp, tint, ..GradeBasic::default() }), grey);
    let none = of(0.0, 0.0);
    assert_eq!(none, grey);
    for (temp, tint) in [(100.0, 0.0), (-100.0, 0.0), (0.0, 100.0), (0.0, -100.0), (40.0, -30.0)] {
        let c = of(temp, tint);
        assert!((luma(c) - 0.2).abs() < 1e-4, "{temp} {tint}: {c:?}");
    }
    let warm = of(100.0, 0.0);
    assert!(warm[0] > warm[1] && warm[1] > warm[2], "warmer is redder and less blue: {warm:?}");
    let cool = of(-100.0, 0.0);
    assert!(cool[2] > cool[1] && cool[1] > cool[0], "cooler is bluer: {cool:?}");
    let magenta = of(0.0, 100.0);
    assert!(magenta[1] < magenta[0] && magenta[1] < magenta[2], "tint + is magenta: {magenta:?}");
    let green = of(0.0, -100.0);
    assert!(green[1] > green[0] && green[1] > green[2], "tint − is green: {green:?}");
    // Exposure doubles light per stop, in the same matrix.
    let bright = apply_matrix(&basic_matrix(&GradeBasic { exposure: 1.0, ..GradeBasic::default() }), grey);
    assert!(bright.iter().all(|v| (v - 0.4).abs() < 1e-12));
}

#[test]
fn vibrance_moves_dull_colours_more_than_bright_ones_and_saturation_all_alike() {
    let dull = oklab([0.25, 0.2, 0.18]);
    let vivid = oklab([0.8, 0.02, 0.02]);
    assert!(chroma_scale(dull, 1.0, 0.0) > chroma_scale(vivid, 1.0, 0.0));
    assert!((chroma_scale(dull, 0.0, -1.0)).abs() < 1e-12, "Saturation −100 is grey");
    assert!((chroma_scale(vivid, 0.0, 1.0) - 2.0).abs() < 1e-12);
    for c in [[0.2, 0.4, 0.6], [1.0, 0.0, 0.0], [3.0, 2.0, 0.5]] {
        let back = unoklab(oklab(c));
        assert!((0..3).all(|i| (back[i] - c[i]).abs() < 1e-6), "{c:?} {back:?}");
    }
}

fn basic(b: GradeBasic) -> GradeStack {
    GradeStack { masks: vec![GradeMask { amount: 1.0, panels: vec![GradePanel::Basic(b)] }], split: None }
}

#[test]
fn a_panel_at_its_defaults_compiles_to_nothing() {
    let stack = GradeStack {
        masks: vec![GradeMask {
            amount: 1.0,
            panels: vec![
                GradePanel::Basic(GradeBasic { exposure: -0.0, ..GradeBasic::default() }),
                GradePanel::ToneCurve(GradeToneCurve {
                    rgb: vec![CurvePoint::new(0.0, 0.0, 0.0), CurvePoint::new(0.4, 0.4, 0.0), CurvePoint::new(1.0, 1.0, 0.0)],
                    ..GradeToneCurve::default()
                }),
            ],
        }],
        split: None,
    };
    assert!(stack.is_identity());
    assert!(compile(&stack).is_empty());
    assert!(GradeStack::default().is_identity());
    let weak = GradeStack { masks: vec![GradeMask { amount: 0.0, ..basic(GradeBasic { exposure: 1.0, ..GradeBasic::default() }).masks[0].clone() }], split: None };
    assert!(weak.is_identity() && compile(&weak).is_empty(), "a mask at amount 0 does nothing");
    assert_eq!(straight_curve().len(), 2);
}

#[test]
fn a_basic_panel_compiles_to_one_step_per_space_it_works_in() {
    let p = compile(&basic(GradeBasic { temp: 20.0, contrast: 30.0, vibrance: 10.0, ..GradeBasic::default() }));
    assert_eq!(p.steps, 3);
    let ops: Vec<(u32, u32)> = {
        let mut at = 1;
        let mut out = Vec::new();
        while at < p.words.len() {
            let h = p.words[at];
            out.push((h[0] as u32, h[1] as u32));
            at += h[2] as usize;
        }
        out
    };
    assert_eq!(ops, [(OP_MATRIX, SPACE_LINEAR), (OP_CURVE, SPACE_TONE), (OP_CHROMA, SPACE_OKLAB)]);
    assert_eq!(p.words[0][0], 3.0);
    // A curve step: its header, the slopes at 0 and its samples.
    assert_eq!(p.words.len(), 1 + 4 + (2 + CURVE_SAMPLES) + 2);
    // The split rides in the header.
    let split = GradeStack { split: Some(0.4), ..basic(GradeBasic { exposure: 1.0, ..GradeBasic::default() }) };
    assert_eq!(compile(&split).words[0], [1.0, 0.4, 0.0, 0.0]);
}

/// The compiled curve read back as the shader reads it.
fn read_curve(p: &Program, at: usize, x: f64, ch: usize) -> f64 {
    let first = at + 2;
    if x < 0.0 {
        return p.words[first][ch] as f64 + x * p.words[at + 1][ch] as f64;
    }
    let t = x / (x + 1.0) * CURVE_SAMPLES as f64;
    let i = t as usize;
    if i >= CURVE_SAMPLES - 1 {
        return x + p.words[first + CURVE_SAMPLES - 1][ch] as f64;
    }
    let (a, b) = (p.words[first + i][ch] as f64, p.words[first + i + 1][ch] as f64);
    x + a + (b - a) * (t - i as f64)
}

#[test]
fn a_baked_curve_reads_back_as_the_curve_within_a_hair() {
    let b = GradeBasic { contrast: 60.0, highlights: -80.0, shadows: 40.0, whites: 30.0, blacks: -20.0, ..GradeBasic::default() };
    let p = compile(&basic(b.clone()));
    for i in -20..2000 {
        let x = i as f64 * 0.001;
        let want = basic_tone(x, &b);
        let got = read_curve(&p, 1, x, 0);
        assert!((got - want).abs() < 2e-4, "{x}: {got} vs {want}");
    }
}

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
    let sliders: [(&str, Slider); 5] = [
        ("contrast", contrast),
        ("highlights", highlights),
        ("shadows", shadows),
        ("whites", whites),
        ("blacks", blacks),
    ];
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
    let of =
        |temp: f32, tint: f32| apply_matrix(&basic_matrix(&GradeBasic { temp, tint, ..GradeBasic::default() }), grey);
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
                    rgb: vec![
                        CurvePoint::new(0.0, 0.0, 0.0),
                        CurvePoint::new(0.4, 0.4, 0.0),
                        CurvePoint::new(1.0, 1.0, 0.0),
                    ],
                    ..GradeToneCurve::default()
                }),
            ],
        }],
        split: None,
    };
    assert!(stack.is_identity());
    assert!(compile(&stack).is_empty());
    assert!(GradeStack::default().is_identity());
    let weak = GradeStack {
        masks: vec![GradeMask {
            amount: 0.0,
            ..basic(GradeBasic { exposure: 1.0, ..GradeBasic::default() }).masks[0].clone()
        }],
        split: None,
    };
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
    let b = GradeBasic {
        contrast: 60.0,
        highlights: -80.0,
        shadows: 40.0,
        whites: 30.0,
        blacks: -20.0,
        ..GradeBasic::default()
    };
    let p = compile(&basic(b.clone()));
    for i in -20..2000 {
        let x = i as f64 * 0.001;
        let want = basic_tone(x, &b);
        let got = read_curve(&p, 1, x, 0);
        assert!((got - want).abs() < 2e-4, "{x}: {got} vs {want}");
    }
}

// ---------------------------------------------------------------------------------------------
// Part 2: the colour panels.

use crate::frame::{GradeCalibration, GradeColourGrading, GradeHsl, GradePick, GradePointColour, GradeWheel};

const RED: [f64; 3] = [1.0, 0.0, 0.0];
const BLUE: [f64; 3] = [0.0, 0.0, 1.0];

fn chroma(c: [f64; 3]) -> f64 {
    lch(oklab(c))[1]
}

fn hue_deg(c: [f64; 3]) -> f64 {
    lch(oklab(c))[2].to_degrees()
}

fn near3(a: [f64; 3], b: [f64; 3], by: f64) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() <= by)
}

fn steps_of(panel: GradePanel) -> u32 {
    compile(&GradeStack { masks: vec![GradeMask { amount: 1.0, panels: vec![panel] }], split: None }).steps
}

#[test]
fn the_bands_sit_on_lightroom_s_hues_in_rising_order_and_share_every_hue() {
    let c = band_hues().map(f64::to_degrees);
    let want = [29.2, 52.8, 109.8, 142.5, 194.8, 264.1, 293.8, 328.4];
    for i in 0..8 {
        assert!((c[i] - want[i]).abs() < 0.2, "{}: {} not {}", BAND_NAMES[i], c[i], want[i]);
    }
    for d in 0..3600 {
        let w = band_weights((d as f64 / 10.0).to_radians());
        assert!((w.iter().sum::<f64>() - 1.0).abs() < 1e-12, "{d}");
        assert!(w.iter().filter(|v| **v > 0.0).count() <= 2, "at most two bands share a hue");
    }
    for (i, h) in band_hues().iter().enumerate() {
        assert!((band_weights(*h)[i] - 1.0).abs() < 1e-9, "a band's centre is its alone");
    }
}

#[test]
fn hsl_at_its_defaults_adds_no_step_and_its_row_is_all_zero() {
    assert_eq!(steps_of(GradePanel::Hsl(GradeHsl::default())), 0);
    assert!(hsl_row(&GradeHsl::default()).iter().flatten().all(|v| *v == 0.0));
    let mut h = GradeHsl::default();
    h.luminance[3] = 10.0;
    assert_eq!(steps_of(GradePanel::Hsl(h)), 1);
}

#[test]
fn hsl_blue_saturation_down_greys_blue_and_leaves_red() {
    let mut h = GradeHsl::default();
    h.saturation[5] = -100.0;
    let row = hsl_row(&h);
    let blue = unoklab(hsl(oklab(BLUE), &row));
    assert!(chroma(blue) < 1e-4, "{blue:?}");
    let red = unoklab(hsl(oklab(RED), &row));
    assert!(near3(red, RED, 1e-6), "{red:?}");
    // A blue a little off the band's centre is greyed in full too.
    let off = unoklab(hsl(oklab([0.03, 0.08, 0.6]), &row));
    assert!(chroma(off) < 1e-4, "{off:?}");
}

#[test]
fn hsl_red_hue_up_turns_red_towards_orange_and_greys_never_move() {
    let mut h = GradeHsl::default();
    h.hue[0] = 100.0;
    let row = hsl_row(&h);
    let turned = hue_deg(unoklab(hsl(oklab(RED), &row)));
    assert!(turned > 29.2 + 15.0 && turned < 52.8, "{turned}");
    let all = GradeHsl { hue: [100.0; 8], saturation: [100.0; 8], luminance: [-100.0; 8] };
    let row = hsl_row(&all);
    for v in [0.0, 0.05, 0.18, 1.0, 4.0] {
        let g = unoklab(hsl(oklab([v; 3]), &row));
        assert!(near3(g, [v; 3], 1e-6 * v.max(1.0)), "{v}: {g:?}");
    }
}

#[test]
fn hsl_luminance_down_darkens_a_band_and_dull_colours_less() {
    let mut h = GradeHsl::default();
    h.luminance[5] = -100.0;
    let row = hsl_row(&h);
    let blue = hsl(oklab(BLUE), &row);
    assert!(blue[0] < oklab(BLUE)[0] * 0.6, "{blue:?}");
    let dull = [0.30, 0.30, 0.34];
    let moved = hsl(oklab(dull), &row)[0] / oklab(dull)[0];
    assert!(moved > 0.6 && moved < 1.0, "{moved}");
}

fn pick(colour: [f32; 3]) -> GradePick {
    GradePick { colour, ..GradePick::default() }
}

#[test]
fn a_point_colour_pick_greys_its_colour_and_leaves_far_ones() {
    let picks: Vec<PickWords> =
        [GradePick { saturation: -100.0, ..pick([1.0, 0.0, 0.0]) }].iter().filter_map(pick_words).collect();
    let red = unoklab(point_colour(oklab(RED), &picks));
    assert!(chroma(red) < 1e-4, "{red:?}");
    let blue = unoklab(point_colour(oklab(BLUE), &picks));
    assert!(near3(blue, BLUE, 1e-6), "{blue:?}");
    let grey = unoklab(point_colour(oklab([0.2; 3]), &picks));
    assert!(near3(grey, [0.2; 3], 1e-6), "a grey is far from red");
}

#[test]
fn a_point_colour_s_range_widens_what_it_takes_and_a_still_pick_adds_nothing() {
    let near = unoklab(from_lch({
        let [l, c, h] = lch(oklab(RED));
        [l, c, h + 20f64.to_radians()]
    }));
    let weight = |range: f32| {
        let p = pick_words(&GradePick { saturation: -100.0, range, ..pick([1.0, 0.0, 0.0]) }).unwrap();
        pick_weight(oklab(near), &p)
    };
    assert!(
        weight(0.0) < weight(50.0) && weight(50.0) < weight(100.0),
        "{} {} {}",
        weight(0.0),
        weight(50.0),
        weight(100.0)
    );
    assert!(weight(100.0) > 0.75, "Range 100 takes most of a red 20° away");
    assert!(weight(0.0) < 0.05, "Range 0 hardly any");
    assert!(pick_words(&pick([1.0, 0.0, 0.0])).is_none());
    let still = GradePointColour { picks: vec![pick([1.0, 0.0, 0.0])] };
    assert_eq!(steps_of(GradePanel::PointColour(still)), 0);
}

fn wheel(hue: f32, saturation: f32, luminance: f32) -> GradeWheel {
    GradeWheel { hue, saturation, luminance }
}

#[test]
fn the_tone_regions_share_every_lightness_and_balance_moves_them() {
    let w = wheels_words(&GradeColourGrading::default());
    for i in 0..=200 {
        let l = i as f64 / 200.0;
        let k = wheel_weights(l, w.head[0], w.head[1]);
        assert!((k.iter().sum::<f64>() - 1.0).abs() < 1e-12, "{l}: {k:?}");
    }
    assert!(wheel_weights(0.1, w.head[0], w.head[1])[0] > 0.95);
    assert!(wheel_weights(0.95, w.head[0], w.head[1])[2] > 0.95);
    let mid = grey_lightness();
    assert!(wheel_weights(mid, w.head[0], w.head[1])[1] > 0.95, "middle grey is the midtones'");
    let up = wheels_words(&GradeColourGrading { balance: 100.0, ..Default::default() });
    let l = mid + 0.1;
    assert!(
        wheel_weights(l, up.head[0], up.head[1])[2] > wheel_weights(l, w.head[0], w.head[1])[2],
        "Balance + gives the highlights more"
    );
    let hard = wheels_words(&GradeColourGrading { blending: 0.0, ..Default::default() });
    let soft = wheels_words(&GradeColourGrading { blending: 100.0, ..Default::default() });
    let l = 0.3;
    assert!(wheel_weights(l, hard.head[0], hard.head[1])[0] > wheel_weights(l, soft.head[0], soft.head[1])[0]);
}

#[test]
fn a_shadows_tint_warms_the_darks_and_leaves_white_and_black() {
    let g = GradeColourGrading { shadows: wheel(30.0, 60.0, 0.0), ..Default::default() };
    assert_eq!(steps_of(GradePanel::ColourGrading(g.clone())), 1);
    let w = wheels_words(&g);
    let dark = unoklab(colour_wheels(oklab([0.03; 3]), &w));
    assert!(dark[0] > dark[2] * 1.2, "warmer: {dark:?}");
    let white = unoklab(colour_wheels(oklab([1.0; 3]), &w));
    assert!(near3(white, [1.0; 3], 1e-3), "{white:?}");
    let black = unoklab(colour_wheels(oklab([0.0; 3]), &w));
    assert!(near3(black, [0.0; 3], 1e-12), "{black:?}");
    let hue_only = GradeColourGrading { shadows: wheel(200.0, 0.0, 0.0), blending: 10.0, ..Default::default() };
    assert_eq!(steps_of(GradePanel::ColourGrading(hue_only)), 0, "a hue with no saturation does nothing");
}

#[test]
fn colour_grading_luminance_lifts_its_region_and_global_scales_all() {
    let lift = wheels_words(&GradeColourGrading { shadows: wheel(0.0, 0.0, 100.0), ..Default::default() });
    let dark = colour_wheels(oklab([0.02; 3]), &lift)[0];
    assert!(dark > oklab([0.02; 3])[0] + 0.15, "{dark}");
    let bright = colour_wheels(oklab([0.9; 3]), &lift)[0];
    assert!((bright - oklab([0.9; 3])[0]).abs() < 0.01, "{bright}");
    let global = wheels_words(&GradeColourGrading { global: wheel(0.0, 0.0, -100.0), ..Default::default() });
    let l = colour_wheels(oklab([0.5; 3]), &global)[0];
    assert!((l / oklab([0.5; 3])[0] - (-0.5f64).exp2()).abs() < 1e-9);
}

#[test]
fn the_wheel_s_hues_point_the_way_lightroom_s_do() {
    // 0 red, 120 green, 240 blue: the tint's direction is the colour's own.
    for (deg, colour) in [(0.0, RED), (120.0, [0.0, 1.0, 0.0]), (240.0, BLUE)] {
        let [a, b] = hsv_hue_direction(deg);
        let h = b.atan2(a).rem_euclid(std::f64::consts::TAU).to_degrees();
        assert!((h - hue_deg(colour)).abs() < 1e-6, "{deg}: {h}");
    }
}

#[test]
fn calibration_keeps_every_grey_and_turns_its_primaries() {
    let c = GradeCalibration {
        red_hue: 60.0,
        red_saturation: -40.0,
        green_hue: -30.0,
        green_saturation: 80.0,
        blue_hue: 100.0,
        blue_saturation: 100.0,
        ..Default::default()
    };
    let m = calibration_matrix(&c);
    for v in [0.0, 0.1, 0.18, 1.0, 5.0] {
        assert!(near3(apply_matrix(&m, [v; 3]), [v; 3], 1e-9 * v.max(1.0)), "{v}");
    }
    let only = |c: GradeCalibration| apply_matrix(&calibration_matrix(&c), RED);
    let turned = only(GradeCalibration { red_hue: 100.0, ..Default::default() });
    assert!(hue_deg(turned) > hue_deg(RED) + 5.0, "Red + goes towards orange: {turned:?}");
    let duller = only(GradeCalibration { red_saturation: -100.0, ..Default::default() });
    assert!(chroma(duller) < chroma(RED) * 0.9, "{duller:?}");
    let still = calibration_matrix(&GradeCalibration::default());
    assert!(near3(apply_matrix(&still, [0.3, 0.5, 0.7]), [0.3, 0.5, 0.7], 1e-6));
    assert_eq!(steps_of(GradePanel::Calibration(GradeCalibration::default())), 0);
    assert_eq!(steps_of(GradePanel::Calibration(c)), 1);
}

#[test]
fn shadows_tint_makes_darks_magenta_at_their_brightness_and_leaves_white() {
    let g = dark_tint_gain(100.0);
    assert!((luminance(g) - 1.0).abs() < 1e-12);
    let dark = dark_tint([0.02; 3], g);
    assert!(dark[1] < dark[0] && dark[1] < dark[2], "{dark:?}");
    assert!((luminance(dark) - 0.02).abs() < 1e-9);
    assert_eq!(dark_tint([0.5; 3], g), [0.5; 3]);
    assert_eq!(dark_tint([0.0; 3], g), [0.0; 3]);
    let green = dark_tint([0.02; 3], dark_tint_gain(-100.0));
    assert!(green[1] > green[0]);
    let both = GradeCalibration { shadows_tint: -20.0, red_hue: 5.0, ..Default::default() };
    assert_eq!(steps_of(GradePanel::Calibration(both)), 2);
}

#[test]
fn a_pick_takes_a_glow_that_shows_as_its_colour() {
    // A glow far above white is matched by the colour the screen shows it as.
    let glow = [4.0, 0.6, 0.2];
    let shown = roll_off(glow);
    assert!(shown.iter().all(|v| *v <= 1.0) && shown[0] > shown[2], "{shown:?}");
    let p = pick_words(&GradePick { saturation: -50.0, ..pick(shown.map(|v| v as f32)) }).unwrap();
    assert!(pick_weight(oklab(glow), &p) > 0.8, "{}", pick_weight(oklab(glow), &p));
}

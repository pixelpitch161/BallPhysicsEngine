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
    GradeStack {
        masks: vec![GradeMask { amount: 1.0, panels: vec![GradePanel::Basic(b)], ..GradeMask::default() }],
        split: None,
    }
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
            ..GradeMask::default()
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
        let mut at = p.segments[0].first as usize;
        let mut out = Vec::new();
        while at < p.words.len() {
            let h = p.words[at];
            out.push((h[0] as u32, h[1] as u32));
            at += h[2] as usize;
        }
        out
    };
    assert_eq!(ops, [(OP_MATRIX, SPACE_LINEAR), (OP_CURVE, SPACE_TONE), (OP_CHROMA, SPACE_OKLAB)]);
    // One segment (no area steps), its three steps right after the table.
    assert_eq!(p.words[0][0], 1.0);
    assert_eq!(p.words[1], [2.0, 3.0, 0.0, 0.0]);
    assert_eq!(p.segments, [Segment { first: 2, steps: 3, pyramid: false }]);
    // A curve step: its header, the slopes at 0 and its samples.
    assert_eq!(p.words.len(), 2 + 4 + (2 + CURVE_SAMPLES) + 2);
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
        let got = read_curve(&p, p.segments[0].first as usize, x, 0);
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
    compile(&GradeStack {
        masks: vec![GradeMask { amount: 1.0, panels: vec![panel], ..GradeMask::default() }],
        split: None,
    })
    .steps
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

// ---------------------------------------------------------------------------------------------
// Part 3: area steps, Detail and Effects.

use crate::frame::{GradeDetail, GradeEffects};

fn program(panels: Vec<GradePanel>) -> Program {
    compile(&GradeStack { masks: vec![GradeMask { amount: 1.0, panels, ..GradeMask::default() }], split: None })
}

/// Each segment's ops, in order.
fn segment_ops(p: &Program) -> Vec<Vec<u32>> {
    p.segments
        .iter()
        .map(|s| {
            let mut at = s.first as usize;
            (0..s.steps)
                .map(|_| {
                    let op = p.words[at][0] as u32;
                    at += p.words[at][2] as usize;
                    op
                })
                .collect()
        })
        .collect()
}

#[test]
fn an_area_step_starts_a_segment_that_reads_the_pyramid() {
    let p = program(vec![GradePanel::Basic(GradeBasic { exposure: 1.0, clarity: 40.0, ..GradeBasic::default() })]);
    assert_eq!(segment_ops(&p), [vec![OP_MATRIX], vec![OP_CLARITY]]);
    assert!(!p.segments[0].pyramid && p.segments[1].pyramid);
    // The table holds the same.
    assert_eq!(p.words[0][0], 2.0);
    assert_eq!(p.words[2], [p.segments[1].first as f32, 1.0, 1.0, 0.0]);
    assert_eq!(p.steps, 2);
}

#[test]
fn area_steps_side_by_side_share_a_segment() {
    let b = GradeBasic { texture: 20.0, clarity: 30.0, dehaze: 40.0, saturation: 10.0, ..GradeBasic::default() };
    let p = program(vec![
        GradePanel::Basic(b.clone()),
        GradePanel::Detail(GradeDetail { amount: 50.0, ..Default::default() }),
    ]);
    // Saturation is a point step between them, so Sharpening starts its own.
    assert_eq!(segment_ops(&p), [vec![], vec![OP_TEXTURE, OP_CLARITY, OP_DEHAZE, OP_CHROMA], vec![OP_SHARPEN]]);
    let b = GradeBasic { saturation: 0.0, ..b };
    let p = program(vec![GradePanel::Basic(b), GradePanel::Detail(GradeDetail { amount: 50.0, ..Default::default() })]);
    assert_eq!(segment_ops(&p), [vec![], vec![OP_TEXTURE, OP_CLARITY, OP_DEHAZE, OP_SHARPEN]]);
    assert!(p.segments[1].pyramid);
}

#[test]
fn sharpening_alone_needs_no_pyramid_and_an_empty_first_pass() {
    let p = program(vec![GradePanel::Detail(GradeDetail { amount: 40.0, ..Default::default() })]);
    assert_eq!(segment_ops(&p), [vec![], vec![OP_SHARPEN]]);
    assert!(!p.segments[1].pyramid);
    let w = p.words[p.segments[1].first as usize + 1];
    assert_eq!(w, [0.4, sharpen_limit(25.0) as f32, 1.0, 0.0]);
}

#[test]
fn a_vignette_is_an_area_step_only_with_highlights_under_a_dark_one() {
    let e = |amount: f32, highlights: f32| {
        program(vec![GradePanel::Effects(GradeEffects {
            vignette_amount: amount,
            vignette_highlights: highlights,
            grain_amount: 10.0,
            ..GradeEffects::default()
        })])
    };
    assert_eq!(segment_ops(&e(-50.0, 0.0)), [vec![OP_VIGNETTE, OP_GRAIN]]);
    assert_eq!(segment_ops(&e(50.0, 80.0)), [vec![OP_VIGNETTE, OP_GRAIN]]);
    assert_eq!(segment_ops(&e(-50.0, 80.0)), [vec![], vec![OP_VIGNETTE, OP_GRAIN]]);
}

#[test]
fn detail_and_effects_at_their_defaults_add_nothing() {
    assert!(GradeDetail::default().is_identity() && GradeEffects::default().is_identity());
    assert_eq!(steps_of(GradePanel::Detail(GradeDetail { radius: 3.0, detail: 90.0, ..Default::default() })), 0);
    let quiet = GradeEffects { vignette_midpoint: 10.0, grain_size: 90.0, ..GradeEffects::default() };
    assert_eq!(steps_of(GradePanel::Effects(quiet)), 0);
    assert!(GradeBasic { texture: -0.0, ..GradeBasic::default() }.is_identity());
    assert!(!GradeBasic { dehaze: 1.0, ..GradeBasic::default() }.is_identity());
    let map = GradeEffects { map_amount: 50.0, ..GradeEffects::default() };
    let p = program(vec![GradePanel::Effects(map)]);
    assert_eq!(segment_ops(&p), [vec![OP_GRADIENT_MAP]]);
    assert_eq!(p.words[p.segments[0].first as usize][2] as usize, 1 + MAP_SAMPLES);
}

#[test]
fn lift_moves_a_grey_by_exactly_the_change_and_keeps_a_bright_colour_s_ratios() {
    for y in [0.0, 0.1, 0.5, 0.9, 1.5] {
        for d in [-0.05, 0.02, 0.2] {
            let out = lift([y; 3], d);
            let want = (y + d).max(-LIFT_SOFT);
            assert!(out.iter().all(|v| (v - want).abs() < 1e-12), "{y} {d}: {out:?}");
        }
    }
    let c = [0.9, 0.5, 0.3];
    let out = lift(c, 0.05);
    let ratio = |c: [f64; 3]| (c[0] + LIFT_SOFT) / (c[2] + LIFT_SOFT);
    assert!((ratio(out) - ratio(c)).abs() < 1e-12);
    assert!((luminance(out) - luminance(c) - 0.05).abs() < 1e-9);
}

#[test]
fn soft_limit_is_odd_rising_and_never_past_its_limit() {
    let mut last = f64::NEG_INFINITY;
    for i in -400..=400 {
        let d = i as f64 * 0.01;
        let y = soft_limit(d, 0.15);
        assert!(y >= last && y.abs() <= 0.15 && (y + soft_limit(-d, 0.15)).abs() < 1e-12);
        last = y;
    }
    assert!((soft_limit(0.001, 0.15) - 0.001).abs() < 1e-6, "small changes pass as they are");
    assert!(sharpen_limit(0.0) < sharpen_limit(25.0) && sharpen_limit(25.0) < sharpen_limit(100.0));
}

#[test]
fn clarity_works_most_in_the_midtones() {
    assert_eq!(clarity_weight(0.5), 1.0);
    assert!(clarity_weight(0.1) < clarity_weight(0.3));
    assert_eq!(clarity_weight(3.0), 0.25);
    assert_eq!(clarity_weight(-1.0), 0.25);
}

#[test]
fn dehaze_takes_a_veil_off_keeping_white_and_black_and_minus_lays_a_fog() {
    let haze = [0.5, 0.5, 0.5];
    let out = dehaze(haze, 0.5, 0.5, 0.5, 1.0);
    assert!(out[0] < 0.5 && out[0] > 0.0, "{out:?}");
    assert_eq!(dehaze([1.0; 3], 1.0, 0.6, 0.6, 1.0), [1.0; 3]);
    assert_eq!(dehaze([0.0; 3], 0.0, 0.6, 0.6, 1.0), [0.0; 3]);
    // A colour in haze comes back more colourful.
    let c = [0.6, 0.4, 0.3];
    let out = dehaze(c, 0.3, 0.3, 0.4, 1.0);
    assert!(out[0] / out[2] > c[0] / c[2]);
    // No channel gains more than 2.5 times.
    let hot = dehaze([0.95, 0.9, 0.9], 0.9, 0.95, 0.9, 1.0);
    assert!(hot.iter().zip([0.95, 0.9, 0.9]).all(|(a, b)| a / b <= 2.5 + 1e-9), "{hot:?}");
    let fogged = dehaze([0.0; 3], 0.0, 0.0, 0.2, -1.0);
    assert!(fogged[0] > 0.3, "{fogged:?}");
}

#[test]
fn the_vignette_lies_in_the_corners_not_the_middle() {
    let e = |m: f32, r: f32, f: f32| {
        vignette_words(&GradeEffects {
            vignette_amount: -100.0,
            vignette_midpoint: m,
            vignette_roundness: r,
            vignette_feather: f,
            ..GradeEffects::default()
        })
    };
    let size = [1080.0, 1920.0];
    let w = e(50.0, 0.0, 50.0);
    assert_eq!(vignette_weight([0.5, 0.5], size, w), 0.0);
    assert!(vignette_weight([0.0, 0.0], size, w) > 0.99);
    // Further out with a bigger midpoint; harder with no feather.
    let edge = [0.0, 0.5];
    assert!(vignette_weight(edge, size, e(90.0, 0.0, 50.0)) < vignette_weight(edge, size, w));
    let hard = e(50.0, 0.0, 0.0);
    assert!(vignette_weight([0.5, 0.5], size, hard) == 0.0 && vignette_weight([0.02, 0.02], size, hard) == 1.0);
    // A circle reaches further down the tall picture's sides than the ellipse does.
    let side = [0.5, 0.08];
    assert!(vignette_weight(side, size, e(50.0, 100.0, 50.0)) > vignette_weight(side, size, w));
    // Darkens by the weight; Highlights keeps a bright place; up lightens towards white.
    assert_eq!(vignette([0.5; 3], 1.0, -1.0, 0.0, 0.0), [0.0; 3]);
    assert_eq!(vignette([2.0; 3], 1.0, -1.0, 1.0, 2.0), [2.0; 3]);
    let up = vignette([0.2; 3], 1.0, 0.5, 0.0, 0.0);
    assert!(up[0] > 0.2 && up[0] < 1.0);
}

#[test]
fn the_gradient_map_runs_from_its_first_colour_to_its_last() {
    let e = GradeEffects {
        map: bpe_core::Gradient::even(&[bpe_core::Color::rgb(0, 0, 255), bpe_core::Color::rgb(255, 0, 0)]),
        map_amount: 100.0,
        ..GradeEffects::default()
    };
    let row = gradient_row(&e);
    assert_eq!(row.len(), MAP_SAMPLES);
    assert!(near3(gradient_map([0.0; 3], &row, 1.0), [0.0, 0.0, 1.0], 1e-6));
    assert!(near3(gradient_map([1.0; 3], &row, 1.0), [1.0, 0.0, 0.0], 1e-6));
    // Brighter than white stays bright; amount 0 leaves the colour.
    assert!(gradient_map([2.0; 3], &row, 1.0)[0] > 1.9);
    assert_eq!(gradient_map([0.3, 0.2, 0.1], &row, 0.0), [0.3, 0.2, 0.1]);
}

// ---------------------------------------------------------------------------------------------
// Part 4: masks.

use crate::frame::{
    GradeArea, GradeColourRange, GradeCombine, GradeLinear, GradeLuminanceRange, GradePart, GradePlace, GradeRadial,
    GradeShape,
};

fn radial_at(x: f32, y: f32, size: f32) -> GradeRadial {
    GradeRadial {
        x,
        y,
        width: size,
        height: size,
        angle: 0.0,
        roundness: 0.0,
        feather: 0.0,
        places: vec![GradePlace::SCREEN],
    }
}

fn part(shape: GradeShape, combine: GradeCombine) -> GradePart {
    GradePart { combine, invert: false, shape, id: 0 }
}

#[test]
fn a_linear_gradient_fades_out_across_its_feather_the_way_it_points() {
    // Pointing down (90°): full effect above, none below, half on its middle line.
    let l = GradeLinear { x: 0.0, y: 100.0, angle: 90.0, feather: 200.0, places: vec![GradePlace::SCREEN] };
    let n = linear_numbers(&l);
    let at = |y: f64| linear_cover(n, &GradePlace::SCREEN, [37.0, y], 1.0);
    assert!((at(100.0) - 0.5).abs() < 1e-9);
    assert_eq!(at(-1.0), 1.0);
    assert_eq!(at(201.0), 0.0);
    assert!(at(50.0) > at(150.0));
    // Turned by its place a quarter turn clockwise, it points left.
    let turned = GradePlace { turn: std::f32::consts::FRAC_PI_2, ..GradePlace::SCREEN };
    let n = linear_numbers(&GradeLinear { y: 0.0, ..l });
    assert!(linear_cover(n, &turned, [150.0, 0.0], 1.0) > 0.99);
    assert!(linear_cover(n, &turned, [-150.0, 0.0], 1.0) < 0.01);
}

#[test]
fn a_radial_gradient_covers_inside_and_fades_over_its_feather() {
    let r = GradeRadial { feather: 50.0, ..radial_at(10.0, -20.0, 200.0) };
    let n = radial_numbers(&r);
    let at = |x: f64, y: f64| radial_cover(n, &GradePlace::SCREEN, [x, y], 1.0);
    assert_eq!(at(10.0, -20.0), 1.0);
    assert_eq!(at(10.0 + 40.0, -20.0), 1.0, "inside the feather's start");
    assert!((at(10.0 + 75.0, -20.0) - 0.5).abs() < 1e-9, "half-way across the feather");
    assert_eq!(at(10.0 + 101.0, -20.0), 0.0);
    // Roundness 100 makes a wide ellipse a circle on its short side.
    let wide = GradeRadial { width: 400.0, height: 100.0, roundness: 100.0, ..radial_at(0.0, 0.0, 0.0) };
    let n = radial_numbers(&wide);
    assert_eq!(radial_cover(n, &GradePlace::SCREEN, [100.0, 0.0], 1.0), 0.0);
    assert_eq!(radial_cover(n, &GradePlace::SCREEN, [40.0, 0.0], 1.0), 1.0);
    // At a place: offset turned and scaled with it, sizes scaled.
    let place = GradePlace { at: [100.0, 50.0], turn: std::f32::consts::FRAC_PI_2, scale: 2.0, world: false };
    let n = radial_numbers(&radial_at(10.0, 0.0, 20.0));
    assert_eq!(radial_cover(n, &place, [100.0, 70.0], 1.0), 1.0, "its middle moved down by 2 × 10");
    assert!(radial_cover(n, &place, [100.0, 70.0 + 19.0], 1.0) > 0.0, "radius 2 × 10");
    assert_eq!(radial_cover(n, &place, [100.0, 70.0 + 21.0], 1.0), 0.0);
}

#[test]
fn parts_join_by_add_subtract_and_intersect_and_invert() {
    assert_eq!(combine(0.25, 0.5, GradeCombine::Add), 0.5);
    assert_eq!(combine(0.5, 0.25, GradeCombine::Subtract), 0.375);
    assert_eq!(combine(0.5, 0.5, GradeCombine::Intersect), 0.25);
    let left = GradeShape::Radial(radial_at(-50.0, 0.0, 80.0));
    let right = GradeShape::Radial(radial_at(50.0, 0.0, 80.0));
    let middle = GradeShape::Radial(radial_at(0.0, 0.0, 80.0));
    let grey = [0.2; 3];
    let cover =
        |parts: Vec<GradePart>, invert: bool, x: f64| area_cover(&GradeArea { parts, invert }, grey, [x, 0.0], 1.0);
    let add = vec![part(left.clone(), GradeCombine::Add), part(right.clone(), GradeCombine::Add)];
    assert_eq!(
        [cover(add.clone(), false, -50.0), cover(add.clone(), false, 50.0), cover(add, false, 0.0)],
        [1.0, 1.0, 0.0]
    );
    let sub = vec![part(middle.clone(), GradeCombine::Add), part(right.clone(), GradeCombine::Subtract)];
    assert_eq!([cover(sub.clone(), false, -30.0), cover(sub, false, 30.0)], [1.0, 0.0]);
    let both = vec![part(middle.clone(), GradeCombine::Add), part(right, GradeCombine::Intersect)];
    assert_eq!([cover(both.clone(), false, -30.0), cover(both.clone(), false, 30.0)], [0.0, 1.0]);
    assert_eq!(cover(both, true, -30.0), 1.0, "the mask's Invert");
    // The first part's Combine is never read; a part's Invert covers its outside.
    let first = vec![GradePart { invert: true, ..part(middle, GradeCombine::Subtract) }];
    assert_eq!([cover(first.clone(), false, 0.0), cover(first, false, 100.0)], [0.0, 1.0]);
    assert_eq!(cover(Vec::new(), true, 0.0), 0.0, "no part covers nothing, inverted or not");
    // A gradient with no place (its thing is gone) covers nothing.
    let gone = GradeShape::Radial(GradeRadial { places: Vec::new(), ..radial_at(0.0, 0.0, 80.0) });
    assert_eq!(cover(vec![part(gone, GradeCombine::Add)], false, 0.0), 0.0);
}

#[test]
fn copies_of_a_gradient_join_with_add() {
    let places = [[-100.0, 0.0], [0.0, 30.0], [100.0, -10.0]].map(|at| GradePlace { at, ..GradePlace::SCREEN });
    let shape = GradeShape::Radial(GradeRadial { places: places.to_vec(), ..radial_at(0.0, 0.0, 20.0) });
    for [x, y] in [[-100.0, 0.0], [0.0, 30.0], [100.0, -10.0]] {
        assert_eq!(part_cover(&shape, [0.2; 3], [x, y], 1.0), 1.0);
    }
    assert_eq!(part_cover(&shape, [0.2; 3], [50.0, 0.0], 1.0), 0.0);
}

#[test]
fn a_luminance_range_takes_the_brightnesses_between_its_ends() {
    let l = luminance_numbers(&GradeLuminanceRange { low: 60.0, high: 100.0, smoothness: 0.0 });
    assert_eq!(luminance_cover([0.9; 3], l), 1.0);
    assert_eq!(luminance_cover([0.05; 3], l), 0.0);
    assert_eq!(luminance_cover([5.0; 3], l), 1.0, "past white reads as white");
    // Low and High kept in order, smoothness widening the fade.
    let swapped = luminance_numbers(&GradeLuminanceRange { low: 40.0, high: 10.0, smoothness: 100.0 });
    assert_eq!([swapped[0], swapped[1], swapped[2]], [0.1, 0.4, 0.3]);
    let y = shown_luma([0.18; 3]);
    assert!((y - tone(0.18)).abs() < 1e-9);
}

#[test]
fn a_colour_range_covers_colours_near_its_picks() {
    let red = [0.6, 0.04, 0.03];
    let r = GradeColourRange { picks: vec![[0.6, 0.04, 0.03], [0.05, 0.6, 0.05]], refine: 50.0 };
    let (picks, widths) = (range_picks(&r), range_widths(r.refine));
    assert_eq!(colour_range_cover(red, &picks, widths), 1.0);
    assert_eq!(colour_range_cover([0.03, 0.08, 0.6], &picks, widths), 0.0, "blue is far from both");
    assert!(colour_range_cover([0.55, 0.06, 0.03], &picks, widths) > 0.9, "a near red");
    // More Refine takes in more.
    let near = [0.45, 0.12, 0.03];
    assert!(
        colour_range_cover(near, &picks, range_widths(100.0)) > colour_range_cover(near, &picks, range_widths(0.0))
    );
    assert_eq!(colour_range_cover(red, &[], widths), 0.0, "no pick covers nothing");
}

/// Where the program's last `OP_END` step starts, walking the steps.
fn last_end(p: &Program) -> usize {
    let mut at = p.segments[0].first as usize;
    let mut end = None;
    for _ in 0..p.steps {
        if p.words[at][0] == OP_END as f32 {
            end = Some(at);
        }
        at += p.words[at][2] as usize;
    }
    end.expect("an end")
}

/// A mask over part of the picture.
fn masked(parts: Vec<GradePart>, panels: Vec<GradePanel>) -> GradeStack {
    GradeStack {
        masks: vec![GradeMask {
            amount: 1.0,
            area: Some(GradeArea { parts, invert: false }),
            panels,
            ..GradeMask::default()
        }],
        split: None,
    }
}

#[test]
fn a_mask_compiles_its_parts_into_its_end_and_the_whole_picture_compiles_as_before() {
    let exposure = || vec![GradePanel::Basic(GradeBasic { exposure: 1.0, ..GradeBasic::default() })];
    let whole = compile(&GradeStack {
        masks: vec![GradeMask { amount: 1.0, panels: exposure(), ..GradeMask::default() }],
        split: None,
    });
    assert_eq!(whole.steps, 1, "no begin or end for the Whole picture at full amount");
    let mask = masked(vec![part(GradeShape::Radial(radial_at(0.0, 0.0, 40.0)), GradeCombine::Add)], exposure());
    let p = compile(&mask);
    assert_eq!(p.steps, 3, "begin, the matrix, end");
    let end = last_end(&p);
    assert_eq!(p.words[end + 1], [1.0, 0.0, 0.0, 0.0], "one part, not inverted, no overlay");
    assert_eq!(p.words[end + 2], [PART_RADIAL as f32, 0.0, 0.0, 5.0]);
    assert_eq!(p.words[end + 5], [1.0, 20.0f32.hypot(20.0) + 2.0, 0.0, 0.0], "one copy and its reach");
    // Nothing to do and no overlay compiles to nothing; with the overlay it compiles.
    let mut quiet = masked(mask.masks[0].area.clone().unwrap().parts, Vec::new());
    assert!(quiet.is_identity() && compile(&quiet).is_empty());
    quiet.masks[0].overlay = true;
    assert!(!quiet.is_identity() && compile(&quiet).steps == 2);
}

#[test]
fn copies_are_sorted_down_the_screen() {
    let places = [[0.0, 30.0], [0.0, -10.0], [5.0, 10.0]].map(|at| GradePlace { at, ..GradePlace::SCREEN }).to_vec();
    let linear = GradeShape::Linear(GradeLinear { x: 0.0, y: 0.0, angle: 0.0, feather: 10.0, places });
    let p = compile(&masked(
        vec![part(linear, GradeCombine::Add)],
        vec![GradePanel::Basic(GradeBasic { exposure: 1.0, ..GradeBasic::default() })],
    ));
    let end = last_end(&p);
    let copies = &p.words[end + 4..end + 8];
    assert_eq!(copies[0][..2], [3.0, -1.0], "three copies; a linear reaches everywhere");
    assert_eq!(copies[1..].iter().map(|w| w[1]).collect::<Vec<_>>(), vec![-10.0, 10.0, 30.0]);
}

// ---------------------------------------------------------------------------------------------
// Part 5: Things selections, the brush's words and the Stage camera.

use crate::frame::{GradeBrush, GradeThings};

fn things_part(things: Option<Vec<u32>>, expand: f32) -> GradePart {
    GradePart {
        combine: GradeCombine::Add,
        invert: false,
        shape: GradeShape::Things(GradeThings { things, background: false, expand, feather: 4.0 }),
        id: 0,
    }
}

fn exposed_mask(parts: Vec<GradePart>) -> GradeMask {
    GradeMask {
        amount: 1.0,
        area: Some(GradeArea { parts, invert: false }),
        panels: vec![GradePanel::Basic(GradeBasic { exposure: 1.0, ..GradeBasic::default() })],
        ..GradeMask::default()
    }
}

#[test]
fn things_parts_share_a_selection_by_their_things_and_none_names_no_thing() {
    let stack = GradeStack {
        masks: vec![
            exposed_mask(vec![things_part(Some(vec![3, 1]), 10.0), things_part(Some(vec![1, 3, 3]), 30.0)]),
            exposed_mask(vec![things_part(None, 0.0), things_part(Some(vec![]), 0.0)]),
        ],
        split: None,
    };
    let p = compile(&stack);
    assert_eq!(p.selections.len(), 2, "{:?}", p.selections);
    assert_eq!(p.selections[0].things, Some(vec![1, 3]));
    assert_eq!(p.selections[0].reach, 34.0, "the most Expand + Feather asked of it");
    assert_eq!(p.selections[1].things, None);
    // A grade without Things parts has none.
    let plain = GradeStack { masks: vec![exposed_mask(vec![])], split: None };
    assert!(compile(&plain).selections.is_empty());
}

#[test]
fn past_the_most_selections_a_things_part_covers_nothing() {
    let parts = (0..MAX_SELECTIONS as u32 + 2).map(|i| things_part(Some(vec![i + 1]), 0.0)).collect();
    let p = compile(&GradeStack { masks: vec![exposed_mask(parts)], split: None });
    assert_eq!(p.selections.len(), MAX_SELECTIONS);
}

#[test]
fn a_brush_without_paint_covers_nothing_and_with_it_has_a_grid() {
    let empty = GradeShape::Brush(GradeBrush { strokes: Default::default(), places: vec![GradePlace::SCREEN] });
    assert_eq!(part_cover(&empty, [0.2; 3], [0.0, 0.0], 1.0), 0.0);
    let dot = bpe_core::Stroke { points: vec![[0.0, 0.0]], size: 20.0, strength: 1.0, ..Default::default() };
    let painted = GradeShape::Brush(GradeBrush {
        strokes: bpe_core::Strokes { list: vec![dot] },
        places: vec![GradePlace { at: [100.0, 0.0], ..GradePlace::SCREEN }],
    });
    assert!(part_cover(&painted, [0.2; 3], [100.0, 0.0], 1.0) > 0.99);
    assert!(part_cover(&painted, [0.2; 3], [0.0, 0.0], 1.0) < 0.01);
    let words = compile(&GradeStack {
        masks: vec![exposed_mask(vec![GradePart { shape: painted, ..things_part(None, 0.0) }])],
        split: None,
    });
    assert!(words.words.len() > 10, "the grid is in the program");
}

#[test]
fn a_moved_stage_camera_puts_world_places_where_it_shows_them_and_leaves_the_screen_alone() {
    use crate::{StageCamera, StageLayout, StageView};
    let flat = crate::CameraView { center: glam::Vec2::new(30.0, -40.0), zoom: 1.3, turn: 0.2 };
    let cam = StageCamera { orbit: 25.0, tilt: 10.0, ..StageCamera::default() };
    let layout = StageLayout { back: 240.0, k: 0.0, floor: None };
    let view = StageView::solve(&cam, &flat, glam::Vec2::new(1080.0, 1920.0), layout);
    assert!(view.motion > 0.0);
    let ball = glam::Vec2::new(200.0, 150.0);
    let s = flat.to_screen(ball);
    let on_ball = GradePlace { at: s.into(), turn: 0.0, scale: 1.3, world: true };
    let screen = GradePlace { at: [10.0, 20.0], ..GradePlace::SCREEN };
    let radial = |places| {
        GradeShape::Radial(GradeRadial {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 50.0,
            angle: 0.0,
            roundness: 0.0,
            feather: 0.0,
            places,
        })
    };
    let part = |shape| GradePart { shape, ..things_part(None, 0.0) };
    let mut stack = GradeStack { masks: vec![exposed_mask(vec![part(radial(vec![on_ball, screen]))])], split: None };
    stack.see_through(&view);
    let GradeShape::Radial(r) = &stack.masks[0].area.as_ref().unwrap().parts[0].shape else { panic!() };
    let seen = view.screen_of(ball).unwrap();
    assert!((glam::Vec2::from(r.places[0].at) - seen).length() < 0.01, "{:?} {seen:?}", r.places[0]);
    let k = view.scale_at(ball).unwrap();
    assert!((r.places[0].scale - k).abs() < 1e-3, "{} {k}", r.places[0].scale);
    assert_eq!(r.places[1], screen);
}

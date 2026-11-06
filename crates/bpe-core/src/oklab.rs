//! OKLab and OKLCH (Björn Ottosson's perceptual colour space), for gradients that mix Even,
//! Round the wheel or the long way round. Everything here works in linear-light sRGB.

/// Linear-light RGB to OKLab (L about 0 to 1, a and b about -0.4 to 0.4).
pub fn linear_to_oklab(c: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = c;
    let l = 0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b;
    let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
    let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;
    let (l, m, s) = (l.cbrt(), m.cbrt(), s.cbrt());
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

/// OKLab back to linear-light RGB. A colour outside sRGB comes back with channels below 0 or
/// above 1 (see [`fit_gamut`]).
pub fn oklab_to_linear(lab: [f32; 3]) -> [f32; 3] {
    let [l, a, b] = lab;
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l, m, s) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    [
        4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
        -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
        -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
    ]
}

/// OKLab to OKLCH: lightness, chroma, and hue in turns (0 to 1, from +a towards +b).
pub fn oklab_to_lch(lab: [f32; 3]) -> [f32; 3] {
    let [l, a, b] = lab;
    let hue = b.atan2(a) / std::f32::consts::TAU;
    [l, a.hypot(b), hue - hue.floor()]
}

/// OKLCH (hue in turns) to OKLab.
pub fn lch_to_oklab(lch: [f32; 3]) -> [f32; 3] {
    let [l, c, h] = lch;
    let (s, co) = (h * std::f32::consts::TAU).sin_cos();
    [l, c * co, c * s]
}

/// How far outside 0 to 1 a channel may sit and still count as in sRGB (rounding).
const GAMUT_SLACK: f32 = 1e-5;

/// Whether linear RGB `c` is a colour sRGB can show.
pub fn in_gamut(c: [f32; 3]) -> bool {
    c.iter().all(|&x| (-GAMUT_SLACK..=1.0 + GAMUT_SLACK).contains(&x))
}

/// The linear-light colour of OKLCH `lch`, brought into sRGB by lowering its chroma at the same
/// lightness and hue (not by clipping each channel, which would shift the hue), then clamped.
pub fn fit_gamut(lch: [f32; 3]) -> [f32; 3] {
    let at = |c: f32| oklab_to_linear(lch_to_oklab([lch[0], c, lch[2]]));
    let full = at(lch[1]);
    let clamp = |c: [f32; 3]| c.map(|x| if x.is_finite() { x.clamp(0.0, 1.0) } else { 0.0 });
    if in_gamut(full) {
        return clamp(full);
    }
    // Chroma `lo` is in sRGB (0 is a grey of that lightness, in sRGB when 0 <= L <= 1), `hi` isn't.
    let (mut lo, mut hi) = (0.0f32, lch[1]);
    for _ in 0..20 {
        let mid = (lo + hi) * 0.5;
        if in_gamut(at(mid)) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    clamp(at(lo))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: [f32; 3], b: [f32; 3], eps: f32) -> bool {
        a.iter().zip(&b).all(|(x, y)| (x - y).abs() <= eps)
    }

    #[test]
    fn matches_the_reference_values() {
        let cases = [
            ([1.0, 1.0, 1.0], [1.0, 0.0, 0.0]),
            ([1.0, 0.0, 0.0], [0.627_955, 0.224_863, 0.125_846]),
            ([0.0, 1.0, 0.0], [0.866_440, -0.233_888, 0.179_498]),
            ([0.0, 0.0, 1.0], [0.452_014, -0.032_457, -0.311_528]),
        ];
        for (rgb, want) in cases {
            let got = linear_to_oklab(rgb);
            assert!(near(got, want, 1e-4), "{rgb:?}: {got:?} vs {want:?}");
        }
        assert!(near(linear_to_oklab([0.0; 3]), [0.0; 3], 1e-6));
    }

    #[test]
    fn round_trips() {
        for i in 0..100u32 {
            let c = [(i % 5) as f32 / 4.0, (i / 5 % 5) as f32 / 4.0, (i / 25) as f32 / 3.0];
            assert!(near(oklab_to_linear(linear_to_oklab(c)), c, 1e-4), "{c:?}");
            let lab = linear_to_oklab(c);
            assert!(near(lch_to_oklab(oklab_to_lch(lab)), lab, 1e-5), "{c:?}");
        }
    }

    #[test]
    fn fitting_keeps_lightness_and_hue_and_lands_in_srgb() {
        // A very saturated cyan-blue no screen can show.
        let lch = [0.6, 0.4, 0.62];
        let out = fit_gamut(lch);
        assert!(in_gamut(out));
        let back = oklab_to_lch(linear_to_oklab(out));
        assert!((back[0] - 0.6).abs() < 2e-3, "{back:?}");
        assert!((back[2] - 0.62).abs() < 5e-3, "{back:?}");
        assert!(back[1] < 0.4);
        // An sRGB colour is left as it is.
        let red = oklab_to_lch(linear_to_oklab([1.0, 0.0, 0.0]));
        assert!(near(fit_gamut(red), [1.0, 0.0, 0.0], 1e-4));
    }
}

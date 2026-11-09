//! The grade's maths on the CPU: the working spaces, white balance, the Basic panel's tone
//! sliders, the parametric and point curves. Curves are baked into lookup rows from here; the
//! shader (`shaders/grade.wgsl`) does the same space changes per pixel.

use crate::frame::{GradeBasic, GradeToneCurve};
use bpe_core::CurvePoint;

/// Linear light to the tone space: the sRGB transfer curve. Above 1 it is the same formula carried
/// on (same slope and curvature at 1, rising for ever, finite at the half-float limit and past it);
/// below 0 it is mirrored.
pub fn tone(x: f64) -> f64 {
    let a = x.abs();
    let e = if a <= 0.003_130_8 { a * 12.92 } else { 1.055 * a.powf(1.0 / 2.4) - 0.055 };
    e.copysign(x)
}

/// The tone space back to linear light ([`tone`] undone).
pub fn untone(p: f64) -> f64 {
    let a = p.abs();
    let l = if a <= 0.040_45 { a / 12.92 } else { ((a + 0.055) / 1.055).powf(2.4) };
    l.copysign(p)
}

/// Middle grey (18% linear) in the tone space: Contrast's pivot.
pub fn grey() -> f64 {
    tone(0.18)
}

/// A cubic Hermite piece from (x0, y0) with slope d0 to (x1, y1) with slope d1, at x.
fn hermite(x0: f64, x1: f64, y0: f64, y1: f64, d0: f64, d1: f64, x: f64) -> f64 {
    let h = x1 - x0;
    let t = (x - x0) / h;
    let (t2, t3) = (t * t, t * t * t);
    (2.0 * t3 - 3.0 * t2 + 1.0) * y0 + (t3 - 2.0 * t2 + t) * h * d0 + (-2.0 * t3 + 3.0 * t2) * y1 + (t3 - t2) * h * d1
}

/// Contrast (−100..100) on tone value `p`: an S through black, middle grey and white, steeper
/// through the grey for more; straight on past white and below black with its end slope.
pub fn contrast(p: f64, amount: f64) -> f64 {
    let c = amount / 100.0;
    if c == 0.0 {
        return p;
    }
    let m = grey();
    let (mid, end) = (1.0 + 0.7 * c, 1.0 - 0.45 * c);
    if p <= 0.0 {
        p * end
    } else if p < m {
        hermite(0.0, m, 0.0, m, end, mid, p)
    } else if p < 1.0 {
        hermite(m, 1.0, m, 1.0, mid, end, p)
    } else {
        1.0 + (p - 1.0) * end
    }
}

/// Where Highlights starts working (tone).
const HIGHLIGHTS_FROM: f64 = 0.5;

/// Highlights (−100..100): below 0 a soft squeeze of everything above the middle, light above
/// white included (−100 brings a linear 3.0 under white); above 0 the bright part is lifted and
/// carries on straight past white.
pub fn highlights(p: f64, amount: f64) -> f64 {
    let h = amount / 100.0;
    let k = HIGHLIGHTS_FROM;
    if h == 0.0 || p <= k {
        return p;
    }
    let x = p - k;
    if h < 0.0 {
        return k + x / (1.0 + 1.3 * -h * x);
    }
    let b = 0.8 * h;
    if p <= 1.0 {
        return k + x * (1.0 + b * x);
    }
    let top = 1.0 - k;
    k + top * (1.0 + b * top) + (p - 1.0) * (1.0 + 2.0 * b * top)
}

/// Where Shadows stops working (tone).
const SHADOWS_TO: f64 = 0.6;

/// Shadows (−100..100): lifts or darkens the dark part, most around a fifth of the way up.
pub fn shadows(p: f64, amount: f64) -> f64 {
    let s = amount / 100.0;
    let k = SHADOWS_TO;
    if s == 0.0 || p >= k {
        return p;
    }
    let a = if s > 0.0 { 1.7 } else { 0.9 };
    if p <= 0.0 {
        return p * (1.0 + s * a);
    }
    let w = 1.0 - p / k;
    p + s * a * p * w * w
}

/// Where Whites starts working (tone), and how far it eases in.
const WHITES_FROM: f64 = 0.6;
const WHITES_EASE: f64 = 0.4;

/// Whites (−100..100): a gain on the top end that eases in from 0.6 and carries on past white,
/// so −100 brings white to about 0.78 and every glow above it down with it.
pub fn whites(p: f64, amount: f64) -> f64 {
    let w = amount / 100.0;
    let k = WHITES_FROM;
    if w == 0.0 || p <= k {
        return p;
    }
    let x = p - k;
    let t = (x / WHITES_EASE).min(1.0);
    let r = t * t * (3.0 - 2.0 * t);
    k + x * (1.0 + 0.45 * w * r)
}

/// Where Blacks stops working (tone).
const BLACKS_TO: f64 = 0.4;

/// Blacks (−100..100): lifts black (above 0) or crushes the darkest part below black (below 0,
/// clamped as black at the end).
pub fn blacks(p: f64, amount: f64) -> f64 {
    let b = amount / 100.0;
    let k = BLACKS_TO;
    if b == 0.0 || p >= k {
        return p;
    }
    let lift = b * if b > 0.0 { 0.08 } else { 0.10 };
    if p <= 0.0 {
        return lift + p * (1.0 - 2.0 * lift / k);
    }
    let w = 1.0 - p / k;
    p + lift * w * w
}

/// The Basic panel's tone sliders on tone value `p`, in Lightroom's order.
pub fn basic_tone(p: f64, b: &GradeBasic) -> f64 {
    let p = contrast(p, b.contrast as f64);
    let p = highlights(p, b.highlights as f64);
    let p = shadows(p, b.shadows as f64);
    let p = whites(p, b.whites as f64);
    blacks(p, b.blacks as f64)
}

/// A monotone cubic (Fritsch–Carlson, as PCHIP) through points in order of x, carried on straight
/// past its ends with its end slopes.
#[derive(Clone, Debug, PartialEq)]
pub struct Pchip {
    xs: Vec<f64>,
    ys: Vec<f64>,
    ds: Vec<f64>,
}

impl Pchip {
    /// Through `points` (x rising; at the same x the later point wins). Fewer than two points is
    /// the straight line.
    pub fn new(points: &[(f64, f64)]) -> Self {
        let mut xs: Vec<f64> = Vec::with_capacity(points.len());
        let mut ys: Vec<f64> = Vec::with_capacity(points.len());
        for &(x, y) in points {
            if xs.last().is_some_and(|&last| x <= last) {
                *ys.last_mut().expect("one there") = y;
                continue;
            }
            xs.push(x);
            ys.push(y);
        }
        if xs.len() < 2 {
            return Self { xs: vec![0.0, 1.0], ys: vec![0.0, 1.0], ds: vec![1.0, 1.0] };
        }
        let n = xs.len();
        let h: Vec<f64> = (0..n - 1).map(|i| xs[i + 1] - xs[i]).collect();
        let d: Vec<f64> = (0..n - 1).map(|i| (ys[i + 1] - ys[i]) / h[i]).collect();
        let mut ds = vec![0.0; n];
        if n == 2 {
            ds = vec![d[0], d[0]];
        } else {
            for i in 1..n - 1 {
                if d[i - 1] * d[i] > 0.0 {
                    let (w1, w2) = (2.0 * h[i] + h[i - 1], h[i] + 2.0 * h[i - 1]);
                    ds[i] = (w1 + w2) / (w1 / d[i - 1] + w2 / d[i]);
                }
            }
            let end = |h0: f64, h1: f64, d0: f64, d1: f64| {
                let s = ((2.0 * h0 + h1) * d0 - h0 * d1) / (h0 + h1);
                if s.signum() != d0.signum() || d0 == 0.0 {
                    0.0
                } else if d0.signum() != d1.signum() && s.abs() > (3.0 * d0).abs() {
                    3.0 * d0
                } else {
                    s
                }
            };
            ds[0] = end(h[0], h[1], d[0], d[1]);
            ds[n - 1] = end(h[n - 2], h[n - 3], d[n - 2], d[n - 3]);
        }
        Self { xs, ys, ds }
    }

    /// The curve at `x`.
    pub fn at(&self, x: f64) -> f64 {
        let n = self.xs.len();
        if x <= self.xs[0] {
            return self.ys[0] + (x - self.xs[0]) * self.ds[0];
        }
        if x >= self.xs[n - 1] {
            return self.ys[n - 1] + (x - self.xs[n - 1]) * self.ds[n - 1];
        }
        let i = self.xs.partition_point(|&xi| xi <= x).clamp(1, n - 1) - 1;
        hermite(self.xs[i], self.xs[i + 1], self.ys[i], self.ys[i + 1], self.ds[i], self.ds[i + 1], x)
    }
}

/// A point curve as a [`Pchip`]: the straight line unless `points` is a usable curve
/// ([`bpe_core::curve_is_valid`]). Bends are not used: a tone curve is smooth through its points.
pub fn point_curve(points: &[CurvePoint]) -> Pchip {
    if !bpe_core::curve_is_valid(points) {
        return Pchip::new(&[]);
    }
    let points: Vec<(f64, f64)> = points.iter().map(|p| (p.x as f64, p.y as f64)).collect();
    Pchip::new(&points)
}

/// The three splits (0..100) as places 0..1 in order, inside 0.05..0.95 and at least 0.02 apart.
pub fn splits(raw: [f32; 3]) -> [f64; 3] {
    let mut s = raw.map(|v| if v.is_finite() { v as f64 / 100.0 } else { 0.5 });
    s.sort_by(f64::total_cmp);
    const GAP: f64 = 0.02;
    s[0] = s[0].clamp(0.05, 0.95 - 2.0 * GAP);
    s[1] = s[1].clamp(s[0] + GAP, 0.95 - GAP);
    s[2] = s[2].clamp(s[1] + GAP, 0.95);
    s
}

/// The parametric curve: a node in the middle of each region (Shadows, Darks, Lights,
/// Highlights between the splits) moved up or down by up to 0.45 of its region's width, through
/// black and white, as a monotone cubic.
pub fn parametric(t: &GradeToneCurve) -> Pchip {
    let [a, b, c] = splits(t.splits);
    let bounds = [0.0, a, b, c, 1.0];
    let amounts = [t.shadows, t.darks, t.lights, t.highlights];
    let mut nodes = vec![(0.0, 0.0)];
    for (i, amount) in amounts.iter().enumerate() {
        let (lo, hi) = (bounds[i], bounds[i + 1]);
        let x = 0.5 * (lo + hi);
        nodes.push((x, x + (*amount as f64 / 100.0).clamp(-1.0, 1.0) * 0.45 * (hi - lo)));
    }
    nodes.push((1.0, 1.0));
    Pchip::new(&nodes)
}

/// The Tone Curve panel on tone value `p`, per channel: parametric, then all channels, then each
/// channel's own.
pub struct ToneCurves {
    parametric: Pchip,
    rgb: Pchip,
    channels: [Pchip; 3],
}

impl ToneCurves {
    pub fn new(t: &GradeToneCurve) -> Self {
        Self {
            parametric: parametric(t),
            rgb: point_curve(&t.rgb),
            channels: [point_curve(&t.red), point_curve(&t.green), point_curve(&t.blue)],
        }
    }

    pub fn at(&self, p: f64) -> [f64; 3] {
        let q = self.rgb.at(self.parametric.at(p));
        self.channels.each_ref().map(|c| c.at(q))
    }
}

type Mat3 = [[f64; 3]; 3];

/// Linear sRGB to CIE XYZ (D65).
const RGB_TO_XYZ: Mat3 = [
    [0.412_456_4, 0.357_576_1, 0.180_437_5],
    [0.212_672_9, 0.715_152_2, 0.072_175_0],
    [0.019_333_9, 0.119_192_0, 0.950_304_1],
];

/// XYZ to the Bradford cone responses.
const BRADFORD: Mat3 = [[0.8951, 0.2664, -0.1614], [-0.7502, 1.7135, 0.0367], [0.0389, -0.0685, 1.0296]];

fn mul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut m = [[0.0; 3]; 3];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

fn apply(m: &Mat3, v: [f64; 3]) -> [f64; 3] {
    m.map(|row| row[0] * v[0] + row[1] * v[1] + row[2] * v[2])
}

fn inverse(m: &Mat3) -> Mat3 {
    let [[a, b, c], [d, e, f], [g, h, i]] = *m;
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    [
        [(e * i - f * h) / det, (c * h - b * i) / det, (b * f - c * e) / det],
        [(f * g - d * i) / det, (a * i - c * g) / det, (c * d - a * f) / det],
        [(d * h - e * g) / det, (b * g - a * h) / det, (a * e - b * d) / det],
    ]
}

/// The Planckian locus at `kelvin` (1667..25000) as CIE 1931 xy (Kim et al. 2002).
fn locus_xy(kelvin: f64) -> [f64; 2] {
    let t = kelvin.clamp(1667.0, 25000.0);
    let (t1, t2, t3) = (1e3 / t, 1e6 / (t * t), 1e9 / (t * t * t));
    let x = if t <= 4000.0 {
        -0.266_123_9 * t3 - 0.234_358_9 * t2 + 0.877_695_6 * t1 + 0.179_910
    } else {
        -3.025_846_9 * t3 + 2.107_037_9 * t2 + 0.222_634_7 * t1 + 0.240_390
    };
    let (x2, x3) = (x * x, x * x * x);
    let y = if t <= 2222.0 {
        -1.106_381_4 * x3 - 1.348_110_20 * x2 + 2.185_558_32 * x - 0.202_196_83
    } else if t <= 4000.0 {
        -0.954_947_6 * x3 - 1.374_185_93 * x2 + 2.091_370_15 * x - 0.167_488_67
    } else {
        3.081_758_0 * x3 - 5.873_386_70 * x2 + 3.751_129_97 * x - 0.370_014_83
    };
    [x, y]
}

fn xy_to_uv([x, y]: [f64; 2]) -> [f64; 2] {
    let d = -2.0 * x + 12.0 * y + 3.0;
    [4.0 * x / d, 6.0 * y / d]
}

fn uv_to_xy([u, v]: [f64; 2]) -> [f64; 2] {
    let d = 2.0 * u - 8.0 * v + 4.0;
    [3.0 * u / d, 2.0 * v / d]
}

fn xyz_to_xy([x, y, z]: [f64; 3]) -> [f64; 2] {
    let s = x + y + z;
    [x / s, y / s]
}

/// Mireds of the picture's own white (D65 sits near 6504 K).
const D65_MIRED: f64 = 1e6 / 6504.0;

/// The white Temp and Tint (−100..100) adapt the picture's white to, as XYZ with Y = 1: along the
/// Planckian locus from D65 in mireds (+100 is 130 mired warmer, about 3500 K; −100 is 100 mired
/// cooler, about 18,600 K), then off it by up to 0.03 Duv (+ towards magenta, − towards green).
pub fn target_white(temp: f64, tint: f64) -> [f64; 3] {
    let white = apply(&RGB_TO_XYZ, [1.0; 3]);
    let base = xy_to_uv(xyz_to_xy(white));
    let t = (temp / 100.0).clamp(-1.0, 1.0);
    let mired = D65_MIRED + t * if t > 0.0 { 130.0 } else { 100.0 };
    let kelvin = 1e6 / mired;
    let at = |k: f64| xy_to_uv(locus_xy(k));
    let (now, from) = (at(kelvin), at(6504.0));
    // Off the locus: the side with the greater v (above it) is greener.
    let (a, b) = (at(kelvin * 1.01), at(kelvin / 1.01));
    let along = [a[0] - b[0], a[1] - b[1]];
    let len = (along[0] * along[0] + along[1] * along[1]).sqrt().max(1e-12);
    let mut green = [-along[1] / len, along[0] / len];
    if green[1] < 0.0 {
        green = [-green[0], -green[1]];
    }
    let duv = -(tint / 100.0).clamp(-1.0, 1.0) * 0.03;
    let uv = [base[0] + now[0] - from[0] + duv * green[0], base[1] + now[1] - from[1] + duv * green[1]];
    let [x, y] = uv_to_xy(uv);
    [x / y, 1.0, (1.0 - x - y) / y]
}

/// The linear-light matrix of Temp, Tint (Bradford adaptation from the picture's white to
/// [`target_white`], so a grey keeps its luminance) and Exposure (× 2^stops). Rows of linear sRGB.
pub fn basic_matrix(b: &GradeBasic) -> [[f64; 3]; 3] {
    let gain = 2f64.powf((b.exposure as f64).clamp(-10.0, 10.0));
    let mut m = if b.temp == 0.0 && b.tint == 0.0 {
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
    } else {
        let src = apply(&BRADFORD, apply(&RGB_TO_XYZ, [1.0; 3]));
        let dst = apply(&BRADFORD, target_white(b.temp as f64, b.tint as f64));
        let scale = [[dst[0] / src[0], 0.0, 0.0], [0.0, dst[1] / src[1], 0.0], [0.0, 0.0, dst[2] / src[2]]];
        let adapt = mul(&inverse(&BRADFORD), &mul(&scale, &BRADFORD));
        mul(&inverse(&RGB_TO_XYZ), &mul(&adapt, &RGB_TO_XYZ))
    };
    for row in &mut m {
        for v in row.iter_mut() {
            *v *= gain;
        }
    }
    m
}

/// Applies a matrix from [`basic_matrix`] to a linear colour (for tests and the editor).
pub fn apply_matrix(m: &[[f64; 3]; 3], c: [f64; 3]) -> [f64; 3] {
    apply(m, c)
}

/// Linear sRGB to Oklab (as the shader does it).
pub fn oklab(c: [f64; 3]) -> [f64; 3] {
    let l = 0.412_221_470_8 * c[0] + 0.536_332_536_3 * c[1] + 0.051_445_992_9 * c[2];
    let m = 0.211_903_498_2 * c[0] + 0.680_699_545_1 * c[1] + 0.107_396_956_6 * c[2];
    let s = 0.088_302_461_9 * c[0] + 0.281_718_837_6 * c[1] + 0.629_978_700_5 * c[2];
    let (l, m, s) = (l.cbrt(), m.cbrt(), s.cbrt());
    [
        0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s,
    ]
}

/// Oklab to linear sRGB.
pub fn unoklab(lab: [f64; 3]) -> [f64; 3] {
    let l = lab[0] + 0.396_337_777_4 * lab[1] + 0.215_803_757_3 * lab[2];
    let m = lab[0] - 0.105_561_345_8 * lab[1] - 0.063_854_172_8 * lab[2];
    let s = lab[0] - 0.089_484_177_5 * lab[1] - 1.291_485_548_0 * lab[2];
    let (l, m, s) = (l * l * l, m * m * m, s * s * s);
    [
        4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s,
        -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s,
        -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701_0 * s,
    ]
}

/// The Oklab hue (radians) skin sits near, which Vibrance leaves half alone.
pub const SKIN_HUE: f64 = 55.0 * std::f64::consts::PI / 180.0;

/// How much Vibrance (−1..1) and Saturation (−1..1) scale an Oklab colour's chroma: Vibrance
/// moves dull colours most and skin half as much; Saturation scales every colour alike.
pub fn chroma_scale(lab: [f64; 3], vibrance: f64, saturation: f64) -> f64 {
    let c = (lab[1] * lab[1] + lab[2] * lab[2]).sqrt();
    let dull = (1.0 - (c / 0.25).powi(2)).clamp(0.0, 1.0);
    let hue = lab[2].atan2(lab[1]);
    let off = (hue - SKIN_HUE + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
    let skin = (-(off / 0.35).powi(2)).exp();
    (1.0 + vibrance * dull * (1.0 - 0.5 * skin)) * (1.0 + saturation)
}

// ---------------------------------------------------------------------------------------------
// Colour (part 2): HSL / Colour, Point Colour, Colour Grading, Calibration.

use crate::frame::{GradeCalibration, GradeColourGrading, GradeEffects, GradeHsl, GradePick};
use std::f64::consts::{PI, TAU};

/// Hermite smoothstep from `a` to `b` (0 below, 1 above).
pub fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// An sRGB-encoded channel (0..1) as linear light.
fn srgb_to_linear(v: f64) -> f64 {
    untone(v.clamp(0.0, 1.0))
}

/// The linear colour at HSV hue `degrees` with full saturation and value (Lightroom's hues: 0
/// red, 120 green, 240 blue).
pub fn hsv_colour(degrees: f64) -> [f64; 3] {
    let h = degrees.rem_euclid(360.0) / 60.0;
    let x = 1.0 - ((h % 2.0) - 1.0).abs();
    let (r, g, b) = match h as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    };
    [srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b)]
}

/// An Oklab colour's lightness, chroma and hue (radians, 0..τ; 0 for a grey).
pub fn lch(lab: [f64; 3]) -> [f64; 3] {
    let c = (lab[1] * lab[1] + lab[2] * lab[2]).sqrt();
    let h = if c < 1e-7 { 0.0 } else { lab[2].atan2(lab[1]).rem_euclid(TAU) };
    [lab[0], c, h]
}

/// Lightness, chroma and hue back to Oklab.
pub fn from_lch([l, c, h]: [f64; 3]) -> [f64; 3] {
    [l, c * h.cos(), c * h.sin()]
}

/// The Oklab hue (radians) of the colour at HSV hue `degrees`.
pub fn oklab_hue_of(degrees: f64) -> f64 {
    lch(oklab(hsv_colour(degrees)))[2]
}

/// The direction in Oklab's (a, b) of the colour at HSV hue `degrees`, of length 1.
pub fn hsv_hue_direction(degrees: f64) -> [f64; 2] {
    let h = oklab_hue_of(degrees);
    [h.cos(), h.sin()]
}

/// Lightroom's HSL bands (red, orange, yellow, green, aqua, blue, purple, magenta) as HSV hues.
pub const BAND_HSV_HUES: [f64; 8] = [0.0, 30.0, 60.0, 120.0, 180.0, 240.0, 270.0, 300.0];

/// The bands' names, as the panel shows them.
pub const BAND_NAMES: [&str; 8] = ["Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple", "Magenta"];

/// The bands' centres as Oklab hues (radians, rising: red ≈ 29°, magenta ≈ 328°).
pub fn band_hues() -> [f64; 8] {
    static HUES: std::sync::OnceLock<[f64; 8]> = std::sync::OnceLock::new();
    *HUES.get_or_init(|| BAND_HSV_HUES.map(oklab_hue_of))
}

/// How far (as a share of the way to the next band) a band keeps a hue to itself before it starts
/// to share it: a colour near a band's centre is moved by that band in full.
pub const BAND_FLAT: f64 = 0.15;

/// How much each band owns Oklab hue `h` (radians): the two bands either side share it by a
/// smoothstep (each band keeps the hues nearest it to itself), so the weights add to 1 and a band's
/// centre is its alone.
pub fn band_weights(h: f64) -> [f64; 8] {
    let centres = band_hues();
    let h = h.rem_euclid(TAU);
    let mut w = [0.0; 8];
    for i in 0..8 {
        let (a, j) = (centres[i], (i + 1) % 8);
        let gap = (centres[j] - a).rem_euclid(TAU);
        let along = (h - a).rem_euclid(TAU);
        if along < gap {
            let s = smoothstep(BAND_FLAT, 1.0 - BAND_FLAT, along / gap);
            w[i] = 1.0 - s;
            w[j] = s;
            return w;
        }
    }
    w[0] = 1.0;
    w
}

/// How far (radians) band `i`'s Hue slider at ±100 turns it: 0.75 of the way to the next band
/// (+) or the one before (−).
pub fn band_turn(i: usize, hue: f64) -> f64 {
    let centres = band_hues();
    let t = (hue / 100.0).clamp(-1.0, 1.0);
    let gap = if t >= 0.0 {
        (centres[(i + 1) % 8] - centres[i]).rem_euclid(TAU)
    } else {
        (centres[i] - centres[(i + 7) % 8]).rem_euclid(TAU)
    };
    0.75 * gap * t
}

/// The HSL panel as a lookup row over the Oklab hue: [`super::HUE_SAMPLES`] samples (sample i at
/// i°), each `[hue turn (radians), saturation −1..1, luminance −1..1, 0]`.
pub fn hsl_row(p: &GradeHsl) -> Vec<[f64; 4]> {
    let n = |v: f32| if v.is_finite() { v as f64 } else { 0.0 };
    let turns: [f64; 8] = std::array::from_fn(|i| band_turn(i, n(p.hue[i])));
    (0..super::HUE_SAMPLES)
        .map(|i| {
            let w = band_weights((i as f64).to_radians());
            let sum = |f: &dyn Fn(usize) -> f64| (0..8).map(|b| w[b] * f(b)).sum::<f64>();
            [
                sum(&|b| turns[b]),
                sum(&|b| (n(p.saturation[b]) / 100.0).clamp(-1.0, 1.0)),
                sum(&|b| (n(p.luminance[b]) / 100.0).clamp(-1.0, 1.0)),
                0.0,
            ]
        })
        .collect()
}

/// The row at Oklab hue `h` (radians), between samples, wrapping round.
pub fn hsl_sample(row: &[[f64; 4]], h: f64) -> [f64; 4] {
    let n = row.len();
    let t = h.rem_euclid(TAU) / TAU * n as f64;
    let i = (t.floor() as usize).min(n - 1);
    let (a, b, f) = (row[i], row[(i + 1) % n], t - i as f64);
    std::array::from_fn(|k| a[k] + (b[k] - a[k]) * f)
}

/// How much a colour of chroma `c` takes a luminance move (none for a grey).
pub fn chroma_weight(c: f64) -> f64 {
    (c / 0.08).clamp(0.0, 1.0)
}

/// The HSL panel on an Oklab colour (as the shader does it).
pub fn hsl(lab: [f64; 3], row: &[[f64; 4]]) -> [f64; 3] {
    let [l, c, h] = lch(lab);
    if c < 1e-7 {
        return lab;
    }
    let [turn, s, lum, _] = hsl_sample(row, h);
    from_lch([l * (0.8 * lum * chroma_weight(c)).exp2(), c * (1.0 + s).max(0.0), h + turn])
}

/// The most a Point Colour pick's Hue turns colours (radians).
pub const PICK_TURN: f64 = 40.0 * PI / 180.0;

/// A Point Colour pick as the shader reads it: where it is in Oklab, its shifts (hue turn in
/// radians, saturation and luminance −1..1) and one over its widths in lightness, chroma, hue.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PickWords {
    pub lab: [f64; 3],
    pub shift: [f64; 3],
    pub inv: [f64; 3],
}

/// Pick `p` as words, or `None` when it shifts nothing.
pub fn pick_words(p: &GradePick) -> Option<PickWords> {
    if p.is_identity() {
        return None;
    }
    let n = |v: f32| if v.is_finite() { (v as f64 / 100.0).clamp(-1.0, 1.0) } else { 0.0 };
    let r = if p.range.is_finite() { (p.range as f64 / 100.0).clamp(0.0, 1.0) } else { 0.5 };
    let colour = p.colour.map(|v| if v.is_finite() { v.max(0.0) as f64 } else { 0.0 });
    Some(PickWords {
        lab: oklab(colour),
        shift: [n(p.hue) * PICK_TURN, n(p.saturation), n(p.luminance)],
        inv: [1.0 / (0.08 + 0.32 * r), 1.0 / (0.04 + 0.16 * r), 1.0 / (0.03 + 0.14 * r)],
    })
}

/// The composite's roll-off (`shaders/post.wgsl::roll_off`): a colour past white squeezed back to
/// what the screen shows.
pub fn roll_off(c: [f64; 3]) -> [f64; 3] {
    let peak = c[0].max(c[1]).max(c[2]);
    if peak <= 1.0 {
        return c;
    }
    let white = c[0].min(c[1]).min(c[2]).clamp(0.0, 1.0);
    let over = (luminance(c) - 1.0).max(0.0);
    let whiter = (white * white).max(over / (over + 0.5)) * ((peak - 1.0) * 4.0).min(1.0);
    c.map(|v| v / peak + (1.0 - v / peak) * whiter)
}

/// An Oklab colour past white as the screen shows it (through [`roll_off`]): a glow is matched
/// by the colour the eyedropper picks from the finished picture, not by how far above white it is.
pub fn tamed(lab: [f64; 3]) -> [f64; 3] {
    let lin = unoklab(lab);
    if lin[0].max(lin[1]).max(lin[2]) > 1.0 { oklab(roll_off(lin)) } else { lab }
}

/// How much pick `p` takes the Oklab colour `lab` (1 on the pick, 0 far from it).
pub fn pick_weight(lab: [f64; 3], p: &PickWords) -> f64 {
    let [l, c, h] = lch(tamed(lab));
    let [lp, cp, hp] = lch(p.lab);
    let dh = 2.0 * (c * cp).sqrt() * ((h - hp) / 2.0).sin();
    let d = (((l - lp) * p.inv[0]).powi(2) + ((c - cp) * p.inv[1]).powi(2) + (dh * p.inv[2]).powi(2)).sqrt();
    1.0 - smoothstep(0.35, 1.0, d)
}

/// The Point Colour panel on an Oklab colour: every pick's weight from the colour as it came,
/// then each pick's shifts in turn.
pub fn point_colour(lab: [f64; 3], picks: &[PickWords]) -> [f64; 3] {
    let weights: Vec<f64> = picks.iter().map(|p| pick_weight(lab, p)).collect();
    let mut out = lab;
    for (p, w) in picks.iter().zip(weights) {
        if w <= 0.0 {
            continue;
        }
        let [l, c, h] = lch(out);
        out = from_lch([l * (0.8 * p.shift[2] * w).exp2(), c * (1.0 + p.shift[1] * w).max(0.0), h + p.shift[0] * w]);
    }
    out
}

/// Oklab lightness of 18% grey: where Balance 0 puts the middle of the tones.
pub fn grey_lightness() -> f64 {
    oklab([0.18; 3])[0]
}

/// Colour Grading as the shader reads it: `head` is `[pivot, overlap, global log2 gain, 0]`,
/// then one `[Δa, Δb, ΔL, 0]` for shadows, midtones, highlights and global.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WheelsWords {
    pub head: [f64; 4],
    pub regions: [[f64; 4]; 4],
}

/// The panel's words.
pub fn wheels_words(g: &GradeColourGrading) -> WheelsWords {
    let n = |v: f32, lo: f64, hi: f64| if v.is_finite() { (v as f64).clamp(lo, hi) } else { 0.0 };
    let balance = n(g.balance, -100.0, 100.0) / 100.0;
    let blending = if g.blending.is_finite() { (g.blending as f64).clamp(0.0, 100.0) / 100.0 } else { 0.5 };
    let pivot = (grey_lightness() - 0.3 * balance).clamp(0.15, 0.85);
    let region = |w: &crate::frame::GradeWheel, lum_scale: f64| {
        let [da, db] = hsv_hue_direction(n(w.hue, -1e6, 1e6));
        let s = n(w.saturation, 0.0, 100.0) / 100.0 * 0.10;
        [da * s, db * s, n(w.luminance, -100.0, 100.0) / 100.0 * lum_scale, 0.0]
    };
    let mut global = region(&g.global, 0.0);
    global[2] = 0.0;
    WheelsWords {
        head: [pivot, 0.02 + 0.313 * blending, n(g.global.luminance, -100.0, 100.0) / 100.0 * 0.5, 0.0],
        regions: [region(&g.shadows, 0.2), region(&g.midtones, 0.2), region(&g.highlights, 0.2), global],
    }
}

/// The shadows, midtones and highlights weights of Oklab lightness `l` (they add to 1).
pub fn wheel_weights(l: f64, pivot: f64, overlap: f64) -> [f64; 3] {
    let x0 = l.clamp(0.0, 1.0);
    let x = if x0 < pivot { 0.5 * x0 / pivot } else { 0.5 + 0.5 * (x0 - pivot) / (1.0 - pivot) };
    let s = 1.0 - smoothstep(1.0 / 3.0 - overlap, 1.0 / 3.0 + overlap, x);
    let h = smoothstep(2.0 / 3.0 - overlap, 2.0 / 3.0 + overlap, x);
    [s, (1.0 - s - h).max(0.0), h]
}

/// Colour Grading on an Oklab colour (as the shader does it).
pub fn colour_wheels(lab: [f64; 3], w: &WheelsWords) -> [f64; 3] {
    let l = lab[0];
    let weights = wheel_weights(l, w.head[0], w.head[1]);
    let tint = (l / 0.25).clamp(0.0, 1.0);
    let mut out = lab;
    let mut lift = 0.0;
    for (r, region) in w.regions.iter().enumerate() {
        let k = if r < 3 { weights[r] } else { 1.0 };
        out[1] += region[0] * k * tint;
        out[2] += region[1] * k * tint;
        lift += region[2] * k;
    }
    out[0] = ((l + lift) * w.head[2].exp2()).max(0.0);
    out
}

/// The Calibration panel's primaries as a linear-light matrix (rows, like [`basic_matrix`]):
/// each primary turned by its Hue (±30°) and its chroma scaled by its Saturation, then the columns
/// scaled so white, and so every grey, stays exactly as it was.
pub fn calibration_matrix(c: &GradeCalibration) -> [[f64; 3]; 3] {
    let n = |v: f32| if v.is_finite() { (v as f64 / 100.0).clamp(-1.0, 1.0) } else { 0.0 };
    let moves = [(c.red_hue, c.red_saturation), (c.green_hue, c.green_saturation), (c.blue_hue, c.blue_saturation)];
    let mut p: Mat3 = [[0.0; 3]; 3];
    for (i, (hue, sat)) in moves.iter().enumerate() {
        let mut e = [0.0; 3];
        e[i] = 1.0;
        let [l, ch, h] = lch(oklab(e));
        let moved = unoklab(from_lch([l, ch * (1.0 + 0.6 * n(*sat)).max(0.0), h + n(*hue) * 30f64.to_radians()]));
        for (row, v) in p.iter_mut().zip(moved) {
            row[i] = v;
        }
    }
    let k = apply(&inverse(&p), [1.0; 3]);
    p.map(|row| [row[0] * k[0], row[1] * k[1], row[2] * k[2]])
}

/// Rec. 709 luminance of a linear colour.
pub fn luminance(c: [f64; 3]) -> f64 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// The gain Shadows Tint (−100 green .. 100 magenta) puts on the darks, at the same luminance.
pub fn dark_tint_gain(tint: f32) -> [f64; 3] {
    let t = if tint.is_finite() { (tint as f64 / 100.0).clamp(-1.0, 1.0) } else { 0.0 };
    let raw = [1.0 + 0.25 * t, 1.0 - 0.35 * t, 1.0 + 0.25 * t];
    let y = luminance(raw);
    raw.map(|v| v / y)
}

/// Shadows Tint on a linear colour: the gain in full on black's side, gone by 18% luminance.
pub fn dark_tint(c: [f64; 3], gain: [f64; 3]) -> [f64; 3] {
    let w = 1.0 - smoothstep(0.0, 0.18, luminance(c));
    std::array::from_fn(|i| c[i] * (1.0 + (gain[i] - 1.0) * w))
}

// ---- Part 3: area steps and effects (the per-pixel parts; `shaders/grade.wgsl` twins) ----

/// Lift's softening near black: a change of tone luma keeps a bright colour's channel ratios and
/// becomes an even shift near black (so black can still be lifted).
pub const LIFT_SOFT: f64 = 0.05;

/// `c` (tone space) with its tone luma moved by `delta`.
pub fn lift(c: [f64; 3], delta: f64) -> [f64; 3] {
    let y = luminance(c);
    let ratio = (y + delta + LIFT_SOFT).max(0.0) / (y + LIFT_SOFT).max(1e-4);
    c.map(|v| (v + LIFT_SOFT) * ratio - LIFT_SOFT)
}

/// `d` softly held under `limit` (odd, rising, never past ±limit): how area steps keep halos down.
pub fn soft_limit(d: f64, limit: f64) -> f64 {
    limit * (d / limit).tanh()
}

/// Sharpening's halo limit for Detail (0..100): small at 0, so big edges get little more.
pub fn sharpen_limit(detail: f64) -> f64 {
    let t = (detail / 100.0).clamp(0.0, 1.0);
    0.02 + 0.5 * t * t
}

/// Clarity's weight at tone luma `y`: the midtones most, the ends and past white a quarter.
pub fn clarity_weight(y: f64) -> f64 {
    let x = 2.0 * y - 1.0;
    (1.0 - 0.75 * x * x).clamp(0.25, 1.0)
}

/// The most a veil of haze may be (linear), and how much of it Dehaze +100 takes off.
pub const VEIL_MOST: f64 = 0.8;
pub const VEIL_TAKEN: f64 = 0.75;

/// Dehaze `d` (−1..1) on linear `c`, given the pass input's own least channel (`own`), the dark
/// channel round it (`local_dark`) and the brightness round it (`local_luma`), all linear.
pub fn dehaze(c: [f64; 3], own: f64, local_dark: f64, local_luma: f64, d: f64) -> [f64; 3] {
    if d >= 0.0 {
        let veil = own.min(local_dark).clamp(0.0, VEIL_MOST);
        let v = VEIL_TAKEN * d * veil;
        c.map(|x| (x - v).max(0.0) / (1.0 - v))
    } else {
        let fog = 0.55 + 0.45 * local_luma.clamp(0.0, 1.0);
        let h = 0.6 * -d;
        c.map(|x| x + (fog - x) * h)
    }
}

/// The vignette's words: `[amount, middle, width, roundness]` (amount and roundness −1..1).
pub fn vignette_words(e: &GradeEffects) -> [f64; 4] {
    let f = |v: f32, lo: f64, hi: f64| if v.is_finite() { (v as f64 / 100.0).clamp(lo, hi) } else { 0.0 };
    [
        f(e.vignette_amount, -1.0, 1.0),
        0.35 + 0.9 * f(e.vignette_midpoint, 0.0, 1.0),
        0.02 + f(e.vignette_feather, 0.0, 1.0),
        f(e.vignette_roundness, -1.0, 1.0),
    ]
}

/// How much of the vignette lies at `uv` (0..1 across and down) on a picture `size` (any unit):
/// 0 inside it, 1 out where it is full.
pub fn vignette_weight(uv: [f64; 2], size: [f64; 2], w: [f64; 4]) -> f64 {
    let [_, mid, width, r] = w;
    let p = [(uv[0] - 0.5) * 2.0, (uv[1] - 0.5) * 2.0];
    let short = size[0].min(size[1]).max(1e-9);
    let circle = [p[0] * size[0] / short, p[1] * size[1] / short];
    let k = r.max(0.0);
    let q = [p[0] + (circle[0] - p[0]) * k, p[1] + (circle[1] - p[1]) * k];
    let n = 2.0 - 6.0 * r.min(0.0);
    let d = (q[0].abs().powf(n) + q[1].abs().powf(n)).powf(1.0 / n);
    smoothstep(mid - 0.5 * width, mid + 0.5 * width, d)
}

/// The vignette on linear `c` at weight `v`, with Highlights `h` (0..1) and the linear brightness
/// round it.
pub fn vignette(c: [f64; 3], v: f64, amount: f64, h: f64, local_luma: f64) -> [f64; 3] {
    if amount < 0.0 {
        let keep = h * smoothstep(0.3, 1.0, local_luma);
        let gain = (1.0 + amount * v * (1.0 - keep)).max(0.0);
        c.map(|x| x * gain)
    } else {
        let t = amount * v;
        // Past white stays where it is.
        c.map(|x| untone(tone(x) + (1.0 - tone(x)).max(0.0) * t))
    }
}

/// The gradient map's row: [`MAP_SAMPLES`] colours of `e.map` in tone space, the first at 0.
pub fn gradient_row(e: &GradeEffects) -> Vec<[f64; 3]> {
    e.map.bake(super::MAP_SAMPLES).into_iter().map(|s| [0, 1, 2].map(|i| tone(s[i] as f64))).collect()
}

/// The gradient map on tone-space `c` with its row, by `amount` (0..1).
pub fn gradient_map(c: [f64; 3], row: &[[f64; 3]], amount: f64) -> [f64; 3] {
    let y = luminance(c);
    let t = y.clamp(0.0, 1.0) * (row.len() - 1) as f64;
    let i = (t as usize).min(row.len() - 2);
    let f = t - i as f64;
    let over = y.max(1.0);
    let m = [0, 1, 2].map(|k| (row[i][k] + (row[i + 1][k] - row[i][k]) * f) * over);
    [0, 1, 2].map(|k| c[k] + (m[k] - c[k]) * amount)
}

// ---- Part 4: masks (the coverage of a mask's parts; `shaders/grade.wgsl` twins) ----

use crate::frame::{
    GradeArea, GradeColourRange, GradeCombine, GradeLinear, GradeLuminanceRange, GradePlace, GradeRadial, GradeShape,
};

fn finite_or(v: f32, or: f32) -> f64 {
    if v.is_finite() { v as f64 } else { or as f64 }
}

/// A Linear Gradient's numbers as the shader reads them: `[x, y, angle in radians, feather]`, a
/// number that isn't one taken as its default and the feather at least 1.
pub fn linear_numbers(l: &GradeLinear) -> [f64; 4] {
    [
        finite_or(l.x, 0.0),
        finite_or(l.y, 0.0),
        finite_or(l.angle, 90.0).to_radians(),
        finite_or(l.feather, 400.0).max(1.0),
    ]
}

/// A Radial Gradient's numbers as the shader reads them: `[x, y, half width, half height]` and
/// `[angle in radians, roundness −1..1, feather 0..1, 0]`.
pub fn radial_numbers(r: &GradeRadial) -> [[f64; 4]; 2] {
    [
        [
            finite_or(r.x, 0.0),
            finite_or(r.y, 0.0),
            0.5 * finite_or(r.width, 500.0).max(1.0),
            0.5 * finite_or(r.height, 500.0).max(1.0),
        ],
        [
            finite_or(r.angle, 0.0).to_radians(),
            (finite_or(r.roundness, 0.0) / 100.0).clamp(-1.0, 1.0),
            (finite_or(r.feather, 50.0) / 100.0).clamp(0.0, 1.0),
            0.0,
        ],
    ]
}

/// Whether a place can be drawn at (finite, a size over 0).
pub fn place_ok(p: &GradePlace) -> bool {
    p.at.iter().all(|v| v.is_finite()) && p.turn.is_finite() && p.scale.is_finite() && p.scale > 0.0
}

/// Point `(x, y)` of a gradient at place `p`, on the screen.
fn placed(p: &GradePlace, x: f64, y: f64) -> [f64; 2] {
    let (s, c) = (p.turn as f64).sin_cos();
    let (x, y) = (x * p.scale as f64, y * p.scale as f64);
    [p.at[0] as f64 + x * c - y * s, p.at[1] as f64 + x * s + y * c]
}

/// One copy of a Linear Gradient at `place`, at screen point `p`; `px` is one output pixel in
/// screen units (the feather is never sharper).
pub fn linear_cover(n: [f64; 4], place: &GradePlace, p: [f64; 2], px: f64) -> f64 {
    let [x, y, angle, feather] = n;
    let c = placed(place, x, y);
    let (s, co) = (angle + place.turn as f64).sin_cos();
    let f = (feather * place.scale as f64).max(px);
    let t = ((p[0] - c[0]) * co + (p[1] - c[1]) * s) / f + 0.5;
    1.0 - smoothstep(0.0, 1.0, t)
}

/// One copy of a Radial Gradient at `place`, at screen point `p` (see [`linear_cover`]).
pub fn radial_cover(n: [[f64; 4]; 2], place: &GradePlace, p: [f64; 2], px: f64) -> f64 {
    let [[x, y, hw, hh], [angle, round, feather, _]] = n;
    let c = placed(place, x, y);
    let (s, co) = (angle + place.turn as f64).sin_cos();
    let d = [p[0] - c[0], p[1] - c[1]];
    let local = [d[0] * co + d[1] * s, -d[0] * s + d[1] * co];
    let scale = place.scale as f64;
    let half = [hw * scale, hh * scale];
    let short = half[0].min(half[1]);
    let r = round.max(0.0);
    let q = [0, 1].map(|i| local[i] / half[i] * (1.0 - r) + local[i] / short * r);
    if q[0].abs().max(q[1].abs()) > 1.0 {
        return 0.0;
    }
    let e = 2.0 - 6.0 * round.min(0.0);
    let dist = (q[0].abs().max(1e-7).powf(e) + q[1].abs().max(1e-7).powf(e)).powf(1.0 / e);
    let inner = (1.0 - feather).min(1.0 - px / short);
    1.0 - smoothstep(inner, 1.0, dist)
}

/// A Colour Range's widths for Refine (0..100): full coverage within `inner` of a pick (Oklab
/// distance, lightness counted half), none past `outer`.
pub fn range_widths(refine: f32) -> [f64; 2] {
    let t = (finite_or(refine, 50.0) / 100.0).clamp(0.0, 1.0);
    let outer = 0.03 + 0.25 * t.powf(1.3);
    [0.35 * outer, outer]
}

/// A Colour Range's picks in Oklab.
pub fn range_picks(r: &GradeColourRange) -> Vec<[f64; 3]> {
    r.picks.iter().filter(|c| c.iter().all(|v| v.is_finite())).map(|c| oklab(c.map(|v| (v as f64).max(0.0)))).collect()
}

/// How much a Colour Range covers the linear colour `c`: its nearest pick decides.
pub fn colour_range_cover(c: [f64; 3], picks: &[[f64; 3]], [inner, outer]: [f64; 2]) -> f64 {
    // Past white is matched as the screen shows it, as Point Colour does.
    let lab = if c[0].max(c[1]).max(c[2]) > 1.0 { oklab(roll_off(c)) } else { oklab(c) };
    let d = picks
        .iter()
        .map(|p| ((0.5 * (lab[0] - p[0])).powi(2) + (lab[1] - p[1]).powi(2) + (lab[2] - p[2]).powi(2)).sqrt())
        .fold(f64::INFINITY, f64::min);
    if d.is_finite() { 1.0 - smoothstep(inner, outer, d) } else { 0.0 }
}

/// A Luminance Range's numbers: `[low, high, fade]` in 0..1, low under high.
pub fn luminance_numbers(l: &GradeLuminanceRange) -> [f64; 3] {
    let a = (finite_or(l.low, 50.0) / 100.0).clamp(0.0, 1.0);
    let b = (finite_or(l.high, 100.0) / 100.0).clamp(0.0, 1.0);
    let fade = (0.3 * finite_or(l.smoothness, 50.0) / 100.0).clamp(1.0 / 512.0, 0.3);
    [a.min(b), a.max(b), fade]
}

/// The brightness a Luminance Range reads: the tone luma of the colour as the screen shows it.
pub fn shown_luma(c: [f64; 3]) -> f64 {
    let shown = roll_off(c.map(|v| v.max(0.0)));
    luminance(shown.map(tone)).clamp(0.0, 1.0)
}

/// How much a Luminance Range covers the linear colour `c`.
pub fn luminance_cover(c: [f64; 3], [low, high, fade]: [f64; 3]) -> f64 {
    let y = shown_luma(c);
    smoothstep(low - fade, low, y) * (1.0 - smoothstep(high, high + fade, y))
}

/// Coverage `cov` so far joined by a part's `v`.
pub fn combine(cov: f64, v: f64, how: GradeCombine) -> f64 {
    match how {
        GradeCombine::Add => cov.max(v),
        GradeCombine::Subtract => cov * (1.0 - v),
        GradeCombine::Intersect => cov * v,
    }
}

/// How much one part covers screen point `p` with colour `c` (the linear colour the mask starts
/// from), before its Invert. Copies of a gradient join with Add; no place covers nothing.
pub fn part_cover(shape: &GradeShape, c: [f64; 3], p: [f64; 2], px: f64) -> f64 {
    let most = |places: &[GradePlace], f: &dyn Fn(&GradePlace) -> f64| {
        places.iter().filter(|q| place_ok(q)).take(crate::grade::MAX_BALLS).map(f).fold(0.0, f64::max)
    };
    match shape {
        GradeShape::Linear(l) => most(&l.places, &|q| linear_cover(linear_numbers(l), q, p, px)),
        GradeShape::Radial(r) => most(&r.places, &|q| radial_cover(radial_numbers(r), q, p, px)),
        GradeShape::ColourRange(r) => colour_range_cover(c, &range_picks(r), range_widths(r.refine)),
        GradeShape::LuminanceRange(l) => luminance_cover(c, luminance_numbers(l)),
        // A Things part needs the silhouettes the renderer draws: see `things_cover`.
        GradeShape::Things(_) => 0.0,
        GradeShape::Brush(b) => match crate::grade::brush::grid(&b.strokes) {
            Some(g) => most(&b.places, &|q| brush_cover(&g, q, p)),
            None => 0.0,
        },
    }
}

/// A screen point in a place's own units (the inverse of [`placed`]).
pub fn unplaced(place: &GradePlace, p: [f64; 2]) -> [f64; 2] {
    let d = [p[0] - place.at[0] as f64, p[1] - place.at[1] as f64];
    let (s, c) = (place.turn as f64).sin_cos();
    let k = 1.0 / place.scale as f64;
    [(d[0] * c + d[1] * s) * k, (-d[0] * s + d[1] * c) * k]
}

/// One copy of a Brush at `place`, at screen point `p`: its grid's paint there.
pub fn brush_cover(grid: &crate::grade::brush::BrushGrid, place: &GradePlace, p: [f64; 2]) -> f64 {
    grid.sample(unplaced(place, p))
}

/// How much a Things part covers a pixel whose silhouette field says `cover` (the chosen things'
/// coverage there) and `distance` (output pixels to the nearest pixel at least half covered):
/// the silhouette itself, grown by `expand` and faded out over `feather` (pixels of the video,
/// `scale` output pixels each); `background` covers the rest instead.
pub fn things_cover(cover: f64, distance: f64, expand: f64, feather: f64, scale: f64, background: bool) -> f64 {
    let (e, f) = (expand.max(0.0) * scale, feather.max(0.0) * scale);
    let mut v = cover.clamp(0.0, 1.0);
    if e + f > 0.0 {
        v = v.max(1.0 - smoothstep(e - 0.5, e + f + 0.5, distance - 0.5));
    }
    if background { 1.0 - v } else { v }
}

/// How much a mask's area covers screen point `p` (pixels of the video from the middle, y down)
/// with colour `c`; `px` is one output pixel in those units. No part covers nothing.
pub fn area_cover(area: &GradeArea, c: [f64; 3], p: [f64; 2], px: f64) -> f64 {
    let mut cov = 0.0;
    for (i, part) in area.parts.iter().enumerate() {
        let mut v = part_cover(&part.shape, c, p, px);
        if part.invert {
            v = 1.0 - v;
        }
        cov = if i == 0 { v } else { combine(cov, v, part.combine) };
    }
    if area.invert && !area.parts.is_empty() { 1.0 - cov } else { cov }
}

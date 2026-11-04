//! A filter's response worked out from its settings, for the editor's curve and the tests.

use super::{FilterParams, FilterType, comb_coefs, formant_bands, ladder, ladder_coefs, svf_stages};
use crate::SAMPLE_RATE;

fn mul(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    (a.0 * b.0 - a.1 * b.1, a.0 * b.1 + a.1 * b.0)
}

/// The quietest the response reads.
pub const FLOOR_DB: f32 = -120.0;

/// The filter's gain at `freq_hz`, in dB (small signals, no drive), down to [`FLOOR_DB`].
pub fn response_db(p: &FilterParams, freq_hz: f32) -> f32 {
    let nyq = SAMPLE_RATE as f64 * 0.5;
    let f = (freq_hz as f64).clamp(1.0, nyq * 0.999);
    let tan_f = (std::f64::consts::PI * f / SAMPLE_RATE as f64).tan();
    let h = match p.kind {
        FilterType::Ladder => ladder::response(&ladder_coefs(p), tan_f),
        FilterType::Comb => {
            let (d, fb, norm) = comb_coefs(p);
            let w = std::f64::consts::TAU * f / SAMPLE_RATE as f64 * d as f64;
            // norm / (1 - fb e^{-jw})
            let (dr, di) = (1.0 - fb as f64 * w.cos(), fb as f64 * w.sin());
            let dd = dr * dr + di * di;
            (norm as f64 * dr / dd, -(norm as f64) * di / dd)
        }
        FilterType::Formant => formant_bands(p).iter().fold((0.0, 0.0), |acc, &(g, k, a)| {
            let band = super::SvfCoefs { g, k, m0: 0.0, m1: k, m2: 0.0 }.response(tan_f);
            (acc.0 + band.0 * a as f64, acc.1 + band.1 * a as f64)
        }),
        _ => match svf_stages(p) {
            Some((stages, n)) => stages[..n].iter().fold((1.0, 0.0), |acc, s| mul(acc, s.response(tan_f))),
            None => (1.0, 0.0),
        },
    };
    let mag = (h.0 * h.0 + h.1 * h.1).sqrt();
    if mag > 0.0 { (20.0 * mag.log10()).max(FLOOR_DB as f64) as f32 } else { FLOOR_DB }
}

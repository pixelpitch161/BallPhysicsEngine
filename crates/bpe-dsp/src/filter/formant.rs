//! The vowel table the Formant filter morphs through: three formants each for A, E, I, O, U
//! (an alto voice, after the Csound manual's table): frequency, level and width.

/// (Hz, dB, bandwidth Hz) for F1..F3 of each vowel.
pub const VOWELS: [[(f64, f64, f64); 3]; 5] = [
    [(800.0, 0.0, 80.0), (1150.0, -4.0, 90.0), (2800.0, -20.0, 120.0)],
    [(400.0, 0.0, 60.0), (1600.0, -24.0, 80.0), (2700.0, -30.0, 120.0)],
    [(350.0, 0.0, 50.0), (1700.0, -20.0, 100.0), (2700.0, -30.0, 120.0)],
    [(450.0, 0.0, 70.0), (800.0, -9.0, 80.0), (2830.0, -16.0, 100.0)],
    [(325.0, 0.0, 50.0), (700.0, -12.0, 60.0), (2530.0, -30.0, 170.0)],
];

/// The three formants at `vowel` (0..4, morphing linearly between neighbours).
pub fn formants(vowel: f64) -> [(f64, f64, f64); 3] {
    let v = if vowel.is_finite() { vowel.clamp(0.0, 4.0) } else { 0.0 };
    let i = (v.floor() as usize).min(3);
    let t = v - i as f64;
    std::array::from_fn(|f| {
        let (a, b) = (VOWELS[i][f], VOWELS[i + 1][f]);
        (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t)
    })
}

//! The plain waves and the ten Warp modes, drawn without band-limiting. The wavetables bake
//! these (band-limited) and the editor's waveform view draws them as they are.

/// A wave a table can hold (Noise is generated, not tabled).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WaveShape {
    Sine,
    Triangle,
    Saw,
    Square,
}

impl WaveShape {
    pub const ALL: [WaveShape; 4] = [WaveShape::Sine, WaveShape::Triangle, WaveShape::Saw, WaveShape::Square];

    /// The plain wave at `phase` (0..1): every wave starts at 0 rising, except the square,
    /// which starts high.
    pub fn at(self, phase: f32) -> f32 {
        let p = phase - phase.floor();
        match self {
            WaveShape::Sine => (std::f32::consts::TAU * p).sin(),
            WaveShape::Triangle => {
                if p < 0.25 {
                    4.0 * p
                } else if p < 0.75 {
                    2.0 - 4.0 * p
                } else {
                    4.0 * p - 4.0
                }
            }
            WaveShape::Saw => {
                // A rising ramp centred on the start, so a note starts at 0.
                let q = p + 0.5;
                2.0 * (q - q.floor()) - 1.0
            }
            WaveShape::Square => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
        }
    }
}

/// How Warp bends a wave; mirrors the settings' Warp mode, in the same order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WarpModeId {
    Sync,
    BendUp,
    BendDown,
    AsymUp,
    AsymDown,
    Pwm,
    Mirror,
    Flip,
    Quantize,
    Squeeze,
}

impl WarpModeId {
    pub const ALL: [WarpModeId; 10] = [
        WarpModeId::Sync,
        WarpModeId::BendUp,
        WarpModeId::BendDown,
        WarpModeId::AsymUp,
        WarpModeId::AsymDown,
        WarpModeId::Pwm,
        WarpModeId::Mirror,
        WarpModeId::Flip,
        WarpModeId::Quantize,
        WarpModeId::Squeeze,
    ];

    /// The mode at `index` in [`WarpModeId::ALL`] (Sync past the end).
    pub fn from_index(index: u8) -> WarpModeId {
        WarpModeId::ALL.get(index as usize).copied().unwrap_or(WarpModeId::Sync)
    }
}

/// The squeeze of `p` that puts the wave's first half before `m` and its second after.
fn split(p: f32, m: f32) -> f32 {
    if p < m { 0.5 * p / m } else { 0.5 + 0.5 * (p - m) / (1.0 - m) }
}

/// Where in the plain wave `phase` (0..1) reads, bent by `mode` at `amount` (0..1). The
/// result is in 0..1, except Sync, whose wave runs several times a cycle (wrapped).
pub fn warp_phase(mode: WarpModeId, wave: WaveShape, amount: f32, phase: f32) -> f32 {
    let a = amount.clamp(0.0, 1.0);
    let p = phase - phase.floor();
    if a == 0.0 {
        return p;
    }
    match mode {
        WarpModeId::Sync => {
            let q = p * (1.0 + 15.0 * a);
            q - q.floor()
        }
        WarpModeId::BendUp => p.powf(1.0 / (1.0 + 4.0 * a)),
        WarpModeId::BendDown => p.powf(1.0 + 4.0 * a),
        WarpModeId::AsymUp => split(p, 0.5 - 0.49 * a),
        WarpModeId::AsymDown => split(p, 0.5 + 0.49 * a),
        WarpModeId::Pwm => {
            if wave == WaveShape::Square {
                // The pulse narrows from half the cycle to 2 %.
                split(p, 0.5 - 0.48 * a)
            } else {
                // The wave runs faster and rests at its start for the rest of the cycle.
                let q = p / (1.0 - 0.95 * a);
                if q >= 1.0 { 0.0 } else { q }
            }
        }
        WarpModeId::Mirror => {
            // Forward to the fold point, then back to the start.
            let fold = 1.0 - 0.5 * a;
            if p <= fold { p } else { fold * (1.0 - p) / (1.0 - fold) }
        }
        WarpModeId::Squeeze => {
            // Slow through the middle of the cycle, quick at its ends.
            let x = 2.0 * p - 1.0;
            0.5 + 0.5 * x.signum() * x.abs().powf(1.0 + 4.0 * a)
        }
        WarpModeId::Flip | WarpModeId::Quantize => p,
    }
}

/// `sample` (read at `phase`, 0..1) changed by the modes that work on the level instead of
/// where the wave reads.
pub fn warp_shape(mode: WarpModeId, amount: f32, phase: f32, sample: f32) -> f32 {
    let a = amount.clamp(0.0, 1.0);
    if a == 0.0 {
        return sample;
    }
    match mode {
        WarpModeId::Flip => {
            let p = phase - phase.floor();
            if p >= 0.5 { sample * (1.0 - 2.0 * a) } else { sample }
        }
        WarpModeId::Quantize => {
            // From 256 levels down to 2.
            let half = 128.0 * (1.0f32 / 128.0).powf(a);
            (sample * half).round() / half
        }
        _ => sample,
    }
}

/// The warped wave at `phase`, not band-limited.
pub fn warped(wave: WaveShape, mode: WarpModeId, amount: f32, phase: f32) -> f32 {
    let s = wave.at(warp_phase(mode, wave, amount, phase));
    warp_shape(mode, amount, phase, s)
}

/// `points` samples of one cycle of the warped wave, for the editor's view.
pub fn draw(wave: WaveShape, mode: WarpModeId, amount: f32, points: usize) -> Vec<f32> {
    let n = points.max(2);
    (0..n).map(|i| warped(wave, mode, amount, i as f32 / n as f32)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_warp_is_the_plain_wave_and_every_wave_starts_quiet_or_high() {
        for mode in WarpModeId::ALL {
            for wave in WaveShape::ALL {
                for k in 0..32 {
                    let p = k as f32 / 32.0;
                    assert_eq!(warped(wave, mode, 0.0, p), wave.at(p), "{mode:?} {wave:?} {p}");
                }
            }
        }
        assert_eq!(WaveShape::Saw.at(0.0), 0.0);
        assert_eq!(WaveShape::Triangle.at(0.25), 1.0);
        assert_eq!(WaveShape::Square.at(0.1), 1.0);
    }

    #[test]
    fn warped_phases_stay_in_a_cycle_and_end_where_they_start() {
        for mode in WarpModeId::ALL {
            for a in [0.25, 0.5, 1.0] {
                for k in 0..200 {
                    let p = warp_phase(mode, WaveShape::Saw, a, k as f32 / 200.0);
                    assert!((0.0..=1.0).contains(&p), "{mode:?} {a} {p}");
                }
            }
        }
        // PWM narrows a square's high part.
        let high = (0..1000).filter(|&k| warped(WaveShape::Square, WarpModeId::Pwm, 1.0, k as f32 / 1000.0) > 0.0);
        assert!((15..=25).contains(&high.count()));
    }
}

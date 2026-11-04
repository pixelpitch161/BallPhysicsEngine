//! A feedback comb: the sound plus itself a period later, again and again, so it rings at a
//! pitch and its harmonics.

use super::svf::flush;

/// The longest period, in samples (20 Hz at 48 kHz, with room to read round it).
pub const COMB_LEN: usize = 4096;

/// One channel's delay line.
#[derive(Clone, Debug)]
pub struct CombLine {
    buf: Vec<f32>,
    pos: usize,
}

impl Default for CombLine {
    fn default() -> Self {
        CombLine { buf: vec![0.0; COMB_LEN], pos: 0 }
    }
}

impl CombLine {
    pub fn reset(&mut self) {
        self.buf.iter_mut().for_each(|x| *x = 0.0);
        self.pos = 0;
    }

    /// Takes on `other`'s memory without allocating.
    pub fn copy_from(&mut self, other: &CombLine) {
        self.buf.copy_from_slice(&other.buf);
        self.pos = other.pos;
    }

    /// `delay` samples (2..COMB_LEN-2), feedback `fb`; the output is scaled by `norm`.
    #[inline]
    pub fn tick(&mut self, x: f32, delay: f32, fb: f32, norm: f32) -> f32 {
        let mask = COMB_LEN - 1;
        let d = delay.clamp(2.0, (COMB_LEN - 2) as f32);
        let whole = d as usize;
        let frac = d - whole as f32;
        let a = self.buf[(self.pos + COMB_LEN - whole) & mask];
        let b = self.buf[(self.pos + COMB_LEN - whole - 1) & mask];
        let past = a + (b - a) * frac;
        let y = x + fb * past;
        self.buf[self.pos] = flush(super::svf::knee(y));
        self.pos = (self.pos + 1) & mask;
        y * norm
    }
}

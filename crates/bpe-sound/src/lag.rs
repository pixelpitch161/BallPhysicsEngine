//! A short stereo delay in fixed memory, for lining up sound that took different paths (a bus
//! with a limiter comes out later than one without).

use crate::BLOCK;

/// The longest delay a [`Lag`] gives, in frames.
pub const MAX_LAG: usize = 2048;
const RING: usize = MAX_LAG + BLOCK;

/// Delays a stereo block by a set number of frames.
#[derive(Clone)]
pub struct Lag {
    ring: Box<[[f32; 2]; RING]>,
    write: usize,
    frames: usize,
}

impl Default for Lag {
    fn default() -> Self {
        Self::new()
    }
}

impl Lag {
    pub fn new() -> Self {
        Self { ring: Box::new([[0.0; 2]; RING]), write: 0, frames: 0 }
    }

    /// The delay, in frames (up to [`MAX_LAG`]).
    pub fn set(&mut self, frames: usize) {
        self.frames = frames.min(MAX_LAG);
    }

    pub fn frames(&self) -> usize {
        self.frames
    }

    pub fn reset(&mut self) {
        self.ring.fill([0.0; 2]);
    }

    /// Delays `l` and `r` in place. With no delay they pass straight through.
    pub fn process(&mut self, l: &mut [f32; BLOCK], r: &mut [f32; BLOCK]) {
        if self.frames == 0 {
            return;
        }
        for i in 0..BLOCK {
            self.ring[self.write] = [l[i], r[i]];
            let read = (self.write + RING - self.frames) % RING;
            [l[i], r[i]] = self.ring[read];
            self.write = (self.write + 1) % RING;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lag_delays_by_its_frames() {
        let mut lag = Lag::new();
        lag.set(70);
        let mut out = Vec::new();
        for b in 0..3 {
            let mut l = [0.0; BLOCK];
            let mut r = [0.0; BLOCK];
            if b == 0 {
                l[5] = 1.0;
                r[5] = -1.0;
            }
            lag.process(&mut l, &mut r);
            out.extend(l.iter().zip(&r).map(|(a, b)| (*a, *b)));
        }
        let at: Vec<usize> = out.iter().enumerate().filter(|(_, v)| v.0 != 0.0).map(|(i, _)| i).collect();
        assert_eq!(at, [75]);
        assert_eq!(out[75], (1.0, -1.0));
    }
}

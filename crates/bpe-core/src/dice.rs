//! The seeded random numbers everything in a scene shares: hashes that mix a few whole numbers
//! into one, small streams of numbers from a seed, and the step from a hash to a number from 0 up
//! to 1. Everything is pure integer maths, the same on every machine, so a scene runs the same
//! every time. The goldens pin what these give: never change one; add another instead.
//!
//! A place that wants its own numbers keeps its own way of mixing its inputs (its seed, its step,
//! its salt) and calls one of these for the mixing itself. Hashes that are a formula of their own
//! stay where they are (the rules' FNV dice, the ball seed, a Diamond's glints, the Light's
//! flicker): they are not copies of these, and changing them would move the goldens. The sound's
//! own noise and LFO (`bpe-dsp`) don't depend on this crate and keep theirs.

/// The 32-bit PCG hash: a well mixed number for `x` (as `vary_pcg` in paint.wgsl).
pub fn pcg32(x: u32) -> u32 {
    let state = x.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let word = ((state >> ((state >> 28) + 4)) ^ state).wrapping_mul(277_803_737);
    (word >> 22) ^ word
}

/// Chris Wellons' "lowbias32" mix of a 32-bit number.
pub fn lowbias32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

/// SplitMix64's finaliser: a well mixed 64-bit number for `z`.
pub fn splitmix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// The shorter 64-bit mix the light shows and the camera's shake use: one multiply between two
/// shifts.
pub fn mix64_short(mut x: u64) -> u64 {
    x ^= x >> 31;
    x = x.wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 29)
}

/// A number from `parts`, the same every time (MurmurHash3's way of folding words in): the
/// particles' hash.
pub fn hash(parts: &[u32]) -> u32 {
    let mut x: u32 = 0x9e37_79b9;
    for &p in parts {
        x ^= p.wrapping_mul(0x85eb_ca6b);
        x = x.rotate_left(13).wrapping_mul(0xc2b2_ae35);
        x ^= x >> 16;
    }
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    x
}

/// The top 24 bits of `x` as a number from 0 up to (not including) 1.
pub fn unit24(x: u32) -> f32 {
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// The top 24 bits of `x` as a number from 0 up to (not including) 1.
pub fn unit24_of_64(x: u64) -> f32 {
    (x >> 40) as f32 / (1u64 << 24) as f32
}

/// The top 24 bits of `x` as a number from 0 to 1, both ends included (a lattice value of a noise).
pub fn unit24_closed(x: u32) -> f32 {
    (x >> 8) as f32 / 16_777_215.0
}

/// A stream of numbers from a 32-bit seed: each one the particles' [`hash`] of the last and a
/// salt of the stream's own.
#[derive(Clone, Debug, PartialEq)]
pub struct Stream {
    state: u32,
    salt: u32,
}

impl Stream {
    pub fn new(seed: u32, salt: u32) -> Stream {
        Stream { state: seed, salt }
    }

    /// 0 up to (not including) 1.
    pub fn unit(&mut self) -> f32 {
        self.state = hash(&[self.state, self.salt]);
        unit24(self.state)
    }

    /// -1 to 1.
    pub fn signed(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }
}

/// SplitMix64: a small, fast stream of numbers, good enough for scattering drops.
#[derive(Clone, Debug, PartialEq)]
pub struct Rng(u64);

impl Rng {
    /// A stream for `seed`, salted so two things with the same seed differ.
    pub fn new(seed: u32, salt: u32) -> Rng {
        Rng(((seed as u64) << 32 | salt as u64) ^ 0x9e37_79b9_7f4a_7c15)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        splitmix64(self.0)
    }

    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// Uniform in `[0, 1)`.
    pub fn unit(&mut self) -> f32 {
        unit24(self.next_u32())
    }

    /// Uniform in `[lo, hi)`.
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The values the goldens rest on, pinned so a change to any of these shows up here first.
    #[test]
    fn the_hashes_give_what_they_always_gave() {
        assert_eq!(pcg32(0), 129_708_002);
        assert_eq!(pcg32(1), 2_831_084_092);
        assert_eq!(lowbias32(1), 0x6889_90c0);
        assert_eq!(splitmix64(1), 0x5692_161d_100b_05e5);
        assert_eq!(mix64_short(1), 0x94d0_49bf_b5b3_5c33);
        assert_eq!(hash(&[1, 2, 3]), 0xde0d_eae5);
        assert_ne!(hash(&[1, 2]), hash(&[2, 1]));
        let mut s = Stream::new(9, 0x5bd1_e995);
        let xs: Vec<f32> = (0..1000).map(|_| s.unit()).collect();
        assert!(xs.iter().all(|x| (0.0..1.0).contains(x)));
        assert!(unit24(u32::MAX) < 1.0 && unit24_of_64(u64::MAX) < 1.0 && unit24_closed(u32::MAX) == 1.0);
    }

    #[test]
    fn the_same_seed_gives_the_same_stream() {
        let mut a = Rng::new(7, 1);
        let mut b = Rng::new(7, 1);
        let mut c = Rng::new(7, 2);
        let xs: Vec<f32> = (0..100).map(|_| a.unit()).collect();
        let ys: Vec<f32> = (0..100).map(|_| b.unit()).collect();
        let zs: Vec<f32> = (0..100).map(|_| c.unit()).collect();
        assert_eq!(xs, ys);
        assert_ne!(xs, zs);
        assert!(xs.iter().all(|x| (0.0..1.0).contains(x)));
        assert!(xs.iter().any(|&x| x > 0.5) && xs.iter().any(|&x| x < 0.5), "not stuck: {xs:?}");
    }

    #[test]
    fn range_stays_inside() {
        let mut r = Rng::new(3, 0);
        for _ in 0..1000 {
            let x = r.range(-3.0, 5.0);
            assert!((-3.0..5.0).contains(&x), "{x}");
        }
        assert_eq!(Rng::new(1, 1).range(2.0, 2.0), 2.0);
    }
}

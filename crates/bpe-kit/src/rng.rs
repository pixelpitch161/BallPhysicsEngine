//! A small seeded random generator, so anything random in a scene is the same every run.

/// SplitMix64: fast, tiny, and good enough for scattering drops.
#[derive(Clone, Debug, PartialEq)]
pub struct Rng(u64);

impl Rng {
    /// A stream for `seed`, salted so two things with the same seed differ.
    pub fn new(seed: u32, salt: u32) -> Rng {
        Rng(((seed as u64) << 32 | salt as u64) ^ 0x9e37_79b9_7f4a_7c15)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// Uniform in `[0, 1)`.
    pub fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Uniform in `[lo, hi)`.
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

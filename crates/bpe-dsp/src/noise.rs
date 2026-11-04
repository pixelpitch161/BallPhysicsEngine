//! Noise: white from a seeded xorshift64*, pink through Paul Kellet's filter. The same seed gives
//! the same noise every time.

/// A seeded noise source.
#[derive(Clone, Debug)]
pub struct Noise {
    state: u64,
    pink: [f32; 7],
}

impl Noise {
    pub fn new(seed: u64) -> Self {
        let mut n = Noise { state: 0, pink: [0.0; 7] };
        n.seed(seed);
        n
    }

    /// Starts over from `seed` (any value, 0 included).
    pub fn seed(&mut self, seed: u64) {
        // SplitMix64 spreads the seed, so neighbouring seeds give unrelated noise.
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        self.state = z | 1;
        self.pink = [0.0; 7];
    }

    /// The next raw 64-bit number.
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number in 0..1.
    #[inline]
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// White noise, -1..1.
    #[inline]
    pub fn white(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }

    /// Pink noise (-3 dB an octave), about as loud as the white.
    #[inline]
    pub fn pink(&mut self) -> f32 {
        let w = self.white();
        let b = &mut self.pink;
        b[0] = 0.99886 * b[0] + w * 0.055_517_9;
        b[1] = 0.99332 * b[1] + w * 0.075_075_9;
        b[2] = 0.96900 * b[2] + w * 0.153_852;
        b[3] = 0.86650 * b[3] + w * 0.310_485_6;
        b[4] = 0.55000 * b[4] + w * 0.532_952_2;
        b[5] = -0.7616 * b[5] - w * 0.016_898;
        let out = b[0] + b[1] + b[2] + b[3] + b[4] + b[5] + b[6] + w * 0.5362;
        b[6] = w * 0.115_926;
        out * 0.11
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_deterministic_per_seed() {
        let take = |seed: u64| {
            let mut n = Noise::new(seed);
            (0..1000).map(|_| (n.white(), n.pink())).collect::<Vec<_>>()
        };
        assert_eq!(take(7), take(7));
        assert_ne!(take(7), take(8));
        let mut n = Noise::new(0);
        let v: Vec<f32> = (0..10_000).map(|_| n.white()).collect();
        assert!(v.iter().all(|x| (-1.0..=1.0).contains(x)));
        let mean = v.iter().sum::<f32>() / v.len() as f32;
        assert!(mean.abs() < 0.03, "{mean}");
    }

    #[test]
    fn pink_noise_slopes_minus_3db_per_octave() {
        use crate::tables::fft;
        let n = 8192;
        let frames = 64;
        let mut psd = vec![0.0f64; n / 2];
        let mut noise = Noise::new(3);
        for _ in 0..200 {
            noise.pink();
        }
        for _ in 0..frames {
            let (mut re, mut im) = (vec![0.0; n], vec![0.0; n]);
            for (i, r) in re.iter_mut().enumerate() {
                let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos();
                *r = noise.pink() as f64 * w;
            }
            fft(&mut re, &mut im, false);
            for k in 0..n / 2 {
                psd[k] += re[k] * re[k] + im[k] * im[k];
            }
        }
        let band = |hz: f64| {
            let bin = |f: f64| (f * n as f64 / crate::SAMPLE_RATE as f64) as usize;
            let (a, b) = (bin(hz / 2f64.sqrt()), bin(hz * 2f64.sqrt()));
            let mean = psd[a..b].iter().sum::<f64>() / (b - a) as f64;
            10.0 * mean.log10()
        };
        let drop = band(100.0) - band(10_000.0);
        let expected = 3.0103 * (10_000.0f64 / 100.0).log2();
        assert!((drop - expected).abs() < 1.0, "fell {drop:.2} dB over the band, not {expected:.2}");
    }
}

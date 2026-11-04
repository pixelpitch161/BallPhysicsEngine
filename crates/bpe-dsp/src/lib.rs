//! Ball Studio's signal processing: plain blocks with no scene knowledge. Every block allocates
//! only in `new`, never while it processes, and takes no locks.

pub mod db;
pub mod denormal;
pub mod envelope;
pub mod fx;
pub mod limiter;
pub mod loudness;
pub mod noise;
pub mod osc;
pub mod oversample;
pub mod smooth;
pub mod tables;
pub mod warp;

pub use db::{db_to_gain, gain_to_db};
pub use denormal::FlushGuard;
pub use envelope::{EnvParams, Envelope, Stage};
pub use limiter::TruePeakLimiter;
pub use loudness::LoudnessMeter;
pub use noise::Noise;
pub use osc::{MAX_UNISON, SubOsc, SubWaveId, UnisonOsc, UnisonParams};
pub use oversample::{Halfband, Oversampler, true_peak};
pub use smooth::Smoother;
pub use tables::{Reader, TableSet, WaveBank};
pub use warp::{WarpModeId, WaveShape};

/// The engine's sample rate, in Hz.
pub const SAMPLE_RATE: f32 = 48_000.0;
/// Samples in one render block.
pub const BLOCK: usize = 64;

#[cfg(test)]
pub(crate) mod testing {
    /// `n` samples of a sine at `hz` and level `amp`, starting at `phase` radians.
    pub fn sine(hz: f64, amp: f64, phase: f64, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (amp * (std::f64::consts::TAU * hz * i as f64 / crate::SAMPLE_RATE as f64 + phase).sin()) as f32)
            .collect()
    }

    /// A small fixed-seed noise source, -1..1.
    pub struct Noise(pub u64);
    impl Noise {
        pub fn next(&mut self) -> f32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            ((self.0 >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
        }
    }
}

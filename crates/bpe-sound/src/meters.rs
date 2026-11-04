//! The levels the editor shows, written by the sound thread once a block and read by the
//! editor without locks.

use std::sync::atomic::{AtomicU32, Ordering};

/// One f32 that any thread can read and write.
#[derive(Debug, Default)]
struct AtomicF32(AtomicU32);

impl AtomicF32 {
    fn new(v: f32) -> Self {
        Self(AtomicU32::new(v.to_bits()))
    }
    fn set(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed);
    }
    fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
}

/// The master's levels as the sound thread last wrote them.
#[derive(Debug)]
pub struct Meters {
    peak_l: AtomicF32,
    peak_r: AtomicF32,
    momentary: AtomicF32,
    short_term: AtomicF32,
    integrated: AtomicF32,
    true_peak: AtomicF32,
    reduction: AtomicF32,
}

/// A copy of the meters at one moment. Levels in dB (dBFS, LUFS, dBTP); minus infinity for
/// silence.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeterReading {
    pub peak_l: f32,
    pub peak_r: f32,
    pub momentary: f32,
    pub short_term: f32,
    pub integrated: f32,
    pub true_peak: f32,
    /// How far the master's limiter turned the sound down, in dB (0 or less).
    pub reduction: f32,
}

impl MeterReading {
    pub const SILENT: MeterReading = MeterReading {
        peak_l: f32::NEG_INFINITY,
        peak_r: f32::NEG_INFINITY,
        momentary: f32::NEG_INFINITY,
        short_term: f32::NEG_INFINITY,
        integrated: f32::NEG_INFINITY,
        true_peak: f32::NEG_INFINITY,
        reduction: 0.0,
    };
}

impl Default for Meters {
    fn default() -> Self {
        let r = MeterReading::SILENT;
        Self {
            peak_l: AtomicF32::new(r.peak_l),
            peak_r: AtomicF32::new(r.peak_r),
            momentary: AtomicF32::new(r.momentary),
            short_term: AtomicF32::new(r.short_term),
            integrated: AtomicF32::new(r.integrated),
            true_peak: AtomicF32::new(r.true_peak),
            reduction: AtomicF32::new(r.reduction),
        }
    }
}

impl Meters {
    pub fn write(&self, r: &MeterReading) {
        self.peak_l.set(r.peak_l);
        self.peak_r.set(r.peak_r);
        self.momentary.set(r.momentary);
        self.short_term.set(r.short_term);
        self.integrated.set(r.integrated);
        self.true_peak.set(r.true_peak);
        self.reduction.set(r.reduction);
    }

    pub fn read(&self) -> MeterReading {
        MeterReading {
            peak_l: self.peak_l.get(),
            peak_r: self.peak_r.get(),
            momentary: self.momentary.get(),
            short_term: self.short_term.get(),
            integrated: self.integrated.get(),
            true_peak: self.true_peak.get(),
            reduction: self.reduction.get(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reading_round_trips() {
        let m = Meters::default();
        assert_eq!(m.read(), MeterReading::SILENT);
        let r = MeterReading { peak_l: -3.0, peak_r: -4.5, momentary: -14.0, ..MeterReading::SILENT };
        m.write(&r);
        assert_eq!(m.read(), r);
    }
}

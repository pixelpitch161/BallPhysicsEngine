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
    /// Keeps the larger of what it holds and `v`.
    fn raise(&self, v: f32) {
        let _ = self.0.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |bits| {
            (f32::from_bits(bits) < v || f32::from_bits(bits).is_nan()).then_some(v.to_bits())
        });
    }
    /// What it holds, leaving silence.
    fn take(&self) -> f32 {
        f32::from_bits(self.0.swap(f32::NEG_INFINITY.to_bits(), Ordering::Relaxed))
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
    /// Each shape's level, by its voice's place in the patch: the thing's id plus one (0 for
    /// none) and its loudest peak since the last read.
    shape_ids: [AtomicU32; SHAPE_METERS],
    shapes: [AtomicF32; SHAPE_METERS],
}

/// How many shapes have a level meter.
pub const SHAPE_METERS: usize = 64;

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
            shape_ids: std::array::from_fn(|_| AtomicU32::new(0)),
            shapes: std::array::from_fn(|_| AtomicF32::new(f32::NEG_INFINITY)),
        }
    }
}

impl Meters {
    pub fn write(&self, r: &MeterReading) {
        // Peaks hold until they are read, so a short hit between two looks still shows.
        self.peak_l.raise(r.peak_l);
        self.peak_r.raise(r.peak_r);
        self.momentary.set(r.momentary);
        self.short_term.set(r.short_term);
        self.integrated.set(r.integrated);
        self.true_peak.set(r.true_peak);
        self.reduction.set(r.reduction);
    }

    /// Voice `place` (of the patch) is `thing`'s and peaked at `peak_db` in the block just made.
    pub fn write_shape(&self, place: usize, thing: u32, peak_db: f32) {
        if place < SHAPE_METERS {
            let id = thing.wrapping_add(1);
            if self.shape_ids[place].swap(id, Ordering::Relaxed) != id {
                self.shapes[place].set(f32::NEG_INFINITY);
            }
            self.shapes[place].raise(peak_db);
        }
    }

    /// No voice at `place` or after it.
    pub fn clear_shapes_from(&self, place: usize) {
        for i in place..SHAPE_METERS {
            self.shape_ids[i].store(0, Ordering::Relaxed);
        }
    }

    /// `thing`'s loudest peak (dB) since the last read, or silence when it has no voice.
    pub fn take_shape(&self, thing: u32) -> f32 {
        let id = thing.wrapping_add(1);
        (0..SHAPE_METERS)
            .find(|&i| self.shape_ids[i].load(Ordering::Relaxed) == id)
            .map_or(f32::NEG_INFINITY, |i| self.shapes[i].take())
    }

    /// The meters now, with the loudest peaks since the last read.
    pub fn read(&self) -> MeterReading {
        MeterReading {
            peak_l: self.peak_l.take(),
            peak_r: self.peak_r.take(),
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
    #[test]
    fn a_peak_holds_until_it_is_read() {
        let m = Meters::default();
        let loud = MeterReading { peak_l: -3.0, peak_r: -4.0, ..MeterReading::SILENT };
        let quiet = MeterReading { peak_l: -20.0, peak_r: -30.0, ..MeterReading::SILENT };
        m.write(&loud);
        m.write(&quiet);
        let r = m.read();
        assert_eq!((r.peak_l, r.peak_r), (-3.0, -4.0), "the loudest block since the last look");
        let r = m.read();
        assert_eq!(r.peak_l, f32::NEG_INFINITY, "nothing played since");
    }

    use super::*;

    #[test]
    fn a_shape_s_level_is_found_by_its_thing() {
        let m = Meters::default();
        m.write_shape(1, 7, -12.0);
        m.write_shape(1, 7, -20.0);
        assert_eq!(m.take_shape(7), -12.0);
        assert_eq!(m.take_shape(7), f32::NEG_INFINITY, "taken");
        assert_eq!(m.take_shape(3), f32::NEG_INFINITY, "no voice");
        m.write_shape(1, 7, -6.0);
        m.clear_shapes_from(1);
        assert_eq!(m.take_shape(7), f32::NEG_INFINITY, "its voice went");
    }

    #[test]
    fn a_reading_round_trips() {
        let m = Meters::default();
        assert_eq!(m.read(), MeterReading::SILENT);
        let r = MeterReading { peak_l: -3.0, peak_r: -4.5, momentary: -14.0, ..MeterReading::SILENT };
        m.write(&r);
        assert_eq!(m.read(), r);
    }
}

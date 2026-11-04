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
    /// The slot each of those shapes plays, plus one (0 for none).
    slots: [AtomicU32; SHAPE_METERS],
    buses: [BusMeter; 3],
    fx: Box<[FxMeter; FX_METERS]>,
}

/// One bus's levels: its output peaks (held until read) and the most any of its compressors or
/// limiters turned it down.
#[derive(Debug)]
struct BusMeter {
    peak_l: AtomicF32,
    peak_r: AtomicF32,
    reduction: AtomicF32,
}

impl Default for BusMeter {
    fn default() -> Self {
        Self {
            peak_l: AtomicF32::new(f32::NEG_INFINITY),
            peak_r: AtomicF32::new(f32::NEG_INFINITY),
            reduction: AtomicF32::new(0.0),
        }
    }
}

/// How many bus effects have their own meters (an EQ's analyser, a compressor's levels).
pub const FX_METERS: usize = 32;
/// Bins in an analyser reading.
pub const FX_BINS: usize = 256;

/// One bus effect's readings, by its layer: the layer id plus one (0 for none).
#[derive(Debug)]
struct FxMeter {
    id: AtomicU32,
    reduction: AtomicF32,
    input: AtomicF32,
    output: AtomicF32,
    /// Whatever else the effect measures (the Width's correlation).
    value: AtomicF32,
    bins: [AtomicF32; FX_BINS],
}

impl Default for FxMeter {
    fn default() -> Self {
        Self {
            id: AtomicU32::new(0),
            reduction: AtomicF32::new(0.0),
            input: AtomicF32::new(f32::NEG_INFINITY),
            output: AtomicF32::new(f32::NEG_INFINITY),
            value: AtomicF32::new(0.0),
            bins: std::array::from_fn(|_| AtomicF32::new(-120.0)),
        }
    }
}

/// A bus effect's readings at one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FxReading {
    /// How far it turned the sound down, in dB (0 or less).
    pub reduction: f32,
    /// Its detector's level going in and its output peak, in dB.
    pub input: f32,
    pub output: f32,
    /// The Width's correlation (-1..1); 0 for the others.
    pub value: f32,
}

impl FxReading {
    pub const SILENT: FxReading =
        FxReading { reduction: 0.0, input: f32::NEG_INFINITY, output: f32::NEG_INFINITY, value: 0.0 };
}

/// A bus's levels at one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BusReading {
    pub peak_l: f32,
    pub peak_r: f32,
    pub reduction: f32,
}

impl BusReading {
    pub const SILENT: BusReading = BusReading { peak_l: f32::NEG_INFINITY, peak_r: f32::NEG_INFINITY, reduction: 0.0 };
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
            buses: Default::default(),
            fx: Box::new(std::array::from_fn(|_| FxMeter::default())),
            slots: std::array::from_fn(|_| AtomicU32::new(0)),
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

    /// Voice `place` plays slot `slot` (0 first; `None` for a voice without slots).
    pub fn write_slot(&self, place: usize, slot: Option<usize>) {
        if place < SHAPE_METERS {
            self.slots[place].store(slot.map_or(0, |s| s as u32 + 1), Ordering::Relaxed);
        }
    }

    /// The slot `thing`'s voice is playing (0 first), if it has one.
    pub fn shape_slot(&self, thing: u32) -> Option<usize> {
        let id = thing.wrapping_add(1);
        (0..SHAPE_METERS)
            .find(|&i| self.shape_ids[i].load(Ordering::Relaxed) == id)
            .and_then(|i| self.slots[i].load(Ordering::Relaxed).checked_sub(1).map(|s| s as usize))
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

    /// Bus `bus` (0..2) peaked at these levels (dB) in the block just made.
    pub fn write_bus(&self, bus: usize, r: &BusReading) {
        if let Some(m) = self.buses.get(bus) {
            m.peak_l.raise(r.peak_l);
            m.peak_r.raise(r.peak_r);
            m.reduction.set(r.reduction);
        }
    }

    /// Bus `bus`'s (0..2) loudest peaks since the last read.
    pub fn read_bus(&self, bus: usize) -> BusReading {
        self.buses.get(bus).map_or(BusReading::SILENT, |m| BusReading {
            peak_l: m.peak_l.take(),
            peak_r: m.peak_r.take(),
            reduction: m.reduction.get(),
        })
    }

    /// The meter place for the effect layer `layer`: its own if it has one, else a free one,
    /// else none.
    fn fx_place(&self, layer: u32, claim: bool) -> Option<usize> {
        let id = layer.wrapping_add(1);
        if let Some(i) = (0..FX_METERS).find(|&i| self.fx[i].id.load(Ordering::Relaxed) == id) {
            return Some(i);
        }
        if !claim {
            return None;
        }
        let i = (0..FX_METERS).find(|&i| self.fx[i].id.load(Ordering::Relaxed) == 0)?;
        self.fx[i].id.store(id, Ordering::Relaxed);
        Some(i)
    }

    /// The effect layer `layer`'s readings in the block just made.
    pub fn write_fx(&self, layer: u32, r: &FxReading) {
        if let Some(i) = self.fx_place(layer, true) {
            let m = &self.fx[i];
            m.reduction.set(r.reduction);
            m.input.set(r.input);
            m.output.set(r.output);
            m.value.set(r.value);
        }
    }

    /// The effect layer `layer`'s analyser, `bins` in dB.
    pub fn write_fx_bins(&self, layer: u32, bins: &[f32]) {
        if let Some(i) = self.fx_place(layer, true) {
            for (m, v) in self.fx[i].bins.iter().zip(bins) {
                m.set(*v);
            }
        }
    }

    /// Forgets every effect meter whose layer is not in `keep` (after a patch change).
    pub fn keep_fx(&self, keep: &[u32]) {
        for m in self.fx.iter() {
            let id = m.id.load(Ordering::Relaxed);
            if id != 0 && !keep.contains(&id.wrapping_sub(1)) {
                m.id.store(0, Ordering::Relaxed);
            }
        }
    }

    /// The effect layer `layer`'s readings, or silence when it has no meter.
    pub fn read_fx(&self, layer: u32) -> FxReading {
        self.fx_place(layer, false).map_or(FxReading::SILENT, |i| {
            let m = &self.fx[i];
            FxReading {
                reduction: m.reduction.get(),
                input: m.input.get(),
                output: m.output.get(),
                value: m.value.get(),
            }
        })
    }

    /// The effect layer `layer`'s analyser into `out` (dB); false when it has none.
    pub fn read_fx_bins(&self, layer: u32, out: &mut [f32; FX_BINS]) -> bool {
        let Some(i) = self.fx_place(layer, false) else { return false };
        for (o, m) in out.iter_mut().zip(self.fx[i].bins.iter()) {
            *o = m.get();
        }
        true
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
    fn bus_and_effect_meters_round_trip() {
        let m = Meters::default();
        m.write_bus(1, &BusReading { peak_l: -6.0, peak_r: -7.0, reduction: -2.0 });
        m.write_bus(1, &BusReading { peak_l: -20.0, peak_r: -30.0, reduction: -1.0 });
        assert_eq!(m.read_bus(1), BusReading { peak_l: -6.0, peak_r: -7.0, reduction: -1.0 });
        assert_eq!(m.read_bus(1).peak_l, f32::NEG_INFINITY);
        assert_eq!(m.read_fx(4), FxReading::SILENT);
        let r = FxReading { reduction: -3.0, input: -10.0, output: -12.0, value: 0.5 };
        m.write_fx(4, &r);
        assert_eq!(m.read_fx(4), r);
        let mut bins = [0.0; FX_BINS];
        assert!(m.read_fx_bins(4, &mut bins));
        assert_eq!(bins[0], -120.0);
        m.write_fx_bins(4, &[-6.0; FX_BINS]);
        assert!(m.read_fx_bins(4, &mut bins));
        assert_eq!(bins[200], -6.0);
        m.keep_fx(&[9]);
        assert_eq!(m.read_fx(4), FxReading::SILENT, "its layer went");
    }

    #[test]
    fn a_shape_s_slot_is_found_by_its_thing() {
        let m = Meters::default();
        assert_eq!(m.shape_slot(7), None);
        m.write_shape(2, 7, -12.0);
        m.write_slot(2, Some(3));
        assert_eq!(m.shape_slot(7), Some(3));
        m.write_slot(2, None);
        assert_eq!(m.shape_slot(7), None);
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

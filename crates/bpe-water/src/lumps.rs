//! Rain, drips and bursts (Water mode part 4.1): pours that come out as **lumps**, each a disc of
//! drops born at its own moment, rather than as a tap's steady rows.
//!
//! Lump `k` of a stream is born at `from + k × every` while that is before `until` (`every` 0 is
//! one lump at `from`). Where it is, how big and which way it sets off come from a hash of the
//! stream's seed and `k`, so the drops born in any stretch of time are worked out from the times
//! alone: a restart point holds nothing for them, and splitting a stretch in two gives the same
//! drops. The engine is handed each frame's lumps as [`Timed`] drops ([`crate::Water::set_timed`])
//! and moves each on from its moment to the end of the step it falls in.

use glam::Vec2;

use crate::drop::Drop;
use crate::solid::{Layout, SolidField};

/// The most lumps one call makes (a frame's worth even at thousands a second).
const MOST_LUMPS: usize = 100_000;
/// Cells from the grid's edge inside which no drop is born (drops stop at 2.05: `EDGE` + 0.05).
const EDGE_CLEAR: f32 = 2.1;

/// How a stream of lumps comes out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LumpKind {
    /// Drops falling over a width, each at a place, size and way of its own.
    Rain,
    /// One drop at a time from a point.
    Drip,
    /// A lump thrown all at once (and again every so often).
    Burst,
}

/// A stream of lumps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lumps {
    pub kind: LumpKind,
    /// Its middle, world pixels.
    pub place: Vec2,
    /// How wide rain falls across its way, pixels (a drip and a burst have none).
    pub width: f32,
    /// A lump's diameter, pixels (rain's vary from three quarters of it to one and a quarter).
    pub size: f32,
    /// Seconds between lumps; 0 (or not a number above 0) is one lump at `from`.
    pub every: f64,
    /// When the first lump comes, and when they stop, seconds.
    pub from: f64,
    pub until: f64,
    /// Tells two streams' patterns apart.
    pub seed: u64,
    /// The drops' kind (their liquid and [`crate::STOPS`]) and colour (as [`crate::Pour`]'s).
    pub kind_bits: u32,
    pub colour: u32,
}

/// How a lump sets off at its moment: the stream's Aim (degrees, 90 is down), Speed (pixels a
/// second) and Spread (degrees) then.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Throw {
    pub aim: f32,
    pub speed: f32,
    pub spread: f32,
}

/// A drop born at its own moment (seconds), as it is then: place and speed in grid units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timed {
    pub at: f64,
    pub drop: Drop,
}

impl Lumps {
    /// Each lump born from `t0` (included) to `t1` (not): its number and its moment.
    pub fn times(&self, t0: f64, t1: f64) -> Vec<(u64, f64)> {
        let mut out = Vec::new();
        if !(t0 < t1 && self.from < self.until) {
            return out;
        }
        let once = !(self.every.is_finite() && self.every > 0.0);
        if once {
            if t0 <= self.from && self.from < t1 {
                out.push((0, self.from));
            }
            return out;
        }
        let at = |k: u64| self.from + k as f64 * self.every;
        // (The first at or after `t0`, found from the quotient and then made exact.)
        let mut k = ((t0 - self.from) / self.every).ceil().max(0.0) as u64;
        while k > 0 && at(k - 1) >= t0 {
            k -= 1;
        }
        while at(k) < t0 {
            k += 1;
        }
        while at(k) < t1 && at(k) < self.until && out.len() < MOST_LUMPS {
            out.push((k, at(k)));
            k += 1;
        }
        out
    }

    /// Lump `k`'s middle (world pixels), diameter (pixels) and the turn of its way (degrees), thrown
    /// as `t` says (rain falls across its width at right angles to its Aim).
    pub fn lump(&self, k: u64, t: Throw) -> (Vec2, f32, f32) {
        match self.kind {
            LumpKind::Rain => {
                let across = way_at(if t.aim.is_finite() { t.aim } else { 90.0 }).perp();
                (
                    self.place + across * ((hash01(self.seed, k, 0) - 0.5) * self.width.max(0.0)),
                    self.size * (0.75 + 0.5 * hash01(self.seed, k, 2)),
                    (hash01(self.seed, k, 1) - 0.5) * if t.spread.is_finite() { t.spread } else { 0.0 },
                )
            }
            LumpKind::Drip | LumpKind::Burst => (self.place, self.size, 0.0),
        }
    }

    /// The drops of every lump born from `t0` to `t1` on `layout`, each at its moment, with the
    /// way it sets off from `throw` at that moment. A drop within `clear` pixels of the solids of
    /// `field`, or too near the course's edge, is left out.
    pub fn born(
        &self,
        layout: &Layout,
        field: &SolidField,
        clear: f32,
        t0: f64,
        t1: f64,
        throw: impl Fn(f64) -> Throw,
    ) -> Vec<Timed> {
        let mut out = Vec::new();
        let s = layout.cell * 0.5;
        let (lo, hi) = (Vec2::splat(EDGE_CLEAR), Vec2::new(layout.cols as f32, layout.rows as f32) - 1.0 - EDGE_CLEAR);
        for (k, at) in self.times(t0, t1) {
            let t = throw(at);
            let (middle, size, turn) = self.lump(k, t);
            let r = 0.5 * size;
            if !(r.is_finite() && r > 0.0 && middle.is_finite()) {
                continue;
            }
            let way = way_at(if t.aim.is_finite() { t.aim } else { 90.0 } + turn);
            let speed = if t.speed.is_finite() { t.speed.max(0.0) } else { 0.0 };
            let apart = (t.spread / 180.0).clamp(0.0, 1.0);
            // (The lattice offset by the lump's own share of a spacing: a disc on a lattice at a
            // random offset holds its area over the spacing squared on average, whatever its size,
            // so rain pours its amount at every detail.)
            let off = Vec2::new(hash01(self.seed, k, 3), hash01(self.seed, k, 4)) - 0.5;
            let n = (r / s).ceil() as i32 + 1;
            for j in -n..=n {
                for i in -n..=n {
                    let d = (Vec2::new(i as f32, j as f32) + off) * s;
                    if d.length() > r {
                        continue;
                    }
                    let p = middle + d;
                    let g = layout.to_grid(p);
                    if !(g.cmpge(lo).all() && g.cmple(hi).all() && field.at(p) > clear) {
                        continue;
                    }
                    let v = match self.kind {
                        LumpKind::Burst => (way * (1.0 - apart) + d / r * apart) * speed,
                        LumpKind::Rain | LumpKind::Drip => way * speed,
                    };
                    out.push(Timed { at, drop: Drop::new(g, v / layout.cell, self.kind_bits, 0, self.colour) });
                }
            }
        }
        out
    }

    /// About how many drops the lumps born from `t0` to `t1` hold on `layout` (their area over the
    /// spacing squared; the solids and the edge left out of count).
    pub fn amount(&self, layout: &Layout, t0: f64, t1: f64) -> f64 {
        let s = f64::from(layout.cell) * 0.5;
        self.times(t0, t1)
            .iter()
            .map(|&(k, _)| {
                let r = f64::from(self.lump(k, Throw { aim: 90.0, speed: 0.0, spread: 0.0 }).1.max(0.0)) * 0.5;
                std::f64::consts::PI * r * r / (s * s)
            })
            .sum()
    }
}

/// A hash of `seed`, `k` and `j` to [0, 1): splitmix64, the same on every machine.
pub fn hash01(seed: u64, k: u64, j: u64) -> f32 {
    let mut z = seed ^ k.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ j.wrapping_mul(0xD1B5_4A32_D192_ED03);
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    // (The top 24 bits: a float that is exactly a share of 2^24, so below 1.)
    (z >> 40) as f32 / (1u64 << 24) as f32
}

fn way_at(degrees: f32) -> Vec2 {
    crate::drop::way_at(degrees)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solid::Solid;

    fn open(cell: f32) -> (Layout, SolidField) {
        let layout = Layout::cover(Vec2::new(-400.0, -400.0), Vec2::new(400.0, 400.0), cell);
        (layout, SolidField::build(layout, &[]))
    }

    fn rain(width: f32, size: f32, every: f64) -> Lumps {
        Lumps {
            kind: LumpKind::Rain,
            place: Vec2::new(0.0, -100.0),
            width,
            size,
            every,
            from: 0.25,
            until: 10.0,
            seed: 7,
            kind_bits: crate::STOPS,
            colour: crate::WATER,
        }
    }

    const DOWN: Throw = Throw { aim: 90.0, speed: 300.0, spread: 0.0 };

    #[test]
    fn lumps_come_at_their_times() {
        let r = rain(100.0, 12.0, 0.1);
        let t = r.times(0.0, 1.0);
        assert_eq!(t.len(), 8, "{t:?}");
        assert_eq!(t[0], (0, 0.25));
        assert!((t[7].1 - 0.95).abs() < 1e-12);
        assert!(r.times(0.0, 0.25).is_empty(), "nothing before Starts");
        let stopped = Lumps { until: 0.5, ..r };
        assert_eq!(stopped.times(0.0, 10.0).len(), 3, "none from Stops on");
        let once = Lumps { every: 0.0, ..r };
        assert_eq!(once.times(0.0, 10.0), vec![(0, 0.25)]);
        assert!(once.times(0.26, 10.0).is_empty());
        assert!(Lumps { from: 2.0, until: 1.0, ..r }.times(0.0, 10.0).is_empty());
        // Split anywhere, the same lumps.
        let whole = r.times(0.0, 3.0);
        let mut parts = Vec::new();
        for (a, b) in [(0.0, 0.35), (0.35, 0.3500001), (0.3500001, 1.7), (1.7, 3.0)] {
            parts.extend(r.times(a, b));
        }
        assert_eq!(whole, parts);
    }

    #[test]
    fn born_drops_split_the_same_way() {
        let (layout, field) = open(6.0);
        let r = rain(200.0, 14.0, 1.0 / 30.0);
        let whole = r.born(&layout, &field, 1.5, 0.0, 1.0, |_| DOWN);
        let mut parts = r.born(&layout, &field, 1.5, 0.0, 0.5, |_| DOWN);
        parts.extend(r.born(&layout, &field, 1.5, 0.5, 1.0, |_| DOWN));
        assert_eq!(whole, parts);
        assert!(!whole.is_empty());
    }

    #[test]
    fn rain_falls_within_its_width_and_differs_by_seed() {
        let (layout, field) = open(4.0);
        let r = rain(300.0, 10.0, 0.01);
        let drops = r.born(&layout, &field, 1.0, 0.0, 2.0, |_| DOWN);
        let xs: Vec<f32> = drops.iter().map(|t| layout.to_world(Vec2::from(t.drop.x)).x).collect();
        assert!(xs.iter().all(|x| x.abs() <= 150.0 + 8.0), "within the width and a drop's size");
        assert!(xs.iter().any(|x| *x < -100.0) && xs.iter().any(|x| *x > 100.0), "across all of it");
        let other = Lumps { seed: 8, ..r }.born(&layout, &field, 1.0, 0.0, 2.0, |_| DOWN);
        assert_ne!(drops, other);
        for t in &drops {
            assert!((Vec2::from(t.drop.v) * layout.cell - Vec2::new(0.0, 300.0)).length() < 1e-2);
        }
    }

    #[test]
    fn a_lump_holds_its_area() {
        // Lumps big and small hold their area over the spacing squared, on average.
        for (cell, size, every) in [(8.0, 12.0, 0.001), (8.0, 3.0, 0.0002), (2.0, 12.0, 0.01), (2.0, 60.0, 0.01)] {
            let (layout, field) = open(cell);
            let drip = Lumps { kind: LumpKind::Drip, width: 0.0, ..rain(0.0, size, every) };
            let drops = drip.born(&layout, &field, 0.0, 0.0, 2.0, |_| DOWN).len() as f64;
            let want = drip.amount(&layout, 0.0, 2.0);
            assert!((drops / want - 1.0).abs() < 0.03, "{cell} px cells, {size} px drops: {drops} of {want}");
        }
    }

    #[test]
    fn a_burst_is_thrown_or_bursts_apart() {
        let (layout, field) = open(4.0);
        let burst = Lumps { kind: LumpKind::Burst, every: 0.0, ..rain(0.0, 80.0, 0.0) };
        let thrown = burst.born(&layout, &field, 0.0, 0.0, 1.0, |_| Throw { aim: 0.0, speed: 500.0, spread: 0.0 });
        assert!(thrown.len() > 300);
        assert!(thrown.iter().all(|t| (Vec2::from(t.drop.v) * 4.0 - Vec2::new(500.0, 0.0)).length() < 1e-2));
        let apart = burst.born(&layout, &field, 0.0, 0.0, 1.0, |_| Throw { aim: 0.0, speed: 500.0, spread: 180.0 });
        for t in &apart {
            let d = layout.to_world(Vec2::from(t.drop.x)) - burst.place;
            let v = Vec2::from(t.drop.v) * 4.0;
            assert!((v - d / 40.0 * 500.0).length() < 1e-2, "outward by how far out: {d} {v}");
        }
        assert!(apart.iter().all(|t| t.at == 0.25), "all at once");
        let again = Lumps { every: 0.5, ..burst };
        assert_eq!(again.times(0.0, 2.0).len(), 4);
    }

    #[test]
    fn no_drop_is_born_in_a_solid() {
        let layout = Layout::cover(Vec2::new(-400.0, -400.0), Vec2::new(400.0, 400.0), 4.0);
        let field =
            SolidField::build(layout, &[Solid::rect(Vec2::new(0.0, -400.0), Vec2::new(400.0, 400.0), true, 0.0)]);
        let burst = Lumps { kind: LumpKind::Burst, every: 0.0, place: Vec2::ZERO, ..rain(0.0, 100.0, 0.0) };
        let drops = burst.born(&layout, &field, 1.0, 0.0, 1.0, |_| DOWN);
        assert!(!drops.is_empty());
        assert!(drops.iter().all(|t| field.at(layout.to_world(Vec2::from(t.drop.x))) > 1.0));
        assert!(drops.iter().all(|t| layout.to_world(Vec2::from(t.drop.x)).x < -1.0), "only the open half");
    }

    #[test]
    fn the_hash_is_a_share() {
        let all: Vec<f32> = (0..10_000).map(|k| hash01(3, k, 0)).collect();
        assert!(all.iter().all(|h| (0.0..1.0).contains(h)));
        let mean = all.iter().sum::<f32>() / all.len() as f32;
        assert!((mean - 0.5).abs() < 0.02, "{mean}");
        assert_eq!(hash01(3, 5, 1), hash01(3, 5, 1));
        assert_ne!(hash01(3, 5, 1), hash01(3, 5, 2));
    }
}

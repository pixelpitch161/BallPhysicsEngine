//! Modulation routes, worked out: which modulator moves which knob by how much. A route's
//! Amount is a share of its knob's own travel, as in Serum: the knob moves from where it is set
//! by `amount × m` of the way round (evenly in ratios on a frequency knob), stopping at the
//! knob's ends, so turning the knob slides the range without changing its size. The one
//! exception is an envelope on a level knob (an oscillator's Gain, Level or Sub level, or the
//! Output's Volume): there each route is a VCA of its own, the knob setting the peak and the
//! envelope shaping it, so an envelope at 0 with amount 1 is silence.

use bpe_core::{LayerId, ModRoute, SettingDef};
use std::collections::BTreeMap;

/// How a route moves its knob.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Combine {
    /// The route adds a share of the knob's travel.
    Add,
    /// The route scales the level (a VCA): an envelope on a level knob.
    Scale,
}

/// Whether the knob at `key` is a level: the oscillators' Gain, Level and Sub level, and the
/// Output's Volume (dB knobs whose bottom is silence).
pub fn is_level_key(key: &str) -> bool {
    let osc = key.starts_with("a_") || key.starts_with("b_");
    (osc && (key.ends_with("_gain") || key.ends_with("_level"))) || key == "out_volume"
}

/// How a route from an envelope (`from_envelope`) or any other modulator moves the knob at
/// `key`: envelopes scale levels, everything else adds.
pub fn combine_of(key: &str, from_envelope: bool) -> Combine {
    if from_envelope && is_level_key(key) { Combine::Scale } else { Combine::Add }
}

/// Whether the knob for `def` turns evenly in ratios (frequencies) rather than in steps: the
/// editor draws it so, and its routes add in the same feel.
pub fn turns_in_ratios(def: &SettingDef) -> bool {
    def.unit == "Hz" && def.range.is_some_and(|(lo, hi)| lo > 0.0 && hi > lo)
}

/// One knob a route can reach: where its value sits in the voice's list, its range, and whether
/// it turns evenly in ratios.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TargetInfo {
    pub index: u16,
    pub range: (f64, f64),
    pub log: bool,
}

/// Every knob of a voice that routes can reach, by key.
#[derive(Clone, Debug, Default)]
pub struct TargetTable {
    pub map: BTreeMap<String, TargetInfo>,
}

impl TargetTable {
    pub fn insert(&mut self, key: impl Into<String>, info: TargetInfo) {
        self.map.insert(key.into(), info);
    }

    pub fn get(&self, key: &str) -> Option<&TargetInfo> {
        self.map.get(key)
    }
}

/// A modulator, by its place in [`ModMatrix::sources`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceId(pub u16);

#[derive(Clone, Copy, Debug, PartialEq)]
struct Route {
    source: u16,
    target: u16,
    amount: f32,
    bipolar: bool,
    invert: bool,
    combine: Combine,
    lo: f32,
    hi: f32,
    log: bool,
}

impl Route {
    /// `v` moved by this adding route with its modulator at `s` (not clamped).
    #[inline]
    fn add(&self, v: f32, s: f32) -> f32 {
        let m = if self.invert { 1.0 - s } else { s };
        let m = if self.bipolar { 2.0 * m - 1.0 } else { m };
        if self.log {
            let span = (self.hi / self.lo).ln();
            // Not clamped between routes: the shares add up first and the knob clamps once.
            let t = (v.max(f32::MIN_POSITIVE) / self.lo).ln() + self.amount * m * span;
            self.lo * t.exp()
        } else {
            v + self.amount * m * (self.hi - self.lo)
        }
    }

    /// The gain this scaling route multiplies its level by, its modulator at `s`.
    #[inline]
    fn scale(&self, s: f32) -> f32 {
        let m = if self.invert { 1.0 - s } else { s };
        (1.0 - self.amount.abs() * (1.0 - m.clamp(0.0, 1.0))).max(0.0)
    }
}

/// A voice's routes, ready to apply each block.
#[derive(Clone, Debug, Default)]
pub struct ModMatrix {
    routes: Vec<Route>,
    sources: Vec<LayerId>,
    targets: usize,
}

impl ModMatrix {
    /// Works out `routes` against `targets`, `envelopes` being the modulators that are ADSRs; a
    /// route to a knob that isn't there, or with a number out of range, is left out.
    pub fn build(routes: &[ModRoute], targets: &TargetTable, envelopes: &[LayerId]) -> ModMatrix {
        let targets_len = targets.map.values().map(|t| t.index as usize + 1).max().unwrap_or(0);
        let mut m = ModMatrix { targets: targets_len, ..Default::default() };
        for r in routes.iter().filter(|r| r.is_valid()) {
            let Some(t) = targets.get(&r.target) else { continue };
            let source = match m.sources.iter().position(|s| *s == r.source) {
                Some(i) => i,
                None => {
                    m.sources.push(r.source);
                    m.sources.len() - 1
                }
            } as u16;
            let combine = combine_of(&r.target, envelopes.contains(&r.source));
            // A scaling knob's amount is how far towards silence; a negative one works the other
            // way round, as Inv does (the louder the modulator, the quieter).
            let flip = combine == Combine::Scale && r.amount < 0.0;
            m.routes.push(Route {
                source,
                target: t.index,
                amount: if flip { -r.amount } else { r.amount },
                bipolar: r.bipolar,
                invert: r.invert != flip,
                combine,
                lo: t.range.0 as f32,
                hi: t.range.1 as f32,
                log: t.log && t.range.0 > 0.0,
            });
        }
        m
    }

    /// The modulator layers the routes use, in [`SourceId`] order.
    pub fn sources(&self) -> &[LayerId] {
        &self.sources
    }

    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }

    /// Whether source `source` moves the knob at `target` in the `combine` way.
    pub fn route_from(&self, source: u16, target: u16, combine: Combine) -> bool {
        self.routes.iter().any(|r| r.source == source && r.target == target && r.combine == combine)
    }

    /// Whether any route reaches the knob at `target`.
    pub fn reaches(&self, target: u16) -> bool {
        self.routes.iter().any(|r| r.target == target)
    }

    /// The knobs' values with every route applied: `sources[i]` is modulator `i` now (0..1),
    /// `base` the knobs as set. Scaling knobs come out in dB (`-inf` for silence).
    pub fn apply(&self, sources: &[f32], base: &[f32], out: &mut [f32]) {
        out[..base.len()].copy_from_slice(base);
        self.apply_routes(sources, out, |_| true);
    }

    /// [`ModMatrix::apply`] for the knobs at places `lo..hi` only: `out[lo..hi]` is `base`'s
    /// with the routes reaching those knobs applied, the rest of `out` is left alone.
    pub fn apply_range(&self, sources: &[f32], base: &[f32], out: &mut [f32], lo: usize, hi: usize) {
        out[lo..hi].copy_from_slice(&base[lo..hi]);
        self.apply_routes(sources, out, |t| (lo..hi).contains(&t));
    }

    /// The routes on the knobs `mine` picks, as the sound works them: the adding routes summed
    /// in the knob's own feel and clamped at its ends, then the envelopes scaling a level.
    fn apply_routes(&self, sources: &[f32], out: &mut [f32], mine: impl Fn(usize) -> bool) {
        let source = |r: &Route| sources.get(r.source as usize).copied().unwrap_or(0.0);
        let adding = || self.routes.iter().filter(|r| r.combine == Combine::Add && mine(r.target as usize));
        for r in adding() {
            let t = r.target as usize;
            out[t] = r.add(out[t], source(r));
        }
        for r in adding() {
            let t = r.target as usize;
            out[t] = out[t].clamp(r.lo, r.hi);
        }
        for r in self.routes.iter().filter(|r| r.combine == Combine::Scale && mine(r.target as usize)) {
            out[r.target as usize] += 20.0 * r.scale(source(r)).log10();
        }
    }

    /// Whether any route reaches a knob at places `lo..hi`.
    pub fn reaches_range(&self, lo: usize, hi: usize) -> bool {
        self.routes.iter().any(|r| (lo..hi).contains(&(r.target as usize)))
    }

    /// The linear gain the scaling routes on `target` multiply its level by, with the
    /// modulators at `sources` (for working per sample).
    #[inline]
    pub fn factor(&self, target: u16, sources: &[f32]) -> f32 {
        let mut f = 1.0;
        for r in &self.routes {
            if r.target == target && r.combine == Combine::Scale {
                f *= r.scale(sources.get(r.source as usize).copied().unwrap_or(0.0));
            }
        }
        f
    }

    /// The scaling routes on `target`: each one's source, amount and whether it is inverted
    /// (for working a block at a time; [`ModMatrix::factor`] is the same, sample by sample).
    pub fn scaling(&self, target: u16) -> impl Iterator<Item = (u16, f32, bool)> + '_ {
        self.routes
            .iter()
            .filter(move |r| r.target == target && r.combine == Combine::Scale)
            .map(|r| (r.source, r.amount, r.invert))
    }

    /// Whether any adding route reaches the knob at `target`.
    pub fn adds(&self, target: u16) -> bool {
        self.routes.iter().any(|r| r.target == target && r.combine == Combine::Add)
    }

    /// The adding routes on `target`, for working sample by sample: each one's source, how far
    /// it moves the knob at modulator value 1 (in the knob's units, before clamping) and whether
    /// it is two-way and inverted. Only for knobs that turn evenly in steps (the levels).
    pub fn adding(&self, target: u16) -> impl Iterator<Item = (u16, f32, bool, bool)> + '_ {
        self.routes
            .iter()
            .filter(move |r| r.target == target && r.combine == Combine::Add)
            .map(|r| (r.source, r.amount * (r.hi - r.lo), r.bipolar, r.invert))
    }

    /// The range the knob at `target` stops at (its ends), when a route reaches it.
    pub fn range_of(&self, target: u16) -> Option<(f32, f32)> {
        self.routes.iter().find(|r| r.target == target).map(|r| (r.lo, r.hi))
    }

    /// The knob at `target`, set at `base`, with only its adding routes applied (clamped to its
    /// ends): a level knob's dB before its envelopes scale it.
    pub fn offset(&self, target: u16, base: f32, sources: &[f32]) -> f32 {
        let mut v = base;
        let mut range = None;
        for r in self.routes.iter().filter(|r| r.target == target && r.combine == Combine::Add) {
            let s = sources.get(r.source as usize).copied().unwrap_or(0.0);
            v = r.add(v, s);
            range = Some((r.lo, r.hi));
        }
        range.map_or(v, |(lo, hi)| v.clamp(lo, hi))
    }

    /// How many knob values `apply` reads and writes.
    pub fn targets(&self) -> usize {
        self.targets
    }
}

/// Where route `r` takes its knob (set at `base`, over `range`, turning evenly in ratios when
/// `log`; the route moving it the `combine` way) with its modulator at 0 and at 1: the ends of
/// its ring. A scaling route's silent end reads as the bottom of the range.
pub fn route_ends(r: &ModRoute, base: f64, range: (f64, f64), log: bool, combine: Combine) -> (f64, f64) {
    let mut table = TargetTable::default();
    table.insert(r.target.clone(), TargetInfo { index: 0, range, log });
    let one = ModRoute { source: LayerId(0), ..r.clone() };
    let envelopes: &[LayerId] = if combine == Combine::Scale { &[LayerId(0)] } else { &[] };
    let m = ModMatrix::build(std::slice::from_ref(&one), &table, envelopes);
    let at = |s: f32| {
        let mut out = [0.0f32];
        m.apply(&[s], &[base as f32], &mut out);
        let v = out[0] as f64;
        if v.is_finite() { v.clamp(range.0, range.1) } else { range.0 }
    };
    (at(0.0), at(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> TargetTable {
        let mut t = TargetTable::default();
        t.insert("a_gain", TargetInfo { index: 0, range: (-60.0, 12.0), log: false });
        t.insert("a_cents", TargetInfo { index: 1, range: (-100.0, 100.0), log: false });
        t.insert("cutoff", TargetInfo { index: 2, range: (20.0, 20_000.0), log: true });
        t
    }

    const ENV1: [LayerId; 1] = [LayerId(1)];
    const ENV4: [LayerId; 1] = [LayerId(4)];

    fn route(source: u32, target: &str, amount: f32) -> ModRoute {
        ModRoute { source: LayerId(source), target: target.into(), amount, bipolar: false, invert: false }
    }

    fn run(m: &ModMatrix, sources: &[f32], base: &[f32]) -> Vec<f32> {
        let mut out = vec![0.0; base.len()];
        m.apply(sources, base, &mut out);
        out
    }

    #[test]
    fn adsr_on_gain_scales_to_silence() {
        let m = ModMatrix::build(&[route(4, "a_gain", 1.0)], &table(), &ENV4);
        assert_eq!(m.sources(), [LayerId(4)]);
        let base = [-3.0, 0.0, 1000.0];
        assert_eq!(run(&m, &[0.0], &base)[0], f32::NEG_INFINITY);
        assert!((run(&m, &[1.0], &base)[0] + 3.0).abs() < 1e-6);
        assert!((run(&m, &[0.5], &base)[0] - (-3.0 - 6.0206)).abs() < 1e-3);
        assert_eq!(m.factor(0, &[0.25]), 0.25);
        // Half the amount never goes silent.
        let half = ModMatrix::build(&[route(4, "a_gain", 0.5)], &table(), &ENV4);
        assert!((half.factor(0, &[0.0]) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_route_s_ends_are_the_knob_at_0_and_1() {
        let ends = |r: &ModRoute, base: f64, range: (f64, f64)| route_ends(r, base, range, false, Combine::Add);
        let r = route(4, "a_cents", 0.5);
        assert_eq!(ends(&r, 0.0, (-100.0, 100.0)), (0.0, 100.0));
        let r = ModRoute { bipolar: true, ..route(4, "a_cents", 0.25) };
        assert_eq!(ends(&r, 10.0, (-100.0, 100.0)), (-40.0, 60.0));
        let r = ModRoute { invert: true, ..route(4, "a_cents", 1.0) };
        assert_eq!(ends(&r, 0.0, (-100.0, 100.0)), (100.0, 0.0), "inverted runs the other way");
        let scale = |r: &ModRoute, base: f64| route_ends(r, base, (-60.0, 12.0), false, Combine::Scale);
        let (silent, full) = scale(&route(4, "a_gain", 1.0), -3.0);
        assert_eq!((silent, full), (-60.0, -3.0), "an envelope's ring runs from silence up to the knob");
        let (half, _) = scale(&route(4, "a_gain", 0.5), 0.0);
        assert!((half + 6.0206).abs() < 1e-3);
        // Any other modulator on a gain adds dB like any knob.
        assert_eq!(ends(&route(4, "a_gain", 0.25), -30.0, (-60.0, 12.0)), (-30.0, -12.0));
    }

    /// Where `v` sits along a knob over `range` (0 at the bottom, 1 at the top).
    fn turn(v: f64, range: (f64, f64), log: bool) -> f64 {
        if log { (v / range.0).ln() / (range.1 / range.0).ln() } else { (v - range.0) / (range.1 - range.0) }
    }

    #[test]
    fn turning_the_knob_slides_the_ring_without_changing_its_size() {
        // As in Serum: the amount is a share of the knob's travel, whatever the knob is set to.
        for (key, range, log) in [("a_cents", (-100.0f64, 100.0f64), false), ("cutoff", (20.0, 20_000.0), true)] {
            for bipolar in [false, true] {
                let r = ModRoute { bipolar, ..route(4, key, 0.2) };
                for t in [0.3, 0.4, 0.5, 0.6] {
                    let base =
                        if log { range.0 * (range.1 / range.0).powf(t) } else { range.0 + (range.1 - range.0) * t };
                    let (a, b) = route_ends(&r, base, range, log, Combine::Add);
                    let size = turn(b, range, log) - turn(a, range, log);
                    let want = if bipolar { 0.4 } else { 0.2 };
                    assert!((size - want).abs() < 1e-4, "{key} at {t}: {size}");
                    let from = turn(a, range, log);
                    let want_from = if bipolar { t - 0.2 } else { t };
                    assert!((from - want_from).abs() < 1e-4, "{key} starts at the knob");
                }
            }
        }
        // Near the top it stops at the knob's end instead of growing or shrinking the amount.
        let (a, b) = route_ends(&route(4, "cutoff", 0.5), 10_000.0, (20.0, 20_000.0), true, Combine::Add);
        assert!((a - 10_000.0).abs() < 0.5 && b == 20_000.0, "{a} {b}");
    }

    #[test]
    fn hz_knobs_turn_in_ratios_and_others_in_steps() {
        let def = |unit: &'static str, range: (f64, f64)| SettingDef {
            key: "k",
            label: "K",
            help: "",
            ty: bpe_core::ValueType::Number,
            default: bpe_core::Value::Number(range.0),
            range: Some(range),
            span: None,
            unit,
            group: "",
            depth: bpe_core::Depth::More,
            flags: Default::default(),
            when: "",
        };
        assert!(turns_in_ratios(&def("Hz", (20.0, 20_000.0))));
        assert!(!turns_in_ratios(&def("Hz", (0.0, 10.0))), "a range from 0 can't turn in ratios");
        assert!(!turns_in_ratios(&def("dB", (-60.0, 12.0))));
        assert!(!turns_in_ratios(&def("%", (0.0, 100.0))));
    }

    #[test]
    fn an_lfo_on_a_gain_adds_db_and_an_envelope_scales() {
        let lfo = ModMatrix::build(&[route(2, "a_gain", 0.25)], &table(), &ENV1);
        assert!(lfo.adds(0) && !lfo.route_from(0, 0, Combine::Scale));
        assert_eq!(lfo.factor(0, &[0.0]), 1.0, "it doesn't scale");
        // From silence a quarter of -60..12 dB is 18 dB up.
        assert!((lfo.offset(0, -60.0, &[1.0]) + 42.0).abs() < 1e-4);
        assert_eq!(lfo.offset(0, 0.0, &[1.0]), 12.0, "clamped to the top");
        assert_eq!(lfo.offset(0, -3.0, &[0.0]), -3.0);
        let both = ModMatrix::build(&[route(1, "a_gain", 1.0), route(2, "a_gain", 0.25)], &table(), &ENV1);
        assert_eq!(both.offset(0, -60.0, &[0.0, 0.5]), -51.0, "the envelope isn't part of the offset");
        assert_eq!(both.factor(0, &[0.5, 1.0]), 0.5, "the envelope still scales");
        let adding: Vec<_> = both.adding(0).collect();
        assert_eq!(adding, [(1, 18.0, false, false)]);
    }

    #[test]
    fn two_scaling_routes_multiply() {
        let m =
            ModMatrix::build(&[route(1, "a_gain", 1.0), route(2, "a_gain", 1.0)], &table(), &[LayerId(1), LayerId(2)]);
        assert!((m.factor(0, &[0.5, 0.5]) - 0.25).abs() < 1e-6);
        let db = run(&m, &[0.5, 0.5], &[0.0, 0.0, 20.0])[0];
        assert!((db + 12.041).abs() < 1e-2, "{db}");
    }

    #[test]
    fn add_routes_sum_and_clamp() {
        let m = ModMatrix::build(&[route(1, "a_cents", 0.25), route(2, "a_cents", 0.5)], &table(), &[]);
        let out = run(&m, &[1.0, 1.0], &[0.0, 10.0, 20.0]);
        assert_eq!(out[1], 100.0, "clamped to the top");
        let out = run(&m, &[0.2, 0.0], &[0.0, 10.0, 20.0]);
        assert!((out[1] - 20.0).abs() < 1e-5);
        // Log knobs add in ratios: a quarter of 20 Hz..20 kHz is a factor of ~5.6.
        let m = ModMatrix::build(&[route(1, "cutoff", 0.25)], &table(), &[]);
        let out = run(&m, &[1.0], &[0.0, 0.0, 100.0]);
        assert!((out[2] / 100.0 - 1000f32.powf(0.25)).abs() < 1e-2, "{}", out[2]);
    }

    #[test]
    fn bipolar_centres_on_the_knob() {
        let mut r = route(1, "a_cents", 0.5);
        r.bipolar = true;
        let m = ModMatrix::build(&[r], &table(), &[]);
        assert!((run(&m, &[0.5], &[0.0, 10.0, 20.0])[1] - 10.0).abs() < 1e-5);
        assert!((run(&m, &[0.0], &[0.0, 10.0, 20.0])[1] - (10.0 - 100.0)).abs() < 1e-5);
        assert!((run(&m, &[1.0], &[0.0, 10.0, 20.0])[1] - 100.0).abs() < 1e-5);
    }

    #[test]
    fn invert_flips() {
        let mut r = route(1, "a_gain", 1.0);
        r.invert = true;
        let m = ModMatrix::build(&[r], &table(), &ENV1);
        assert_eq!(m.factor(0, &[1.0]), 0.0);
        assert_eq!(m.factor(0, &[0.0]), 1.0);
        let mut r = route(1, "a_cents", 0.5);
        r.invert = true;
        let m = ModMatrix::build(&[r], &table(), &[]);
        assert!((run(&m, &[1.0], &[0.0, 10.0, 0.0])[1] - 10.0).abs() < 1e-5);
    }

    #[test]
    fn a_negative_amount_on_a_gain_works_the_other_way() {
        let down = ModMatrix::build(&[route(1, "a_gain", -0.5)], &table(), &ENV1);
        let up = ModMatrix::build(&[route(1, "a_gain", 0.5)], &table(), &ENV1);
        // At 1 the positive one leaves the gain; the negative one takes it half way to silence.
        assert!((run(&up, &[1.0], &[0.0, 0.0, 1000.0])[0]).abs() < 1e-4);
        let half = 20.0 * 0.5f32.log10();
        assert!((run(&down, &[1.0], &[0.0, 0.0, 1000.0])[0] - half).abs() < 1e-3);
        assert!((run(&down, &[0.0], &[0.0, 0.0, 1000.0])[0]).abs() < 1e-4);
        assert!((down.factor(0, &[1.0]) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn combine_of_names_the_vca_knobs() {
        for k in ["a_gain", "b_gain", "a_level", "b_level", "a_sub_level", "out_volume"] {
            assert!(is_level_key(k), "{k}");
            assert_eq!(combine_of(k, true), Combine::Scale, "{k}");
            assert_eq!(combine_of(k, false), Combine::Add, "{k} from an LFO");
        }
        for k in ["a_cents", "a_warp", "out_pan", "3/cutoff", "3/output", "play_glide"] {
            assert_eq!(combine_of(k, true), Combine::Add, "{k}");
        }
    }

    #[test]
    fn shares_on_a_frequency_knob_add_up_before_it_clamps() {
        // Down past the bottom then back up lands where the shares sum to, whatever the order.
        let a = ModMatrix::build(&[route(1, "cutoff", -0.5), route(2, "cutoff", 0.3)], &table(), &[]);
        let b = ModMatrix::build(&[route(2, "cutoff", 0.3), route(1, "cutoff", -0.5)], &table(), &[]);
        let want = 200.0 * 1000f32.powf(-0.2);
        for m in [a, b] {
            let got = run(&m, &[1.0, 1.0], &[0.0, 0.0, 200.0])[2];
            assert!((got - want).abs() < 0.05, "{got} against {want}");
        }
    }

    #[test]
    fn apply_clamps_a_level_before_its_envelope_scales_it() {
        // As the sound does: an LFO's 18 dB stop at the 12 dB top, then the envelope takes 6 dB.
        let m = ModMatrix::build(&[route(1, "a_gain", 1.0), route(2, "a_gain", 0.25)], &table(), &ENV1);
        let got = run(&m, &[0.5, 1.0], &[0.0, 0.0, 1000.0])[0];
        assert!((got - (12.0 + 20.0 * 0.5f32.log10())).abs() < 1e-3, "{got}");
    }

    #[test]
    fn routes_to_missing_knobs_are_left_out() {
        let m = ModMatrix::build(&[route(1, "nothing", 1.0), route(1, "a_gain", 2.0)], &table(), &[]);
        assert!(m.is_empty());
    }
}

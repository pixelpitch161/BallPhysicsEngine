//! Modulation routes, worked out: which modulator moves which knob by how much. Most knobs add
//! their modulations (in the knob's own feel); the gain and level knobs scale instead, each
//! route a VCA of its own, so an envelope at 0 with amount 1 is silence.

use bpe_core::{LayerId, ModRoute};
use std::collections::BTreeMap;

/// How a knob takes its modulations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Combine {
    /// Each route adds a share of the knob's range.
    Add,
    /// Each route scales the level (a VCA): dB knobs.
    Scale,
}

/// How the knob at `key` takes modulations: the oscillators' Gain, Level and Sub level and the
/// Output volume scale, everything else adds.
pub fn combine_of(key: &str) -> Combine {
    let osc = key.starts_with("a_") || key.starts_with("b_");
    let vca = osc && (key.ends_with("_gain") || key.ends_with("_level"));
    if vca || key == "out_volume" { Combine::Scale } else { Combine::Add }
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

/// A voice's routes, ready to apply each block.
#[derive(Clone, Debug, Default)]
pub struct ModMatrix {
    routes: Vec<Route>,
    sources: Vec<LayerId>,
    targets: usize,
}

impl ModMatrix {
    /// Works out `routes` against `targets`; a route to a knob that isn't there, or with a
    /// number out of range, is left out.
    pub fn build(routes: &[ModRoute], targets: &TargetTable) -> ModMatrix {
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
            m.routes.push(Route {
                source,
                target: t.index,
                amount: r.amount,
                bipolar: r.bipolar,
                invert: r.invert,
                combine: combine_of(&r.target),
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
        // Adding knobs first gather their sums in the knob's own feel, then clamp.
        for r in &self.routes {
            let (t, s) = (r.target as usize, sources.get(r.source as usize).copied().unwrap_or(0.0));
            let m = if r.invert { 1.0 - s } else { s };
            match r.combine {
                Combine::Add => {
                    let m = if r.bipolar { 2.0 * m - 1.0 } else { m };
                    if r.log {
                        let span = (r.hi / r.lo).ln();
                        let v = (out[t].max(r.lo) / r.lo).ln() + r.amount * m * span;
                        out[t] = r.lo * v.exp();
                    } else {
                        out[t] += r.amount * m * (r.hi - r.lo);
                    }
                }
                Combine::Scale => {
                    let f = 1.0 - r.amount.abs() * (1.0 - m.clamp(0.0, 1.0));
                    out[t] += 20.0 * f.max(0.0).log10();
                }
            }
        }
        for r in self.routes.iter().filter(|r| r.combine == Combine::Add) {
            let t = r.target as usize;
            out[t] = out[t].clamp(r.lo, r.hi);
        }
    }

    /// The linear gain the scaling routes on `target` multiply its level by, with the
    /// modulators at `sources` (for working per sample).
    #[inline]
    pub fn factor(&self, target: u16, sources: &[f32]) -> f32 {
        let mut f = 1.0;
        for r in &self.routes {
            if r.target == target && r.combine == Combine::Scale {
                let s = sources.get(r.source as usize).copied().unwrap_or(0.0);
                let m = if r.invert { 1.0 - s } else { s };
                f *= (1.0 - r.amount.abs() * (1.0 - m.clamp(0.0, 1.0))).max(0.0);
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

    /// How many knob values `apply` reads and writes.
    pub fn targets(&self) -> usize {
        self.targets
    }
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
        let m = ModMatrix::build(&[route(4, "a_gain", 1.0)], &table());
        assert_eq!(m.sources(), [LayerId(4)]);
        let base = [-3.0, 0.0, 1000.0];
        assert_eq!(run(&m, &[0.0], &base)[0], f32::NEG_INFINITY);
        assert!((run(&m, &[1.0], &base)[0] + 3.0).abs() < 1e-6);
        assert!((run(&m, &[0.5], &base)[0] - (-3.0 - 6.0206)).abs() < 1e-3);
        assert_eq!(m.factor(0, &[0.25]), 0.25);
        // Half the amount never goes silent.
        let half = ModMatrix::build(&[route(4, "a_gain", 0.5)], &table());
        assert!((half.factor(0, &[0.0]) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn two_scaling_routes_multiply() {
        let m = ModMatrix::build(&[route(1, "a_gain", 1.0), route(2, "a_gain", 1.0)], &table());
        assert!((m.factor(0, &[0.5, 0.5]) - 0.25).abs() < 1e-6);
        let db = run(&m, &[0.5, 0.5], &[0.0, 0.0, 20.0])[0];
        assert!((db + 12.041).abs() < 1e-2, "{db}");
    }

    #[test]
    fn add_routes_sum_and_clamp() {
        let m = ModMatrix::build(&[route(1, "a_cents", 0.25), route(2, "a_cents", 0.5)], &table());
        let out = run(&m, &[1.0, 1.0], &[0.0, 10.0, 20.0]);
        assert_eq!(out[1], 100.0, "clamped to the top");
        let out = run(&m, &[0.2, 0.0], &[0.0, 10.0, 20.0]);
        assert!((out[1] - 20.0).abs() < 1e-5);
        // Log knobs add in ratios: a quarter of 20 Hz..20 kHz is a factor of ~5.6.
        let m = ModMatrix::build(&[route(1, "cutoff", 0.25)], &table());
        let out = run(&m, &[1.0], &[0.0, 0.0, 100.0]);
        assert!((out[2] / 100.0 - 1000f32.powf(0.25)).abs() < 1e-2, "{}", out[2]);
    }

    #[test]
    fn bipolar_centres_on_the_knob() {
        let mut r = route(1, "a_cents", 0.5);
        r.bipolar = true;
        let m = ModMatrix::build(&[r], &table());
        assert!((run(&m, &[0.5], &[0.0, 10.0, 20.0])[1] - 10.0).abs() < 1e-5);
        assert!((run(&m, &[0.0], &[0.0, 10.0, 20.0])[1] - (10.0 - 100.0)).abs() < 1e-5);
        assert!((run(&m, &[1.0], &[0.0, 10.0, 20.0])[1] - 100.0).abs() < 1e-5);
    }

    #[test]
    fn invert_flips() {
        let mut r = route(1, "a_gain", 1.0);
        r.invert = true;
        let m = ModMatrix::build(&[r], &table());
        assert_eq!(m.factor(0, &[1.0]), 0.0);
        assert_eq!(m.factor(0, &[0.0]), 1.0);
        let mut r = route(1, "a_cents", 0.5);
        r.invert = true;
        let m = ModMatrix::build(&[r], &table());
        assert!((run(&m, &[1.0], &[0.0, 10.0, 0.0])[1] - 10.0).abs() < 1e-5);
    }

    #[test]
    fn combine_of_names_the_vca_knobs() {
        for k in ["a_gain", "b_gain", "a_level", "b_level", "a_sub_level", "out_volume"] {
            assert_eq!(combine_of(k), Combine::Scale, "{k}");
        }
        for k in ["a_cents", "a_warp", "out_pan", "3/cutoff", "play_glide"] {
            assert_eq!(combine_of(k), Combine::Add, "{k}");
        }
    }

    #[test]
    fn routes_to_missing_knobs_are_left_out() {
        let m = ModMatrix::build(&[route(1, "nothing", 1.0), route(1, "a_gain", 2.0)], &table());
        assert!(m.is_empty());
    }
}

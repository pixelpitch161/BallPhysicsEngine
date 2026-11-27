//! What the physics of a Water scene reads, described part by part (part 8), so two setups can say
//! the earliest water time they differ at: a final made for one is good up to there for the other,
//! and Make final remakes only the tail after it.
//!
//! Each part says from when it can first change the water (`from`: 0, or a tap's start less three
//! frames, as a tap takes no part before it pours but for the frame before it starts, which steps
//! for its speed), a hash of what it holds that no keyframe moves, and each keyframed setting's
//! keys. A keyframed setting whose keys differ changes the water from the key before the first that
//! differs (the curve up to that key is the same), or from the part's `from` when the first
//! differs. Taps are told apart by their place in the list, as the engine keeps each tap's progress
//! by its place.

use std::hash::{DefaultHasher, Hash, Hasher};

use serde::{Deserialize, Serialize};

use super::{Keyed, PourKind, WaterSetup, world_keys};
use crate::modules::water::PourCard;

/// A tap takes part from this long before it starts (three frames).
pub const TAP_LEAD: f64 = 3.0 / 60.0;

/// What the physics reads, part by part.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WaterInputs {
    pub parts: Vec<InputPart>,
}

/// One part of what the physics reads.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InputPart {
    pub name: String,
    /// The earliest water time (seconds) it can change the water.
    pub from: f64,
    /// What no keyframe moves.
    pub fixed: u64,
    /// Each keyframed setting's keys: (time, hash of the key).
    pub keys: Vec<(String, Vec<(f64, u64)>)>,
}

impl Keyed {
    /// The settings named in `keys` (all when `None`): a hash of those no keyframe moves, and each
    /// keyframed one's keys.
    fn describe(&self, keys: Option<&[&str]>, h: &mut DefaultHasher) -> Vec<(String, Vec<(f64, u64)>)> {
        let wanted = |k: &str| keys.is_none_or(|keys| keys.contains(&k));
        let keyed = |k: &str| self.tracks.iter().any(|(t, _)| t == k);
        for (k, v) in self.params.0.iter().filter(|(k, _)| wanted(k) && !keyed(k)) {
            format!("{k}={v:?}").hash(h);
        }
        self.tracks
            .iter()
            .filter(|(k, _)| wanted(k))
            .map(|(k, t)| {
                let list = t
                    .keys
                    .iter()
                    .map(|key| {
                        let mut kh = DefaultHasher::new();
                        format!("{:?}|{:?}|{:?}", key.value, key.ease, t.range).hash(&mut kh);
                        (key.time, kh.finish())
                    })
                    .collect();
                (k.clone(), list)
            })
            .collect()
    }
}

/// A part from the parts of what it holds.
fn part(name: String, from: f64, fill: impl FnOnce(&mut DefaultHasher) -> Vec<(String, Vec<(f64, u64)>)>) -> InputPart {
    let mut h = DefaultHasher::new();
    let mut keys = fill(&mut h);
    keys.sort_by(|a, b| a.0.cmp(&b.0));
    InputPart { name, from, fixed: h.finish(), keys }
}

impl WaterSetup {
    /// What this setup's physics reads, part by part.
    pub fn inputs(&self) -> WaterInputs {
        let detail_key = match self.detail {
            super::Detail::Draft => "draft_detail",
            super::Detail::Final => "final_detail",
        };
        let mut parts = vec![
            part("world".into(), 0.0, |h| {
                (self.cell.to_bits(), self.size.x.to_bits(), self.size.y.to_bits()).hash(h);
                format!("{:?}", self.numbers()).hash(h);
                self.world.describe(Some(&world_keys(detail_key)), h)
            }),
            part("course".into(), 0.0, |h| {
                self.strokes.hash64().hash(h);
                for s in &self.solids {
                    (s.id, s.kind, s.at.x.to_bits(), s.at.y.to_bits(), s.turn.to_bits()).hash(h);
                    s.form.hash_into(None, h);
                    s.material.hash_into(Some(&["wetting"]), h);
                }
                Vec::new()
            }),
            part("rock".into(), 0.0, |h| self.rock.describe(Some(&["wetting"]), h)),
            part("fills".into(), 0.0, |h| {
                for p in self.pours.iter().filter(|p| p.kind == PourKind::FillAtTheStart) {
                    (p.id, p.liquid, p.place.x.to_bits(), p.place.y.to_bits()).hash(h);
                    p.card.hash_into(None, h);
                }
                Vec::new()
            }),
        ];
        for (k, l) in self.liquids.iter().enumerate() {
            parts.push(part(format!("liquid:{k}"), 0.0, |h| {
                l.id.hash(h);
                let mut keys = l.feel.describe(None, h);
                l.mixing.is_some().hash(h);
                if let Some(m) = &l.mixing {
                    let mixing = m.describe(Some(&super::MIXING_KEYS), h);
                    keys.extend(mixing.into_iter().map(|(key, list)| (format!("mixing/{key}"), list)));
                }
                keys
            }));
        }
        parts.push(part("drains".into(), 0.0, |h| {
            let mut keys = Vec::new();
            for (k, d) in self.drains.iter().enumerate() {
                (d.id, d.place.x.to_bits(), d.place.y.to_bits()).hash(h);
                keys.extend(d.card.describe(None, h).into_iter().map(|(key, list)| (format!("{k}/{key}"), list)));
            }
            keys
        }));
        // (Rain, drips and bursts keep nothing from frame to frame: each is told apart by its own id.)
        let lumps = self
            .pours
            .iter()
            .filter(|p| matches!(p.kind, PourKind::Rain | PourKind::Drip | PourKind::Burst) && p.liquid.is_some());
        for p in lumps {
            parts.push(part(format!("lumps:{}", p.id.0), lead_from(p), |h| {
                (p.id, p.liquid, p.kind, p.place.x.to_bits(), p.place.y.to_bits()).hash(h);
                p.card.describe(None, h)
            }));
        }
        let taps = self.pours.iter().filter(|p| p.kind == PourKind::Tap && p.liquid.is_some());
        for (k, p) in taps.enumerate() {
            parts.push(part(format!("tap:{k}"), lead_from(p), |h| {
                (p.id, p.liquid, p.place.x.to_bits(), p.place.y.to_bits()).hash(h);
                p.card.describe(None, h)
            }));
        }
        WaterInputs { parts }
    }
}

/// From when pour `p` can first change the water: its Starts less [`TAP_LEAD`]. (A pour keyed to
/// start later still reads its Starts at the time: it may start at 0.)
fn lead_from(p: &super::PourSetup) -> f64 {
    let starts = p.card.card::<PourCard>(0.0).starts;
    let keyed = p.card.tracks.iter().any(|(key, _)| key == "starts");
    if keyed || !starts.is_finite() { 0.0 } else { (f64::from(starts) - TAP_LEAD).max(0.0) }
}

impl WaterInputs {
    /// The earliest water time (seconds) at which the water these inputs make can differ from what
    /// `other`'s make; `None` when they make the same water.
    pub fn earliest_change(&self, other: &WaterInputs) -> Option<f64> {
        let mut earliest: Option<f64> = None;
        let mut at = |t: f64| earliest = Some(earliest.map_or(t, |e| e.min(t)));
        for a in &self.parts {
            match other.parts.iter().find(|b| b.name == a.name) {
                None => at(a.from),
                Some(b) => {
                    if let Some(t) = a.changes_from(b) {
                        at(t);
                    }
                }
            }
        }
        for b in &other.parts {
            if !self.parts.iter().any(|a| a.name == b.name) {
                at(b.from);
            }
        }
        earliest
    }
}

impl InputPart {
    fn changes_from(&self, other: &InputPart) -> Option<f64> {
        let from = self.from.min(other.from);
        if self.fixed != other.fixed || self.from != other.from {
            return Some(from);
        }
        let mut earliest: Option<f64> = None;
        let mut names: Vec<&str> = self.keys.iter().chain(&other.keys).map(|(n, _)| n.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        let empty: Vec<(f64, u64)> = Vec::new();
        for name in names {
            let find = |keys: &'_ [(String, Vec<(f64, u64)>)]| keys.iter().position(|(n, _)| n == name);
            let a = find(&self.keys).map_or(empty.as_slice(), |i| self.keys[i].1.as_slice());
            let b = find(&other.keys).map_or(empty.as_slice(), |i| other.keys[i].1.as_slice());
            if a == b {
                continue;
            }
            let same = a.iter().zip(b).take_while(|(x, y)| x == y).count();
            let t = if same == 0 || a.is_empty() || b.is_empty() { from } else { a[same - 1].0.max(from) };
            earliest = Some(earliest.map_or(t, |e: f64| e.min(t)));
        }
        earliest
    }
}

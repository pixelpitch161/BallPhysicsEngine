//! Keyframes and pulses: settings that change over time, named by address, and the
//! [`Animation`] that writes their values into a resolved scene at any moment.
//!
//! A track holds keys for one setting; between keys the value eases from one to the next. A
//! pulse adds to a setting with the beat, a band of the song, or the hits. Both are stored on
//! the scene by [`Address`], so they keep pointing at the right setting when things are renamed
//! or reordered. Nothing here needs the registry once the keys are in: the editor checks them
//! when they are made, and the runtime, the renderer and the mixer only read them.

use crate::{
    Address, Color, EditError, Owner, Registry, Resolved, Scene, SettingDef, SlotId, SlotLink, ThingId, Value,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Keys closer together than this many seconds are the same key.
pub const SAME_TIME: f64 = 0.004;

/// How a value moves from one key to the next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Ease {
    #[default]
    Smooth,
    Linear,
    EaseIn,
    EaseOut,
    /// Jumps to the next value when its key is reached.
    Hold,
    /// Goes a little past the next value and settles back.
    Overshoot,
    /// Lands on the next value with a few bounces.
    Bounce,
    /// A cubic curve of the user's own, through (0, 0), (x1, y1), (x2, y2) and (1, 1).
    Curve {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
    },
}

impl Ease {
    /// Every ease but the user's own curve, in menu order.
    pub const NAMED: [Ease; 7] =
        [Ease::Smooth, Ease::Linear, Ease::EaseIn, Ease::EaseOut, Ease::Hold, Ease::Overshoot, Ease::Bounce];

    pub fn name(&self) -> &'static str {
        match self {
            Ease::Smooth => "Smooth",
            Ease::Linear => "Linear",
            Ease::EaseIn => "Ease in",
            Ease::EaseOut => "Ease out",
            Ease::Hold => "Hold",
            Ease::Overshoot => "Overshoot",
            Ease::Bounce => "Bounce",
            Ease::Curve { .. } => "Own curve",
        }
    }

    /// Where the value is, 0 to 1 (a little beyond for Overshoot and some curves), when `t` of
    /// the way from one key to the next.
    pub fn apply(&self, t: f32) -> f32 {
        let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 1.0 };
        match *self {
            Ease::Smooth => t * t * (3.0 - 2.0 * t),
            Ease::Linear => t,
            Ease::EaseIn => t * t,
            Ease::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
            Ease::Hold => {
                if t >= 1.0 {
                    1.0
                } else {
                    0.0
                }
            }
            Ease::Overshoot => {
                let c = 1.70158;
                let u = t - 1.0;
                1.0 + (c + 1.0) * u * u * u + c * u * u
            }
            Ease::Bounce => bounce_out(t),
            Ease::Curve { x1, y1, x2, y2 } => {
                bezier(x1.clamp(0.0, 1.0), y1.clamp(-0.5, 1.5), x2.clamp(0.0, 1.0), y2.clamp(-0.5, 1.5), t)
            }
        }
    }
}

fn bounce_out(t: f32) -> f32 {
    let (n1, d1) = (7.5625, 2.75);
    if t < 1.0 / d1 {
        n1 * t * t
    } else if t < 2.0 / d1 {
        let t = t - 1.5 / d1;
        n1 * t * t + 0.75
    } else if t < 2.5 / d1 {
        let t = t - 2.25 / d1;
        n1 * t * t + 0.9375
    } else {
        let t = t - 2.625 / d1;
        n1 * t * t + 0.984375
    }
}

/// The y of a cubic Bézier through (0,0), (x1,y1), (x2,y2), (1,1) at the point whose x is `x`.
fn bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    let along = |a: f32, b: f32, s: f32| 3.0 * (1.0 - s) * (1.0 - s) * s * a + 3.0 * (1.0 - s) * s * s * b + s * s * s;
    let slope = |a: f32, b: f32, s: f32| {
        3.0 * (1.0 - s) * (1.0 - s) * a + 6.0 * (1.0 - s) * s * (b - a) + 3.0 * s * s * (1.0 - b)
    };
    // Newton's method from s = x, falling back to bisection where the curve is flat.
    let mut s = x;
    for _ in 0..8 {
        let d = slope(x1, x2, s);
        if d.abs() < 1e-6 {
            break;
        }
        s = (s - (along(x1, x2, s) - x) / d).clamp(0.0, 1.0);
    }
    if (along(x1, x2, s) - x).abs() > 1e-4 {
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..30 {
            s = (lo + hi) * 0.5;
            if along(x1, x2, s) < x {
                lo = s;
            } else {
                hi = s;
            }
        }
    }
    along(y1, y2, s)
}

/// One keyframe: the value a setting has at `time`. The ease shapes the change from this key
/// to the next.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Key {
    pub time: f64,
    pub value: Value,
    #[serde(default)]
    pub ease: Ease,
}

/// Every key of one setting, earliest first.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Track {
    pub keys: Vec<Key>,
    /// The setting's range, copied from its description when the first key was made, so eases
    /// that overshoot stay inside it.
    pub range: Option<(f64, f64)>,
}

impl Track {
    /// The value at `time`: the first key's before it, the last key's after it, eased between.
    /// `None` for an empty track.
    pub fn value_at(&self, time: f64) -> Option<Value> {
        let keys = &self.keys;
        let first = keys.first()?;
        if time <= first.time {
            return Some(self.clamp(first.value.clone()));
        }
        let i = keys.partition_point(|k| k.time <= time);
        if i >= keys.len() {
            return Some(self.clamp(keys[keys.len() - 1].value.clone()));
        }
        let (a, b) = (&keys[i - 1], &keys[i]);
        let span = b.time - a.time;
        let t = if span <= 0.0 { 1.0 } else { ((time - a.time) / span) as f32 };
        Some(self.clamp(a.value.lerp(&b.value, a.ease.apply(t))))
    }

    fn clamp(&self, v: Value) -> Value {
        clamp_to(v, self.range)
    }

    /// The key nearest `time` if it is within `within` seconds.
    pub fn key_near(&self, time: f64, within: f64) -> Option<usize> {
        self.keys
            .iter()
            .enumerate()
            .filter(|(_, k)| (k.time - time).abs() <= within)
            .min_by(|a, b| (a.1.time - time).abs().total_cmp(&(b.1.time - time).abs()))
            .map(|(i, _)| i)
    }

    /// Puts a key at `time`, replacing the value of one already there (which keeps its ease).
    /// Returns the key's index.
    pub fn set_key(&mut self, time: f64, value: Value) -> usize {
        let time = time.max(0.0);
        if let Some(i) = self.key_near(time, SAME_TIME) {
            self.keys[i].value = value;
            return i;
        }
        let i = self.keys.partition_point(|k| k.time <= time);
        self.keys.insert(i, Key { time, value, ease: Ease::default() });
        i
    }

    /// Moves key `index` to `time` (never before 0), keeping the keys in order. Returns its
    /// new index.
    pub fn move_key(&mut self, index: usize, time: f64) -> usize {
        if index >= self.keys.len() {
            return index;
        }
        let mut key = self.keys.remove(index);
        key.time = time.max(0.0);
        let i = self.keys.partition_point(|k| k.time <= key.time);
        self.keys.insert(i, key);
        i
    }

    pub fn remove_key(&mut self, index: usize) {
        if index < self.keys.len() {
            self.keys.remove(index);
        }
    }
}

fn clamp_to(v: Value, range: Option<(f64, f64)>) -> Value {
    match (v, range) {
        (Value::Number(x), Some((min, max))) => Value::Number(x.clamp(min, max)),
        (Value::Int(x), Some((min, max))) => Value::Int((x as f64).clamp(min, max).round() as i64),
        (v, _) => v,
    }
}

impl Value {
    /// `t` of the way from this value to `other`: numbers and points move straight, colours
    /// blend in linear light, anything else switches over at the end.
    pub fn lerp(&self, other: &Value, t: f32) -> Value {
        let t64 = t as f64;
        match (self, other) {
            (Value::Number(a), Value::Number(b)) => Value::Number(a + (b - a) * t64),
            (Value::Int(a), Value::Int(b)) => Value::Int((*a as f64 + (*b - *a) as f64 * t64).round() as i64),
            (Value::Vec2(a), Value::Vec2(b)) => Value::Vec2([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]),
            (Value::Color(a), Value::Color(b)) => Value::Color(Color::mix(*a, *b, t)),
            _ => {
                if t < 1.0 {
                    self.clone()
                } else {
                    other.clone()
                }
            }
        }
    }
}

/// What a pulse follows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PulseSource {
    #[default]
    Beat,
    Bass,
    Mid,
    Treble,
    Level,
    /// The bounces. Never allowed on a setting that moves the physics.
    Hits,
}

impl PulseSource {
    pub const NAMED: [PulseSource; 6] = [
        PulseSource::Beat,
        PulseSource::Bass,
        PulseSource::Mid,
        PulseSource::Treble,
        PulseSource::Level,
        PulseSource::Hits,
    ];

    pub fn name(self) -> &'static str {
        match self {
            PulseSource::Beat => "Beat",
            PulseSource::Bass => "Bass",
            PulseSource::Mid => "Mid",
            PulseSource::Treble => "Treble",
            PulseSource::Level => "Level",
            PulseSource::Hits => "Hits",
        }
    }
}

/// A setting that moves with the music or the hits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pulse {
    pub source: PulseSource,
    /// What is added at full strength (numbers and points), or the colour mixed towards.
    pub amount: Value,
    /// Seconds a beat's pulse takes to die away.
    pub decay: f32,
    /// The setting's range, so a pulse never pushes it out.
    pub range: Option<(f64, f64)>,
}

impl Default for Pulse {
    fn default() -> Self {
        Self { source: PulseSource::Beat, amount: Value::Number(0.0), decay: 0.25, range: None }
    }
}

/// A song's loudness in bands over time, `rate` frames a second, each 0 to 1.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bands {
    pub rate: f32,
    pub bass: Vec<f32>,
    pub mid: Vec<f32>,
    pub treble: Vec<f32>,
    pub level: Vec<f32>,
}

impl Bands {
    /// Bass, mid, treble and level at `time`, blended between frames; all 0 outside the song.
    pub fn at(&self, time: f64) -> [f32; 4] {
        let n = self.level.len();
        if n == 0 || !time.is_finite() || time < 0.0 || self.rate <= 0.0 {
            return [0.0; 4];
        }
        let x = time * self.rate as f64;
        let i = x.floor() as usize;
        if i >= n {
            return [0.0; 4];
        }
        let j = (i + 1).min(n - 1);
        let t = (x - i as f64) as f32;
        let lerp = |v: &[f32]| if v.len() == n { v[i] + (v[j] - v[i]) * t } else { 0.0 };
        [lerp(&self.bass), lerp(&self.mid), lerp(&self.treble), lerp(&self.level)]
    }
}

/// What pulses follow: the beat times, and the song's bands when there is a song.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PulseInputs {
    pub beats: Vec<f64>,
    pub bands: Option<Bands>,
}

/// Everything a pulse can read at one moment.
#[derive(Clone, Copy, Debug, Default)]
pub struct Inputs<'a> {
    pub pulse: Option<&'a PulseInputs>,
    /// How hard things are hitting right now, 0 to 1. The runtime always passes 0.
    pub hits: f32,
}

impl Inputs<'_> {
    pub const NONE: Inputs<'static> = Inputs { pulse: None, hits: 0.0 };
}

/// How strongly `pulse` pushes at `time`, 0 to 1.
pub fn pulse_strength(pulse: &Pulse, time: f64, inputs: &Inputs) -> f32 {
    let s = match pulse.source {
        PulseSource::Hits => inputs.hits,
        PulseSource::Beat => {
            let beats = inputs.pulse.map(|p| p.beats.as_slice()).unwrap_or(&[]);
            match beats.partition_point(|&b| b <= time).checked_sub(1) {
                Some(i) => (-((time - beats[i]) / pulse.decay.max(0.01) as f64)).exp() as f32,
                None => 0.0,
            }
        }
        band => {
            let bands = inputs.pulse.and_then(|p| p.bands.as_ref());
            let at = bands.map(|b| b.at(time)).unwrap_or([0.0; 4]);
            match band {
                PulseSource::Bass => at[0],
                PulseSource::Mid => at[1],
                PulseSource::Treble => at[2],
                _ => at[3],
            }
        }
    };
    if s.is_finite() { s.clamp(0.0, 1.0) } else { 0.0 }
}

/// `base` pushed by `pulse` at strength `s`.
fn pulsed(base: &Value, pulse: &Pulse, s: f32) -> Value {
    let v = match (base, &pulse.amount) {
        (Value::Number(b), Value::Number(a)) => Value::Number(b + a * s as f64),
        (Value::Int(b), Value::Int(a)) => Value::Int((*b as f64 + *a as f64 * s as f64).round() as i64),
        (Value::Int(b), Value::Number(a)) => Value::Int((*b as f64 + a * s as f64).round() as i64),
        (Value::Vec2(b), Value::Vec2(a)) => Value::Vec2([b[0] + a[0] * s, b[1] + a[1] * s]),
        (Value::Color(b), Value::Color(a)) => Value::Color(Color::mix(*b, *a, s)),
        _ => base.clone(),
    };
    clamp_to(v, pulse.range)
}

impl Scene {
    /// Checks that `a` names a setting that can be written here, like `set_at` does, and that
    /// the setting has the `keyframe` (or `pulse`) flag.
    fn animatable(&self, reg: &Registry, a: &Address, pulse: bool) -> Result<&'static SettingDef, EditError> {
        match a.owner {
            Owner::Thing(id) if !self.things.contains_key(&id) => return Err(EditError::Missing(id)),
            Owner::Module(m) if !self.library.modules.contains_key(&m) => return Err(EditError::MissingModule(m)),
            _ => {}
        }
        if let (Owner::Thing(id), Some(slot)) = (a.owner, a.slot) {
            match self.things[&id].slots.get(&slot) {
                Some(SlotLink::Linked { module, .. }) if self.library.modules.contains_key(module) => {}
                Some(SlotLink::Linked { module, .. }) => return Err(EditError::MissingModule(*module)),
                _ => return Err(EditError::NotLinked),
            }
        }
        let def = self.def_at(reg, a).ok_or_else(|| EditError::UnknownSetting(a.key.clone()))?;
        let allowed = if pulse { def.flags.pulse } else { def.flags.keyframe };
        if !allowed {
            return Err(EditError::UnknownSetting(a.key.clone()));
        }
        Ok(def)
    }

    /// Puts a key on the setting at `a`. The setting must have the keyframe flag and `value`
    /// must be its kind of value; it is pulled into range. Returns the key's index.
    pub fn set_key(&mut self, reg: &Registry, a: &Address, time: f64, value: Value) -> Result<usize, EditError> {
        let def = self.animatable(reg, a, false)?;
        if !def.ty.accepts(&value) {
            return Err(EditError::WrongType);
        }
        let value = def.clamp(value);
        let track = self.tracks.entry(a.clone()).or_default();
        track.range = def.range;
        Ok(track.set_key(time, value))
    }

    /// Removes key `index` of the track at `a`; an emptied track goes too. False if there was
    /// no such key.
    pub fn remove_key(&mut self, a: &Address, index: usize) -> bool {
        let Some(track) = self.tracks.get_mut(a) else { return false };
        if index >= track.keys.len() {
            return false;
        }
        track.remove_key(index);
        if track.keys.is_empty() {
            self.tracks.remove(a);
        }
        true
    }

    /// Moves key `index` of the track at `a` to `time`. Returns its new index.
    pub fn move_key(&mut self, a: &Address, index: usize, time: f64) -> Option<usize> {
        let track = self.tracks.get_mut(a)?;
        (index < track.keys.len()).then(|| track.move_key(index, time))
    }

    /// Sets how key `index` of the track at `a` eases into the next key.
    pub fn set_ease(&mut self, a: &Address, index: usize, ease: Ease) -> bool {
        match self.tracks.get_mut(a).and_then(|t| t.keys.get_mut(index)) {
            Some(key) => {
                key.ease = ease;
                true
            }
            None => false,
        }
    }

    /// Makes the setting at `a` pulse. The setting must have the pulse flag.
    pub fn set_pulse(
        &mut self,
        reg: &Registry,
        a: &Address,
        source: PulseSource,
        amount: Value,
        decay: f32,
    ) -> Result<(), EditError> {
        let def = self.animatable(reg, a, true)?;
        if !def.ty.accepts(&amount) && !matches!((&def.ty, &amount), (crate::ValueType::Int, Value::Number(_))) {
            return Err(EditError::WrongType);
        }
        self.pulses.insert(a.clone(), Pulse { source, amount, decay: decay.max(0.01), range: def.range });
        Ok(())
    }

    pub fn remove_pulse(&mut self, a: &Address) -> bool {
        self.pulses.remove(a).is_some()
    }

    /// Whether the setting at `a` has keys.
    pub fn keyed(&self, a: &Address) -> bool {
        self.tracks.get(a).is_some_and(|t| !t.keys.is_empty())
    }

    /// The value the setting at `a` has at `time`: its track's when it has keys, else its
    /// plain value.
    pub fn value_at_time(&self, reg: &Registry, a: &Address, time: f64) -> Option<Value> {
        self.tracks.get(a).and_then(|t| t.value_at(time)).or_else(|| self.value_at(reg, a))
    }
}

/// One place an animated setting lands in a resolved scene.
#[derive(Clone, Debug)]
struct Target {
    thing: ThingId,
    slot: Option<SlotId>,
    key: String,
    track: Option<Track>,
    pulse: Option<Pulse>,
}

/// A scene's tracks and pulses, worked out to where each one lands in the resolved scene: a
/// module's track lands on every thing that uses the module, unless the thing overrides that
/// setting; a thing's override track lands on the thing and whatever inherits the slot from it.
#[derive(Clone, Debug, Default)]
pub struct Animation {
    /// Module-owned targets first, then things', so an override wins where both land.
    targets: Vec<Target>,
}

impl Animation {
    pub fn new(scene: &Scene, resolved: &Resolved) -> Animation {
        let addresses: BTreeSet<&Address> = scene.tracks.keys().chain(scene.pulses.keys()).collect();
        let mut targets = Vec::new();
        let of_module = |a: &Address| matches!(a.owner, Owner::Module(_));
        let modules = addresses.iter().copied().filter(|a| of_module(a));
        let things = addresses.iter().copied().filter(|a| !of_module(a));
        for a in modules.chain(things) {
            let track = scene.tracks.get(a).filter(|t| !t.keys.is_empty()).cloned();
            let pulse = scene.pulses.get(a).cloned();
            if track.is_none() && pulse.is_none() {
                continue;
            }
            for (thing, slot) in lands_on(scene, resolved, a) {
                targets.push(Target { thing, slot, key: a.key.clone(), track: track.clone(), pulse: pulse.clone() });
            }
        }
        Animation { targets }
    }

    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    /// Every (thing, slot, setting) the animation writes, each once, in order.
    pub fn destinations(&self) -> Vec<(ThingId, Option<SlotId>, &str)> {
        let mut out: Vec<(ThingId, Option<SlotId>, &str)> =
            self.targets.iter().map(|t| (t.thing, t.slot, t.key.as_str())).collect();
        out.sort();
        out.dedup();
        out
    }

    /// Keeps only the targets `keep` says yes to.
    pub fn retain(&mut self, mut keep: impl FnMut(ThingId, Option<SlotId>, &str) -> bool) {
        self.targets.retain(|t| keep(t.thing, t.slot, &t.key));
    }

    /// Writes every animated setting's value at `time` into `resolved`: the keyed value, with
    /// the pulse added on top.
    pub fn apply(&self, resolved: &mut Resolved, time: f64, inputs: &Inputs) {
        for t in &self.targets {
            let Some(thing) = resolved.things.get_mut(&t.thing) else { continue };
            let params = match t.slot {
                None => &mut thing.params,
                Some(slot) => match thing.slots.get_mut(&slot) {
                    Some(s) => &mut s.params,
                    None => continue,
                },
            };
            let Some(current) = params.get(&t.key).cloned() else { continue };
            let mut value = t.track.as_ref().and_then(|track| track.value_at(time)).unwrap_or(current);
            if let Some(pulse) = &t.pulse {
                value = pulsed(&value, pulse, pulse_strength(pulse, time, inputs));
            }
            params.set(&t.key, value);
        }
    }
}

/// Where the setting at `a` shows up in the resolved scene.
fn lands_on(scene: &Scene, resolved: &Resolved, a: &Address) -> Vec<(ThingId, Option<SlotId>)> {
    let mut out = Vec::new();
    match (a.owner, a.slot) {
        (Owner::Thing(t), None) => {
            if resolved.things.contains_key(&t) {
                out.push((t, None));
            }
        }
        (Owner::Thing(t), Some(slot)) => {
            for (id, r) in &resolved.things {
                if r.slots.get(&slot).is_some_and(|s| s.from == t) {
                    out.push((*id, Some(slot)));
                }
            }
        }
        (Owner::Module(m), _) => {
            let Some(module) = scene.library.modules.get(&m) else { return out };
            let slot = module.slot;
            for (id, r) in &resolved.things {
                let Some(s) = r.slots.get(&slot) else { continue };
                if s.module != m {
                    continue;
                }
                let overridden = scene.things.get(&s.from).and_then(|from| from.slots.get(&slot)).is_some_and(
                    |link| matches!(link, SlotLink::Linked { overrides, .. } if overrides.get(&a.key).is_some()),
                );
                if !overridden {
                    out.push((*id, Some(slot)));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ModuleId, resolve, testkit};

    fn key(v: f64) -> Value {
        Value::Number(v)
    }

    fn two_key_track(ease: Ease) -> Track {
        Track {
            keys: vec![Key { time: 0.0, value: key(1.0), ease }, Key { time: 2.0, value: key(10.0), ease }],
            range: None,
        }
    }

    #[test]
    fn eases_start_at_zero_and_end_at_one() {
        for ease in Ease::NAMED.iter().chain([Ease::Curve { x1: 0.3, y1: 0.0, x2: 0.7, y2: 1.2 }].iter()) {
            assert!(ease.apply(0.0).abs() < 1e-5, "{}", ease.name());
            assert!((ease.apply(1.0) - 1.0).abs() < 1e-4, "{}", ease.name());
        }
        assert_eq!(Ease::Hold.apply(0.99), 0.0);
        assert!((Ease::Linear.apply(0.25) - 0.25).abs() < 1e-6);
        assert!((Ease::Smooth.apply(0.5) - 0.5).abs() < 1e-6);
        assert!(Ease::Overshoot.apply(0.8) > 1.0);
        let s = Ease::Curve { x1: 0.0, y1: 0.0, x2: 1.0, y2: 1.0 };
        assert!((s.apply(0.5) - 0.5).abs() < 1e-3, "a straight curve is linear: {}", s.apply(0.5));
    }

    #[test]
    fn a_track_interpolates_and_holds_its_ends() {
        let t = two_key_track(Ease::Linear);
        assert_eq!(t.value_at(1.0), Some(key(5.5)));
        assert_eq!(t.value_at(-1.0), Some(key(1.0)));
        assert_eq!(t.value_at(5.0), Some(key(10.0)));
        assert_eq!(Track::default().value_at(1.0), None);
        let held = two_key_track(Ease::Hold);
        assert_eq!(held.value_at(1.9), Some(key(1.0)));
        assert_eq!(held.value_at(2.0), Some(key(10.0)));
        let ranged = Track { range: Some((0.0, 4.0)), ..two_key_track(Ease::Linear) };
        assert_eq!(ranged.value_at(5.0), Some(key(4.0)));
    }

    #[test]
    fn set_key_replaces_a_key_at_the_same_time() {
        let mut t = two_key_track(Ease::Bounce);
        assert_eq!(t.set_key(0.001, key(3.0)), 0);
        assert_eq!(t.keys.len(), 2);
        assert_eq!(t.keys[0].value, key(3.0));
        assert_eq!(t.keys[0].ease, Ease::Bounce, "a replaced key keeps its ease");
        assert_eq!(t.set_key(1.0, key(7.0)), 1);
        assert_eq!(t.keys.len(), 3);
        assert_eq!(t.key_near(1.002, SAME_TIME), Some(1));
        assert_eq!(t.key_near(1.5, SAME_TIME), None);
    }

    #[test]
    fn moving_a_key_keeps_the_track_sorted_and_in_range() {
        let mut t = two_key_track(Ease::Linear);
        t.set_key(1.0, key(5.0));
        assert_eq!(t.move_key(0, 3.0), 2);
        assert_eq!(t.keys.iter().map(|k| k.time).collect::<Vec<_>>(), [1.0, 2.0, 3.0]);
        assert_eq!(t.keys[2].value, key(1.0));
        assert_eq!(t.move_key(2, -5.0), 0);
        assert_eq!(t.keys[0].time, 0.0);
        assert_eq!(t.move_key(9, 1.0), 9, "a missing key is left alone");
        t.remove_key(0);
        assert_eq!(t.keys.len(), 2);
    }

    #[test]
    fn colours_lerp_in_linear_light() {
        let (black, white) = (Color::rgb(0, 0, 0), Color::WHITE);
        assert_eq!(Value::Color(black).lerp(&Value::Color(white), 0.5), Value::Color(Color::mix(black, white, 0.5)));
        assert_eq!(Value::Int(0).lerp(&Value::Int(10), 0.26), Value::Int(3));
        assert_eq!(Value::Vec2([0.0, 0.0]).lerp(&Value::Vec2([10.0, 20.0]), 0.5), Value::Vec2([5.0, 10.0]));
        assert_eq!(Value::Bool(false).lerp(&Value::Bool(true), 0.5), Value::Bool(false));
        assert_eq!(Value::Bool(false).lerp(&Value::Bool(true), 1.0), Value::Bool(true));
    }

    /// Scene defaults with a Plain look, and two balls things linking a second look module.
    fn scene() -> (Registry, Scene, ModuleId, ThingId, ThingId) {
        let reg = testkit::registry();
        let mut scene = Scene::empty(&reg);
        let root_look = scene.add_module(&reg, "look.plain", "Root look").unwrap();
        scene.link(&reg, ThingId::ROOT, SlotId::Look, root_look).unwrap();
        let shared = scene.add_module(&reg, "look.plain", "Shared").unwrap();
        let a = scene.add_thing(&reg, "balls", ThingId::ROOT).unwrap();
        let b = scene.add_thing(&reg, "balls", ThingId::ROOT).unwrap();
        scene.link(&reg, a, SlotId::Look, shared).unwrap();
        scene.link(&reg, b, SlotId::Look, shared).unwrap();
        (reg, scene, shared, a, b)
    }

    fn glow_of(m: ModuleId) -> Address {
        Address { owner: Owner::Module(m), slot: None, key: "glow".into() }
    }

    #[test]
    fn scene_set_key_checks_the_flag_and_the_type() {
        let (reg, mut scene, shared, a, _) = scene();
        let length = Address { owner: Owner::Thing(ThingId::ROOT), slot: None, key: "duration".into() };
        assert_eq!(scene.set_key(&reg, &length, 1.0, key(5.0)), Err(EditError::UnknownSetting("duration".into())));
        assert_eq!(scene.set_key(&reg, &glow_of(shared), 1.0, Value::Color(Color::WHITE)), Err(EditError::WrongType));
        assert_eq!(scene.set_key(&reg, &glow_of(shared), 1.0, key(9.0)), Ok(0));
        let track = &scene.tracks[&glow_of(shared)];
        assert_eq!(track.range, Some((0.0, 2.0)));
        assert_eq!(track.keys[0].value, key(2.0), "pulled into range");
        let inherited = Address { owner: Owner::Thing(a), slot: Some(SlotId::Sound), key: "volume".into() };
        assert_eq!(scene.set_key(&reg, &inherited, 1.0, key(0.0)), Err(EditError::NotLinked));
        let size = Address { owner: Owner::Thing(a), slot: None, key: "radius".into() };
        assert_eq!(scene.set_key(&reg, &size, 2.0, key(10.0)), Ok(0));
        assert!(scene.keyed(&size));
        assert!(scene.remove_key(&size, 0));
        assert!(!scene.keyed(&size) && !scene.tracks.contains_key(&size), "an emptied track goes");
        assert!(!scene.remove_key(&size, 0));
    }

    #[test]
    fn value_at_time_prefers_the_track() {
        let (reg, mut scene, shared, _, _) = scene();
        let glow = glow_of(shared);
        assert_eq!(scene.value_at_time(&reg, &glow, 1.0), Some(key(0.0)));
        scene.set_key(&reg, &glow, 0.0, key(0.0)).unwrap();
        scene.set_key(&reg, &glow, 2.0, key(2.0)).unwrap();
        scene.set_ease(&glow, 0, Ease::Linear);
        assert_eq!(scene.value_at_time(&reg, &glow, 1.0), Some(key(1.0)));
        assert_eq!(scene.move_key(&glow, 1, 4.0), Some(1));
        assert_eq!(scene.value_at_time(&reg, &glow, 1.0), Some(key(0.5)));
        assert_eq!(scene.value_at(&reg, &glow), Some(key(0.0)), "the plain value is untouched");
    }

    #[test]
    fn a_module_track_lands_on_every_user_but_an_overriding_one() {
        let (reg, mut scene, shared, a, b) = scene();
        scene.set_key(&reg, &glow_of(shared), 0.0, key(1.0)).unwrap();
        let resolved = resolve(&scene, &reg);
        let anim = Animation::new(&scene, &resolved);
        assert_eq!(anim.destinations(), vec![(a, Some(SlotId::Look), "glow"), (b, Some(SlotId::Look), "glow")]);
        let over = Address { owner: Owner::Thing(b), slot: Some(SlotId::Look), key: "glow".into() };
        scene.set_at(&reg, &over, key(1.5)).unwrap();
        let resolved = resolve(&scene, &reg);
        let anim = Animation::new(&scene, &resolved);
        assert_eq!(anim.destinations(), vec![(a, Some(SlotId::Look), "glow")]);
        let mut animated = resolved.clone();
        anim.apply(&mut animated, 0.0, &Inputs::NONE);
        assert_eq!(animated.things[&a].slots[&SlotId::Look].params.get("glow"), Some(&key(1.0)));
        assert_eq!(animated.things[&b].slots[&SlotId::Look].params.get("glow"), Some(&key(1.5)));
    }

    #[test]
    fn an_override_track_lands_on_the_thing_and_its_inheriting_children() {
        let (reg, mut scene, shared, a, _) = scene();
        let child = scene.add_thing(&reg, "balls", a).unwrap();
        let over = Address { owner: Owner::Thing(a), slot: Some(SlotId::Look), key: "glow".into() };
        scene.set_key(&reg, &over, 0.0, key(1.2)).unwrap();
        let resolved = resolve(&scene, &reg);
        assert_eq!(resolved.things[&child].slots[&SlotId::Look].from, a);
        let anim = Animation::new(&scene, &resolved);
        assert_eq!(anim.destinations(), vec![(a, Some(SlotId::Look), "glow"), (child, Some(SlotId::Look), "glow")]);
        // The module's own track lands on the other user only: `a` overrides glow by keying it,
        // so once a value is stored there the module track skips it.
        scene.set_at(&reg, &over, key(0.5)).unwrap();
        scene.set_key(&reg, &glow_of(shared), 0.0, key(0.1)).unwrap();
        let resolved = resolve(&scene, &reg);
        let mut anim = Animation::new(&scene, &resolved);
        assert_eq!(anim.destinations().len(), 3);
        anim.retain(|t, _, _| t == a);
        assert_eq!(anim.destinations().len(), 1);
    }

    #[test]
    fn apply_writes_keys_and_pulses_and_clamps() {
        let (reg, mut scene, shared, a, _) = scene();
        let glow = glow_of(shared);
        scene.set_key(&reg, &glow, 0.0, key(1.8)).unwrap();
        scene.set_pulse(&reg, &glow, PulseSource::Beat, key(0.5), 0.25).unwrap();
        let resolved = resolve(&scene, &reg);
        let anim = Animation::new(&scene, &resolved);
        let inputs = PulseInputs { beats: vec![1.0, 2.0], bands: None };
        let at = |time: f64| {
            let mut r = resolved.clone();
            anim.apply(&mut r, time, &Inputs { pulse: Some(&inputs), hits: 0.0 });
            match r.things[&a].slots[&SlotId::Look].params.get("glow") {
                Some(Value::Number(x)) => *x,
                other => panic!("{other:?}"),
            }
        };
        assert_eq!(at(0.5), 1.8, "before the first beat the pulse is silent");
        assert_eq!(at(1.0), 2.0, "on the beat it is clamped to the range");
        let later = at(1.5);
        assert!(later > 1.8 && later < 2.0, "{later}");
        assert!((later - (1.8 + 0.5 * (-2.0f64).exp())).abs() < 1e-5, "{later}");
        // A pulse with nothing to follow adds nothing; hits pulse only through the inputs.
        scene.set_pulse(&reg, &glow, PulseSource::Hits, key(0.1), 0.25).unwrap();
        let anim = Animation::new(&scene, &resolve(&scene, &reg));
        let mut r = resolved.clone();
        anim.apply(&mut r, 1.0, &Inputs::NONE);
        assert_eq!(r.things[&a].slots[&SlotId::Look].params.get("glow"), Some(&key(1.8)));
        anim.apply(&mut r, 1.0, &Inputs { pulse: None, hits: 1.0 });
        match r.things[&a].slots[&SlotId::Look].params.get("glow") {
            Some(Value::Number(x)) => assert!((x - 1.9).abs() < 1e-9, "{x}"),
            other => panic!("{other:?}"),
        }
        assert!(scene.remove_pulse(&glow));
        assert!(!scene.remove_pulse(&glow));
        let pulse = Pulse { source: PulseSource::Bass, amount: key(1.0), decay: 0.25, range: None };
        let bands =
            Bands { rate: 2.0, bass: vec![0.0, 1.0], mid: vec![0.0; 2], treble: vec![0.0; 2], level: vec![0.0; 2] };
        let inputs = PulseInputs { beats: vec![], bands: Some(bands) };
        assert!((pulse_strength(&pulse, 0.25, &Inputs { pulse: Some(&inputs), hits: 0.0 }) - 0.5).abs() < 1e-6);
        assert_eq!(pulse_strength(&pulse, 5.0, &Inputs { pulse: Some(&inputs), hits: 0.0 }), 0.0);
    }

    #[test]
    fn a_scene_with_tracks_saves_and_loads() {
        let (reg, mut scene, shared, a, _) = scene();
        scene.set_key(&reg, &glow_of(shared), 1.0, key(1.0)).unwrap();
        scene.set_ease(&glow_of(shared), 0, Ease::Curve { x1: 0.2, y1: 0.0, x2: 0.8, y2: 1.0 });
        let size = Address { owner: Owner::Thing(a), slot: None, key: "radius".into() };
        scene.set_key(&reg, &size, 0.5, key(3.0)).unwrap();
        scene.set_pulse(&reg, &glow_of(shared), PulseSource::Level, key(0.3), 0.5).unwrap();
        let text = crate::save(&scene);
        let back = crate::load(&text, &reg).unwrap();
        assert_eq!(back.tracks, scene.tracks);
        assert_eq!(back.pulses, scene.pulses);
        // A file from before tracks existed opens with none.
        let plain = text.split("tracks:").next().unwrap().to_owned() + ")";
        let old = crate::load(&plain, &reg).unwrap();
        assert!(old.tracks.is_empty() && old.pulses.is_empty());
    }
}

//! Keyframes and pulses: settings that change over time, named by address, and the
//! [`Animation`] that writes their values into a resolved scene at any moment.
//!
//! A track holds keys for one setting; between keys the value eases from one to the next. A
//! pulse adds to a setting with the hits. Both are stored on
//! the scene by [`Address`], so they keep pointing at the right setting when things are renamed
//! or reordered. Nothing here needs the registry once the keys are in: the editor checks them
//! when they are made, and the runtime and the renderer only read them.

use crate::{
    Address, Color, EditError, Gradient, LayerId, Registry, Resolved, Scene, SettingDef, SlotId, Stop, ThingId, Value,
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

    /// Puts key `index` down where it is: any other key within [`SAME_TIME`] of it goes, so a
    /// key moved onto another replaces it. Called when a drag ends, so keys passed on the way
    /// stay. Returns its new index.
    pub fn settle_key(&mut self, index: usize) -> usize {
        let Some(time) = self.keys.get(index).map(|k| k.time) else { return index };
        let mut at = index;
        let mut i = 0;
        self.keys.retain(|k| {
            let keep = i == index || (k.time - time).abs() > SAME_TIME;
            if !keep && i < index {
                at -= 1;
            }
            i += 1;
            keep
        });
        at
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
    /// blend in linear light (gradients stop by stop, when they have as many stops, otherwise they
    /// switch over halfway), anything else switches over at the end.
    pub fn lerp(&self, other: &Value, t: f32) -> Value {
        let t64 = t as f64;
        match (self, other) {
            (Value::Number(a), Value::Number(b)) => Value::Number(a + (b - a) * t64),
            (Value::Int(a), Value::Int(b)) => Value::Int((*a as f64 + (*b - *a) as f64 * t64).round() as i64),
            (Value::Vec2(a), Value::Vec2(b)) => Value::Vec2([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]),
            (Value::Color(a), Value::Color(b)) => Value::Color(Color::mix(*a, *b, t)),
            (Value::Gradient(a), Value::Gradient(b)) if a.stops.len() == b.stops.len() => {
                let half = t >= 0.5;
                let stops = a
                    .stops
                    .iter()
                    .zip(&b.stops)
                    .map(|(sa, sb)| Stop {
                        at: sa.at + (sb.at - sa.at) * t,
                        color: Color::mix(sa.color, sb.color, t),
                        blend: if half { sb.blend } else { sa.blend },
                        middle: sa.middle + (sb.middle - sa.middle) * t,
                    })
                    .collect();
                let (mix, looped) = if half { (b.mix, b.looped) } else { (a.mix, a.looped) };
                Value::Gradient(Gradient { stops, mix, looped, grain: a.grain, moves: a.moves.clone() })
            }
            (Value::Gradient(a), Value::Gradient(_)) if t < 0.5 => Value::Gradient(a.clone()),
            (Value::Gradient(_), Value::Gradient(b)) => Value::Gradient(b.clone()),
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

/// What a pulse follows: the hits, or a shape's sound.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PulseSource {
    /// The bounces. Never allowed on a setting that moves the physics.
    #[default]
    Hits,
    /// One part of a shape's sound (Follow sound). Worked out from the physics alone, so it
    /// needs no sound to be playing; a setting that moves the physics reads it as it was at the
    /// end of the previous step.
    Sound { shape: ThingId, source: SoundSource },
    /// How lit a thing is (Follow light, lighting part 7): worked out by a CPU light probe from
    /// the lights and the world, never by the renderer. A setting that moves the physics reads it
    /// as it was at the end of the previous step.
    Light { thing: ThingId, source: LightSource },
    /// How much health a thing has left (Follow health, rules part 4): its health ÷ its starting
    /// health, 0 to 1 (a thing with balls: the mean over its live balls). Kept by the world's
    /// rules; a setting that moves the physics reads it as the last step left it.
    Health { thing: ThingId },
}

/// What of the light on a thing a pulse follows. Each reads 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LightSource {
    /// How much light falls on it (its balls on average, round a shape's outline, or its place).
    On,
    /// How bright the single brightest light on it is (no ambient), the most over its balls.
    Brightest,
    /// How much of the light that would reach it its shadows take (1 in full shadow).
    Shadow,
    /// A switch: the share of its balls more than half in shadow (one ball: 0 or 1).
    InShadow,
    /// A Light thing's strength as it shines now (rigs, scenes, flares) over its own Strength.
    Strength,
}

impl LightSource {
    pub const ALL: [LightSource; 5] = [Self::On, Self::Brightest, Self::Shadow, Self::InShadow, Self::Strength];

    /// What it is in words, after a thing's name: "Light on", "In shadow".
    pub fn name(self) -> &'static str {
        match self {
            Self::On => "Light on",
            Self::Brightest => "Brightest light on",
            Self::Shadow => "Shadow on",
            Self::InShadow => "In shadow",
            Self::Strength => "Strength of",
        }
    }
}

/// Which part of a shape's sound a pulse follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SoundSource {
    /// One of the shape's modulators.
    Modulator(LayerId),
    /// How loud the shape is.
    Level,
    /// How loud bus `n` (1..=3) is, worked out from its shapes' levels and its fader (the
    /// pulse's `shape` is ignored).
    Bus(u8),
}

impl PulseSource {
    /// The sources that need nothing chosen; sound sources are listed per scene.
    pub const NAMED: [PulseSource; 1] = [PulseSource::Hits];

    pub fn name(self) -> &'static str {
        match self {
            PulseSource::Hits => "Hits",
            PulseSource::Sound { .. } => "Sound",
            PulseSource::Light { .. } => "Light",
            PulseSource::Health { .. } => "Health",
        }
    }

    /// Whether it follows light (Follow light).
    pub fn follows_light(self) -> bool {
        matches!(self, PulseSource::Light { .. })
    }

    /// Whether it follows a thing's health (Follow health).
    pub fn follows_health(self) -> bool {
        matches!(self, PulseSource::Health { .. })
    }
}

/// A setting that moves with the hits.
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
        Self { source: PulseSource::Hits, amount: Value::Number(0.0), decay: 0.25, range: None }
    }
}

/// A shape's sound as a picture or the physics reads it (Follow sound): each part's level at a
/// moment, 0 to 1. The sound crate works it out from the physics alone.
pub trait SoundLevels {
    /// How far `source` of shape `shape` is up at `time`, 0 to 1 (0 for a part it hasn't got).
    fn level(&self, shape: ThingId, source: &SoundSource, time: f64) -> f32;
}

/// How lit things are as a picture or the physics reads it (Follow light): each source's level
/// at a moment, 0 to 1. The kit works it out from the lights and the world; the renderer never
/// reads it.
pub trait LightLevels {
    /// How far `source` of thing `thing` is up at `time`, 0 to 1 (0 for one it can't have).
    fn level(&self, thing: ThingId, source: &LightSource, time: f64) -> f32;
}

/// How much health things have left as a picture or the physics reads it (Follow health): each
/// thing's share of its starting health, 0 to 1. The world's rules keep it.
pub trait HealthLevels {
    /// `thing`'s health ÷ its starting health, 0 to 1 (0 for a thing without health).
    fn share(&self, thing: ThingId) -> f32;
}

/// Everything a pulse can read at one moment.
#[derive(Clone, Copy, Default)]
pub struct Inputs<'a> {
    /// How hard things are hitting right now, 0 to 1. The runtime always passes 0.
    pub hits: f32,
    /// The shapes' sound, for pulses that follow it (none: they don't move).
    pub sound: Option<&'a dyn SoundLevels>,
    /// How lit things are, for pulses that follow light (none: they don't move).
    pub light: Option<&'a dyn LightLevels>,
    /// How much health things have left, for pulses that follow it (none: they don't move).
    pub health: Option<&'a dyn HealthLevels>,
}

impl std::fmt::Debug for Inputs<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inputs")
            .field("hits", &self.hits)
            .field("sound", &self.sound.is_some())
            .field("light", &self.light.is_some())
            .field("health", &self.health.is_some())
            .finish()
    }
}

impl Inputs<'static> {
    pub const NONE: Inputs<'static> = Inputs { hits: 0.0, sound: None, light: None, health: None };
}

/// How strongly `pulse` pushes at `time`, 0 to 1.
pub fn pulse_strength(pulse: &Pulse, time: f64, inputs: &Inputs) -> f32 {
    let s = match pulse.source {
        PulseSource::Hits => inputs.hits,
        PulseSource::Sound { shape, source } => inputs.sound.map_or(0.0, |sound| sound.level(shape, &source, time)),
        PulseSource::Light { thing, source } => inputs.light.map_or(0.0, |light| light.level(thing, &source, time)),
        PulseSource::Health { thing } => inputs.health.map_or(0.0, |health| health.share(thing)),
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
        let thing = self.things.get(&a.thing).ok_or(EditError::Missing(a.thing))?;
        if let Some(slot) = a.slot
            && !thing.slots.contains_key(&slot)
        {
            return Err(EditError::NoSlot(slot));
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
        let i = track.set_key(time, value);
        if def.flags.hold {
            track.keys[i].ease = Ease::Hold;
        }
        Ok(i)
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

    /// Puts key `index` of the track at `a` down where it is, replacing any key at the same
    /// time ([`Track::settle_key`]). Returns its new index.
    pub fn settle_key(&mut self, a: &Address, index: usize) -> Option<usize> {
        let track = self.tracks.get_mut(a)?;
        (index < track.keys.len()).then(|| track.settle_key(index))
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

    /// Makes the setting at `a` pulse. The setting must have the pulse flag (or, following
    /// sound, the keyframe flag).
    pub fn set_pulse(
        &mut self,
        reg: &Registry,
        a: &Address,
        source: PulseSource,
        amount: Value,
        decay: f32,
    ) -> Result<(), EditError> {
        // Follow sound, light and health may move any setting that can take keys; the hits only those that pulse.
        let def = match source {
            PulseSource::Sound { .. } | PulseSource::Light { .. } | PulseSource::Health { .. } => {
                self.animatable(reg, a, false).or_else(|_| self.animatable(reg, a, true))?
            }
            PulseSource::Hits => self.animatable(reg, a, true)?,
        };
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
    /// The setting this came from.
    address: Address,
    thing: ThingId,
    slot: Option<SlotId>,
    key: String,
    /// The setting's value before any animation, so a pulse adds to the same base every time
    /// `apply` runs on the same resolved scene.
    base: Value,
    track: Option<Track>,
    pulse: Option<Pulse>,
}

/// A scene's tracks and pulses, worked out to where each one lands in the resolved scene: on
/// the setting's own thing, when the thing (and the card) is there.
#[derive(Clone, Debug, Default)]
pub struct Animation {
    targets: Vec<Target>,
}

impl Animation {
    pub fn new(scene: &Scene, resolved: &Resolved) -> Animation {
        let addresses: BTreeSet<&Address> = scene.tracks.keys().chain(scene.pulses.keys()).collect();
        let mut targets = Vec::new();
        for a in addresses {
            let track = scene.tracks.get(a).filter(|t| !t.keys.is_empty()).cloned();
            let pulse = scene.pulses.get(a).cloned();
            if track.is_none() && pulse.is_none() {
                continue;
            }
            let (thing, slot) = (a.thing, a.slot);
            {
                let Some(r) = resolved.things.get(&thing) else { continue };
                let params = match slot {
                    None => Some(&r.params),
                    Some(slot) => r.slots.get(&slot).map(|s| &s.params),
                };
                // A gradient's companion at its default isn't stored: it starts from that default
                // when its gradient is there.
                let companion = || {
                    let (gradient, _) = crate::gradient_extras::split(&a.key)?;
                    params?.get(gradient).filter(|v| matches!(v, Value::Gradient(_)))?;
                    crate::gradient_extras::default_of(&a.key)
                };
                let Some(base) = params.and_then(|p| p.get(&a.key)).cloned().or_else(companion) else { continue };
                targets.push(Target {
                    address: (*a).clone(),
                    thing,
                    slot,
                    key: a.key.clone(),
                    base,
                    track: track.clone(),
                    pulse: pulse.clone(),
                });
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

    /// The addresses whose tracks or pulses land somewhere, each once.
    pub fn addresses(&self) -> BTreeSet<&Address> {
        self.targets.iter().map(|t| &t.address).collect()
    }

    /// Keeps only the targets `keep` says yes to.
    pub fn retain(&mut self, mut keep: impl FnMut(ThingId, Option<SlotId>, &str) -> bool) {
        self.targets.retain(|t| keep(t.thing, t.slot, &t.key));
    }

    /// Writes every animated setting's value at `time` into `resolved`: the keyed value (or the
    /// value the scene had when the animation was made), with the pulse added on top. Applying
    /// to the same resolved scene again and again gives the same values as applying once.
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
            let mut value = t.track.as_ref().and_then(|track| track.value_at(time)).unwrap_or_else(|| t.base.clone());
            if let Some(pulse) = &t.pulse {
                value = pulsed(&value, pulse, pulse_strength(pulse, time, inputs));
            }
            params.set(&t.key, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{resolve, testkit};

    const RED: Color = Color::rgb(255, 0, 0);
    const BLUE: Color = Color::rgb(0, 0, 255);

    fn gradient(stops: &[(f32, Color)]) -> Value {
        let stops = stops.iter().map(|&(at, color)| Stop::new(at, color)).collect();
        Value::Gradient(Gradient { stops, ..Gradient::default() })
    }

    #[test]
    fn lerp_blends_gradients_stop_by_stop() {
        let a = gradient(&[(0.0, RED), (1.0, BLUE)]);
        let b = gradient(&[(0.2, BLUE), (1.0, RED)]);
        let Value::Gradient(mid) = a.lerp(&b, 0.5) else { panic!() };
        assert!((mid.stops[0].at - 0.1).abs() < 1e-6);
        assert_eq!(mid.stops[0].color, Color::mix(RED, BLUE, 0.5));
        assert_eq!(mid.stops[1].at, 1.0);
    }

    #[test]
    fn lerp_with_different_counts_jumps_at_half() {
        let a = gradient(&[(0.0, RED), (1.0, BLUE)]);
        let b = gradient(&[(0.0, RED), (0.5, BLUE), (1.0, RED)]);
        assert_eq!(a.lerp(&b, 0.49), a);
        assert_eq!(a.lerp(&b, 0.5), b);
    }

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
    fn a_key_put_down_on_another_replaces_it() {
        // Keys at 0, 1 (value 5) and 2: the one at 0 is dragged past 1 onto 2.
        let mut t = two_key_track(Ease::Linear);
        t.set_key(1.0, key(5.0));
        let i = t.move_key(0, 1.5);
        let i = t.move_key(i, 2.001);
        assert_eq!(t.keys.len(), 3, "keys passed on the way are kept until the drag ends");
        let i = t.settle_key(i);
        assert_eq!(t.keys.iter().map(|k| k.time).collect::<Vec<_>>(), [1.0, 2.001]);
        assert_eq!(i, 1);
        assert_eq!(t.keys[1].value, key(1.0), "the moved key's value wins");
        assert_eq!(t.value_at(3.0), Some(key(1.0)));
        // A key put down on its own stays as it is.
        assert_eq!(t.settle_key(0), 0);
        assert_eq!(t.keys.len(), 2);
        assert_eq!(t.settle_key(9), 9, "a missing key is left alone");
        let (reg, mut scene, a, _) = scene();
        let glow = glow_of(a);
        scene.set_key(&reg, &glow, 0.0, key(0.0)).unwrap();
        scene.set_key(&reg, &glow, 1.0, key(1.0)).unwrap();
        assert_eq!(scene.move_key(&glow, 0, 1.0), Some(1));
        assert_eq!(scene.settle_key(&glow, 1), Some(0));
        assert_eq!(scene.tracks[&glow].keys.len(), 1);
        assert_eq!(scene.tracks[&glow].keys[0].value, key(0.0));
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

    /// Two balls, each with its own Plain look.
    fn scene() -> (Registry, Scene, ThingId, ThingId) {
        let reg = testkit::registry();
        let mut scene = Scene::empty(&reg);
        let a = scene.add_thing(&reg, "balls", ThingId::ROOT).unwrap();
        let b = scene.add_thing(&reg, "balls", ThingId::ROOT).unwrap();
        for t in [a, b] {
            scene.add_slot(&reg, t, SlotId::Look, "look.plain").unwrap();
        }
        (reg, scene, a, b)
    }

    fn glow_of(t: ThingId) -> Address {
        Address::on(t, SlotId::Look, "glow")
    }

    #[test]
    fn scene_set_key_checks_the_flag_and_the_type() {
        let (reg, mut scene, a, _) = scene();
        let length = Address::own(ThingId::ROOT, "duration");
        assert_eq!(scene.set_key(&reg, &length, 1.0, key(5.0)), Err(EditError::UnknownSetting("duration".into())));
        assert_eq!(scene.set_key(&reg, &glow_of(a), 1.0, Value::Color(Color::WHITE)), Err(EditError::WrongType));
        assert_eq!(scene.set_key(&reg, &glow_of(a), 1.0, key(9.0)), Ok(0));
        let track = &scene.tracks[&glow_of(a)];
        assert_eq!(track.range, Some((0.0, 2.0)));
        assert_eq!(track.keys[0].value, key(2.0), "pulled into range");
        let missing = Address::on(a, SlotId::Trail, "width");
        assert_eq!(scene.set_key(&reg, &missing, 1.0, key(0.0)), Err(EditError::NoSlot(SlotId::Trail)));
        let size = Address::own(a, "radius");
        assert_eq!(scene.set_key(&reg, &size, 2.0, key(10.0)), Ok(0));
        assert!(scene.keyed(&size));
        assert!(scene.remove_key(&size, 0));
        assert!(!scene.keyed(&size) && !scene.tracks.contains_key(&size), "an emptied track goes");
        assert!(!scene.remove_key(&size, 0));
    }

    #[test]
    fn value_at_time_prefers_the_track() {
        let (reg, mut scene, a, _) = scene();
        let glow = glow_of(a);
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
    fn a_track_lands_only_on_its_own_thing() {
        let (reg, mut scene, a, b) = scene();
        scene.set_key(&reg, &glow_of(a), 0.0, key(1.0)).unwrap();
        let resolved = resolve(&scene, &reg);
        let mut anim = Animation::new(&scene, &resolved);
        assert_eq!(anim.destinations(), vec![(a, Some(SlotId::Look), "glow")]);
        let mut animated = resolved.clone();
        anim.apply(&mut animated, 0.0, &Inputs::NONE);
        assert_eq!(animated.things[&a].slots[&SlotId::Look].params.get("glow"), Some(&key(1.0)));
        assert_eq!(animated.things[&b].slots[&SlotId::Look].params.get("glow"), Some(&key(0.0)));
        anim.retain(|t, _, _| t == b);
        assert!(anim.is_empty());
    }

    #[test]
    fn apply_writes_keys_and_pulses_and_clamps() {
        let (reg, mut scene, a, _) = scene();
        let glow = glow_of(a);
        scene.set_key(&reg, &glow, 0.0, key(1.8)).unwrap();
        // A hits pulse adds nothing until something hits.
        scene.set_pulse(&reg, &glow, PulseSource::Hits, key(0.1), 0.25).unwrap();
        let resolved = resolve(&scene, &reg);
        let anim = Animation::new(&scene, &resolved);
        let mut r = resolved.clone();
        anim.apply(&mut r, 1.0, &Inputs::NONE);
        assert_eq!(r.things[&a].slots[&SlotId::Look].params.get("glow"), Some(&key(1.8)));
        anim.apply(&mut r, 1.0, &Inputs { hits: 1.0, ..Inputs::NONE });
        match r.things[&a].slots[&SlotId::Look].params.get("glow") {
            Some(Value::Number(x)) => assert!((x - 1.9).abs() < 1e-9, "{x}"),
            other => panic!("{other:?}"),
        }
        // Applying to the same copy twice does not add the pulse twice.
        anim.apply(&mut r, 1.0, &Inputs { hits: 1.0, ..Inputs::NONE });
        match r.things[&a].slots[&SlotId::Look].params.get("glow") {
            Some(Value::Number(x)) => assert!((x - 1.9).abs() < 1e-9, "{x}"),
            other => panic!("{other:?}"),
        }
        // It is clamped to the setting's range.
        scene.set_pulse(&reg, &glow, PulseSource::Hits, key(0.5), 0.25).unwrap();
        let anim = Animation::new(&scene, &resolve(&scene, &reg));
        let mut r = resolved.clone();
        anim.apply(&mut r, 1.0, &Inputs { hits: 1.0, ..Inputs::NONE });
        assert_eq!(r.things[&a].slots[&SlotId::Look].params.get("glow"), Some(&key(2.0)));
        assert!(scene.remove_pulse(&glow));
        assert!(!scene.remove_pulse(&glow));
    }

    /// A shape's sound as a fixed table of levels, for the tests.
    struct Levels(f32);

    impl SoundLevels for Levels {
        fn level(&self, shape: ThingId, source: &SoundSource, time: f64) -> f32 {
            match source {
                SoundSource::Modulator(LayerId(1)) if shape == ThingId(7) => self.0 * time as f32,
                _ => 0.0,
            }
        }
    }

    #[test]
    fn a_pulse_following_sound_reads_its_level() {
        let (reg, mut scene, a, _) = scene();
        let glow = glow_of(a);
        let source = PulseSource::Sound { shape: ThingId(7), source: SoundSource::Modulator(LayerId(1)) };
        scene.set_pulse(&reg, &glow, source, key(1.0), 0.25).unwrap();
        let resolved = resolve(&scene, &reg);
        let anim = Animation::new(&scene, &resolved);
        let mut r = resolved.clone();
        anim.apply(&mut r, 0.5, &Inputs::NONE);
        assert_eq!(r.things[&a].slots[&SlotId::Look].params.get("glow"), Some(&key(0.0)), "no sound: no pulse");
        let levels = Levels(1.0);
        anim.apply(&mut r, 0.5, &Inputs { sound: Some(&levels), ..Inputs::NONE });
        assert_eq!(r.things[&a].slots[&SlotId::Look].params.get("glow"), Some(&key(0.5)));
        // Hits don't move it; another shape's or modulator's sound doesn't either.
        anim.apply(&mut r, 0.5, &Inputs { hits: 1.0, ..Inputs::NONE });
        assert_eq!(r.things[&a].slots[&SlotId::Look].params.get("glow"), Some(&key(0.0)));
    }

    /// How lit things are as a fixed table, for the tests.
    struct Lit(f32);

    impl LightLevels for Lit {
        fn level(&self, thing: ThingId, source: &LightSource, _time: f64) -> f32 {
            if thing == ThingId(9) && *source == LightSource::On { self.0 } else { 0.0 }
        }
    }

    #[test]
    fn a_pulse_following_light_reads_its_level() {
        let (reg, mut scene, a, _) = scene();
        let glow = glow_of(a);
        let source = PulseSource::Light { thing: ThingId(9), source: LightSource::On };
        assert!(source.follows_light() && !PulseSource::Hits.follows_light());
        scene.set_pulse(&reg, &glow, source, key(1.0), 0.25).unwrap();
        let resolved = resolve(&scene, &reg);
        let anim = Animation::new(&scene, &resolved);
        let mut r = resolved.clone();
        anim.apply(&mut r, 0.5, &Inputs::NONE);
        assert_eq!(r.things[&a].slots[&SlotId::Look].params.get("glow"), Some(&key(0.0)), "no probe: no pulse");
        let lit = Lit(0.75);
        anim.apply(&mut r, 0.5, &Inputs { light: Some(&lit), ..Inputs::NONE });
        assert_eq!(r.things[&a].slots[&SlotId::Look].params.get("glow"), Some(&key(0.75)));
        let size = Address::own(a, "radius");
        assert!(scene.set_pulse(&reg, &size, source, key(1.0), 0.25).is_ok(), "any keyframed setting");
    }

    #[test]
    fn a_sound_pulse_fits_any_keyframed_setting() {
        let (reg, mut scene, a, _) = scene();
        let size = Address::own(a, "radius");
        assert!(scene.set_pulse(&reg, &size, PulseSource::Hits, key(1.0), 0.25).is_err(), "no Hits pulse");
        let source = PulseSource::Sound { shape: ThingId(7), source: SoundSource::Level };
        scene.set_pulse(&reg, &size, source, key(1.0), 0.25).unwrap();
        assert_eq!(scene.pulses[&size].source, source);
    }

    #[test]
    fn a_scene_with_tracks_saves_and_loads() {
        let (reg, mut scene, a, _) = scene();
        scene.set_key(&reg, &glow_of(a), 1.0, key(1.0)).unwrap();
        scene.set_ease(&glow_of(a), 0, Ease::Curve { x1: 0.2, y1: 0.0, x2: 0.8, y2: 1.0 });
        let size = Address::own(a, "radius");
        scene.set_key(&reg, &size, 0.5, key(3.0)).unwrap();
        scene.set_pulse(&reg, &glow_of(a), PulseSource::Hits, key(0.3), 0.5).unwrap();
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

//! Keyframes: settings that change over the length of the video.
//!
//! A [`Track`] holds the keyframes of one [`Property`]. Between two keyframes the value moves
//! along the first keyframe's [`Ease`]; before the first and after the last it stays put. While a
//! setting is animated its plain value in the scene is not used.

use crate::{Color, Container, Scene};
use serde::{Deserialize, Serialize};

/// Two keyframes closer together than this, in seconds, count as the same moment.
pub const SAME_TIME: f32 = 0.004;

/// A setting that can be animated.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Property {
    Gravity,
    SidewaysGravity,
    Bounciness,
    AirDrag,
    SpeedGain,
    /// [`crate::ArenaMotion::size`].
    ArenaSize,
    ArenaSpin,
    ShrinkSpeed,
    /// Line colour of a ring, box or polygon arena.
    ArenaColor,
    BackgroundTop,
    BackgroundBottom,
    TrailWidth,
    TrailOpacity,
    BallGlow,
    FlashStrength,
    FlashSize,
    Bloom,
    Exposure,
    StringOpacity,
    StringWidth,
    /// Amount of the effect at this index in [`Scene::effects`].
    EffectAmount(u32),
    /// A slider of the effect at this index, by name.
    EffectParam(u32, String),
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Number(f32),
    Color(Color),
}

/// How the value moves from a keyframe to the next one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Ease {
    /// Starts and ends gently.
    #[default]
    Smooth,
    /// Steady speed.
    Linear,
    /// Starts slowly and speeds up.
    EaseIn,
    /// Starts quickly and slows down.
    EaseOut,
    /// Stays at this keyframe's value, then jumps to the next.
    Hold,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Key {
    /// Seconds from the start of the video.
    pub time: f32,
    pub value: Value,
    #[serde(default)]
    pub ease: Ease,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub property: Property,
    /// Earliest first.
    pub keys: Vec<Key>,
}

impl Ease {
    pub const ALL: [Ease; 5] = [Ease::Smooth, Ease::Linear, Ease::EaseIn, Ease::EaseOut, Ease::Hold];

    pub fn name(self) -> &'static str {
        match self {
            Ease::Smooth => "Smooth",
            Ease::Linear => "Linear",
            Ease::EaseIn => "Ease in",
            Ease::EaseOut => "Ease out",
            Ease::Hold => "Hold",
        }
    }

    /// Maps progress `t` (0 to 1) between two keyframes to how far the value has moved.
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Ease::Smooth => t * t * (3.0 - 2.0 * t),
            Ease::Linear => t,
            Ease::EaseIn => t * t * t,
            Ease::EaseOut => 1.0 - (1.0 - t).powi(3),
            Ease::Hold => 0.0,
        }
    }
}

impl Value {
    pub fn number(self) -> Option<f32> {
        match self {
            Value::Number(v) => Some(v),
            Value::Color(_) => None,
        }
    }

    /// Blends towards `other`, colours in linear light. Mismatched kinds keep `self`.
    pub fn lerp(self, other: Value, t: f32) -> Value {
        match (self, other) {
            (Value::Number(a), Value::Number(b)) => Value::Number(a + (b - a) * t),
            (Value::Color(a), Value::Color(b)) => Value::Color(Color::mix(a, b, t)),
            _ => self,
        }
    }
}

impl Track {
    pub fn new(property: Property) -> Self {
        Self { property, keys: Vec::new() }
    }

    /// The value at `time`, or None if the track has no keyframes.
    pub fn value_at(&self, time: f32) -> Option<Value> {
        let keys = &self.keys;
        let first = keys.first()?;
        if time <= first.time {
            return Some(first.value);
        }
        // The first keyframe later than `time`.
        let next = keys.partition_point(|k| k.time <= time);
        if next == keys.len() {
            return keys.last().map(|k| k.value);
        }
        let (a, b) = (&keys[next - 1], &keys[next]);
        let span = b.time - a.time;
        let t = if span > 0.0 { (time - a.time) / span } else { 1.0 };
        Some(a.value.lerp(b.value, a.ease.apply(t)))
    }

    /// Index of the keyframe at `time`, within `within` seconds.
    pub fn key_near(&self, time: f32, within: f32) -> Option<usize> {
        self.keys
            .iter()
            .enumerate()
            .filter(|(_, k)| (k.time - time).abs() <= within)
            .min_by(|(_, a), (_, b)| (a.time - time).abs().total_cmp(&(b.time - time).abs()))
            .map(|(i, _)| i)
    }

    /// Sets the value at `time`, changing the keyframe already there or adding one. Returns its index.
    pub fn set_key(&mut self, time: f32, value: Value) -> usize {
        if let Some(i) = self.key_near(time, SAME_TIME) {
            self.keys[i].value = value;
            return i;
        }
        let ease = match self.keys.partition_point(|k| k.time <= time) {
            0 => Ease::default(),
            // A new keyframe in the middle of a segment eases the same way as the one before it.
            i => self.keys[i - 1].ease,
        };
        let index = self.keys.partition_point(|k| k.time <= time);
        self.keys.insert(index, Key { time: time.max(0.0), value, ease });
        index
    }

    /// Moves keyframe `index` to `time`. Returns its new index.
    pub fn move_key(&mut self, index: usize, time: f32) -> usize {
        let mut key = self.keys.remove(index);
        key.time = time.max(0.0);
        let index = self.keys.partition_point(|k| k.time <= key.time);
        self.keys.insert(index, key);
        index
    }

    pub fn sort(&mut self) {
        self.keys.sort_by(|a, b| a.time.total_cmp(&b.time));
    }
}

impl Property {
    /// Every setting that doesn't belong to an effect.
    pub const PLAIN: [Property; 20] = [
        Property::Gravity,
        Property::SidewaysGravity,
        Property::Bounciness,
        Property::AirDrag,
        Property::SpeedGain,
        Property::ArenaSize,
        Property::ArenaSpin,
        Property::ShrinkSpeed,
        Property::ArenaColor,
        Property::BackgroundTop,
        Property::BackgroundBottom,
        Property::TrailWidth,
        Property::TrailOpacity,
        Property::BallGlow,
        Property::FlashStrength,
        Property::FlashSize,
        Property::Bloom,
        Property::Exposure,
        Property::StringOpacity,
        Property::StringWidth,
    ];

    /// Whether changing it changes how balls move (and so the sound), not just the picture.
    pub fn affects_simulation(&self) -> bool {
        matches!(
            self,
            Property::Gravity
                | Property::SidewaysGravity
                | Property::Bounciness
                | Property::AirDrag
                | Property::SpeedGain
                | Property::ArenaSize
                | Property::ArenaSpin
                | Property::ShrinkSpeed
        )
    }

    pub fn is_color(&self) -> bool {
        matches!(self, Property::ArenaColor | Property::BackgroundTop | Property::BackgroundBottom)
    }

    /// The effect this belongs to, if any.
    pub fn effect(&self) -> Option<u32> {
        match self {
            Property::EffectAmount(i) | Property::EffectParam(i, _) => Some(*i),
            _ => None,
        }
    }

    /// The setting's current plain value, or None if the scene doesn't have it (an arena colour
    /// for escape rings, a missing effect, or an effect slider left at the shader's default).
    pub fn get(&self, scene: &Scene) -> Option<Value> {
        let number = |v: f32| Some(Value::Number(v));
        let look = &scene.look;
        match self {
            Property::Gravity => number(scene.physics.gravity.y),
            Property::SidewaysGravity => number(scene.physics.gravity.x),
            Property::Bounciness => number(scene.physics.restitution),
            Property::AirDrag => number(scene.physics.air_drag),
            Property::SpeedGain => number(scene.rules.speed_gain),
            Property::ArenaSize => number(scene.arena_motion.size),
            Property::ArenaSpin => number(scene.arena_motion.spin),
            Property::ShrinkSpeed => number(scene.arena_motion.shrink_speed),
            Property::ArenaColor => match &scene.container {
                Container::Rings { .. } => None,
                container => Some(Value::Color(container.color())),
            },
            Property::BackgroundTop => Some(Value::Color(look.background_top)),
            Property::BackgroundBottom => Some(Value::Color(look.background_bottom)),
            Property::TrailWidth => number(look.trail_width),
            Property::TrailOpacity => number(look.trail_opacity),
            Property::BallGlow => number(look.ball_glow),
            Property::FlashStrength => number(look.flash_strength),
            Property::FlashSize => number(look.flash_size),
            Property::Bloom => number(look.bloom_strength),
            Property::Exposure => number(look.exposure),
            Property::StringOpacity => number(look.string_opacity),
            Property::StringWidth => number(look.string_width),
            Property::EffectAmount(i) => number(scene.effects.get(*i as usize)?.amount),
            Property::EffectParam(i, name) => number(*scene.effects.get(*i as usize)?.params.get(name)?),
        }
    }

    /// Sets the setting's plain value. Values of the wrong kind, and settings the scene doesn't
    /// have, are ignored.
    pub fn set(&self, scene: &mut Scene, value: Value) {
        match value {
            Value::Number(v) => {
                if let Some(slot) = self.number_mut(scene) {
                    *slot = v;
                }
            }
            Value::Color(c) => {
                if let Some(slot) = self.color_mut(scene) {
                    *slot = c;
                }
            }
        }
    }

    /// Copies this setting from `from` to `to` exactly, including an effect slider being unset.
    pub fn copy(&self, from: &Scene, to: &mut Scene) {
        match (self, self.get(from)) {
            (_, Some(value)) => self.set(to, value),
            (Property::EffectParam(i, name), None) => {
                if let Some(effect) = to.effects.get_mut(*i as usize) {
                    effect.params.remove(name);
                }
            }
            _ => {}
        }
    }

    fn number_mut<'a>(&self, scene: &'a mut Scene) -> Option<&'a mut f32> {
        let look = &mut scene.look;
        Some(match self {
            Property::Gravity => &mut scene.physics.gravity.y,
            Property::SidewaysGravity => &mut scene.physics.gravity.x,
            Property::Bounciness => &mut scene.physics.restitution,
            Property::AirDrag => &mut scene.physics.air_drag,
            Property::SpeedGain => &mut scene.rules.speed_gain,
            Property::ArenaSize => &mut scene.arena_motion.size,
            Property::ArenaSpin => &mut scene.arena_motion.spin,
            Property::ShrinkSpeed => &mut scene.arena_motion.shrink_speed,
            Property::TrailWidth => &mut look.trail_width,
            Property::TrailOpacity => &mut look.trail_opacity,
            Property::BallGlow => &mut look.ball_glow,
            Property::FlashStrength => &mut look.flash_strength,
            Property::FlashSize => &mut look.flash_size,
            Property::Bloom => &mut look.bloom_strength,
            Property::Exposure => &mut look.exposure,
            Property::StringOpacity => &mut look.string_opacity,
            Property::StringWidth => &mut look.string_width,
            Property::EffectAmount(i) => &mut scene.effects.get_mut(*i as usize)?.amount,
            Property::EffectParam(i, name) => {
                scene.effects.get_mut(*i as usize)?.params.entry(name.clone()).or_insert(0.0)
            }
            _ => return None,
        })
    }

    fn color_mut<'a>(&self, scene: &'a mut Scene) -> Option<&'a mut Color> {
        match self {
            Property::BackgroundTop => Some(&mut scene.look.background_top),
            Property::BackgroundBottom => Some(&mut scene.look.background_bottom),
            Property::ArenaColor => match &mut scene.container {
                Container::Circle { color, .. }
                | Container::Rect { color, .. }
                | Container::Polygon { color, .. }
                | Container::Custom { color, .. } => Some(color),
                Container::Rings { .. } => None,
            },
            _ => None,
        }
    }
}

impl Scene {
    /// The scene as it is at `time`, with every animated setting at its value then.
    pub fn at_time(&self, time: f32) -> Scene {
        let mut scene = self.clone();
        scene.apply_animation(time, false);
        scene
    }

    /// Sets every animated setting (or only those that change how balls move) to its value at `time`.
    pub fn apply_animation(&mut self, time: f32, simulation_only: bool) {
        if self.animation.is_empty() {
            return;
        }
        let tracks = std::mem::take(&mut self.animation);
        for track in &tracks {
            if simulation_only && !track.property.affects_simulation() {
                continue;
            }
            if let Some(value) = track.value_at(time) {
                track.property.set(self, value);
            }
        }
        self.animation = tracks;
    }

    /// The scene at `time` with keyframes and modulators applied, following `audio`.
    pub fn at_time_with(&self, time: f32, audio: &Audio) -> Scene {
        let mut scene = self.clone();
        scene.apply_animation_with(time, false, audio);
        scene
    }

    /// [`Scene::apply_animation`], then every modulator on top. Safe to call again and again on
    /// the same scene, as the simulation does every step: number settings start each time from
    /// their plain or keyframed value, not from last step's pulse.
    pub fn apply_animation_with(&mut self, time: f32, simulation_only: bool, audio: &Audio) {
        self.apply_animation(time, simulation_only);
        if self.modulators.is_empty() {
            return;
        }
        let mut modulators = std::mem::take(&mut self.modulators);
        for m in &mut modulators {
            if simulation_only && !m.property.affects_simulation() {
                continue;
            }
            let Some(current) = m.property.get(self) else { continue };
            let keyed = self.track(&m.property).is_some_and(|t| !t.keys.is_empty());
            let base = match current {
                Value::Number(v) if !keyed => Value::Number(v - m.applied),
                other => other,
            };
            let value = m.modulate(base, time as f64, audio);
            m.applied = match (base, value) {
                (Value::Number(b), Value::Number(v)) => v - b,
                _ => 0.0,
            };
            if value != current {
                m.property.set(self, value);
            }
        }
        self.modulators = modulators;
    }

    /// Whether any setting pulses with the music.
    pub fn has_modulators(&self) -> bool {
        self.modulators.iter().any(|m| m.enabled)
    }

    pub fn modulator(&self, property: &Property) -> Option<&Modulator> {
        self.modulators.iter().find(|m| m.property == *property)
    }

    pub fn track(&self, property: &Property) -> Option<&Track> {
        self.animation.iter().find(|t| t.property == *property)
    }

    pub fn track_mut(&mut self, property: &Property) -> Option<&mut Track> {
        self.animation.iter_mut().find(|t| t.property == *property)
    }

    /// Whether any animated setting changes how balls move.
    pub fn animates_simulation(&self) -> bool {
        self.animation.iter().any(|t| t.property.affects_simulation() && !t.keys.is_empty())
            || self.modulators.iter().any(|m| m.enabled && m.property.affects_simulation())
    }

    /// Keeps effect animations attached to their effects after the effect list changes:
    /// `map` gives each old effect index its new one, or None if the effect is gone.
    /// Modulators on effects move with them too.
    pub fn remap_effect_tracks(&mut self, map: impl Fn(u32) -> Option<u32>) {
        remap_effect_modulators(&mut self.modulators, &map);
        self.animation.retain_mut(|track| {
            let Some(old) = track.property.effect() else { return true };
            let Some(new) = map(old) else { return false };
            match &mut track.property {
                Property::EffectAmount(i) | Property::EffectParam(i, _) => *i = new,
                _ => {}
            }
            true
        });
    }

    /// Copies the animations of effect `from` onto effect `to`, replacing any it had.
    pub fn copy_effect_tracks(&mut self, from: u32, to: u32) {
        self.modulators.retain(|m| m.property.effect() != Some(to));
        let modulators: Vec<Modulator> = self
            .modulators
            .iter()
            .filter(|m| m.property.effect() == Some(from))
            .map(|m| {
                let mut copy = m.clone();
                match &mut copy.property {
                    Property::EffectAmount(i) | Property::EffectParam(i, _) => *i = to,
                    _ => {}
                }
                copy
            })
            .collect();
        self.modulators.extend(modulators);
        self.animation.retain(|t| t.property.effect() != Some(to));
        let copies: Vec<Track> = self
            .animation
            .iter()
            .filter(|t| t.property.effect() == Some(from))
            .map(|t| {
                let mut copy = t.clone();
                match &mut copy.property {
                    Property::EffectAmount(i) | Property::EffectParam(i, _) => *i = to,
                    _ => {}
                }
                copy
            })
            .collect();
        self.animation.extend(copies);
    }
}

// ---- Modulators: settings that pulse with the music ----

/// Which part of the music a [`Modulator`] follows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AudioBand {
    /// Kick drums and bass lines.
    #[default]
    Bass,
    /// Vocals, chords and most instruments.
    Mid,
    /// Hi-hats, cymbals and sparkle.
    Treble,
    /// Overall loudness.
    Level,
    /// 1 on every beat, fading quickly to 0.
    Beat,
    /// Jumps whenever a new sound starts.
    Onset,
}

/// Which sound a [`Modulator`] listens to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AudioFrom {
    /// The finished soundtrack: the song plus the bounce sounds. Visual settings only.
    #[default]
    Mix,
    /// Only the loaded song. Settings that change how balls move always use this, so the
    /// balls can never react to the sounds they make themselves.
    Song,
}

/// How the music's loudness maps onto the setting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Curve {
    /// In step with the music.
    #[default]
    Linear,
    /// Reacts to quiet sounds too.
    Soft,
    /// Only reacts to loud hits.
    Sharp,
    /// Off or fully on, nothing between.
    Gate,
}

/// One moment of the music, each part from 0 to about 1. `beat` is the phase through the
/// current beat: 0 on the beat, rising to 1 just before the next.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Levels {
    pub bass: f32,
    pub mid: f32,
    pub treble: f32,
    pub level: f32,
    pub beat: f32,
    pub onset: f32,
}

/// Something that knows the music at any time in seconds, such as a song's analysis.
pub type Listener<'a> = &'a dyn Fn(f64) -> Levels;

/// The music modulators can follow. Either can be missing, and then modulators using it rest.
#[derive(Clone, Copy, Default)]
pub struct Audio<'a> {
    pub song: Option<Listener<'a>>,
    pub mix: Option<Listener<'a>>,
}

impl std::fmt::Debug for Audio<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Audio").field("song", &self.song.is_some()).field("mix", &self.mix.is_some()).finish()
    }
}

/// "Pulse with music": moves a setting with the music, on top of its value or keyframes.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Modulator {
    pub property: Property,
    pub source: AudioBand,
    pub from: AudioFrom,
    /// How far the setting moves when the music is at full strength, in the setting's own units.
    /// For colours, how far towards [`Modulator::color`] (0 to 1).
    pub amount: f32,
    /// Seconds the music is averaged over; 0 follows every hit exactly, higher is gentler.
    pub smoothing: f32,
    pub curve: Curve,
    /// For colour settings: the colour the music pushes towards.
    pub color: Color,
    pub enabled: bool,
    /// How far this modulator last moved a number setting in a scene changed in place, such as
    /// the simulation's own copy, so the next step starts again from the plain value.
    #[serde(skip)]
    #[doc(hidden)]
    pub applied: f32,
}

impl PartialEq for Modulator {
    fn eq(&self, other: &Self) -> bool {
        self.property == other.property
            && self.source == other.source
            && self.from == other.from
            && self.amount == other.amount
            && self.smoothing == other.smoothing
            && self.curve == other.curve
            && self.color == other.color
            && self.enabled == other.enabled
    }
}

impl Default for Modulator {
    fn default() -> Self {
        Self {
            property: Property::Bloom,
            source: AudioBand::Bass,
            from: AudioFrom::Mix,
            amount: 0.5,
            smoothing: 0.08,
            curve: Curve::Linear,
            color: Color::WHITE,
            enabled: true,
            applied: 0.0,
        }
    }
}

/// Most smoothing allowed, in seconds.
pub const MAX_SMOOTHING: f32 = 2.0;
/// Samples averaged for smoothing. Fixed, so the result only depends on the time.
const SMOOTHING_TAPS: usize = 8;

impl AudioBand {
    pub const ALL: [AudioBand; 6] =
        [AudioBand::Bass, AudioBand::Mid, AudioBand::Treble, AudioBand::Level, AudioBand::Beat, AudioBand::Onset];

    pub fn name(self) -> &'static str {
        match self {
            AudioBand::Bass => "Bass",
            AudioBand::Mid => "Mid",
            AudioBand::Treble => "Treble",
            AudioBand::Level => "Loudness",
            AudioBand::Beat => "Beat",
            AudioBand::Onset => "Every new sound",
        }
    }

    /// This part of the music, from 0 to 1.
    pub fn of(self, levels: &Levels) -> f32 {
        let value = match self {
            AudioBand::Bass => levels.bass,
            AudioBand::Mid => levels.mid,
            AudioBand::Treble => levels.treble,
            AudioBand::Level => levels.level,
            AudioBand::Beat => (1.0 - levels.beat.clamp(0.0, 1.0)).powi(4),
            AudioBand::Onset => levels.onset,
        };
        if value.is_finite() { value.clamp(0.0, 1.5) } else { 0.0 }
    }
}

impl Curve {
    pub const ALL: [Curve; 4] = [Curve::Linear, Curve::Soft, Curve::Sharp, Curve::Gate];

    pub fn name(self) -> &'static str {
        match self {
            Curve::Linear => "Even",
            Curve::Soft => "Soft",
            Curve::Sharp => "Sharp",
            Curve::Gate => "On or off",
        }
    }

    pub fn apply(self, x: f32) -> f32 {
        let x = x.max(0.0);
        match self {
            Curve::Linear => x,
            Curve::Soft => x.sqrt(),
            Curve::Sharp => x * x,
            Curve::Gate => {
                let t = ((x - 0.4) / 0.2).clamp(0.0, 1.0);
                t * t * (3.0 - 2.0 * t)
            }
        }
    }
}

impl Modulator {
    pub fn new(property: Property, source: AudioBand) -> Self {
        let from = if property.affects_simulation() { AudioFrom::Song } else { AudioFrom::Mix };
        Self { property, source, from, ..Self::default() }
    }

    /// The sound this modulator really listens to: settings that move balls always use the song.
    pub fn listens_to(&self) -> AudioFrom {
        if self.property.affects_simulation() { AudioFrom::Song } else { self.from }
    }

    /// How strongly the music pushes the setting at `time`, from 0 to about 1, after smoothing
    /// and the curve. None when the music it listens to isn't there.
    pub fn strength(&self, time: f64, audio: &Audio) -> Option<f32> {
        // Without a mixed soundtrack, the song alone is the next best thing for visuals.
        let listener = match self.listens_to() {
            AudioFrom::Song => audio.song?,
            AudioFrom::Mix => audio.mix.or(audio.song)?,
        };
        let smoothing = if self.smoothing.is_finite() { self.smoothing.clamp(0.0, MAX_SMOOTHING) } else { 0.0 };
        let raw = if smoothing < 0.005 {
            self.source.of(&listener(time))
        } else {
            // A weighted look back over the last two smoothing times: recent music counts most.
            let step = 2.0 * smoothing / SMOOTHING_TAPS as f32;
            let (mut total, mut weights) = (0.0, 0.0);
            for k in 0..SMOOTHING_TAPS {
                let ago = k as f32 * step;
                let weight = (-ago / smoothing).exp();
                total += self.source.of(&listener((time - ago as f64).max(0.0))) * weight;
                weights += weight;
            }
            total / weights
        };
        Some(self.curve.apply(raw))
    }

    /// The setting's value at `time`, starting from `base`.
    pub fn modulate(&self, base: Value, time: f64, audio: &Audio) -> Value {
        if !self.enabled || !self.amount.is_finite() {
            return base;
        }
        let Some(strength) = self.strength(time, audio) else { return base };
        match base {
            Value::Number(v) => Value::Number(self.property.limit(v + self.amount * strength)),
            Value::Color(c) => Value::Color(Color::mix(c, self.color, (self.amount * strength).clamp(0.0, 1.0))),
        }
    }
}

impl Property {
    /// Keeps a value made by a modulator within what the setting can take.
    pub fn limit(&self, value: f32) -> f32 {
        match self {
            Property::Gravity
            | Property::SidewaysGravity
            | Property::ArenaSpin
            | Property::ShrinkSpeed
            | Property::EffectParam(..) => value,
            Property::ArenaSize => value.max(0.05),
            Property::EffectAmount(_) => value.clamp(0.0, 1.0),
            _ => value.max(0.0),
        }
    }
}

/// Applies `modulators` to `scene` at `time` (only those on settings that move balls, with
/// `simulation_only`). Modulators come after keyframes, so they pulse around the animated value.
#[cfg(test)]
fn apply_modulators(scene: &mut Scene, modulators: &[Modulator], time: f64, simulation_only: bool, audio: &Audio) {
    for modulator in modulators {
        if simulation_only && !modulator.property.affects_simulation() {
            continue;
        }
        if let Some(base) = modulator.property.get(scene) {
            let value = modulator.modulate(base, time, audio);
            if value != base {
                modulator.property.set(scene, value);
            }
        }
    }
}

/// Keeps modulators attached to their effects after the effect list changes, like
/// [`Scene::remap_effect_tracks`] does for keyframes.
pub fn remap_effect_modulators(modulators: &mut Vec<Modulator>, map: impl Fn(u32) -> Option<u32>) {
    modulators.retain_mut(|m| {
        let Some(old) = m.property.effect() else { return true };
        let Some(new) = map(old) else { return false };
        match &mut m.property {
            Property::EffectAmount(i) | Property::EffectParam(i, _) => *i = new,
            _ => {}
        }
        true
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Effect;

    fn steady(levels: Levels) -> impl Fn(f64) -> Levels {
        move |_| levels
    }

    #[test]
    fn modulators_rest_without_music() {
        let m = Modulator::new(Property::Bloom, AudioBand::Bass);
        assert_eq!(m.modulate(Value::Number(0.3), 1.0, &Audio::default()), Value::Number(0.3));
        // Without a mixed soundtrack, a visual setting follows the song.
        let song = steady(Levels { bass: 1.0, ..Levels::default() });
        let audio = Audio { song: Some(&song), mix: None };
        assert_eq!(m.modulate(Value::Number(0.3), 1.0, &audio), Value::Number(0.8));
    }

    #[test]
    fn modulators_follow_their_band() {
        let mix = steady(Levels { bass: 1.0, treble: 0.25, beat: 0.0, ..Levels::default() });
        let audio = Audio { song: None, mix: Some(&mix) };
        let mut m = Modulator { amount: 2.0, smoothing: 0.0, ..Modulator::new(Property::Bloom, AudioBand::Bass) };
        assert_eq!(m.modulate(Value::Number(0.5), 1.0, &audio), Value::Number(2.5));
        m.source = AudioBand::Treble;
        assert_eq!(m.modulate(Value::Number(0.5), 1.0, &audio), Value::Number(1.0));
        m.curve = Curve::Sharp;
        assert_eq!(m.modulate(Value::Number(0.5), 1.0, &audio), Value::Number(0.625));
        m.source = AudioBand::Beat;
        m.curve = Curve::Linear;
        assert_eq!(m.modulate(Value::Number(0.5), 1.0, &audio), Value::Number(2.5));
        m.enabled = false;
        assert_eq!(m.modulate(Value::Number(0.5), 1.0, &audio), Value::Number(0.5));
        // Values stay usable: bloom never goes negative.
        let m = Modulator { amount: -10.0, smoothing: 0.0, ..Modulator::new(Property::Bloom, AudioBand::Bass) };
        assert_eq!(m.modulate(Value::Number(0.5), 1.0, &audio), Value::Number(0.0));
    }

    #[test]
    fn settings_that_move_balls_only_hear_the_song() {
        let mix = steady(Levels { bass: 1.0, ..Levels::default() });
        let mut m = Modulator { amount: 100.0, smoothing: 0.0, ..Modulator::new(Property::Gravity, AudioBand::Bass) };
        assert_eq!(m.from, AudioFrom::Song);
        // Even when set to the mix by hand, the bounce sounds can't move the balls.
        m.from = AudioFrom::Mix;
        assert_eq!(m.listens_to(), AudioFrom::Song);
        let audio = Audio { song: None, mix: Some(&mix) };
        assert_eq!(m.modulate(Value::Number(160.0), 1.0, &audio), Value::Number(160.0));
        let audio = Audio { song: Some(&mix), mix: None };
        assert_eq!(m.modulate(Value::Number(160.0), 1.0, &audio), Value::Number(260.0));
    }

    #[test]
    fn smoothing_only_looks_back_and_stays_steady() {
        // A short loud moment from t = 1.
        let hit = |t: f64| Levels { bass: if (1.0..1.06).contains(&t) { 1.0 } else { 0.0 }, ..Levels::default() };
        let audio = Audio { song: None, mix: Some(&hit) };
        let m = Modulator { smoothing: 0.2, ..Modulator::new(Property::Bloom, AudioBand::Bass) };
        assert_eq!(m.strength(0.99, &audio), Some(0.0), "never reacts before the sound");
        let at = m.strength(1.0, &audio).unwrap();
        let later = m.strength(1.2, &audio).unwrap();
        assert!(at > 0.0 && later > 0.0 && later < at, "{at} {later}");
        assert_eq!(m.strength(1.0, &audio), m.strength(1.0, &audio), "same time, same value");
        // Odd values don't break anything.
        let odd = Modulator { smoothing: f32::NAN, amount: f32::INFINITY, ..m.clone() };
        assert_eq!(odd.modulate(Value::Number(1.0), 1.0, &audio), Value::Number(1.0));
        let nan = |_| Levels { bass: f32::NAN, ..Levels::default() };
        let audio = Audio { song: None, mix: Some(&nan) };
        assert_eq!(m.strength(1.0, &audio), Some(0.0));
    }

    #[test]
    fn colours_pulse_towards_the_target() {
        let mix = steady(Levels { bass: 1.0, ..Levels::default() });
        let audio = Audio { song: None, mix: Some(&mix) };
        let red = Color::rgb(255, 0, 0);
        let m = Modulator { amount: 1.0, smoothing: 0.0, color: red, ..Modulator::new(Property::BackgroundTop, AudioBand::Bass) };
        assert_eq!(m.modulate(Value::Color(Color::rgb(0, 0, 0)), 0.0, &audio), Value::Color(red));
    }

    #[test]
    fn apply_modulators_after_keyframes_and_by_kind() {
        let mix = steady(Levels { bass: 1.0, ..Levels::default() });
        let audio = Audio { song: Some(&mix), mix: Some(&mix) };
        let mut scene = Scene::default();
        let mut track = Track::new(Property::Bloom);
        track.set_key(0.0, Value::Number(1.0));
        scene.animation.push(track);
        let modulators = vec![
            Modulator { amount: 0.5, smoothing: 0.0, ..Modulator::new(Property::Bloom, AudioBand::Bass) },
            Modulator { amount: 10.0, smoothing: 0.0, ..Modulator::new(Property::Gravity, AudioBand::Bass) },
        ];
        let gravity = scene.physics.gravity.y;
        let mut sim = scene.clone();
        apply_modulators(&mut sim, &modulators, 1.0, true, &audio);
        assert_eq!((sim.physics.gravity.y, sim.look.bloom_strength), (gravity + 10.0, scene.look.bloom_strength));
        scene.apply_animation(1.0, false);
        apply_modulators(&mut scene, &modulators, 1.0, false, &audio);
        assert_eq!(scene.look.bloom_strength, 1.5);
    }

    #[test]
    fn modulators_follow_their_effects() {
        let mut modulators = vec![
            Modulator::new(Property::EffectAmount(0), AudioBand::Bass),
            Modulator::new(Property::EffectParam(2, "zoom".into()), AudioBand::Bass),
            Modulator::new(Property::Bloom, AudioBand::Bass),
        ];
        remap_effect_modulators(&mut modulators, |i| if i == 0 { None } else { Some(i - 1) });
        let properties: Vec<_> = modulators.iter().map(|m| m.property.clone()).collect();
        assert_eq!(properties, [Property::EffectParam(1, "zoom".into()), Property::Bloom]);
    }

    #[test]
    fn modulators_never_pile_up_when_applied_in_place() {
        let song = steady(Levels { bass: 1.0, ..Levels::default() });
        let audio = Audio { song: Some(&song), mix: None };
        let mut scene = Scene::default();
        let gravity = scene.physics.gravity.y;
        scene.modulators.push(Modulator { amount: 50.0, smoothing: 0.0, ..Modulator::new(Property::Gravity, AudioBand::Bass) });
        assert!(scene.animates_simulation());
        for step in 0..10 {
            scene.apply_animation_with(step as f32 / 240.0, true, &audio);
            assert_eq!(scene.physics.gravity.y, gravity + 50.0);
        }
        // A quiet moment puts the plain value back.
        let quiet = steady(Levels::default());
        scene.apply_animation_with(1.0, true, &Audio { song: Some(&quiet), mix: None });
        assert_eq!(scene.physics.gravity.y, gravity);
        // Keyframes are the base when there are some.
        let mut track = Track::new(Property::Gravity);
        track.set_key(0.0, Value::Number(500.0));
        scene.animation.push(track);
        for _ in 0..3 {
            scene.apply_animation_with(2.0, true, &audio);
            assert_eq!(scene.physics.gravity.y, 550.0);
        }
        // The plain scene used for pictures is never changed by at_time_with.
        let shown = scene.at_time_with(2.0, &audio);
        assert_eq!(shown.physics.gravity.y, 550.0);
        assert_eq!(shown, scene, "the hidden offset doesn't count as a change");
    }

    #[test]
    fn effect_modulators_move_and_copy_with_their_effects() {
        let mut scene = Scene::default();
        scene.effects = vec![Effect::new("echo"), Effect::new("film")];
        scene.modulators.push(Modulator::new(Property::EffectAmount(1), AudioBand::Treble));
        scene.copy_effect_tracks(1, 0);
        assert_eq!(scene.modulators.len(), 2);
        scene.remap_effect_tracks(|i| if i == 0 { None } else { Some(0) });
        assert_eq!(scene.modulators.iter().map(|m| m.property.clone()).collect::<Vec<_>>(), [Property::EffectAmount(0)]);
    }

    #[test]
    fn modulators_save_and_load() {
        let m = Modulator { curve: Curve::Gate, source: AudioBand::Onset, ..Modulator::new(Property::Exposure, AudioBand::Mid) };
        let text = ron::to_string(&vec![m.clone()]).unwrap();
        assert_eq!(ron::from_str::<Vec<Modulator>>(&text).unwrap(), vec![m]);
        // Missing fields take their defaults.
        let short: Modulator = ron::from_str("(property: Bloom)").unwrap();
        assert_eq!(short, Modulator::default());
    }

    fn track(keys: &[(f32, f32)], ease: Ease) -> Track {
        Track {
            property: Property::Bloom,
            keys: keys.iter().map(|&(time, v)| Key { time, value: Value::Number(v), ease }).collect(),
        }
    }

    fn number(value: Option<Value>) -> f32 {
        value.and_then(Value::number).unwrap()
    }

    #[test]
    fn values_hold_outside_the_keyframes_and_blend_between() {
        let track = track(&[(1.0, 10.0), (3.0, 20.0)], Ease::Linear);
        assert_eq!(number(track.value_at(0.0)), 10.0);
        assert_eq!(number(track.value_at(2.0)), 15.0);
        assert_eq!(number(track.value_at(3.0)), 20.0);
        assert_eq!(number(track.value_at(9.0)), 20.0);
        assert!(Track::new(Property::Bloom).value_at(1.0).is_none());
    }

    #[test]
    fn eases_shape_the_middle() {
        let at = |ease| number(track(&[(0.0, 0.0), (1.0, 1.0)], ease).value_at(0.25));
        assert_eq!(at(Ease::Hold), 0.0);
        assert!(at(Ease::EaseIn) < at(Ease::Smooth));
        assert!(at(Ease::Smooth) < at(Ease::Linear));
        assert!(at(Ease::Linear) < at(Ease::EaseOut));
        for ease in Ease::ALL {
            assert_eq!(ease.apply(0.0), 0.0, "{ease:?}");
            assert_eq!(ease.apply(1.0), if ease == Ease::Hold { 0.0 } else { 1.0 }, "{ease:?}");
        }
    }

    #[test]
    fn colours_blend() {
        let black = Color::rgb(0, 0, 0);
        let white = Color::WHITE;
        let track = Track {
            property: Property::BackgroundTop,
            keys: vec![
                Key { time: 0.0, value: Value::Color(black), ease: Ease::Linear },
                Key { time: 1.0, value: Value::Color(white), ease: Ease::Linear },
            ],
        };
        let Some(Value::Color(mid)) = track.value_at(0.5) else { panic!() };
        // Halfway in linear light is brighter than halfway in sRGB numbers.
        assert!(mid.r > 128 && mid.r == mid.g && mid.g == mid.b, "{mid:?}");
        assert_eq!(track.value_at(1.0), Some(Value::Color(white)));
    }

    #[test]
    fn setting_keys_keeps_them_in_order() {
        let mut track = Track::new(Property::Bloom);
        track.set_key(2.0, Value::Number(2.0));
        track.set_key(1.0, Value::Number(1.0));
        track.set_key(3.0, Value::Number(3.0));
        assert_eq!(track.set_key(2.001, Value::Number(5.0)), 1);
        assert_eq!(track.keys.iter().map(|k| k.time).collect::<Vec<_>>(), [1.0, 2.0, 3.0]);
        assert_eq!(track.keys[1].value, Value::Number(5.0));
        assert_eq!(track.move_key(0, 2.5), 1);
        assert_eq!(track.keys.iter().map(|k| k.time).collect::<Vec<_>>(), [2.0, 2.5, 3.0]);
    }

    #[test]
    fn at_time_sets_animated_settings_only() {
        let mut scene = Scene::default();
        let mut bloom = track(&[(0.0, 0.0), (2.0, 1.0)], Ease::Linear);
        bloom.property = Property::Bloom;
        scene.animation.push(bloom);
        let later = scene.at_time(1.0);
        assert_eq!(later.look.bloom_strength, 0.5);
        assert_eq!(later.look.exposure, scene.look.exposure);
        assert_eq!(later.animation, scene.animation);
        assert!(!scene.animates_simulation());
    }

    #[test]
    fn every_plain_setting_round_trips() {
        let scene = Scene::default();
        for property in Property::PLAIN {
            let value = property.get(&scene).unwrap();
            let changed = match value {
                Value::Number(v) => Value::Number(v + 1.5),
                Value::Color(_) => Value::Color(Color::rgb(1, 2, 3)),
            };
            let mut copy = scene.clone();
            property.set(&mut copy, changed);
            assert_eq!(property.get(&copy), Some(changed), "{property:?}");
            property.copy(&scene, &mut copy);
            assert_eq!(copy, scene, "{property:?}");
        }
    }

    #[test]
    fn effect_sliders_left_at_default_copy_back_as_unset() {
        let mut scene = Scene::default();
        scene.effects.push(Effect::new("echo"));
        let property = Property::EffectParam(0, "zoom".into());
        assert_eq!(property.get(&scene), None);
        let mut copy = scene.clone();
        property.set(&mut copy, Value::Number(2.0));
        assert_eq!(property.get(&copy), Some(Value::Number(2.0)));
        property.copy(&scene, &mut copy);
        assert_eq!(copy, scene);
    }

    #[test]
    fn effect_tracks_follow_their_effects() {
        let mut scene = Scene::default();
        for i in 0..3 {
            scene.animation.push(Track::new(Property::EffectAmount(i)));
        }
        scene.animation.push(Track::new(Property::Bloom));
        // Remove effect 1.
        scene.remap_effect_tracks(|i| match i {
            1 => None,
            i if i > 1 => Some(i - 1),
            i => Some(i),
        });
        let names: Vec<_> = scene.animation.iter().map(|t| t.property.clone()).collect();
        assert_eq!(names, [Property::EffectAmount(0), Property::EffectAmount(1), Property::Bloom]);
        scene.copy_effect_tracks(0, 1);
        assert_eq!(scene.animation.iter().filter(|t| t.property == Property::EffectAmount(1)).count(), 1);
    }

    #[test]
    fn scenes_with_keyframes_save_and_load() {
        let mut scene = Scene::default();
        let mut track = Track::new(Property::EffectParam(0, "zoom".into()));
        track.set_key(1.0, Value::Number(2.0));
        scene.animation.push(track);
        let mut colour = Track::new(Property::BackgroundTop);
        colour.set_key(0.5, Value::Color(Color::rgb(10, 20, 30)));
        colour.keys[0].ease = Ease::Hold;
        scene.animation.push(colour);
        let text = scene.to_ron().unwrap();
        assert_eq!(Scene::from_ron(&text).unwrap(), scene);
    }
}

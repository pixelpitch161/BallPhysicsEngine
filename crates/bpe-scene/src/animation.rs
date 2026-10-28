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
    // ---- 3D (WS4): see `crate::view`. They only change the picture. ----
    /// Degrees around the target, from in front.
    CameraTurn,
    /// Degrees above the target.
    CameraHeight,
    CameraDistance,
    CameraFov,
    /// Where the main light comes from, in degrees.
    LightTurn,
    LightHeight,
    LightBrightness,
    /// How much the glass of the container shows.
    Glass,
    /// Glow of the container's edges and tunnel rings.
    EdgeGlow,
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

/// A setting that follows the music, such as an effect amount pulsing with the bass. Owned by
/// the audio-reactive workstream, which replaces this placeholder.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Modulator {}

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
    /// Whether two values are the same but for rounding errors, such as a 3D camera angle
    /// read back after another angle changed.
    pub fn same(self, other: Value) -> bool {
        match (self, other) {
            (Value::Number(a), Value::Number(b)) => (a - b).abs() <= 1e-4 * a.abs().max(b.abs()).max(1.0),
            (a, b) => a == b,
        }
    }

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

    /// 3D (WS4): the settings of a 3D scene. A flat scene has none of them.
    pub const SPACE: [Property; 9] = [
        Property::CameraTurn,
        Property::CameraHeight,
        Property::CameraDistance,
        Property::CameraFov,
        Property::LightTurn,
        Property::LightHeight,
        Property::LightBrightness,
        Property::Glass,
        Property::EdgeGlow,
    ];

    /// 3D (WS4): whether this is one of [`Property::SPACE`].
    pub fn is_space(&self) -> bool {
        Property::SPACE.contains(self)
    }

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
            // 3D (WS4)
            property => number(scene.space()?.number(property)?),
        }
    }

    /// Sets the setting's plain value. Values of the wrong kind, and settings the scene doesn't
    /// have, are ignored.
    pub fn set(&self, scene: &mut Scene, value: Value) {
        match value {
            Value::Number(v) => {
                if let Some(slot) = self.number_mut(scene) {
                    *slot = v;
                } else if let Some(space) = scene.space_mut() {
                    space.set_number(self, v); // 3D (WS4)
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

    pub fn track(&self, property: &Property) -> Option<&Track> {
        self.animation.iter().find(|t| t.property == *property)
    }

    pub fn track_mut(&mut self, property: &Property) -> Option<&mut Track> {
        self.animation.iter_mut().find(|t| t.property == *property)
    }

    /// Whether any animated setting changes how balls move.
    pub fn animates_simulation(&self) -> bool {
        self.animation.iter().any(|t| t.property.affects_simulation() && !t.keys.is_empty())
    }

    /// Keeps effect animations attached to their effects after the effect list changes:
    /// `map` gives each old effect index its new one, or None if the effect is gone.
    pub fn remap_effect_tracks(&mut self, map: impl Fn(u32) -> Option<u32>) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Effect;

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

    /// 3D (WS4)
    #[test]
    fn every_space_setting_round_trips() {
        let scene = Scene { view: crate::View::Space(Box::default()), ..Scene::default() };
        for property in Property::SPACE {
            assert_eq!(property.get(&Scene::default()), None, "{property:?}");
            let Some(Value::Number(v)) = property.get(&scene) else { panic!("{property:?}") };
            let changed = Value::Number(v + 1.5);
            let mut copy = scene.clone();
            property.set(&mut copy, changed);
            let Some(Value::Number(now)) = property.get(&copy) else { panic!() };
            assert!((now - (v + 1.5)).abs() < 0.01, "{property:?}: {now} after setting {}", v + 1.5);
            property.copy(&scene, &mut copy);
            let Some(Value::Number(back)) = property.get(&copy) else { panic!() };
            assert!((back - v).abs() < 0.01, "{property:?}");
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

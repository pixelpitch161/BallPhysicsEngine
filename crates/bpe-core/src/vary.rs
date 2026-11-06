//! Vary across balls: a setting of a group of balls that gives each ball its own value, between
//! the setting's own value and a second one, **To**, by where the ball sits in a spread (its
//! number, round, launch angle, place, a random pick, its speed or its hits).
//!
//! A varied setting keeps everything beside its value in the same params: `"<key>~to"` (the
//! other end), `"<key>~by"` (the spread) and `"<key>~ease"` (how the values bunch). A layer's
//! keep their layer prefix: `"3/width~to"`. A setting varies exactly when its `~by` key is there.

use std::sync::LazyLock;

use crate::{Choice, Color, Depth, Flags, Gradient, Params, SettingDef, Stop, Value, ValueType};

/// What follows a key to name its other end, its spread and its easing.
pub const TO: &str = "~to";
pub const BY: &str = "~by";
pub const EASE: &str = "~ease";

/// How many steps a spread is cut into where values are shared between balls (a card's or a
/// look's settings): 0 and 1 are steps, so both ends are exact.
pub const VARY_LEVELS: u32 = 1024;

/// What spreads a setting's values across the balls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum VaryBy {
    /// The balls' numbers, first to last.
    #[default]
    Number,
    /// The round a ball was fired in, first to last.
    Round,
    /// The direction a ball was launched in, round the circle from pointing right.
    LaunchAngle,
    /// Where a ball starts in the group, from left to right.
    Place,
    /// A pick for each ball, the same every run.
    Random,
    /// How fast a ball goes, up to 1500 px/s.
    Speed,
    /// How many times a ball has hit something, up to 10.
    Hits,
    /// How much light falls on a ball (with a Lighting card: lighting part 7's light probe).
    Light,
    /// How much of the light that would reach a ball its shadows take.
    Shadow,
}

impl VaryBy {
    /// Whether a ball's place in this spread changes while it flies (its speed, its hits).
    pub fn changes(self) -> bool {
        matches!(self, Self::Speed | Self::Hits | Self::Light | Self::Shadow)
    }

    /// Whether a ball's place in this spread is read from the light probe.
    pub fn reads_light(self) -> bool {
        matches!(self, Self::Light | Self::Shadow)
    }

    /// The spreads offered for a setting read the way `use_` says: one read once at launch
    /// can't follow the speed, hits or light. Light and Shadow only where `lit` (a card's or a
    /// layer's setting in a scene with a Lighting card: a thing's own settings are worked out by
    /// its sim, which has no light probe).
    pub fn offered(use_: VaryUse, lit: bool) -> impl Iterator<Item = VaryBy> {
        Self::NAMES
            .iter()
            .filter_map(|n| VaryBy::from_name(n))
            .filter(move |b| use_ == VaryUse::Live || !b.changes())
            .filter(move |b| lit || !b.reads_light())
    }
}

/// How the values bunch between the two ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum VaryEase {
    #[default]
    Even,
    EaseIn,
    EaseOut,
    EaseBoth,
}

impl VaryEase {
    /// `t` (0 to 1) eased: Even straight, Ease in squared, Ease out the other way, Ease both an
    /// S (the group gradient's easings at strength 2).
    pub fn apply(self, t: f32) -> f32 {
        let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
        match self {
            Self::Even => t,
            Self::EaseIn => t * t,
            Self::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
            Self::EaseBoth => {
                let (a, b) = (t * t, (1.0 - t) * (1.0 - t));
                if a + b > 0.0 { a / (a + b) } else { t }
            }
        }
    }
}

/// How a setting that may vary is read, which decides the spreads it is offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaryUse {
    /// Read while the ball lives: every spread.
    Live,
    /// Read once, when the ball is let go: not Speed or Hits.
    AtLaunch,
}

/// Which part of a vary a key names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaryPart {
    To,
    By,
    Ease,
}

impl VaryPart {
    fn suffix(self) -> &'static str {
        match self {
            Self::To => TO,
            Self::By => BY,
            Self::Ease => EASE,
        }
    }
}

/// `key`'s other end: `"radius~to"`.
pub fn to_key(key: &str) -> String {
    format!("{key}{TO}")
}

/// `key`'s spread: `"radius~by"`.
pub fn by_key(key: &str) -> String {
    format!("{key}{BY}")
}

/// `key`'s easing: `"radius~ease"`.
pub fn ease_key(key: &str) -> String {
    format!("{key}{EASE}")
}

/// `key`'s `part`.
pub fn part_key(key: &str, part: VaryPart) -> String {
    format!("{key}{}", part.suffix())
}

/// The setting a vary key belongs to and which part it is: `"3/width~to"` is `("3/width", To)`.
/// `None` for any other key.
pub fn split(key: &str) -> Option<(&str, VaryPart)> {
    [VaryPart::To, VaryPart::By, VaryPart::Ease]
        .into_iter()
        .find_map(|p| key.strip_suffix(p.suffix()).filter(|b| !b.is_empty()).map(|b| (b, p)))
}

/// The setting `key` belongs to: itself unless it is a vary key.
pub fn base_key(key: &str) -> &str {
    split(key).map_or(key, |(b, _)| b)
}

pub fn is_vary_key(key: &str) -> bool {
    split(key).is_some()
}

/// Whether a setting of this type can have a second value to vary towards.
pub fn can_vary_type(ty: &ValueType) -> bool {
    matches!(ty, ValueType::Number | ValueType::Int | ValueType::Vec2 | ValueType::Color | ValueType::Gradient)
}

fn choice_def(
    key: &'static str,
    label: &'static str,
    help: &'static str,
    names: &'static [&'static str],
) -> SettingDef {
    SettingDef {
        key,
        label,
        help,
        ty: ValueType::Choice(names),
        default: Value::Choice(names[0].to_owned()),
        range: None,
        span: None,
        unit: "",
        group: "",
        depth: Depth::More,
        flags: Flags::default(),
        when: "",
    }
}

/// What every `~by` key is.
pub static VARY_BY_DEF: LazyLock<SettingDef> =
    LazyLock::new(|| choice_def("~by", "Varies by", "What spreads the values across the balls", VaryBy::NAMES));
/// What every `~ease` key is.
pub static VARY_EASE_DEF: LazyLock<SettingDef> =
    LazyLock::new(|| choice_def("~ease", "Easing", "How the values bunch between the two ends", VaryEase::NAMES));

/// The description of a vary key's `part` for a setting described by `base`: To is the setting
/// itself (its type, range and keyframes), the spread and easing their own choices. `None` when
/// there is no such setting or it can't vary.
pub fn def_for(base: Option<&'static SettingDef>, part: VaryPart) -> Option<&'static SettingDef> {
    let base = base.filter(|d| can_vary_type(&d.ty))?;
    Some(match part {
        VaryPart::To => base,
        VaryPart::By => &VARY_BY_DEF,
        VaryPart::Ease => &VARY_EASE_DEF,
    })
}

/// One varied setting as its params hold it.
#[derive(Clone, Debug, PartialEq)]
pub struct Vary {
    /// The setting's own key (layered keys keep their prefix).
    pub key: String,
    /// The other end; `None` when it isn't stored (its default).
    pub to: Option<Value>,
    pub by: VaryBy,
    pub ease: VaryEase,
}

impl Vary {
    /// How far between the two ends a ball at `t` in the spread sits: `t` eased.
    pub fn amount(&self, t: f32) -> f32 {
        self.ease.apply(t)
    }

    /// This vary's random salt: different for every setting, so two settings varied by Random
    /// pick independently.
    pub fn salt(&self) -> u32 {
        salt(&self.key)
    }
}

fn choice<C: Choice>(v: Option<&Value>) -> Option<C> {
    match v {
        Some(Value::Choice(n)) => C::from_name(n),
        _ => None,
    }
}

/// The vary of setting `key` in `params`, when it varies.
pub fn vary_of(params: &Params, key: &str) -> Option<Vary> {
    let by = choice::<VaryBy>(params.get(&by_key(key)))?;
    Some(Vary {
        key: key.to_owned(),
        to: params.get(&to_key(key)).cloned(),
        by,
        ease: choice::<VaryEase>(params.get(&ease_key(key))).unwrap_or_default(),
    })
}

/// Every varied setting in `params`, in key order.
pub fn varies_in(params: &Params) -> Vec<Vary> {
    params
        .iter()
        .filter_map(|(k, _)| k.strip_suffix(BY).filter(|b| !b.is_empty()))
        .filter_map(|base| vary_of(params, base))
        .collect()
}

/// What one ball brings to the spreads.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BallSpread {
    /// Its number in the group, and how many the group has in all.
    pub number: u32,
    pub count: u32,
    /// Its round, launch angle and place, each already 0 to 1.
    pub round: f32,
    pub launch: f32,
    pub place: f32,
    /// How fast it goes, in px/s, and its hits so far.
    pub speed: f32,
    pub hits: u32,
    /// How lit it is and how much of its light its shadows take, each 0 to 1 (0 unless some
    /// spread reads them: they are asked of the light probe only then).
    pub light: f32,
    pub shadow: f32,
    /// The group's seed, for Random.
    pub seed: u32,
    /// A ball on its own (a Balls thing that isn't a group): it has nothing to vary across,
    /// so it sits at the start of every spread.
    pub alone: bool,
}

/// How fast a ball reaches the far end for Speed, in px/s (the group gradient's).
pub const FULL_SPEED: f32 = 1500.0;
/// How many hits reach the far end for Hits.
pub const FULL_HITS: f32 = 10.0;

impl BallSpread {
    /// Where this ball sits in spread `by` (0 to 1), `salt` picking its Random.
    pub fn t(&self, by: VaryBy, salt: u32) -> f32 {
        if self.alone {
            return 0.0;
        }
        let t = match by {
            VaryBy::Number => {
                if self.count <= 1 {
                    0.0
                } else {
                    self.number as f32 / (self.count - 1) as f32
                }
            }
            VaryBy::Round => self.round,
            VaryBy::LaunchAngle => self.launch,
            VaryBy::Place => self.place,
            VaryBy::Random => {
                (mix32(mix32(self.seed ^ 0x5bd1_e995) ^ self.number.wrapping_mul(0x9e37_79b9) ^ salt) >> 8) as f32
                    / (1u32 << 24) as f32
            }
            VaryBy::Speed => self.speed / FULL_SPEED,
            VaryBy::Hits => self.hits as f32 / FULL_HITS,
            VaryBy::Light => self.light,
            VaryBy::Shadow => self.shadow,
        };
        if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 }
    }
}

/// A launch direction (clockwise from pointing right, y down) as 0 to 1 round the circle.
pub fn launch_turn(direction: glam::Vec2) -> f32 {
    if direction.length_squared() < 1e-12 {
        return 0.0;
    }
    let deg = direction.y.atan2(direction.x).to_degrees();
    (deg.rem_euclid(360.0) / 360.0).clamp(0.0, 1.0)
}

fn mix32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

/// A key's own number for Random (FNV-1a).
pub fn salt(key: &str) -> u32 {
    key.bytes().fold(0x811c_9dc5u32, |h, b| (h ^ b as u32).wrapping_mul(0x0100_0193))
}

/// `t` cut to one of [`VARY_LEVELS`] steps.
pub fn level(t: f32) -> u16 {
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
    (t * (VARY_LEVELS - 1) as f32).round() as u16
}

/// Where step `q` sits, 0 to 1.
pub fn at_level(q: u16) -> f32 {
    q as f32 / (VARY_LEVELS - 1) as f32
}

/// The value `amount` (0 to 1, eased already) of the way from `value` to `to`.
pub fn mixed(value: &Value, to: &Value, amount: f32) -> Value {
    if amount <= 0.0 {
        return value.clone();
    }
    value.lerp(to, amount.min(1.0))
}

/// `params` with each of `varies` mixed `ts[i]` of the way (before easing) to its To. `default`
/// gives a To that isn't stored; a value that isn't there is left alone.
pub fn with_varies(params: &Params, varies: &[Vary], ts: &[f32], default: &dyn Fn(&str) -> Option<Value>) -> Params {
    let mut out = params.clone();
    for (v, &t) in varies.iter().zip(ts) {
        let Some(value) = params.get(&v.key) else { continue };
        let Some(to) = v.to.clone().or_else(|| default(&v.key)) else { continue };
        out.set(&v.key, mixed(value, &to, v.amount(t)));
    }
    out
}

/// The other end a setting starts varying towards from `value`: somewhere clearly different, so
/// the balls show it straight away. A number goes a quarter of its slider's span towards its far
/// end; a colour or gradient turns a third of the way round the colour wheel; a pair grows by half.
pub fn starting_to(def: &SettingDef, value: &Value) -> Value {
    // A grey has no hue to turn: it goes to a clear colour of its own brightness instead.
    let turn = |c: Color| {
        let (h, s, v) = c.to_hsv();
        if s < 0.15 {
            Color { a: c.a, ..Color::from_hsv(0.58, 0.7, v.max(0.5)) }
        } else {
            Color { a: c.a, ..Color::from_hsv(h + 1.0 / 3.0, s, v) }
        }
    };
    match value {
        Value::Number(x) => Value::Number(nudge(def, *x)),
        Value::Int(x) => {
            let to = nudge(def, *x as f64).round() as i64;
            let to = if to == *x {
                if def.range.is_some_and(|(_, hi)| (*x as f64) >= hi) { x - 1 } else { x + 1 }
            } else {
                to
            };
            def.clamp(Value::Int(to))
        }
        Value::Vec2([x, y]) => {
            if x.abs() + y.abs() < 1e-6 {
                Value::Vec2([100.0, 0.0])
            } else {
                Value::Vec2([x * 1.5, y * 1.5])
            }
        }
        Value::Color(c) => Value::Color(turn(*c)),
        Value::Gradient(g) => Value::Gradient(Gradient {
            stops: g.stops.iter().map(|s| Stop { at: s.at, color: turn(s.color) }).collect(),
            transition: g.transition,
        }),
        other => other.clone(),
    }
}

fn nudge(def: &SettingDef, x: f64) -> f64 {
    let Some((lo, hi)) = def.range.or(def.span).filter(|(lo, hi)| hi > lo) else {
        return if x.abs() > 1e-9 { x * 2.0 } else { 1.0 };
    };
    // The slider's span for a setting with a very wide hard range is its usual span, if it has one.
    let (lo, hi) = if def.span.is_some() && def.range.is_some() { def.span.unwrap_or((lo, hi)) } else { (lo, hi) };
    let step = (hi - lo) * 0.25;
    let to = if x <= (lo + hi) * 0.5 { x + step } else { x - step };
    match def.range {
        Some((a, b)) => to.clamp(a, b),
        None => to,
    }
}

impl crate::Scene {
    /// Starts setting `a` varying across the balls: spread by Number, Even, towards
    /// [`starting_to`] of its value now. Nothing changes when it varies already.
    pub fn start_varying(&mut self, reg: &crate::Registry, a: &crate::Address) -> Result<(), crate::EditError> {
        let def = self.def_at(reg, a).ok_or_else(|| crate::EditError::UnknownSetting(a.key.clone()))?;
        if self.vary_use(reg, a).is_none() {
            return Err(crate::EditError::WrongType);
        }
        let part = |p: VaryPart| crate::Address { key: part_key(&a.key, p), ..a.clone() };
        if self.value_of_part(a, VaryPart::By).is_some() {
            return Ok(());
        }
        let value = self.value_at(reg, a).unwrap_or_else(|| def.default.clone());
        self.set_at(reg, &part(VaryPart::To), starting_to(def, &value))?;
        self.set_at(reg, &part(VaryPart::Ease), Value::Choice(VaryEase::Even.name().into()))?;
        self.set_at(reg, &part(VaryPart::By), Value::Choice(VaryBy::Number.name().into()))
    }

    /// Whether setting `a` can vary across the balls, and how it is read: as the kind of thing,
    /// card or layer it belongs to says (never a vary key itself, nor a type that can't mix).
    pub fn vary_use(&self, reg: &crate::Registry, a: &crate::Address) -> Option<VaryUse> {
        if is_vary_key(&a.key) {
            return None;
        }
        let def = self.def_at(reg, a)?;
        if !can_vary_type(&def.ty) {
            return None;
        }
        let thing = self.things.get(&a.thing)?;
        match a.slot {
            None => reg.thing(&thing.kind)?.vary(&a.key),
            Some(slot) => {
                let card = thing.slots.get(&slot)?;
                match crate::split_layered(&a.key) {
                    Some((id, _)) => reg.layer(&card.layer(id)?.kind)?.vary(def),
                    None => reg.module(&card.kind)?.vary(&a.key),
                }
            }
        }
    }

    /// Stops setting `a` varying: its To, spread and easing go, with any keys and pulse on To.
    /// False when it didn't vary.
    pub fn stop_varying(&mut self, a: &crate::Address) -> bool {
        let mut any = false;
        for p in [VaryPart::To, VaryPart::By, VaryPart::Ease] {
            let at = crate::Address { key: part_key(&a.key, p), ..a.clone() };
            any |= self.tracks.remove(&at).is_some();
            any |= self.pulses.remove(&at).is_some();
            let Some(thing) = self.things.get_mut(&a.thing) else { continue };
            let params = match a.slot {
                None => &mut thing.params,
                Some(slot) => match thing.slots.get_mut(&slot) {
                    Some(card) => &mut card.params,
                    None => continue,
                },
            };
            any |= params.remove(&at.key).is_some();
        }
        any
    }

    /// The stored value of part `p` of setting `a`'s vary, if any.
    pub fn value_of_part(&self, a: &crate::Address, p: VaryPart) -> Option<&Value> {
        let thing = self.things.get(&a.thing)?;
        let params = match a.slot {
            None => &thing.params,
            Some(slot) => &thing.slots.get(&slot)?.params,
        };
        params.get(&part_key(&a.key, p))
    }

    /// How setting `a` varies, when it does.
    pub fn vary_at(&self, a: &crate::Address) -> Option<Vary> {
        let thing = self.things.get(&a.thing)?;
        let params = match a.slot {
            None => &thing.params,
            Some(slot) => &thing.slots.get(&slot)?.params,
        };
        vary_of(params, &a.key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_split_into_their_setting_and_part() {
        assert_eq!(split("radius~to"), Some(("radius", VaryPart::To)));
        assert_eq!(split("3/width~by"), Some(("3/width", VaryPart::By)));
        assert_eq!(split("3/width~ease"), Some(("3/width", VaryPart::Ease)));
        assert_eq!(split("radius"), None);
        assert_eq!(split("~to"), None, "a part needs a setting");
        assert_eq!(base_key("3/width~to"), "3/width");
        assert_eq!(base_key("speed"), "speed");
        assert_eq!(to_key("a"), "a~to");
        assert_eq!(part_key("a", VaryPart::Ease), "a~ease");
    }

    #[test]
    fn spreads_run_from_nothing_to_all() {
        let b = |number| BallSpread { number, count: 5, ..BallSpread::default() };
        assert_eq!(b(0).t(VaryBy::Number, 0), 0.0);
        assert_eq!(b(4).t(VaryBy::Number, 0), 1.0);
        assert_eq!(b(2).t(VaryBy::Number, 0), 0.5);
        let one = BallSpread { count: 1, ..BallSpread::default() };
        assert_eq!(one.t(VaryBy::Number, 0), 0.0);
        let fast =
            BallSpread { speed: 3000.0, hits: 5, place: 0.25, round: 0.5, launch: 0.75, ..BallSpread::default() };
        assert_eq!(fast.t(VaryBy::Speed, 0), 1.0);
        assert_eq!(fast.t(VaryBy::Hits, 0), 0.5);
        assert_eq!(fast.t(VaryBy::Place, 0), 0.25);
        assert_eq!(fast.t(VaryBy::Round, 0), 0.5);
        assert_eq!(fast.t(VaryBy::LaunchAngle, 0), 0.75);
        let nan = BallSpread { place: f32::NAN, ..BallSpread::default() };
        assert_eq!(nan.t(VaryBy::Place, 0), 0.0);
        // A ball on its own has nothing to vary across: every spread starts it at From.
        let alone = BallSpread { alone: true, ..fast };
        for by in [VaryBy::Speed, VaryBy::Hits, VaryBy::Random, VaryBy::Place, VaryBy::LaunchAngle] {
            assert_eq!(alone.t(by, 7), 0.0, "{by:?}");
        }
    }

    #[test]
    fn random_is_stable_and_differs_by_setting() {
        let ts = |salt| {
            (0..50)
                .map(|n| BallSpread { number: n, seed: 3, ..BallSpread::default() }.t(VaryBy::Random, salt))
                .collect::<Vec<_>>()
        };
        let a = ts(salt("radius"));
        assert_eq!(a, ts(salt("radius")));
        assert_ne!(a, ts(salt("3/width")));
        assert!(a.iter().any(|t| *t < 0.2) && a.iter().any(|t| *t > 0.8));
        assert!(a.iter().all(|t| (0.0..1.0).contains(t)));
    }

    #[test]
    fn launch_turns_clockwise_from_pointing_right() {
        assert_eq!(launch_turn(glam::Vec2::X), 0.0);
        assert!((launch_turn(glam::Vec2::Y) - 0.25).abs() < 1e-6, "down is a quarter turn clockwise");
        assert!((launch_turn(glam::Vec2::NEG_Y) - 0.75).abs() < 1e-6);
        assert_eq!(launch_turn(glam::Vec2::ZERO), 0.0);
    }

    #[test]
    fn eases_keep_their_ends() {
        for e in [VaryEase::Even, VaryEase::EaseIn, VaryEase::EaseOut, VaryEase::EaseBoth] {
            assert_eq!(e.apply(0.0), 0.0);
            assert!((e.apply(1.0) - 1.0).abs() < 1e-6);
        }
        assert_eq!(VaryEase::EaseIn.apply(0.5), 0.25);
        assert_eq!(VaryEase::EaseOut.apply(0.5), 0.75);
        assert!((VaryEase::EaseBoth.apply(0.5) - 0.5).abs() < 1e-6);
        assert!(VaryEase::EaseBoth.apply(0.2) < 0.2);
    }

    #[test]
    fn levels_keep_both_ends_exact() {
        assert_eq!(at_level(level(0.0)), 0.0);
        assert_eq!(at_level(level(1.0)), 1.0);
        assert_eq!(level(f32::NAN), 0);
        assert!((at_level(level(0.3)) - 0.3).abs() < 1.0 / 2000.0);
    }

    #[test]
    fn varies_are_read_from_params() {
        let mut p = Params::default();
        p.set("radius", Value::Number(10.0));
        p.set("radius~by", Value::Choice("hits".into()));
        p.set("radius~to", Value::Number(40.0));
        p.set("3/width~by", Value::Choice("number".into()));
        p.set("3/width~ease", Value::Choice("ease_in".into()));
        p.set("odd~by", Value::Choice("nonsense".into()));
        let v = varies_in(&p);
        assert_eq!(v.len(), 2, "{v:?}");
        assert_eq!(v[0], Vary { key: "3/width".into(), to: None, by: VaryBy::Number, ease: VaryEase::EaseIn });
        assert_eq!(
            v[1],
            Vary { key: "radius".into(), to: Some(Value::Number(40.0)), by: VaryBy::Hits, ease: VaryEase::Even }
        );
        let mixed = with_varies(&p, &v[1..], &[0.5], &|_| None);
        assert_eq!(mixed.get("radius"), Some(&Value::Number(25.0)));
    }

    #[test]
    fn ints_round_and_colours_blend() {
        assert_eq!(mixed(&Value::Int(0), &Value::Int(3), 0.5), Value::Int(2));
        assert_eq!(mixed(&Value::Number(2.0), &Value::Number(4.0), 0.0), Value::Number(2.0));
        let c = mixed(&Value::Color(Color::rgb(255, 0, 0)), &Value::Color(Color::rgb(0, 0, 255)), 1.0);
        assert_eq!(c, Value::Color(Color::rgb(0, 0, 255)));
    }

    fn number_def(range: Option<(f64, f64)>, span: Option<(f64, f64)>) -> SettingDef {
        SettingDef { range, span, ..choice_def("x", "X", "", &["a"]) }
    }

    #[test]
    fn a_new_vary_starts_somewhere_else() {
        let d = number_def(Some((0.0, 1.0)), None);
        assert_eq!(starting_to(&d, &Value::Number(0.2)), Value::Number(0.45));
        assert_eq!(starting_to(&d, &Value::Number(0.9)), Value::Number(0.65));
        let wide = number_def(Some((0.5, 200.0)), Some((1.0, 50.0)));
        assert_ne!(starting_to(&wide, &Value::Number(6.0)), Value::Number(6.0));
        let free = number_def(None, None);
        assert_eq!(starting_to(&free, &Value::Number(3.0)), Value::Number(6.0));
        assert_eq!(starting_to(&free, &Value::Number(0.0)), Value::Number(1.0));
        let ints = number_def(Some((0.0, 2.0)), None);
        assert_ne!(starting_to(&ints, &Value::Int(2)), Value::Int(2));
        assert_ne!(starting_to(&ints, &Value::Int(0)), Value::Int(0));
        let red = Value::Color(Color::rgb(255, 0, 0));
        assert_ne!(starting_to(&d, &red), red);
        assert_eq!(starting_to(&d, &Value::Vec2([2.0, 0.0])), Value::Vec2([3.0, 0.0]));
        assert_eq!(starting_to(&d, &Value::Vec2([0.0, 0.0])), Value::Vec2([100.0, 0.0]));
    }

    #[test]
    fn at_launch_settings_leave_out_speed_and_hits() {
        let live: Vec<_> = VaryBy::offered(VaryUse::Live, false).collect();
        assert_eq!(live.len(), 7);
        let lit: Vec<_> = VaryBy::offered(VaryUse::Live, true).collect();
        assert!(lit.contains(&VaryBy::Light) && lit.contains(&VaryBy::Shadow) && lit.len() == 9);
        let once: Vec<_> = VaryBy::offered(VaryUse::AtLaunch, true).collect();
        assert!(!once.contains(&VaryBy::Speed) && !once.contains(&VaryBy::Hits) && once.len() == 5);
    }

    #[test]
    fn light_and_shadow_spreads_read_the_balls_light() {
        let b = BallSpread { light: 0.25, shadow: 0.75, count: 3, ..BallSpread::default() };
        assert_eq!(b.t(VaryBy::Light, 0), 0.25);
        assert_eq!(b.t(VaryBy::Shadow, 0), 0.75);
        assert!(VaryBy::Light.changes() && VaryBy::Shadow.reads_light() && !VaryBy::Speed.reads_light());
    }

    #[test]
    fn a_vary_is_set_kept_keyed_and_taken_away() {
        use crate::{Address, SlotId, ThingId, testkit};
        let r = testkit::registry();
        let mut s = crate::Scene::empty(&r);
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.add_slot(&r, b, SlotId::Look, "look.plain").unwrap();
        let glow = Address::on(b, SlotId::Look, "glow");
        let before = s.clone();
        s.start_varying(&r, &glow).unwrap();
        let v = s.vary_at(&glow).unwrap();
        assert_eq!((v.by, v.ease), (VaryBy::Number, VaryEase::Even));
        assert_ne!(v.to, s.value_at(&r, &glow), "To starts somewhere else");
        // The parts' descriptions: To is the setting's own, the spread a choice.
        let to = Address { key: to_key("glow"), ..glow.clone() };
        assert_eq!(s.def_at(&r, &to).map(|d| d.key), Some("glow"));
        assert_eq!(s.def_at(&r, &Address { key: by_key("glow"), ..glow.clone() }).map(|d| d.key), Some("~by"));
        assert!(s.def_at(&r, &Address { key: by_key("nothing"), ..glow.clone() }).is_none());
        // Kept at its default.
        s.set_at(&r, &Address { key: by_key("glow"), ..glow.clone() }, Value::Choice("number".into())).unwrap();
        assert!(s.vary_at(&glow).is_some());
        // Keys on To are ordinary keys, written into the resolved params.
        s.set_key(&r, &to, 0.0, Value::Number(0.5)).unwrap();
        s.set_key(&r, &to, 2.0, Value::Number(1.5)).unwrap();
        let mut resolved = crate::resolve(&s, &r);
        crate::Animation::new(&s, &resolved).apply(&mut resolved, 1.0, &crate::Inputs::NONE);
        assert_eq!(resolved.things[&b].slots[&SlotId::Look].params.get("glow~to"), Some(&Value::Number(1.0)));
        assert_eq!(
            resolved.things[&b].slots[&SlotId::Look].params.get("glow~by"),
            Some(&Value::Choice("number".into()))
        );
        // Saved and loaded, it is still there.
        let loaded = crate::load(&crate::save(&s), &r).unwrap();
        assert_eq!(loaded.vary_at(&glow), s.vary_at(&glow));
        assert!(s.stop_varying(&glow));
        assert_eq!(s.things, before.things, "nothing is left behind");
        assert!(s.tracks.is_empty());
        assert!(!s.stop_varying(&glow));
    }

    #[test]
    fn choices_and_text_never_vary() {
        use crate::{Address, ThingId, testkit};
        let r = testkit::registry();
        let mut s = crate::Scene::empty(&r);
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let key = r.thing("balls").unwrap().settings().iter().find(|d| !can_vary_type(&d.ty)).map(|d| d.key);
        if let Some(key) = key {
            assert!(s.start_varying(&r, &Address::own(b, key)).is_err());
        }
    }

    #[test]
    fn a_layer_s_vary_resolves_and_survives_saving() {
        use crate::{Address, SlotId, ThingId, layered_key, testkit};
        let r = testkit::registry();
        let mut s = crate::Scene::empty(&r);
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.add_slot(&r, b, SlotId::Look, "look.stack").unwrap();
        let ring = s.add_layer(&r, b, SlotId::Look, "layer.ring").unwrap();
        let width = Address::on(b, SlotId::Look, &layered_key(ring, "width"));
        s.start_varying(&r, &width).unwrap();
        let resolved = crate::resolve(&s, &r);
        let look = &resolved.things[&b].slots[&SlotId::Look];
        let plain = look.layer_params(ring);
        let v = vary_of(&plain, "width").expect("the layer's own params carry its vary");
        assert_eq!(v.by, VaryBy::Number);
        assert!(v.to.is_some());
        let loaded = crate::load(&crate::save(&s), &r).unwrap();
        assert_eq!(loaded.vary_at(&width), s.vary_at(&width));
        // A vary on a setting the layer doesn't have is dropped on load.
        let mut odd = s.clone();
        odd.card_mut(b, SlotId::Look)
            .unwrap()
            .params
            .set(&layered_key(ring, "nothing~by"), Value::Choice("number".into()));
        let loaded = crate::load(&crate::save(&odd), &r).unwrap();
        assert!(loaded.card(b, SlotId::Look).unwrap().params.get(&layered_key(ring, "nothing~by")).is_none());
    }
}

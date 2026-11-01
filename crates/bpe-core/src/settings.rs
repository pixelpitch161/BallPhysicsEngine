//! The settings schema: one description of each setting drives the inspector, keyframes, dice,
//! locks and pulses.

use crate::{Color, Gradient, Params, Target, Value, ValueType};

/// Whether a setting shows in a module's first, short view or only under More.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Depth {
    Simple,
    More,
}

/// What else a setting can do besides being set by hand.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flags {
    /// It can change over time on the timeline.
    pub keyframe: bool,
    /// The dice can pick it.
    pub random: bool,
    /// It can pulse to the beat or to hits.
    pub pulse: bool,
    /// It decides what the thing is, such as its shape or how its balls are arranged, so the
    /// dice only change it close to "All new".
    pub structural: bool,
}

/// One setting of a thing or module kind.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingDef {
    /// Its name in saved scenes and addresses.
    pub key: &'static str,
    pub label: &'static str,
    /// One plain sentence for the hover help.
    pub help: &'static str,
    pub ty: ValueType,
    pub default: Value,
    /// Smallest and largest allowed values, for numbers.
    pub range: Option<(f64, f64)>,
    /// The part of the range the dice roll in, when it is narrower than `range` (or when a
    /// setting such as a speed has no range of its own).
    pub dice: Option<(f64, f64)>,
    pub unit: &'static str,
    /// A heading the inspector groups it under.
    pub group: &'static str,
    pub depth: Depth,
    pub flags: Flags,
    /// When the setting does anything, so the inspector can leave it out otherwise: empty for
    /// always, else terms joined by " and " and " or " (and binds tighter). A term is `key`
    /// (that setting is on: true, not zero, not empty), `!key`, `key=a|b` or `key!=a|b`
    /// (a choice's name, or `true`/`false`). The key `thing` is the kind of thing the setting
    /// belongs to, such as `thing=balls|soft`; where no thing is known (the Library) it holds.
    pub when: &'static str,
}

impl SettingDef {
    /// Where the dice roll: the dice range when one is set, else the whole range.
    pub fn dice_range(&self) -> Option<(f64, f64)> {
        self.dice.or(self.range)
    }

    /// Whether the setting does anything for a thing of `kind` (`None`: any kind) whose
    /// other settings read as `get` says. See `when`.
    pub fn applies(&self, kind: Option<&str>, get: &dyn Fn(&str) -> Option<Value>) -> bool {
        applies(self.when, kind, get)
    }

    /// The value pulled into this setting's range. Values other than numbers are left as they are.
    pub fn clamp(&self, v: Value) -> Value {
        match (v, self.range) {
            (Value::Number(x), Some((min, max))) => Value::Number(x.clamp(min, max)),
            (Value::Int(x), Some((min, max))) => Value::Int((x as f64).clamp(min, max) as i64),
            (v, _) => v,
        }
    }
}

/// The inspector's sections, in the order they read down a card.
pub const SECTION_ORDER: &[&str] = &[
    "Basics",
    "Shape",
    "Style",
    "Burst",
    "Each particle",
    "Across the group",
    "Colours",
    "Inside each one",
    "Surface",
    "Light",
    "Motion",
    "Growing",
    "Instrument",
    "Notes",
    "Which hits",
    "Level",
    "Chords",
    "Other balls",
    "Against some things",
    "Variation",
    "Channel",
    "Blending",
];

/// The sections that stay folded until opened.
pub const FOLDED_SECTIONS: &[&str] = &["Variation", "Blending", "Other balls", "Against some things", "Channel"];

/// Where a group sits in [`SECTION_ORDER`]; a group ending in " variation" (a paint's own
/// variation rows) ranks with "Variation". `None` for a group that is not a section.
pub fn section_rank(group: &str) -> Option<usize> {
    let name = if group.ends_with(" variation") { "Variation" } else { group };
    SECTION_ORDER.iter().position(|s| *s == name)
}

/// A Rust type a setting can hold.
pub trait SettingValue: Sized {
    fn value_type() -> ValueType;
    fn to_value(&self) -> Value;
    /// `None` when `v` is the wrong kind of value.
    fn from_value(v: &Value) -> Option<Self>;
}

/// An enum of plain names, shown as a dropdown.
pub trait Choice: Sized + Copy {
    /// Every name, in order.
    const NAMES: &'static [&'static str];
    fn name(self) -> &'static str;
    fn from_name(n: &str) -> Option<Self>;
}

/// A struct whose fields are settings. Usually derived with `#[derive(Settings)]`.
pub trait Settings: Sized + Default {
    /// One description per field, in field order.
    fn defs() -> &'static [SettingDef];
    /// Fills the struct from saved values, using the default for missing or wrong-typed ones.
    fn from_params(p: &Params) -> Self;
    /// Only the fields that differ from the default.
    fn to_params(&self) -> Params;
}

/// How a struct of settings is embedded in another one, as `#[setting(paint, ...)]` says.
#[derive(Clone, Copy, Debug)]
pub struct GroupOpts {
    /// Put before every key of the group, such as `outline_`. May be empty.
    pub prefix: &'static str,
    /// The group's own name, shown on its main setting.
    pub label: &'static str,
    /// The heading the inspector groups the settings under.
    pub group: &'static str,
    /// The embedding field's own condition, which every setting of the group also needs.
    pub when: &'static str,
    /// The names of the choices the group may offer.
    pub follows: &'static [&'static str],
    /// No `space` setting: positions are always read from the screen.
    pub screen_only: bool,
    /// The group's main setting shows in the simple view.
    pub simple: bool,
    /// Whether each instance may vary the group's colours (false with `no_vary`).
    pub vary: bool,
}

/// A struct of settings that another struct embeds as one field, its keys all starting with a
/// prefix. Used through `#[setting(paint, ...)]`.
/// `default` is the field's value in the outer struct's `Default`, so a group's settings
/// default, read and save against what the outer struct starts with.
pub trait SettingGroup: Sized + Default {
    /// The group's settings. Keys and conditions are leaked once per outer type.
    fn group_defs(o: &GroupOpts, default: &Self) -> Vec<SettingDef>;
    /// Fills the group from saved values, using the default for missing or wrong-typed ones.
    fn group_read(o: &GroupOpts, default: &Self, p: &Params) -> Self;
    /// Saves only the fields that differ from the default.
    fn group_write(&self, o: &GroupOpts, default: &Self, p: &mut Params);
}

impl SettingValue for f32 {
    fn value_type() -> ValueType {
        ValueType::Number
    }
    fn to_value(&self) -> Value {
        Value::Number(*self as f64)
    }
    fn from_value(v: &Value) -> Option<Self> {
        if let Value::Number(x) = v { Some(*x as f32) } else { None }
    }
}

impl SettingValue for f64 {
    fn value_type() -> ValueType {
        ValueType::Number
    }
    fn to_value(&self) -> Value {
        Value::Number(*self)
    }
    fn from_value(v: &Value) -> Option<Self> {
        if let Value::Number(x) = v { Some(*x) } else { None }
    }
}

impl SettingValue for i32 {
    fn value_type() -> ValueType {
        ValueType::Int
    }
    fn to_value(&self) -> Value {
        Value::Int(*self as i64)
    }
    fn from_value(v: &Value) -> Option<Self> {
        if let Value::Int(x) = v { i32::try_from(*x).ok() } else { None }
    }
}

impl SettingValue for u32 {
    fn value_type() -> ValueType {
        ValueType::Int
    }
    fn to_value(&self) -> Value {
        Value::Int(*self as i64)
    }
    fn from_value(v: &Value) -> Option<Self> {
        if let Value::Int(x) = v { u32::try_from(*x).ok() } else { None }
    }
}

impl SettingValue for bool {
    fn value_type() -> ValueType {
        ValueType::Bool
    }
    fn to_value(&self) -> Value {
        Value::Bool(*self)
    }
    fn from_value(v: &Value) -> Option<Self> {
        if let Value::Bool(x) = v { Some(*x) } else { None }
    }
}

impl SettingValue for [f32; 2] {
    fn value_type() -> ValueType {
        ValueType::Vec2
    }
    fn to_value(&self) -> Value {
        Value::Vec2(*self)
    }
    fn from_value(v: &Value) -> Option<Self> {
        if let Value::Vec2(x) = v { Some(*x) } else { None }
    }
}

impl SettingValue for glam::Vec2 {
    fn value_type() -> ValueType {
        ValueType::Vec2
    }
    fn to_value(&self) -> Value {
        Value::Vec2(self.to_array())
    }
    fn from_value(v: &Value) -> Option<Self> {
        if let Value::Vec2(x) = v { Some(glam::Vec2::from_array(*x)) } else { None }
    }
}

impl SettingValue for Color {
    fn value_type() -> ValueType {
        ValueType::Color
    }
    fn to_value(&self) -> Value {
        Value::Color(*self)
    }
    fn from_value(v: &Value) -> Option<Self> {
        if let Value::Color(x) = v { Some(*x) } else { None }
    }
}

impl SettingValue for Gradient {
    fn value_type() -> ValueType {
        ValueType::Gradient
    }
    fn to_value(&self) -> Value {
        Value::Gradient(self.clone())
    }
    fn from_value(v: &Value) -> Option<Self> {
        if let Value::Gradient(x) = v { Some(x.clone()) } else { None }
    }
}

impl SettingValue for String {
    fn value_type() -> ValueType {
        ValueType::Text
    }
    fn to_value(&self) -> Value {
        Value::Text(self.clone())
    }
    fn from_value(v: &Value) -> Option<Self> {
        if let Value::Text(x) = v { Some(x.clone()) } else { None }
    }
}

impl SettingValue for Target {
    fn value_type() -> ValueType {
        ValueType::Target
    }
    fn to_value(&self) -> Value {
        Value::Target(self.clone())
    }
    fn from_value(v: &Value) -> Option<Self> {
        if let Value::Target(x) = v { Some(x.clone()) } else { None }
    }
}

impl<T: Choice> SettingValue for T {
    fn value_type() -> ValueType {
        ValueType::Choice(T::NAMES)
    }
    fn to_value(&self) -> Value {
        Value::Choice(self.name().to_owned())
    }
    fn from_value(v: &Value) -> Option<Self> {
        if let Value::Choice(n) = v { T::from_name(n) } else { None }
    }
}

/// Whether a `when` condition (see `SettingDef::when`) holds.
pub fn applies(when: &str, kind: Option<&str>, get: &dyn Fn(&str) -> Option<Value>) -> bool {
    let when = when.trim();
    when.is_empty() || when.split(" or ").any(|all| all.split(" and ").all(|term| term_holds(term.trim(), kind, get)))
}

fn term_holds(term: &str, kind: Option<&str>, get: &dyn Fn(&str) -> Option<Value>) -> bool {
    let (key, names, wanted) = if let Some((key, names)) = term.split_once("!=") {
        (key, Some(names), false)
    } else if let Some((key, names)) = term.split_once('=') {
        (key, Some(names), true)
    } else if let Some(key) = term.strip_prefix('!') {
        (key, None, false)
    } else {
        (term, None, true)
    };
    let key = key.trim();
    let Some(names) = names else {
        return get(key).is_some_and(|v| is_on(&v)) == wanted;
    };
    let names = names.split('|').map(str::trim);
    if key == "thing" {
        return kind.is_none_or(|k| names.clone().any(|n| n == k) == wanted);
    }
    let Some(value) = get(key) else { return true };
    let name = match &value {
        Value::Choice(c) => c.clone(),
        Value::Bool(b) => b.to_string(),
        _ => return true,
    };
    names.into_iter().any(|n| n == name) == wanted
}

/// A setting counts as on when it is true, not zero, not empty, or a gradient with more than one stop.
fn is_on(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Number(x) => *x != 0.0,
        Value::Int(x) => *x != 0,
        Value::Text(t) => !t.trim().is_empty(),
        Value::Gradient(g) => g.stops.len() > 1,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_rank_in_order_and_variations_rank_together() {
        assert_eq!(section_rank("Basics"), Some(0));
        assert_eq!(section_rank("Outline variation"), section_rank("Variation"));
        assert!(section_rank("Variation") < section_rank("Blending"));
        assert_eq!(section_rank("Flash"), None);
        assert!(FOLDED_SECTIONS.iter().all(|f| section_rank(f).is_some()));
    }

    use crate::{Color, Gradient, Value};

    fn get(key: &str) -> Option<Value> {
        match key {
            "shape" => Some(Value::Choice("box".into())),
            "on" => Some(Value::Bool(true)),
            "off" => Some(Value::Bool(false)),
            "zero" => Some(Value::Number(0.0)),
            "none" => Some(Value::Gradient(Gradient::solid(Color::WHITE))),
            "two" => Some(Value::Gradient(Gradient::even(&[Color::WHITE, Color::WHITE]))),
            _ => None,
        }
    }

    #[test]
    fn conditions_read_choices_switches_and_the_kind() {
        let holds = |when: &str, kind: Option<&str>| applies(when, kind, &get);
        assert!(holds("", Some("balls")));
        assert!(holds("shape=box", None));
        assert!(holds("shape=circle|box", None));
        assert!(!holds("shape=circle", None));
        assert!(holds("shape!=circle", None));
        assert!(!holds("shape!=box|circle", None));
        assert!(holds("on", None) && !holds("off", None) && holds("!off", None));
        assert!(!holds("zero", None) && !holds("none", None) && holds("two", None));
        assert!(holds("on=true", None) && holds("off=false", None));
        assert!(holds("thing=balls|soft", Some("soft")) && !holds("thing=balls", Some("arena")));
        assert!(holds("thing=balls", None), "the Library shows every kind's settings");
        assert!(holds("thing!=arena", Some("balls")) && !holds("thing!=arena", Some("arena")));
        assert!(holds("thing=balls or on", Some("arena")));
        assert!(!holds("thing=balls or off", Some("arena")));
        assert!(!holds("thing=balls and off", Some("balls")));
        assert!(holds("thing=arena and on or off", Some("arena")));
    }
}

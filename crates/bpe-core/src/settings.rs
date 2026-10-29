//! The settings schema: one description of each setting drives the inspector, keyframes, dice,
//! locks and pulses.

use crate::{Color, Params, Target, Value, ValueType};

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
}

impl SettingDef {
    /// Where the dice roll: the dice range when one is set, else the whole range.
    pub fn dice_range(&self) -> Option<(f64, f64)> {
        self.dice.or(self.range)
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

impl SettingValue for Vec<Color> {
    fn value_type() -> ValueType {
        ValueType::Colors
    }
    fn to_value(&self) -> Value {
        Value::Colors(self.clone())
    }
    fn from_value(v: &Value) -> Option<Self> {
        if let Value::Colors(x) = v { Some(x.clone()) } else { None }
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

//! Setting values, their types, and sparse maps of them.

use crate::{Color, Target};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One setting's value, as saved in a scene.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Number(f64),
    Int(i64),
    Bool(bool),
    Vec2([f32; 2]),
    Color(Color),
    Colors(Vec<Color>),
    /// One name from a fixed list.
    Choice(String),
    Text(String),
    /// Which other things something reacts to.
    Target(Target),
}

/// What kind of value a setting holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueType {
    Number,
    Int,
    Bool,
    Vec2,
    Color,
    Colors,
    /// One of these names.
    Choice(&'static [&'static str]),
    Text,
    Target,
    /// A chord progression in the library, stored as `Value::Int`: its id, or -1 for Scene
    /// defaults' one.
    Progression,
}

impl ValueType {
    /// Whether `v` is a value of this type. A choice accepts only the names in its list, and
    /// numbers must be finite: NaN and infinity would spread through a simulation.
    pub fn accepts(&self, v: &Value) -> bool {
        match (self, v) {
            (ValueType::Number, Value::Number(x)) => x.is_finite(),
            (ValueType::Vec2, Value::Vec2([x, y])) => x.is_finite() && y.is_finite(),
            (ValueType::Int, Value::Int(_))
            | (ValueType::Bool, Value::Bool(_))
            | (ValueType::Color, Value::Color(_))
            | (ValueType::Colors, Value::Colors(_))
            | (ValueType::Text, Value::Text(_))
            | (ValueType::Target, Value::Target(_)) => true,
            (ValueType::Choice(names), Value::Choice(n)) => names.contains(&n.as_str()),
            (ValueType::Progression, Value::Int(x)) => *x >= -1 && *x <= u32::MAX as i64,
            _ => false,
        }
    }
}

/// Setting values by name. Settings left out use whatever comes from further up.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Params(pub BTreeMap<String, Value>);

impl Params {
    /// The value set for `key`, if any.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    /// Sets `key`, replacing any earlier value.
    pub fn set(&mut self, key: &str, v: Value) {
        self.0.insert(key.to_owned(), v);
    }

    /// Clears `key`, so it falls back to whatever comes from further up.
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        self.0.remove(key)
    }

    /// Whether nothing is set.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Every set key and value, in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.0.iter()
    }

    /// Lays `over`'s values on top of these: keys in `over` win.
    pub fn apply(&mut self, over: &Params) {
        self.0.extend(over.0.iter().map(|(k, v)| (k.clone(), v.clone())));
    }

    /// These values with `over`'s laid on top: keys in `over` win.
    pub fn merged(&self, over: &Params) -> Params {
        let mut out = self.clone();
        out.apply(over);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_round_trip_through_ron() {
        let mut p = Params::default();
        p.set("glow", Value::Number(0.7));
        p.set("color", Value::Color(Color::rgb(242, 92, 92)));
        let text = ron::to_string(&p).unwrap();
        assert_eq!(ron::from_str::<Params>(&text).unwrap(), p);
    }

    #[test]
    fn merged_prefers_the_override() {
        let mut base = Params::default();
        base.set("glow", Value::Number(0.2));
        base.set("size", Value::Number(4.5));
        let mut over = Params::default();
        over.set("glow", Value::Number(0.7));
        let m = base.merged(&over);
        assert_eq!(m.get("glow"), Some(&Value::Number(0.7)));
        assert_eq!(m.get("size"), Some(&Value::Number(4.5)));
    }

    #[test]
    fn numbers_must_be_finite() {
        assert!(ValueType::Number.accepts(&Value::Number(-1e300)));
        assert!(!ValueType::Number.accepts(&Value::Number(f64::NAN)));
        assert!(!ValueType::Number.accepts(&Value::Number(f64::INFINITY)));
        assert!(!ValueType::Vec2.accepts(&Value::Vec2([0.0, f32::NAN])));
    }

    #[test]
    fn choice_type_accepts_only_listed_names() {
        let t = ValueType::Choice(&["circle", "box"]);
        assert!(t.accepts(&Value::Choice("box".into())));
        assert!(!t.accepts(&Value::Choice("star".into())));
        assert!(!t.accepts(&Value::Number(1.0)));
    }
}

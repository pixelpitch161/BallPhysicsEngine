//! Numbers that can come from the firing: a number setting `k` may have `k~from` (the name of
//! a value the firing carries, empty for the number itself), `k~times` and `k~plus` beside it,
//! so "set its speed to the hit's strength × 2" is `from = strength, times = 2`.

use bpe_core::Params;
use bpe_core::Value;
use bpe_core::rules::{from_key, plus_key, times_key};
use bpe_runtime::Firing;

/// A number setting as a block reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct Amount {
    /// The number set.
    pub fixed: f64,
    /// The firing's value to use instead, with its factor and offset.
    pub from: Option<(String, f64, f64)>,
}

fn number(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Number(x)) => Some(*x),
        Some(Value::Int(x)) => Some(*x as f64),
        _ => None,
    }
}

impl Amount {
    /// Reads setting `key` (whose own value is `fixed`, defaults applied) and its companions.
    pub fn read(params: &Params, key: &str, fixed: f64) -> Amount {
        let from = match params.get(&from_key(key)) {
            Some(Value::Text(name)) if !name.is_empty() => Some((
                name.clone(),
                number(params.get(&times_key(key))).unwrap_or(1.0),
                number(params.get(&plus_key(key))).unwrap_or(0.0),
            )),
            _ => None,
        };
        Amount { fixed, from }
    }

    /// Its value for `firing`; the firing's value missing, `None`.
    pub fn of(&self, firing: &Firing) -> Option<f64> {
        match &self.from {
            None => Some(self.fixed),
            Some((name, times, plus)) => firing.values.get(name).map(|v| v * times + plus).filter(|v| v.is_finite()),
        }
    }

    /// Its value for `firing`, the number set when the firing hasn't the value.
    pub fn or_fixed(&self, firing: &Firing) -> f64 {
        self.of(firing).unwrap_or(self.fixed)
    }
}

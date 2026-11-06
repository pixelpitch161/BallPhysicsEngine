//! Setting changes a rule made: each one's key (whole thing or one ball), how it combines with
//! what the keyframes give, when it started, its glide and how long it lasts. A change's value at
//! any moment is worked out from these alone, so drawing between two steps is exact.

use bpe_core::{Ease, Params, SlotId, ThingId, Value};

/// Which setting a change is on: a thing's own (`slot` none) or one on a card, for the whole
/// thing or one ball (its body row).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChangeKey {
    pub thing: ThingId,
    pub slot: Option<SlotId>,
    pub row: Option<u32>,
    pub key: String,
}

/// How a change combines with the value under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum How {
    /// Takes its value (gliding there from where the setting is).
    Set,
    /// Adds its value.
    Add,
    /// Multiplies by its value (a number).
    Multiply,
}

/// One running change.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub how: How,
    pub value: Value,
    /// When it started, in seconds.
    pub start: f64,
    /// Seconds to glide there (and back).
    pub glide: f64,
    pub ease: Ease,
    /// Seconds it lasts before it glides back; `None` for good.
    pub lasts: Option<f64>,
    /// The setting's hard range, which a change never pushes past.
    pub range: Option<(f64, f64)>,
}

impl Change {
    /// How much of the change is in, 0 to 1 (a little beyond for some eases), at `time`.
    pub fn weight(&self, time: f64) -> f32 {
        if time < self.start {
            return 0.0;
        }
        let ramp =
            |since: f64| -> f32 { if self.glide > 0.0 { self.ease.apply((since / self.glide) as f32) } else { 1.0 } };
        let up = ramp(time - self.start);
        match self.lasts {
            Some(n) if time >= self.start + n => {
                let back = time - self.start - n;
                let down = if self.glide > 0.0 { 1.0 - ramp(back) } else { 0.0 };
                up.min(down)
            }
            _ => up,
        }
    }

    /// Whether the change is over at `time`: it lasted and has glided back.
    pub fn done(&self, time: f64) -> bool {
        self.lasts.is_some_and(|n| time >= self.start + n + self.glide.max(0.0))
    }

    /// Whether the change's value moved between `since` and `time` (gliding in or back, the
    /// ends of both glides counted), so whatever was worked out from it must be worked out again.
    /// Asking from the step before makes sure the step after a glide ends sees the end value,
    /// even for a glide shorter than a step.
    pub fn moving(&self, since: f64, time: f64) -> bool {
        let g = self.glide.max(0.0);
        let meets = |from: f64| g > 0.0 && time >= from && since <= from + g;
        meets(self.start) || self.lasts.is_some_and(|n| meets(self.start + n))
    }

    /// Whether the change is fully in at `time` and stays so for good.
    pub fn settled(&self, time: f64) -> bool {
        self.lasts.is_none() && time >= self.start + self.glide.max(0.0)
    }

    /// This change and `next` (both settled, one after the other) as one: two Adds summed, two
    /// Multiplies multiplied. `None` for anything else.
    pub fn merge(&self, next: &Change) -> Option<Change> {
        let value = match (self.how, &self.value, &next.value) {
            (How::Add, Value::Number(a), Value::Number(b)) => Value::Number(a + b),
            (How::Add, Value::Vec2([a, b]), Value::Vec2([c, d])) => Value::Vec2([a + c, b + d]),
            (How::Multiply, Value::Number(a), Value::Number(b)) => Value::Number(a * b),
            _ => return None,
        };
        (self.how == next.how && self.range == next.range).then(|| Change { value, glide: 0.0, ..self.clone() })
    }

    /// Whether this change, once fully in, hides every change before it on the same setting.
    pub fn covers(&self) -> bool {
        self.how == How::Set && self.lasts.is_none()
    }

    /// `base` with this change on top at `time`.
    pub fn on(&self, base: &Value, time: f64) -> Value {
        let w = self.weight(time);
        let out = match (self.how, base, &self.value) {
            (How::Add, Value::Number(a), Value::Number(b)) => Value::Number(a + b * w as f64),
            (How::Add, Value::Int(a), Value::Number(b)) => Value::Int((*a as f64 + b * w as f64).round() as i64),
            (How::Add, Value::Int(a), Value::Int(b)) => Value::Int((*a as f64 + *b as f64 * w as f64).round() as i64),
            (How::Add, Value::Vec2([x, y]), Value::Vec2([a, b])) => Value::Vec2([x + a * w, y + b * w]),
            (How::Multiply, Value::Number(a), Value::Number(m)) => Value::Number(a * (1.0 + (m - 1.0) * w as f64)),
            (How::Multiply, Value::Int(a), Value::Number(m)) => {
                Value::Int((*a as f64 * (1.0 + (m - 1.0) * w as f64)).round() as i64)
            }
            (How::Multiply, Value::Vec2([x, y]), Value::Number(m)) => {
                let f = 1.0 + (*m as f32 - 1.0) * w;
                Value::Vec2([x * f, y * f])
            }
            // A Set, and anything that can't add or multiply: glide to the value.
            (_, base, to) => {
                let to = match (base, to) {
                    (Value::Int(_), Value::Number(x)) => Value::Int(x.round() as i64),
                    (Value::Number(_), Value::Int(x)) => Value::Number(*x as f64),
                    _ => to.clone(),
                };
                if std::mem::discriminant(base) != std::mem::discriminant(&to) {
                    return base.clone();
                }
                if w >= 1.0 { to } else { base.lerp(&to, w) }
            }
        };
        let out = clamp(out, self.range);
        if finite(&out) { out } else { base.clone() }
    }
}

/// Every change on one setting, in the order they were made, on top of `base` at `time`.
pub fn fold(changes: &[Change], base: &Value, time: f64) -> Value {
    changes.iter().fold(base.clone(), |v, c| c.on(&v, time))
}

/// `params` with the changes in `keyed` (setting key and its changes) on top, at `time`. A key
/// that isn't in `params` is left alone.
pub fn apply_to<'a>(params: &mut Params, keyed: impl Iterator<Item = (&'a str, &'a [Change])>, time: f64) {
    for (key, changes) in keyed {
        let Some(base) = params.get(key) else { continue };
        let v = fold(changes, base, time);
        params.set(key, v);
    }
}

fn clamp(v: Value, range: Option<(f64, f64)>) -> Value {
    match (v, range) {
        (Value::Number(x), Some((lo, hi))) => Value::Number(x.clamp(lo, hi)),
        (Value::Int(x), Some((lo, hi))) => Value::Int((x as f64).clamp(lo, hi).round() as i64),
        (v, _) => v,
    }
}

fn finite(v: &Value) -> bool {
    match v {
        Value::Number(x) => x.is_finite(),
        Value::Vec2([x, y]) => x.is_finite() && y.is_finite(),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(how: How, value: f64) -> Change {
        Change {
            how,
            value: Value::Number(value),
            start: 1.0,
            glide: 0.0,
            ease: Ease::Linear,
            lasts: None,
            range: None,
        }
    }

    #[test]
    fn set_add_and_multiply_from_the_value_under_them() {
        let base = Value::Number(10.0);
        assert_eq!(change(How::Set, 3.0).on(&base, 2.0), Value::Number(3.0));
        assert_eq!(change(How::Add, 3.0).on(&base, 2.0), Value::Number(13.0));
        assert_eq!(change(How::Multiply, 3.0).on(&base, 2.0), Value::Number(30.0));
        assert_eq!(change(How::Add, 3.0).on(&base, 0.5), base, "nothing before it starts");
        let ints = change(How::Add, 2.6).on(&Value::Int(4), 2.0);
        assert_eq!(ints, Value::Int(7));
        let pair = Change { value: Value::Vec2([1.0, 2.0]), ..change(How::Add, 0.0) };
        assert_eq!(pair.on(&Value::Vec2([10.0, 10.0]), 2.0), Value::Vec2([11.0, 12.0]));
    }

    #[test]
    fn a_glide_moves_there_and_back() {
        let c = Change { glide: 1.0, lasts: Some(2.0), ..change(How::Add, 10.0) };
        let at = |t: f64| match c.on(&Value::Number(0.0), t) {
            Value::Number(x) => x,
            _ => unreachable!(),
        };
        assert_eq!(at(1.0), 0.0);
        assert!((at(1.5) - 5.0).abs() < 1e-5);
        assert_eq!(at(2.5), 10.0);
        assert_eq!(at(3.0), 10.0, "lasting");
        assert!((at(3.5) - 5.0).abs() < 1e-5, "on its way back");
        assert_eq!(at(4.0), 0.0);
        assert!(!c.done(3.9) && c.done(4.0));
        assert!(c.moving(1.5, 1.5) && !c.moving(2.5, 2.5) && c.moving(3.5, 3.5) && !c.moving(0.5, 0.5));
        assert!(c.moving(2.0, 2.1), "the step after it got there");
        let short = Change { glide: 0.001, lasts: None, ..change(How::Add, 10.0) };
        assert!(short.moving(1.0, 1.0) && short.moving(1.0, 1.004) && !short.moving(1.004, 1.008));
    }

    #[test]
    fn without_a_glide_it_jumps_and_ends() {
        let c = Change { lasts: Some(0.5), ..change(How::Set, 4.0) };
        assert_eq!(c.on(&Value::Number(1.0), 1.2), Value::Number(4.0));
        assert_eq!(c.on(&Value::Number(1.0), 1.5), Value::Number(1.0));
        assert!(c.done(1.5));
        assert!(!c.moving(1.2, 1.2));
    }

    #[test]
    fn changes_stack_in_order_and_a_set_wins() {
        let changes = [change(How::Add, 5.0), change(How::Multiply, 2.0), change(How::Set, 1.0)];
        assert_eq!(fold(&changes[..2], &Value::Number(1.0), 2.0), Value::Number(12.0));
        assert_eq!(fold(&changes, &Value::Number(1.0), 2.0), Value::Number(1.0));
        assert!(changes[2].covers() && !changes[0].covers());
    }

    #[test]
    fn the_hard_range_holds_and_nan_keeps_the_value() {
        let c = Change { range: Some((0.0, 2.0)), ..change(How::Add, 10.0) };
        assert_eq!(c.on(&Value::Number(1.0), 2.0), Value::Number(2.0));
        let nan = change(How::Multiply, f64::INFINITY);
        assert_eq!(nan.on(&Value::Number(0.0), 2.0), Value::Number(0.0));
    }

    #[test]
    fn colours_glide_and_switches_only_set() {
        use bpe_core::Color;
        let red = Value::Color(Color::rgb(255, 0, 0));
        let c = Change { value: Value::Color(Color::rgb(0, 0, 255)), glide: 2.0, ..change(How::Set, 0.0) };
        assert_ne!(c.on(&red, 2.0), red);
        assert_eq!(c.on(&red, 3.0), Value::Color(Color::rgb(0, 0, 255)));
        let on = Change { value: Value::Bool(true), ..change(How::Add, 0.0) };
        assert_eq!(on.on(&Value::Bool(false), 2.0), Value::Bool(true), "an Add on a switch is a Set");
        let wrong = change(How::Set, 1.0);
        assert_eq!(wrong.on(&Value::Bool(false), 2.0), Value::Bool(false), "a value of another type is ignored");
    }

    #[test]
    fn settled_adds_and_multiplies_merge() {
        let a = change(How::Add, 2.0);
        let b = change(How::Add, 3.0);
        assert_eq!(a.merge(&b).unwrap().value, Value::Number(5.0));
        let m = a.merge(&b).unwrap();
        assert_eq!(fold(&[m], &Value::Number(1.0), 9.0), fold(&[a.clone(), b], &Value::Number(1.0), 9.0));
        assert_eq!(change(How::Multiply, 2.0).merge(&change(How::Multiply, 3.0)).unwrap().value, Value::Number(6.0));
        assert!(a.merge(&change(How::Multiply, 3.0)).is_none());
        assert!(change(How::Set, 1.0).merge(&change(How::Set, 1.0)).is_none());
    }
}

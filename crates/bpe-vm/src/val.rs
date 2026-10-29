//! What travels along a wire while a graph runs.

use bpe_core::ThingId;
use bpe_graph::PinType;
use glam::Vec2;

/// One ball: a thing and the row of its ball table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ball {
    pub thing: ThingId,
    pub item: u32,
}

/// What a host knows about a ball.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BallInfo {
    pub position: Vec2,
    pub velocity: Vec2,
    pub radius: f32,
    pub hits: u32,
}

/// A value on a wire. Pins convert loosely, as the graph's wiring rules allow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Val {
    Number(f32),
    Vector(Vec2),
    /// Linear RGBA.
    Color([f32; 4]),
    Bool(bool),
    Ball(Option<Ball>),
}

impl Val {
    /// The value as a number: yes/no as 1 or 0, a vector as its length, anything else 0.
    pub fn number(&self) -> f32 {
        match self {
            Val::Number(x) => *x,
            Val::Vector(v) => v.length(),
            Val::Bool(b) => *b as u8 as f32,
            Val::Color(_) | Val::Ball(_) => 0.0,
        }
    }

    /// The value as a vector; anything that isn't one is zero.
    pub fn vector(&self) -> Vec2 {
        match self {
            Val::Vector(v) => *v,
            _ => Vec2::ZERO,
        }
    }

    /// The value as yes/no: a number is yes when it isn't 0, a ball when there is one.
    pub fn boolean(&self) -> bool {
        match self {
            Val::Bool(b) => *b,
            Val::Number(x) => *x != 0.0,
            Val::Vector(v) => *v != Vec2::ZERO,
            Val::Ball(b) => b.is_some(),
            Val::Color(_) => false,
        }
    }

    /// The value as a colour; anything that isn't one is clear black.
    pub fn color(&self) -> [f32; 4] {
        match self {
            Val::Color(c) => *c,
            _ => [0.0; 4],
        }
    }

    pub fn ball(&self) -> Option<Ball> {
        match self {
            Val::Ball(b) => *b,
            _ => None,
        }
    }

    pub fn pin_type(&self) -> PinType {
        match self {
            Val::Number(_) => PinType::Number,
            Val::Vector(_) => PinType::Vector,
            Val::Color(_) => PinType::Color,
            Val::Bool(_) => PinType::Bool,
            Val::Ball(_) => PinType::Ball,
        }
    }

    /// The nothing value of a pin type: 0, zero, clear, no, no ball.
    pub fn zero(ty: PinType) -> Val {
        match ty {
            PinType::Vector => Val::Vector(Vec2::ZERO),
            PinType::Color => Val::Color([0.0; 4]),
            PinType::Bool => Val::Bool(false),
            PinType::Ball => Val::Ball(None),
            PinType::Number | PinType::Trigger | PinType::Text | PinType::Field => Val::Number(0.0),
        }
    }

    /// The same value with any infinity or NaN replaced by 0, so bad maths never spreads.
    pub fn finite(self) -> Val {
        match self {
            Val::Number(x) if !x.is_finite() => Val::Number(0.0),
            Val::Vector(v) if !v.is_finite() => Val::Vector(Vec2::ZERO),
            Val::Color(c) if c.iter().any(|x| !x.is_finite()) => Val::Color([0.0; 4]),
            v => v,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::ThingId;
    use bpe_graph::PinType;
    use glam::Vec2;

    #[test]
    fn values_convert_loosely_and_know_their_pin_type() {
        assert_eq!(Val::Bool(true).number(), 1.0);
        assert_eq!(Val::Number(2.5).boolean(), true);
        assert_eq!(Val::Vector(Vec2::new(3.0, 4.0)).number(), 5.0);
        assert_eq!(Val::Ball(Some(Ball { thing: ThingId(2), item: 1 })).boolean(), true);
        assert_eq!(Val::Ball(None).number(), 0.0);
        assert_eq!(Val::Number(f32::NAN).finite(), Val::Number(0.0));
        assert_eq!(Val::Vector(Vec2::new(f32::INFINITY, 1.0)).finite(), Val::Vector(Vec2::ZERO));
        assert_eq!(Val::Color([1.0; 4]).pin_type(), PinType::Color);
        assert_eq!(Val::zero(PinType::Ball), Val::Ball(None));
    }
}

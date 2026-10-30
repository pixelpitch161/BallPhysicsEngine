//! How a thing's colours mix with the picture under it: the Blend and Blend amount settings that
//! every draw module carries.

use serde::{Deserialize, Serialize};

use crate::{Choice, Params, Value};

/// How a thing mixes with what is under it. The editor shows each name with its first letter
/// capitalised and underscores as spaces, so `SoftLight` reads "Soft light".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Choice)]
pub enum Blend {
    /// Paints over what is under it.
    #[default]
    Normal,
    /// Adds its light.
    Add,
    Screen,
    Multiply,
    Lighten,
    Darken,
    Subtract,
    Overlay,
    SoftLight,
    Difference,
    Hue,
    Colour,
}

impl Blend {
    /// The blend and its amount (0..1) a module's `params` hold under `blend` and
    /// `blend_amount`: Normal and 1 when missing or of the wrong kind.
    pub fn from_params(params: &Params) -> (Blend, f32) {
        let blend = match params.get("blend") {
            Some(Value::Choice(n)) => Blend::from_name(n).unwrap_or_default(),
            _ => Blend::Normal,
        };
        let amount = match params.get("blend_amount") {
            Some(Value::Number(x)) if x.is_finite() => (*x as f32).clamp(0.0, 1.0),
            Some(Value::Int(x)) => (*x as f32).clamp(0.0, 1.0),
            _ => 1.0,
        };
        (blend, amount)
    }

    /// The blend's number in the composite shader.
    pub fn shader_code(self) -> u32 {
        match self {
            Self::Normal => 0,
            Self::Add => 1,
            Self::Screen => 2,
            Self::Multiply => 3,
            Self::Lighten => 4,
            Self::Darken => 5,
            Self::Subtract => 6,
            Self::Overlay => 7,
            Self::SoftLight => 8,
            Self::Difference => 9,
            Self::Hue => 10,
            Self::Colour => 11,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_params_reads_mode_and_clamps_amount() {
        let mut p = Params::default();
        p.set("blend", Value::Choice("multiply".into()));
        p.set("blend_amount", Value::Number(1.7));
        assert_eq!(Blend::from_params(&p), (Blend::Multiply, 1.0));
        assert_eq!(Blend::from_params(&Params::default()), (Blend::Normal, 1.0));
        p.set("blend_amount", Value::Number(-0.5));
        assert_eq!(Blend::from_params(&p).1, 0.0);
        p.set("blend_amount", Value::Number(0.25));
        assert_eq!(Blend::from_params(&p).1, 0.25);
        p.set("blend", Value::Choice("no_such_blend".into()));
        assert_eq!(Blend::from_params(&p).0, Blend::Normal);
    }

    #[test]
    fn names_are_snake_case_and_codes_follow_the_list() {
        assert_eq!(
            Blend::NAMES,
            [
                "normal",
                "add",
                "screen",
                "multiply",
                "lighten",
                "darken",
                "subtract",
                "overlay",
                "soft_light",
                "difference",
                "hue",
                "colour"
            ]
        );
        for (i, n) in Blend::NAMES.iter().enumerate() {
            assert_eq!(Blend::from_name(n).unwrap().shader_code(), i as u32);
        }
    }
}

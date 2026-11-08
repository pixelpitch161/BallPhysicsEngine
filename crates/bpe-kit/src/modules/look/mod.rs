//! The looks things wear: a ball's stack of layers ([`layers`]), a shape's ([`shape`]), and the
//! old single-card look ([`basic`]) that test scenes still write on balls.

pub mod basic;
pub mod draw;
pub mod layers;
pub mod shape;
pub mod surface;

pub use basic::{BallShape, BasicLook, BasicLookKind, Style};
pub use layers::LayersLookKind;
pub use shape::ShapeLookKind;

#[cfg(test)]
mod tests {
    use crate::registry;
    use bpe_core::Value;

    #[test]
    fn every_look_has_a_blend_that_round_trips() {
        use super::BasicLook;
        use bpe_core::{Blend, Settings};
        assert_eq!(BasicLook::default().blend, Blend::Normal);
        assert_eq!(BasicLook::default().blend_amount, 1.0);
        let reg = registry();
        for look in ["look.basic"] {
            let defs = reg.module(look).unwrap().settings();
            for key in ["blend", "blend_amount"] {
                let def = defs.iter().find(|d| d.key == key).unwrap_or_else(|| panic!("{look} has no {key}"));
                assert_eq!(def.group, "Blending", "{look} {key}");
            }
            let amount = defs.iter().find(|d| d.key == "blend_amount").unwrap();
            assert_eq!(amount.range, Some((0.0, 1.0)));
            assert!(amount.flags.keyframe);
        }
        let look = BasicLook { blend: Blend::Screen, blend_amount: 0.25, ..BasicLook::default() };
        let params = look.to_params();
        assert_eq!(Blend::from_params(&params), (Blend::Screen, 0.25));
        assert_eq!(BasicLook::from_params(&params), look);
    }

    #[test]
    fn two_looks_can_blend_with_each_other_in_the_modes_that_combine() {
        use bpe_core::{Blend, Choice};
        let reg = registry();
        for look in ["look.basic"] {
            let defs = reg.module(look).unwrap().settings();
            let def = defs.iter().find(|d| d.key == "blend_each").unwrap_or_else(|| panic!("{look} has no blend_each"));
            assert_eq!(def.default, Value::Bool(false), "{look}");
            assert_eq!(def.group, "Blending", "{look}");
            assert_eq!(def.label, "Blend with each other", "{look}");
            for name in Blend::NAMES {
                let get = |k: &str| (k == "blend").then(|| Value::Choice(name.to_string()));
                let shown = def.applies(Some("balls"), &get);
                let b = Blend::from_name(name).unwrap();
                assert_eq!(shown, b.combines_with_itself(), "{look}: {name}");
            }
        }
    }
}

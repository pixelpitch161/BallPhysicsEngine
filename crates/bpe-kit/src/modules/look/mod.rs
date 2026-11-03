//! The look a ball (or any other thing but a liquid or a swarm) wears: see [`basic`].

pub mod basic;
pub mod draw;

pub use basic::{BallColours, BallShape, BasicLook, BasicLookKind, Style};

#[cfg(test)]
mod tests {
    use crate::registry;
    use bpe_core::Value;

    #[test]
    fn a_liquid_wearing_a_look_shows_its_colours() {
        let reg = registry();
        let defs = reg.module("look.basic").unwrap().settings();
        let colours = defs.iter().find(|d| d.key == "colours").unwrap();
        let get = |k: &str| defs.iter().find(|d| d.key == k).map(|d| d.default.clone());
        assert!(colours.applies(Some("liquid"), &get), "a liquid paints with its Colours");
        assert!(colours.applies(Some("balls"), &get));
    }

    #[test]
    fn an_arena_card_hides_the_ball_only_look_rows() {
        let reg = registry();
        let defs = reg.module("look.basic").expect("look.basic is registered").settings();
        let get = |k: &str| defs.iter().find(|d| d.key == k).map(|d| d.default.clone());
        let shown = |key: &str| {
            let d = defs.iter().find(|d| d.key == key).unwrap_or_else(|| panic!("no {key}"));
            d.applies(Some("arena"), &get)
        };
        for key in ["style", "ball_shape", "in_pattern", "outline"] {
            assert!(!shown(key), "{key} is for balls");
        }
        for key in ["colours", "glow"] {
            assert!(shown(key), "{key} shows on an arena");
        }
    }

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

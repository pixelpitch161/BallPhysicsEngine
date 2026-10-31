//! The classic look: a lit sphere with a halo, and a flash where it hits.

pub mod draw;
pub mod styled;

use bpe_core::{Blend, ModuleKind, SettingDef, Settings, SlotId};

use crate::paint::Paint;

pub struct ClassicLookKind;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ClassicLook {
    /// The ball's colours and what spreads them across the balls, with an optional pattern
    /// painted inside each one.
    #[setting(
        paint,
        label = "Colours",
        prefix = "",
        group = "Colours",
        follows = "place|start|order|speed|hits|step|beat|time|path",
        inside,
        simple
    )]
    pub fill: Paint,
    /// How brightly the halo shines.
    #[setting(label = "Glow", when = "thing!=liquid", range(0, 2), simple, keyframe, random, pulse)]
    pub glow: f32,
    /// How bright the flash on each hit is.
    #[setting(label = "Flash", when = "thing=balls", range(0, 3), simple, keyframe, random)]
    pub flash: f32,
    /// Every hit turns the colour this far round the colour wheel.
    #[setting(label = "Hue shift per hit", when = "thing=balls", range(-180, 180), unit = "deg", group = "Colours", keyframe, random)]
    pub hue_per_hit: f32,
    /// How big the flash is.
    #[setting(label = "Flash size", when = "thing=balls and flash", range(0, 300), unit = "px", keyframe)]
    pub flash_size: f32,
    /// How long the flash lasts.
    #[setting(label = "Flash time", when = "thing=balls and flash", range(0.02, 2), unit = "s")]
    pub flash_seconds: f32,
    /// How it mixes with what is under it: Normal paints over it, the others mix with it.
    /// Everything with the same Blend and amount is mixed in once, as one picture, so where its
    /// parts overlap they do not stack, unless Blend with each other is on.
    #[setting(label = "Blend", group = "Blending")]
    pub blend: Blend,
    /// How much of the blend shows: 0 is as if it weren't there, 1 is the whole blend.
    #[setting(label = "Blend amount", range(0, 1), group = "Blending", keyframe)]
    pub blend_amount: f32,
    /// Where its own parts overlap, they blend with each other too, as they do with what is
    /// under them; off, they cover each other and are mixed in as one picture.
    #[setting(
        label = "Blend with each other",
        when = "blend=add|screen|multiply|lighten|darken|subtract",
        group = "Blending"
    )]
    pub blend_each: bool,
}

impl Default for ClassicLook {
    fn default() -> Self {
        Self {
            fill: Paint::default(),
            glow: 0.12,
            flash: 0.4,
            hue_per_hit: 0.0,
            flash_size: 26.0,
            flash_seconds: 0.25,
            blend: Blend::Normal,
            blend_amount: 1.0,
            blend_each: false,
        }
    }
}

impl ClassicLook {
    /// The look's first colour, in linear light: what things that paint one colour show.
    pub fn first_colour(&self) -> [f32; 4] {
        self.fill.colours.first().to_linear()
    }
}

impl ModuleKind for ClassicLookKind {
    fn id(&self) -> &'static str {
        "look.classic"
    }
    fn name(&self) -> &'static str {
        "Classic"
    }
    fn help(&self) -> &'static str {
        "A lit ball with a soft halo and a flash where it hits."
    }
    fn slot(&self) -> SlotId {
        SlotId::Look
    }
    fn settings(&self) -> &'static [SettingDef] {
        ClassicLook::defs()
    }
}

#[cfg(test)]
mod tests {
    use crate::registry;
    use bpe_core::{Color, Gradient, Params, Value};

    #[test]
    fn a_liquid_wearing_a_look_shows_its_colours() {
        let reg = registry();
        for look in ["look.classic", "look.styled"] {
            let defs = reg.module(look).unwrap().settings();
            let colours = defs.iter().find(|d| d.key == "colours").unwrap();
            let get = |k: &str| defs.iter().find(|d| d.key == k).map(|d| d.default.clone());
            assert!(colours.applies(Some("liquid"), &get), "{look}: a liquid paints with its Colours");
            assert!(colours.applies(Some("balls"), &get), "{look}");
        }
    }

    #[test]
    fn every_look_has_a_blend_that_round_trips() {
        use super::ClassicLook;
        use super::styled::StyledLook;
        use crate::modules::{look_liquid::LiquidLook, look_swarm::SwarmLook};
        use bpe_core::{Blend, Settings};
        assert_eq!(ClassicLook::default().blend, Blend::Normal);
        assert_eq!(ClassicLook::default().blend_amount, 1.0);
        assert_eq!(StyledLook::default().blend, Blend::Normal);
        assert_eq!(LiquidLook::default().blend, Blend::Normal);
        assert_eq!(SwarmLook::default().blend, Blend::Normal);
        let reg = registry();
        for look in ["look.classic", "look.styled", "look.liquid", "look.swarm"] {
            let defs = reg.module(look).unwrap().settings();
            for key in ["blend", "blend_amount"] {
                let def = defs.iter().find(|d| d.key == key).unwrap_or_else(|| panic!("{look} has no {key}"));
                assert_eq!(def.group, "Blending", "{look} {key}");
            }
            let amount = defs.iter().find(|d| d.key == "blend_amount").unwrap();
            assert_eq!(amount.range, Some((0.0, 1.0)));
            assert!(amount.flags.keyframe);
        }
        let look = ClassicLook { blend: Blend::Screen, blend_amount: 0.25, ..ClassicLook::default() };
        let params = look.to_params();
        assert_eq!(Blend::from_params(&params), (Blend::Screen, 0.25));
        assert_eq!(ClassicLook::from_params(&params), look);
    }

    #[test]
    fn three_looks_can_blend_with_each_other_in_the_modes_that_combine() {
        use bpe_core::{Blend, Choice};
        let reg = registry();
        for look in ["look.classic", "look.styled", "look.swarm"] {
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
        let reg_liquid = reg.module("look.liquid").unwrap().settings();
        assert!(reg_liquid.iter().all(|d| d.key != "blend_each"), "a liquid is one surface already");
    }

    #[test]
    fn no_saved_settings_read_as_the_default_look() {
        use super::ClassicLook;
        use bpe_core::Settings;
        assert_eq!(ClassicLook::from_params(&Params::default()), ClassicLook::default());
        assert!(ClassicLook::default().to_params().0.is_empty(), "the defaults are not saved");
    }

    #[test]
    fn switching_look_kind_keeps_the_shared_colour_and_glow() {
        let reg = registry();
        let colour = Value::Gradient(Gradient::even(&[Color { r: 12, g: 200, b: 90, a: 255 }, Color::WHITE]));
        let mut classic = Params::default();
        classic.set("colours", colour.clone());
        classic.set("follow", Value::Choice("hits".into()));
        classic.set("inside", Value::Choice("rings".into()));
        classic.set("glow", Value::Number(0.7));
        classic.set("flash", Value::Number(0.4));
        let styled = reg.carry_over(&classic, "look.styled");
        assert_eq!(styled.get("colours"), Some(&colour));
        assert_eq!(styled.get("follow"), Some(&Value::Choice("hits".into())));
        assert_eq!(styled.get("inside"), Some(&Value::Choice("rings".into())));
        assert_eq!(styled.get("glow"), Some(&Value::Number(0.7)));
        assert_eq!(styled.get("flash"), Some(&Value::Number(0.4)));

        let mut star = styled.clone();
        star.set("ball_shape", Value::Choice("star".into()));
        let back = reg.carry_over(&star, "look.classic");
        assert_eq!(back.get("colours"), Some(&colour));
        assert_eq!(back.get("follow"), Some(&Value::Choice("hits".into())));
        assert_eq!(back.get("ball_shape"), None, "the classic look has no shapes");

        let mut wrong = Params::default();
        wrong.set("glow", Value::Text("bright".into()));
        assert_eq!(reg.carry_over(&wrong, "look.styled").get("glow"), None, "a value of the wrong type stays behind");
        assert!(reg.carry_over(&classic, "no.such.kind").0.is_empty());
    }
}

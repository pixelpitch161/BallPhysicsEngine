//! The classic look: a lit sphere with a halo, and a flash where it hits.

pub mod draw;
pub mod styled;

use bpe_core::{ModuleKind, SettingDef, Settings, SlotId};

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
        when = "thing!=liquid",
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
}

impl Default for ClassicLook {
    fn default() -> Self {
        Self { fill: Paint::default(), glow: 0.12, flash: 0.4, hue_per_hit: 0.0, flash_size: 26.0, flash_seconds: 0.25 }
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

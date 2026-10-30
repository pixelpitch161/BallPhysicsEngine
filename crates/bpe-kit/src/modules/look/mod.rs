//! The classic look: a lit sphere with a halo, and a flash where it hits.

pub mod draw;
pub mod styled;

use bpe_core::{Choice, Color, Gradient, ModuleKind, SettingDef, Settings, SlotId};

pub struct ClassicLookKind;

/// What spreads the Colours across the balls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum GradientBy {
    /// The row they start in.
    #[default]
    Row,
    /// Their order, first to last.
    Order,
    /// How fast each one is going right now.
    Speed,
    /// How often each one has hit something.
    Hits,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ClassicLook {
    /// The ball's colour.
    #[setting(label = "Colour", simple, keyframe, random)]
    pub color: Color,
    /// How brightly the halo shines.
    #[setting(label = "Glow", when = "thing!=liquid", range(0, 2), simple, keyframe, random, pulse)]
    pub glow: f32,
    /// How bright the flash on each hit is.
    #[setting(label = "Flash", when = "thing=balls", range(0, 3), simple, keyframe, random)]
    pub flash: f32,
    /// Spread across the balls from the first row to the last; a single colour uses Colour.
    #[setting(label = "Colours", when = "thing=balls", keyframe, random)]
    pub gradient: Gradient,
    /// What spreads the Colours across the balls: the row they start in, their order, how fast
    /// they go, or how often they have hit.
    #[setting(label = "Colours follow", when = "thing=balls and gradient", group = "Colours")]
    pub gradient_by: GradientBy,
    /// The speed in px/s, or number of hits, at which the last colour is reached.
    #[setting(
        label = "Colours end at",
        when = "thing=balls and gradient and gradient_by=speed|hits",
        range(1, 20000),
        group = "Colours",
        random(1, 200)
    )]
    pub gradient_full: f32,
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
        Self {
            color: Color::WHITE,
            glow: 0.12,
            flash: 0.4,
            gradient: Gradient::default(),
            gradient_by: GradientBy::Row,
            gradient_full: 1000.0,
            hue_per_hit: 0.0,
            flash_size: 26.0,
            flash_seconds: 0.25,
        }
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
    use bpe_core::{Color, Params, Value};

    #[test]
    fn switching_look_kind_keeps_the_shared_colour_and_glow() {
        let reg = registry();
        let colour = Value::Color(Color { r: 12, g: 200, b: 90, a: 255 });
        let mut classic = Params::default();
        classic.set("color", colour.clone());
        classic.set("glow", Value::Number(0.7));
        classic.set("flash", Value::Number(0.4));
        let styled = reg.carry_over(&classic, "look.styled");
        assert_eq!(styled.get("color"), Some(&colour));
        assert_eq!(styled.get("glow"), Some(&Value::Number(0.7)));
        assert_eq!(styled.get("flash"), Some(&Value::Number(0.4)));

        let mut star = styled.clone();
        star.set("shape", Value::Choice("star".into()));
        let back = reg.carry_over(&star, "look.classic");
        assert_eq!(back.get("color"), Some(&colour));
        assert_eq!(back.get("shape"), None, "the classic look has no shapes");

        let mut wrong = Params::default();
        wrong.set("glow", Value::Text("bright".into()));
        assert_eq!(reg.carry_over(&wrong, "look.styled").get("glow"), None, "a value of the wrong type stays behind");
        assert!(reg.carry_over(&classic, "no.such.kind").0.is_empty());
    }
}

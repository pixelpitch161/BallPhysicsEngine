//! The classic look: a lit sphere with a halo, and a flash where it hits.

use bpe_core::{Color, ModuleKind, SettingDef, Settings, SlotId};

pub struct ClassicLookKind;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ClassicLook {
    /// The ball's colour.
    #[setting(label = "Colour", simple, keyframe, random)]
    pub color: Color,
    /// How brightly the halo shines.
    #[setting(label = "Glow", range(0, 2), simple, keyframe, random, pulse)]
    pub glow: f32,
    /// How bright the flash on each hit is.
    #[setting(label = "Flash", range(0, 3), simple, keyframe, random)]
    pub flash: f32,
    /// Spread across the balls from the first row to the last; empty uses Colour.
    #[setting(label = "Colours", random)]
    pub gradient: Vec<Color>,
    /// How big the flash is.
    #[setting(label = "Flash size", range(0, 300), unit = "px", keyframe)]
    pub flash_size: f32,
    /// How long the flash lasts.
    #[setting(label = "Flash time", range(0.02, 2), unit = "s")]
    pub flash_seconds: f32,
}

impl Default for ClassicLook {
    fn default() -> Self {
        Self { color: Color::WHITE, glow: 0.12, flash: 0.4, gradient: Vec::new(), flash_size: 26.0, flash_seconds: 0.25 }
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

//! Reactions: they change the whole ball when something happens, never its physics.

use bpe_core::Settings;

/// A flash of light where the ball hits something.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct FlashOnHit {
    /// How bright the flash on each hit is.
    #[setting(label = "Brightness", range(0, 3), simple, keyframe)]
    pub flash: f32,
    /// How big the flash is.
    #[setting(label = "Size", range(0, 300), unit = "px", simple, keyframe)]
    pub flash_size: f32,
    /// How long the flash lasts.
    #[setting(label = "Time", range(0.02, 2), unit = "s", simple)]
    pub flash_seconds: f32,
}

impl Default for FlashOnHit {
    fn default() -> Self {
        Self { flash: 0.4, flash_size: 26.0, flash_seconds: 0.25 }
    }
}

/// Every hit turns the body's colour round the colour wheel.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ColourOnHit {
    /// How far round the colour wheel each hit turns the colour.
    #[setting(label = "Turn per hit", range(-180, 180), unit = "deg", simple, keyframe)]
    pub hue_per_hit: f32,
}

impl Default for ColourOnHit {
    fn default() -> Self {
        Self { hue_per_hit: 30.0 }
    }
}

/// Fast balls stretch along their path and thin out across it.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct StretchWhenFast {
    /// How much fast balls stretch.
    #[setting(label = "Amount", range(0, 1), simple, keyframe)]
    pub stretch: f32,
}

impl Default for StretchWhenFast {
    fn default() -> Self {
        Self { stretch: 0.3 }
    }
}

/// The body's colour cycles round the rainbow.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ColourOverTime {
    /// Full turns around the colour wheel per second.
    #[setting(label = "Rainbow", range(-2, 2), unit = "turns/s", simple, keyframe)]
    pub rainbow: f32,
}

impl Default for ColourOverTime {
    fn default() -> Self {
        Self { rainbow: 0.25 }
    }
}

/// The ball's shape and pattern turn.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct Turning {
    /// The shape's starting angle; the pattern turns with it.
    #[setting(label = "Turn", range(-180, 180), unit = "°", simple, keyframe)]
    pub turn: f32,
    /// How fast the shape turns; the pattern turns with it.
    #[setting(label = "Spin", range(-720, 720), unit = "°/s", simple, keyframe)]
    pub spin: f32,
    /// Turns the shape to point the way the ball is going (not a circle, which has no point).
    #[setting(label = "Face motion", simple)]
    pub face_motion: bool,
}

impl Default for Turning {
    fn default() -> Self {
        Self { turn: 0.0, spin: 90.0, face_motion: false }
    }
}

//! Basic motion: gravity, air drag and wind.

pub mod sim;

use bpe_core::{Choice, ModuleKind, Params, SettingDef, Settings, SlotId, Value, VaryUse};
use glam::Vec2;

pub struct BasicMotionKind;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct BasicMotion {
    /// How strongly the scene's gravity pulls these, as a multiple.
    #[setting(label = "Gravity ×", range(-5, 5), group = "Motion", simple, keyframe, pulse)]
    pub gravity_scale: f32,
    /// How quickly the air slows them.
    #[setting(label = "Air drag", range(0, 5), unit = "per s", group = "Motion", simple, keyframe)]
    pub drag: f32,
    /// A steady push on these, in pixels per second squared, like a tilted table. For the
    /// scene's wind, which swirls round balls and shapes, use Follow the air.
    #[setting(label = "Steady push", group = "Motion", unit = "px/s²", keyframe, pulse)]
    pub wind: Vec2,
    /// How quickly these balls take on the air's speed where they are: the Air card's wind with
    /// its gusts, swirled by the balls moving through it and blocked by shapes. 0 leaves them out
    /// of the air.
    #[setting(label = "Follow the air", range(0, 1e9), span(0, 20), unit = "per s", keyframe, pulse, group = "Air")]
    pub follow_air: f32,
    /// Light sensing (needs a Lighting card): whether the lights draw these balls in, like moths,
    /// or push them away, like light pressure. Off leaves the physics as it always was.
    #[setting(label = "Moths", group = "Light sensing")]
    pub moths: Moths,
    /// How hard a light pulls, in pixels per second squared where its light is as bright as an
    /// unlit picture; brighter light pulls harder.
    #[setting(
        label = "Pull",
        when = "moths!=off",
        span(0, 4000),
        unit = "px/s²",
        keyframe,
        pulse,
        group = "Light sensing"
    )]
    pub moth_pull: f32,
    /// Closer to a light than about this, its pull eases off, so moths circle it instead of
    /// sticking to it.
    #[setting(label = "Near", when = "moths!=off", span(0, 300), unit = "px", keyframe, group = "Light sensing")]
    pub moth_near: f32,
    /// Whether a light behind something that blocks it pulls only as much as its light gets
    /// through.
    #[setting(label = "Shadows hide light", when = "moths!=off", group = "Light sensing")]
    pub moth_shadows: bool,
}

/// How the lights move a Motion card's balls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Moths {
    /// The lights don't move them.
    #[default]
    Off,
    /// Drawn towards the lights, the more the brighter.
    DrawnToLight,
    /// Pushed away from the lights, the more the brighter.
    PushedByLight,
}

impl Default for BasicMotion {
    fn default() -> Self {
        Self {
            gravity_scale: 0.0,
            drag: 0.0,
            wind: Vec2::ZERO,
            follow_air: 0.0,
            moths: Moths::Off,
            moth_pull: 1200.0,
            moth_near: 60.0,
            moth_shadows: true,
        }
    }
}

impl BasicMotion {
    /// How the lights pull these balls, when Moths is on.
    pub fn light_pull(&self) -> Option<bpe_runtime::LightPull> {
        let strength = match self.moths {
            Moths::Off => return None,
            Moths::DrawnToLight => self.moth_pull,
            Moths::PushedByLight => -self.moth_pull,
        };
        let finite = |v: f32| if v.is_finite() { v } else { 0.0 };
        Some(bpe_runtime::LightPull {
            strength: finite(strength),
            near: finite(self.moth_near).max(0.0),
            shadows: self.moth_shadows,
        })
    }
}

/// The Motion card's setting that makes its balls follow the scene's air.
pub const FOLLOW_AIR: &str = "follow_air";

impl BasicMotion {
    /// How quickly these balls follow the air, per second; 0 when they don't.
    pub fn air_grip(&self) -> f32 {
        if self.follow_air.is_finite() && self.follow_air > 0.0 { self.follow_air } else { 0.0 }
    }
}

/// Whether a Motion card's settings `params` have Moths on.
pub fn moths_on(params: &Params) -> bool {
    matches!(params.get("moths"), Some(Value::Choice(n)) if Moths::from_name(n).is_some_and(|m| m != Moths::Off))
}

impl ModuleKind for BasicMotionKind {
    fn id(&self) -> &'static str {
        "motion.basic"
    }
    fn name(&self) -> &'static str {
        "Basic"
    }
    fn help(&self) -> &'static str {
        "Falls under gravity, is slowed by the air, and can be pushed steadily or carried by the scene's air."
    }
    fn slot(&self) -> SlotId {
        SlotId::Motion
    }
    /// Every thing with this slot but a Particles thing, which has its own card.
    fn fits(&self, thing_kind: &str) -> bool {
        thing_kind != crate::things::particles::KIND
    }
    fn settings(&self) -> &'static [SettingDef] {
        BasicMotion::defs()
    }
    /// Every motion setting can differ from ball to ball.
    fn vary(&self, key: &str) -> Option<VaryUse> {
        BasicMotion::defs()
            .iter()
            .any(|d| d.key == key && bpe_core::vary::can_vary_type(&d.ty))
            .then_some(VaryUse::Live)
    }
}

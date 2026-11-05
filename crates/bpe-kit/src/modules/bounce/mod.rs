//! Basic bounce: how much speed a hit keeps, and what else a hit does to the ball.

pub mod sim;

use bpe_core::{ModuleKind, SettingDef, Settings, SlotId, Target};

pub struct BasicBounceKind;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct BasicBounce {
    /// How much of its speed into the wall the ball keeps. Above 1 it speeds up.
    #[setting(
        label = "Bounciness",
        group = "Bounce",
        when = "thing!=shape or takes_over",
        range(0, 1.5),
        simple,
        keyframe,
        pulse
    )]
    pub bounciness: f32,
    /// The whole speed is multiplied by this on every hit.
    #[setting(
        label = "Speed gain",
        group = "Bounce",
        when = "thing!=shape or takes_over",
        range(0.5, 1.5),
        simple,
        keyframe
    )]
    pub speed_gain: f32,
    /// The ball can go as fast as the hits make it (the world still keeps it from leaving
    /// the walls behind).
    #[setting(label = "No speed limit", group = "Bounce", when = "thing!=shape or takes_over", simple)]
    pub no_speed_limit: bool,
    /// The ball never goes faster than this.
    #[setting(
        label = "Top speed",
        group = "Bounce",
        when = "thing!=shape and !no_speed_limit or takes_over and !no_speed_limit",
        range(50, 20000),
        unit = "px/s",
        simple,
        keyframe
    )]
    pub top_speed: f32,
    /// How much sideways speed rubs off when the ball touches walls or other balls.
    #[setting(label = "Friction", group = "Bounce", when = "thing!=shape or takes_over", range(0, 1), keyframe)]
    pub friction: f32,
    /// Decides every bounce against this thing, even when the other thing is the one moving.
    #[setting(label = "Takes over", group = "Bounce", when = "thing=shape")]
    pub takes_over: bool,
    /// How much bigger the ball gets on every hit. Negative shrinks it.
    #[setting(label = "Grow per hit", group = "Growing", when = "thing!=shape or takes_over", range(-20, 20), unit = "px", simple, keyframe)]
    pub grow: f32,
    /// The ball can grow as big as its hits make it.
    #[setting(label = "No size limit", group = "Growing", when = "thing!=shape or takes_over")]
    pub no_size_limit: bool,
    /// The ball never grows past this.
    #[setting(
        label = "Biggest size",
        group = "Growing",
        when = "thing!=shape and !no_size_limit or takes_over and !no_size_limit",
        range(0.5, 400),
        unit = "px"
    )]
    pub max_size: f32,
    /// The ball disappears after this many hits. 0 never pops it.
    #[setting(label = "Pops after", group = "Growing", when = "thing!=shape", range(0, 1000), unit = "hits")]
    pub pops_after: u32,
    /// These balls bump into other balls instead of passing through them.
    #[setting(label = "Hits other balls", group = "Other balls", when = "thing!=shape", simple)]
    pub hit_balls: bool,
    /// Only balls on these things are bumped into; both sides have to agree.
    #[setting(label = "Which balls", when = "thing!=shape and hit_balls", group = "Other balls")]
    pub balls_to_hit: Target,
    /// Use the settings below when hitting the things picked here.
    #[setting(label = "Different against some things", when = "thing!=shape", group = "Against some things")]
    pub special: bool,
    /// The things the different bounce applies to.
    #[setting(label = "Those things", when = "thing!=shape and special", group = "Against some things")]
    pub special_target: Target,
    /// How much speed the ball keeps against those things.
    #[setting(
        label = "Bounciness against them",
        when = "thing!=shape and special",
        range(0, 1.5),
        group = "Against some things",
        keyframe
    )]
    pub special_bounciness: f32,
    /// How much the ball grows on each hit against those things.
    #[setting(label = "Grow against them", when = "thing!=shape and special", range(-20, 20), unit = "px", group = "Against some things", keyframe)]
    pub special_grow: f32,
}

impl Default for BasicBounce {
    fn default() -> Self {
        Self {
            bounciness: 1.0,
            speed_gain: 1.0,
            no_speed_limit: true,
            top_speed: 3000.0,
            grow: 0.0,
            hit_balls: false,
            no_size_limit: true,
            max_size: 60.0,
            takes_over: false,
            pops_after: 0,
            balls_to_hit: Target::Anything,
            friction: 0.0,
            special: false,
            special_target: Target::Anything,
            special_bounciness: 1.0,
            special_grow: 0.0,
        }
    }
}

impl ModuleKind for BasicBounceKind {
    fn id(&self) -> &'static str {
        "bounce.basic"
    }
    fn name(&self) -> &'static str {
        "Basic"
    }
    fn help(&self) -> &'static str {
        "How much speed a hit keeps, how the ball grows with each one, and whether it hits other balls."
    }
    fn slot(&self) -> SlotId {
        SlotId::Bounce
    }
    fn settings(&self) -> &'static [SettingDef] {
        BasicBounce::defs()
    }
}

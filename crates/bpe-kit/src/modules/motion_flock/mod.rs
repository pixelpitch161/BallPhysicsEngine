//! Flocking: how a swarm moves. Runs on the graphics card only, so there is no simulation part
//! here; the swarm's drawing part hands these settings to the renderer.

use bpe_core::{ModuleKind, SettingDef, Settings, SlotId};

pub struct FlockingKind;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct Flocking {
    /// The fastest a point may go.
    #[setting(
        label = "Top speed",
        group = "Basics",
        range(20, 2000),
        unit = "px/s",
        simple,
        keyframe,
        random(100, 900)
    )]
    pub top_speed: f32,
    /// How strongly the points ride the swirling air.
    #[setting(label = "Flow", group = "Basics", range(0, 3), simple, keyframe, random, pulse)]
    pub flow: f32,
    /// How much the points stick together.
    #[setting(label = "Flock", group = "Basics", range(0, 2), simple, keyframe, random)]
    pub flock: f32,
    /// How strongly the points are drawn to the balls.
    #[setting(label = "Attraction", group = "Basics", range(0, 3), simple, keyframe, random, pulse)]
    pub attraction: f32,
    /// How hard the points keep apart.
    #[setting(label = "Separation", group = "Basics", range(0, 2), simple, keyframe, random)]
    pub separation: f32,
    /// How strongly the scene's gravity pulls these, as a multiple.
    #[setting(label = "Gravity ×", group = "Basics", range(-5, 5), simple, keyframe, random, pulse)]
    pub gravity_scale: f32,
    /// How quickly the air slows them.
    #[setting(label = "Air drag", group = "Basics", range(0, 5), unit = "per s", keyframe, random(0, 1.5))]
    pub drag: f32,
    /// How big the swirls in the air are.
    #[setting(label = "Swirl size", group = "Basics", range(20, 1000), unit = "px", keyframe, random(100, 600))]
    pub swirl_size: f32,
    /// How quickly the swirls change.
    #[setting(label = "Swirl speed", group = "Basics", range(0, 1), keyframe, random)]
    pub swirl_speed: f32,
    /// How far away a ball still attracts.
    #[setting(label = "Reach", group = "Basics", range(10, 2000), unit = "px", keyframe, random(50, 500))]
    pub reach: f32,
    /// How hard the points steer away from the walls.
    #[setting(label = "Avoid walls", group = "Basics", range(0, 3), keyframe, random)]
    pub avoid_walls: f32,
}

impl Default for Flocking {
    fn default() -> Self {
        Self {
            top_speed: 260.0,
            flow: 1.0,
            flock: 0.4,
            attraction: 0.6,
            separation: 0.3,
            gravity_scale: 0.0,
            drag: 0.6,
            swirl_size: 180.0,
            swirl_speed: 0.05,
            reach: 220.0,
            avoid_walls: 1.0,
        }
    }
}

impl ModuleKind for FlockingKind {
    fn id(&self) -> &'static str {
        "motion.flock"
    }
    fn name(&self) -> &'static str {
        "Flocking"
    }
    fn help(&self) -> &'static str {
        "Swirls with the air, keeps together, follows the balls and avoids the walls."
    }
    fn fits(&self, thing_kind: &str) -> bool {
        thing_kind == "swarm"
    }
    fn slot(&self) -> SlotId {
        SlotId::Motion
    }
    fn settings(&self) -> &'static [SettingDef] {
        Flocking::defs()
    }
}

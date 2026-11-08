//! The modular Ball Studio's engine.
//!
//! A [`World`] is built from a resolved scene by plugins, stepped at a fixed rate, and writes
//! everything that happens to a [`Log`] that drawing reads.

pub mod bodies;
pub mod camera;
pub mod events;
pub mod history;
pub mod letters;
pub mod light;
pub mod pairs;
pub mod parts;
pub mod playback;
pub mod plugins;
pub mod pose;
mod rows;
pub mod rules;
pub mod sound;
#[cfg(test)]
pub mod testing;
pub mod walls;
pub mod world;

pub use bodies::Bodies;
pub use camera::CameraView;
pub use events::{Event, EventKind, Log, Party, hit_strength};
pub use history::{History, HistoryWant};
pub use letters::{LetterWall, LetterWalls};
pub use light::{BallLight, LightAt, LightLevelsAt, LightMaker, LightPull, level_of};
pub use pairs::{Grid, Pairs, Support, pop};
pub use parts::Parts;
pub use playback::Playback;
pub use plugins::{
    Limits, ModuleSim, ModuleState, NewBall, PairRule, SimCx, Sims, ThingSim, ThingState, state, state_mut,
};
pub use pose::Pose;
pub use rules::{
    Action, BallAt, BlockSim, BreakShow, Change, ChangeKey, CountHow, Firing, How, Kick, MotionChange, PrepareCx,
    Remains, RuleText, RulesState, SceneOp, Signal, StageCx, TextOp, Values,
};
pub use sound::{LevelsAt, SoundAt, SoundMaker};
pub use walls::{
    Contact, MAX_SIDES, Outline, Touch, Wall, keep_clear, keep_clear_from, nearest_on_ellipse, on_own_side, reflect,
    wall_velocity,
};
pub use world::{
    BESIDE_SLOTS, Between, Card, Lent, RunId, World, beside_fingerprint, fingerprint, has_beside, heard_part,
    is_beside, main_part, needs_light, physics_animation, step_at,
};

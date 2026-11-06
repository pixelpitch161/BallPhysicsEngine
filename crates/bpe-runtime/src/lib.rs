//! The modular Ball Studio's engine.
//!
//! A [`World`] is built from a resolved scene by plugins, stepped at a fixed rate, and writes
//! everything that happens to a [`Log`] that drawing reads.

pub mod bodies;
pub mod camera;
pub mod events;
pub mod history;
pub mod light;
pub mod pairs;
pub mod playback;
pub mod plugins;
pub mod pose;
mod rows;
pub mod sound;
#[cfg(test)]
pub mod testing;
pub mod walls;
pub mod world;

pub use bodies::Bodies;
pub use camera::CameraView;
pub use events::{Event, EventKind, Log, Party};
pub use history::{History, HistoryWant};
pub use light::{BallLight, LightAt, LightLevelsAt, LightMaker, LightPull, level_of};
pub use pairs::{Grid, Pairs, Support, pop};
pub use playback::Playback;
pub use plugins::{Limits, ModuleSim, ModuleState, PairRule, SimCx, Sims, ThingSim, ThingState, state, state_mut};
pub use pose::Pose;
pub use sound::{LevelsAt, SoundAt, SoundMaker};
pub use walls::{
    Contact, MAX_SIDES, Outline, Touch, Wall, keep_clear, keep_clear_from, nearest_on_ellipse, on_own_side, reflect,
    wall_velocity,
};
pub use world::{Card, World, fingerprint, needs_light, physics_animation, step_at};

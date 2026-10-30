//! The modular Ball Studio's engine.
//!
//! A [`World`] is built from a resolved scene by plugins, stepped at a fixed rate, and writes
//! everything that happens to a [`Log`] that sound, drawing and rules read.

pub mod actions;
pub mod bodies;
pub mod camera;
pub mod events;
pub mod history;
pub mod pairs;
pub mod playback;
pub mod plugins;
pub mod pose;
pub mod solids;
#[cfg(test)]
pub mod testing;
pub mod walls;
pub mod world;

pub use actions::{Action, MAX_BALLS, SPLIT_SPREAD_DEGREES};
pub use bodies::Bodies;
pub use camera::CameraView;
pub use events::{Event, EventKind, Log, MOTION_RATE, Party, SceneSample, TRACK_SECONDS, TrackSample};
pub use history::History;
pub use pairs::{Grid, Pairs, Support};
pub use playback::Playback;
pub use plugins::{
    Limits, ModuleSim, ModuleState, PairRule, RuleCx, SimCx, Sims, ThingSim, ThingState, state, state_mut,
};
pub use pose::Pose;
pub use solids::{Solid, Solids, keep_outside};
pub use walls::{Contact, Outline, Wall, keep_inside, reflect, wall_velocity};
pub use world::{World, fingerprint, physics_animation};

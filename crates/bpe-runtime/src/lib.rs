//! The modular Ball Studio's engine.
//!
//! A [`World`] is built from a resolved scene by plugins, stepped at a fixed rate, and writes
//! everything that happens to a [`Log`] that drawing reads.

pub mod bodies;
pub mod camera;
pub mod events;
pub mod history;
pub mod pairs;
pub mod playback;
pub mod plugins;
pub mod pose;
#[cfg(test)]
pub mod testing;
pub mod walls;
pub mod world;

pub use bodies::Bodies;
pub use camera::CameraView;
pub use events::{Event, EventKind, Log, Party};
pub use history::History;
pub use pairs::{Grid, Pairs, Support};
pub use playback::Playback;
pub use plugins::{Limits, ModuleSim, ModuleState, PairRule, SimCx, Sims, ThingSim, ThingState, state, state_mut};
pub use pose::Pose;
pub use walls::{Contact, MAX_SIDES, Outline, Wall, keep_clear, reflect, wall_velocity};
pub use world::{World, fingerprint, physics_animation};

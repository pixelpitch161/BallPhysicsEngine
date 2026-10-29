//! The modular Ball Studio's engine.
//!
//! A [`World`] is built from a resolved scene by plugins, stepped at a fixed rate, and writes
//! everything that happens to a [`Log`] that sound, drawing and rules read.

pub mod bodies;
pub mod events;
pub mod pairs;
pub mod history;
pub mod playback;
pub mod plugins;
pub mod pose;
pub mod solids;
#[cfg(test)]
pub mod testing;
pub mod walls;
pub mod world;

pub use bodies::Bodies;
pub use events::{Event, EventKind, Log, Party};
pub use pairs::{Grid, Pairs, Support};
pub use history::History;
pub use playback::Playback;
pub use plugins::{Limits, ModuleSim, ModuleState, PairRule, Sims, ThingSim, ThingState, state, state_mut};
pub use pose::Pose;
pub use solids::{Solid, Solids, keep_outside};
pub use walls::{Contact, Outline, Wall, keep_inside, reflect, wall_velocity};
pub use world::{World, fingerprint};

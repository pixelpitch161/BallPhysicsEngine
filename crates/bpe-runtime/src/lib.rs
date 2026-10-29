//! The modular Ball Studio's engine.
//!
//! A [`World`] is built from a resolved scene by plugins, stepped at a fixed rate, and writes
//! everything that happens to a [`Log`] that sound, drawing and rules read.

pub mod events;

pub use events::{Event, EventKind, Log, Party};

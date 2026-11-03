//! The data model of the modular Ball Studio.
//!
//! A scene is a tree of things (balls, folders, arenas and so on). Each thing has its own settings
//! and its own cards of settings (Look, Trail, Bounce...), which nothing else shares. Every setting
//! is described once by a settings schema, which drives the inspector, keyframes and pulses.
//! Nothing here simulates, or draws.

extern crate self as bpe_core;

pub mod address;
pub mod animation;
pub mod blend;
pub mod color;
pub mod contract;
pub mod edit;
pub mod ids;
pub mod registry;
pub mod resolve;
pub mod save;
pub mod scene;
pub mod settings;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
pub mod value;

pub use address::Address;
pub use animation::{Animation, Ease, Inputs, Key, Pulse, PulseSource, SAME_TIME, Track, pulse_strength};
pub use blend::Blend;
pub use color::{Color, Gradient, Stop, Transition};
pub use contract::check_registry;
pub use edit::EditError;
pub use ids::{KindId, SlotId, ThingId};
pub use registry::{ModuleKind, Registry, ThingKind};
pub use resolve::{Problem, ProblemKind, Resolved, ResolvedSlot, ResolvedThing, resolve};
pub use save::{LoadError, load, save};
pub use scene::{Attach, Scene, Slot, SlotStash, Target, Thing, Transform};
pub use settings::{
    Choice, Depth, FOLDED_SECTIONS, Flags, GroupOpts, SECTION_ORDER, SettingDef, SettingGroup, SettingValue, Settings,
    section_rank,
};
pub use value::{Params, Value, ValueType};

/// `#[derive(Settings)]` and `#[derive(Choice)]`.
pub use bpe_settings_derive::{Choice, Settings};

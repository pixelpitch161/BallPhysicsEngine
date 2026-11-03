//! The data model of the modular Ball Studio.
//!
//! A scene is a tree of things (balls, folders, arenas and so on). Each thing fills slots
//! (Look, Trail, Bounce...) by linking modules from the scene's library, and anything it leaves
//! alone comes from its parent, up to the scene settings at the root. Every setting a thing or module
//! has is described once by a settings schema, which drives the inspector, keyframes, dice and
//! pulses. Nothing here simulates, or draws.

extern crate self as bpe_core;

pub mod address;
pub mod animation;
pub mod blend;
pub mod color;
pub mod contract;
pub mod edit;
pub mod ids;
pub mod library;
pub mod registry;
pub mod resolve;
pub mod save;
pub mod scene;
pub mod settings;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
pub mod value;

pub use address::{Address, Owner};
pub use animation::{Animation, Ease, Inputs, Key, Pulse, PulseSource, SAME_TIME, Track, pulse_strength};
pub use blend::Blend;
pub use color::{Color, Gradient, Stop, Transition};
pub use contract::check_registry;
pub use edit::{EditError, starter_name};
pub use ids::{KindId, ModuleId, SlotId, ThingId};
pub use library::{Library, Module, Stash};
pub use registry::{ModuleKind, Registry, ThingKind};
pub use resolve::{
    Problem, ProblemKind, Resolved, ResolvedSlot, ResolvedThing, resolve, resolve_slot,
};
pub use save::{LoadError, load, save};
pub use scene::{Attach, Scene, SlotLink, Target, Thing, Transform};
pub use settings::{
    Choice, Depth, FOLDED_SECTIONS, Flags, GroupOpts, SECTION_ORDER, SettingDef, SettingGroup, SettingValue, Settings,
    section_rank,
};
pub use value::{Params, Value, ValueType};

/// `#[derive(Settings)]` and `#[derive(Choice)]`.
pub use bpe_settings_derive::{Choice, Settings};

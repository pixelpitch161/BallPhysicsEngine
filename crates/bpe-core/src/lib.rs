//! The data model of the modular Ball Studio.
//!
//! A scene is a tree of things (balls, folders, arenas and so on). Each thing fills slots
//! (Look, Sound, Bounce...) by linking modules from the scene's library, and anything it leaves
//! alone comes from its parent, up to Scene defaults at the root. Every setting a thing or module
//! has is described once by a settings schema, which drives the inspector, keyframes, dice and
//! pulses. Nothing here simulates, draws or plays sound.

extern crate self as bpe_core;

pub mod address;
pub mod animation;
pub mod blend;
pub mod color;
pub mod contract;
pub mod edit;
pub mod ids;
pub mod instrument;
pub mod library;
pub mod progression;
pub mod registry;
pub mod resolve;
pub mod save;
pub mod scene;
pub mod settings;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
pub mod value;

pub use address::{Address, Owner};
pub use animation::{
    Animation, Bands, Ease, Inputs, Key, Pulse, PulseInputs, PulseSource, SAME_TIME, Track, pulse_strength,
};
pub use blend::Blend;
pub use color::{Color, Gradient, Stop, Transition};
pub use contract::check_registry;
pub use edit::EditError;
pub use ids::{KindId, ModuleId, SlotId, ThingId};
pub use instrument::{Instrument, InstrumentId, InstrumentRef, InstrumentSource, NoteLength, VoiceSettings};
pub use library::{Library, Module, ModuleGraph};
pub use progression::{
    Chord, Extension, MAX_BEATS, Mode, MusicKey, Progression, ProgressionId, ProgressionRef, Quality,
};
pub use registry::{ModuleKind, Registry, ThingKind};
pub use resolve::{
    Broken, Problem, ProblemKind, Resolved, ResolvedSlot, ResolvedThing, resolve, resolve_slot, resolve_with,
};
pub use save::{LoadError, load, save};
pub use scene::{Attach, Scene, SlotLink, Target, Thing, Transform};
pub use settings::{Choice, Depth, Flags, GroupOpts, SettingDef, SettingGroup, SettingValue, Settings};
pub use value::{Params, Value, ValueType};

/// `#[derive(Settings)]` and `#[derive(Choice)]`.
pub use bpe_settings_derive::{Choice, Settings};

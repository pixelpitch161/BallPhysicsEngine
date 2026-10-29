//! The data model of the modular Ball Studio.
//!
//! A scene is a tree of things (balls, folders, arenas and so on). Each thing fills slots
//! (Look, Sound, Bounce...) by linking modules from the scene's library, and anything it leaves
//! alone comes from its parent, up to Scene defaults at the root. Every setting a thing or module
//! has is described once by a settings schema, which drives the inspector, keyframes, dice and
//! pulses. Nothing here simulates, draws or plays sound.

extern crate self as bpe_core;

pub mod color;
pub mod settings;
pub mod value;

pub use color::Color;
pub use settings::{Choice, Depth, Flags, SettingDef, SettingValue, Settings};
pub use value::{Params, Value, ValueType};

/// `#[derive(Settings)]` and `#[derive(Choice)]`.
pub use bpe_settings_derive::{Choice, Settings};

//! The data model of the modular Ball Studio.
//!
//! A scene is a tree of things (balls, folders, shapes and so on). Each thing has its own settings
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
pub mod gradient_extras;
pub mod gradient_form;
pub mod ids;
pub mod layers;
pub mod light_scenes;
pub mod oklab;
pub mod registry;
pub mod resolve;
pub mod rules;
pub mod save;
pub mod scene;
pub mod settings;
pub mod shape_path;
pub mod sound_values;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
pub mod value;
pub mod vary;

pub use address::Address;
pub use animation::{
    Animation, Ease, HealthLevels, Inputs, Key, LightLevels, LightSource, Pulse, PulseSource, SAME_TIME, SoundLevels,
    SoundSource, Track, pulse_strength,
};
pub use blend::Blend;
pub use color::{Color, GapBlend, Gradient, HitWave, Mix, Morph, Moves, Stop, hue_turned, mix_colours, mix_linear};
pub use contract::check_registry;
pub use edit::EditError;
pub use gradient_form::{Area, GradientForm, Layout, Repeat, RippleFrom};
pub use ids::{KindId, SlotId, ThingId};
pub use layers::{CopiedLayer, CopiedLook, Layer, LayerId, MAX_SUBS, layered_key, split_layered};
pub use light_scenes::{LIGHT_SCENES_KEY, LightingScene, SceneLight};
pub use registry::{LayerKind, LayerSection, ModuleKind, Registry, ThingKind, layer_def};
pub use resolve::{Problem, ProblemKind, Resolved, ResolvedLayer, ResolvedSlot, ResolvedThing, resolve};
pub use rules::{Block, BlockId, BlockSection, Rule, RuleBlockKind, RuleId, Rules, Who};
pub use save::{LoadError, load, save};
pub use scene::{Attach, Scene, Slot, SlotStash, Target, Thing, TrackColour, Transform};
pub use settings::{
    Choice, Depth, FOLDED_SECTIONS, Flags, GroupOpts, SECTION_ORDER, SettingDef, SettingGroup, SettingValue, Settings,
    section_rank,
};
pub use shape_path::ShapePath;
pub use sound_values::{CurvePoint, MAX_CURVE_POINTS, ModRoute, NoteSlot, ROUTES_KEY, SLOTS_KEY, curve_is_valid};
pub use value::{Params, Stroke, Strokes, Value, ValueType};
pub use vary::{BallSpread, Vary, VaryBy, VaryEase, VaryUse};

/// `#[derive(Settings)]` and `#[derive(Choice)]`.
pub use bpe_settings_derive::{Choice, Settings};

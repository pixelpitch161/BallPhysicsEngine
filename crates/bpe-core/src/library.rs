//! The scene's library of modules, which things link to and share.

use crate::{Instrument, InstrumentId, KindId, ModuleId, Params, Progression, ProgressionId, SlotId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// A module's node graph, kept next to the graph data rather than inside it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModuleGraph {
    pub graph: bpe_graph::Graph,
    /// The template it started from.
    pub template: String,
    /// Edited away from the template, so the native settings no longer describe it.
    pub custom: bool,
    /// (node id, setting name) pairs shown at Simple depth.
    pub simple: BTreeSet<(u32, String)>,
}

/// One shared group of settings for a slot, such as a look called "Neon glow".
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Module {
    pub id: ModuleId,
    pub name: String,
    pub slot: SlotId,
    /// The module kind's plugin id.
    pub kind: KindId,
    /// Values that differ from the kind's defaults.
    #[serde(skip_serializing_if = "Params::is_empty")]
    pub params: Params,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph: Option<ModuleGraph>,
    /// This module's own values for its instrument's macros, keyed "<node>.<param>".
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub macros: BTreeMap<String, f32>,
}

/// Every module and chord progression in a scene, by id.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
    pub modules: BTreeMap<ModuleId, Module>,
    /// Chord progressions that sound modules and Scene defaults point at.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub progressions: BTreeMap<ProgressionId, Progression>,
    /// Instruments that sound modules play through.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub instruments: BTreeMap<InstrumentId, Instrument>,
}

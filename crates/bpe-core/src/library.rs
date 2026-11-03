//! The scene's library of modules, which things link to and share.

use crate::{KindId, ModuleId, Params, Pulse, SlotId, ThingId, Track};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// What a module set aside when it changed kind: the settings, keyframes, pulses, locks and
/// per-thing changes its new kind has no use for. Switching back to the old kind puts them back.
/// Module-level entries are keyed by setting key; per-thing ones by (thing, setting key).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stash {
    pub params: Params,
    pub tracks: BTreeMap<String, Track>,
    pub pulses: BTreeMap<String, Pulse>,
    pub locks: BTreeSet<String>,
    /// Each linked thing's own changes to the module, by thing.
    pub overrides: BTreeMap<ThingId, Params>,
    pub thing_tracks: BTreeMap<(ThingId, String), Track>,
    pub thing_pulses: BTreeMap<(ThingId, String), Pulse>,
    pub thing_locks: BTreeSet<(ThingId, String)>,
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
    /// The thing this module was made for as its starter. It is named after the thing, renamed
    /// with it, and removed with it when nothing else uses it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<ThingId>,
    /// What earlier kinds of this module left behind, by kind, to restore if it switches back.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub stash: BTreeMap<KindId, Stash>,
}

/// Every module in a scene, by id.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
    pub modules: BTreeMap<ModuleId, Module>,
}

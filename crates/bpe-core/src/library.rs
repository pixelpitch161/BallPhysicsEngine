//! The scene's library of modules, which things link to and share.

use crate::{Instrument, InstrumentId, KindId, ModuleId, Params, Progression, ProgressionId, SlotId, ThingId};
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
    /// A sound module's channel in the mix: its effects and its sends.
    #[serde(skip_serializing_if = "ChannelStrip::is_default")]
    pub channel: ChannelStrip,
    /// The thing this module was made for as its starter. It is named after the thing, renamed
    /// with it, and removed with it when nothing else uses it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<ThingId>,
}

/// One return bus in a scene's mix. Never reused within a scene.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BusId(pub u32);

/// A return bus: channels send to it, it runs its effects and adds them to the mix.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Bus {
    pub id: BusId,
    pub name: String,
    /// Its level in dB.
    pub volume: f32,
    /// Its effects; no blocks passes the sound through.
    pub fx: bpe_graph::Graph,
}

/// A sound module's channel: its insert effects and how much it sends to each bus.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelStrip {
    /// Its effects; no blocks passes the sound through.
    pub inserts: bpe_graph::Graph,
    /// dB sent to each bus, after the volume. A bus that isn't here gets nothing.
    pub sends: BTreeMap<BusId, f32>,
}

impl ChannelStrip {
    pub fn is_default(&self) -> bool {
        self.inserts.nodes.is_empty() && self.sends.is_empty()
    }
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
    /// The mix's return buses, in the order they're shown.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub buses: Vec<Bus>,
    /// Effects over the whole mix, before the master stage.
    #[serde(skip_serializing_if = "graph_is_empty")]
    pub master_fx: bpe_graph::Graph,
}

fn graph_is_empty(g: &bpe_graph::Graph) -> bool {
    g.nodes.is_empty()
}

impl Library {
    /// Adds a return bus, at the next unused id, and gives that id.
    pub fn add_bus(&mut self, name: &str, fx: bpe_graph::Graph) -> BusId {
        let id = BusId(self.buses.iter().map(|b| b.id.0 + 1).max().unwrap_or(0));
        self.buses.push(Bus { id, name: name.into(), volume: 0.0, fx });
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_round_trips_buses_and_strips() {
        let mut fx = bpe_graph::Graph::new("Effects");
        fx.nodes.push(bpe_graph::Node { id: 0, kind: "reverb".into(), ..Default::default() });
        let mut library = Library::default();
        let bus = library.add_bus("Reverb", fx.clone());
        assert_eq!(bus, BusId(0));
        assert_eq!(library.add_bus("Delay", bpe_graph::Graph::default()), BusId(1));
        let mut module = Module::default();
        module.channel.sends.insert(bus, -6.0);
        module.channel.inserts = fx.clone();
        library.modules.insert(ModuleId(1), module);
        library.instruments.insert(InstrumentId(0), Instrument { fx: fx.clone(), ..Instrument::default() });
        library.master_fx = fx;
        let text = ron::to_string(&library).unwrap();
        let back: Library = ron::from_str(&text).unwrap();
        assert_eq!(back, library);
        assert!(!library.modules[&ModuleId(1)].channel.is_default());
        assert!(ChannelStrip::default().is_default());
    }
}

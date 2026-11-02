//! A scene whose rules end the video, for tests of the end across the crates.

use bpe_core::{ModuleGraph, ModuleKind, Registry, Scene, SlotId, ThingId};
use bpe_graph::{Catalog, Link, Param};
use bpe_vm::{Role, catalog};

use crate::{RulesKind, new_scene};

/// The new scene with a rule on its balls: at `ask_at` seconds, End the video `after` seconds
/// later. So the video ends at `ask_at + after`.
pub fn scene_that_ends(reg: &Registry, ask_at: f32, after: f32) -> Scene {
    let mut scene = new_scene(reg);
    let m = scene.add_module(reg, "rules.graph", "End").expect("rules are registered");
    let mut graph = RulesKind.template().expect("rules have a template");
    let c = catalog(Role::Rules);
    let when = graph.add(c.spec("when.at").expect("When at"), [40.0, 40.0]);
    graph.node_mut(when).expect("block").params.insert("time".into(), Param::Number(ask_at));
    let end = graph.add(c.spec("do.end").expect("End the video"), [260.0, 40.0]);
    graph.node_mut(end).expect("block").params.insert("after".into(), Param::Number(after));
    assert!(graph.connect(Link { from: (when, 0), to: (end, 0) }, c));
    scene.library.modules.get_mut(&m).expect("module").graph =
        Some(ModuleGraph { graph, template: "rules.graph".into(), custom: true, simple: Default::default() });
    scene.link(reg, ThingId(2), SlotId::Rules, m).expect("the balls have a Rules slot");
    scene
}

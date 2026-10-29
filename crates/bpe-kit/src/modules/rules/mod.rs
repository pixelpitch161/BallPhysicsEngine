//! Rules: a graph of When, Only if and Do blocks that reads what happens and acts on it.

pub mod recipes;
pub mod sim;

use bpe_core::{ModuleKind, SettingDef, SlotId};
use bpe_graph::{Graph, Note};

pub use recipes::{Recipe, recipes};
pub use sim::{RulesSim, RulesState};

pub struct RulesKind;

impl ModuleKind for RulesKind {
    fn id(&self) -> &'static str {
        "rules.graph"
    }
    fn name(&self) -> &'static str {
        "Rules"
    }
    fn help(&self) -> &'static str {
        "What happens when these things hit, spawn, time out or count up."
    }
    fn slot(&self) -> SlotId {
        SlotId::Rules
    }
    fn settings(&self) -> &'static [SettingDef] {
        &[]
    }
    fn template(&self) -> Option<Graph> {
        let mut g = Graph::new("Rules");
        g.notes.push(Note {
            pos: [40.0, 40.0],
            text: "Add a rule with the Add rule menu, or drop a When block here.".into(),
            ..Note::default()
        });
        Some(g)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{kit, new_scene, resolve_scene};
    use bpe_core::{ModuleGraph, ModuleKind, SlotId, ThingId};
    use bpe_runtime::{EventKind, ModuleSim, Playback};

    #[test]
    fn a_grow_on_hit_rule_grows_the_ball_in_the_world() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let rules = scene.add_module(&kit.registry, "rules.graph", "Grow").unwrap();
        let mut g = RulesKind.template().unwrap();
        let recipe = recipes().into_iter().find(|r| r.name == "Grow on hit").unwrap();
        let simple = (recipe.add)(&mut g);
        assert_eq!(simple.len(), 1);
        scene.library.modules.get_mut(&rules).unwrap().graph = Some(ModuleGraph {
            graph: g,
            template: "rules.graph".into(),
            custom: true,
            simple: simple.into_iter().collect(),
        });
        scene.add_slot(&kit.registry, ThingId(2), SlotId::Rules).unwrap();
        scene.link(&kit.registry, ThingId(2), SlotId::Rules, rules).unwrap();
        let resolved = resolve_scene(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(10.0);
        let hits = playback.log().events().iter().filter(|e| e.kind == EventKind::Hit).count() as f32;
        assert!(hits >= 3.0);
        let radius = playback.world().bodies[&ThingId(2)].radius[0];
        assert!(radius >= 24.0 + 2.0 * (hits - 1.0), "radius {radius} after {hits} hits");
        // The state keeps the program and a running VM, for the editor's live view.
        let (kind, state) = playback.world().module_state(ThingId(2), SlotId::Rules).unwrap();
        assert_eq!(kind, "rules.graph");
        let st = bpe_runtime::state::<RulesState>(state);
        assert!(st.program.is_some() && st.error.is_none());
        assert!(st.vm.as_ref().and_then(|vm| vm.fired_at(1)).is_some(), "the When block fired");
        // Seeking back and forward again gives the same world.
        let hash = playback.log().hash();
        playback.seek(2.0);
        playback.seek(10.0);
        assert_eq!(playback.log().hash(), hash);
        assert_eq!(playback.world().bodies[&ThingId(2)].radius[0], radius);
    }

    #[test]
    fn every_recipe_compiles_and_runs() {
        let all = recipes();
        assert!(all.len() >= 10);
        for recipe in &all {
            let mut g = RulesKind.template().unwrap();
            let simple = (recipe.add)(&mut g);
            let program =
                crate::graphs::compile_module(SlotId::Rules, &g).unwrap_or_else(|e| panic!("{}: {e}", recipe.name));
            assert!(!program.is_empty(), "{} does nothing", recipe.name);
            assert!(!recipe.help.is_empty());
            for (node, param) in &simple {
                let n = g.node(*node).unwrap_or_else(|| panic!("{}: node {node} missing", recipe.name));
                assert!(n.params.contains_key(param), "{}: {}.{param} is not a setting", recipe.name, n.kind);
            }
            // Adding it twice stacks below, with fresh ids.
            let before = g.nodes.len();
            (recipe.add)(&mut g);
            assert_eq!(g.nodes.len(), before * 2);
            assert!(crate::graphs::compile_module(SlotId::Rules, &g).is_ok());
        }
    }

    #[test]
    fn a_rules_graph_that_does_not_compile_keeps_the_scene_running() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let rules = scene.add_module(&kit.registry, "rules.graph", "Broken").unwrap();
        let mut g = RulesKind.template().unwrap();
        g.nodes.push(bpe_graph::Node { id: 1, kind: "when.nothing".into(), ..Default::default() });
        scene.library.modules.get_mut(&rules).unwrap().graph =
            Some(ModuleGraph { graph: g, template: "rules.graph".into(), custom: true, simple: Default::default() });
        scene.add_slot(&kit.registry, ThingId(2), SlotId::Rules).unwrap();
        scene.link(&kit.registry, ThingId(2), SlotId::Rules, rules).unwrap();
        let resolved = resolve_scene(&scene, &kit.registry);
        assert_eq!(resolved.problems.len(), 1);
        assert!(!resolved.things[&ThingId(2)].slots.contains_key(&SlotId::Rules));
        let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(2.0);
        assert!(!playback.log().is_empty());
        // Prepared straight from the broken graph, the sim keeps the message instead.
        let state = RulesSim.prepare_graph(&bpe_core::Params::default(), scene.library.modules[&rules].graph.as_ref());
        let st = bpe_runtime::state::<RulesState>(state.as_ref());
        assert!(st.program.is_none());
        assert!(st.error.as_deref().is_some_and(|e| e.contains("when.nothing")), "{:?}", st.error);
    }
}

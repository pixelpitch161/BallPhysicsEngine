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

    /// A ball hit by another ball was hit too: the rule fires for both balls of the pair.
    #[test]
    fn a_ball_to_ball_hit_fires_the_rule_for_both_balls() {
        use bpe_core::Value;
        use bpe_runtime::{Log, Party, World};
        use glam::Vec2;
        let (mut scene, _, kit) = crate::testing::scene_with_balls("block", |p| {
            p.set("columns", Value::Int(2));
            p.set("rows", Value::Int(1));
            p.set("spacing", Value::Number(60.0));
            p.set("velocity", Value::Vec2([0.0, 0.0]));
        });
        let bounce = crate::testing::module_of(&scene, &kit.registry, ThingId(2), SlotId::Bounce);
        scene.library.modules.get_mut(&bounce).unwrap().params.set("hit_balls", Value::Bool(true));
        scene.things.get_mut(&ThingId(2)).unwrap().transform.position = Vec2::new(0.0, 300.0);
        let rules = scene.add_module(&kit.registry, "rules.graph", "Grow").unwrap();
        let mut g = RulesKind.template().unwrap();
        let simple = (recipes().into_iter().find(|r| r.name == "Grow on hit").unwrap().add)(&mut g);
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
        let mut w = World::build(&scene, &resolved, &kit.sims);
        {
            let b = w.bodies.get_mut(&ThingId(2)).unwrap();
            b.velocity[0] = Vec2::new(200.0, 0.0);
            b.velocity[1] = Vec2::new(-200.0, 0.0);
        }
        let mut log = Log::default();
        w.advance_to(0.2, &kit.sims, &mut log);
        // What the rule asked for applies at the start of the next step.
        w.step(&kit.sims, &mut log);
        let pair =
            log.events().iter().find(|e| e.b == Some(Party { thing: ThingId(2), item: 1 })).expect("the balls met");
        assert_eq!(pair.a, Party { thing: ThingId(2), item: 0 });
        assert_eq!(log.events().iter().filter(|e| e.kind == EventKind::Hit).count(), 1, "no wall hits yet");
        let radius = &w.bodies[&ThingId(2)].radius;
        assert_eq!((radius[0], radius[1]), (26.0, 26.0), "both balls grew by the recipe's 2 px");
    }

    /// Stage 7 logs a Beat on every beat of the music, so "On the beat" fires.
    #[test]
    fn a_beat_fires_an_on_the_beat_rule() {
        use bpe_core::Value;
        use bpe_graph::{Catalog, Link, Param};
        use bpe_runtime::{Log, World};
        use bpe_vm::{Role, catalog};
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("bpm", Value::Number(120.0));
        let rules = scene.add_module(&kit.registry, "rules.graph", "Heavy beat").unwrap();
        let mut g = RulesKind.template().unwrap();
        let c = catalog(Role::Rules);
        let beat = g.add(c.spec("when.beat").unwrap(), [40.0, 40.0]);
        let heavy = g.add(c.spec("do.gravity").unwrap(), [260.0, 40.0]);
        g.node_mut(heavy).unwrap().params.insert("gravity".into(), Param::Vector([0.0, 2000.0]));
        assert!(g.connect(Link { from: (beat, 0), to: (heavy, 0) }, c));
        scene.library.modules.get_mut(&rules).unwrap().graph =
            Some(ModuleGraph { graph: g, template: "rules.graph".into(), custom: true, simple: Default::default() });
        scene.add_slot(&kit.registry, ThingId(2), SlotId::Rules).unwrap();
        scene.link(&kit.registry, ThingId(2), SlotId::Rules, rules).unwrap();
        let resolved = resolve_scene(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let mut w = World::build(&scene, &resolved, &kit.sims);
        assert_ne!(w.gravity.y, 2000.0);
        let mut log = Log::default();
        // The first step logs the beat at 0 s; the rule's action applies at the next step.
        w.step(&kit.sims, &mut log);
        assert_eq!(log.events().iter().filter(|e| e.kind == EventKind::Beat).count(), 1);
        w.step(&kit.sims, &mut log);
        assert_eq!(w.gravity.y, 2000.0, "the beat set the gravity");
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

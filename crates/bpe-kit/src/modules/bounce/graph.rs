//! The bounce module as a node graph: what the Nodes depth opens, and what runs when it has
//! been edited.

use bpe_core::Settings;
use bpe_graph::{Graph, Param};
use bpe_vm::Role;

use super::BasicBounce;
use crate::graphs::Builder;

/// The native bounce, block by block: reflect keeping `bounciness`, then on a real hit the speed
/// gain and the growth, then the top speed.
pub fn template() -> Graph {
    let defs = BasicBounce::defs();
    let def = |key: &str| defs.iter().find(|d| d.key == key).expect("a bounce setting");
    let mut b = Builder::new("Bounce", Role::Bounce);
    let input = b.add("bounce.in", [40.0, 200.0]);
    let output = b.add("bounce.out", [1340.0, 300.0]);

    // Velocity: bounce off, gain speed on a hit, never faster than the top speed.
    let bounciness = b.setting(def("bounciness"), [40.0, 440.0]);
    let reflect = b.add("reflect", [300.0, 200.0]);
    b.wire((input, 1), (reflect, 0));
    b.wire((input, 2), (reflect, 1));
    b.wire((bounciness, 0), (reflect, 2));
    b.wire((input, 3), (reflect, 3));
    let speed_gain = b.setting(def("speed_gain"), [300.0, 440.0]);
    let gained = b.add("vec.scale", [560.0, 260.0]);
    b.wire((reflect, 0), (gained, 0));
    b.wire((speed_gain, 0), (gained, 1));
    let on_hit = b.add("choose.vector", [820.0, 200.0]);
    b.wire((input, 7), (on_hit, 0));
    b.wire((gained, 0), (on_hit, 1));
    b.wire((reflect, 0), (on_hit, 2));
    let top_speed = b.setting(def("top_speed"), [820.0, 440.0]);
    let limited = b.add("vec.limit", [1080.0, 200.0]);
    b.wire((on_hit, 0), (limited, 0));
    b.wire((top_speed, 0), (limited, 1));
    b.wire((limited, 0), (output, 0));

    // Size: grow on a hit, between half a pixel and the biggest size.
    let grow = b.setting(def("grow"), [300.0, 640.0]);
    let grown = b.add("math", [560.0, 600.0]);
    b.set(grown, "op", Param::Choice("add".into()));
    b.wire((input, 5), (grown, 0));
    b.wire((grow, 0), (grown, 1));
    let max_size = b.setting(def("max_size"), [560.0, 800.0]);
    let kept = b.add("clamp", [820.0, 600.0]);
    b.set(kept, "min", Param::Number(0.5));
    b.wire((grown, 0), (kept, 0));
    b.wire((max_size, 0), (kept, 2));
    let size_on_hit = b.add("choose", [1080.0, 600.0]);
    b.wire((input, 7), (size_on_hit, 0));
    b.wire((kept, 0), (size_on_hit, 1));
    b.wire((input, 5), (size_on_hit, 2));
    b.wire((size_on_hit, 0), (output, 1));

    let takes_over = b.setting(def("takes_over"), [1080.0, 800.0]);
    b.wire((takes_over, 0), (output, 2));
    b.finish()
}

#[cfg(test)]
mod tests {
    use super::template;
    use crate::graphs::resolve_scene;
    use crate::modules::bounce::BasicBounce;
    use crate::modules::bounce::sim::{BasicBounceSim, BounceState};
    use crate::testing::{BALLS, log_hash_and_samples, module_of, one_ball_with_bounce, with_custom_template};
    use bpe_core::{ModuleGraph, Params, Settings, SlotId, Value};
    use bpe_graph::{Catalog, Link, Param};
    use bpe_runtime::{ModuleSim, Playback, state};
    use bpe_vm::{Role, catalog};

    #[test]
    fn a_custom_bounce_graph_runs_instead_of_the_native_one() {
        let (mut scene, _, kit) = one_ball_with_bounce(|_| {});
        let native = log_hash_and_samples(&scene, &kit);
        let module = module_of(&scene, &kit.registry, BALLS, SlotId::Bounce);
        with_custom_template(&mut scene, module, &kit.registry);
        // A dead bounce: the "keep" of the reflect block comes from a fixed 0 instead of the
        // bounciness setting.
        let g = &mut scene.library.modules.get_mut(&module).unwrap().graph.as_mut().unwrap().graph;
        let reflect = g.nodes.iter().find(|n| n.kind == "reflect").unwrap().id;
        g.links.retain(|l| l.to != (reflect, 2));
        let c = catalog(Role::Bounce);
        let zero = g.add(c.spec("number").unwrap(), [0.0, 0.0]);
        g.node_mut(zero).unwrap().params.insert("value".into(), Param::Number(0.0));
        assert!(g.connect(Link { from: (zero, 0), to: (reflect, 2) }, c));

        let resolved = resolve_scene(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(20.0);
        assert_ne!(playback.log().hash(), native.0, "the custom graph should change the bounces");
        let body = &playback.world().bodies[&BALLS];
        assert!((body.position[0].length() - 482.5).abs() < 1.0, "{:?}", body.position[0]);
        assert!(body.velocity[0].length() < 1200.0, "{:?}", body.velocity[0]);
    }

    #[test]
    fn a_custom_graph_that_fails_to_compile_keeps_the_native_bounce() {
        let mut g = template();
        // A second output block is one too many.
        g.add(catalog(Role::Bounce).spec("bounce.out").unwrap(), [0.0, 0.0]);
        let broken =
            ModuleGraph { graph: g, template: "bounce.basic".into(), custom: true, simple: Default::default() };
        let mut params = Params::default();
        for d in BasicBounce::defs() {
            params.set(d.key, d.default.clone());
        }
        params.set("bounciness", Value::Number(0.25));
        let st = BasicBounceSim.prepare_graph(&params, Some(&broken));
        let st = state::<BounceState>(st.as_ref());
        assert!(st.program.is_none());
        assert_eq!(st.settings, BasicBounce::from_params(&params));
        assert_eq!(st.settings.bounciness, 0.25);
    }
}

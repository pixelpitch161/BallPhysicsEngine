//! The motion module as a node graph.

use bpe_core::Settings;
use bpe_graph::{Graph, Param};
use bpe_vm::Role;

use super::BasicMotion;
use crate::graphs::Builder;

/// The native motion, block by block: gravity scaled and applied over dt, then the air drag.
pub fn template() -> Graph {
    let defs = BasicMotion::defs();
    let def = |key: &str| defs.iter().find(|d| d.key == key).expect("a motion setting");
    let mut b = Builder::new("Motion", Role::Motion);
    let input = b.add("motion.in", [40.0, 200.0]);
    let output = b.add("motion.out", [1600.0, 200.0]);

    let gravity_scale = b.setting(def("gravity_scale"), [40.0, 440.0]);
    let scaled = b.add("vec.scale", [300.0, 200.0]);
    b.wire((input, 3), (scaled, 0));
    b.wire((gravity_scale, 0), (scaled, 1));
    let pull = b.add("vec.scale", [560.0, 200.0]);
    b.wire((scaled, 0), (pull, 0));
    b.wire((input, 4), (pull, 1));
    let pulled = b.add("vec.math", [820.0, 200.0]);
    b.set(pulled, "op", Param::Choice("add".into()));
    b.wire((input, 2), (pulled, 0));
    b.wire((pull, 0), (pulled, 1));

    let drag = b.setting(def("drag"), [300.0, 520.0]);
    let drag_dt = b.add("math", [560.0, 520.0]);
    b.set(drag_dt, "op", Param::Choice("multiply".into()));
    b.wire((drag, 0), (drag_dt, 0));
    b.wire((input, 4), (drag_dt, 1));
    let left = b.add("math", [820.0, 520.0]);
    b.set(left, "op", Param::Choice("subtract".into()));
    b.set(left, "a", Param::Number(1.0));
    b.wire((drag_dt, 0), (left, 1));
    let keep = b.add("math", [1080.0, 520.0]);
    b.set(keep, "op", Param::Choice("max".into()));
    b.set(keep, "b", Param::Number(0.0));
    b.wire((left, 0), (keep, 0));

    let slowed = b.add("vec.scale", [1340.0, 200.0]);
    b.wire((pulled, 0), (slowed, 0));
    b.wire((keep, 0), (slowed, 1));
    b.wire((slowed, 0), (output, 0));
    b.finish()
}

#[cfg(test)]
mod tests {
    use crate::graphs::resolve_scene;
    use crate::testing::{BALLS, module_of, one_ball_with_bounce, with_custom_template};
    use bpe_core::SlotId;
    use bpe_graph::{Catalog, Link, Param};
    use bpe_runtime::Playback;
    use bpe_vm::{Role, catalog};

    #[test]
    fn a_custom_motion_graph_can_add_wind() {
        let (mut scene, resolved, kit) = one_ball_with_bounce(|_| {});
        let mut native = Playback::new(&scene, &resolved, kit.sims.clone());
        native.seek(0.5);
        let native_x = native.world().bodies[&BALLS].position[0].x;

        let module = module_of(&scene, &kit.registry, BALLS, SlotId::Motion);
        with_custom_template(&mut scene, module, &kit.registry);
        let g = &mut scene.library.modules.get_mut(&module).unwrap().graph.as_mut().unwrap().graph;
        let c = catalog(Role::Motion);
        let input = g.nodes.iter().find(|n| n.kind == "motion.in").unwrap().id;
        let output = g.nodes.iter().find(|n| n.kind == "motion.out").unwrap().id;
        let (from, _) = g.links.iter().find(|l| l.to == (output, 0)).map(|l| (l.from, l.to)).unwrap();
        g.links.retain(|l| l.to != (output, 0));
        // Wind: 400 px/s² to the right, applied over dt.
        let wind = g.add(c.spec("vec.make").unwrap(), [0.0, 0.0]);
        g.node_mut(wind).unwrap().params.insert("x".into(), Param::Number(400.0));
        let over_dt = g.add(c.spec("vec.scale").unwrap(), [0.0, 0.0]);
        let add = g.add(c.spec("vec.math").unwrap(), [0.0, 0.0]);
        assert!(g.connect(Link { from: (wind, 0), to: (over_dt, 0) }, c));
        assert!(g.connect(Link { from: (input, 4), to: (over_dt, 1) }, c));
        assert!(g.connect(Link { from, to: (add, 0) }, c));
        assert!(g.connect(Link { from: (over_dt, 0), to: (add, 1) }, c));
        assert!(g.connect(Link { from: (add, 0), to: (output, 0) }, c));

        let resolved = resolve_scene(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let mut windy = Playback::new(&scene, &resolved, kit.sims.clone());
        windy.seek(0.5);
        let x = windy.world().bodies[&BALLS].position[0].x;
        assert!(x - native_x > 20.0, "native {native_x}, with wind {x}");
    }
}

//! The notes module as a node graph.

use bpe_core::Settings;
use bpe_graph::{Graph, Param};
use bpe_vm::Role;

use super::NoteSound;
use crate::graphs::Builder;

/// The native notes, block by block: a hit at least as fast as the quietest hit plays a note of
/// the chord, as hard as its speed between the quietest and loudest hit, panned to where it was.
/// The note range settings feed nothing here: the chord block reads them from the module.
pub fn template() -> Graph {
    let defs = NoteSound::defs();
    let def = |key: &str| defs.iter().find(|d| d.key == key).expect("a notes setting");
    let mut b = Builder::new("Notes", Role::Sound);
    let input = b.add("sound.in", [40.0, 200.0]);
    let output = b.add("sound.out", [1340.0, 300.0]);

    // Play: fast enough, and the chord has a note.
    let quietest = b.setting(def("quietest_hit"), [40.0, 440.0]);
    let fast_enough = b.add("compare", [300.0, 200.0]);
    b.set(fast_enough, "op", Param::Choice("at least".into()));
    b.wire((input, 1), (fast_enough, 0));
    b.wire((quietest, 0), (fast_enough, 1));
    let chord = b.add("sound.chord", [300.0, 360.0]);
    b.wire((input, 2), (chord, 0));
    let play = b.add("logic", [560.0, 200.0]);
    b.set(play, "op", Param::Choice("and".into()));
    b.wire((fast_enough, 0), (play, 0));
    b.wire((chord, 1), (play, 1));
    b.wire((play, 0), (output, 0));
    b.wire((chord, 0), (output, 1));

    // How hard: where the speed sits between the quietest and the loudest hit.
    let loudest = b.setting(def("loudest_hit"), [40.0, 600.0]);
    let over = b.add("math", [300.0, 520.0]);
    b.set(over, "op", Param::Choice("subtract".into()));
    b.wire((input, 1), (over, 0));
    b.wire((quietest, 0), (over, 1));
    let span = b.add("math", [300.0, 680.0]);
    b.set(span, "op", Param::Choice("subtract".into()));
    b.wire((loudest, 0), (span, 0));
    b.wire((quietest, 0), (span, 1));
    let span_floor = b.add("math", [560.0, 680.0]);
    b.set(span_floor, "op", Param::Choice("max".into()));
    b.set(span_floor, "b", Param::Number(1.0));
    b.wire((span, 0), (span_floor, 0));
    let part = b.add("math", [820.0, 520.0]);
    b.set(part, "op", Param::Choice("divide".into()));
    b.wire((over, 0), (part, 0));
    b.wire((span_floor, 0), (part, 1));
    let velocity = b.add("clamp", [1080.0, 520.0]);
    b.set(velocity, "min", Param::Number(0.0));
    b.set(velocity, "max", Param::Number(1.0));
    b.wire((part, 0), (velocity, 0));
    b.wire((velocity, 0), (output, 2));

    let volume = b.setting(def("volume"), [1080.0, 200.0]);
    b.wire((volume, 0), (output, 3));

    // Where: left to right with the hit, as wide as the stereo width.
    let side = b.add("clamp", [560.0, 860.0]);
    b.set(side, "min", Param::Number(-1.0));
    b.set(side, "max", Param::Number(1.0));
    b.wire((input, 4), (side, 0));
    let stereo_width = b.setting(def("stereo_width"), [560.0, 1020.0]);
    let pan = b.add("math", [820.0, 860.0]);
    b.set(pan, "op", Param::Choice("multiply".into()));
    b.wire((side, 0), (pan, 0));
    b.wire((stereo_width, 0), (pan, 1));
    b.wire((pan, 0), (output, 4));

    let decay = b.setting(def("decay"), [1080.0, 860.0]);
    b.wire((decay, 0), (output, 5));
    let brightness = b.setting(def("brightness"), [1080.0, 1020.0]);
    b.wire((brightness, 0), (output, 6));

    b.setting(def("low_note"), [40.0, 860.0]);
    b.setting(def("high_note"), [40.0, 1020.0]);
    b.finish()
}

#[cfg(test)]
mod tests {
    use crate::graphs::resolve_scene;
    use crate::mix_settings;
    use crate::modules::sound::mix::NoteSoundMix;
    use crate::testing::{BALLS, module_of, one_ball_with_bounce, with_custom_template};
    use bpe_core::{SlotId, ThingId};
    use bpe_graph::{Catalog, Link, Param};
    use bpe_mix::{MixCx, ModuleMix};
    use bpe_runtime::{Event, EventKind, Party};
    use bpe_vm::{Role, catalog};
    use glam::Vec2;

    fn hit(step: u64, speed: f32) -> Event {
        Event {
            step,
            kind: EventKind::Hit,
            a: Party { thing: BALLS, item: 0 },
            b: Some(Party { thing: ThingId(1), item: 0 }),
            point: Vec2::new(100.0, 0.0),
            normal: Vec2::X,
            speed,
        }
    }

    #[test]
    fn a_custom_sound_graph_can_pick_its_own_key() {
        let (mut scene, _, kit) = one_ball_with_bounce(|_| {});
        let module = module_of(&scene, &kit.registry, BALLS, SlotId::Sound);
        with_custom_template(&mut scene, module, &kit.registry);
        let g = &mut scene.library.modules.get_mut(&module).unwrap().graph.as_mut().unwrap().graph;
        let c = catalog(Role::Sound);
        let output = g.nodes.iter().find(|n| n.kind == "sound.out").unwrap().id;
        g.links.retain(|l| l.to != (output, 1));
        let sixty = g.add(c.spec("number").unwrap(), [0.0, 0.0]);
        g.node_mut(sixty).unwrap().params.insert("value".into(), Param::Number(60.0));
        assert!(g.connect(Link { from: (sixty, 0), to: (output, 1) }, c));

        let resolved = resolve_scene(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let slot = &resolved.things[&BALLS].slots[&SlotId::Sound];
        let graph = scene.library.modules[&slot.module].graph.as_ref();
        let settings = mix_settings(&scene, &resolved);
        let index = |e: &Event| e.step;
        let any = |_: &bpe_core::Target, _: ThingId, _: ThingId| true;
        let cx = MixCx { settings: &settings, frames: usize::MAX, hit_index: &index, matches: &any };
        let events = [hit(240, 500.0), hit(480, 300.0), hit(720, 5.0)];
        let mut notes = Vec::new();
        NoteSoundMix.notes_with(&slot.params, graph, BALLS, &events, &cx, &mut notes);
        assert_eq!(notes.len(), 2, "the hit under the quietest hit stays silent");
        assert!(notes.iter().all(|n| n.key == 60), "{notes:?}");
        assert!(notes[0].velocity > notes[1].velocity);
        assert!(notes[0].left > 0.0 && notes[0].right > notes[0].left, "a hit on the right sounds on the right");
    }
}

//! Rules part 1: the scenes where rules move the physics keep their own event logs (the other
//! goldens have no rules). Bless with `BLESS_GOLDEN=1`, once with `--release` and once without.

use bpe_kit::{kit, scenes};
use bpe_runtime::{EventKind, Playback};

const SECONDS: f64 = 20.0;

#[test]
fn rule_scenes_keep_their_event_logs() {
    let kit = kit();
    let mut lines = String::from("# scene\tevents\trule events\thash\n");
    for (name, scene) in scenes::rule_scenes(&kit.registry) {
        let resolved = bpe_kit::resolve_scene(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{name}: {:?}", resolved.problems);
        let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(SECONDS);
        let world = playback.world();
        let rules = world.rules().expect("rules");
        for r in &scene.rules.list {
            assert_eq!(rules.problem(r.id), None, "{name}: {}", r.name);
        }
        let log = playback.log();
        assert!(log.events().windows(2).all(|w| w[0].step <= w[1].step), "{name}: events out of order");
        let count = |k: EventKind| log.events().iter().filter(|e| e.kind == k).count();
        let fired = count(EventKind::Rule);
        assert!(count(EventKind::Hit) > 10, "{name}: too few hits");
        // Every rule's Do block fired.
        for r in &scene.rules.list {
            let last = r.blocks.last().expect("blocks").id;
            let item = bpe_runtime::rules::rule_item(r.id, last);
            assert!(
                log.events().iter().any(|e| e.kind == EventKind::Rule && e.b.is_some_and(|b| b.item == item)),
                "{name}: {} never did anything",
                r.name
            );
        }
        for bodies in world.bodies.values() {
            assert!(bodies.position.iter().all(|p| p.is_finite()), "{name}: a ball went non-finite");
        }
        lines += &format!("{name}\t{}\t{fired}\t{:016x}\n", log.len(), log.hash());
        let mut jumpy = Playback::new(&scene, &resolved, kit.sims.clone());
        for t in [7.0, 3.0, 11.5, SECONDS] {
            jumpy.seek(t);
        }
        assert_eq!(jumpy.log().hash(), log.hash(), "{name}: seeking changed the run");
    }
    let file = if cfg!(debug_assertions) { "tests/golden-rules-debug.txt" } else { "tests/golden-rules.txt" };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file);
    if std::env::var_os("BLESS_GOLDEN").is_some() {
        std::fs::write(&path, &lines).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).expect("the golden file is missing; run with BLESS_GOLDEN=1");
    assert_eq!(expected, lines, "rule event logs changed");
}

#[test]
fn breakable_bricks_break() {
    let kit = kit();
    let scene = scenes::breakable_bricks(&kit.registry);
    let resolved = bpe_kit::resolve_scene(&scene, &kit.registry);
    let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
    playback.seek(SECONDS);
    let broken = playback.world().rules().unwrap().broken().count();
    assert!(broken >= 3, "only {broken} bricks broke");
}

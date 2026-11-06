//! Rules part 4: the scenes where health moves the physics keep their own event logs (the other
//! goldens have no Health card). Bless with `BLESS_GOLDEN=1`, once with `--release` and once
//! without.

use bpe_kit::{kit, scenes};
use bpe_runtime::{EventKind, Playback};

const SECONDS: f64 = 20.0;

#[test]
fn health_scenes_keep_their_event_logs() {
    let kit = kit();
    let mut lines = String::from("# scene\tevents\tbreaks\trule events\thash\n");
    for (name, scene) in scenes::health_scenes(&kit.registry) {
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
        let count = |k: EventKind| log.events().iter().filter(|e| e.kind == k).count();
        assert!(count(EventKind::Hit) > 10, "{name}: too few hits");
        let breaks = count(EventKind::Break);
        assert!(breaks > 0, "{name}: nothing broke");
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
        lines += &format!("{name}\t{}\t{breaks}\t{}\t{:016x}\n", log.len(), count(EventKind::Rule), log.hash());
        let mut jumpy = Playback::new(&scene, &resolved, kit.sims.clone());
        for t in [7.0, 3.0, 11.5, SECONDS] {
            jumpy.seek(t);
        }
        assert_eq!(jumpy.log().hash(), log.hash(), "{name}: seeking changed the run");
    }
    let file =
        if cfg!(debug_assertions) { "tests/golden-rules-health-debug.txt" } else { "tests/golden-rules-health.txt" };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file);
    if std::env::var_os("BLESS_GOLDEN").is_some() {
        std::fs::write(&path, &lines).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).expect("the golden file is missing; run with BLESS_GOLDEN=1");
    assert_eq!(expected, lines, "health event logs changed");
}

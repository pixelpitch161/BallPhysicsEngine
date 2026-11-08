//! Golden Text letters (Text part 3): balls bouncing off, inside and round a Text's letters, the
//! letters' health and breaks, must keep exactly the same event logs. Once a second the log's
//! hash is written down, with which letters still stand; a seek back and forth must give the same.
//!
//! When a change is meant to move them, run with `BLESS_GOLDEN=1` (once with `--release` and
//! once without) and say why in the pull request.

use bpe_kit::{kit, scenes};
use bpe_runtime::{EventKind, Playback};

const SECONDS: u32 = 8;

#[test]
fn every_text_scene_keeps_its_event_log() {
    let kit = kit();
    let mut lines = String::from("# scene\tsecond\tevents\tletter hits\tstanding\thash\n");
    for (name, scene) in scenes::text_scenes(&kit.registry) {
        let resolved = bpe_kit::resolve_scene(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{name}: {:?}", resolved.problems);
        let text = scene.order.iter().copied().find(|id| scene.things[id].kind == "text").expect("a Text");
        let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
        let mut last = 0;
        for second in 1..=SECONDS {
            playback.seek(second as f64);
            let log = playback.log();
            let on_letters = log
                .events()
                .iter()
                .filter(|e| e.kind == EventKind::Hit && e.b.is_some_and(|b| b.thing == text))
                .count();
            let standing: String = playback
                .world()
                .parts
                .get(&text)
                .map(|p| p.alive.iter().map(|a| if *a { '1' } else { '0' }).collect())
                .unwrap_or_default();
            last = log.hash();
            lines += &format!("{name}\t{second}\t{}\t{on_letters}\t{standing}\t{last:016x}\n", log.len());
            if second == SECONDS {
                assert!(on_letters > 0, "{name}: no ball hit the letters");
            }
        }
        let mut jumpy = Playback::new(&scene, &resolved, kit.sims.clone());
        for t in [5.0, 2.0, SECONDS as f64] {
            jumpy.seek(t);
        }
        assert_eq!(jumpy.log().hash(), last, "{name}: seeking changed the run");
    }
    let file = if cfg!(debug_assertions) { "tests/golden-text-debug.txt" } else { "tests/golden-text.txt" };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file);
    if std::env::var_os("BLESS_GOLDEN").is_some() {
        std::fs::write(&path, &lines).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).expect("the golden file is missing; run with BLESS_GOLDEN=1");
    assert_eq!(expected, lines, "text event logs changed");
}

//! Golden event logs for the stage 7 scenes: every scene must keep producing exactly the same
//! events. Any change
//! to the physics that moves a single hit shows up here.
//!
//! When a change is meant to alter the logs, run with `BLESS_GOLDEN=1` (once with `--release`
//! and once without) to rewrite the golden files, and say why in the pull request. Release and
//! debug builds each have their own file, because the optimiser may fold sine and cosine a hair
//! differently from the maths library at run time.

use bpe_kit::{kit, scenes};
use bpe_runtime::Playback;

const SECONDS: f64 = 20.0;

fn playbacks() -> Vec<(String, Playback)> {
    let kit = kit();
    scenes::stage7(&kit.registry)
        .into_iter()
        .map(|(name, scene)| {
            let resolved = bpe_core::resolve(&scene, &kit.registry);
            assert!(resolved.problems.is_empty(), "{name}: {:?}", resolved.problems);
            (name, Playback::new(&scene, &resolved, kit.sims.clone()))
        })
        .collect()
}

#[test]
fn every_stage_scene_keeps_its_event_log() {
    let mut lines = String::from("# scene\tevents\thash\n");
    for (name, mut playback) in playbacks() {
        playback.seek(SECONDS);
        let log = playback.log();
        assert!(log.events().windows(2).all(|w| w[0].step <= w[1].step), "{name}: events out of order");
        lines += &format!("{name}\t{}\t{:016x}\n", log.len(), log.hash());
    }
    let file = if cfg!(debug_assertions) { "tests/golden7-debug.txt" } else { "tests/golden7.txt" };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file);
    if std::env::var_os("BLESS_GOLDEN").is_some() {
        std::fs::write(&path, &lines).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).expect("the golden file is missing; run with BLESS_GOLDEN=1");
    let differ: Vec<_> = expected
        .lines()
        .zip(lines.lines())
        .filter(|(a, b)| a != b)
        .map(|(a, b)| format!("  was  {a}\n  now  {b}"))
        .collect();
    assert!(
        differ.is_empty() && expected.lines().count() == lines.lines().count(),
        "event logs changed:\n{}",
        differ.join("\n")
    );
}

#[test]
fn seeking_around_replays_every_scene_identically() {
    for ((name, mut straight), (_, mut jumpy)) in playbacks().into_iter().zip(playbacks()) {
        straight.seek(SECONDS);
        for t in [7.0, 3.0, SECONDS] {
            jumpy.seek(t);
        }
        assert_eq!(straight.log().hash(), jumpy.log().hash(), "{name}");
        assert_eq!(straight.log().len(), jumpy.log().len(), "{name}");
    }
}

#[test]
fn the_scenes_are_lively() {
    use bpe_runtime::EventKind;
    for (name, mut playback) in playbacks() {
        playback.seek(SECONDS);
        let hits = playback.log().events().iter().filter(|e| e.kind == EventKind::Hit).count();
        let beats = playback.log().events().iter().filter(|e| e.kind == EventKind::Beat).count();
        let (least_hits, least_beats) = match name.as_str() {
            "Jelly drop" => (5, 0),
            "Rope bridge" => (5, 0),
            "Cloth hammock" => (3, 0),
            "Captioned reel" => (300, 0),
            "Drum beat" => (10, 40),
            _ => (1, 0),
        };
        assert!(hits >= least_hits, "{name}: only {hits} hits in {SECONDS} s");
        assert!(beats >= least_beats, "{name}: only {beats} beats in {SECONDS} s");
        for bodies in playback.world().bodies.values() {
            assert!(bodies.position.iter().all(|p| p.is_finite()), "{name}: a ball went non-finite");
        }
        for state in playback.world().states.values() {
            if let Some(soft) = state.as_any().downcast_ref::<bpe_kit::SoftState>() {
                assert!(
                    soft.world.points.iter().all(|p| p.position.is_finite()),
                    "{name}: a soft point went non-finite"
                );
            }
        }
    }
}

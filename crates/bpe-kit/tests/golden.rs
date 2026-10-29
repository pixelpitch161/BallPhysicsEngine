//! Golden event logs: every stage scene must keep producing exactly the same events. Any change
//! to the physics that moves a single hit shows up here.
//!
//! When a change is meant to alter the logs, run with `BLESS_GOLDEN=1` (once with `--release`
//! and once without) to rewrite the golden files, and say why in the pull request. Release and
//! debug builds each have their own file, because the optimiser may fold sine and cosine a hair
//! differently from the maths library at run time.

use bpe_kit::{kit, scenes};
use bpe_runtime::{EventKind, Playback};

const SECONDS: f64 = 20.0;

fn playbacks() -> Vec<(String, Playback)> {
    let kit = kit();
    scenes::all(&kit.registry)
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
    let file = if cfg!(debug_assertions) { "tests/golden-debug.txt" } else { "tests/golden.txt" };
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
    for (name, mut playback) in playbacks() {
        playback.seek(SECONDS);
        let (least, kind) = match name.as_str() {
            "Reel block" => (1000, EventKind::Hit),
            "Peg board" | "Ball pit" => (100, EventKind::Hit),
            "One ball" => (10, EventKind::Hit),
            "Fountain" => (400, EventKind::Spawn),
            "Wind tunnel" => (4, EventKind::Enter),
            "Small pool" | "Deep pool" => (3, EventKind::Hit),
            "Fireflies" => (10, EventKind::Hit),
            _ => (1, EventKind::Hit),
        };
        let events = playback.log().events().iter().filter(|e| e.kind == kind).count();
        assert!(events >= least, "{name}: only {events} {kind:?} events in {SECONDS} s");
        for bodies in playback.world().bodies.values() {
            assert!(bodies.position.iter().all(|p| p.is_finite()), "{name}: a ball went non-finite");
        }
    }
}

/// A liquid is pushed by the balls and never pushes back; a swarm lives on the graphics card.
/// Neither may change a single hit of the balls.
#[test]
fn liquids_and_swarms_never_move_the_balls_log() {
    let kit = kit();
    for (scene_name, thing_name) in [("Small pool", "Pool"), ("Fireflies", "Fireflies")] {
        let (_, scene) = scenes::all(&kit.registry).into_iter().find(|(n, _)| n == scene_name).unwrap();
        let mut without = scene.clone();
        let id = *without.things.iter().find(|(_, t)| t.name == thing_name).map(|(id, _)| id).unwrap();
        without.remove_thing(id).unwrap();
        let logs: Vec<(usize, u64)> = [&scene, &without]
            .into_iter()
            .map(|scene| {
                let resolved = bpe_core::resolve(scene, &kit.registry);
                assert!(resolved.problems.is_empty(), "{scene_name}: {:?}", resolved.problems);
                let mut playback = Playback::new(scene, &resolved, kit.sims.clone());
                playback.seek(SECONDS);
                (playback.log().len(), playback.log().hash())
            })
            .collect();
        assert!(logs[0].0 > 0, "{scene_name}: the ball hit nothing");
        assert_eq!(logs[0], logs[1], "{scene_name}: removing {thing_name} changed the balls' log");
    }
}

#[test]
fn stage5_scenes_keep_their_stage2_twins_logs() {
    let kit = kit();
    let twins: std::collections::BTreeMap<String, Playback> = playbacks().into_iter().collect();
    for (name, scene, twin) in scenes::stage5_with_twins(&kit.registry) {
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(SECONDS);
        let mut expected = twins[&twin].clone();
        expected.seek(SECONDS);
        assert_eq!(playback.log().hash(), expected.log().hash(), "{name} drifted from {twin}");
        assert_eq!(playback.log().len(), expected.log().len(), "{name} drifted from {twin}");
        // The trail's history is the one thing the world keeps that its twin doesn't.
        let has_trail = resolved.things.values().any(|t| t.slots.contains_key(&bpe_core::SlotId::Trail));
        assert_eq!(!playback.world().history.is_empty(), has_trail, "{name}");
    }
}

//! Golden event logs: every preset and every scene file in `scenes/` must keep producing exactly
//! the same events. Any change to the physics that moves a single hit shows up here.
//!
//! When a change is meant to alter the logs, run with `BLESS_GOLDEN=1` (once with `--release`
//! and once without) to rewrite the golden files, and say why in the pull request.
//!
//! Release and debug builds each have their own file: a few presets work out starting speeds
//! with sine and cosine, which the optimiser can work out at compile time a hair differently
//! from the maths library at run time. The release file is the one that matches the program
//! people use.

use bpe_scene::Scene;
use bpe_sim::{Event, EventKind, collect_wall_hits};

fn fnv(hash: &mut u64, bytes: &[u8]) {
    for b in bytes {
        *hash ^= *b as u64;
        *hash = hash.wrapping_mul(0x100_0000_01b3);
    }
}

/// The kind as a tag and a number. Wall, Obstacle and Escape keep the tags they had before the
/// event log grew, so bounce hashes from older logs still compare.
fn tag(kind: EventKind) -> (u8, u32) {
    match kind {
        EventKind::Wall => (0, 0),
        EventKind::Obstacle(i) => (1, i),
        EventKind::Escape(k) => (2, k),
        EventKind::Ball(i) => (3, i),
        EventKind::Soft { body } => (4, body),
        EventKind::Spawn => (5, 0),
        EventKind::Remove => (6, 0),
        EventKind::Beat { index, downbeat } => (7, index * 2 + downbeat as u32),
        EventKind::Timer(t) => (8, t as u32),
        EventKind::Action { rule, action } => (9, (rule as u32) << 16 | action as u32),
    }
}

fn hash<'a>(events: impl Iterator<Item = &'a Event>, with_depth: bool) -> (usize, u64) {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut count = 0;
    for e in events {
        count += 1;
        fnv(&mut h, &e.time.to_bits().to_le_bytes());
        fnv(&mut h, &e.ball.to_le_bytes());
        fnv(&mut h, &e.position.x.to_bits().to_le_bytes());
        fnv(&mut h, &e.position.y.to_bits().to_le_bytes());
        fnv(&mut h, &e.speed.to_bits().to_le_bytes());
        let (tag, value) = tag(e.kind);
        fnv(&mut h, &[tag]);
        fnv(&mut h, &value.to_le_bytes());
        if with_depth {
            fnv(&mut h, &e.depth.to_bits().to_le_bytes());
        }
    }
    (count, h)
}

fn scenes() -> Vec<(String, Scene)> {
    let mut scenes: Vec<(String, Scene)> =
        Scene::presets().into_iter().map(|p| (p.name.to_string(), p.scene)).collect();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenes");
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "ron") {
            scenes.push((path.file_name().unwrap().to_string_lossy().into(), Scene::load(&path).unwrap()));
        }
    }
    scenes.sort_by(|a, b| a.0.cmp(&b.0));
    scenes
}

#[test]
fn every_scene_keeps_its_event_log() {
    let mut lines = String::from("# scene\tevents\tall-hash\tbounces\tbounce-hash\n");
    for (name, scene) in scenes() {
        let events = collect_wall_hits(&scene);
        assert!(events.windows(2).all(|w| w[0].time <= w[1].time), "{name}: events out of order");
        let (all, all_hash) = hash(events.iter(), true);
        let (bounces, bounce_hash) = hash(events.iter().filter(|e| e.kind.is_bounce()), false);
        lines += &format!("{name}\t{all}\t{all_hash:016x}\t{bounces}\t{bounce_hash:016x}\n");
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

//! A sweep over random variations of every preset: nothing panics, no ball turns into NaN,
//! the event log is in time order, a second run gives the same log, and seeking back and forth
//! lands on the same balls as playing straight through.
//!
//! Slow, so it only runs when asked: `cargo test --release -p bpe-sim --test fuzz -- --ignored`.
//! `FUZZ_SEEDS` sets how many variations to try (default 500), `FUZZ_SECONDS` how long each runs.

use bpe_scene::{Group, Scene};
use bpe_sim::{Event, Playback, Simulation};

const GROUPS: [Group; 11] = [
    Group::Shape,
    Group::Movement,
    Group::World,
    Group::Bounces,
    Group::NewBalls,
    Group::Background,
    Group::Trails,
    Group::Glow,
    Group::Strings,
    Group::Chords,
    Group::Notes,
];

fn env(name: &str, default: f64) -> f64 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn run(scene: &Scene, seconds: f64) -> (Vec<Event>, Simulation) {
    let mut sim = Simulation::new(scene);
    let mut events = Vec::new();
    sim.advance_to(seconds, &mut events);
    (events, sim)
}

#[test]
#[ignore]
fn random_variations_stay_sane_and_repeatable() {
    let seeds = env("FUZZ_SEEDS", 500.0) as u64;
    let seconds = env("FUZZ_SECONDS", 10.0);
    let presets = Scene::presets();
    let mut failures = Vec::new();
    for seed in 0..seeds {
        let preset = &presets[seed as usize % presets.len()];
        let groups: Vec<Group> = (0..preset.scene.spawners.len()).map(Group::Balls).chain(GROUPS).collect();
        let mut scene = preset.scene.variation(&groups, seed, 1.0);
        scene.video.duration = seconds as f32;
        let (events, sim) = run(&scene, seconds);
        let label = format!("{} seed {seed}", preset.name);
        if sim.balls().iter().any(|b| !b.position.is_finite() || !b.velocity.is_finite()) {
            failures.push(format!("{label}: a ball's position or speed is not a number"));
        }
        if events.windows(2).any(|w| w[1].time < w[0].time) {
            failures.push(format!("{label}: events out of time order"));
        }
        if events.iter().any(|e| !e.speed.is_finite() || !e.position.is_finite()) {
            failures.push(format!("{label}: an event has a non-number"));
        }
        let (again, _) = run(&scene, seconds);
        if again != events {
            failures.push(format!("{label}: a second run gave a different log"));
        }
        if seed % 10 == 0 {
            let mut playback = Playback::new(&scene);
            playback.seek(seconds * 0.8);
            playback.seek(seconds * 0.3);
            playback.seek(seconds * 0.6);
            let mut straight = Simulation::new(&scene);
            straight.advance_to(playback.simulation().time(), &mut Vec::new());
            let a: Vec<_> = playback.simulation().balls().iter().map(|b| b.position).collect();
            let b: Vec<_> = straight.balls().iter().map(|b| b.position).collect();
            if a != b {
                failures.push(format!("{label}: seeking ended somewhere else than playing"));
            }
        }
    }
    assert!(failures.is_empty(), "{} problems:\n{}", failures.len(), failures.join("\n"));
}

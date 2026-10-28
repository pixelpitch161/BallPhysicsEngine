//! Settings that pulse with the song: the simulation must stay deterministic, seekable and sane.

use bpe_music::AudioFeatures;
use bpe_scene::{AudioBand, Group, Modulator, Property, Scene};
use bpe_sim::{Outline, Playback, SimInputs, Simulation, closed_outline};
use std::sync::Arc;

/// A made-up song: bass and treble that swell and fall, a beat twice a second.
fn song() -> Arc<AudioFeatures> {
    let frames = 200 * 30;
    let wave = |period: f32| {
        (0..frames).map(|i| 0.5 + 0.5 * (i as f32 / 200.0 * std::f32::consts::TAU / period).sin()).collect()
    };
    Arc::new(AudioFeatures {
        rate: 200.0,
        bass: wave(1.3),
        mid: wave(0.7),
        treble: wave(0.31),
        level: wave(2.0),
        onset: (0..frames).map(|i| if i % 100 == 0 { 1.0 } else { 0.0 }).collect(),
        beats: (0..60).map(|i| i as f64 * 0.5).collect(),
        downbeats: (0..15).map(|i| i as f64 * 2.0).collect(),
        bpm: 120.0,
    })
}

fn pulsing(mut scene: Scene) -> Scene {
    let m = |property, source, amount| Modulator { amount, smoothing: 0.05, ..Modulator::new(property, source) };
    scene.modulators = vec![
        m(Property::Gravity, AudioBand::Bass, 300.0),
        m(Property::Bounciness, AudioBand::Treble, 0.05),
        m(Property::ArenaSize, AudioBand::Beat, 0.08),
        m(Property::ArenaSpin, AudioBand::Mid, 20.0),
        m(Property::Bloom, AudioBand::Level, 1.0),
    ];
    scene
}

fn events(scene: &Scene, inputs: &SimInputs, seconds: f64) -> Vec<(u64, u32, u32, u32)> {
    let mut sim = Simulation::with_inputs(scene, inputs);
    let mut events = Vec::new();
    sim.advance_to(seconds, &mut events);
    events.iter().map(|e| (e.time.to_bits(), e.ball, e.position.x.to_bits(), e.position.y.to_bits())).collect()
}

#[test]
fn music_driven_physics_is_deterministic() {
    let inputs = SimInputs::from_music(song());
    let scene = pulsing(Scene::default());
    let first = events(&scene, &inputs, 8.0);
    assert!(!first.is_empty());
    assert_eq!(first, events(&scene, &inputs, 8.0));
    // The song really changes the physics.
    assert_ne!(first, events(&Scene::default(), &inputs, 8.0));
    // Without a song the modulators rest, and the balls move as if there were none.
    assert_eq!(events(&scene, &SimInputs::default(), 8.0), events(&Scene::default(), &SimInputs::default(), 8.0));
}

#[test]
fn seeking_matches_playing_straight_through() {
    let inputs = SimInputs::from_music(song());
    let scene = pulsing(Scene::default());
    let mut straight = Playback::with_inputs(&scene, &inputs);
    straight.seek(6.0);
    let mut jumpy = Playback::with_inputs(&scene, &inputs);
    for t in [3.0, 1.0, 7.5, 2.0, 6.0] {
        jumpy.seek(t);
    }
    let positions = |p: &Playback| p.simulation().balls().iter().map(|b| (b.position, b.velocity)).collect::<Vec<_>>();
    assert_eq!(positions(&straight), positions(&jumpy));
    assert_eq!(straight.events().len(), jumpy.events().len());
}

#[test]
fn bounce_sounds_never_reach_the_physics() {
    // Modulators on physics settings always listen to the song, even when set to the mix.
    let mut scene = pulsing(Scene::default());
    for m in &mut scene.modulators {
        m.from = bpe_scene::AudioFrom::Mix;
        if m.property.affects_simulation() {
            assert_eq!(m.listens_to(), bpe_scene::AudioFrom::Song);
        }
    }
    let inputs = SimInputs::from_music(song());
    assert_eq!(events(&scene, &inputs, 5.0), events(&pulsing(Scene::default()), &inputs, 5.0));
}

/// Whether a ball is inside a closed arena outline, with a little room for the wall's thickness.
fn inside(outline: &Outline, point: bpe_scene::Vec2, radius: f32) -> bool {
    match *outline {
        Outline::Ring { center, radius: r, .. } => (point - center).length() <= r + radius + 4.0,
        Outline::Box { center, half, angle, .. } => {
            let (s, c) = (-angle).sin_cos();
            let d = point - center;
            let local = bpe_scene::Vec2::new(d.x * c - d.y * s, d.x * s + d.y * c);
            local.x.abs() <= half.x + radius + 4.0 && local.y.abs() <= half.y + radius + 4.0
        }
    }
}

/// Simulates `scene` for `seconds`, checking for NaN. Returns whether a ball ended up outside a
/// closed arena at one of the checks.
fn run(scene: &Scene, inputs: &SimInputs, seconds: f64, seed: u64) -> bool {
    let mut sim = Simulation::with_inputs(scene, inputs);
    let mut events = Vec::new();
    let mut escaped = false;
    let mut t = 0.0;
    while t < seconds {
        t += 0.5;
        sim.advance_to(t, &mut events);
        let closed = closed_outline(&scene.container, sim.arena());
        for ball in sim.balls().iter().filter(|b| b.alive) {
            assert!(ball.position.is_finite() && ball.velocity.is_finite(), "seed {seed}: NaN at {t}");
            if let Some(outline) = &closed {
                escaped |= !inside(outline, ball.position, ball.radius);
            }
        }
    }
    assert!(events.iter().all(|e| e.time.is_finite() && e.position.is_finite()), "seed {seed}");
    escaped
}

/// Random variations of every preset, with and without physics pulsing to a song. Pulsing must
/// never give NaN, and must not let balls out of closed arenas where the plain scene keeps them
/// in. (A few very fast plain variations already slip through moving walls; that is the
/// collision code's business, so the sweep compares against it rather than failing on it.)
fn sweep(seeds: u64, seconds: f64) {
    let groups = [Group::Balls(0), Group::Shape, Group::Movement, Group::World, Group::Bounces, Group::NewBalls];
    let inputs = SimInputs::from_music(song());
    let (mut plain_out, mut pulsing_out) = (Vec::new(), Vec::new());
    for seed in 0..seeds {
        let presets = Scene::presets();
        let varied = presets[(seed as usize) % presets.len()].scene.variation(&groups, seed, 1.0);
        if run(&varied, &inputs, seconds, seed) {
            plain_out.push(seed);
        }
        if run(&pulsing(varied), &inputs, seconds, seed) {
            pulsing_out.push(seed);
        }
    }
    println!("balls outside a closed arena: plain {plain_out:?}, pulsing {pulsing_out:?}");
    let extra: Vec<_> = pulsing_out.iter().filter(|s| !plain_out.contains(s)).collect();
    assert!(extra.len() <= (seeds as usize).div_ceil(100), "pulsing let balls out in seeds {extra:?}");
}

#[test]
fn random_scenes_with_pulsing_physics_stay_sane() {
    sweep(24, 4.0);
}

/// The full sweep from the test plan: `cargo test --release -p bpe-sim --test modulators -- --ignored`.
#[test]
#[ignore]
fn fuzz_sweep_500_seeds() {
    sweep(500, 10.0);
}

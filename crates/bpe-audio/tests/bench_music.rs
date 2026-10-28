//! Timings for the music features, against the plan's targets. Run in release:
//! `cargo test --release -p bpe-audio --test bench_music -- --ignored --nocapture`.

use bpe_audio::{MixOptions, SAMPLE_RATE};
use bpe_scene::{EventFilter, Instrument, Melody, PitchMode, Scene, SyncMode, Vec2, VoiceMap};
use bpe_sim::{Event, EventKind};
use std::time::Instant;

fn best_of<T>(runs: usize, mut f: impl FnMut() -> T) -> (f64, T) {
    let mut best = f64::MAX;
    let mut last = None;
    for _ in 0..runs {
        let started = Instant::now();
        let out = f();
        best = best.min(started.elapsed().as_secs_f64());
        last = Some(out);
    }
    (best, last.unwrap())
}

#[test]
#[ignore]
fn mixing_5000_hits_in_30_seconds() {
    let mut scene = Scene::default();
    scene.video.duration = 30.0;
    scene.sound.instrument = Instrument::library("Grand piano");
    scene.sound.voices = vec![
        VoiceMap { on: EventFilter::Escapes, instrument: Instrument::library("Drum kit"), pitch: PitchMode::Fixed(49), ..VoiceMap::default() },
        VoiceMap { on: EventFilter::Walls, instrument: Instrument::library("Marimba"), gain: -6.0, ..VoiceMap::default() },
    ];
    scene.music.melody = Some(Melody::default());
    let events: Vec<Event> = (0..5000)
        .map(|i| Event {
            time: i as f64 * 30.0 / 5000.0,
            ball: (i % 40) as u32,
            position: Vec2::new((i % 13) as f32 * 60.0 - 360.0, 0.0),
            depth: 0.0,
            speed: 300.0 + (i % 17) as f32 * 80.0,
            kind: if i % 50 == 0 { EventKind::Escape(0) } else { EventKind::Wall },
        })
        .collect();
    let song: Vec<f32> = (0..30 * SAMPLE_RATE as usize * 2).map(|i| ((i as f32) * 0.001).sin() * 0.3).collect();
    let options = MixOptions { song: Some(&song), ..MixOptions::default() };
    let (seconds, mix) = best_of(3, || bpe_audio::mix(&scene, &events, &options).unwrap());
    println!("mix: 5000 hits, 3 voices, melody and a song over 30 s in {:.0} ms (target under 1000)", seconds * 1000.0);
    assert!(mix.notes.len() >= 5000);
    assert!(seconds < 1.0);
}

#[test]
#[ignore]
fn lock_cost_per_hit() {
    let scene = Scene::beat_locked_bounce();
    let inputs = bpe_audio::sim_inputs(&scene, None);
    let mut free = scene.clone();
    free.music.beat_sync.mode = SyncMode::Off;
    let (locked, events) = best_of(3, || bpe_sim::collect_events(&scene, &inputs));
    let (plain, _) = best_of(3, || bpe_sim::collect_events(&free, &inputs));
    let hits = events.iter().filter(|e| e.kind.is_bounce()).count().max(1);
    let per_hit = (locked - plain) / hits as f64 * 1000.0;
    println!("lock: {hits} hits, {:.3} ms extra per hit (target under 2)", per_hit);
    assert!(per_hit < 2.0);
}

#[test]
#[ignore]
fn nudge_time() {
    for scene in [Scene::rainbow_trails(), Scene::default()] {
        let mut scene = scene;
        scene.music.bpm = 120.0;
        scene.music.beat_sync.mode = SyncMode::Nudge;
        let beats = bpe_sim::beat_sync::subdivided(&bpe_sim::beat_sync::beat_grid(&scene, &[]), 2);
        let (seconds, found) = best_of(1, || bpe_sim::beat_sync::nudge(&scene, &beats));
        println!(
            "nudge {}: {:.2} s, on the beat {:.0}% to {:.0}% (target under 3 s)",
            scene.name,
            seconds,
            found.before * 100.0,
            found.after * 100.0
        );
        assert!(seconds < 3.0);
    }
}

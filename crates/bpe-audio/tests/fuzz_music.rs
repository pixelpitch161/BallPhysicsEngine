//! Random scenes with random music settings: the soundtrack is always made, stays within full
//! scale, has no NaNs, and its notes come in time order.

use bpe_scene::{
    Advance, BeatSync, Dice, Group, Instrument, Melody, NoteTrigger, PitchMode, Scene, SyncMode, VoiceMap,
};

const SEEDS: u64 = 500;
const LIBRARY: [&str; 13] = [
    "Pluck",
    "Electric piano",
    "Grand piano",
    "Marimba",
    "Vibraphone",
    "Glass",
    "Harp",
    "Bells",
    "Kalimba",
    "Supersaw lead",
    "Pad",
    "Synth bass",
    "Drum kit",
];

fn random_scene(seed: u64) -> Scene {
    let presets = Scene::presets();
    let base = &presets[seed as usize % presets.len()].scene;
    let groups = [Group::Balls(0), Group::Shape, Group::World, Group::Bounces, Group::Chords, Group::Notes];
    let mut scene = base.variation(&groups, seed, 1.0);
    scene.video.duration = 1.5;
    let mut dice = Dice::new(seed ^ 0x5eed);
    if dice.chance(0.5) {
        scene.sound.instrument = Instrument::library(dice.pick(&LIBRARY));
    }
    let filters = NoteTrigger::ALL;
    for _ in 0..dice.int(0, 4) {
        scene.sound.voices.push(VoiceMap {
            on: dice.pick(&filters),
            instrument: Instrument::library(dice.pick(&LIBRARY)),
            gain: dice.range(-20.0, 6.0),
            pitch: if dice.chance(0.5) { PitchMode::Tune } else { PitchMode::Fixed(dice.int(0, 127) as u8) },
            enabled: dice.chance(0.8),
        });
    }
    if dice.chance(0.4) {
        let tunes = ["builtin:ode-to-joy", "builtin:fur-elise", "builtin:twinkle"];
        scene.music.melody = Some(Melody {
            path: dice.pick(&tunes).into(),
            transpose: dice.int(0, 72) as i32 - 36,
            advance_on: dice.pick(&[Advance::AllHits, Advance::WallHits, Advance::PerBall]),
            velocity_from_impact: dice.chance(0.5),
            track: None,
        });
    }
    scene.music.bpm = if dice.chance(0.5) { dice.range(30.0, 300.0) } else { 0.0 };
    scene.music.beat_sync = BeatSync {
        mode: dice.pick(&[SyncMode::Off, SyncMode::Lock, SyncMode::Warp]),
        lead_balls: dice.int(1, 4),
        max_change: dice.range(0.0, 0.5),
        subdivisions: dice.pick(&[1, 2, 4]),
        tolerance: dice.range(0.0, 0.1),
        warp_strength: dice.range(0.0, 0.95),
    };
    scene.sound.target_loudness = dice.range(-40.0, -6.0);
    scene.sound.stereo_width = dice.range(0.0, 1.0);
    scene
}

#[test]
fn random_music_settings_always_mix_cleanly() {
    let threads = std::thread::available_parallelism().map_or(2, |n| n.get()).min(16) as u64;
    let failures = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for t in 0..threads {
            let failures = &failures;
            scope.spawn(move || {
                for seed in (t..SEEDS).step_by(threads as usize) {
                    let scene = random_scene(seed);
                    let problem = match bpe_audio::render_scene(&scene, None) {
                        Err(error) => Some(format!("{error:#}")),
                        Ok(sound) => {
                            let samples = &sound.mix.track.samples;
                            let frames = (scene.video.duration * bpe_audio::SAMPLE_RATE as f32).round() as usize;
                            if samples.len() != frames * 2 {
                                Some(format!("{} samples, expected {}", samples.len(), frames * 2))
                            } else if let Some(bad) = samples.iter().find(|s| !s.is_finite() || s.abs() > 1.0) {
                                Some(format!("sample {bad}"))
                            } else if sound.mix.notes.windows(2).any(|w| w[1].time < w[0].time) {
                                Some("notes out of order".into())
                            } else if sound.on_beat.is_some_and(|s| !(0.0..=1.0).contains(&s)) {
                                Some(format!("on-beat share {:?}", sound.on_beat))
                            } else {
                                None
                            }
                        }
                    };
                    if let Some(problem) = problem {
                        failures.lock().unwrap().push(format!("seed {seed}: {problem}"));
                    }
                }
            });
        }
    });
    let failures = failures.into_inner().unwrap();
    assert!(failures.is_empty(), "{} of {SEEDS} seeds failed:\n{}", failures.len(), failures.join("\n"));
}

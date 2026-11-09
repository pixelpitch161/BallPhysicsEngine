//! W11: a Sample oscillator plays a sound file the scene keeps, at the hit's note from its root
//! (or as recorded with Follow notes off), and the Mixer's backing track plays with the mix from
//! its Starts at, through every seek and into the export.

use std::sync::Arc;

use bpe_core::{Params, Scene, Settings, SlotId, ThingId, Value};
use bpe_kit::sound::{MixerSettings, SampleLoop, VoiceSettings, Wave, starter_voice};
use bpe_kit::{Kit, new_scene, resolve_scene};
use bpe_runtime::{Event, EventKind, Log, Party, Playback};
use bpe_sound::patch::shape_voice;
use bpe_sound::{BLOCK, BlockCx, BlockHit, Engine, TempoMap, VoiceSource};
use glam::Vec2;

const SHAPE: ThingId = ThingId(1);

/// A WAV of a sine at `hz`, `seconds` long, at `rate`, 16-bit mono, at half level.
fn wav(hz: f64, seconds: f64, rate: u32) -> Arc<[u8]> {
    let spec =
        hound::WavSpec { channels: 1, sample_rate: rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut out = std::io::Cursor::new(Vec::new());
    {
        let mut w = hound::WavWriter::new(&mut out, spec).unwrap();
        for i in 0..(seconds * rate as f64) as usize {
            let v = 0.5 * (std::f64::consts::TAU * hz * i as f64 / rate as f64).sin();
            w.write_sample((v * 32767.0) as i16).unwrap();
        }
        w.finalize().unwrap();
    }
    out.into_inner().into()
}

/// The new scene with a sample of `hz` kept as "Tone" on the shape's Osc A (Osc B off), set by
/// `edit`.
fn sampled(kit: &Kit, hz: f64, edit: impl FnOnce(&mut VoiceSettings)) -> Scene {
    let mut scene = new_scene(&kit.registry);
    let name = scene.add_audio("Tone", wav(hz, 1.0, 44_100));
    let mut card = starter_voice(&kit.registry);
    let mut v = VoiceSettings::from_params(&card.params);
    v.play_retrigger = 0.0;
    v.a.wave = Wave::Sample;
    v.a.sample = name;
    v.a.sample_root = 69;
    v.b.on = false;
    edit(&mut v);
    let mut params: Params = v.to_params();
    for (k, val) in card.params.iter() {
        if params.get(k).is_none() {
            params.set(k, val.clone());
        }
    }
    card.params = params;
    scene.things.get_mut(&SHAPE).unwrap().slots.insert(SlotId::Sound, card);
    scene
}

/// What the shape's voice plays for one hit at sample 0, `blocks` blocks of the left channel.
fn played(scene: &Scene, kit: &Kit, blocks: usize) -> Vec<f32> {
    let resolved = resolve_scene(scene, &kit.registry);
    let card = &resolved.things[&SHAPE].slots[&SlotId::Sound];
    let mut v = shape_voice(scene, SHAPE, card, 0, 1080.0).expect("its sound is on");
    let p = Playback::new(scene, &resolved, kit.sims.clone());
    let tempo = TempoMap::default();
    let log = Log::default();
    let hit = Event {
        step: 1,
        kind: EventKind::Hit,
        a: Party { thing: ThingId(2), item: 0 },
        b: Some(Party { thing: SHAPE, item: 0 }),
        point: Vec2::ZERO,
        normal: Vec2::Y,
        speed: 1500.0,
        at: 0.0,
    };
    let mut out = Vec::new();
    for b in 0..blocks {
        let start = b as u64 * BLOCK as u64;
        let cx = BlockCx {
            start_sample: start,
            time: start as f64 / 48_000.0,
            tempo: &tempo,
            world: p.world(),
            log: &log,
            playing: true,
        };
        let hits = if b == 0 { vec![BlockHit { offset: 0, event: hit.clone() }] } else { Vec::new() };
        let (mut l, mut r) = ([0.0; BLOCK], [0.0; BLOCK]);
        v.on_block(&cx, &hits, &mut l, &mut r);
        out.extend_from_slice(&l);
    }
    out
}

/// The sound's frequency from the rises between its first and last loud ones.
fn frequency(x: &[f32]) -> f64 {
    let loud = x.iter().fold(0.0f32, |m, v| m.max(v.abs())) * 0.05;
    let rises: Vec<usize> = (1..x.len()).filter(|&i| x[i - 1] <= 0.0 && x[i] > 0.0 && x[i].abs() < 1.0).collect();
    let rises: Vec<usize> =
        rises.into_iter().filter(|&i| x[i..(i + 40).min(x.len())].iter().any(|v| v.abs() > loud)).collect();
    assert!(rises.len() > 4, "it sounds");
    let (first, last) = (rises[0], *rises.last().unwrap());
    (rises.len() - 1) as f64 * 48_000.0 / (last - first) as f64
}

#[test]
fn a_sample_plays_the_notes_pitch_from_its_root() {
    let kit = bpe_sound::kit();
    // The starter plays C5 (60); the sample is an A at 440 Hz recorded as note 69.
    let out = played(&sampled(&kit, 440.0, |_| {}), &kit, 150);
    let hz = frequency(&out[..9600]);
    let want = 440.0 * 2f64.powf((60.0 - 69.0) / 12.0);
    assert!((hz - want).abs() < 2.0, "{hz} against {want}");
}

#[test]
fn follow_notes_off_plays_it_as_recorded_moved_only_by_semi() {
    let kit = bpe_sound::kit();
    let out = played(&sampled(&kit, 440.0, |v| v.a.sample_follow = false), &kit, 150);
    let hz = frequency(&out[..9600]);
    assert!((hz - 440.0).abs() < 2.0, "{hz}");
    let out = played(
        &sampled(&kit, 440.0, |v| {
            v.a.sample_follow = false;
            v.a.semi = 12;
        }),
        &kit,
        150,
    );
    let hz = frequency(&out[..9600]);
    assert!((hz - 880.0).abs() < 4.0, "{hz}");
}

#[test]
fn a_missing_sample_is_silent_and_a_loop_keeps_going() {
    let kit = bpe_sound::kit();
    let out = played(&sampled(&kit, 440.0, |v| v.a.sample = "Gone".into()), &kit, 40);
    assert!(out.iter().all(|x| *x == 0.0));
    // A held note (sustain up) on a looped sample sounds past the sample's own second.
    let scene = sampled(&kit, 440.0, |v| {
        v.a.sample_loop = SampleLoop::Forward;
        v.a.sample_loop_start = 50.0;
        v.a.sample_loop_end = 60.0;
        v.a.sample_follow = false;
    });
    let mut scene = scene;
    // The first envelope holds at full level.
    let card = scene.things.get_mut(&SHAPE).unwrap().slots.get_mut(&SlotId::Sound).unwrap();
    let env = card.layers.iter().find(|l| l.kind == bpe_kit::sound::ADSR).unwrap().id;
    card.params.set(&bpe_core::layered_key(env, "sustain"), Value::Number(100.0));
    card.params.set(&bpe_core::layered_key(env, "gate_ms"), Value::Number(5000.0));
    let out = played(&scene, &kit, 1100);
    let tail = &out[60_000..70_000];
    assert!(tail.iter().any(|v| v.abs() > 0.05), "still sounding at 1.25 s");
}

/// The new scene, silent but for a backing track of a 100 Hz tone, set by `edit`.
fn with_song(kit: &Kit, edit: impl FnOnce(&mut MixerSettings)) -> Scene {
    let mut scene = new_scene(&kit.registry);
    let name = scene.add_audio("Song", wav(100.0, 1.0, 48_000));
    bpe_kit::ensure_mixer(&mut scene, &kit.registry);
    let card = scene.things.get_mut(&ThingId::ROOT).unwrap().slots.get_mut(&SlotId::Mixer).unwrap();
    let mut m = MixerSettings::from_params(&card.params);
    m.backing = name;
    edit(&mut m);
    card.params = m.to_params();
    scene
}

fn first_sound(x: &[f32]) -> Option<usize> {
    x.chunks(2).position(|f| f[0].abs() > 1e-4)
}

#[test]
fn the_backing_track_comes_in_at_starts_at() {
    let kit = bpe_sound::kit();
    let scene = with_song(&kit, |m| m.backing_at = 0.25);
    assert!(bpe_sound::has_sound(&scene));
    let out = bpe_sound::render_samples(&scene, &kit, 1.5);
    let first = first_sound(&out).expect("the song plays");
    // A rising sine's first sample past the threshold is a frame or two in.
    assert!((12_000..12_004).contains(&first), "{first}");
    // It ends a second later.
    let last = out.chunks(2).rposition(|f| f[0].abs() > 1e-4).unwrap();
    assert!((59_990..60_000).contains(&last), "{last}");
}

#[test]
fn a_negative_start_skips_into_the_song_and_level_and_off_work() {
    let kit = bpe_sound::kit();
    let full = bpe_sound::render_samples(&with_song(&kit, |_| {}), &kit, 0.8);
    let skipped = bpe_sound::render_samples(&with_song(&kit, |m| m.backing_at = -0.5), &kit, 0.25);
    let tail = &full[2 * 24_000..2 * 36_000];
    assert!(skipped.iter().zip(tail).all(|(a, b)| (a - b).abs() < 1e-5), "half a second in");
    let quiet = bpe_sound::render_samples(&with_song(&kit, |m| m.backing_level = -6.0), &kit, 0.5);
    let ratio = quiet[2 * 1000 + 50] / full[2 * 1000 + 50];
    assert!((ratio - 0.501).abs() < 0.01, "{ratio}");
    let off = with_song(&kit, |m| m.backing_on = false);
    assert!(!bpe_sound::has_sound(&off));
    assert!(bpe_sound::render_samples(&off, &kit, 0.5).iter().all(|x| *x == 0.0));
}

#[test]
fn a_seek_lands_in_the_right_place_of_the_song() {
    let kit = bpe_sound::kit();
    let scene = with_song(&kit, |_| {});
    let whole = bpe_sound::render_samples(&scene, &kit, 1.0);
    let resolved = resolve_scene(&scene, &kit.registry);
    let mut engine = Engine::new(&scene, &resolved, &kit);
    engine.seek(0.5);
    let mut block = [0.0f32; BLOCK * 2];
    engine.render(&mut block);
    let at = 2 * 24_000;
    assert!(block.iter().zip(&whole[at..at + BLOCK * 2]).all(|(a, b)| (a - b).abs() < 1e-5));
}

#[test]
fn the_backing_track_is_in_the_mix_but_no_bus_stem() {
    let kit = bpe_sound::kit();
    let scene = with_song(&kit, |_| {});
    let resolved = resolve_scene(&scene, &kit.registry);
    let mut engine = Engine::new(&scene, &resolved, &kit);
    engine.record_stems(true);
    let mut block = [0.0f32; BLOCK * 2];
    let (mut mix, mut stems) = (0.0f32, 0.0f32);
    for _ in 0..200 {
        engine.render(&mut block);
        mix = block.iter().fold(mix, |m, v| m.max(v.abs()));
        stems = engine.stems().iter().flatten().fold(stems, |m, v| m.max(v.abs()));
    }
    assert!(mix > 0.1, "the song is in the mix: {mix}");
    assert_eq!(stems, 0.0, "and in no bus stem");
}

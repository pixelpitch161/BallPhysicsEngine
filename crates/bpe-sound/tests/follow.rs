//! Follow sound: a setting moves with a shape's modulator, level or bus, worked out from the
//! physics alone, the same in the preview, the export and the physics.

use bpe_core::{
    Address, Animation, Inputs, LayerId, PulseSource, Resolved, Scene, SlotId, SoundSource, ThingId, Value, layered_key,
};
use bpe_kit::sound::{MixerSettings, starter_voice};
use bpe_kit::{Kit, new_scene, resolve_scene};
use bpe_runtime::{EventKind, LevelsAt, Log, Playback, SoundAt, World};
use bpe_sound::ControlSampler;
use bpe_sound::hits::hit_sample;

fn shape_of(scene: &Scene) -> ThingId {
    scene.order.iter().copied().find(|id| scene.things[id].kind == "shape").unwrap()
}

fn ball_of(scene: &Scene) -> ThingId {
    scene.order.iter().copied().find(|id| scene.things[id].kind == "balls").unwrap()
}

/// The new scene with the starter sound on its shape, Distance 1 and LFO 1 on.
fn scene_with_sound(kit: &Kit) -> Scene {
    let mut scene = new_scene(&kit.registry);
    bpe_kit::ensure_mixer(&mut scene, &kit.registry);
    let shape = shape_of(&scene);
    let mut card = starter_voice(&kit.registry);
    card.params.set(&layered_key(LayerId(2), "near"), Value::Number(100.0));
    card.params.set(&layered_key(LayerId(2), "far"), Value::Number(500.0));
    for l in card.layers.iter_mut().filter(|l| l.id == LayerId(2) || l.id == LayerId(4)) {
        l.enabled = true;
    }
    scene.things.get_mut(&shape).unwrap().slots.insert(SlotId::Sound, card);
    scene
}

const BODY_OPACITY: &str = "0/opacity";

/// The ball's body opacity at 0.2, following `source` of the shape by 0.8.
fn follow_opacity(kit: &Kit, scene: &mut Scene, source: SoundSource) -> Address {
    let (ball, shape) = (ball_of(scene), shape_of(scene));
    let look = scene.things.get_mut(&ball).unwrap().slots.get_mut(&SlotId::Look).unwrap();
    look.params.set(BODY_OPACITY, Value::Number(0.2));
    let a = Address::on(ball, SlotId::Look, BODY_OPACITY);
    scene.set_pulse(&kit.registry, &a, PulseSource::Sound { shape, source }, Value::Number(0.8), 0.25).unwrap();
    a
}

/// The ball's opacity as the picture shows it at `time`, the world and log as given.
fn opacity(scene: &Scene, resolved: &Resolved, sound: &dyn SoundAt, world: &World, log: &Log, time: f64) -> f64 {
    let ball = ball_of(scene);
    let mut r = resolved.clone();
    let levels = LevelsAt { sound, world, log };
    Animation::new(scene, resolved).apply(&mut r, time, &Inputs { sound: Some(&levels), ..Inputs::NONE });
    match r.things[&ball].slots[&SlotId::Look].params.get(BODY_OPACITY) {
        Some(Value::Number(v)) => *v,
        other => panic!("{other:?}"),
    }
}

/// The time of the first hit on the shape in `log`.
fn first_hit(log: &Log, shape: ThingId, steps_per_second: u32) -> f64 {
    let e = log
        .events()
        .iter()
        .find(|e| e.kind == EventKind::Hit && e.b.is_some_and(|b| b.thing == shape))
        .expect("the ball hits the shape");
    hit_sample(e, 48_000.0 / steps_per_second as f64) as f64 / 48_000.0
}

#[test]
fn a_glow_following_adsr1_pulses_on_hits() {
    let kit = bpe_sound::kit();
    let mut scene = scene_with_sound(&kit);
    follow_opacity(&kit, &mut scene, SoundSource::Modulator(LayerId(0)));
    let resolved = resolve_scene(&scene, &kit.registry);
    let sampler = ControlSampler::new(&scene, &resolved);
    let mut p = Playback::new(&scene, &resolved, kit.sims.clone());
    p.seek(4.0);
    let hit = first_hit(p.log(), shape_of(&scene), p.world().steps_per_second);
    let mut at = |t: f64| {
        p.seek(t);
        opacity(&scene, &resolved, &sampler, p.world(), p.log(), t)
    };
    let before = at(hit - 0.01);
    let after = at(hit + 0.01);
    assert!((before - 0.2).abs() < 1e-6, "nothing has hit yet: {before}");
    assert!(after > before + 0.3, "ADSR 1 is up just after the hit: {after}");
    let later = at(hit + 2.0 * 0.35 + 0.3);
    assert!(later < after, "and dies away: {later}");
}

#[test]
fn export_and_preview_follow_identically() {
    let kit = bpe_sound::kit();
    let scene = scene_with_sound(&kit);
    let shape = shape_of(&scene);
    let resolved = resolve_scene(&scene, &kit.registry);
    let (export, preview) = (ControlSampler::new(&scene, &resolved), ControlSampler::new(&scene, &resolved));
    let sources = [
        SoundSource::Modulator(LayerId(0)),
        SoundSource::Modulator(LayerId(2)),
        SoundSource::Modulator(LayerId(4)),
        SoundSource::Level,
        SoundSource::Bus(1),
    ];
    // The export steps straight through; the preview jumps about (and back), as scrubbing does.
    let times: Vec<f64> = (0..100).map(|i| i as f64 * 0.06).collect();
    let mut world = World::build(&scene, &resolved, &kit.sims);
    let mut log = Log::default();
    let mut exported = Vec::new();
    for &t in &times {
        world.advance_to(t, &kit.sims, &mut log);
        exported.push(sources.map(|s| export.value(&world, &log, shape, &s, t)));
    }
    let mut p = Playback::new(&scene, &resolved, kit.sims.clone());
    p.seek(5.0);
    let mut order: Vec<usize> = (0..100).collect();
    order.reverse();
    order.rotate_left(37);
    for i in order {
        let t = times[i];
        p.seek(t);
        let got = sources.map(|s| preview.value(p.world(), p.log(), shape, &s, t));
        assert_eq!(got, exported[i], "at {t}");
    }
    assert!(exported.iter().any(|v| v[0] > 0.5), "ADSR 1 moves");
    assert!(exported.iter().any(|v| v[3] > 0.1), "and the level with it");
    assert!(exported.iter().any(|v| v[1] > 0.0), "Distance 1 moves");
    assert!(exported.iter().any(|v| (v[2] - exported[0][2]).abs() > 0.1), "LFO 1 moves");
}

#[test]
fn physics_follow_uses_the_previous_step() {
    let kit = bpe_sound::kit();
    let mut scene = scene_with_sound(&kit);
    let (ball, shape) = (ball_of(&scene), shape_of(&scene));
    let size = Address::own(ball, "radius");
    let source = PulseSource::Sound { shape, source: SoundSource::Modulator(LayerId(2)) };
    scene.set_pulse(&kit.registry, &size, source, Value::Number(30.0), 0.25).unwrap();
    let resolved = resolve_scene(&scene, &kit.registry);
    let base = match resolved.things[&ball].params.get("radius") {
        Some(Value::Number(r)) => *r as f32,
        other => panic!("{other:?}"),
    };
    let sampler = ControlSampler::new(&scene, &resolved);
    let mut world = World::build(&scene, &resolved, &kit.sims);
    let mut log = Log::default();
    let mut grew = false;
    for _ in 0..600 {
        let level = sampler.value(&world, &log, shape, &SoundSource::Modulator(LayerId(2)), world.time());
        world.step(&kit.sims, &mut log);
        let r = world.bodies[&ball].radius[0];
        assert!((r - (base + 30.0 * level)).abs() < 1e-3, "step {}: {r} against {level}", world.step);
        grew |= level > 0.1;
    }
    assert!(grew, "the ball comes near enough to grow");
    // The same scene runs the same way again.
    let mut again = World::build(&scene, &resolved, &kit.sims);
    let mut log2 = Log::default();
    again.advance_to(world.time(), &kit.sims, &mut log2);
    assert_eq!(log.hash(), log2.hash());
    // Without Follow sound in the sims nothing grows.
    let plain = bpe_kit::kit();
    let mut still = World::build(&scene, &resolved, &plain.sims);
    still.advance_to(world.time(), &plain.sims, &mut Log::default());
    assert_eq!(still.bodies[&ball].radius[0], base);
}

#[test]
fn following_a_bus_level() {
    let kit = bpe_sound::kit();
    let mut scene = scene_with_sound(&kit);
    let shape = shape_of(&scene);
    let resolved = resolve_scene(&scene, &kit.registry);
    let mut p = Playback::new(&scene, &resolved, kit.sims.clone());
    p.seek(4.0);
    let hit = first_hit(p.log(), shape, p.world().steps_per_second);
    let t = hit + 0.01;
    p.seek(t);
    let level_of = |scene: &Scene, source: SoundSource| {
        let resolved = resolve_scene(scene, &kit.registry);
        ControlSampler::new(scene, &resolved).value(p.world(), p.log(), shape, &source, t)
    };
    let level = level_of(&scene, SoundSource::Level);
    assert!(level > 0.1, "{level}");
    // Bus 1 at 0 dB is the shape's level; at -6 dB about half of it.
    assert!((level_of(&scene, SoundSource::Bus(1)) - level).abs() < 1e-6);
    assert_eq!(level_of(&scene, SoundSource::Bus(2)), 0.0, "nothing plays into Bus 2");
    let mixer = scene.things.get_mut(&ThingId::ROOT).unwrap().slots.get_mut(&SlotId::Mixer).unwrap();
    mixer.params.set(&MixerSettings::bus_key(1, "fader"), Value::Number(-6.0));
    let half = level_of(&scene, SoundSource::Bus(1));
    assert!((half - level * 0.501).abs() < 0.01, "{half} against {level}");
    let mixer = scene.things.get_mut(&ThingId::ROOT).unwrap().slots.get_mut(&SlotId::Mixer).unwrap();
    mixer.params.set(&MixerSettings::bus_key(1, "mute"), Value::Bool(true));
    assert_eq!(level_of(&scene, SoundSource::Bus(1)), 0.0, "a muted bus is silent");
}

#[test]
fn the_engine_and_follow_sound_agree() {
    let kit = bpe_sound::kit();
    let scene = scene_with_sound(&kit);
    let shape = shape_of(&scene);
    let resolved = resolve_scene(&scene, &kit.registry);
    let sampler = ControlSampler::new(&scene, &resolved);
    let mut engine = bpe_sound::Engine::new(&scene, &resolved, &kit);
    let meters = engine.meters();
    let mut p = Playback::new(&scene, &resolved, kit.sims.clone());
    let mut out = [0.0f32; bpe_sound::BLOCK * 2];
    let mut compared = 0;
    for block in 0..(4 * 48_000 / bpe_sound::BLOCK) {
        engine.render(&mut out);
        if block % 30 != 0 {
            continue;
        }
        let t = engine.voice_time();
        p.seek(t);
        for layer in [LayerId(0), LayerId(2), LayerId(4)] {
            let Some(heard) = meters.mod_value(shape.0, layer) else { continue };
            let ours = sampler.value(p.world(), p.log(), shape, &SoundSource::Modulator(layer), t);
            assert!((heard - ours).abs() < 0.05, "{layer:?} at {t}: the engine has {heard}, Follow sound {ours}");
            compared += 1;
        }
    }
    assert!(compared > 50, "{compared}");
}

/// The scene with the ball's size following LFO 1, and its physics fingerprint.
fn size_follows_lfo(kit: &Kit) -> Scene {
    let mut scene = scene_with_sound(kit);
    let (ball, shape) = (ball_of(&scene), shape_of(&scene));
    let source = PulseSource::Sound { shape, source: SoundSource::Modulator(LayerId(4)) };
    scene.set_pulse(&kit.registry, &Address::own(ball, "radius"), source, Value::Number(30.0), 0.25).unwrap();
    scene
}

fn print(kit: &Kit, scene: &Scene) -> u64 {
    bpe_runtime::fingerprint(scene, &resolve_scene(scene, &kit.registry), &kit.sims)
}

#[test]
fn a_physics_follow_restarts_on_what_it_hears_and_the_tempo() {
    let kit = bpe_sound::kit();
    let scene = size_follows_lfo(&kit);
    let before = print(&kit, &scene);
    // The tempo sets synced LFO rates and gates in beats.
    let mut tempo = scene.clone();
    tempo.things.get_mut(&ThingId::ROOT).unwrap().params.set("tempo", Value::Number(90.0));
    assert_ne!(print(&kit, &tempo), before, "the tempo");
    let mut rate = scene.clone();
    let card = rate.things.get_mut(&shape_of(&rate)).unwrap().slots.get_mut(&SlotId::Sound).unwrap();
    card.params.set(&layered_key(LayerId(0), "attack"), Value::Number(250.0));
    assert_ne!(print(&kit, &rate), before, "ADSR 1's attack");
    let mut fader = scene.clone();
    let mixer = fader.things.get_mut(&ThingId::ROOT).unwrap().slots.get_mut(&SlotId::Mixer).unwrap();
    mixer.params.set(&MixerSettings::bus_key(1, "fader"), Value::Number(-6.0));
    assert_ne!(print(&kit, &fader), before, "a bus fader");
}

#[test]
fn a_physics_follow_keeps_playing_through_sound_it_does_not_hear() {
    let kit = bpe_sound::kit();
    let scene = size_follows_lfo(&kit);
    let before = print(&kit, &scene);
    let shape = shape_of(&scene);
    for key in ["a_cents", "b_detune", "out_pan", "out_send2"] {
        let mut s = scene.clone();
        let card = s.things.get_mut(&shape).unwrap().slots.get_mut(&SlotId::Sound).unwrap();
        card.params.set(key, Value::Number(7.0));
        assert_eq!(print(&kit, &s), before, "{key}");
    }
    let mut s = scene.clone();
    let mixer = s.things.get_mut(&ThingId::ROOT).unwrap().slots.get_mut(&SlotId::Mixer).unwrap();
    mixer.params.set(&MixerSettings::bus_key(1, "pan"), Value::Number(0.5));
    mixer.params.set("master_output", Value::Number(-3.0));
    assert_eq!(print(&kit, &s), before, "the mixer's pans and Master");
}

//! Rules on the Sound and Mixer cards (W13): a Change setting on a shape's Volume or a bus's
//! Fader is heard from when it is made, in the engine and in what Follow sound reads.

use bpe_core::{LayerId, Registry, Scene, SlotId, SoundSource, ThingId, Value, Who, layered_key};
use bpe_kit::sound::starter_voice;
use bpe_kit::{new_scene, resolve_scene};
use bpe_runtime::Playback;
use bpe_sound::{BLOCK, ControlSampler, Engine, mixer_rule_keys, voice_rule_keys};

fn shape_of(scene: &Scene) -> ThingId {
    scene.order.iter().copied().find(|id| scene.things[id].kind == "shape").unwrap()
}

/// The new scene with the starter sound on its shape (the ball hits it now and then).
fn sounding(reg: &Registry) -> Scene {
    let mut scene = new_scene(reg);
    bpe_kit::ensure_mixer(&mut scene, reg);
    let shape = shape_of(&scene);
    scene.things.get_mut(&shape).unwrap().slots.insert(SlotId::Sound, starter_voice(reg));
    scene
}

/// A rule setting `key` of `thing`'s `card` to `to` at `at` seconds.
fn change_at(scene: &mut Scene, reg: &Registry, at: f64, thing: ThingId, card: &str, key: &str, to: f64) {
    let rule = scene.add_rule("Change");
    let when = scene.add_block(reg, rule, "when.time", None).unwrap();
    scene.set_block_param(reg, rule, when, "at", Value::Number(at)).unwrap();
    let change = scene.add_block(reg, rule, "do.change", Some(when)).unwrap();
    for (k, v) in [
        ("who", Value::Who(Who::Thing { thing, ball: None })),
        ("card", Value::Choice(card.into())),
        ("setting", Value::Text(key.into())),
        ("to_number", Value::Number(to)),
    ] {
        scene.set_block_param(reg, rule, change, k, v).unwrap();
    }
}

/// The loudest sample of `seconds` of the scene's sound, from `from` seconds on.
fn loudest(scene: &Scene, from: f64, seconds: f64) -> f32 {
    let kit = bpe_sound::kit();
    let resolved = resolve_scene(scene, &kit.registry);
    let mut engine = Engine::new(scene, &resolved, &kit);
    let mut block = [0.0f32; BLOCK * 2];
    let skip = (from * 48_000.0) as usize / BLOCK;
    let mut peak = 0.0f32;
    for i in 0..((from + seconds) * 48_000.0) as usize / BLOCK {
        engine.render(&mut block);
        if i >= skip {
            peak = block.iter().fold(peak, |m, s| m.max(s.abs()));
        }
    }
    peak
}

#[test]
fn a_rule_turning_the_volume_all_the_way_down_silences_the_shape() {
    let kit = bpe_sound::kit();
    let reg = &kit.registry;
    let scene = sounding(reg);
    assert!(loudest(&scene, 1.0, 5.0) > 0.01, "the hits are heard");
    let mut quiet = scene.clone();
    change_at(&mut quiet, reg, 0.0, shape_of(&scene), "sound", "out_volume", -60.0);
    let left = loudest(&quiet, 1.0, 5.0);
    assert!(left < 0.002, "silenced from the start: {left}");
}

#[test]
fn a_rule_pulling_its_bus_fader_down_silences_it_from_then_on() {
    let kit = bpe_sound::kit();
    let reg = &kit.registry;
    let mut scene = sounding(reg);
    let before = loudest(&scene, 0.0, 6.0);
    assert!(before > 0.01);
    change_at(&mut scene, reg, 0.0, ThingId::ROOT, "mixer", "bus1_fader", -60.0);
    let after = loudest(&scene, 0.5, 5.5);
    assert!(after < 0.002, "the bus is all the way down: {after}");
}

#[test]
fn follow_sound_reads_the_volume_a_rule_set() {
    let kit = bpe_sound::kit();
    let reg = &kit.registry;
    let scene = sounding(reg);
    let shape = shape_of(&scene);
    let mut quiet = scene.clone();
    change_at(&mut quiet, reg, 0.0, shape, "sound", "out_volume", -60.0);
    let levels = |scene: &Scene| {
        let resolved = resolve_scene(scene, &kit.registry);
        let sampler = ControlSampler::new(scene, &resolved);
        let mut p = Playback::new(scene, &resolved, kit.sims.clone());
        (1..60)
            .map(|i| {
                let t = i as f64 * 0.1;
                p.seek(t);
                let level = sampler.value(p.world(), p.log(), shape, &SoundSource::Level, t);
                let bus = sampler.value(p.world(), p.log(), shape, &SoundSource::Bus(1), t);
                level.max(bus)
            })
            .fold(0.0f32, f32::max)
    };
    let loud = levels(&scene);
    assert!(loud > 0.05, "the hits show: {loud}");
    let left = levels(&quiet);
    assert!(left < loud * 0.01, "the rule's Volume shows too: {left} against {loud}");
}

#[test]
fn the_keys_a_rule_can_change_are_the_ones_the_sound_reads() {
    let kit = bpe_sound::kit();
    let reg = &kit.registry;
    let mut scene = sounding(reg);
    let shape = shape_of(&scene);
    let eq = scene.add_layer_in(reg, shape, SlotId::Sound, bpe_kit::sound::EQ, Some(0)).unwrap();
    let delay = scene.add_layer_in(reg, ThingId::ROOT, SlotId::Mixer, "fx.delay", Some(0)).unwrap();
    let resolved = resolve_scene(&scene, reg);
    let voice = voice_rule_keys(&scene, &resolved, shape);
    for key in ["out_volume", "out_pan", "a_gain", "out_send1", &layered_key(eq, "band5_gain")] {
        assert!(voice.binary_search(&key.to_owned()).is_ok(), "{key} in {voice:?}");
    }
    for key in ["a_wave", "out_bus", "notes_plays"] {
        assert!(voice.binary_search(&key.to_owned()).is_err(), "{key} is built into the patch");
    }
    assert!(voice.iter().any(|k| k.starts_with(&format!("{}/", LayerId(0).0))), "the ADSR's numbers");
    let mixer = mixer_rule_keys(&scene, reg);
    for key in ["bus1_fader", "bus3_send2", "master_output", &layered_key(delay, "time_ms")] {
        assert!(mixer.binary_search(&key.to_owned()).is_ok(), "{key} in {mixer:?}");
    }
    assert!(mixer.binary_search(&"bus1_on".to_owned()).is_err(), "switches are built into the patch");
    // A shape without sound has none.
    let ball = scene.order.iter().copied().find(|id| scene.things[id].kind == "balls").unwrap();
    assert!(voice_rule_keys(&scene, &resolved, ball).is_empty());
}

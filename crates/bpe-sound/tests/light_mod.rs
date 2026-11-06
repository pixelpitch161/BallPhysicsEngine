//! The Light modulator (lighting part 7): how much light falls on the balls, heard by the
//! engine and read the same by Follow sound.

use bpe_core::{LayerId, Scene, Settings, SlotId, SoundSource, ThingId, Value, layered_key};
use bpe_kit::sound::{LIGHT_MOD, add_modulator, starter_voice};
use bpe_kit::things::light::{LightSettings, LightType, add_light};
use bpe_kit::{Kit, new_scene, resolve_scene};
use bpe_runtime::Playback;
use bpe_sound::ControlSampler;

fn shape_of(scene: &Scene) -> ThingId {
    scene.order.iter().copied().find(|id| scene.things[id].kind == "shape").unwrap()
}

/// The new scene with the starter sound on its shape, a lamp of `strength` over the middle and a
/// Light modulator (no smoothing). With `lit` false the scene has no Lighting card.
fn lit_sound_scene(kit: &Kit, strength: f32, lit: bool) -> (Scene, ThingId, LayerId) {
    let mut scene = new_scene(&kit.registry);
    bpe_kit::ensure_mixer(&mut scene, &kit.registry);
    let shape = shape_of(&scene);
    scene.things.get_mut(&shape).unwrap().slots.insert(SlotId::Sound, starter_voice(&kit.registry));
    let light = add_light(&mut scene, &kit.registry, LightType::Point, ThingId::ROOT).unwrap();
    let mut s = LightSettings::of_kind(LightType::Point);
    s.position = glam::Vec2::new(0.0, -150.0);
    s.reach = 4000.0;
    s.strength = strength;
    scene.things.get_mut(&light).unwrap().params = s.to_params();
    if !lit {
        scene.things.get_mut(&ThingId::ROOT).unwrap().slots.remove(&SlotId::Lighting);
    }
    let layer = add_modulator(&mut scene, &kit.registry, shape, LIGHT_MOD).unwrap();
    let card = scene.things.get_mut(&shape).unwrap().slots.get_mut(&SlotId::Sound).unwrap();
    card.params.set(&layered_key(layer, "smooth"), Value::Number(0.0));
    card.params.set(&layered_key(layer, "dark"), Value::Number(0.0));
    card.params.set(&layered_key(layer, "bright"), Value::Number(1.0));
    (scene, shape, layer)
}

/// The Light modulator's value in the engine after `seconds`.
fn heard_after(kit: &Kit, scene: &Scene, shape: ThingId, layer: LayerId, seconds: f64) -> f32 {
    let resolved = resolve_scene(scene, &kit.registry);
    let mut engine = bpe_sound::Engine::new(scene, &resolved, kit);
    let meters = engine.meters();
    let mut out = [0.0f32; bpe_sound::BLOCK * 2];
    for _ in 0..(seconds * 48_000.0 / bpe_sound::BLOCK as f64) as usize {
        engine.render(&mut out);
    }
    meters.mod_value(shape.0, layer).expect("the Light modulator runs")
}

#[test]
fn a_light_modulator_hears_more_under_a_brighter_lamp() {
    let kit = bpe_sound::kit();
    let (bright, shape, layer) = lit_sound_scene(&kit, 3.0, true);
    let (dim, _, _) = lit_sound_scene(&kit, 0.3, true);
    let (unlit, _, _) = lit_sound_scene(&kit, 3.0, false);
    let a = heard_after(&kit, &bright, shape, layer, 1.0);
    let b = heard_after(&kit, &dim, shape, layer, 1.0);
    let c = heard_after(&kit, &unlit, shape, layer, 1.0);
    assert!(a > 0.5, "under a bright lamp: {a}");
    assert!(b < a - 0.2, "under a dim lamp {b}, a bright one {a}");
    assert_eq!(c, 0.0, "no Lighting card: nothing to hear");
}

#[test]
fn the_engine_and_follow_sound_hear_the_same_light() {
    let kit = bpe_sound::kit();
    let (scene, shape, layer) = lit_sound_scene(&kit, 1.5, true);
    let resolved = resolve_scene(&scene, &kit.registry);
    let sampler = ControlSampler::new(&scene, &resolved);
    let mut engine = bpe_sound::Engine::new(&scene, &resolved, &kit);
    let meters = engine.meters();
    let mut p = Playback::new(&scene, &resolved, kit.sims.clone());
    let mut out = [0.0f32; bpe_sound::BLOCK * 2];
    let mut compared = 0;
    for block in 0..(3 * 48_000 / bpe_sound::BLOCK) {
        engine.render(&mut out);
        if block % 40 != 0 {
            continue;
        }
        let t = engine.voice_time();
        p.seek(t);
        let Some(heard) = meters.mod_value(shape.0, layer) else { continue };
        let ours = sampler.value(p.world(), p.log(), shape, &SoundSource::Modulator(layer), t);
        assert!((heard - ours).abs() < 0.05, "at {t}: the engine has {heard}, Follow sound {ours}");
        compared += 1;
    }
    assert!(compared > 20, "{compared}");
}

#[test]
fn a_light_modulator_follows_the_lights_in_the_sound_fingerprint() {
    let kit = bpe_sound::kit();
    let (scene, _, _) = lit_sound_scene(&kit, 1.5, true);
    let (brighter, _, _) = lit_sound_scene(&kit, 2.5, true);
    assert_ne!(
        bpe_sound::control::heard_fingerprint(&scene),
        bpe_sound::control::heard_fingerprint(&brighter),
        "Follow sound hears the lamp through the Light modulator"
    );
}

//! Particles part 7: a Particles thing's Sound card plays its particles' moments, on their
//! samples, the same every run and after a seek, and a scene without it stays silent.

use bpe_core::{Registry, Scene, Settings, SlotId, ThingId};
use bpe_kit::sound::{VoiceSettings, starter_voice};
use bpe_kit::things::particles::looks::{Look, add_particles};
use bpe_kit::things::particles::meets::{AtShapes, MeetsSettings};
use bpe_kit::things::particles::{BornFrom, Emit, Heading, ParticlesSettings};
use bpe_kit::{new_scene, resolve_scene};
use bpe_sound::{BLOCK, Engine};

/// The new scene with its ball switched off and a Particles thing bursting 60 particles from the
/// middle at the start, bouncing round the circle; given the starter sound when `sound`.
fn scene_of(reg: &Registry, sound: bool) -> (Scene, ThingId) {
    let mut scene = new_scene(reg);
    let ball = scene.order.iter().copied().find(|id| scene.things[id].kind == "balls").unwrap();
    scene.things.get_mut(&ball).unwrap().enabled = false;
    let id = add_particles(&mut scene, reg, Look::Empty, ThingId::ROOT).unwrap();
    let source = ParticlesSettings {
        from: BornFrom::APoint,
        emit: Emit::OnceAtTheStart,
        count: 60,
        speed: 900.0,
        heading: Heading::AllRound,
        life: 20.0,
        ..ParticlesSettings::default()
    };
    let t = scene.things.get_mut(&id).unwrap();
    t.params = source.to_params();
    let meets = MeetsSettings { at_shapes: AtShapes::Bounce, bounciness: 1.0, friction: 0.0, ..Default::default() };
    t.slots.get_mut(&SlotId::Bounce).unwrap().params = meets.to_params();
    if sound {
        let mut card = starter_voice(reg);
        let mut v = VoiceSettings::from_params(&card.params);
        v.moments_most = 40.0;
        for (k, value) in v.to_params().iter() {
            card.params.set(k, value.clone());
        }
        t.slots.insert(SlotId::Sound, card);
    }
    (scene, id)
}

/// `seconds` of the engine's sound.
fn render(engine: &mut Engine, seconds: f64) -> Vec<f32> {
    let mut out = Vec::new();
    let mut block = [0.0f32; BLOCK * 2];
    for _ in 0..((seconds * 48_000.0) as usize).div_ceil(BLOCK) {
        engine.render(&mut block);
        out.extend_from_slice(&block);
    }
    out
}

#[test]
fn particles_play_their_moments_and_are_silent_without_a_sound_card() {
    let kit = bpe_sound::kit();
    let (quiet, _) = scene_of(&kit.registry, false);
    let resolved = resolve_scene(&quiet, &kit.registry);
    let mut engine = Engine::new(&quiet, &resolved, &kit);
    assert!(render(&mut engine, 2.0).iter().all(|s| *s == 0.0), "no sound card, no sound");

    let (scene, id) = scene_of(&kit.registry, true);
    let resolved = resolve_scene(&scene, &kit.registry);
    let mut engine = Engine::new(&scene, &resolved, &kit);
    engine.record_notes(true);
    let first = render(&mut engine, 2.0);
    let spans = engine.note_log().spans(96_000);
    // 60 particles reach the circle (about 400 px away at 900 px/s) after about 0.43 s.
    assert!(!spans.is_empty());
    assert!(spans.iter().all(|s| s.0 == id), "the Particles thing plays them");
    assert!(spans.iter().all(|s| s.3 > 19_200), "not before they reach the circle: {:?}", spans[0]);
    assert!(spans.len() <= 1 + 4 + 40 * 2, "at most Most a second: {}", spans.len());
    assert!(first.iter().any(|s| s.abs() > 0.01));

    let mut again = Engine::new(&scene, &resolved, &kit);
    assert_eq!(render(&mut again, 2.0), first, "the same every run");

    let mut seeked = Engine::new(&scene, &resolved, &kit);
    seeked.seek(1.0);
    let after = render(&mut seeked, 1.0);
    let straight = &first[96_000..];
    let close = after.iter().zip(straight).all(|(a, b)| (a - b).abs() < 1e-3);
    assert!(close, "a seek hears the same notes");
}

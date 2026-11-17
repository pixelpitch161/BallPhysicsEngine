//! Liquid part 5: a Liquid thing's Sound card plays its splashes, a ball's note is muffled under
//! the liquid, Pitch from Colour plays the liquid's colour, and the Liquid modulator hears it.

use std::sync::{Arc, OnceLock};

use bpe_core::{LayerId, Scene, Settings, SlotId, ThingId, Value, layered_key};
use bpe_gfx::swarm::{SwarmGpu, use_on_this_thread};
use bpe_kit::scenes::liquid_scenes;
use bpe_kit::sound::{LIQUID_MOD, LiquidPreset, PitchFrom, VoiceSettings, add_modulator, liquid_sound, starter_voice};
use bpe_kit::things::liquid::KIND;
use bpe_kit::things::liquid::cards::MeetsSettings;
use bpe_kit::{Kit, resolve_scene};
use bpe_sound::{BLOCK, Engine};

fn gpu() {
    static GPU: OnceLock<Arc<SwarmGpu>> = OnceLock::new();
    let gpu = GPU.get_or_init(|| {
        let (d, q) = bpe_gfx::test_device().expect("a graphics device (install mesa-vulkan-drivers)");
        SwarmGpu::for_device(&d, &q)
    });
    use_on_this_thread(Some(gpu.clone()));
}

fn water(kit: &Kit) -> (Scene, ThingId, ThingId) {
    let scene = liquid_scenes(&kit.registry).into_iter().find(|(n, _)| n == "Liquid water").unwrap().1;
    let liquid = scene.order.iter().copied().find(|t| scene.things[t].kind == KIND).unwrap();
    let ball = scene.things.values().find(|t| t.kind == "balls").unwrap().id;
    (scene, liquid, ball)
}

/// The water with its ball heavy enough to sink to the bottom of the ring.
fn sinking(kit: &Kit) -> (Scene, ThingId, ThingId) {
    let (mut scene, liquid, ball) = water(kit);
    let card = scene.card_mut(liquid, SlotId::Bounce).unwrap();
    let mut m = MeetsSettings::from_params(&card.params);
    m.ball_weight = 3.0;
    card.params = m.to_params();
    (scene, liquid, ball)
}

fn voice(card: &mut bpe_core::Slot, edit: impl FnOnce(&mut VoiceSettings)) {
    let mut v = VoiceSettings::from_params(&card.params);
    edit(&mut v);
    for (k, value) in v.to_params().iter() {
        card.params.set(k, value.clone());
    }
}

fn render(kit: &Kit, scene: &Scene, seconds: f64) -> Vec<f32> {
    let resolved = resolve_scene(scene, &kit.registry);
    let mut engine = Engine::new(scene, &resolved, kit);
    let mut out = Vec::new();
    let mut block = [0.0f32; BLOCK * 2];
    for _ in 0..((seconds * 48_000.0) as usize).div_ceil(BLOCK) {
        engine.render(&mut block);
        out.extend_from_slice(&block);
    }
    out
}

fn energy(x: &[f32]) -> f32 {
    x.iter().map(|s| s * s).sum()
}

/// The energy of the differences between samples: much more for a bright sound than a dull one.
fn brightness(x: &[f32]) -> f32 {
    let left: Vec<f32> = x.iter().step_by(2).copied().collect();
    energy(&left.windows(2).map(|w| w[1] - w[0]).collect::<Vec<_>>()) / energy(&left).max(1e-12)
}

#[test]
fn a_liquid_plays_its_splash_and_is_silent_without_its_sound() {
    gpu();
    let kit = bpe_sound::kit();
    let (mut scene, liquid, _) = water(&kit);
    assert_eq!(energy(&render(&kit, &scene, 3.0)), 0.0, "no sound card");
    let mut card = liquid_sound(&kit.registry, LiquidPreset::Splash);
    scene.things.get_mut(&liquid).unwrap().slots.insert(SlotId::Sound, card.clone());
    let splash = render(&kit, &scene, 3.0);
    assert!(energy(&splash) > 1e-3, "the ball splashing in plays: {}", energy(&splash));
    assert!(splash.iter().all(|s| s.is_finite()));
    voice(&mut card, |v| v.moments_splash = false);
    scene.things.get_mut(&liquid).unwrap().slots.insert(SlotId::Sound, card);
    assert_eq!(energy(&render(&kit, &scene, 3.0)), 0.0, "splash switched off");
}

#[test]
fn a_ball_under_the_liquid_sounds_muffled() {
    gpu();
    let kit = bpe_sound::kit();
    // The bubbles of the sinking ball play under the water (its top goes under after about
    // 2.5 s; before then it is at the surface and plays as it is).
    let (mut scene, liquid, _) = sinking(&kit);
    let mut card = liquid_sound(&kit.registry, LiquidPreset::Bubbles);
    voice(&mut card, |v| v.a.wave = bpe_kit::sound::Wave::Saw);
    scene.things.get_mut(&liquid).unwrap().slots.insert(SlotId::Sound, card.clone());
    let deep = (2.6 * 48_000.0) as usize * 2;
    let open = render(&kit, &scene, 4.0).split_off(deep);
    voice(&mut card, |v| {
        v.play_under = 100.0;
        v.play_deep_at = 20.0;
    });
    scene.things.get_mut(&liquid).unwrap().slots.insert(SlotId::Sound, card);
    let muffled = render(&kit, &scene, 4.0).split_off(deep);
    let (a, b) = (brightness(&open), brightness(&muffled));
    eprintln!("open {a}, muffled {b}; energy {} and {}", energy(&open), energy(&muffled));
    assert!(energy(&open) > 1e-3 && energy(&muffled) > 0.0, "the bubbles under water play");
    assert!(b < a * 0.5, "muffled {b} against open {a}");
}

#[test]
fn pitch_from_colour_plays_a_note_of_the_liquids_colour() {
    gpu();
    let kit = bpe_sound::kit();
    let (mut scene, _, ball) = sinking(&kit);
    let mut card = starter_voice(&kit.registry);
    voice(&mut card, |v| v.a.wave = bpe_kit::sound::Wave::Sine);
    scene.things.get_mut(&ball).unwrap().slots.insert(SlotId::Sound, card.clone());
    let written = render(&kit, &scene, 6.0);
    voice(&mut card, |v| {
        v.play_pitch_from = PitchFrom::Colour;
        v.play_pitch_range = 24.0;
    });
    scene.things.get_mut(&ball).unwrap().slots.insert(SlotId::Sound, card);
    let coloured = render(&kit, &scene, 6.0);
    assert!(energy(&written) > 1e-3 && energy(&coloured) > 1e-3);
    // The water's blue moves the note up: a higher sine is brighter.
    let (a, b) = (brightness(&written), brightness(&coloured));
    eprintln!("written {a}, coloured {b}");
    assert!(b > a * 1.2, "coloured {b} against written {a}");
}

#[test]
fn a_liquid_modulator_hears_how_high_the_liquid_stands() {
    gpu();
    let kit = bpe_sound::kit();
    let (mut scene, liquid, ball) = water(&kit);
    scene.things.get_mut(&ball).unwrap().slots.insert(SlotId::Sound, starter_voice(&kit.registry));
    let layer: LayerId = add_modulator(&mut scene, &kit.registry, ball, LIQUID_MOD).unwrap();
    let card = scene.card_mut(ball, SlotId::Sound).unwrap();
    card.params.set(&layered_key(layer, "reads"), Value::Choice("height".into()));
    card.params.set(&layered_key(layer, "smooth"), Value::Number(0.0));
    assert!(bpe_kit::things::liquid::scene_hears_liquid(&scene));
    let resolved = resolve_scene(&scene, &kit.registry);
    let mut engine = Engine::new(&scene, &resolved, &kit);
    let meters = engine.meters();
    let mut block = [0.0f32; BLOCK * 2];
    for _ in 0..(48_000 / BLOCK) {
        engine.render(&mut block);
    }
    let level = meters.mod_value(ball.0, layer).expect("the Liquid modulator runs");
    eprintln!("level {level}");
    assert!(level > 0.1 && level < 0.6, "the water fills the lower part of the ring: {level}");
    let _ = liquid;
}

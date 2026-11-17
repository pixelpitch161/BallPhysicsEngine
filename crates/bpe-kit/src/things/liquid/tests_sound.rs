//! Liquid part 5 on the software graphics card: a Liquid thing's sound keeps it with the physics
//! (without moving the balls when it doesn't push them), its pours and bubbles reach the log when
//! its sound asks, and the sound reads how wet each ball is and what each liquid is doing.

use std::sync::{Arc, OnceLock};

use bpe_core::{Scene, Settings, SlotId, ThingId, resolve};
use bpe_gfx::swarm::{SwarmGpu, use_on_this_thread};
use bpe_runtime::{EventKind, Log, Party, Playback, World, main_part};

use super::KIND;
use super::cards::{MEETS, MeetsSettings};
use super::feel::SPLASH;
use super::sound::{BUBBLE, POURING, scene_hears_liquid, stats_of, wet_of};
use crate::scenes::liquid_scenes;
use crate::sound::{LiquidPreset, VoiceSettings, liquid_sound, starter_voice};
use crate::{kit, registry};

fn gpu() {
    static GPU: OnceLock<Arc<SwarmGpu>> = OnceLock::new();
    let gpu = GPU.get_or_init(|| {
        let (d, q) = bpe_gfx::test_device().expect("a graphics device (install mesa-vulkan-drivers)");
        SwarmGpu::for_device(&d, &q)
    });
    use_on_this_thread(Some(gpu.clone()));
}

fn scene_named(name: &str) -> Scene {
    liquid_scenes(&registry()).into_iter().find(|(n, _)| n == name).map(|(_, s)| s).expect("a test scene")
}

fn liquid_of(scene: &Scene) -> ThingId {
    scene.order.iter().copied().find(|t| scene.things[t].kind == KIND).expect("a liquid")
}

fn ball_of(scene: &Scene) -> ThingId {
    scene.things.values().find(|t| t.kind == "balls").map(|t| t.id).expect("a ball")
}

fn meets(scene: &mut Scene, id: ThingId, edit: impl FnOnce(&mut MeetsSettings)) {
    let card = scene.card_mut(id, SlotId::Bounce).expect("a Meets card");
    assert_eq!(card.kind, MEETS);
    let mut m = MeetsSettings::from_params(&card.params);
    edit(&mut m);
    card.params = m.to_params();
}

fn sound(scene: &mut Scene, id: ThingId, preset: LiquidPreset) {
    let card = liquid_sound(&registry(), preset);
    scene.things.get_mut(&id).expect("the liquid").slots.insert(SlotId::Sound, card);
}

fn run(scene: &Scene, time: f64) -> (World, Log) {
    let kit = kit();
    let resolved = resolve(scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    let mut world = World::build(scene, &resolved, &kit.sims);
    let mut log = Log::default();
    world.advance_to(time, &kit.sims, &mut log);
    (world, log)
}

fn liquid_items(log: &Log, item: u32) -> usize {
    log.events().iter().filter(|e| e.kind == EventKind::Liquid && e.b.is_some_and(|b| b.item == item)).count()
}

#[test]
fn a_sound_hears_a_liquid_only_when_it_asks_to() {
    let reg = registry();
    let mut scene = scene_named("Liquid water");
    assert!(!scene_hears_liquid(&scene), "no sound");
    let (id, ball) = (liquid_of(&scene), ball_of(&scene));
    let mut voice = starter_voice(&reg);
    scene.things.get_mut(&ball).unwrap().slots.insert(SlotId::Sound, voice.clone());
    assert!(!scene_hears_liquid(&scene), "a ball's sound that doesn't read the liquid");
    let mut v = VoiceSettings::from_params(&voice.params);
    v.play_under = 0.5;
    voice.params = v.to_params();
    scene.things.get_mut(&ball).unwrap().slots.insert(SlotId::Sound, voice);
    assert!(scene_hears_liquid(&scene), "muffled under it");
    scene.things.get_mut(&ball).unwrap().slots.remove(&SlotId::Sound);
    sound(&mut scene, id, LiquidPreset::Splash);
    assert!(scene_hears_liquid(&scene), "its own sound");
}

#[test]
fn a_sounding_liquid_that_doesnt_push_steps_with_the_physics_and_leaves_the_balls_alone() {
    gpu();
    let kit = kit();
    let mut scene = scene_named("Liquid water");
    let (id, ball) = (liquid_of(&scene), ball_of(&scene));
    meets(&mut scene, id, |m| m.pushes_balls = false);
    let resolved = resolve(&scene, &kit.registry);
    let mut quiet = Playback::new(&scene, &main_part(&scene, &resolved, &kit.sims), kit.sims.clone());
    quiet.seek(3.0);
    assert_eq!(liquid_items(quiet.log(), SPLASH), 0, "beside the physics it logs nothing");
    sound(&mut scene, id, LiquidPreset::Splash);
    let resolved = resolve(&scene, &kit.registry);
    let main = main_part(&scene, &resolved, &kit.sims);
    assert!(main.things.contains_key(&id), "the sound keeps it in the main world");
    let mut heard = Playback::new(&scene, &main, kit.sims.clone());
    heard.seek(3.0);
    assert!(liquid_items(heard.log(), SPLASH) > 0, "the splash reaches the log");
    assert_eq!(heard.world().bodies[&ball], quiet.world().bodies[&ball], "the ball moves as without it");
}

#[test]
fn pours_log_while_they_pour_when_the_sound_asks() {
    gpu();
    let mut scene = scene_named("Liquid paint mixing");
    let id = liquid_of(&scene);
    let (_, log) = run(&scene, 1.0);
    assert_eq!(liquid_items(&log, POURING), 0, "no sound, no pours logged");
    sound(&mut scene, id, LiquidPreset::Splash);
    let (_, log) = run(&scene, 1.0);
    assert_eq!(liquid_items(&log, POURING), 0, "a splash doesn't ask for pours");
    sound(&mut scene, id, LiquidPreset::Pour);
    let (_, log) = run(&scene, 1.0);
    let pours = liquid_items(&log, POURING);
    eprintln!("{pours} pours in a second");
    assert!((10..=31).contains(&pours), "about 30 a second: {pours}");
    let e = log.events().iter().find(|e| e.b.is_some_and(|b| b.item == POURING)).unwrap();
    assert_eq!((e.a.thing, e.b.unwrap().thing), (id, id));
    assert!(e.speed > 0.0);
}

#[test]
fn a_ball_sinking_under_the_water_is_wet_and_gives_off_bubbles() {
    gpu();
    let mut scene = scene_named("Liquid water");
    let (id, ball) = (liquid_of(&scene), ball_of(&scene));
    meets(&mut scene, id, |m| m.ball_weight = 3.0);
    sound(&mut scene, id, LiquidPreset::Bubbles);
    let (world, log) = run(&scene, 3.0);
    let bubbles = liquid_items(&log, BUBBLE);
    eprintln!("{bubbles} bubbles");
    assert!(bubbles > 0, "it gave off bubbles");
    let wet = wet_of(&world, Party { thing: ball, item: 0 }).expect("it is in the water");
    eprintln!("{wet:?}");
    assert!(wet.depth > 10.0, "under the surface: {}", wet.depth);
    assert_eq!((wet.n, wet.liquids[0].0), (1, id));
    assert!((wet.liquids[0].1 - 1.0).abs() < 1e-3);
    let stat = stats_of(&world, |t| t == id).expect("the water's stats");
    eprintln!("{stat:?}");
    assert!(stat.drops > 1000);
    assert!(stat.height > 0.05 && stat.height < 0.5, "it fills the lower part: {}", stat.height);
    assert!(stat.mixed > 0.95, "one colour all through: {}", stat.mixed);
    let most = super::state_of(&world, id).expect("its state").kept;
    assert!((stat.amount - stat.drops as f32 / most as f32).abs() < 1e-4, "drops over its Most drops");
    assert!(stats_of(&world, |_| false).is_some_and(|s| s.drops == 0), "picks nothing");
}

#[test]
fn paint_poured_in_reads_less_mixed_than_one_colour() {
    gpu();
    let mut scene = scene_named("Liquid paint mixing");
    let id = liquid_of(&scene);
    sound(&mut scene, id, LiquidPreset::Splash);
    let (world, _) = run(&scene, 2.0);
    let stat = stats_of(&world, |t| t == id).expect("the paint's stats");
    eprintln!("{stat:?}");
    assert!(stat.mixed < 0.98, "yellow with blue on it isn't one colour yet: {}", stat.mixed);
    assert!(stat.speed > 0.0);
}

#[test]
fn a_sounding_liquid_plays_alike_after_a_seek() {
    gpu();
    let kit = kit();
    let mut scene = scene_named("Liquid water");
    let id = liquid_of(&scene);
    meets(&mut scene, id, |m| m.ball_weight = 3.0);
    sound(&mut scene, id, LiquidPreset::Bubbles);
    let resolved = resolve(&scene, &kit.registry);
    let mut a = Playback::new(&scene, &resolved, kit.sims.clone());
    a.seek(3.0);
    let mut b = Playback::new(&scene, &resolved, kit.sims.clone());
    for t in [2.0, 0.5, 3.0] {
        b.seek(t);
    }
    assert_eq!(a.world().bodies, b.world().bodies);
    assert_eq!(a.log().hash(), b.log().hash());
}

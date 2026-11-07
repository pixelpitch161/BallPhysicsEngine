//! The modular Ball Studio's plugins. Each thing and module kind lives in its own folder with
//! its settings, its simulation and its drawing side by side.

#[cfg(test)]
mod layer_tests;
pub mod modules;
pub mod paint;
pub mod rng;
pub mod rules;
#[cfg(any(test, feature = "test-scenes"))]
pub mod scenes;
pub mod sensing;
pub mod sound;
pub mod starters;
#[cfg(test)]
pub mod testing;
pub mod things;

use std::sync::Arc;

use bpe_core::{Registry, Resolved, Scene, ThingId};
use bpe_gfx::Draws;
use bpe_runtime::Sims;

pub use modules::bounce::sim::{BasicBounceSim, BounceState};
pub use modules::bounce::{BasicBounce, BasicBounceKind, WallTouch};
pub use modules::effects::draw::EffectsDraw;
pub use modules::effects::{EffectsCard, OwnEffect, PictureEffect};
pub use modules::look::{BallShape, BasicLook, BasicLookKind, Style};
pub use modules::motion::sim::{BasicMotionSim, MotionState};
pub use modules::motion::{BasicMotion, BasicMotionKind};
pub use starters::{add_thing, fill_slot};
pub use things::balls::draw::BallsDraw;
pub use things::balls::sim::{BallsSim, BallsState};
pub use things::balls::{BallsKind, BallsSettings, GroupShape, Launch, Release, ReleaseOrder};
pub use things::camera::sim::{CameraSim, CameraState};
pub use things::camera::{CameraKind, CameraSettings};
pub use things::emitter::draw::EmitterDraw;
pub use things::emitter::sim::{EmitterSim, EmitterState};
pub use things::emitter::{EmitterKind, EmitterSettings};
pub use things::folder::{FolderKind, FolderSettings, FolderSim};
pub use things::scene::ROOT_CARDS;
pub use things::scene::draw::SceneDraw;
pub use things::scene::sim::SceneSim;
pub use things::scene::{SceneKind, SceneSettings, TimeBottom};
pub use things::shape::draw::ShapeDraw;
pub use things::shape::sim::{ShapeSim, ShapeState};
pub use things::shape::{BODY_HEADINGS, ShapeFill, ShapeKind, ShapeSettings, ShapeStart};

/// Every kind's simulation part, for the runtime.
pub fn sims() -> Sims {
    let mut s = Sims::default();
    s.add_thing(SceneSim);
    s.add_thing(ShapeSim);
    s.add_thing(BallsSim);
    s.add_thing(EmitterSim);
    s.add_thing(FolderSim);
    s.add_thing(CameraSim);
    s.add_thing(things::particles::sim::ParticlesSim);
    s.add_module(BasicMotionSim);
    s.add_module(BasicBounceSim);
    rules::add_sims(&mut s);
    s.add_health(modules::health::HealthCardSim);
    // Light sensing: worlds whose physics follows light get the probe (none do unless asked).
    s.light = Some(std::sync::Arc::new(sensing::KitLight));
    s
}

/// The kit's sims, made once for the process: what test flights step their stripped copies of
/// the world with (`things::emitter::test_fire`), since a thing's sim hooks aren't handed the
/// sims.
pub fn lone_sims() -> &'static Sims {
    static SIMS: std::sync::OnceLock<Sims> = std::sync::OnceLock::new();
    SIMS.get_or_init(sims)
}

/// Every kind's drawing part, for the renderer.
pub fn draws() -> Draws {
    let mut d = Draws::default();
    d.add(SceneDraw);
    d.add(ShapeDraw);
    d.add(BallsDraw);
    d.add(EmitterDraw);
    d.add(things::light::draw::LightDraw);
    d.add(things::camera::draw::CameraDraw);
    d.add(things::particles::draw::ParticlesDraw);
    d.add_module(EffectsDraw);
    d
}

/// Everything the kit gives the editor and the exporter, built once.
pub struct Kit {
    pub registry: Registry,
    pub sims: Arc<Sims>,
    pub draws: Draws,
}

pub fn kit() -> Kit {
    Kit { registry: registry(), sims: Arc::new(sims()), draws: draws() }
}

/// Resolves a scene with the kit's registry: what every thing uses, and what couldn't be used.
pub fn resolve_scene(scene: &Scene, reg: &Registry) -> Resolved {
    bpe_core::resolve(scene, reg)
}

/// Every kind the kit has, for the core's resolve and the editor's lists.
pub fn registry() -> Registry {
    let mut r = Registry::new();
    r.add_thing(SceneKind);
    r.add_thing(ShapeKind);
    r.add_thing(BallsKind);
    r.add_thing(EmitterKind);
    r.add_thing(FolderKind);
    r.add_thing(CameraKind);
    r.add_thing(things::light::LightKind);
    r.add_thing(things::rig::RigKind);
    r.add_thing(things::particles::ParticlesKind);
    r.add_module(things::particles::forces::ForcesCard);
    r.add_module(things::particles::meets::MeetsCard);
    r.add_module(things::particles::look::ParticlesLookKind);
    for k in things::particles::forces::kinds().into_iter().chain(things::particles::look::kinds()) {
        r.add_layer(k);
    }
    r.add_module(BasicLookKind);
    r.add_module(modules::look::LayersLookKind);
    for k in modules::look::layers::kinds() {
        r.add_layer(k);
    }
    r.add_module(modules::look::ShapeLookKind);
    r.add_layer(modules::look::shape::ShapeBody);
    for k in modules::look::shape::kinds() {
        r.add_layer(k);
    }
    r.add_module(BasicMotionKind);
    r.add_module(modules::lighting::LightingKind);
    r.add_module(BasicBounceKind);
    r.add_module(modules::health::HealthKind);
    r.add_module(EffectsCard);
    for k in modules::effects::kinds() {
        r.add_layer(k);
    }
    r.add_layer(OwnEffect);
    sound::register(&mut r);
    rules::register(&mut r);
    // Format 8 masks open as Cut sub-layers.
    r.add_upgrade(9, modules::look::layers::masks_to_cuts);
    // Trail and Particles cards before format 10 open as layers of the Look.
    r.add_upgrade(10, modules::look::layers::cards_to_layers::cards_to_layers);
    // A scene's one Shader card opens as an Effects stack. Every format runs it: it does nothing
    // to a scene without an old card.
    r.add_upgrade(u32::MAX, modules::effects::upgrade::single_effect_to_stack);
    r
}

/// The test scenes' starting point: a hollow circle shape (the old arena's) and one pink ball, with the values the
/// settings had before the blank project (a dark blue background, a little bloom and glow, and
/// gravity on). Only tests build it; the program starts from [`new_studio_scene`].
#[cfg(any(test, feature = "test-scenes"))]
pub fn new_scene(reg: &Registry) -> Scene {
    use bpe_core::{Settings, SlotId};
    use glam::Vec2;
    let mut scene = Scene::empty(reg);
    scene.name = "New scene".into();
    let root = ThingId::ROOT;
    scenes::classic_scene_settings(&mut scene);
    let shape = add_thing(&mut scene, reg, "shape", root).expect("kit kinds are registered");
    scene.things.get_mut(&shape).expect("just added").params = ShapeSettings::circle(508.0, 3.0).to_params();
    let ball = add_thing(&mut scene, reg, "balls", root).expect("kit kinds are registered");
    scene.rename_thing(ball, "Ball");
    scenes::put_card(&mut scene, reg, ball, SlotId::Look, "look.basic");
    let look = scene.things.get_mut(&ball).and_then(|t| t.slots.get_mut(&SlotId::Look)).expect("a new ball's look");
    look.params = scenes::classic_ball_look().to_params();
    scenes::classic_cards(&mut scene);
    scene.things.get_mut(&ball).expect("just added").transform.position = Vec2::new(0.0, -200.0);
    scene
}

/// The scene's camera, if it has one.
pub fn camera_of(scene: &Scene) -> Option<ThingId> {
    scene.order.iter().copied().find(|id| scene.things.get(id).is_some_and(|t| t.kind == "camera"))
}

/// The scene's camera, added under Scene settings when it has none. It is always the newest
/// thing, so no other thing's id moves, and nothing else changes.
pub fn ensure_camera(scene: &mut Scene, reg: &Registry) -> ThingId {
    if let Some(id) = camera_of(scene) {
        return id;
    }
    scene.add_thing(reg, "camera", ThingId::ROOT).expect("kit kinds are registered")
}

/// The scene's mixer card, added to the scene settings when it has none (older files).
pub fn ensure_mixer(scene: &mut Scene, reg: &Registry) {
    if scene.card(ThingId::ROOT, bpe_core::SlotId::Mixer).is_none() {
        scene.add_slot(reg, ThingId::ROOT, bpe_core::SlotId::Mixer, "sound.mixer").expect("the kit has the mixer");
    }
}

/// The blank project the editor opens to: Scene settings (with its mixer) and a camera,
/// nothing else.
pub fn new_studio_scene(reg: &Registry) -> Scene {
    let mut scene = Scene::empty(reg);
    scene.name = "New scene".into();
    ensure_camera(&mut scene, reg);
    ensure_mixer(&mut scene, reg);
    scene
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Settings, SlotId, ThingId, Value, resolve};
    use bpe_runtime::{Log, World};

    #[test]
    fn the_blank_project_is_scene_settings_and_a_camera() {
        let reg = registry();
        let scene = new_studio_scene(&reg);
        let kinds: Vec<&str> = scene.order.iter().map(|id| scene.things[id].kind.as_str()).collect();
        assert_eq!(kinds, ["scene", "camera"]);
        let cards: Vec<(SlotId, &str)> =
            scene.things.values().flat_map(|t| t.slots.iter().map(|(s, c)| (*s, c.kind.as_str()))).collect();
        assert_eq!(cards, [(SlotId::Mixer, "sound.mixer")], "nothing has a card yet but the mixer");
        let s = SceneSettings::from_params(&resolve(&scene, &reg).things[&ThingId::ROOT].params);
        assert_eq!(s.background, paint::Paint::solid(bpe_core::Color::rgb(0x80, 0x80, 0x80)), "grey");
        assert_eq!(s.bloom, 0.0);
    }

    #[test]
    fn the_blank_project_has_a_mixer_card_with_a_minus_one_ceiling() {
        let reg = registry();
        let scene = new_studio_scene(&reg);
        let resolved = resolve(&scene, &reg);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let card = &resolved.things[&ThingId::ROOT].slots[&SlotId::Mixer];
        assert_eq!(card.kind, "sound.mixer");
        let m = sound::MixerSettings::from_params(&card.params);
        assert_eq!((m.master_ceiling, m.master_release, m.master_input, m.master_output), (-1.0, 60.0, 0.0, 0.0));
    }

    #[test]
    fn tempo_defaults_to_120_in_four_four() {
        let reg = registry();
        let scene = new_studio_scene(&reg);
        let s = SceneSettings::from_params(&resolve(&scene, &reg).things[&ThingId::ROOT].params);
        assert_eq!((s.tempo, s.time_top, s.time_bottom), (120.0, 4, things::scene::TimeBottom::Four));
        assert_eq!(things::scene::TimeBottom::Eight.beats(), 8);
        let defs = SceneSettings::defs();
        for key in ["tempo", "time_top", "time_bottom"] {
            let d = defs.iter().find(|d| d.key == key).unwrap();
            assert_eq!(d.group, "Sound", "{key}");
            assert!(!d.flags.keyframe, "{key} is not keyframed");
        }
    }

    #[test]
    fn tempo_does_not_restart_the_physics() {
        let kit = kit();
        let mut scene = new_studio_scene(&kit.registry);
        add_thing(&mut scene, &kit.registry, "balls", ThingId::ROOT).unwrap();
        let before = bpe_runtime::fingerprint(&scene, &resolve(&scene, &kit.registry), &kit.sims);
        let root = scene.things.get_mut(&ThingId::ROOT).unwrap();
        root.params.set("tempo", Value::Number(93.5));
        root.params.set("time_top", Value::Int(7));
        root.params.set("time_bottom", Value::Choice("8".into()));
        let resolved = resolve(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        assert_eq!(SceneSettings::from_params(&resolved.things[&ThingId::ROOT].params).tempo, 93.5);
        assert_eq!(bpe_runtime::fingerprint(&scene, &resolved, &kit.sims), before);
    }

    #[test]
    fn ensure_mixer_is_idempotent() {
        let reg = registry();
        let mut scene = bpe_core::Scene::empty(&reg);
        ensure_mixer(&mut scene, &reg);
        let once = scene.clone();
        ensure_mixer(&mut scene, &reg);
        assert_eq!(scene, once);
        assert!(scene.card(ThingId::ROOT, SlotId::Mixer).is_some());
    }

    #[test]
    fn a_shape_and_a_ball_added_to_the_blank_project_have_no_glow_or_gravity() {
        let reg = registry();
        let mut scene = new_studio_scene(&reg);
        let shape = add_thing(&mut scene, &reg, "shape", ThingId::ROOT).unwrap();
        let ball = add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
        let thing = scene.things.get_mut(&ball).unwrap();
        let mut still = BallsSettings::from_params(&thing.params);
        still.velocity = glam::Vec2::ZERO;
        thing.params = still.to_params();
        let resolved = resolve(&scene, &reg);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let shape_look = &scene.things[&shape].slots[&SlotId::Look];
        assert_eq!(shape_look.kind, "look.shape");
        assert_eq!(shape_look.layers.len(), 1, "the shape is its plain body, with no glow");
        let stack = modules::look::layers::gpu::Stack::of(&resolved.things[&ball].slots[&SlotId::Look]).unwrap();
        assert!(stack.drawn.is_empty(), "a new ball is its body alone, with no glow");
        let motion = BasicMotion::from_params(&resolved.things[&ball].slots[&SlotId::Motion].params);
        assert_eq!(motion.gravity_scale, 0.0);
        let mut world = World::build(&scene, &resolved, &sims());
        let start = world.bodies[&ball].position[0];
        world.advance_to(1.0, &sims(), &mut Log::default());
        assert_eq!(world.bodies[&ball].position[0], start, "a ball at rest stays put");
    }

    #[test]
    fn every_addable_thing_has_a_family() {
        let families = ["Bouncers", "Shapes", "Lights", "Particles"];
        let reg = registry();
        for kind in reg.things().filter(|k| !["scene", "folder", "camera"].contains(&k.id())) {
            assert!(families.contains(&kind.family()), "{} has family {}", kind.id(), kind.family());
        }
    }

    fn defs_of(reg: &Registry, kind: &str) -> &'static [bpe_core::SettingDef] {
        reg.module(kind).unwrap_or_else(|| panic!("no module {kind}")).settings()
    }

    fn group_of(reg: &Registry, kind: &str, key: &str) -> &'static str {
        defs_of(reg, kind).iter().find(|d| d.key == key).unwrap_or_else(|| panic!("{kind} has no {key}")).group
    }

    #[test]
    fn every_module_setting_has_a_known_section() {
        let reg = registry();
        for m in reg.modules() {
            for d in m.settings() {
                assert!(
                    !d.group.is_empty() && bpe_core::section_rank(d.group).is_some(),
                    "{} setting {} has group {:?}, which is not a section",
                    m.id(),
                    d.key,
                    d.group
                );
            }
        }
    }

    #[test]
    fn the_look_card_reads_in_section_order() {
        let reg = registry();
        let defs = defs_of(&reg, "look.basic");
        // A paint's variation rows sit inside the paint, so the inspector places them by rank.
        let mut seen: Vec<&str> = Vec::new();
        for d in defs.iter().filter(|d| bpe_core::section_rank(d.group) != bpe_core::section_rank("Variation")) {
            if !seen.contains(&d.group) {
                seen.push(d.group);
            }
        }
        let ranks: Vec<_> = seen.iter().map(|g| bpe_core::section_rank(g).unwrap()).collect();
        assert!(ranks.windows(2).all(|w| w[0] < w[1]), "out of order: {seen:?}");
        assert_eq!(seen, ["Shape", "Style", "Across the group", "Inside each one", "Light", "Motion", "Blending"]);
        for key in ["glow", "flash", "flash_size", "flash_seconds"] {
            assert_eq!(group_of(&reg, "look.basic", key), "Light", "{key}");
        }
        for key in ["turn", "spin"] {
            assert_eq!(group_of(&reg, "look.basic", key), "Motion", "{key}");
        }
        for key in ["ball_shape", "outline", "outline_colours", "outline_follow"] {
            assert_eq!(group_of(&reg, "look.basic", key), "Shape", "{key}");
        }
    }

    #[test]
    fn bounce_sections() {
        let reg = registry();
        let keys_in = |group: &str| -> Vec<&str> {
            let mut k: Vec<_> =
                defs_of(&reg, "bounce.basic").iter().filter(|d| d.group == group).map(|d| d.key).collect();
            k.sort_unstable();
            k
        };
        let sorted = |mut v: Vec<&'static str>| {
            v.sort_unstable();
            v
        };
        assert_eq!(
            keys_in("Bounce"),
            sorted(vec!["bounciness", "speed_gain", "no_speed_limit", "top_speed", "friction", "takes_over", "touch"])
        );
        assert_eq!(keys_in("Growing"), sorted(vec!["grow", "no_size_limit", "max_size", "pops_after"]));
        assert_eq!(keys_in("Other balls"), sorted(vec!["hit_balls", "balls_to_hit"]));
    }

    #[test]
    fn a_paint_s_form_is_its_colours_shape_not_a_setting_of_its_own() {
        let reg = registry();
        let defs = defs_of(&reg, "look.basic");
        assert!(defs.iter().all(|d| d.key != "shape"), "the old Gradient choice is gone");
        let colours = defs.iter().find(|d| d.key == "colours").unwrap();
        assert!(colours.flags.area.is_area(), "the colours take a form on their Shape tab");
        assert_eq!(defs.iter().find(|d| d.key == "repeat").unwrap().label, "Beyond the last colour");
    }

    #[test]
    fn motion_gravity_is_a_multiple() {
        let reg = registry();
        let d = defs_of(&reg, "motion.basic").iter().find(|d| d.key == "gravity_scale").unwrap();
        assert_eq!(d.label, "Gravity ×");
        assert_eq!(d.help, "How strongly the scene's gravity pulls these, as a multiple.");
    }

    #[test]
    fn a_launch_speed_slider_keeps_its_usual_span() {
        let reg = registry();
        let velocity = reg.thing("balls").unwrap().settings().iter().find(|d| d.key == "velocity").unwrap();
        assert_eq!((velocity.range, velocity.span), (None, Some((100.0, 900.0))));
    }

    #[test]
    fn every_kit_plugin_passes_the_contract() {
        assert_eq!(bpe_core::check_registry(&registry()), Vec::<String>::new());
    }

    #[test]
    fn kinds_fitting_limits_looks_and_motions_by_thing() {
        let reg = registry();
        let ids = |thing: &str, slot: SlotId| -> Vec<&'static str> {
            let t = reg.thing(thing).unwrap();
            reg.kinds_fitting(slot, t).map(|m| m.id()).collect()
        };
        assert_eq!(ids("balls", SlotId::Look), ["look.layers"]);
        assert_eq!(ids("shape", SlotId::Look), ["look.shape"]);
        assert!(ids("scene", SlotId::Look).is_empty());
        assert_eq!(ids("balls", SlotId::Motion), ["motion.basic"]);
        // Effects are the scene's; a ball's shaders are layers of its Look.
        assert_eq!(ids("scene", SlotId::Shader), ["effects"]);
        assert!(ids("balls", SlotId::Shader).is_empty() && ids("shape", SlotId::Shader).is_empty());
    }

    #[test]
    fn the_look_registry_has_a_look_for_balls_shapes_particles_and_the_old_one() {
        let reg = registry();
        let looks: Vec<&str> = reg.modules_for(SlotId::Look).map(|m| m.id()).collect();
        assert_eq!(looks, ["look.basic", "look.layers", "look.particles", "look.shape"]);
        for look in looks {
            assert_eq!(reg.module(look).unwrap().name(), "Look");
        }
    }

    #[test]
    fn a_ball_bigger_than_its_shape_is_pinned_and_logs_no_hits() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        scene.things.get_mut(&ThingId(1)).unwrap().params.set("size", Value::Number(20.0));
        scene.things.get_mut(&ThingId(2)).unwrap().params.set("radius", Value::Number(24.0));
        // It starts inside the shape (a ball that starts outside a hollow shape stays outside).
        scene.things.get_mut(&ThingId(2)).unwrap().transform.position = glam::Vec2::new(0.0, -5.0);
        let resolved = resolve(&scene, &kit.registry);
        let mut world = World::build(&scene, &resolved, &kit.sims);
        let mut log = Log::default();
        world.advance_to(1.0, &kit.sims, &mut log);
        assert!(log.is_empty(), "a pinned ball buzzed {} hits", log.len());
        let p = world.bodies[&ThingId(2)].position[0];
        assert!(p.is_finite() && p.length() < 1e-3, "{p:?}");
    }

    #[test]
    fn ensure_camera_appends_once() {
        let reg = registry();
        let mut scene = scenes::all(&reg).into_iter().next().unwrap().1;
        let before: Vec<ThingId> = scene.things.keys().copied().collect();
        let newest = ThingId(scene.next_thing);
        let a = ensure_camera(&mut scene, &reg);
        let b = ensure_camera(&mut scene, &reg);
        assert_eq!(a, b);
        assert_eq!(a, newest);
        assert_eq!(scene.things.values().filter(|t| t.kind == "camera").count(), 1);
        assert!(before.iter().all(|id| scene.things.contains_key(id)));
        assert_eq!(scene.add_thing(&reg, "camera", ThingId::ROOT), Err(bpe_core::EditError::OnlyOne("camera".into())));
    }

    #[test]
    fn old_file_loads_without_screen_or_camera() {
        let reg = registry();
        let scene = new_scene(&reg);
        let saved = bpe_core::save(&scene);
        assert!(!saved.contains("screen"), "{saved}");
        let loaded = bpe_core::load(&saved, &reg).unwrap();
        assert_eq!(camera_of(&loaded), None);
        let b = &loaded.things[&ThingId(2)];
        assert!(!b.sticks_to_screen(reg.thing("balls").unwrap()));
    }

    #[test]
    fn new_studio_scene_has_a_camera() {
        let reg = registry();
        let scene = new_studio_scene(&reg);
        assert!(camera_of(&scene).is_some());
    }
}

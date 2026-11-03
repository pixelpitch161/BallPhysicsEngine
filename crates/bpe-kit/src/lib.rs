//! The modular Ball Studio's plugins. Each thing and module kind lives in its own folder with
//! its settings, its simulation and its drawing side by side.

#[cfg(test)]
mod layer_tests;
pub mod modules;
pub mod paint;
pub mod random;
pub mod rng;
#[cfg(any(test, feature = "test-scenes"))]
pub mod scenes;
pub mod starters;
#[cfg(test)]
pub mod testing;
pub mod things;

use std::sync::Arc;

use bpe_core::{Registry, Resolved, Scene, ThingId};
use bpe_gfx::Draws;
use bpe_runtime::Sims;

pub use modules::bounce::sim::{BasicBounceSim, BounceState};
pub use modules::bounce::{BasicBounce, BasicBounceKind};
pub use modules::look::{BallShape, BasicLook, BasicLookKind, Style};
pub use modules::motion::sim::{BasicMotionSim, MotionState};
pub use modules::motion::{BasicMotion, BasicMotionKind};
pub use modules::particles::draw::BurstDraw;
pub use modules::particles::{BurstPreset, BurstSource, HitBurst, HitBurstKind};
pub use modules::shader::EffectKind;
pub use modules::shader::draw::EffectDraw;
pub use modules::trail::draw::StreakDraw;
pub use modules::trail::{StreakTrail, StreakTrailKind, TrailSource};
pub use starters::{add_thing, fill_slot, keep_followers, make_folder, move_thing};
pub use things::arena::draw::ArenaDraw;
pub use things::arena::sim::{ArenaSim, ArenaState};
pub use things::arena::{ArenaKind, ArenaSettings, ArenaShape};
pub use things::balls::draw::BallsDraw;
pub use things::balls::sim::{BallsSim, BallsState};
pub use things::balls::{Arrangement, BallsKind, BallsSettings};
pub use things::camera::sim::{CameraSim, CameraState};
pub use things::camera::{CameraKind, CameraSettings};
pub use things::folder::{FolderKind, FolderSettings, FolderSim};
pub use things::scene::ROOT_CARDS;
pub use things::scene::draw::SceneDraw;
pub use things::scene::sim::SceneSim;
pub use things::scene::{SceneKind, SceneSettings};

/// Every kind's simulation part, for the runtime.
pub fn sims() -> Sims {
    let mut s = Sims::default();
    s.add_thing(SceneSim);
    s.add_thing(ArenaSim);
    s.add_thing(BallsSim);
    s.add_thing(FolderSim);
    s.add_thing(CameraSim);
    s.add_module(BasicMotionSim);
    s.add_module(BasicBounceSim);
    s
}

/// Every kind's drawing part, for the renderer.
pub fn draws() -> Draws {
    let mut d = Draws::default();
    d.add(SceneDraw);
    d.add(ArenaDraw);
    d.add(BallsDraw);
    for b in &bpe_gfx::effects::BUILTINS {
        d.add_module(EffectDraw(b));
    }
    d.add_module(StreakDraw);
    d.add_module(BurstDraw);
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
    r.add_thing(ArenaKind);
    r.add_thing(BallsKind);
    r.add_thing(FolderKind);
    r.add_thing(CameraKind);
    r.add_module(BasicLookKind);
    r.add_module(StreakTrailKind);
    r.add_module(HitBurstKind);
    r.add_module(BasicMotionKind);
    r.add_module(BasicBounceKind);
    for k in modules::shader::kinds() {
        r.add_module(k);
    }
    r
}

/// The test scenes' starting point: a circle arena and one pink ball, with the values the
/// settings had before the blank project (a dark blue background, a little bloom and glow, and
/// gravity on). Only tests build it; the program starts from [`new_studio_scene`].
#[cfg(any(test, feature = "test-scenes"))]
pub fn new_scene(reg: &Registry) -> Scene {
    use bpe_core::{Color, Settings, SlotId, SlotLink};
    use glam::Vec2;
    use paint::Paint;
    let mut scene = Scene::empty(reg);
    scene.name = "New scene".into();
    let root = ThingId::ROOT;
    scenes::classic_scene_settings(&mut scene);
    add_thing(&mut scene, reg, "arena", root).expect("kit kinds are registered");
    let ball = add_thing(&mut scene, reg, "balls", root).expect("kit kinds are registered");
    scene.rename_thing(ball, "Ball");
    let SlotLink::Linked { module: look, .. } = scene.things[&ball].slots[&SlotId::Look] else {
        unreachable!("a new ball owns its look")
    };
    scene.library.modules.get_mut(&look).expect("just added").params =
        BasicLook { fill: Paint::solid(Color::rgb(0xff, 0x5c, 0xa8)), ..scenes::classic_look() }.to_params();
    scenes::classic_modules(&mut scene);
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

/// The blank project the editor opens to: Scene settings and a camera, nothing else.
pub fn new_studio_scene(reg: &Registry) -> Scene {
    let mut scene = Scene::empty(reg);
    scene.name = "New scene".into();
    ensure_camera(&mut scene, reg);
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
        assert!(scene.library.modules.is_empty(), "{:?}", scene.library.modules);
        let s = SceneSettings::from_params(&resolve(&scene, &reg).things[&ThingId::ROOT].params);
        assert_eq!(s.background, paint::Paint::solid(bpe_core::Color::rgb(0x80, 0x80, 0x80)), "grey");
        assert_eq!(s.bloom, 0.0);
    }

    #[test]
    fn an_arena_and_a_ball_added_to_the_blank_project_have_no_glow_or_gravity() {
        let reg = registry();
        let mut scene = new_studio_scene(&reg);
        let arena = add_thing(&mut scene, &reg, "arena", ThingId::ROOT).unwrap();
        let ball = add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
        let thing = scene.things.get_mut(&ball).unwrap();
        let mut still = BallsSettings::from_params(&thing.params);
        still.velocity = glam::Vec2::ZERO;
        thing.params = still.to_params();
        let resolved = resolve(&scene, &reg);
        for id in [arena, ball] {
            let look = BasicLook::from_params(&resolved.things[&id].slots[&SlotId::Look].params);
            assert_eq!(look.glow, 0.0, "{}", scene.things[&id].name);
        }
        let motion = BasicMotion::from_params(&resolved.things[&ball].slots[&SlotId::Motion].params);
        assert_eq!(motion.gravity_scale, 0.0);
        let mut world = World::build(&scene, &resolved, &sims());
        let start = world.bodies[&ball].position[0];
        world.advance_to(1.0, &sims(), &mut Log::default());
        assert_eq!(world.bodies[&ball].position[0], start, "a ball at rest stays put");
    }

    #[test]
    fn every_addable_thing_has_a_family() {
        let families = ["Bouncers", "Walls"];
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
        assert_eq!(keys_in("Basics"), sorted(vec!["bounciness", "speed_gain", "top_speed", "friction", "takes_over"]));
        assert_eq!(keys_in("Growing"), sorted(vec!["grow", "max_size", "pops_after"]));
        assert_eq!(keys_in("Other balls"), sorted(vec!["hit_balls", "balls_to_hit"]));
    }

    #[test]
    fn the_place_choice_reads_straight_or_round() {
        let reg = registry();
        let defs = defs_of(&reg, "look.basic");
        let shape = defs.iter().find(|d| d.key == "shape").unwrap();
        assert_eq!(shape.label, "Gradient");
        assert_eq!(shape.ty, bpe_core::ValueType::Choice(&["straight", "round"]));
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
    fn dice_ranges_are_sensible() {
        let reg = registry();
        let def = |kind: &str, key: &str| {
            let defs = reg.thing(kind).map(|k| k.settings()).or_else(|| reg.module(kind).map(|k| k.settings()));
            defs.unwrap_or_else(|| panic!("no kind {kind}")).iter().find(|d| d.key == key).unwrap().clone()
        };
        assert_eq!(def("balls", "radius").dice_range(), Some((6.0, 60.0)));
        assert_eq!(def("balls", "count").dice_range(), Some((3.0, 200.0)));
        assert_eq!(def("balls", "columns").dice_range(), Some((1.0, 30.0)));
        assert_eq!(def("balls", "rows").dice_range(), Some((1.0, 30.0)));
        assert_eq!(def("balls", "spacing").dice_range(), Some((8.0, 60.0)));
        assert_eq!(def("balls", "ring_radius").dice_range(), Some((60.0, 400.0)));
        assert_eq!(def("balls", "velocity").dice_range(), Some((100.0, 900.0)));
        assert_eq!(def("arena", "radius").dice_range(), Some((150.0, 500.0)));
        for key in ["duration", "fps", "width", "height", "steps_per_second", "gravity"] {
            assert!(!def("scene", key).flags.random, "{key} must not roll");
        }
        assert!(!def("bounce.basic", "pops_after").flags.random, "popping changes what the scene is");
        let all = reg.things().flat_map(|k| k.settings()).chain(reg.modules().flat_map(|k| k.settings()));
        for d in all {
            if let (Some((lo, hi)), Some((min, max))) = (d.dice, d.range) {
                assert!(
                    lo >= min && hi <= max && lo < hi,
                    "{}: the dice range {lo}..{hi} sits inside {min}..{max}",
                    d.key
                );
            }
            if d.flags.random && matches!(d.ty, bpe_core::ValueType::Number | bpe_core::ValueType::Int) {
                assert!(d.dice_range().is_some(), "{}: a rolling number needs a range", d.key);
            }
        }
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
        assert_eq!(ids("balls", SlotId::Look), ["look.basic"]);
        assert_eq!(ids("balls", SlotId::Motion), ["motion.basic"]);
        assert_eq!(ids("balls", SlotId::Shader).len(), 13);
    }

    #[test]
    fn the_look_registry_has_one_ball_look() {
        let reg = registry();
        let looks: Vec<&str> = reg.modules_for(SlotId::Look).map(|m| m.id()).collect();
        assert_eq!(looks, ["look.basic"]);
        assert_eq!(reg.module("look.basic").unwrap().name(), "Look");
    }

    #[test]
    fn a_ball_bigger_than_its_arena_is_pinned_and_logs_no_hits() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        scene.things.get_mut(&ThingId(1)).unwrap().params.set("radius", Value::Number(20.0));
        scene.things.get_mut(&ThingId(2)).unwrap().params.set("radius", Value::Number(24.0));
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

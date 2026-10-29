//! The modular Ball Studio's plugins. Each thing and module kind lives in its own folder with
//! its settings, its simulation, its drawing and its sound side by side.

pub mod graphs;
pub mod modules;
pub mod scenes;
#[cfg(test)]
pub mod testing;
pub mod things;

use std::sync::Arc;

use bpe_core::{Color, Registry, Resolved, Scene, Settings, SlotId, ThingId};
use bpe_gfx::Draws;
use bpe_mix::{MixSettings, Mixes, Soundtrack};
use bpe_runtime::{Log, Sims};
use glam::Vec2;

pub use graphs::{graph_problems, resolve_scene, role_of};
pub use modules::bounce::sim::{BasicBounceSim, BounceState};
pub use modules::bounce::{BasicBounce, BasicBounceKind};
pub use modules::look::styled::{BallFill, BallShape, StyledLook, StyledLookKind};
pub use modules::look::{ClassicLook, ClassicLookKind};
pub use modules::motion::sim::{BasicMotionSim, MotionState};
pub use modules::motion::{BasicMotion, BasicMotionKind};
pub use modules::particles::draw::BurstDraw;
pub use modules::particles::{BurstBlend, BurstPreset, BurstSource, HitBurst, HitBurstKind};
pub use modules::rules::{Recipe, RulesKind, RulesSim, RulesState, recipes};
pub use modules::shader::EffectKind;
pub use modules::shader::draw::EffectDraw;
pub use modules::sound::mix::NoteSoundMix;
pub use modules::sound::{NoteSound, NoteSoundKind};
pub use modules::trail::draw::StreakDraw;
pub use modules::trail::{StreakTrail, StreakTrailKind, TrailBlend, TrailSource};
pub use things::arena::draw::ArenaDraw;
pub use things::arena::sim::{ArenaSim, ArenaState};
pub use things::arena::{ArenaKind, ArenaSettings, ArenaShape};
pub use things::balls::draw::BallsDraw;
pub use things::balls::sim::{BallsSim, BallsState};
pub use things::balls::{Arrangement, BallsKind, BallsSettings};
pub use things::folder::{FolderKind, FolderSettings, FolderSim};
pub use things::obstacle::draw::ObstacleDraw;
pub use things::obstacle::{MovePath, ObstacleKind, ObstacleSettings, ObstacleShape, ObstacleSim, ObstacleState};
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
    s.add_thing(ObstacleSim);
    s.add_module(BasicMotionSim);
    s.add_module(BasicBounceSim);
    s.add_module(RulesSim);
    s
}

/// Every kind's drawing part, for the renderer.
pub fn draws() -> Draws {
    let mut d = Draws::default();
    d.add(SceneDraw);
    d.add(ArenaDraw);
    d.add(BallsDraw);
    d.add(ObstacleDraw);
    for b in &bpe_gfx::effects::BUILTINS {
        d.add_module(EffectDraw(b));
    }
    d.add_module(StreakDraw);
    d.add_module(BurstDraw);
    d
}

/// Every kind's sound part, for the mixer.
pub fn mixes() -> Mixes {
    let mut m = Mixes::default();
    m.add(NoteSoundMix);
    m
}

/// Everything the kit gives the editor and the exporter, built once.
pub struct Kit {
    pub registry: Registry,
    pub sims: Arc<Sims>,
    pub draws: Draws,
    pub mixes: Mixes,
}

pub fn kit() -> Kit {
    Kit { registry: registry(), sims: Arc::new(sims()), draws: draws(), mixes: mixes() }
}

/// What the mixer needs from the root's settings, and every thing's tags.
pub fn mix_settings(scene: &Scene, resolved: &Resolved) -> MixSettings {
    let root = resolved.things.get(&ThingId::ROOT).map(|r| SceneSettings::from_params(&r.params)).unwrap_or_default();
    MixSettings {
        duration: root.duration,
        steps_per_second: root.steps_per_second,
        chords: root.chords,
        loudness: root.loudness,
        seed: root.seed as u64,
        half_width: root.width as f32 * 0.5,
        tags: scene.things.iter().filter(|(_, t)| !t.tags.is_empty()).map(|(id, t)| (*id, t.tags.clone())).collect(),
    }
}

/// The soundtrack of a finished run of `scene`.
pub fn mix_scene(scene: &Scene, resolved: &Resolved, log: &Log, kit: &Kit) -> Soundtrack {
    bpe_mix::mix_with(&mix_settings(scene, resolved), resolved, &scene.library, log, &kit.mixes)
}

/// Every kind the kit has, for the core's resolve and the editor's lists.
pub fn registry() -> Registry {
    let mut r = Registry::new();
    r.add_thing(SceneKind);
    r.add_thing(ArenaKind);
    r.add_thing(BallsKind);
    r.add_thing(FolderKind);
    r.add_thing(ObstacleKind);
    r.add_module(ClassicLookKind);
    r.add_module(StyledLookKind);
    r.add_module(StreakTrailKind);
    r.add_module(HitBurstKind);
    r.add_module(BasicMotionKind);
    r.add_module(BasicBounceKind);
    r.add_module(NoteSoundKind);
    for k in modules::shader::kinds() {
        r.add_module(k);
    }
    r.add_module(RulesKind);
    r
}

/// What a brand-new scene holds: a circle arena and one pink ball, sharing the root's plain
/// look, motion, bounce and sound.
pub fn new_scene(reg: &Registry) -> Scene {
    let mut scene = Scene::empty(reg);
    scene.name = "New scene".into();
    let root = ThingId::ROOT;
    let look = scene.add_module(reg, "look.classic", "Plain look").expect("kit kinds are registered");
    let motion = scene.add_module(reg, "motion.basic", "Default motion").expect("kit kinds are registered");
    let bounce = scene.add_module(reg, "bounce.basic", "Default bounce").expect("kit kinds are registered");
    let sound = scene.add_module(reg, "sound.notes", "Default sound").expect("kit kinds are registered");
    for (slot, m) in [(SlotId::Look, look), (SlotId::Motion, motion), (SlotId::Bounce, bounce), (SlotId::Sound, sound)]
    {
        scene.link(reg, root, slot, m).expect("the root has every slot");
    }
    scene.add_thing(reg, "arena", root).expect("kit kinds are registered");
    let ball = scene.add_thing(reg, "balls", root).expect("kit kinds are registered");
    let ball_look = scene.add_module(reg, "look.classic", "Ball look").expect("kit kinds are registered");
    scene.library.modules.get_mut(&ball_look).expect("just added").params =
        ClassicLook { color: Color::rgb(0xff, 0x5c, 0xa8), ..ClassicLook::default() }.to_params();
    scene.link(reg, ball, SlotId::Look, ball_look).expect("balls have a look slot");
    let thing = scene.things.get_mut(&ball).expect("just added");
    thing.name = "Ball".into();
    thing.transform.position = Vec2::new(0.0, -200.0);
    scene
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Color, SlotId, ThingId, Value, resolve};
    use bpe_runtime::{Log, World};

    #[test]
    fn every_kit_plugin_passes_the_contract() {
        assert_eq!(bpe_core::check_registry(&registry()), Vec::<String>::new());
    }

    #[test]
    fn a_new_scene_is_an_arena_and_one_ball_with_shared_modules() {
        let reg = registry();
        let s = new_scene(&reg);
        assert_eq!(s.name, "New scene");
        let kinds: Vec<&str> = s.order.iter().map(|id| s.things[id].kind.as_str()).collect();
        assert_eq!(kinds, ["scene", "arena", "balls"]);
        assert_eq!(s.things[&ThingId(2)].name, "Ball");
        assert_eq!(s.things[&ThingId(2)].transform.position, glam::Vec2::new(0.0, -200.0));
        assert_eq!(s.library.modules.len(), 5);
        let res = bpe_core::resolve(&s, &reg);
        assert!(res.problems.is_empty(), "{:?}", res.problems);
        let ball = &res.things[&ThingId(2)];
        assert_eq!(ball.slots[&SlotId::Look].params.get("color"), Some(&Value::Color(Color::rgb(0xff, 0x5c, 0xa8))));
        assert_eq!(ball.slots[&SlotId::Bounce].from, ThingId::ROOT);
        assert_eq!(ball.slots[&SlotId::Motion].from, ThingId::ROOT);
        assert_eq!(ball.slots[&SlotId::Sound].from, ThingId::ROOT);
        assert_eq!(res.things[&ThingId(1)].slots[&SlotId::Look].from, ThingId::ROOT);
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
}

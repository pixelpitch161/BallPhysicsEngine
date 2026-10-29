//! The modular Ball Studio's plugins. Each thing and module kind lives in its own folder with
//! its settings, its simulation, its drawing and its sound side by side.

pub mod gizmo;
pub mod graphs;
pub mod modules;
pub mod music;
pub mod rng;
pub mod scenes;
#[cfg(test)]
pub mod testing;
pub mod things;

use std::sync::Arc;

use bpe_core::{Color, EditError, ModuleId, Registry, Resolved, Scene, Settings, SlotId, ThingId, Value};
use bpe_gfx::{Draws, FlowSource};
use bpe_mix::{MixSettings, Mixes, Soundtrack};
use bpe_runtime::{Log, Sims};
use glam::Vec2;

pub use gizmo::{GIZMO_GRAB, Gizmo, gizmos};
pub use graphs::{graph_problems, resolve_scene, role_of};
pub use modules::bounce::sim::{BasicBounceSim, BounceState};
pub use modules::bounce::{BasicBounce, BasicBounceKind};
pub use modules::drums::mix::DrumSoundMix;
pub use modules::drums::{DrumSound, DrumSoundKind, Piece};
pub use modules::look::styled::{BallFill, BallShape, StyledLook, StyledLookKind};
pub use modules::look::{ClassicLook, ClassicLookKind};
pub use modules::look_liquid::{LiquidLook, LiquidLookKind};
pub use modules::look_swarm::{SwarmColors, SwarmLook, SwarmLookKind};
pub use modules::melody::mix::MelodySoundMix;
pub use modules::melody::{MelodySound, MelodySoundKind, load_tune};
pub use modules::motion::sim::{BasicMotionSim, MotionState};
pub use modules::motion::{BasicMotion, BasicMotionKind};
pub use modules::motion_flock::{Flocking, FlockingKind};
pub use modules::motion_liquid::sim::FlowingSim;
pub use modules::motion_liquid::{Flowing, FlowingKind};
pub use modules::particles::draw::BurstDraw;
pub use modules::particles::{BurstBlend, BurstPreset, BurstSource, HitBurst, HitBurstKind};
pub use modules::rules::{Recipe, RulesKind, RulesSim, RulesState, recipes};
pub use modules::shader::EffectKind;
pub use modules::shader::draw::EffectDraw;
pub use modules::sound::mix::NoteSoundMix;
pub use modules::sound::{NoteSound, NoteSoundKind};
pub use modules::trail::draw::StreakDraw;
pub use modules::trail::{StreakTrail, StreakTrailKind, TrailBlend, TrailSource};
pub use music::{
    Song, SongState, TimeWarp, beats_for, forget_song, load_song, load_song_in_background, wants_beats, warp_for,
};
pub use things::arena::draw::ArenaDraw;
pub use things::arena::sim::{ArenaSim, ArenaState};
pub use things::arena::{ArenaKind, ArenaSettings, ArenaShape};
pub use things::balls::draw::BallsDraw;
pub use things::balls::sim::{BallsSim, BallsState};
pub use things::balls::{Arrangement, BallsKind, BallsSettings};
pub use things::emitter::sim::{EmitterSim, EmitterState, item_radius};
pub use things::emitter::{EmitterKind, EmitterSettings, Trigger};
pub use things::field::sim::{FieldSim, FieldState, weight as field_weight};
pub use things::field::{Falloff, FieldKind, FieldKindChoice, FieldSettings, FieldShape};
pub use things::folder::{FolderKind, FolderSettings, FolderSim};
pub use things::liquid::draw::LiquidDraw;
pub use things::liquid::pbf::Rect;
pub use things::liquid::sim::{LiquidSim, LiquidState};
pub use things::liquid::{LiquidKind, LiquidSettings, LiquidStart};
pub use things::obstacle::draw::ObstacleDraw;
pub use things::obstacle::{MovePath, ObstacleKind, ObstacleSettings, ObstacleShape, ObstacleSim, ObstacleState};
pub use things::scene::draw::SceneDraw;
pub use things::scene::sim::{SceneSim, SceneState};
pub use things::scene::{BeatSync, SceneKind, SceneSettings};
pub use things::soft::{Impact, Pinned, SoftDraw, SoftKind, SoftSettings, SoftShape, SoftSim, SoftState, SoftWorld};
pub use things::swarm::draw::SwarmDraw;
pub use things::swarm::sim::{SwarmSim, SwarmState};
pub use things::swarm::{SwarmKind, SwarmSettings, SwarmStart};
pub use things::text::{Align, Appear, Font, TextDraw, TextKind, TextSettings};

/// Every kind's simulation part, for the runtime.
pub fn sims() -> Sims {
    let mut s = Sims::default();
    s.add_thing(SceneSim);
    s.add_thing(ArenaSim);
    s.add_thing(BallsSim);
    s.add_thing(EmitterSim);
    s.add_thing(FieldSim);
    s.add_thing(LiquidSim);
    s.add_thing(SwarmSim);
    s.add_thing(SoftSim);
    s.add_thing(FolderSim);
    s.add_thing(ObstacleSim);
    s.add_module(BasicMotionSim);
    s.add_module(BasicBounceSim);
    s.add_module(RulesSim);
    s.add_module(FlowingSim);
    s
}

/// Every kind's drawing part, for the renderer.
pub fn draws() -> Draws {
    let mut d = Draws::default();
    d.add(SceneDraw);
    d.add(ArenaDraw);
    d.add(BallsDraw);
    d.add(LiquidDraw);
    d.add(SwarmDraw);
    d.add(SoftDraw);
    d.add(TextDraw);
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
    m.add(DrumSoundMix);
    m.add(MelodySoundMix);
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

/// A module a new thing starts with: the module kind, its name in the library, and the
/// settings that differ from the kind's defaults.
type Starter = (&'static str, &'static str, &'static [(&'static str, Value)]);

/// The shared modules a new thing of `kind` starts with.
fn starter_modules(kind: &str) -> &'static [Starter] {
    const SOFT: &[(&str, Value)] = &[("bounciness", Value::Number(0.0))];
    match kind {
        "liquid" => &[
            ("look.liquid", "Liquid surface", &[]),
            ("motion.liquid", "Flowing", &[]),
            ("bounce.basic", "Soft landing", SOFT),
        ],
        "swarm" => &[("look.swarm", "Glowing dots", &[]), ("motion.flock", "Flocking", &[])],
        _ => &[],
    }
}

/// Adds a thing of `kind` under `parent` and links the kit's shared starter modules for that
/// kind (a liquid gets its surface look, flowing motion and a soft landing; a swarm its glowing
/// dots and flocking), reusing a library module of the same kind and name when the scene
/// already has one. Other kinds are added plain.
pub fn add_thing(scene: &mut Scene, reg: &Registry, kind: &str, parent: ThingId) -> Result<ThingId, EditError> {
    let id = scene.add_thing(reg, kind, parent)?;
    for &(module_kind, name, settings) in starter_modules(kind) {
        let existing: Option<ModuleId> =
            scene.library.modules.values().find(|m| m.kind == module_kind && m.name == name).map(|m| m.id);
        let module = match existing {
            Some(m) => m,
            None => {
                let m = scene.add_module(reg, module_kind, name)?;
                let params = &mut scene.library.modules.get_mut(&m).expect("just added").params;
                for (key, value) in settings {
                    params.set(key, value.clone());
                }
                m
            }
        };
        let slot = reg.module(module_kind).expect("kit kinds are registered").slot();
        scene.link(reg, id, slot, module)?;
    }
    Ok(id)
}

/// What the renderer's graphics-card layers step along with: the scene, keyed by its physics
/// fingerprint so a change that moves the balls starts the layers over, and each ball thing's
/// colour for swarms coloured by the nearest ball. `budget` caps how long one frame may spend
/// catching up; `None` always catches up, as an export must.
pub fn flow_source<'a>(
    scene: &'a Scene,
    resolved: &'a Resolved,
    sims: &'a Sims,
    budget: Option<std::time::Duration>,
) -> FlowSource<'a> {
    let ball_colors = resolved
        .things
        .iter()
        .filter(|(_, r)| r.kind == "balls")
        .filter_map(|(id, r)| {
            let look = r.slots.get(&SlotId::Look)?;
            (look.kind == "look.classic").then(|| (*id, ClassicLook::from_params(&look.params).color.to_linear()))
        })
        .collect();
    FlowSource { scene, resolved, sims, key: bpe_runtime::fingerprint(scene, resolved, sims), budget, ball_colors }
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

/// The soundtrack of a finished run of `scene`: its keyframed sound settings followed, its
/// notes placed through the beat sync's time warp, and its song mixed in underneath.
pub fn mix_scene(scene: &Scene, resolved: &Resolved, log: &Log, kit: &Kit) -> Soundtrack {
    let animation = bpe_core::Animation::new(scene, resolved);
    let animation = (!animation.is_empty()).then_some(&animation);
    let warp = warp_for(scene, resolved, log);
    let mut track =
        bpe_mix::mix_with(&mix_settings(scene, resolved), resolved, &scene.library, log, &kit.mixes, animation, &|t| {
            warp.video_time(t)
        });
    let root = music::root_settings(resolved);
    if let Some(song) = load_song(&root.song) {
        music::mix_in(&mut track.samples, &song, root.song_offset, root.song_volume, root.ducking, root.loudness);
    }
    track
}

/// Every kind the kit has, for the core's resolve and the editor's lists.
pub fn registry() -> Registry {
    let mut r = Registry::new();
    r.add_thing(SceneKind);
    r.add_thing(ArenaKind);
    r.add_thing(BallsKind);
    r.add_thing(SoftKind);
    r.add_thing(TextKind);
    r.add_thing(FolderKind);
    r.add_thing(ObstacleKind);
    r.add_thing(EmitterKind);
    r.add_thing(FieldKind);
    r.add_thing(LiquidKind);
    r.add_thing(SwarmKind);
    r.add_module(ClassicLookKind);
    r.add_module(StyledLookKind);
    r.add_module(LiquidLookKind);
    r.add_module(SwarmLookKind);
    r.add_module(StreakTrailKind);
    r.add_module(HitBurstKind);
    r.add_module(BasicMotionKind);
    r.add_module(FlowingKind);
    r.add_module(FlockingKind);
    r.add_module(BasicBounceKind);
    r.add_module(NoteSoundKind);
    r.add_module(DrumSoundKind);
    r.add_module(MelodySoundKind);
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
        assert_eq!(def("obstacle", "radius").dice_range(), Some((10.0, 200.0)));
        assert_eq!(def("liquid", "count").dice_range(), Some((500.0, 20000.0)));
        assert_eq!(def("swarm", "count").dice_range(), Some((500.0, 50000.0)));
        assert_eq!(def("soft", "size").dice_range(), Some((60.0, 400.0)));
        for key in ["duration", "fps", "width", "height", "steps_per_second", "song", "bpm", "gravity"] {
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
    fn the_new_module_kinds_are_registered_on_their_slots() {
        let reg = registry();
        let looks: Vec<&str> = reg.modules_for(SlotId::Look).map(|m| m.id()).collect();
        assert_eq!(looks, ["look.classic", "look.liquid", "look.styled", "look.swarm"]);
        let motions: Vec<&str> = reg.modules_for(SlotId::Motion).map(|m| m.id()).collect();
        assert_eq!(motions, ["motion.basic", "motion.flock", "motion.liquid"]);
        assert_eq!(reg.module("look.liquid").unwrap().name(), "Liquid surface");
        assert_eq!(reg.module("motion.liquid").unwrap().name(), "Flowing");
        assert_eq!(reg.module("look.swarm").unwrap().name(), "Glowing dots");
        assert_eq!(reg.module("motion.flock").unwrap().name(), "Flocking");
        assert!(sims().module("motion.liquid").is_some());
        assert!(sims().module("motion.flock").is_none(), "flocking runs on the GPU only");
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

    #[test]
    fn adding_a_liquid_links_its_three_shared_modules() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        let a = add_thing(&mut scene, &reg, "liquid", ThingId::ROOT).unwrap();
        let module = |scene: &Scene, id: ThingId, slot: SlotId| {
            let bpe_core::SlotLink::Linked { module: m, .. } = scene.things[&id].slots[&slot] else { panic!("linked") };
            let module = &scene.library.modules[&m];
            (m, module.kind.clone(), module.name.clone())
        };
        let (look, look_kind, look_name) = module(&scene, a, SlotId::Look);
        let (motion, motion_kind, motion_name) = module(&scene, a, SlotId::Motion);
        let (bounce, bounce_kind, bounce_name) = module(&scene, a, SlotId::Bounce);
        assert_eq!((look_kind.as_str(), look_name.as_str()), ("look.liquid", "Liquid surface"));
        assert_eq!((motion_kind.as_str(), motion_name.as_str()), ("motion.liquid", "Flowing"));
        assert_eq!((bounce_kind.as_str(), bounce_name.as_str()), ("bounce.basic", "Soft landing"));
        assert_eq!(scene.library.modules[&bounce].params.get("bounciness"), Some(&Value::Number(0.0)));
        let modules = scene.library.modules.len();
        let b = add_thing(&mut scene, &reg, "liquid", ThingId::ROOT).unwrap();
        assert_eq!(scene.library.modules.len(), modules, "the second liquid shares the first one's modules");
        assert_eq!(module(&scene, b, SlotId::Look).0, look);
        assert_eq!(module(&scene, b, SlotId::Motion).0, motion);
        assert_eq!(module(&scene, b, SlotId::Bounce).0, bounce);
        let resolved = resolve(&scene, &reg);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    }

    #[test]
    fn adding_a_swarm_links_dots_and_flocking() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        let s = add_thing(&mut scene, &reg, "swarm", ThingId::ROOT).unwrap();
        let kinds: Vec<(SlotId, String, String)> = scene.things[&s]
            .slots
            .iter()
            .map(|(slot, link)| {
                let bpe_core::SlotLink::Linked { module: m, .. } = link else { panic!("linked") };
                (*slot, scene.library.modules[m].kind.clone(), scene.library.modules[m].name.clone())
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                (SlotId::Look, "look.swarm".to_string(), "Glowing dots".to_string()),
                (SlotId::Motion, "motion.flock".to_string(), "Flocking".to_string())
            ]
        );
    }

    #[test]
    fn adding_balls_is_unchanged() {
        let reg = registry();
        let mut plain = new_scene(&reg);
        let mut kit_scene = plain.clone();
        let a = plain.add_thing(&reg, "balls", ThingId::ROOT).unwrap();
        let b = add_thing(&mut kit_scene, &reg, "balls", ThingId::ROOT).unwrap();
        assert_eq!(a, b);
        assert_eq!(plain, kit_scene);
    }
}

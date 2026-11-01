//! The modular Ball Studio's plugins. Each thing and module kind lives in its own folder with
//! its settings, its simulation, its drawing and its sound side by side.

pub mod gizmo;
pub mod graphs;
#[cfg(test)]
mod layer_tests;
pub mod modules;
pub mod music;
pub mod paint;
pub mod random;
pub mod rng;
pub mod scenes;
pub mod score;
pub mod starters;
#[cfg(test)]
pub mod testing;
pub mod things;

use std::sync::Arc;

use bpe_core::{Color, Registry, Resolved, Scene, Settings, SlotId, SlotLink, ThingId, Value};
use bpe_gfx::{Draws, FlowSource};
use bpe_mix::{MixSettings, Mixes, Soundtrack};
use bpe_runtime::{Log, Sims};
use glam::Vec2;
use paint::Paint;

pub use gizmo::{GIZMO_GRAB, Gizmo, gizmos};
pub use graphs::{graph_problems, resolve_scene, role_of};
pub use modules::bounce::sim::{BasicBounceSim, BounceState};
pub use modules::bounce::{BasicBounce, BasicBounceKind};
pub use modules::drums::mix::DrumSoundMix;
pub use modules::drums::{DrumSound, DrumSoundKind, Piece};
pub use modules::look::{BallShape, BasicLook, BasicLookKind, Style};
pub use modules::look_liquid::{LiquidLook, LiquidLookKind};
pub use modules::look_swarm::{SwarmLook, SwarmLookKind};
pub use modules::melody::mix::MelodySoundMix;
pub use modules::melody::{MelodySound, MelodySoundKind, load_tune};
pub use modules::motion::sim::{BasicMotionSim, MotionState};
pub use modules::motion::{BasicMotion, BasicMotionKind};
pub use modules::motion_flock::{Flocking, FlockingKind};
pub use modules::motion_liquid::sim::FlowingSim;
pub use modules::motion_liquid::{Flowing, FlowingKind};
pub use modules::particles::draw::BurstDraw;
pub use modules::particles::{BurstPreset, BurstSource, HitBurst, HitBurstKind};
pub use modules::rules::{Recipe, RulesKind, RulesSim, RulesState, recipes};
pub use modules::shader::EffectKind;
pub use modules::shader::draw::EffectDraw;
pub use modules::sound::mix::NoteSoundMix;
pub use modules::sound::{NoteSound, NoteSoundKind};
pub use modules::trail::draw::StreakDraw;
pub use modules::trail::{StreakTrail, StreakTrailKind, TrailSource};
pub use music::{Extras, SceneExport, midi_path, mix_scene_export, stems_folder, write_extras};
pub use music::{
    Song, SongState, TimeWarp, beats_for, forget_song, load_song, load_song_in_background, physics_inputs,
    uses_live_inputs, wants_beats, warp_for,
};
pub use starters::{add_thing, fill_slot, keep_followers, make_folder, move_thing};
pub use things::arena::draw::ArenaDraw;
pub use things::arena::sim::{ArenaSim, ArenaState};
pub use things::arena::{ArenaKind, ArenaSettings, ArenaShape};
pub use things::balls::draw::BallsDraw;
pub use things::balls::sim::{BallsSim, BallsState};
pub use things::balls::{Arrangement, BallsKind, BallsSettings};
pub use things::camera::sim::{CameraSim, CameraState};
pub use things::camera::{CameraKind, CameraSettings};
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
pub use things::scene::{BeatSync, Clip, SceneKind, SceneSettings};
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
    s.add_thing(CameraSim);
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
            (look.kind == "look.basic").then(|| (*id, BasicLook::from_params(&look.params).first_colour()))
        })
        .collect();
    FlowSource { scene, resolved, sims, key: bpe_runtime::fingerprint(scene, resolved, sims), budget, ball_colors }
}

/// What the mixer needs from the root's settings, and every thing's tags.
/// The progression the whole song follows: that of the first thing in the tree whose sound picks
/// one from the library, else the library's first, else none (the built-in starter).
fn song_chords(scene: &Scene, resolved: &Resolved) -> bpe_core::ProgressionRef {
    let picked = scene.order.iter().find_map(|id| {
        let params = &resolved.things.get(id)?.slots.get(&SlotId::Sound)?.params;
        let p = <bpe_core::ProgressionRef as bpe_core::SettingValue>::from_value(params.get("chords")?)?.0?;
        scene.library.progressions.contains_key(&p).then_some(p)
    });
    bpe_core::ProgressionRef(picked.or_else(|| scene.library.progressions.keys().next().copied()))
}

pub fn mix_settings(scene: &Scene, resolved: &Resolved) -> MixSettings {
    let root = resolved.things.get(&ThingId::ROOT).map(|r| SceneSettings::from_params(&r.params)).unwrap_or_default();
    MixSettings {
        duration: root.duration,
        steps_per_second: root.steps_per_second,
        progressions: scene.library.progressions.clone(),
        chords: song_chords(scene, resolved),
        beats: music::chord_beats(&root),
        master: bpe_mix::MasterSettings {
            loudness: root.loudness,
            clipper: match root.clipper {
                Clip::Off => bpe_mix::ClipMode::Off,
                Clip::Soft => bpe_mix::ClipMode::Soft,
                Clip::Hard => bpe_mix::ClipMode::Hard,
            },
            drive_db: root.clip_drive,
            ceiling_db: root.ceiling,
            release_ms: (!root.auto_release).then_some(root.release),
        },
        seed: root.seed as u64,
        half_width: root.width as f32 * 0.5,
        tags: scene.things.iter().filter(|(_, t)| !t.tags.is_empty()).map(|(id, t)| (*id, t.tags.clone())).collect(),
        off_shot_fade: root.off_shot_fade.max(0.0),
        half_height: root.height as f32 * 0.5,
        instruments: scene.library.instruments.clone(),
        macros: scene
            .library
            .modules
            .iter()
            .filter(|(_, m)| !m.macros.is_empty())
            .map(|(id, m)| (id.0, m.macros.clone()))
            .collect(),
        strips: scene
            .library
            .modules
            .iter()
            .filter(|(_, m)| !m.channel.is_default())
            .map(|(id, m)| (id.0, m.channel.clone()))
            .collect(),
        buses: scene.library.buses.clone(),
        master_fx: scene.library.master_fx.clone(),
        channel_names: scene
            .library
            .modules
            .iter()
            .filter(|(_, m)| m.slot == SlotId::Sound)
            .map(|(id, m)| (id.0, m.name.clone()))
            .collect(),
    }
}

/// The two return buses a new scene starts with: "Reverb" and a quarter-note ping-pong "Delay",
/// each fully wet.
pub fn add_starter_buses(library: &mut bpe_core::Library) {
    use bpe_graph::Param;
    let wet = |kind: &str, extra: &[(&str, Param)]| {
        let mut knobs: std::collections::BTreeMap<String, Param> = [("mix".to_string(), Param::Number(1.0))].into();
        knobs.extend(extra.iter().map(|(k, v)| (k.to_string(), v.clone())));
        bpe_synth::chain_graph(&[(kind.to_string(), knobs, true)])
    };
    library.add_bus("Reverb", wet("reverb", &[]));
    library.add_bus(
        "Delay",
        wet("delay", &[("time_mode", Param::Choice("1/4".into())), ("ping_pong", Param::Bool(true))]),
    );
}

/// A library instrument that plays the built-in `which`, named after it. The pluck gets the
/// decay and brightness it always had.
pub fn built_in_instrument(which: bpe_mix::Instrument) -> bpe_core::Instrument {
    let knobs = match which {
        bpe_mix::Instrument::Pluck => [("decay".to_string(), 0.9), ("brightness".to_string(), 0.5)].into(),
        _ => Default::default(),
    };
    bpe_core::Instrument {
        name: which.label().into(),
        source: bpe_core::InstrumentSource::BuiltIn { which: bpe_core::Choice::name(which).into(), knobs },
        ..Default::default()
    }
}

/// The soundtrack of a finished run of `scene`: its keyframed sound settings followed, its
/// notes placed through the beat sync's time warp, and its song mixed in underneath.
pub fn mix_scene(scene: &Scene, resolved: &Resolved, log: &Log, kit: &Kit) -> Soundtrack {
    mix_scene_cached(scene, resolved, log, kit, &mut bpe_mix::ChannelCache::default())
}

/// [`mix_scene`], rendering only the channels that changed since the mix that filled `cache`.
pub fn mix_scene_cached(
    scene: &Scene,
    resolved: &Resolved,
    log: &Log,
    kit: &Kit,
    cache: &mut bpe_mix::ChannelCache,
) -> Soundtrack {
    let animation = bpe_core::Animation::new(scene, resolved);
    let animation = (!animation.is_empty()).then_some(&animation);
    let warp = warp_for(scene, resolved, log);
    let settings = mix_settings(scene, resolved);
    let video_time = |t| warp.video_time(t);
    let physics = music::physics_inputs(scene, resolved, log, &settings, &video_time);
    let mut track = bpe_mix::mix_premaster(
        &settings,
        resolved,
        &scene.library,
        log,
        &kit.mixes,
        animation,
        &video_time,
        &physics,
        cache,
    );
    let root = music::root_settings(resolved);
    if let Some(song) = load_song(&root.song) {
        music::mix_in(&mut track.samples, &song, root.song_offset, root.song_volume, root.ducking, root.loudness);
    }
    // The master effects and the master run once, over the notes and the song together.
    let problems = bpe_mix::run_master_fx(&mut track.samples, &settings, &physics.with_levels(&track.levels));
    track.problems.extend(problems);
    track.report = Some(bpe_mix::master(&mut track.samples, &settings.master));
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
    r.add_thing(CameraKind);
    r.add_thing(LiquidKind);
    r.add_thing(SwarmKind);
    r.add_module(BasicLookKind);
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
    add_starter_buses(&mut scene.library);
    let root = ThingId::ROOT;
    add_thing(&mut scene, reg, "arena", root).expect("kit kinds are registered");
    let ball = add_thing(&mut scene, reg, "balls", root).expect("kit kinds are registered");
    scene.rename_thing(ball, "Ball");
    let SlotLink::Linked { module: look, .. } = scene.things[&ball].slots[&SlotId::Look] else {
        unreachable!("a new ball owns its look")
    };
    scene.library.modules.get_mut(&look).expect("just added").params =
        BasicLook { fill: Paint::solid(Color::rgb(0xff, 0x5c, 0xa8)), ..BasicLook::default() }.to_params();
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

/// A new scene as the editor starts it: [`new_scene`] with a camera, and hits off the shot
/// fading away.
pub fn new_studio_scene(reg: &Registry) -> Scene {
    let mut scene = new_scene(reg);
    ensure_camera(&mut scene, reg);
    scene.things.get_mut(&ThingId::ROOT).expect("root").params.set("off_shot_fade", Value::Number(1.5));
    scene
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{SlotId, ThingId, Value, resolve};
    use bpe_runtime::{Log, World};

    #[test]
    fn every_addable_thing_has_a_family() {
        let families = ["Bouncers", "Walls", "Flowing", "Forces", "On screen"];
        let reg = registry();
        for kind in reg.things().filter(|k| !["scene", "folder", "camera"].contains(&k.id())) {
            assert!(families.contains(&kind.family()), "{} has family {}", kind.id(), kind.family());
        }
    }

    #[test]
    fn new_scenes_have_reverb_and_delay_buses() {
        let scene = new_scene(&registry());
        let names: Vec<&str> = scene.library.buses.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, ["Reverb", "Delay"]);
        let kinds = |g: &bpe_graph::Graph| -> Vec<String> {
            bpe_synth::straight_chain(g).unwrap().iter().map(|id| g.node(*id).unwrap().kind.clone()).collect()
        };
        assert_eq!(kinds(&scene.library.buses[0].fx), ["reverb"]);
        assert_eq!(kinds(&scene.library.buses[1].fx), ["delay"]);
    }

    #[test]
    fn a_new_scene_owns_a_starter_progression() {
        let reg = registry();
        let scene = new_scene(&reg);
        assert_eq!(scene.library.progressions.len(), 1);
        let (&id, p) = scene.library.progressions.iter().next().unwrap();
        assert_eq!(p.name, "Starter");
        let sound = resolve(&scene, &reg).things[&ThingId(2)].slots[&SlotId::Sound].params.clone();
        assert_eq!(sound.get("chords"), Some(&Value::Int(id.0 as i64)));
    }

    #[test]
    fn a_new_scene_plays_its_library_pluck() {
        let reg = registry();
        let scene = new_scene(&reg);
        let settings = mix_settings(&scene, &resolve(&scene, &reg));
        let (&id, pluck) = scene.library.instruments.iter().next().unwrap();
        assert_eq!(pluck.name, "Pluck");
        let sound = resolve(&scene, &reg).things[&ThingId(2)].slots[&SlotId::Sound].params.clone();
        assert_eq!(sound.get("instrument"), Some(&Value::Int(id.0 as i64)));
        assert_eq!(
            bpe_mix::instrument_for(&settings, None, 0),
            bpe_mix::InstrumentChoice::BuiltIn { which: bpe_mix::Instrument::Pluck, decay: 0.9, brightness: 0.5 }
        );
    }

    #[test]
    fn the_songs_chords_are_the_first_sounding_things() {
        let reg = registry();
        let (_, scene) = crate::scenes::all(&reg).into_iter().find(|(n, _)| n == "Drum beat").unwrap();
        let settings = mix_settings(&scene, &resolve(&scene, &reg));
        assert_eq!(settings.progression(settings.chords).name, "Epic minor", "the MIDI and the patches' chords");
        let new = new_scene(&reg);
        let settings = mix_settings(&new, &resolve(&new, &reg));
        assert_eq!(settings.progression(settings.chords).name, "Starter");
    }

    #[test]
    fn mix_settings_carry_progressions_and_beats() {
        let reg = registry();
        let scene = new_scene(&reg);
        let settings = mix_settings(&scene, &resolve(&scene, &reg));
        assert_eq!(settings.progression(bpe_core::ProgressionRef(None)).name, "Starter");
        assert!(settings.beats.len() >= 40, "120 bpm over 20 s: {}", settings.beats.len());
        let missing = bpe_core::ProgressionRef(Some(bpe_core::ProgressionId(99)));
        assert_eq!(settings.progression(missing).name, "Starter", "a missing one falls back to the scene's");
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
        for m in reg.modules().filter(|m| m.slot() != SlotId::Sound) {
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
        for kind in ["motion.basic", "motion.flock", "motion.liquid"] {
            let d = defs_of(&reg, kind).iter().find(|d| d.key == "gravity_scale").unwrap();
            assert_eq!(d.label, "Gravity ×", "{kind}");
            assert_eq!(d.help, "How strongly the scene's gravity pulls these, as a multiple.", "{kind}");
        }
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
        assert_eq!(def("obstacle", "radius").dice_range(), Some((6.0, 60.0)));
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
    fn kinds_fitting_limits_looks_and_motions_by_thing() {
        let reg = registry();
        let ids = |thing: &str, slot: SlotId| -> Vec<&'static str> {
            let t = reg.thing(thing).unwrap();
            reg.kinds_fitting(slot, t).map(|m| m.id()).collect()
        };
        assert_eq!(ids("balls", SlotId::Look), ["look.basic"]);
        assert_eq!(ids("liquid", SlotId::Look), ["look.liquid"]);
        assert_eq!(ids("swarm", SlotId::Look), ["look.swarm"]);
        assert_eq!(ids("balls", SlotId::Motion), ["motion.basic"]);
        assert_eq!(ids("liquid", SlotId::Motion), ["motion.liquid"]);
        assert_eq!(ids("swarm", SlotId::Motion), ["motion.flock"]);
        assert_eq!(ids("balls", SlotId::Shader).len(), 13);
        assert_eq!(ids("scene", SlotId::Sound), ["sound.drums", "sound.melody"]);
    }

    #[test]
    fn the_look_registry_has_one_ball_look() {
        let reg = registry();
        let looks: Vec<&str> = reg.modules_for(SlotId::Look).map(|m| m.id()).collect();
        assert_eq!(looks, ["look.basic", "look.liquid", "look.swarm"]);
        assert_eq!(reg.module("look.basic").unwrap().name(), "Look");
    }

    #[test]
    fn the_new_module_kinds_are_registered_on_their_slots() {
        let reg = registry();
        let looks: Vec<&str> = reg.modules_for(SlotId::Look).map(|m| m.id()).collect();
        assert_eq!(looks, ["look.basic", "look.liquid", "look.swarm"]);
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
                (SlotId::Look, "look.swarm".to_string(), "Swarm look".to_string()),
                (SlotId::Motion, "motion.flock".to_string(), "Swarm motion".to_string())
            ]
        );
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
        let mut scene = new_scene(&reg);
        let text = scene.add_thing(&reg, "text", ThingId::ROOT).unwrap();
        let saved = bpe_core::save(&scene);
        assert!(!saved.contains("screen"), "{saved}");
        let loaded = bpe_core::load(&saved, &reg).unwrap();
        assert_eq!(camera_of(&loaded), None);
        let t = &loaded.things[&text];
        assert!(t.sticks_to_screen(reg.thing("text").unwrap()));
        let b = &loaded.things[&ThingId(2)];
        assert!(!b.sticks_to_screen(reg.thing("balls").unwrap()));
    }

    #[test]
    fn new_studio_scene_has_a_camera_and_fades_off_shot_hits() {
        let reg = registry();
        let scene = new_studio_scene(&reg);
        assert!(camera_of(&scene).is_some());
        let root = SceneSettings::from_params(&bpe_core::resolve(&scene, &reg).things[&ThingId::ROOT].params);
        assert_eq!(root.off_shot_fade, 1.5);
    }
}

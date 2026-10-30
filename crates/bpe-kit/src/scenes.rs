//! The scenes the stages ship with: the new-scene default and the ones that exercise every
//! setting. They double as the determinism test scenes.

#[path = "scenes_stage5.rs"]
mod stage5;
pub use stage5::{stage5, stage5_with_twins};

use bpe_core::{
    Address, Attach, Choice, Color, Ease, InstrumentRef, InstrumentSource, ModuleGraph, ModuleId, ModuleKind, MusicKey,
    Owner, Params, Progression, ProgressionRef, PulseSource, Registry, Scene, SettingValue, Settings, SlotId, Target,
    ThingId, Value,
};
use bpe_graph::{Catalog, Graph, Link, Param};
use bpe_mix::Instrument;
use bpe_vm::{Role, catalog};
use glam::Vec2;

use crate::graphs::module_graph;
use crate::modules::look::GradientBy;
use crate::modules::sound::{Order, PlaysWhen};
use crate::{
    Align, Appear, ArenaSettings, ArenaShape, Arrangement, BallsSettings, BasicBounce, BasicMotion, BeatSync,
    ClassicLook, DrumSound, EmitterSettings, FieldKindChoice, FieldSettings, FieldShape, Flocking, Font,
    LiquidSettings, MelodySound, MovePath, NoteSound, ObstacleSettings, ObstacleShape, Piece, Pinned, RulesKind,
    SceneSettings, SoftSettings, SoftShape, SwarmColors, SwarmLook, SwarmSettings, TextSettings, new_scene, recipes,
};

const ARENA: ThingId = ThingId(1);
const BALLS: ThingId = ThingId(2);

fn build_all(reg: &Registry, builders: &[fn(&Registry) -> Scene]) -> Vec<(String, Scene)> {
    builders
        .iter()
        .map(|build| {
            let scene = build(reg);
            (scene.name.clone(), scene)
        })
        .collect()
}

/// The five stage 2 scenes, in order, each with its name.
pub fn stage2(reg: &Registry) -> Vec<(String, Scene)> {
    build_all(reg, &[one_ball, reel_block, box_drop, spinning_hexagon, shrinking_ring])
}

/// The five stage 3 scenes: obstacles, folders, riders, rounded arenas and balls hitting balls.
pub fn stage3(reg: &Registry) -> Vec<(String, Scene)> {
    build_all(reg, &[peg_board, spinning_bars, ball_pit, riders, pitch_climb])
}

/// The five stage 4 scenes: rules on things, and modules whose graphs have been edited.
pub fn stage4(reg: &Registry) -> Vec<(String, Scene)> {
    build_all(reg, &[grow_on_hit, split_on_hit, colour_by_speed, bouncy_when_slow, wind_and_melody])
}

/// The five stage 6 scenes: an emitter, fields, a liquid on the processor, one on the graphics
/// card, and a swarm.
pub fn stage6(reg: &Registry) -> Vec<(String, Scene)> {
    build_all(reg, &[fountain, wind_tunnel, small_pool, deep_pool, fireflies])
}

/// The five stage 7 scenes: soft bodies, captions, keyframes, a drum machine, a melody and beat
/// sync.
pub fn stage7(reg: &Registry) -> Vec<(String, Scene)> {
    build_all(reg, &[jelly_drop, rope_bridge, cloth_hammock, captioned_reel, drum_beat])
}

/// Every shipped scene, stage by stage.
pub fn all(reg: &Registry) -> Vec<(String, Scene)> {
    let mut scenes = stage2(reg);
    scenes.extend(stage3(reg));
    scenes.extend(stage4(reg));
    scenes.extend(stage5(reg));
    scenes.extend(stage6(reg));
    scenes.extend(stage7(reg));
    scenes
}

/// The scenes the Start window offers, in order: a spread of what the editor can do.
pub const STARTERS: [&str; 9] = [
    "Reel block",
    "Lava ball",
    "Spinning hexagon",
    "Peg board",
    "Long drop",
    "Deep pool",
    "Fireflies",
    "Jelly drop",
    "Drum beat",
];

/// The [`STARTERS`], each with its name. Long drop is not in [`all`], so the golden tests keep
/// the scenes they had.
pub fn starters(reg: &Registry) -> Vec<(String, Scene)> {
    let mut all = all(reg);
    all.push(("Long drop".into(), long_drop(reg)));
    STARTERS.iter().filter_map(|name| all.iter().position(|(n, _)| n == name).map(|i| all.swap_remove(i))).collect()
}

/// The camera scene: a peg course four screens tall, with the camera following the ball down,
/// punching in on hits and staying inside the course.
pub fn long_drop(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Long drop");
    b.scene_settings(|s| {
        s.gravity = Vec2::new(0.0, 900.0);
        s.off_shot_fade = 1.5;
    });
    // The course runs from just above the first screen to the bottom of the fourth.
    let course = Vec2::new(0.0, 2880.0);
    b.arena(|a| {
        a.shape = ArenaShape::Box;
        a.width = 1000.0;
        a.height = 7680.0;
        a.corner = 60.0;
    });
    b.scene.things.get_mut(&ARENA).expect("arena").transform.position = course;
    let pegs = b.add(
        "obstacle",
        ThingId::ROOT,
        "Pegs",
        course + Vec2::new(0.0, 200.0),
        obstacle(|o| {
            o.shape = ObstacleShape::PegGrid;
            o.columns = 8;
            o.rows = 60;
            o.spacing = 115.0;
            o.radius = 10.0;
            o.stagger = true;
        }),
    );
    b.tag(pegs, "pegs");
    let peg_sound = NoteSound {
        instrument: b.built_in(Instrument::Marimba),
        plays_when: PlaysWhen::BeingHit,
        order: Order::Speed,
        ..NoteSound::default()
    };
    b.own_module(pegs, "sound.notes", "Peg sound", peg_sound.to_params());
    b.balls("Ball", Vec2::new(30.0, -820.0), |s| {
        s.radius = 22.0;
        s.velocity = Vec2::new(60.0, 0.0);
    });
    b.scene.set_slot_off(BALLS, SlotId::Sound);
    let bumpy = BasicBounce { bounciness: 0.55, ..BasicBounce::default() };
    b.own_module(BALLS, "bounce.basic", "Bumpy", bumpy.to_params());
    b.text("Title", Vec2::new(0.0, -780.0), |t| {
        t.text = "Long drop".into();
        t.font = Font::Anton;
        t.size = 110.0;
        t.outline = 6.0;
    });
    let camera = crate::CameraSettings {
        position: Vec2::new(0.0, -480.0),
        follow: true,
        follow_target: Target::Thing(BALLS),
        smoothness: 0.5,
        look_ahead: 0.2,
        slack: 0.05,
        punch: 0.12,
        punch_recovery: 0.25,
        hits_from: Target::Thing(BALLS),
        stay_inside: true,
        box_size: Vec2::new(1080.0, 7680.0 + 120.0),
        box_centre: course,
        ..crate::CameraSettings::default()
    };
    b.add("camera", ThingId::ROOT, "Camera", Vec2::ZERO, camera.to_params());
    b.scene
}

/// A ready-made rule's builder, by its menu name.
fn recipe(name: &str) -> fn(&mut Graph) -> Vec<(u32, String)> {
    recipes().into_iter().find(|r| r.name == name).unwrap_or_else(|| panic!("no recipe called {name}")).add
}

/// The block of `kind` in a graph that has exactly one.
fn block(g: &Graph, kind: &str) -> u32 {
    g.nodes.iter().find(|n| n.kind == kind).unwrap_or_else(|| panic!("no {kind} block")).id
}

fn set(g: &mut Graph, node: u32, name: &str, p: Param) {
    g.node_mut(node).expect("a block of the graph").params.insert(name.into(), p);
}

fn wire(g: &mut Graph, role: Role, from: (u32, u8), to: (u32, u8)) {
    assert!(g.connect(Link { from, to }, catalog(role)), "{from:?} can't wire into {to:?}");
}

/// A new scene with a name, and handles on its things and shared modules.
struct Builder<'a> {
    reg: &'a Registry,
    scene: Scene,
}

impl<'a> Builder<'a> {
    fn new(reg: &'a Registry, name: &str) -> Builder<'a> {
        let mut scene = new_scene(reg);
        scene.name = name.into();
        Builder { reg, scene }
    }

    fn scene_settings(&mut self, f: impl FnOnce(&mut SceneSettings)) {
        let root = self.scene.things.get_mut(&ThingId::ROOT).expect("root");
        // Keep what the new scene set, such as its chords.
        let mut s = SceneSettings::from_params(&root.params);
        f(&mut s);
        root.params = s.to_params();
    }

    /// A library instrument playing the built-in `which` with its usual knobs, added the first
    /// time it is asked for.
    fn built_in(&mut self, which: Instrument) -> InstrumentRef {
        let found = self
            .scene
            .library
            .instruments
            .iter()
            .find(|(_, i)| matches!(&i.source, InstrumentSource::BuiltIn { which: w, .. } if w == which.name()));
        if let Some((&id, _)) = found {
            return InstrumentRef(Some(id));
        }
        InstrumentRef(Some(self.scene.add_instrument(crate::built_in_instrument(which))))
    }

    /// The library's copy of the starter patch `name`, added the first time it's asked for.
    fn starter(&mut self, name: &str) -> InstrumentRef {
        if let Some((&id, _)) = self.scene.library.instruments.iter().find(|(_, i)| i.name == name) {
            return InstrumentRef(Some(id));
        }
        let (name, graph, voices) =
            bpe_synth::starters().into_iter().find(|(n, ..)| *n == name).expect("a starter patch of that name");
        InstrumentRef(Some(self.scene.add_instrument(bpe_core::Instrument {
            name: name.into(),
            source: InstrumentSource::Patch { graph },
            voices,
        })))
    }

    /// Scene defaults' chords become the preset `name`, in the key on `root`.
    fn chords(&mut self, name: &str, root: u8) {
        let preset = bpe_mix::PRESETS.iter().find(|p| p.name == name).expect("a known preset");
        let a = own(ThingId::ROOT, "chords");
        let id = self.scene.value_at(self.reg, &a).and_then(|v| ProgressionRef::from_value(&v)).and_then(|r| r.0);
        let p = Progression {
            name: preset.name.into(),
            key: MusicKey { root, mode: preset.mode },
            chords: preset.chords(root),
            ..Progression::default()
        };
        self.scene.set_progression(id.expect("a new scene has chords"), p).expect("the progression is there");
    }

    fn arena(&mut self, f: impl FnOnce(&mut ArenaSettings)) {
        let mut s = ArenaSettings::default();
        f(&mut s);
        self.scene.things.get_mut(&ARENA).expect("arena").params = s.to_params();
    }

    fn balls(&mut self, name: &str, position: Vec2, f: impl FnOnce(&mut BallsSettings)) {
        let mut s = BallsSettings::default();
        f(&mut s);
        let thing = self.scene.things.get_mut(&BALLS).expect("balls");
        thing.name = name.into();
        thing.transform.position = position;
        thing.params = s.to_params();
    }

    /// The module linked into `slot` on `thing` (the root's shared one when the thing inherits).
    fn module_of(&self, thing: ThingId, slot: SlotId) -> ModuleId {
        let resolved = bpe_core::resolve_slot(&self.scene, self.reg, thing, slot, &mut Vec::new());
        resolved.expect("the new scene fills every slot").module
    }

    fn set_params(&mut self, m: ModuleId, params: bpe_core::Params) {
        self.scene.library.modules.get_mut(&m).expect("module").params = params;
    }

    fn root_look(&mut self, f: impl FnOnce(&mut ClassicLook)) {
        let mut s = ClassicLook::default();
        f(&mut s);
        let m = self.module_of(ThingId::ROOT, SlotId::Look);
        self.set_params(m, s.to_params());
    }

    fn ball_look(&mut self, f: impl FnOnce(&mut ClassicLook)) {
        let mut s = ClassicLook { color: Color::rgb(0xff, 0x5c, 0xa8), ..ClassicLook::default() };
        f(&mut s);
        let m = self.module_of(BALLS, SlotId::Look);
        self.set_params(m, s.to_params());
    }

    fn motion(&mut self, f: impl FnOnce(&mut BasicMotion)) {
        let mut s = BasicMotion::default();
        f(&mut s);
        let m = self.module_of(ThingId::ROOT, SlotId::Motion);
        self.set_params(m, s.to_params());
    }

    fn bounce(&mut self, f: impl FnOnce(&mut BasicBounce)) {
        let mut s = BasicBounce::default();
        f(&mut s);
        let m = self.module_of(ThingId::ROOT, SlotId::Bounce);
        self.set_params(m, s.to_params());
    }

    fn sound(&mut self, f: impl FnOnce(&mut NoteSound)) {
        let mut s = NoteSound::default();
        f(&mut s);
        let m = self.module_of(ThingId::ROOT, SlotId::Sound);
        self.set_params(m, s.to_params());
    }

    /// A new module linked onto one thing.
    fn own_module(&mut self, thing: ThingId, kind: &str, name: &str, params: bpe_core::Params) -> ModuleId {
        let m = self.scene.add_module(self.reg, kind, name).expect("kit kinds are registered");
        self.set_params(m, params);
        let slot = self.scene.library.modules[&m].slot;
        self.scene.link(self.reg, thing, slot, m).expect("the kind has the slot");
        m
    }

    /// A new thing of `kind` under `parent`, named and placed, with its settings, and the starter
    /// modules the kit gives that kind (a liquid's surface look, a swarm's flocking).
    fn add(&mut self, kind: &str, parent: ThingId, name: &str, position: Vec2, params: Params) -> ThingId {
        let id = crate::add_thing(&mut self.scene, self.reg, kind, parent).expect("kit kinds are registered");
        let thing = self.scene.things.get_mut(&id).expect("just added");
        thing.name = name.into();
        thing.transform.position = position;
        thing.params = params;
        id
    }

    fn soft(&mut self, name: &str, position: Vec2, f: impl FnOnce(&mut SoftSettings)) -> ThingId {
        let mut s = SoftSettings::default();
        f(&mut s);
        self.add("soft", ThingId::ROOT, name, position, s.to_params())
    }

    fn text(&mut self, name: &str, position: Vec2, f: impl FnOnce(&mut TextSettings)) -> ThingId {
        let mut s = TextSettings::default();
        f(&mut s);
        self.add("text", ThingId::ROOT, name, position, s.to_params())
    }

    /// Keys on one setting: (time, value, ease into the next).
    fn track(&mut self, a: Address, keys: &[(f64, Value, Ease)]) {
        for (time, value, ease) in keys {
            let i = self.scene.set_key(self.reg, &a, *time, value.clone()).expect("the setting takes keys");
            self.scene.set_ease(&a, i, *ease);
        }
    }

    fn pulse(&mut self, a: Address, source: PulseSource, amount: Value, decay: f32) {
        self.scene.set_pulse(self.reg, &a, source, amount, decay).expect("the setting pulses");
    }
    /// Makes `id` ride on `parent`.
    fn ride(&mut self, id: ThingId, parent: ThingId, offset: Vec2, turn_with_parent: bool) {
        self.scene.set_parent(id, parent, Attach::Ride { offset, turn_with_parent, item: None }).expect("no loops");
    }

    fn tag(&mut self, id: ThingId, tag: &str) {
        self.scene.things.get_mut(&id).expect("thing").tags.insert(tag.into());
    }

    fn turn(&mut self, id: ThingId, degrees: f32) {
        self.scene.things.get_mut(&id).expect("thing").transform.rotation = degrees.to_radians();
    }

    /// A Rules module on `thing`, with `build` adding its rules and naming the settings to show
    /// at Simple depth.
    fn rules(&mut self, thing: ThingId, name: &str, build: impl FnOnce(&mut Graph) -> Vec<(u32, String)>) {
        let m = self.scene.add_module(self.reg, "rules.graph", name).expect("rules are registered");
        let mut graph = RulesKind.template().expect("rules have a template");
        let simple = build(&mut graph).into_iter().collect();
        self.scene.library.modules.get_mut(&m).expect("module").graph =
            Some(ModuleGraph { graph, template: "rules.graph".into(), custom: true, simple });
        self.scene.link(self.reg, thing, SlotId::Rules, m).expect("the thing has a Rules slot");
    }

    /// Edits the root's shared module for `slot` as nodes, starting from its template with the
    /// module's settings in.
    fn custom(&mut self, slot: SlotId, edit: impl FnOnce(&mut Graph)) {
        let reg = self.reg;
        let m = self.module_of(ThingId::ROOT, slot);
        let module = self.scene.library.modules.get_mut(&m).expect("module");
        let kind = reg.module(&module.kind).expect("a registered kind");
        let mut g = module_graph(kind, module).expect("the kind has a template");
        edit(&mut g.graph);
        g.custom = true;
        module.graph = Some(g);
    }
}

fn own(thing: ThingId, key: &str) -> Address {
    Address { owner: Owner::Thing(thing), slot: None, key: key.into() }
}

fn of_module(m: ModuleId, key: &str) -> Address {
    Address { owner: Owner::Module(m), slot: None, key: key.into() }
}

fn obstacle(f: impl FnOnce(&mut ObstacleSettings)) -> Params {
    let mut s = ObstacleSettings::default();
    f(&mut s);
    s.to_params()
}

/// The new-scene default: a circle arena and one ball.
fn one_ball(reg: &Registry) -> Scene {
    Builder::new(reg, "One ball").scene
}

/// The reference reel: a block of 784 tiny balls in a big ring, coloured by row.
fn reel_block(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Reel block");
    b.scene_settings(|s| {
        s.gravity = Vec2::new(0.0, 160.0);
        s.duration = 23.0;
    });
    b.arena(|a| {
        a.radius = 508.0;
        a.thickness = 3.0;
    });
    b.balls("Block", Vec2::new(37.0, -127.0), |s| {
        s.arrangement = Arrangement::Block;
        s.columns = 28;
        s.rows = 28;
        s.spacing = 19.0;
        s.radius = 4.5;
        s.velocity = Vec2::new(-480.0, 265.0);
    });
    b.ball_look(|l| {
        l.gradient = vec![
            Color::rgb(0x3e, 0x9b, 0xff),
            Color::rgb(0x8a, 0x4d, 0xff),
            Color::rgb(0xff, 0x3e, 0xc8),
            Color::rgb(0xff, 0x6a, 0x5c),
            Color::rgb(0xff, 0xb8, 0x2e),
        ];
    });
    let arena_look = ClassicLook { color: Color::rgb(0xd8, 0xdc, 0xd0), ..ClassicLook::default() }.to_params();
    b.own_module(ARENA, "look.classic", "Arena look", arena_look);
    b.scene
}

/// A box with a block of balls dropped in, losing a little speed on every hit.
fn box_drop(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Box drop");
    b.scene_settings(|s| s.gravity = Vec2::new(0.0, 900.0));
    b.arena(|a| {
        a.shape = ArenaShape::Box;
        a.width = 900.0;
        a.height = 1500.0;
    });
    b.balls("Block", Vec2::new(0.0, -500.0), |s| {
        s.arrangement = Arrangement::Block;
        s.columns = 10;
        s.rows = 10;
        s.spacing = 40.0;
        s.radius = 12.0;
        s.velocity = Vec2::new(200.0, 0.0);
    });
    b.bounce(|s| s.bounciness = 0.85);
    b.motion(|s| s.drag = 0.05);
    b.scene
}

/// A turning hexagon with a ring of balls that gain a little speed on every hit.
fn spinning_hexagon(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Spinning hexagon");
    b.arena(|a| {
        a.shape = ArenaShape::Polygon;
        a.sides = 6;
        a.radius = 450.0;
        a.spin = 25.0;
    });
    b.balls("Ring", Vec2::ZERO, |s| {
        s.arrangement = Arrangement::Ring;
        s.count = 24;
        s.radius = 10.0;
        s.ring_radius = 200.0;
        s.velocity = Vec2::ZERO;
    });
    b.bounce(|s| {
        s.speed_gain = 1.02;
        s.top_speed = 2500.0;
    });
    b.root_look(|l| l.glow = 0.5);
    b.scene
}

/// A ring that closes in while one ball grows with every hit against a lively wall.
fn shrinking_ring(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Shrinking ring");
    b.arena(|a| {
        a.radius = 500.0;
        a.shrink = 0.03;
        a.min_size = 200.0;
    });
    b.balls("Ball", Vec2::new(0.0, -200.0), |s| {
        s.radius = 30.0;
        s.velocity = Vec2::new(600.0, -300.0);
    });
    b.bounce(|s| {
        s.grow = 1.5;
        s.max_size = 90.0;
    });
    let marimba = b.built_in(Instrument::Marimba);
    b.sound(|s| s.instrument = marimba);
    let lively = BasicBounce { bounciness: 1.05, takes_over: true, ..BasicBounce::default() }.to_params();
    b.own_module(ARENA, "bounce.basic", "Lively wall", lively);
    b.scene
}

// ---------- stage 7 ----------

/// A jelly dropped into a ring with a ball to knock it about.
fn jelly_drop(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Jelly drop");
    b.scene_settings(|s| s.gravity = Vec2::new(0.0, 900.0));
    b.arena(|a| {
        a.radius = 480.0;
        a.thickness = 4.0;
    });
    b.balls("Ball", Vec2::new(-200.0, -380.0), |s| {
        s.radius = 30.0;
        s.velocity = Vec2::new(300.0, 0.0);
    });
    let jelly = b.soft("Jelly", Vec2::new(0.0, -350.0), |s| {
        s.shape = SoftShape::Jelly;
        s.size = 110.0;
        s.softness = 0.6;
        s.velocity = Vec2::new(220.0, 0.0);
    });
    let look = ClassicLook { color: Color::rgb(0xff, 0xb0, 0x20), glow: 0.3, ..ClassicLook::default() };
    b.own_module(jelly, "look.classic", "Jelly look", look.to_params());
    b.scene
}

/// A rope slung across a box with balls dropped onto it.
fn rope_bridge(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Rope bridge");
    b.arena(|a| {
        a.shape = ArenaShape::Box;
        a.width = 900.0;
        a.height = 1500.0;
    });
    b.balls("Balls", Vec2::new(0.0, -700.0), |s| {
        s.arrangement = Arrangement::Block;
        s.columns = 4;
        s.rows = 1;
        s.spacing = 150.0;
        s.radius = 28.0;
        s.velocity = Vec2::ZERO;
    });
    b.ball_look(|l| l.gradient = vec![Color::rgb(0x3e, 0x9b, 0xff), Color::rgb(0xff, 0x3e, 0xc8)]);
    let rope = b.soft("Rope", Vec2::new(0.0, -100.0), |s| {
        s.shape = SoftShape::Rope;
        s.size = 760.0;
        s.softness = 0.3;
        s.detail = 40;
        s.thickness = 10.0;
        s.pinned = Pinned::BothEnds;
    });
    let look = ClassicLook { color: Color::rgb(0xe8, 0xe8, 0xe8), ..ClassicLook::default() };
    b.own_module(rope, "look.classic", "Rope look", look.to_params());
    let marimba = b.built_in(Instrument::Marimba);
    b.sound(|s| s.instrument = marimba);
    b.scene
}

/// A cloth slung from its two top corners, swinging as a ring of balls lands in it.
fn cloth_hammock(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Cloth hammock");
    b.scene_settings(|s| s.gravity = Vec2::new(0.0, 700.0));
    b.arena(|a| {
        a.shape = ArenaShape::Box;
        a.width = 900.0;
        a.height = 1800.0;
    });
    b.balls("Balls", Vec2::new(0.0, -800.0), |s| {
        s.arrangement = Arrangement::Ring;
        s.count = 5;
        s.ring_radius = 80.0;
        s.radius = 26.0;
        s.velocity = Vec2::ZERO;
    });
    let cloth = b.soft("Cloth", Vec2::new(0.0, -350.0), |s| {
        s.shape = SoftShape::Cloth;
        s.size = 700.0;
        s.height = 500.0;
        s.detail = 26;
        s.rows = 20;
        s.softness = 0.45;
        s.pinned = Pinned::TopCorners;
    });
    let look = ClassicLook { color: Color::rgb(0x3e, 0x9b, 0xff), ..ClassicLook::default() };
    b.own_module(cloth, "look.classic", "Cloth look", look.to_params());
    b.scene
}

/// The reel's ring with a block of balls, a hit counter that jumps on hits, a countdown, and
/// gravity, bloom and glow that grow over the video.
fn captioned_reel(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Captioned reel");
    b.scene_settings(|s| s.gravity = Vec2::new(0.0, 160.0));
    b.arena(|a| {
        a.radius = 508.0;
        a.thickness = 3.0;
    });
    b.balls("Block", Vec2::new(30.0, -120.0), |s| {
        s.arrangement = Arrangement::Block;
        s.columns = 14;
        s.rows = 14;
        s.spacing = 30.0;
        s.radius = 7.0;
        s.velocity = Vec2::new(-420.0, 240.0);
    });
    b.ball_look(|l| {
        l.gradient = vec![Color::rgb(0x3e, 0x9b, 0xff), Color::rgb(0xff, 0x3e, 0xc8), Color::rgb(0xff, 0xb8, 0x2e)];
    });
    let hits = b.text("Hit counter", Vec2::new(0.0, -780.0), |t| {
        t.text = "Hits {hits}".into();
        t.font = Font::Anton;
        t.size = 120.0;
        t.appear = Appear::Pop;
        t.outline = 6.0;
    });
    b.text("Countdown", Vec2::new(0.0, 760.0), |t| {
        t.text = "{countdown}".into();
        t.font = Font::SpaceMono;
        t.size = 150.0;
        t.align = Align::Center;
    });
    b.pulse(own(hits, "size"), PulseSource::Hits, Value::Number(24.0), 0.25);
    b.track(
        own(ThingId::ROOT, "gravity"),
        &[(0.0, Value::Vec2([0.0, 160.0]), Ease::Smooth), (12.0, Value::Vec2([0.0, 900.0]), Ease::Smooth)],
    );
    b.track(
        own(ThingId::ROOT, "bloom"),
        &[(0.0, Value::Number(0.15), Ease::Linear), (20.0, Value::Number(0.6), Ease::Linear)],
    );
    let ball_look = b.module_of(BALLS, SlotId::Look);
    b.track(
        of_module(ball_look, "glow"),
        &[(0.0, Value::Number(0.1), Ease::EaseIn), (20.0, Value::Number(1.2), Ease::EaseIn)],
    );
    b.scene
}

/// A spinning hexagon at 128 bpm: hits land on beats, the walls play drums and the beat keeps
/// time, while the ball plays a tune.
fn drum_beat(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Drum beat");
    b.scene_settings(|s| {
        s.bpm = 128.0;
        s.beat_sync = BeatSync::Warp;
        s.warp_strength = 0.5;
    });
    b.chords("Epic minor", 9);
    b.arena(|a| {
        a.shape = ArenaShape::Polygon;
        a.sides = 6;
        a.radius = 470.0;
        a.spin = 25.0;
    });
    b.balls("Ball", Vec2::new(0.0, -300.0), |s| {
        s.radius = 40.0;
        s.velocity = Vec2::new(380.0, -120.0);
    });
    let drums = DrumSound { piece: Piece::Auto, on_beat: Piece::Auto, beat_volume: -4.0, ..DrumSound::default() };
    b.own_module(ThingId::ROOT, "sound.drums", "Drum machine", drums.to_params());
    let melody = MelodySound {
        file: "builtin:twinkle".into(),
        instrument: b.starter("Warm pad"),
        volume: -2.0,
        ..MelodySound::default()
    };
    b.own_module(BALLS, "sound.melody", "Tune", melody.to_params());
    b.text("Clock", Vec2::new(0.0, 850.0), |t| {
        t.text = "{time}".into();
        t.font = Font::VarelaRound;
        t.size = 72.0;
        t.appear = Appear::Fade;
    });
    b.scene
}

/// A Plinko board: a staggered grid of pegs that sound when hit, and a shower of balls that
/// bump into each other on the way down.
fn peg_board(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Peg board");
    b.scene_settings(|s| s.gravity = Vec2::new(0.0, 900.0));
    b.arena(|a| {
        a.shape = ArenaShape::Box;
        a.width = 900.0;
        a.height = 1500.0;
        a.corner = 60.0;
    });
    let pegs = b.add(
        "obstacle",
        ThingId::ROOT,
        "Pegs",
        Vec2::new(0.0, 100.0),
        obstacle(|o| {
            o.shape = ObstacleShape::PegGrid;
            o.columns = 7;
            o.rows = 9;
            o.spacing = 100.0;
            o.radius = 9.0;
        }),
    );
    b.tag(pegs, "pegs");
    let peg_sound = NoteSound {
        instrument: b.built_in(Instrument::Marimba),
        plays_when: PlaysWhen::BeingHit,
        order: Order::Speed,
        ..NoteSound::default()
    };
    b.own_module(pegs, "sound.notes", "Peg sound", peg_sound.to_params());
    b.balls("Drops", Vec2::new(0.0, -650.0), |s| {
        s.arrangement = Arrangement::Block;
        s.columns = 8;
        s.rows = 6;
        s.spacing = 30.0;
        s.radius = 8.0;
        s.velocity = Vec2::ZERO;
    });
    let bumpy = BasicBounce { hit_balls: true, bounciness: 0.6, ..BasicBounce::default() };
    b.own_module(BALLS, "bounce.basic", "Bumpy", bumpy.to_params());
    // Only the pegs make a sound.
    b.scene.set_slot_off(BALLS, SlotId::Sound);
    b.ball_look(|l| {
        l.gradient = vec![Color::rgb(0xff, 0xd1, 0x4a), Color::rgb(0xff, 0x5c, 0xa8), Color::rgb(0x6a, 0x8d, 0xff)];
        l.gradient_by = GradientBy::Hits;
        l.gradient_full = 10.0;
    });
    b.scene
}

/// Three bars in a folder, turning together, with a ring of balls speeding up around them.
fn spinning_bars(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Spinning bars");
    b.arena(|a| a.radius = 500.0);
    let bars = b.add("folder", ThingId::ROOT, "Bars", Vec2::ZERO, Params::default());
    for (i, degrees) in [0.0f32, 60.0, 120.0].into_iter().enumerate() {
        let bar = b.add(
            "obstacle",
            bars,
            &format!("Bar {}", i + 1),
            Vec2::ZERO,
            obstacle(|o| {
                o.shape = ObstacleShape::Bar;
                o.length = 600.0;
                o.thickness = 10.0;
                o.spin = 40.0;
            }),
        );
        b.ride(bar, bars, Vec2::ZERO, true);
        b.turn(bar, degrees);
    }
    b.balls("Ring", Vec2::ZERO, |s| {
        s.arrangement = Arrangement::Ring;
        s.count = 18;
        s.radius = 12.0;
        s.ring_radius = 380.0;
        s.velocity = Vec2::ZERO;
    });
    b.bounce(|s| s.speed_gain = 1.01);
    let bar_look = ClassicLook { color: Color::rgb(0x7f, 0xe8, 0xff), glow: 0.3, ..ClassicLook::default() };
    b.own_module(bars, "look.classic", "Bar look", bar_look.to_params());
    b.scene
}

/// A capsule full of balls that pile up on each other, coloured by how often they have hit.
fn ball_pit(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Ball pit");
    b.scene_settings(|s| {
        s.gravity = Vec2::new(0.0, 1200.0);
        s.duration = 15.0;
    });
    b.arena(|a| {
        a.shape = ArenaShape::Capsule;
        a.width = 700.0;
        a.height = 1400.0;
    });
    b.balls("Pit", Vec2::new(0.0, -300.0), |s| {
        s.arrangement = Arrangement::Block;
        s.columns = 10;
        s.rows = 14;
        s.spacing = 48.0;
        s.radius = 20.0;
        s.velocity = Vec2::new(120.0, 0.0);
    });
    b.bounce(|s| {
        s.hit_balls = true;
        s.bounciness = 0.5;
        s.friction = 0.3;
    });
    b.ball_look(|l| {
        l.gradient = vec![Color::rgb(0x3e, 0x9b, 0xff), Color::rgb(0xff, 0x3e, 0xc8), Color::rgb(0xff, 0xb8, 0x2e)];
        l.gradient_by = GradientBy::Hits;
        l.gradient_full = 12.0;
    });
    b.sound(|s| s.min_gap = 0.03);
    b.scene
}

/// Balls and a peg riding a turning hexagon; the balls change hue with every hit and pop after
/// forty.
fn riders(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Riders");
    b.arena(|a| {
        a.shape = ArenaShape::Polygon;
        a.sides = 6;
        a.radius = 450.0;
        a.spin = 30.0;
    });
    b.balls("Rim riders", Vec2::ZERO, |s| {
        s.arrangement = Arrangement::Ring;
        s.count = 6;
        s.radius = 14.0;
        s.ring_radius = 60.0;
        s.velocity = Vec2::ZERO;
    });
    b.ride(BALLS, ARENA, Vec2::new(0.0, -330.0), true);
    let peg = b.add("obstacle", ARENA, "Peg", Vec2::ZERO, obstacle(|o| o.radius = 24.0));
    b.ride(peg, ARENA, Vec2::new(0.0, 200.0), true);
    b.bounce(|s| s.pops_after = 40);
    b.ball_look(|l| l.hue_per_hit = 12.0);
    b.scene
}

/// One ball blown sideways, playing a climbing note on every hit and barely bouncing off the
/// soft bar.
fn pitch_climb(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Pitch climb");
    b.balls("Ball", Vec2::new(0.0, -300.0), |s| {
        s.radius = 30.0;
        s.velocity = Vec2::new(350.0, 0.0);
    });
    let bar = b.add(
        "obstacle",
        ThingId::ROOT,
        "Soft bar",
        Vec2::new(0.0, 350.0),
        obstacle(|o| {
            o.shape = ObstacleShape::Bar;
            o.length = 500.0;
            o.thickness = 12.0;
            o.path = MovePath::Line;
            o.path_distance = Vec2::new(150.0, 0.0);
            o.path_seconds = 6.0;
        }),
    );
    b.tag(bar, "soft");
    b.bounce(|s| {
        s.special = true;
        s.special_target = Target::Tag("soft".into());
        s.special_bounciness = 0.3;
    });
    let bell = b.starter("Glass FM bell");
    b.sound(|s| {
        s.order = Order::Hits;
        s.instrument = bell;
    });
    b.chords("Dreamy loop", 9);
    b.motion(|s| s.wind = Vec2::new(60.0, 0.0));
    b.scene
}

/// Three balls that grow a little on every hit, counting their hits.
fn grow_on_hit(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Grow on hit");
    b.balls("Trio", Vec2::ZERO, |s| {
        s.arrangement = Arrangement::Ring;
        s.count = 3;
        s.ring_radius = 160.0;
        s.radius = 18.0;
        s.velocity = Vec2::new(420.0, -260.0);
    });
    b.rules(BALLS, "Grow", |g| {
        let mut simple = recipe("Grow on hit")(g);
        let grow = block(g, "do.grow");
        set(g, grow, "by", Param::Number(1.0));
        simple.extend(recipe("Count hits")(g));
        simple
    });
    b.scene
}

/// One ball that splits in two on every hit, until there are 64.
fn split_on_hit(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Split on hit");
    b.balls("Ball", Vec2::new(0.0, -200.0), |s| {
        s.radius = 40.0;
        s.velocity = Vec2::new(500.0, -200.0);
    });
    b.bounce(|s| s.bounciness = 1.02);
    b.rules(BALLS, "Split", |g| recipe("Split on hit")(g));
    b.scene
}

/// A block of balls, each tinted by how hard it last hit: blue for soft, red for hard.
fn colour_by_speed(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Colour by speed");
    b.scene_settings(|s| s.gravity = Vec2::new(0.0, 700.0));
    b.balls("Block", Vec2::new(0.0, -300.0), |s| {
        s.arrangement = Arrangement::Block;
        s.columns = 4;
        s.rows = 4;
        s.spacing = 70.0;
        s.radius = 22.0;
        s.velocity = Vec2::new(260.0, 0.0);
    });
    b.bounce(|s| s.bounciness = 0.95);
    b.rules(BALLS, "Colour by speed", |g| {
        let c = catalog(Role::Rules);
        let hit = g.add(c.spec("when.hit").unwrap(), [40.0, 40.0]);
        let part = g.add(c.spec("math").unwrap(), [260.0, 200.0]);
        set(g, part, "op", Param::Choice("divide".into()));
        set(g, part, "b", Param::Number(1500.0));
        wire(g, Role::Rules, (hit, 3), (part, 0));
        let hue = g.add(c.spec("clamp").unwrap(), [480.0, 200.0]);
        set(g, hue, "min", Param::Number(0.0));
        set(g, hue, "max", Param::Number(1.0));
        wire(g, Role::Rules, (part, 0), (hue, 0));
        let colour = g.add(c.spec("color.hsv").unwrap(), [700.0, 200.0]);
        wire(g, Role::Rules, (hue, 0), (colour, 0));
        let tint = g.add(c.spec("do.color").unwrap(), [920.0, 40.0]);
        wire(g, Role::Rules, (hit, 0), (tint, 0));
        wire(g, Role::Rules, (colour, 0), (tint, 2));
        vec![(tint, "amount".into())]
    });
    b.scene
}

/// A bounce graph that keeps more speed on slow hits than on fast ones, so the ball settles
/// into a steady bounce instead of dying or running away.
fn bouncy_when_slow(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Bouncy when slow");
    b.arena(|a| {
        a.shape = ArenaShape::Box;
        a.width = 900.0;
        a.height = 1400.0;
    });
    b.balls("Ball", Vec2::new(-200.0, -300.0), |s| {
        s.radius = 36.0;
        s.velocity = Vec2::new(350.0, 0.0);
    });
    b.custom(SlotId::Bounce, |g| {
        let c = catalog(Role::Bounce);
        let input = block(g, "bounce.in");
        let reflect = block(g, "reflect");
        g.links.retain(|l| l.to != (reflect, 2));
        let slow = g.add(c.spec("compare").unwrap(), [40.0, 640.0]);
        set(g, slow, "op", Param::Choice("less".into()));
        set(g, slow, "b", Param::Number(300.0));
        wire(g, Role::Bounce, (input, 4), (slow, 0));
        let keep = g.add(c.spec("choose").unwrap(), [300.0, 640.0]);
        set(g, keep, "then", Param::Number(1.15));
        set(g, keep, "else", Param::Number(0.9));
        wire(g, Role::Bounce, (slow, 0), (keep, 0));
        wire(g, Role::Bounce, (keep, 0), (reflect, 2));
    });
    b.scene
}

/// A motion graph with a wind that swings left and right, and a sound graph that walks up a
/// scale with each ball's hits.
fn wind_and_melody(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Wind and melody");
    b.scene_settings(|s| s.gravity = Vec2::new(0.0, 350.0));
    b.balls("Ring", Vec2::ZERO, |s| {
        s.arrangement = Arrangement::Ring;
        s.count = 6;
        s.ring_radius = 220.0;
        s.radius = 16.0;
        s.velocity = Vec2::new(0.0, -200.0);
    });
    let marimba = b.built_in(Instrument::Marimba);
    b.sound(|s| s.instrument = marimba);
    b.custom(SlotId::Motion, |g| {
        let c = catalog(Role::Motion);
        let input = block(g, "motion.in");
        let output = block(g, "motion.out");
        let from = g.links.iter().find(|l| l.to == (output, 0)).expect("the template wires the velocity").from;
        g.links.retain(|l| l.to != (output, 0));
        let faster = g.add(c.spec("math").unwrap(), [40.0, 800.0]);
        set(g, faster, "op", Param::Choice("multiply".into()));
        set(g, faster, "b", Param::Number(2.0));
        wire(g, Role::Motion, (input, 5), (faster, 0));
        let swing = g.add(c.spec("fn").unwrap(), [300.0, 800.0]);
        set(g, swing, "f", Param::Choice("sin".into()));
        wire(g, Role::Motion, (faster, 0), (swing, 0));
        let strength = g.add(c.spec("math").unwrap(), [560.0, 800.0]);
        set(g, strength, "op", Param::Choice("multiply".into()));
        set(g, strength, "b", Param::Number(400.0));
        wire(g, Role::Motion, (swing, 0), (strength, 0));
        let wind = g.add(c.spec("vec.make").unwrap(), [820.0, 800.0]);
        wire(g, Role::Motion, (strength, 0), (wind, 0));
        let over_dt = g.add(c.spec("vec.scale").unwrap(), [1080.0, 800.0]);
        wire(g, Role::Motion, (wind, 0), (over_dt, 0));
        wire(g, Role::Motion, (input, 4), (over_dt, 1));
        let blown = g.add(c.spec("vec.math").unwrap(), [1340.0, 500.0]);
        set(g, blown, "op", Param::Choice("add".into()));
        wire(g, Role::Motion, from, (blown, 0));
        wire(g, Role::Motion, (over_dt, 0), (blown, 1));
        wire(g, Role::Motion, (blown, 0), (output, 0));
    });
    b.custom(SlotId::Sound, |g| {
        let c = catalog(Role::Sound);
        let input = block(g, "sound.in");
        let output = block(g, "sound.out");
        g.links.retain(|l| l.to != (output, 1));
        let step = g.add(c.spec("math").unwrap(), [820.0, 360.0]);
        set(g, step, "op", Param::Choice("modulo".into()));
        set(g, step, "b", Param::Number(12.0));
        wire(g, Role::Sound, (input, 6), (step, 0));
        let key = g.add(c.spec("math").unwrap(), [1080.0, 360.0]);
        set(g, key, "op", Param::Choice("add".into()));
        set(g, key, "a", Param::Number(48.0));
        wire(g, Role::Sound, (step, 0), (key, 1));
        wire(g, Role::Sound, (key, 0), (output, 1));
    });
    b.scene
}

/// Drops shot up from a spout, falling back through each other inside a ring.
fn fountain(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Fountain");
    b.arena(|a| a.radius = 500.0);
    b.balls("Drops", Vec2::new(0.0, 380.0), |s| {
        s.radius = 9.0;
        s.velocity = Vec2::ZERO;
    });
    b.bounce(|s| s.bounciness = 0.9);
    b.ball_look(|l| {
        l.gradient = vec![Color::rgb(0x3e, 0xd8, 0xff), Color::rgb(0x8a, 0x4d, 0xff), Color::rgb(0xff, 0x5c, 0xa8)];
    });
    let spout = EmitterSettings {
        target: Target::Thing(BALLS),
        rate: 25.0,
        speed: 900.0,
        spread: 30.0,
        limit: 400,
        ..EmitterSettings::default()
    };
    b.add("emitter", ThingId::ROOT, "Spout", Vec2::new(0.0, 380.0), spout.to_params());
    b.scene
}

/// Light feathers blown across a tall box while heavy stones fall through a whirl that
/// catches everything.
fn wind_tunnel(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Wind tunnel");
    b.scene_settings(|s| {
        s.gravity = Vec2::new(0.0, 300.0);
        s.duration = 20.0;
    });
    b.arena(|a| {
        a.shape = ArenaShape::Box;
        a.width = 900.0;
        a.height = 1500.0;
    });
    b.balls("Feathers", Vec2::new(-220.0, -520.0), |s| {
        s.arrangement = Arrangement::Block;
        s.columns = 6;
        s.rows = 6;
        s.spacing = 22.0;
        s.radius = 6.0;
        s.velocity = Vec2::ZERO;
    });
    b.tag(BALLS, "light");
    let stones = BallsSettings {
        arrangement: Arrangement::Block,
        columns: 3,
        rows: 3,
        spacing: 60.0,
        radius: 16.0,
        velocity: Vec2::ZERO,
        ..BallsSettings::default()
    };
    let stones = b.add("balls", ThingId::ROOT, "Stones", Vec2::new(220.0, -520.0), stones.to_params());
    b.tag(stones, "heavy");
    let breeze = FieldSettings {
        kind: FieldKindChoice::Wind,
        direction: 0.0,
        strength: 900.0,
        shape: FieldShape::Everywhere,
        targets: Target::Tag("light".into()),
        ..FieldSettings::default()
    };
    b.add("field", ThingId::ROOT, "Breeze", Vec2::ZERO, breeze.to_params());
    let whirl = FieldSettings {
        kind: FieldKindChoice::Vortex,
        radius: 350.0,
        strength: 1200.0,
        targets: Target::Anything,
        ..FieldSettings::default()
    };
    b.add("field", ThingId::ROOT, "Whirl", Vec2::new(0.0, 200.0), whirl.to_params());
    b.scene
}

/// A ball dropped into a small pool that runs on the processor.
fn small_pool(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Small pool");
    b.scene_settings(|s| s.gravity = Vec2::new(0.0, 900.0));
    b.arena(|a| a.radius = 480.0);
    b.balls("Ball", Vec2::new(0.0, -500.0), |s| {
        s.radius = 40.0;
        s.velocity = Vec2::ZERO;
    });
    b.bounce(|s| s.bounciness = 0.6);
    let pool = LiquidSettings { count: 1500, fill: 0.4, ..LiquidSettings::default() };
    b.add("liquid", ThingId::ROOT, "Pool", Vec2::ZERO, pool.to_params());
    b.scene
}

/// A block of balls dropped into a deep pool of twelve thousand particles on the graphics card.
fn deep_pool(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Deep pool");
    b.scene_settings(|s| s.gravity = Vec2::new(0.0, 900.0));
    b.arena(|a| {
        a.shape = ArenaShape::Box;
        a.width = 900.0;
        a.height = 1500.0;
    });
    b.balls("Block", Vec2::new(0.0, -400.0), |s| {
        s.arrangement = Arrangement::Block;
        s.columns = 3;
        s.rows = 2;
        s.spacing = 90.0;
        s.radius = 30.0;
        s.velocity = Vec2::ZERO;
    });
    let deep = LiquidSettings { count: 12_000, ..LiquidSettings::default() };
    b.add("liquid", ThingId::ROOT, "Deep", Vec2::ZERO, deep.to_params());
    b.scene
}

/// Twenty thousand points of light drawn to one ball bouncing round a ring.
fn fireflies(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Fireflies");
    b.arena(|a| a.radius = 500.0);
    b.balls("Ball", Vec2::ZERO, |s| {
        s.radius = 30.0;
        s.velocity = Vec2::new(400.0, -300.0);
    });
    let swarm = SwarmSettings { count: 20_000, ..SwarmSettings::default() };
    let swarm = b.add("swarm", ThingId::ROOT, "Fireflies", Vec2::ZERO, swarm.to_params());
    let look = SwarmLook {
        colors_by: SwarmColors::Gradient,
        colors: vec![Color::rgb(0xff, 0xe0, 0x66), Color::rgb(0x8a, 0xff, 0x80), Color::rgb(0x40, 0xc8, 0xff)],
        ..SwarmLook::default()
    };
    let m = b.module_of(swarm, SlotId::Look);
    b.set_params(m, look.to_params());
    let m = b.module_of(swarm, SlotId::Motion);
    b.set_params(m, Flocking { attraction: 1.2, ..Flocking::default() }.to_params());
    b.scene
}

#[cfg(test)]
mod tests {
    use crate::{registry, scenes};

    #[test]
    fn starters_are_nine_named_scenes_from_the_collection() {
        let reg = registry();
        let starters = scenes::starters(&reg);
        assert_eq!(starters.len(), 9);
        let mut all: std::collections::BTreeMap<String, bpe_core::Scene> = scenes::all(&reg).into_iter().collect();
        all.insert("Long drop".into(), scenes::long_drop(&reg));
        let mut names = std::collections::BTreeSet::new();
        for (name, scene) in &starters {
            assert_eq!(all.get(name), Some(scene), "{name} is one of the shipped scenes");
            assert_eq!(&scene.name, name);
            assert!(names.insert(name.clone()), "{name} twice");
        }
        assert_eq!(starters[0].0, "Reel block", "the reel the first video was made from leads");
    }

    #[test]
    fn stage_scenes_resolve_and_round_trip() {
        let reg = registry();
        let scenes = scenes::all(&reg);
        assert_eq!(scenes.len(), 30);
        let in_order: Vec<String> = [
            scenes::stage2(&reg),
            scenes::stage3(&reg),
            scenes::stage4(&reg),
            scenes::stage5(&reg),
            scenes::stage6(&reg),
            scenes::stage7(&reg),
        ]
        .concat()
        .into_iter()
        .map(|(n, _)| n)
        .collect();
        let names: Vec<&str> = scenes.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, in_order);
        assert_eq!(names[..5], ["One ball", "Reel block", "Box drop", "Spinning hexagon", "Shrinking ring"]);
        assert_eq!(names[5..10], ["Peg board", "Spinning bars", "Ball pit", "Riders", "Pitch climb"]);
        assert_eq!(
            names[10..15],
            ["Grow on hit", "Split on hit", "Colour by speed", "Bouncy when slow", "Wind and melody"]
        );
        assert_eq!(names[15..20], ["Lava ball", "Confetti drop", "Spark block", "Kaleido hexagon", "Neon ring"]);
        assert_eq!(names[20..25], ["Fountain", "Wind tunnel", "Small pool", "Deep pool", "Fireflies"]);
        assert_eq!(names[25..], ["Jelly drop", "Rope bridge", "Cloth hammock", "Captioned reel", "Drum beat"]);
        let mut scenes = scenes;
        scenes.push(("Long drop".into(), scenes::long_drop(&reg)));
        for (name, scene) in &scenes {
            assert_eq!(&scene.name, name);
            assert!(crate::resolve_scene(scene, &reg).problems.is_empty(), "{name}");
            let text = bpe_core::save(scene);
            assert_eq!(&bpe_core::load(&text, &reg).unwrap(), scene, "{name}");
            if std::env::var_os("BPE_BLESS").is_some() {
                let dir = format!("{}/../../scenes/next", env!("CARGO_MANIFEST_DIR"));
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(format!("{dir}/{}.ron", name.to_lowercase().replace(' ', "-")), text).unwrap();
            }
        }
    }

    #[test]
    fn the_shipped_scene_files_match_the_code() {
        let reg = registry();
        let mut shipped = scenes::all(&reg);
        shipped.push(("Long drop".into(), scenes::long_drop(&reg)));
        for (name, scene) in shipped {
            let file = format!(
                "{}/../../scenes/next/{}.ron",
                env!("CARGO_MANIFEST_DIR"),
                name.to_lowercase().replace(' ', "-")
            );
            let text = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{file}: {e} (run with BPE_BLESS=1)"));
            assert_eq!(
                bpe_core::load(&text, &reg).unwrap(),
                scene,
                "{name}: scenes/next is stale, run with BPE_BLESS=1"
            );
        }
    }

    #[test]
    fn long_drop_keeps_the_ball_in_shot() {
        let kit = crate::kit();
        let scene = scenes::long_drop(&kit.registry);
        let resolved = crate::resolve_scene(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let mut world = bpe_runtime::World::build(&scene, &resolved, &kit.sims);
        let mut log = bpe_runtime::Log::default();
        let mut lowest = f32::MIN;
        for i in 1..=200 {
            world.advance_to(i as f64 * 0.1, &kit.sims, &mut log);
            let bodies = &world.bodies[&bpe_core::ThingId(2)];
            let p = bodies.position[0];
            lowest = lowest.max(p.y);
            assert!(
                world.camera.sees(world.video, p, -bodies.radius[0]),
                "at {:.1} s the ball at {p:?} left {:?}",
                i as f64 * 0.1,
                world.camera
            );
        }
        assert!(lowest > 2000.0, "the ball got down the course: {lowest}");
    }
}

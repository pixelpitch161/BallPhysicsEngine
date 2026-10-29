//! The scenes the stages ship with: the new-scene default and the ones that exercise every
//! setting. They double as the determinism test scenes.

use bpe_core::{Attach, Color, ModuleId, Params, Registry, Scene, Settings, SlotId, Target, ThingId};
use bpe_mix::Instrument;
use glam::Vec2;

use crate::modules::look::GradientBy;
use crate::modules::sound::{PitchBy, PlaysWhen};
use crate::{
    ArenaSettings, ArenaShape, Arrangement, BallsSettings, BasicBounce, BasicMotion, ClassicLook, MovePath, NoteSound,
    ObstacleSettings, ObstacleShape, SceneSettings, new_scene,
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

/// Every shipped scene, stage by stage.
pub fn all(reg: &Registry) -> Vec<(String, Scene)> {
    let mut scenes = stage2(reg);
    scenes.extend(stage3(reg));
    scenes
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
        let mut s = SceneSettings::default();
        f(&mut s);
        self.scene.things.get_mut(&ThingId::ROOT).expect("root").params = s.to_params();
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
    fn own_module(&mut self, thing: ThingId, kind: &str, name: &str, params: bpe_core::Params) {
        let m = self.scene.add_module(self.reg, kind, name).expect("kit kinds are registered");
        self.set_params(m, params);
        let slot = self.scene.library.modules[&m].slot;
        self.scene.link(self.reg, thing, slot, m).expect("the kind has the slot");
    }

    /// A new thing of `kind` under `parent`, named and placed, with its settings.
    fn add(&mut self, kind: &str, parent: ThingId, name: &str, position: Vec2, params: Params) -> ThingId {
        let id = self.scene.add_thing(self.reg, kind, parent).expect("kit kinds are registered");
        let thing = self.scene.things.get_mut(&id).expect("just added");
        thing.name = name.into();
        thing.transform.position = position;
        thing.params = params;
        id
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
    b.sound(|s| s.instrument = Instrument::Marimba);
    let lively = BasicBounce { bounciness: 1.05, takes_over: true, ..BasicBounce::default() }.to_params();
    b.own_module(ARENA, "bounce.basic", "Lively wall", lively);
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
        instrument: Instrument::Marimba,
        plays_when: PlaysWhen::BeingHit,
        pitch_by: PitchBy::Speed,
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
    b.sound(|s| {
        s.pitch_by = PitchBy::Hits;
        s.instrument = Instrument::Marimba;
    });
    b.motion(|s| s.wind = Vec2::new(60.0, 0.0));
    b.scene
}

#[cfg(test)]
mod tests {
    use crate::{registry, scenes};

    #[test]
    fn stage_scenes_resolve_and_round_trip() {
        let reg = registry();
        let scenes = scenes::all(&reg);
        assert_eq!(scenes.len(), 10);
        let names: Vec<&str> = scenes.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names[..5], ["One ball", "Reel block", "Box drop", "Spinning hexagon", "Shrinking ring"]);
        assert_eq!(names[5..], ["Peg board", "Spinning bars", "Ball pit", "Riders", "Pitch climb"]);
        for (name, scene) in &scenes {
            assert_eq!(&scene.name, name);
            assert!(bpe_core::resolve(scene, &reg).problems.is_empty(), "{name}");
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
        for (name, scene) in scenes::all(&reg) {
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
}

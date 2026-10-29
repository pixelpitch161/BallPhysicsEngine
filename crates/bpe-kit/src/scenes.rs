//! The scenes the studio ships with: the new-scene default, four more from stage 2 that
//! exercise every ball setting, and five from stage 6 with emitters, fields, liquids and a
//! swarm. They double as the determinism test scenes.

use bpe_core::{Color, ModuleId, Registry, Scene, Settings, SlotId, Target, ThingId};
use bpe_mix::Instrument;
use glam::Vec2;

use crate::{
    ArenaSettings, ArenaShape, Arrangement, BallsSettings, BasicBounce, BasicMotion, ClassicLook, EmitterSettings,
    FieldKindChoice, FieldSettings, FieldShape, Flocking, LiquidSettings, NoteSound, SceneSettings, SwarmColors,
    SwarmLook, SwarmSettings, new_scene,
};

const ARENA: ThingId = ThingId(1);
const BALLS: ThingId = ThingId(2);

/// The five stage 2 scenes, in order, each with its name.
pub fn stage2(reg: &Registry) -> Vec<(String, Scene)> {
    [one_ball, reel_block, box_drop, spinning_hexagon, shrinking_ring]
        .iter()
        .map(|build| {
            let scene = build(reg);
            (scene.name.clone(), scene)
        })
        .collect()
}

/// The five stage 6 scenes, in order, each with its name.
pub fn stage6(reg: &Registry) -> Vec<(String, Scene)> {
    [fountain, wind_tunnel, small_pool, deep_pool, fireflies]
        .iter()
        .map(|build| {
            let scene = build(reg);
            (scene.name.clone(), scene)
        })
        .collect()
}

/// Every shipped scene: stage 2's, then stage 6's.
pub fn all(reg: &Registry) -> Vec<(String, Scene)> {
    let mut scenes = stage2(reg);
    scenes.extend(stage6(reg));
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

    /// One more thing under the root, with the kit's starter modules for its kind.
    fn thing(&mut self, kind: &str, name: &str, position: Vec2, params: bpe_core::Params) -> ThingId {
        let id = crate::add_thing(&mut self.scene, self.reg, kind, ThingId::ROOT).expect("kit kinds are registered");
        let thing = self.scene.things.get_mut(&id).expect("just added");
        thing.name = name.into();
        thing.transform.position = position;
        thing.params = params;
        id
    }

    fn tag(&mut self, thing: ThingId, tag: &str) {
        self.scene.things.get_mut(&thing).expect("thing").tags.insert(tag.into());
    }

    /// A new module linked onto one thing.
    fn own_module(&mut self, thing: ThingId, kind: &str, name: &str, params: bpe_core::Params) {
        let m = self.scene.add_module(self.reg, kind, name).expect("kit kinds are registered");
        self.set_params(m, params);
        let slot = self.scene.library.modules[&m].slot;
        self.scene.link(self.reg, thing, slot, m).expect("the kind has the slot");
    }
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
    b.thing("emitter", "Spout", Vec2::new(0.0, 380.0), spout.to_params());
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
    let stones = b.thing("balls", "Stones", Vec2::new(220.0, -520.0), stones.to_params());
    b.tag(stones, "heavy");
    let breeze = FieldSettings {
        kind: FieldKindChoice::Wind,
        direction: 0.0,
        strength: 900.0,
        shape: FieldShape::Everywhere,
        targets: Target::Tag("light".into()),
        ..FieldSettings::default()
    };
    b.thing("field", "Breeze", Vec2::ZERO, breeze.to_params());
    let whirl = FieldSettings {
        kind: FieldKindChoice::Vortex,
        radius: 350.0,
        strength: 1200.0,
        targets: Target::Anything,
        ..FieldSettings::default()
    };
    b.thing("field", "Whirl", Vec2::new(0.0, 200.0), whirl.to_params());
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
    b.thing("liquid", "Pool", Vec2::ZERO, pool.to_params());
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
    b.thing("liquid", "Deep", Vec2::ZERO, deep.to_params());
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
    let swarm = b.thing("swarm", "Fireflies", Vec2::ZERO, swarm.to_params());
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
    fn stage_scenes_resolve_and_round_trip() {
        let reg = registry();
        let scenes = scenes::all(&reg);
        assert_eq!(scenes.len(), 10);
        let names: Vec<&str> = scenes.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "One ball",
                "Reel block",
                "Box drop",
                "Spinning hexagon",
                "Shrinking ring",
                "Fountain",
                "Wind tunnel",
                "Small pool",
                "Deep pool",
                "Fireflies"
            ]
        );
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

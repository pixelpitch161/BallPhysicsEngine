//! The scenes this stage ships with: the new-scene default and four more that exercise every
//! setting. They double as the determinism test scenes.

#[path = "scenes_stage5.rs"]
mod stage5;
pub use stage5::{stage5, stage5_with_twins};

use bpe_core::{Color, ModuleId, Registry, Scene, Settings, SlotId, ThingId};
use bpe_mix::Instrument;
use glam::Vec2;

use crate::{
    ArenaSettings, ArenaShape, Arrangement, BallsSettings, BasicBounce, BasicMotion, ClassicLook, NoteSound,
    SceneSettings, new_scene,
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

#[cfg(test)]
mod tests {
    use crate::{registry, scenes};

    #[test]
    fn stage_scenes_resolve_and_round_trip() {
        let reg = registry();
        let scenes = scenes::stage2(&reg);
        assert_eq!(scenes.len(), 5);
        let names: Vec<&str> = scenes.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["One ball", "Reel block", "Box drop", "Spinning hexagon", "Shrinking ring"]);
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
        for (name, scene) in scenes::stage2(&reg) {
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

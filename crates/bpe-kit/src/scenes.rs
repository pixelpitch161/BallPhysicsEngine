//! The scenes this stage ships with: the new-scene default and four more that exercise every
//! setting. They double as the determinism test scenes.

#[path = "scenes_stage5.rs"]
mod stage5;
pub use stage5::{stage5, stage5_with_twins};

use bpe_core::{Color, ModuleGraph, ModuleId, ModuleKind, Registry, Scene, Settings, SlotId, ThingId};
use bpe_graph::{Catalog, Graph, Link, Param};
use bpe_mix::Instrument;
use bpe_vm::{Role, catalog};
use glam::Vec2;

use crate::graphs::module_graph;
use crate::{
    ArenaSettings, ArenaShape, Arrangement, BallsSettings, BasicBounce, BasicMotion, ClassicLook, NoteSound, RulesKind,
    SceneSettings, new_scene, recipes,
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

/// The five stage 4 scenes: rules on things, and modules whose graphs have been edited.
pub fn stage4(reg: &Registry) -> Vec<(String, Scene)> {
    [grow_on_hit, split_on_hit, colour_by_speed, bouncy_when_slow, wind_and_melody]
        .iter()
        .map(|build| {
            let scene = build(reg);
            (scene.name.clone(), scene)
        })
        .collect()
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
    b.sound(|s| s.instrument = Instrument::Marimba);
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

#[cfg(test)]
mod tests {
    use crate::{registry, scenes};

    fn all(reg: &bpe_core::Registry) -> Vec<(String, bpe_core::Scene)> {
        scenes::stage2(reg).into_iter().chain(scenes::stage4(reg)).collect()
    }

    /// Stage 4 scenes are filed under a stage4- prefix.
    fn file_name(name: &str) -> String {
        let base = name.to_lowercase().replace(' ', "-");
        let stage4 = ["grow-on-hit", "split-on-hit", "colour-by-speed", "bouncy-when-slow", "wind-and-melody"];
        if stage4.contains(&base.as_str()) { format!("stage4-{base}") } else { base }
    }

    #[test]
    fn stage_scenes_resolve_and_round_trip() {
        let reg = registry();
        let scenes = all(&reg);
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
                "Grow on hit",
                "Split on hit",
                "Colour by speed",
                "Bouncy when slow",
                "Wind and melody",
            ]
        );
        for (name, scene) in &scenes {
            assert_eq!(&scene.name, name);
            assert!(crate::resolve_scene(scene, &reg).problems.is_empty(), "{name}");
            let text = bpe_core::save(scene);
            assert_eq!(&bpe_core::load(&text, &reg).unwrap(), scene, "{name}");
            if std::env::var_os("BPE_BLESS").is_some() {
                let dir = format!("{}/../../scenes/next", env!("CARGO_MANIFEST_DIR"));
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(format!("{dir}/{}.ron", file_name(name)), text).unwrap();
            }
        }
    }

    #[test]
    fn the_shipped_scene_files_match_the_code() {
        let reg = registry();
        for (name, scene) in all(&reg) {
            let file = format!("{}/../../scenes/next/{}.ron", env!("CARGO_MANIFEST_DIR"), file_name(&name));
            let text = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{file}: {e} (run with BPE_BLESS=1)"));
            assert_eq!(
                bpe_core::load(&text, &reg).unwrap(),
                scene,
                "{name}: scenes/next is stale, run with BPE_BLESS=1"
            );
        }
    }
}

//! The scenes the stages ship with: the new-scene default and others that exercise every
//! setting. They double as the determinism test scenes.

#[path = "scenes_stage5.rs"]
mod stage5;
pub use stage5::{stage5, stage5_with_twins};

use bpe_core::{Address, Color, Ease, ModuleId, Owner, PulseSource, Registry, Scene, Settings, SlotId, ThingId, Value};
use bpe_mix::Instrument;
use glam::Vec2;

use crate::{
    Align, Appear, ArenaSettings, ArenaShape, Arrangement, BallsSettings, BasicBounce, BasicMotion, BeatSync,
    ClassicLook, DrumSound, Font, MelodySound, NoteSound, Piece, Pinned, SceneSettings, SoftSettings, SoftShape,
    TextSettings, new_scene,
};

const ARENA: ThingId = ThingId(1);
const BALLS: ThingId = ThingId(2);

/// Every shipped scene: stage 2's, then stage 5's, then stage 7's.
pub fn all(reg: &Registry) -> Vec<(String, Scene)> {
    let mut scenes = stage2(reg);
    scenes.extend(stage5(reg));
    scenes.extend(stage7(reg));
    scenes
}

/// The five stage 7 scenes, in order, each with its name: soft bodies, captions, keyframes,
/// a drum machine, a melody and beat sync.
pub fn stage7(reg: &Registry) -> Vec<(String, Scene)> {
    [jelly_drop, rope_bridge, cloth_hammock, captioned_reel, drum_beat]
        .iter()
        .map(|build| {
            let scene = build(reg);
            (scene.name.clone(), scene)
        })
        .collect()
}

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
    fn own_module(&mut self, thing: ThingId, kind: &str, name: &str, params: bpe_core::Params) -> ModuleId {
        let m = self.scene.add_module(self.reg, kind, name).expect("kit kinds are registered");
        self.set_params(m, params);
        let slot = self.scene.library.modules[&m].slot;
        self.scene.link(self.reg, thing, slot, m).expect("the kind has the slot");
        m
    }

    /// A new thing of `kind` under the root, placed and set up.
    fn thing(&mut self, kind: &str, name: &str, position: Vec2, params: bpe_core::Params) -> ThingId {
        let id = self.scene.add_thing(self.reg, kind, ThingId::ROOT).expect("kit kinds are registered");
        let thing = self.scene.things.get_mut(&id).expect("just added");
        thing.name = name.into();
        thing.transform.position = position;
        thing.params = params;
        id
    }

    fn soft(&mut self, name: &str, position: Vec2, f: impl FnOnce(&mut SoftSettings)) -> ThingId {
        let mut s = SoftSettings::default();
        f(&mut s);
        self.thing("soft", name, position, s.to_params())
    }

    fn text(&mut self, name: &str, position: Vec2, f: impl FnOnce(&mut TextSettings)) -> ThingId {
        let mut s = TextSettings::default();
        f(&mut s);
        self.thing("text", name, position, s.to_params())
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
}

fn own(thing: ThingId, key: &str) -> Address {
    Address { owner: Owner::Thing(thing), slot: None, key: key.into() }
}

fn of_module(m: ModuleId, key: &str) -> Address {
    Address { owner: Owner::Module(m), slot: None, key: key.into() }
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
    b.sound(|s| s.instrument = Instrument::Marimba);
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
        instrument: Instrument::Marimba,
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

#[cfg(test)]
mod tests {
    use crate::{registry, scenes};

    #[test]
    fn stage_scenes_resolve_and_round_trip() {
        let reg = registry();
        let scenes = scenes::all(&reg);
        let in_order: Vec<String> = [scenes::stage2(&reg), scenes::stage5(&reg), scenes::stage7(&reg)]
            .concat()
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        let names: Vec<&str> = scenes.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, in_order);
        assert_eq!(&names[..5], ["One ball", "Reel block", "Box drop", "Spinning hexagon", "Shrinking ring"]);
        assert_eq!(
            &names[names.len() - 5..],
            ["Jelly drop", "Rope bridge", "Cloth hammock", "Captioned reel", "Drum beat"]
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

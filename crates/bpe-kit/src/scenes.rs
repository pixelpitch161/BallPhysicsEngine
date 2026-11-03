//! The scenes the stages ship with: the new-scene default and the ones that exercise every
//! setting. They double as the determinism test scenes.

#[path = "scenes_stage5.rs"]
mod stage5;
pub use stage5::{stage5, stage5_with_twins};

use bpe_core::{
    Address, Attach, Blend, Color, Ease, Gradient, ModuleId, Owner, Params, Registry, Scene, Settings, SlotId,
    SlotLink, Target, ThingId, Value,
};
use glam::Vec2;
use std::collections::BTreeMap;

use crate::paint::{Follow, InsideFrom, InsidePaint, Paint, Pattern, PlaceShape, Repeat};
use crate::{
    ArenaSettings, ArenaShape, Arrangement, BallsSettings, BasicBounce, BasicLook, BasicMotion, BurstSource,
    HitBurst, SceneSettings, StreakTrail, Style, TrailSource, new_scene,
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

/// The stage 3 scenes: balls hitting balls in a rounded arena, and balls riding a wall.
pub fn stage3(reg: &Registry) -> Vec<(String, Scene)> {
    build_all(reg, &[ball_pit, riders])
}

/// Every shipped scene, stage by stage.
pub fn all(reg: &Registry) -> Vec<(String, Scene)> {
    let mut scenes = stage2(reg);
    scenes.extend(stage3(reg));
    scenes.extend(stage5(reg));
    scenes.push(("Colour paints".into(), colour_demo(reg)));
    scenes.push(("Blend modes".into(), blend_demo(reg)));
    scenes.push(("Graded sunset".into(), graded_sunset(reg)));
    scenes
}

/// The scenes the Start window offers, in order: a spread of what the editor can do.
pub const STARTERS: [&str; 6] =
    ["Reel block", "Lava ball", "Spinning hexagon", "Ball pit", "Long drop", "Colour paints"];

/// The [`STARTERS`], each with its name. Long drop is not in [`all`], so the golden tests keep
/// the scenes they had.
pub fn starters(reg: &Registry) -> Vec<(String, Scene)> {
    let mut all = all(reg);
    all.push(("Long drop".into(), long_drop(reg)));
    STARTERS.iter().filter_map(|name| all.iter().position(|(n, _)| n == name).map(|i| all.swap_remove(i))).collect()
}

/// The camera scene: a box four screens tall, with the camera following the ball down,
/// punching in on hits and staying inside the box.
pub fn long_drop(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Long drop");
    b.scene_settings(|s| {
        s.gravity = Vec2::new(0.0, 900.0);
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
    b.balls("Ball", Vec2::new(30.0, -820.0), |s| {
        s.radius = 22.0;
        s.velocity = Vec2::new(60.0, 0.0);
    });
    let bumpy = BasicBounce { bounciness: 0.55, ..BasicBounce::default() };
    b.own_module(BALLS, "bounce.basic", "Bumpy", bumpy.to_params());
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
    b.finish()
}

/// The colour showcase: a block of balls painted by where they are at an angle with rings of the
/// same gradient inside each one, a ring whose colours run round its path, streaks and sparks
/// that change colour over their life, and a background glowing from the centre.
pub fn colour_demo(reg: &Registry) -> Scene {
    let hex = |h: u32| Color::rgb((h >> 16) as u8, (h >> 8) as u8, h as u8);
    let mut b = Builder::new(reg, "Colour paints");
    b.scene_settings(|s| {
        s.gravity = Vec2::new(0.0, 700.0);
        s.bloom = 0.35;
        s.background = Paint {
            colours: Gradient::even(&[hex(0x2b1458), hex(0x140a33), hex(0x05040f)]),
            shape: PlaceShape::Round,
            span: 1150.0,
            ..Paint::default()
        };
    });
    b.arena(|a| {
        a.radius = 470.0;
        a.thickness = 12.0;
    });
    let neon = [hex(0xff3ec8), hex(0xffb82e), hex(0x3ee6ff), hex(0x8a4dff), hex(0xff3ec8)];
    let ring = BasicLook {
        fill: Paint {
            colours: Gradient::even(&neon),
            follow: Follow::Path,
            repeat: Repeat::Repeat,
            scroll: 0.12,
            ..Paint::default()
        },
        glow: 0.7,
        ..BasicLook::default()
    };
    b.own_module(ARENA, "look.basic", "Ring look", ring.to_params());
    b.balls("Block", Vec2::new(-20.0, -170.0), |s| {
        s.arrangement = Arrangement::Block;
        s.columns = 8;
        s.rows = 8;
        s.spacing = 46.0;
        s.radius = 19.0;
        s.velocity = Vec2::new(-420.0, 220.0);
    });
    b.ball_look(|l| {
        let colours = Gradient::even(&[hex(0xffd23e), hex(0xff6a5c), hex(0xff3ec8), hex(0x8a4dff), hex(0x3e9bff)]);
        l.fill =
            Paint { colours: colours.clone(), angle: 45.0, span: 700.0, repeat: Repeat::Mirror, ..Paint::default() };
        l.inside = InsidePaint {
            pattern: Pattern::Rings,
            from: InsideFrom::OwnGradient,
            colours,
            repeat: Repeat::Mirror,
            angle: 0.0,
            size: 1.2,
            scroll: 0.25,
            ..InsidePaint::default()
        };
        l.glow = 0.3;
    });
    let streak = StreakTrail {
        seconds: 0.35,
        width: 0.55,
        brightness: 0.45,
        source: TrailSource::Own,
        colour: Paint {
            colours: Gradient::even(&[hex(0xffffff), hex(0x7cf4ff), hex(0x8a4dff), hex(0x2b1458)]),
            follow: Follow::Life,
            ..Paint::default()
        },
        ..StreakTrail::default()
    };
    b.own_module(BALLS, "trail.streak", "Life streak", streak.to_params());
    let sparks = HitBurst {
        count: 6,
        speed: 360.0,
        size: 3.0,
        life: 0.55,
        least_speed: 160.0,
        source: BurstSource::Own,
        colour: Paint {
            colours: Gradient::even(&[hex(0xffffff), hex(0xffd23e), hex(0xff6a2e), hex(0xc8104a)]),
            follow: Follow::Life,
            ..Paint::default()
        },
        ..HitBurst::default()
    };
    b.own_module(BALLS, "particles.burst", "Life sparks", sparks.to_params());
    b.finish()
}

/// The blend showcase over a background that runs from dark to light: four rows of overlapping
/// balls (Multiply, Screen, Overlay, Difference), a bar that subtracts, streaks that add, and a
/// group of styled balls whose hues drift from ball to ball and over time.
pub fn blend_demo(reg: &Registry) -> Scene {
    let hex = |h: u32| Color::rgb((h >> 16) as u8, (h >> 8) as u8, h as u8);
    let mut b = Builder::new(reg, "Blend modes");
    b.scene_settings(|s| {
        s.gravity = Vec2::ZERO;
        s.background = Paint {
            colours: Gradient::even(&[hex(0x0b1030), hex(0x3a4a8c), hex(0xd9a38c), hex(0xf4ecd8)]),
            angle: 90.0,
            span: 1500.0,
            ..Paint::default()
        };
    });
    b.arena(|a| {
        a.shape = ArenaShape::Box;
        a.width = 1000.0;
        a.height = 1400.0;
        a.thickness = 8.0;
    });
    let rim = BasicLook { fill: Paint::solid(hex(0xffffff)), ..BasicLook::default() };
    b.own_module(ARENA, "look.basic", "Rim look", rim.to_params());
    // Screen suits the dark top, Multiply the light bottom.
    let rows = [
        ("Screen", Blend::Screen, [0x3ee6ff, 0x4d7bff, 0xff3ec8], -170.0),
        ("Difference", Blend::Difference, [0xffffff, 0xffb82e, 0x3ee6ff], -130.0),
        ("Overlay", Blend::Overlay, [0xffd23e, 0x3eff9a, 0x3e9bff], 190.0),
        ("Multiply", Blend::Multiply, [0xff9a3e, 0xff4f7b, 0x8a4dff], 150.0),
    ];
    for (i, (name, blend, colours, vx)) in rows.into_iter().enumerate() {
        let y = -470.0 + 190.0 * i as f32;
        let id = b.add(
            "balls",
            ThingId::ROOT,
            name,
            Vec2::new(-190.0, y),
            BallsSettings {
                arrangement: Arrangement::Block,
                columns: 5,
                rows: 1,
                spacing: 95.0,
                radius: 70.0,
                velocity: Vec2::new(vx, 0.0),
                ..BallsSettings::default()
            }
            .to_params(),
        );
        let look = BasicLook {
            fill: Paint { colours: Gradient::even(&colours.map(hex)), follow: Follow::Order, ..Paint::default() },
            blend,
            ..BasicLook::default()
        };
        b.own_module(id, "look.basic", &format!("{name} look"), look.to_params());
    }
    // Streaks that add their light.
    let comets = b.add(
        "balls",
        ThingId::ROOT,
        "Comets",
        Vec2::new(0.0, 450.0),
        BallsSettings {
            arrangement: Arrangement::Ring,
            count: 3,
            ring_radius: 60.0,
            radius: 14.0,
            velocity: Vec2::new(520.0, 380.0),
            ..BallsSettings::default()
        }
        .to_params(),
    );
    let comet_look = BasicLook { fill: Paint::solid(hex(0xffffff)), glow: 0.6, ..BasicLook::default() };
    b.own_module(comets, "look.basic", "Comet look", comet_look.to_params());
    let streak = StreakTrail {
        seconds: 0.6,
        width: 0.8,
        brightness: 0.9,
        source: TrailSource::Own,
        colour: Paint {
            colours: Gradient::even(&[hex(0xffffff), hex(0x7cf4ff), hex(0xff3ec8)]),
            follow: Follow::Life,
            ..Paint::default()
        },
        blend: Blend::Add,
        ..StreakTrail::default()
    };
    b.own_module(comets, "trail.streak", "Add streak", streak.to_params());
    // Styled balls whose hue drifts around one colour.
    b.balls("Drifters", Vec2::new(-260.0, 600.0), |s| {
        s.arrangement = Arrangement::Block;
        s.columns = 8;
        s.rows = 2;
        s.spacing = 52.0;
        s.radius = 22.0;
        s.velocity = Vec2::new(160.0, -90.0);
    });
    let drift = BasicLook {
        fill: Paint { colours: Gradient::even(&[hex(0xff7a3e)]), vary_hue: 25.0, wobble: 0.5, ..Paint::default() },
        style: Style::Shiny,
        ..BasicLook::default()
    };
    b.own_module(BALLS, "look.basic", "Drift look", drift.to_params());
    b.finish()
}

/// The palette and grade showcase: balls painted from the Sunset palette in a ring whose wall
/// wears the Triad colours that match it, a cluster of glowing balls that Screen onto each other,
/// and a warm, contrasty grade with a vignette whose Sunset gradient map comes in over the video.
pub fn graded_sunset(reg: &Registry) -> Scene {
    use crate::paint::harmony::{Harmony, add_matching};
    let hex = |h: u32| Color::rgb((h >> 16) as u8, (h >> 8) as u8, h as u8);
    let sunset = crate::paint::palettes::built_in()
        .into_iter()
        .find(|p| p.name == "Sunset")
        .expect("Sunset is a built-in palette")
        .colours;
    let mut b = Builder::new(reg, "Graded sunset");
    let duration = 20.0;
    b.scene_settings(|s| {
        s.duration = duration;
        s.gravity = Vec2::new(0.0, 600.0);
        s.bloom = 0.3;
        s.background = Paint {
            colours: Gradient::even(&[hex(0x1a0b3a), hex(0x2b1055), hex(0x4a2a7a)]),
            angle: 90.0,
            span: 1900.0,
            ..Paint::default()
        };
        s.grade_warmth = 0.3;
        s.grade_contrast = 0.2;
        s.grade_vignette = 0.4;
        s.grade_map = sunset.clone();
        s.grade_map_amount = 0.0;
    });
    b.arena(|a| {
        a.radius = 470.0;
        a.thickness = 14.0;
    });
    // The wall: the Sunset coral and the two colours a third of the way round the wheel from it.
    let coral = Gradient::even(&[sunset.stops[3].color]);
    let wall = BasicLook {
        fill: Paint {
            colours: add_matching(&coral, Harmony::Triad),
            follow: Follow::Path,
            repeat: Repeat::Mirror,
            scroll: 0.08,
            ..Paint::default()
        },
        glow: 0.5,
        ..BasicLook::default()
    };
    b.own_module(ARENA, "look.basic", "Wall look", wall.to_params());
    b.balls("Sunset balls", Vec2::new(0.0, -150.0), |s| {
        s.arrangement = Arrangement::Block;
        s.columns = 7;
        s.rows = 5;
        s.spacing = 52.0;
        s.radius = 22.0;
        s.velocity = Vec2::new(300.0, -120.0);
    });
    b.ball_look(|l| {
        l.fill = Paint { colours: sunset.clone(), follow: Follow::Order, ..Paint::default() };
        l.glow = 0.2;
    });
    // Overlapping glow balls that Screen onto each other as well as onto the scene, so where
    // they cross they brighten.
    let glow = b.add(
        "balls",
        ThingId::ROOT,
        "Glow balls",
        Vec2::new(0.0, 230.0),
        BallsSettings {
            arrangement: Arrangement::Ring,
            count: 5,
            ring_radius: 50.0,
            radius: 55.0,
            velocity: Vec2::new(-260.0, -380.0),
            ..BallsSettings::default()
        }
        .to_params(),
    );
    let screen = BasicLook {
        fill: Paint { colours: sunset.clone(), follow: Follow::Order, ..Paint::default() },
        glow: 0.0,
        blend: Blend::Screen,
        blend_each: true,
        ..BasicLook::default()
    };
    b.own_module(glow, "look.basic", "Screen look", screen.to_params());
    b.track(
        own(ThingId::ROOT, "grade_map_amount"),
        &[(0.0, Value::Number(0.0), Ease::Linear), (duration as f64, Value::Number(0.5), Ease::Linear)],
    );
    b.finish()
}

/// A new scene with a name, and handles on its things and the modules its things share.
/// Gives every thing its own modules: a module several things use becomes one copy each (with
/// the same settings and keyframes), and one only a single thing uses becomes that thing's.
/// Modules nothing uses are left in the library.
pub(crate) fn own_everything(scene: &mut Scene) {
    let links: Vec<(ThingId, SlotId, ModuleId)> = scene
        .order
        .iter()
        .flat_map(|id| {
            scene.things[id].slots.iter().filter_map(move |(slot, link)| match link {
                SlotLink::Linked { module, .. } => Some((*id, *slot, *module)),
                _ => None,
            })
        })
        .collect();
    let shared: std::collections::BTreeSet<ModuleId> =
        links.iter().map(|l| l.2).filter(|m| scene.users_of(*m).len() > 1).collect();
    let mut emptied = Vec::new();
    for (id, slot, m) in links {
        let Some(module) = scene.library.modules.get(&m) else { continue };
        if module.owner == Some(id) {
            continue;
        }
        if !shared.contains(&m) && module.owner.is_none() {
            scene.library.modules.get_mut(&m).expect("checked").owner = Some(id);
            continue;
        }
        scene.own_linked(id, slot).expect("the slot is linked");
        if scene.users_of(m).is_empty() {
            emptied.push(m);
        }
    }
    for m in emptied {
        let _ = scene.remove_module(m);
    }
}

struct Builder<'a> {
    reg: &'a Registry,
    scene: Scene,
    /// One module per slot that every thing of a fitting kind uses instead of its own starter,
    /// so a scene's looks, motion, bounce and sound can be set in one place.
    shared: BTreeMap<SlotId, ModuleId>,
}

impl<'a> Builder<'a> {
    fn new(reg: &'a Registry, name: &str) -> Builder<'a> {
        let mut scene = new_scene(reg);
        scene.name = name.into();
        let mut shared = BTreeMap::new();
        for (slot, kind, module_name) in [
            (SlotId::Look, "look.basic", "Plain look"),
            (SlotId::Motion, "motion.basic", "Default motion"),
            (SlotId::Bounce, "bounce.basic", "Default bounce"),
        ] {
            let m = scene.add_module(reg, kind, module_name).expect("kit kinds are registered");
            shared.insert(slot, m);
        }
        let mut b = Builder { reg, scene, shared };
        b.share(ARENA, &SlotId::ALL);
        b.share(BALLS, &[SlotId::Motion, SlotId::Bounce]);
        b
    }

    /// Swaps `thing`'s own starters in `slots` for the shared modules of the same kind, and
    /// drops the starters.
    fn share(&mut self, thing: ThingId, slots: &[SlotId]) {
        for slot in slots {
            let Some(&shared) = self.shared.get(slot) else { continue };
            let Some(SlotLink::Linked { module, .. }) = self.scene.things[&thing].slots.get(slot).cloned() else {
                continue;
            };
            let own = &self.scene.library.modules[&module];
            if own.owner != Some(thing) || own.kind != self.scene.library.modules[&shared].kind {
                continue;
            }
            self.scene.link(self.reg, thing, *slot, shared).expect("the thing has the slot");
            self.scene.remove_module(module).expect("nothing else uses a fresh starter");
        }
    }

    /// The finished scene, with every thing owning the modules it uses.
    fn finish(mut self) -> Scene {
        own_everything(&mut self.scene);
        self.scene
    }

    fn scene_settings(&mut self, f: impl FnOnce(&mut SceneSettings)) {
        let root = self.scene.things.get_mut(&ThingId::ROOT).expect("root");
        // Keep what the new scene set.
        let mut s = SceneSettings::from_params(&root.params);
        f(&mut s);
        root.params = s.to_params();
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

    /// The module that fills `slot` on `thing`.
    fn module_of(&self, thing: ThingId, slot: SlotId) -> ModuleId {
        let resolved = bpe_core::resolve_slot(&self.scene, self.reg, thing, slot, &mut Vec::new());
        resolved.expect("the thing has a module in that slot").module
    }

    fn set_params(&mut self, m: ModuleId, params: bpe_core::Params) {
        self.scene.library.modules.get_mut(&m).expect("module").params = params;
    }

    fn root_look(&mut self, f: impl FnOnce(&mut BasicLook)) {
        let mut s = BasicLook::default();
        f(&mut s);
        let m = self.shared[&SlotId::Look];
        self.set_params(m, s.to_params());
    }

    /// The balls' look in `colours`, from the block's first row to its last as the balls start
    /// out: follows where each ball started, top to bottom, across the block's height. Balls
    /// that start in one row (or not in a block) all wear the first colour.
    fn ball_look_by_row(&mut self, colours: &[Color]) {
        let thing = &self.scene.things[&BALLS];
        let s = BallsSettings::from_params(&thing.params);
        let height = s.rows.saturating_sub(1) as f32 * s.spacing;
        let fill = if s.arrangement == Arrangement::Block && height > 0.0 {
            Paint {
                colours: Gradient::even(colours),
                follow: Follow::Start,
                angle: 90.0,
                centre: thing.transform.position,
                span: height,
                ..Paint::default()
            }
        } else {
            Paint::solid(colours.first().copied().unwrap_or(Color::WHITE))
        };
        self.ball_look(|l| l.fill = fill);
    }

    fn ball_look(&mut self, f: impl FnOnce(&mut BasicLook)) {
        let mut s = BasicLook { fill: Paint::solid(Color::rgb(0xff, 0x5c, 0xa8)), ..BasicLook::default() };
        f(&mut s);
        let m = self.module_of(BALLS, SlotId::Look);
        self.set_params(m, s.to_params());
    }

    fn motion(&mut self, f: impl FnOnce(&mut BasicMotion)) {
        let mut s = BasicMotion::default();
        f(&mut s);
        let m = self.shared[&SlotId::Motion];
        self.set_params(m, s.to_params());
    }

    fn bounce(&mut self, f: impl FnOnce(&mut BasicBounce)) {
        let mut s = BasicBounce::default();
        f(&mut s);
        let m = self.shared[&SlotId::Bounce];
        self.set_params(m, s.to_params());
    }

    /// A new module linked onto one thing. On the scene settings it also takes the place of the
    /// shared module for its slot, so every thing that used that one uses this instead (the way
    /// they used to inherit it). On a folder, the things inside that used the shared module
    /// follow the folder instead.
    fn own_module(&mut self, thing: ThingId, kind: &str, name: &str, params: bpe_core::Params) -> ModuleId {
        let m = self.scene.add_module(self.reg, kind, name).expect("kit kinds are registered");
        self.set_params(m, params);
        let slot = self.scene.library.modules[&m].slot;
        let shared = self.shared.get(&slot).copied();
        let users: Vec<ThingId> = self
            .scene
            .things
            .values()
            .filter(|t| matches!(t.slots.get(&slot), Some(SlotLink::Linked { module, .. }) if Some(*module) == shared))
            .map(|t| t.id)
            .collect();
        if thing == ThingId::ROOT {
            if self.reg.thing("scene").is_some_and(|k| k.accepts(slot, kind)) {
                self.scene.link(self.reg, thing, slot, m).expect("the scene settings have the slot");
            }
            if let Some(old) = shared {
                for user in users {
                    self.scene.link(self.reg, user, slot, m).expect("the thing has the slot");
                }
                self.shared.insert(slot, m);
                let _ = self.scene.remove_module(old);
            }
            return m;
        }
        self.scene.link(self.reg, thing, slot, m).expect("the kind has the slot");
        for user in users {
            if self.scene.ancestors(user).contains(&thing) {
                self.scene.things.get_mut(&user).expect("thing").slots.insert(slot, SlotLink::Inherit);
            }
        }
        m
    }

    /// A new thing of `kind` under `parent`, named and placed, with its settings, and the starter
    /// modules the kit gives that kind.
    fn add(&mut self, kind: &str, parent: ThingId, name: &str, position: Vec2, params: Params) -> ThingId {
        let id = crate::add_thing(&mut self.scene, self.reg, kind, parent).expect("kit kinds are registered");
        self.scene.rename_thing(id, name);
        self.share(id, &SlotId::ALL);
        let thing = self.scene.things.get_mut(&id).expect("just added");
        thing.transform.position = position;
        thing.params = params;
        id
    }

    /// Keys on one setting: (time, value, ease into the next).
    fn track(&mut self, a: Address, keys: &[(f64, Value, Ease)]) {
        for (time, value, ease) in keys {
            let i = self.scene.set_key(self.reg, &a, *time, value.clone()).expect("the setting takes keys");
            self.scene.set_ease(&a, i, *ease);
        }
    }

    /// Makes `id` ride on `parent`.
    fn ride(&mut self, id: ThingId, parent: ThingId, offset: Vec2, turn_with_parent: bool) {
        self.scene.set_parent(id, parent, Attach::Ride { offset, turn_with_parent, item: None }).expect("no loops");
    }

}

fn own(thing: ThingId, key: &str) -> Address {
    Address { owner: Owner::Thing(thing), slot: None, key: key.into() }
}

/// The new-scene default: a circle arena and one ball.
fn one_ball(reg: &Registry) -> Scene {
    Builder::new(reg, "One ball").finish()
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
    b.ball_look_by_row(&[
        Color::rgb(0x3e, 0x9b, 0xff),
        Color::rgb(0x8a, 0x4d, 0xff),
        Color::rgb(0xff, 0x3e, 0xc8),
        Color::rgb(0xff, 0x6a, 0x5c),
        Color::rgb(0xff, 0xb8, 0x2e),
    ]);
    let arena_look = BasicLook { fill: Paint::solid(Color::rgb(0xd8, 0xdc, 0xd0)), ..BasicLook::default() }.to_params();
    b.own_module(ARENA, "look.basic", "Arena look", arena_look);
    b.finish()
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
    b.finish()
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
    b.finish()
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
    let lively = BasicBounce { bounciness: 1.05, takes_over: true, ..BasicBounce::default() }.to_params();
    b.own_module(ARENA, "bounce.basic", "Lively wall", lively);
    b.finish()
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
        l.fill = Paint {
            colours: Gradient::even(&[
                Color::rgb(0x3e, 0x9b, 0xff),
                Color::rgb(0xff, 0x3e, 0xc8),
                Color::rgb(0xff, 0xb8, 0x2e),
            ]),
            follow: Follow::Hits,
            span: 12.0,
            ..Paint::default()
        };
    });
    b.finish()
}

/// Balls riding a turning hexagon; the balls change hue with every hit and pop after
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
    b.bounce(|s| s.pops_after = 40);
    b.ball_look(|l| l.hue_per_hit = 12.0);
    b.finish()
}

#[cfg(test)]
mod tests {
    use crate::{registry, scenes};

    #[test]
    fn every_thing_in_a_built_in_scene_has_its_own_modules() {
        let reg = registry();
        let mut all = scenes::all(&reg);
        all.push(("Long drop".into(), scenes::long_drop(&reg)));
        for (name, scene) in &all {
            for (id, thing) in &scene.things {
                for (slot, link) in &thing.slots {
                    let bpe_core::SlotLink::Linked { module, .. } = link else { continue };
                    let m = &scene.library.modules[module];
                    assert_eq!(scene.users_of(*module), [*id], "{name}: {} {slot:?} shares {}", thing.name, m.name);
                    assert_eq!(m.owner, Some(*id), "{name}: {} {slot:?} uses {}", thing.name, m.name);
                }
            }
        }
    }

    #[test]
    fn starters_are_six_named_scenes_from_the_collection() {
        let reg = registry();
        let starters = scenes::starters(&reg);
        assert_eq!(starters.len(), 6);
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
        assert_eq!(scenes.len(), 15);
        let in_order: Vec<String> = [
            scenes::stage2(&reg),
            scenes::stage3(&reg),
            scenes::stage5(&reg),
            vec![
                ("Colour paints".into(), scenes::colour_demo(&reg)),
                ("Blend modes".into(), scenes::blend_demo(&reg)),
                ("Graded sunset".into(), scenes::graded_sunset(&reg)),
            ],
        ]
        .concat()
        .into_iter()
        .map(|(n, _)| n)
        .collect();
        let names: Vec<&str> = scenes.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, in_order);
        assert_eq!(names[..5], ["One ball", "Reel block", "Box drop", "Spinning hexagon", "Shrinking ring"]);
        assert_eq!(names[5..7], ["Ball pit", "Riders"]);
        assert_eq!(names[7..12], ["Lava ball", "Confetti drop", "Spark block", "Kaleido hexagon", "Neon ring"]);
        assert_eq!(names[12..], ["Colour paints", "Blend modes", "Graded sunset"]);
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
    fn no_built_in_scene_uses_additive_colours() {
        use bpe_core::Value;
        fn additive(v: &Value) -> bool {
            match v {
                Value::Color(c) => c.is_additive(),
                Value::Gradient(g) => g.stops.iter().any(|s| s.color.is_additive()),
                _ => false,
            }
        }
        let reg = registry();
        let mut all = scenes::all(&reg);
        all.push(("Long drop".into(), scenes::long_drop(&reg)));
        for (name, scene) in &all {
            let mut values: Vec<(String, &Value)> = Vec::new();
            for (id, m) in &scene.library.modules {
                values.extend(m.params.0.iter().map(|(k, v)| (format!("module {id:?} {k}"), v)));
            }
            for (id, t) in &scene.things {
                values.extend(t.params.0.iter().map(|(k, v)| (format!("thing {id:?} {k}"), v)));
            }
            for (a, track) in &scene.tracks {
                values.extend(track.keys.iter().map(|k| (format!("track {a:?}"), &k.value)));
            }
            for (at, v) in values {
                assert!(!additive(v), "{name}: {at} is an additive colour, use Blend Add instead");
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

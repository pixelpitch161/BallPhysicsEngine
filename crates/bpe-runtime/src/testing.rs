//! Small test kinds and sims shared by this crate's tests: a round wall, a set of dots and a
//! root that sets gravity.

use bpe_core::{
    Attach, ModuleKind, Params, Registry, Resolved, ResolvedThing, Scene, SettingDef, Settings, SlotId, Target, Thing,
    ThingId, ThingKind, Value, resolve,
};
use glam::Vec2;

use crate::plugins::{ModuleSim, PairRule, Sims, ThingSim, ThingState};
use crate::{Bodies, Contact, Limits, Outline, Pose, Solid, Solids, Wall, World};

pub const WALL: ThingId = ThingId(1);
pub const DOTS: ThingId = ThingId(2);
pub const PEG: ThingId = ThingId(3);
/// The second dots thing in [`dots_pair`].
pub const DOTS2: ThingId = ThingId(3);

#[derive(Settings)]
struct RootSettings {
    #[setting(label = "Gravity", keyframe)]
    gravity: Vec2,
    #[setting(label = "Tint", range(0, 10), keyframe)]
    tint: f32,
}
impl Default for RootSettings {
    fn default() -> Self {
        Self { gravity: Vec2::new(0.0, 500.0), tint: 0.0 }
    }
}

#[derive(Settings)]
struct WallSettings {
    #[setting(label = "Radius", range(1, 10000))]
    radius: f32,
    #[setting(label = "Spin", range(-100, 100))]
    spin: f32,
}
impl Default for WallSettings {
    fn default() -> Self {
        Self { radius: 100.0, spin: 0.0 }
    }
}

#[derive(Settings)]
struct DotsSettings {
    #[setting(label = "Count", range(1, 10000))]
    count: u32,
    #[setting(label = "Velocity")]
    velocity: Vec2,
    #[setting(label = "Tint", range(0, 10))]
    tint: f32,
}
impl Default for DotsSettings {
    fn default() -> Self {
        Self { count: 1, velocity: Vec2::new(300.0, -200.0), tint: 0.0 }
    }
}

#[derive(Settings)]
struct PegSettings {
    #[setting(label = "Radius", range(1, 1000))]
    radius: f32,
}
impl Default for PegSettings {
    fn default() -> Self {
        Self { radius: 10.0 }
    }
}

#[derive(Default, Settings)]
struct NoSettings {}

#[derive(Clone, Settings)]
struct BumpySettings {
    #[setting(label = "Bounciness", range(0, 2))]
    bounciness: f32,
    #[setting(label = "Target")]
    target: Target,
    #[setting(label = "Pops after", range(0, 1000))]
    pops_after: u32,
}
impl Default for BumpySettings {
    fn default() -> Self {
        Self { bounciness: 1.0, target: Target::Anything, pops_after: 0 }
    }
}

struct RootKind;
struct WallKind;
struct DotsKind;
struct CounterKind;
struct PegKind;
struct StickyKind;
struct BouncyKind;
struct BumpyKind;
impl ModuleKind for BumpyKind {
    fn id(&self) -> &'static str {
        "bounce.bumpy"
    }
    fn name(&self) -> &'static str {
        "Bumpy"
    }
    fn help(&self) -> &'static str {
        ""
    }
    fn slot(&self) -> SlotId {
        SlotId::Bounce
    }
    fn settings(&self) -> &'static [SettingDef] {
        BumpySettings::defs()
    }
}

macro_rules! thing_kind {
    ($t:ty, $id:literal, $defs:ty) => {
        impl ThingKind for $t {
            fn id(&self) -> &'static str {
                $id
            }
            fn name(&self) -> &'static str {
                $id
            }
            fn icon(&self) -> &'static str {
                $id
            }
            fn help(&self) -> &'static str {
                ""
            }
            fn settings(&self) -> &'static [SettingDef] {
                <$defs>::defs()
            }
            fn slots(&self) -> &'static [SlotId] {
                &SlotId::ALL
            }
            fn default_slots(&self) -> &'static [SlotId] {
                &[]
            }
        }
    };
}
thing_kind!(RootKind, "scene", RootSettings);
thing_kind!(WallKind, "wall", WallSettings);
thing_kind!(DotsKind, "dots", DotsSettings);
thing_kind!(CounterKind, "counter", NoSettings);
thing_kind!(PegKind, "peg", PegSettings);

macro_rules! bounce_kind {
    ($t:ty, $id:literal) => {
        impl ModuleKind for $t {
            fn id(&self) -> &'static str {
                $id
            }
            fn name(&self) -> &'static str {
                $id
            }
            fn help(&self) -> &'static str {
                ""
            }
            fn slot(&self) -> SlotId {
                SlotId::Bounce
            }
            fn settings(&self) -> &'static [SettingDef] {
                NoSettings::defs()
            }
        }
    };
}
bounce_kind!(StickyKind, "bounce.sticky");
bounce_kind!(BouncyKind, "bounce.bouncy");

/// A trail, which asks the runtime to keep a history of where the dots have been.
struct TrailKind;
impl ModuleKind for TrailKind {
    fn id(&self) -> &'static str {
        "trail.test"
    }
    fn name(&self) -> &'static str {
        "trail.test"
    }
    fn help(&self) -> &'static str {
        ""
    }
    fn slot(&self) -> SlotId {
        SlotId::Trail
    }
    fn settings(&self) -> &'static [SettingDef] {
        NoSettings::defs()
    }
}

pub struct TestRoot;
impl ThingSim for TestRoot {
    fn kind(&self) -> &'static str {
        "scene"
    }
    fn build(&self, _thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let s = RootSettings::from_params(&resolved.params);
        world.steps_per_second = 240;
        world.gravity = s.gravity;
    }
    fn affects_physics(&self, key: &str) -> bool {
        key != "tint"
    }
    fn settings_changed(&self, _thing: ThingId, resolved: &ResolvedThing, world: &mut World) {
        world.gravity = RootSettings::from_params(&resolved.params).gravity;
    }
}

/// Logs one custom event after every step.
pub struct TestCounter;
impl ThingSim for TestCounter {
    fn kind(&self) -> &'static str {
        "counter"
    }
    fn build(&self, _thing: &Thing, _resolved: &ResolvedThing, _world: &mut World) {}
    fn after_step(&self, thing: ThingId, world: &mut World, log: &mut crate::Log) {
        log.push(crate::Event {
            step: world.step,
            kind: crate::EventKind::Custom("tick".into()),
            a: crate::Party { thing, item: 0 },
            b: None,
            point: Vec2::ZERO,
            normal: Vec2::ZERO,
            speed: 0.0,
        });
    }
}

pub struct TestWall;
impl ThingSim for TestWall {
    fn kind(&self) -> &'static str {
        "wall"
    }
    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let s = WallSettings::from_params(&resolved.params);
        let base = world.anchor(thing.id);
        let outline =
            Outline::Ring { center: base.position, radius: s.radius, sides: 0, angle: base.angle, thickness: 2.0 };
        world.walls.insert(thing.id, Wall { outline, spin: s.spin, growth: 0.0, velocity: base.velocity });
        world.poses.insert(thing.id, Pose { spin: s.spin, ..base });
        world.states.insert(thing.id, Box::new(s.spin));
    }

    fn scripted(&self, thing: ThingId, world: &mut World) {
        let dt = world.dt();
        let spin = world.states.get(&thing).map_or(0.0, |s| *crate::state::<f32>(s.as_ref()));
        let base = world.anchor(thing);
        if let Some(pose) = world.poses.get_mut(&thing) {
            pose.angle += spin * dt;
            pose.position = base.position;
            pose.velocity = base.velocity;
        }
        if let Some(Wall { outline: Outline::Ring { center, angle, .. }, velocity, .. }) = world.walls.get_mut(&thing) {
            *angle += spin * dt;
            *center = base.position;
            *velocity = base.velocity;
        }
    }
}

pub struct TestDots;
impl ThingSim for TestDots {
    fn kind(&self) -> &'static str {
        "dots"
    }
    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let s = DotsSettings::from_params(&resolved.params);
        let at = world.anchor(thing.id);
        let mut bodies = Bodies::default();
        // A block 12 px apart, as square as the count allows, centred on the thing.
        let columns = (s.count as f32).sqrt().ceil().max(1.0) as u32;
        let rows = s.count.div_ceil(columns);
        for i in 0..s.count {
            let column = (i % columns) as f32 - (columns - 1) as f32 * 0.5;
            let row = (i / columns) as f32 - (rows - 1) as f32 * 0.5;
            let offset = Vec2::new(column, row) * 12.0;
            bodies.push(at.position + offset, s.velocity + at.velocity, 5.0);
        }
        world.bodies.insert(thing.id, bodies);
        if resolved.slots.get(&SlotId::Trail).is_some_and(|t| t.kind.starts_with("trail.")) {
            world.keep_history(thing.id);
        }
    }
    fn affects_physics(&self, key: &str) -> bool {
        key != "tint"
    }
}

/// A round peg at the thing's place that rides along with its parent.
pub struct TestPeg;
impl ThingSim for TestPeg {
    fn kind(&self) -> &'static str {
        "peg"
    }
    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let s = PegSettings::from_params(&resolved.params);
        world.states.insert(thing.id, Box::new(s.radius));
        self.scripted(thing.id, world);
    }
    fn scripted(&self, thing: ThingId, world: &mut World) {
        let radius = world.states.get(&thing).map_or(10.0, |s| *crate::state::<f32>(s.as_ref()));
        let pose = world.anchor(thing);
        world.poses.insert(thing, pose);
        world.solids.insert(thing, Solids::new(pose, vec![Solid::Disc { center: pose.position, radius }]));
    }
}

/// Gravity for everything, so balls fall.
struct TestGravity;
impl ModuleSim for TestGravity {
    fn kind(&self) -> &'static str {
        "motion.gravity"
    }
    fn prepare(&self, _params: &Params) -> Box<dyn ThingState> {
        Box::new(())
    }
    fn forces(&self, _state: &dyn ThingState, bodies: &mut Bodies, gravity: Vec2, dt: f32) {
        for (v, alive) in bodies.velocity.iter_mut().zip(&bodies.alive) {
            if *alive {
                *v += gravity * dt;
            }
        }
    }
}
struct GravityKind;
impl ModuleKind for GravityKind {
    fn id(&self) -> &'static str {
        "motion.gravity"
    }
    fn name(&self) -> &'static str {
        "Gravity"
    }
    fn help(&self) -> &'static str {
        ""
    }
    fn slot(&self) -> SlotId {
        SlotId::Motion
    }
    fn settings(&self) -> &'static [SettingDef] {
        NoSettings::defs()
    }
}

/// Stops the ball dead, and insists on doing so when it is the wall's module.
struct Sticky {
    takes_over: bool,
}
impl ModuleSim for Sticky {
    fn kind(&self) -> &'static str {
        "bounce.sticky"
    }
    fn prepare(&self, _params: &Params) -> Box<dyn ThingState> {
        Box::new(())
    }
    fn respond(&self, _state: &dyn ThingState, _contact: &Contact, bodies: &mut Bodies, item: usize, _limits: &Limits) {
        bodies.velocity[item] = Vec2::ZERO;
    }
    fn takes_over(&self, _state: &dyn ThingState) -> bool {
        self.takes_over
    }
}

/// The default bounce, as a named module.
struct Bouncy;
impl ModuleSim for Bouncy {
    fn kind(&self) -> &'static str {
        "bounce.bouncy"
    }
    fn prepare(&self, _params: &Params) -> Box<dyn ThingState> {
        Box::new(())
    }
}

/// Balls that bump into other balls and pop after so many hits.
struct Bumpy;
impl ModuleSim for Bumpy {
    fn kind(&self) -> &'static str {
        "bounce.bumpy"
    }
    fn prepare(&self, params: &Params) -> Box<dyn ThingState> {
        Box::new(BumpySettings::from_params(params))
    }
    fn bounciness(&self, state: &dyn ThingState) -> f32 {
        crate::state::<BumpySettings>(state).bounciness
    }
    fn pairs(&self, state: &dyn ThingState) -> Option<PairRule> {
        Some(PairRule { target: crate::state::<BumpySettings>(state).target.clone(), friction: 0.0 })
    }
    fn effects(&self, state: &dyn ThingState, bodies: &mut Bodies, item: usize, _speed: f32, _limits: &Limits) -> bool {
        let pops = crate::state::<BumpySettings>(state).pops_after;
        pops > 0 && bodies.hits[item] >= pops
    }
}

pub fn reg() -> Registry {
    let mut r = Registry::new();
    r.add_thing(RootKind);
    r.add_thing(WallKind);
    r.add_thing(DotsKind);
    r.add_thing(CounterKind);
    r.add_thing(PegKind);
    r.add_module(GravityKind);
    r.add_module(StickyKind);
    r.add_module(BouncyKind);
    r.add_module(BumpyKind);
    r.add_module(TrailKind);
    r
}

pub fn sims(sticky_takes_over: bool) -> Sims {
    let mut s = Sims::default();
    s.add_thing(TestRoot);
    s.add_thing(TestWall);
    s.add_thing(TestDots);
    s.add_thing(TestCounter);
    s.add_thing(TestPeg);
    s.add_module(TestGravity);
    s.add_module(Sticky { takes_over: sticky_takes_over });
    s.add_module(Bouncy);
    s.add_module(Bumpy);
    s
}

pub fn dots_in_ring(count: u32) -> (Scene, Resolved, Sims) {
    let reg = reg();
    let mut scene = Scene::empty(&reg);
    let gravity = scene.add_module(&reg, "motion.gravity", "Gravity").unwrap();
    scene.link(&reg, ThingId::ROOT, SlotId::Motion, gravity).unwrap();
    let wall = scene.add_thing(&reg, "wall", ThingId::ROOT).unwrap();
    let dots = scene.add_thing(&reg, "dots", ThingId::ROOT).unwrap();
    assert_eq!((wall, dots), (WALL, DOTS));
    scene.things.get_mut(&DOTS).unwrap().params.set("count", Value::Int(count as i64));
    let resolved = resolve(&scene, &reg);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    (scene, resolved, sims(true))
}

/// One dot riding on a ring that turns clockwise at 1 rad/s.
pub fn ring_with_rider(offset: Vec2, turn: bool) -> (Scene, Resolved, Sims) {
    let (mut scene, _, sims) = dots_in_ring(1);
    scene.things.get_mut(&WALL).unwrap().params.set("spin", Value::Number(1.0));
    scene.set_parent(DOTS, WALL, Attach::Ride { offset, turn_with_parent: turn, item: None }).unwrap();
    let resolved = resolve(&scene, &reg());
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    (scene, resolved, sims)
}

/// One still dot above a peg inside the ring, so it falls onto the peg.
pub fn dots_and_peg() -> (Scene, Resolved, Sims) {
    let (mut scene, _, sims) = dots_in_ring(1);
    let reg = reg();
    let peg = scene.add_thing(&reg, "peg", ThingId::ROOT).unwrap();
    assert_eq!(peg, PEG);
    scene.things.get_mut(&PEG).unwrap().transform.position = Vec2::new(0.0, 40.0);
    scene.things.get_mut(&DOTS).unwrap().transform.position = Vec2::new(0.0, -50.0);
    scene.things.get_mut(&DOTS).unwrap().params.set("velocity", Value::Vec2([0.0, 0.0]));
    let resolved = resolve(&scene, &reg);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    (scene, resolved, sims)
}

/// Two bumpy dots heading at each other at 100 px/s inside the ring, no gravity: the first at
/// `(-gap, 0)`, the second (tagged "soft") at `(gap, 0)`. Both bumpy modules point at `target`.
pub fn dots_pair(gap: f32, target: Target) -> (Scene, Resolved, Sims) {
    let (mut scene, _, sims) = dots_in_ring(1);
    let reg = reg();
    scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, 0.0]));
    let second = scene.add_thing(&reg, "dots", ThingId::ROOT).unwrap();
    assert_eq!(second, DOTS2);
    scene.things.get_mut(&DOTS2).unwrap().tags.insert("soft".into());
    for (id, x) in [(DOTS, -gap), (DOTS2, gap)] {
        let t = scene.things.get_mut(&id).unwrap();
        t.transform.position = Vec2::new(x, 0.0);
        t.params.set("velocity", Value::Vec2([if x < 0.0 { 100.0 } else { -100.0 }, 0.0]));
    }
    let bumpy = scene.add_module(&reg, "bounce.bumpy", "Bumpy").unwrap();
    scene.library.modules.get_mut(&bumpy).unwrap().params.set("target", Value::Target(target));
    scene.link(&reg, ThingId::ROOT, SlotId::Bounce, bumpy).unwrap();
    let resolved = resolve(&scene, &reg);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    (scene, resolved, sims)
}

/// [`dots_pair`] with gravity, where the first dots' module pops its ball after `pops` hits.
pub fn dots_pair_popping(pops: u32) -> (Scene, Resolved, Sims) {
    let (mut scene, _, sims) = dots_pair(30.0, Target::Anything);
    let reg = reg();
    scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, 500.0]));
    let popping = scene.add_module(&reg, "bounce.bumpy", "Popping").unwrap();
    scene.library.modules.get_mut(&popping).unwrap().params.set("pops_after", Value::Int(pops as i64));
    scene.link(&reg, DOTS, SlotId::Bounce, popping).unwrap();
    let resolved = resolve(&scene, &reg);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    (scene, resolved, sims)
}

/// `count` bumpy dots in a ring of 150 under gravity, spread over a block so they pile up.
pub fn dots_pile(count: u32) -> (Scene, Resolved, Sims) {
    let (mut scene, _, sims) = dots_in_ring(count);
    let reg = reg();
    scene.things.get_mut(&WALL).unwrap().params.set("radius", Value::Number(150.0));
    scene.things.get_mut(&DOTS).unwrap().params.set("velocity", Value::Vec2([0.0, 0.0]));
    let bumpy = scene.add_module(&reg, "bounce.bumpy", "Bumpy").unwrap();
    scene.library.modules.get_mut(&bumpy).unwrap().params.set("bounciness", Value::Number(0.5));
    scene.link(&reg, ThingId::ROOT, SlotId::Bounce, bumpy).unwrap();
    let resolved = resolve(&scene, &reg);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    (scene, resolved, sims)
}

/// The dots carry a bouncy module; the wall a sticky one that may or may not take over.
pub fn dots_in_sticky_ring(takes_over: bool) -> (Scene, Resolved, Sims) {
    let (mut scene, _, _) = dots_in_ring(1);
    let reg = reg();
    let sticky = scene.add_module(&reg, "bounce.sticky", "Sticky").unwrap();
    let bouncy = scene.add_module(&reg, "bounce.bouncy", "Bouncy").unwrap();
    scene.link(&reg, WALL, SlotId::Bounce, sticky).unwrap();
    scene.link(&reg, DOTS, SlotId::Bounce, bouncy).unwrap();
    let resolved = resolve(&scene, &reg);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    (scene, resolved, sims(takes_over))
}

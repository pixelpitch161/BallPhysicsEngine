//! Small test kinds and sims shared by this crate's tests: a round wall, a set of dots and a
//! root that sets gravity.

use bpe_core::{
    ModuleKind, Params, Registry, Resolved, ResolvedThing, Scene, SettingDef, Settings, SlotId, Thing, ThingId,
    ThingKind, Value, resolve,
};
use glam::Vec2;

use crate::plugins::{ModuleSim, Sims, ThingSim, ThingState};
use crate::{Bodies, Contact, Limits, Outline, Wall, World};

pub const WALL: ThingId = ThingId(1);
pub const DOTS: ThingId = ThingId(2);
pub const EMITTER: ThingId = ThingId(3);

#[derive(Settings)]
struct RootSettings {
    #[setting(label = "Gravity")]
    gravity: Vec2,
    #[setting(label = "Tint", range(0, 10))]
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
}
impl Default for WallSettings {
    fn default() -> Self {
        Self { radius: 100.0 }
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

#[derive(Default, Settings)]
struct NoSettings {}

struct RootKind;
struct WallKind;
struct DotsKind;
struct EmitterKind;
struct StickyKind;
struct BouncyKind;

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
thing_kind!(EmitterKind, "emitter", NoSettings);

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
}

pub struct TestWall;
impl ThingSim for TestWall {
    fn kind(&self) -> &'static str {
        "wall"
    }
    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let s = WallSettings::from_params(&resolved.params);
        let outline =
            Outline::Ring { center: thing.transform.position, radius: s.radius, sides: 0, angle: 0.0, thickness: 2.0 };
        world.walls.insert(thing.id, Wall { outline, spin: 0.0, growth: 0.0 });
    }
}

pub struct TestDots;
impl ThingSim for TestDots {
    fn kind(&self) -> &'static str {
        "dots"
    }
    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let s = DotsSettings::from_params(&resolved.params);
        let mut bodies = Bodies::default();
        for i in 0..s.count {
            let offset = Vec2::new(i as f32 * 0.5, 0.0);
            bodies.push(thing.transform.position + offset, s.velocity, 5.0);
        }
        world.bodies.insert(thing.id, bodies);
    }
    fn affects_physics(&self, key: &str) -> bool {
        key != "tint"
    }
}

/// Exercises every hook: adds a dot each step, pushes the dots sideways, checks that settling
/// runs after the move, and ticks at the end of the step.
pub struct TestEmitter;
impl ThingSim for TestEmitter {
    fn kind(&self) -> &'static str {
        "emitter"
    }
    fn build(&self, thing: &Thing, _resolved: &ResolvedThing, world: &mut World) {
        world.states.insert(thing.id, Box::new(0u32));
    }
    fn spawn(&self, thing: ThingId, world: &mut World, log: &mut crate::Log) {
        let step = world.step;
        let row = world.bodies.get_mut(&DOTS).unwrap().push(Vec2::ZERO, Vec2::ZERO, 5.0);
        log.push(crate::Event {
            step,
            kind: crate::EventKind::Spawn,
            a: crate::Party { thing: DOTS, item: row },
            b: Some(crate::Party { thing, item: 0 }),
            point: Vec2::ZERO,
            normal: Vec2::ZERO,
            speed: 0.0,
        });
    }
    fn forces(&self, _thing: ThingId, world: &mut World, dt: f32) {
        for v in &mut world.bodies.get_mut(&DOTS).unwrap().velocity {
            *v += Vec2::X * 1000.0 * dt;
        }
    }
    fn settle(&self, thing: ThingId, world: &mut World, _dt: f32) {
        let moved = world.bodies[&DOTS].position[0] != world.bodies[&DOTS].previous[0];
        if moved {
            *crate::state_mut::<u32>(world.states.get_mut(&thing).unwrap().as_mut()) += 1;
        }
    }
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

pub fn reg() -> Registry {
    let mut r = Registry::new();
    r.add_thing(RootKind);
    r.add_thing(WallKind);
    r.add_thing(DotsKind);
    r.add_thing(EmitterKind);
    r.add_module(GravityKind);
    r.add_module(StickyKind);
    r.add_module(BouncyKind);
    r
}

pub fn sims(sticky_takes_over: bool) -> Sims {
    let mut s = Sims::default();
    s.add_thing(TestRoot);
    s.add_thing(TestWall);
    s.add_thing(TestDots);
    s.add_thing(TestEmitter);
    s.add_module(TestGravity);
    s.add_module(Sticky { takes_over: sticky_takes_over });
    s.add_module(Bouncy);
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

/// One dot in the ring plus an emitter that exercises every hook.
pub fn dots_with_emitter() -> (Scene, Resolved, Sims) {
    let (mut scene, _, sims) = dots_in_ring(1);
    let reg = reg();
    let emitter = scene.add_thing(&reg, "emitter", ThingId::ROOT).unwrap();
    assert_eq!(emitter, EMITTER);
    let resolved = resolve(&scene, &reg);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    (scene, resolved, sims)
}

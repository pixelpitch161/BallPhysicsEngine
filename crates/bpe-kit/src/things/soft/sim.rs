//! Moving a soft body each step and logging its impacts.

use bpe_core::{ResolvedThing, Settings, SlotId, Thing, ThingId};
use bpe_runtime::{Event, EventKind, Log, Party, ThingSim, Wall, World, keep_inside, reflect, state_mut};
use glam::Vec2;

use super::solver::SoftWorld;
use super::{SoftSettings, build};
use crate::modules::bounce::BasicBounce;
use crate::modules::motion::BasicMotion;

pub struct SoftSim;

/// A hit between a soft body's point and a wall or a ball.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Impact {
    pub point: u32,
    pub at: Vec2,
    /// From what was hit towards the body.
    pub normal: Vec2,
    pub speed: f32,
    /// The wall or ball hit.
    pub other: Option<Party>,
}

/// What a soft body keeps between steps.
#[derive(Clone, Debug, PartialEq)]
pub struct SoftState {
    pub world: SoftWorld,
    /// The hardest impact this step.
    pub impact: Option<Impact>,
    /// When the body last logged a hit.
    last_event: f64,
}

/// Bounciness of soft points against walls at the default Bounce: the squash and spring of the
/// body does the rest.
const SOFT_RESTITUTION: f32 = 0.15;
/// Bounciness between soft points and balls.
const BALL_RESTITUTION: f32 = 0.3;
/// Seconds a body waits after logging a hit before it logs another, so a jelly ball rolling
/// along the floor doesn't buzz.
const SOUND_GAP: f64 = 0.08;

fn harder(hardest: &mut Option<Impact>, impact: Impact) {
    if hardest.is_none_or(|h| impact.speed > h.speed) {
        *hardest = Some(impact);
    }
}

/// The Motion module's gravity scale and drag, when it is the basic one.
fn motion_of(world: &World, thing: ThingId) -> (f32, f32) {
    match world.module_state(thing, SlotId::Motion).and_then(|(_, st)| st.as_any().downcast_ref::<BasicMotion>()) {
        Some(m) => (m.gravity_scale, m.drag),
        None => (1.0, 0.0),
    }
}

/// How bouncy the points are against walls: the Bounce module's bounciness, scaled so the
/// default gives [`SOFT_RESTITUTION`].
fn restitution_of(world: &World, thing: ThingId) -> f32 {
    match world.module_state(thing, SlotId::Bounce).and_then(|(_, st)| st.as_any().downcast_ref::<BasicBounce>()) {
        Some(b) => (SOFT_RESTITUTION * b.bounciness / BasicBounce::default().bounciness.max(1e-3)).clamp(0.0, 0.9),
        None => SOFT_RESTITUTION,
    }
}

impl ThingSim for SoftSim {
    fn kind(&self) -> &'static str {
        "soft"
    }

    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let settings = SoftSettings::from_params(&resolved.params);
        let soft = build(&settings, thing.transform.position, thing.transform.rotation);
        world.states.insert(thing.id, Box::new(SoftState { world: soft, impact: None, last_event: f64::NEG_INFINITY }));
    }

    /// The whole step's solver passes, each one bouncing the points off every wall, then the
    /// touches with every other thing's balls.
    fn scripted(&self, thing: ThingId, world: &mut World) {
        let dt = world.dt();
        let gravity = world.gravity;
        let (scale, drag) = motion_of(world, thing);
        let restitution = restitution_of(world, thing);
        let walls: Vec<(ThingId, Wall)> = world.walls.iter().map(|(id, w)| (*id, *w)).collect();
        let Some(st) = world.states.get_mut(&thing) else { return };
        let st = state_mut::<SoftState>(st.as_mut());
        if st.world.is_empty() {
            return;
        }
        let passes = st.world.passes.max(1);
        let h = dt / passes as f32;
        let mut hardest = None;
        st.world.find_pairs();
        for _ in 0..passes {
            st.world.predict(h, gravity * scale, drag);
            st.world.solve(h);
            for (i, p) in st.world.points.iter_mut().enumerate() {
                if p.pinned() {
                    continue;
                }
                for (wall_id, wall) in &walls {
                    if let Some(contact) = keep_inside(&mut p.position, p.velocity, p.radius, wall) {
                        p.velocity = reflect(p.velocity, &contact, restitution);
                        let impact = Impact {
                            point: i as u32,
                            at: contact.point,
                            normal: contact.normal,
                            speed: contact.speed,
                            other: Some(Party { thing: *wall_id, item: 0 }),
                        };
                        harder(&mut hardest, impact);
                    }
                }
            }
        }
        for (id, bodies) in world.bodies.iter_mut() {
            if *id == thing {
                continue;
            }
            if let Some(impact) = st.world.collide_balls(*id, bodies, BALL_RESTITUTION) {
                harder(&mut hardest, impact);
            }
        }
        st.impact = hardest;
    }

    /// Logs the step's hardest impact as a Hit when it is more than resting, at most once every
    /// [`SOUND_GAP`].
    fn after_step(&self, thing: ThingId, world: &mut World, log: &mut Log) {
        let time = world.time();
        let resting = world.gravity.length() * world.dt() * 8.0 + 20.0;
        let step = world.step;
        let Some(st) = world.states.get_mut(&thing) else { return };
        let st = state_mut::<SoftState>(st.as_mut());
        let Some(impact) = st.impact.take() else { return };
        if impact.speed < resting || time - st.last_event < SOUND_GAP {
            return;
        }
        st.last_event = time;
        log.push(Event {
            step,
            kind: EventKind::Hit,
            a: Party { thing, item: impact.point },
            b: impact.other,
            point: impact.at,
            normal: impact.normal,
            speed: impact.speed,
        });
    }

    fn affects_physics(&self, _key: &str) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{ARENA, BALLS};
    use crate::{kit, new_scene};
    use bpe_core::{Scene, Value, resolve};
    use bpe_runtime::{Playback, state};

    fn soft_of(world: &World, id: ThingId) -> &SoftState {
        state::<SoftState>(world.states[&id].as_ref())
    }

    /// The new scene with a soft thing at `at`, with `f`'s changes to its settings.
    fn scene_with_soft(at: Vec2, f: impl FnOnce(&mut bpe_core::Params)) -> (Scene, ThingId) {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let soft = scene.add_thing(&kit.registry, "soft", ThingId::ROOT).unwrap();
        let thing = scene.things.get_mut(&soft).unwrap();
        thing.transform.position = at;
        f(&mut thing.params);
        (scene, soft)
    }

    #[test]
    fn a_jelly_dropped_in_the_new_scene_stays_inside_and_hits_the_floor() {
        let (mut scene, soft) = scene_with_soft(Vec2::new(0.0, -300.0), |_| {});
        // The ball starts inside the jelly and would be trapped in it; this test is about the floor.
        scene.things.get_mut(&BALLS).unwrap().enabled = false;
        let kit = kit();
        let resolved = resolve(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let mut w = World::build(&scene, &resolved, &kit.sims);
        let mut log = Log::default();
        // Gravity is a gentle 500 px/s²: it lands after 1.7 s and has settled by 4 s.
        w.advance_to(4.0, &kit.sims, &mut log);
        let st = soft_of(&w, soft);
        for p in &st.world.points {
            assert!(p.position.length() <= 508.0 + 1.0, "{:?}", p.position);
            assert!(p.position.is_finite());
        }
        assert!(st.world.middle().y > 330.0, "it fell and settled: {:?}", st.world.middle());
        let hits: Vec<&Event> = log.events().iter().filter(|e| e.a.thing == soft).collect();
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|e| e.kind == EventKind::Hit && e.b == Some(Party { thing: ARENA, item: 0 })));
        let squash = st.world.squash();
        assert!(squash > 0.6 && squash < 1.4, "{squash}");
    }

    #[test]
    fn a_resting_jelly_does_not_buzz() {
        // A box arena 1500 tall: its floor is at y 750, and the jelly sits on it from the start.
        let (mut scene, soft) = scene_with_soft(Vec2::new(0.0, 750.0 - 1.5 - 120.0), |_| {});
        scene.things.get_mut(&ARENA).unwrap().params.set("shape", Value::Choice("box".into()));
        scene.things.get_mut(&BALLS).unwrap().enabled = false;
        let kit = kit();
        let resolved = resolve(&scene, &kit.registry);
        let mut w = World::build(&scene, &resolved, &kit.sims);
        let mut log = Log::default();
        w.advance_to(4.0, &kit.sims, &mut log);
        let rate = w.steps_per_second;
        let late = log.events().iter().filter(|e| e.a.thing == soft && e.time(rate) >= 2.0).count();
        assert!(late <= 10, "{late} hits while resting");
        let st = soft_of(&w, soft);
        assert!((st.world.middle().y - (750.0 - 1.5 - 120.0)).abs() < 30.0, "{:?}", st.world.middle());
    }

    #[test]
    fn a_ball_lands_on_a_pinned_cloth() {
        // A hammock: a band of cloth pinned at its top corners, with the ball dropped on it.
        let (mut scene, soft) = scene_with_soft(Vec2::new(0.0, -200.0), |p| {
            p.set("shape", Value::Choice("cloth".into()));
            p.set("size", Value::Number(600.0));
            p.set("height", Value::Number(60.0));
            p.set("detail", Value::Int(40));
            p.set("rows", Value::Int(4));
            p.set("pinned", Value::Choice("top_corners".into()));
        });
        scene.things.get_mut(&ARENA).unwrap().params.set("shape", Value::Choice("box".into()));
        let ball = scene.things.get_mut(&BALLS).unwrap();
        ball.transform.position = Vec2::new(0.0, -700.0);
        ball.params.set("velocity", Value::Vec2([0.0, 0.0]));
        let kit = kit();
        let resolved = resolve(&scene, &kit.registry);
        let mut w = World::build(&scene, &resolved, &kit.sims);
        let top_middle = soft_of(&w, soft).world.points[20].position;
        let corner = soft_of(&w, soft).world.points[0].position;
        let mut log = Log::default();
        w.advance_to(2.0, &kit.sims, &mut log);
        let ball_y = w.bodies[&BALLS].position[0].y;
        assert!(ball_y < 300.0, "the ball stayed on the cloth, not the floor: {ball_y}");
        assert!(ball_y > -260.0, "the ball came down onto the cloth: {ball_y}");
        let st = soft_of(&w, soft);
        assert!(st.world.points[20].position.y > top_middle.y + 5.0, "the cloth sagged: {:?}", st.world.points[20]);
        assert_eq!(st.world.points[0].position, corner, "a pinned corner stays");
        assert!(log.events().iter().any(|e| e.a.thing == soft && e.b.is_some_and(|b| b.thing == BALLS)));
    }

    #[test]
    fn soft_runs_are_deterministic() {
        let (mut scene, soft) = scene_with_soft(Vec2::new(0.0, -300.0), |p| {
            p.set("shape", Value::Choice("rope".into()));
            p.set("pinned", Value::Choice("start".into()));
        });
        scene.things.get_mut(&soft).unwrap().transform.rotation = 0.3;
        let kit = kit();
        let resolved = resolve(&scene, &kit.registry);
        let points = |p: &Playback| soft_of(p.world(), soft).world.points.clone();
        let mut a = Playback::new(&scene, &resolved, kit.sims.clone());
        a.seek(3.0);
        let mut b = Playback::new(&scene, &resolved, kit.sims.clone());
        b.seek(2.0);
        b.seek(1.0);
        b.seek(3.0);
        assert_eq!(points(&a), points(&b));
        assert_eq!(a.log(), b.log());
        assert!(points(&a)[0].pinned());
        assert!(points(&a).last().unwrap().position.y > -300.0, "the free end swung down");
    }
}

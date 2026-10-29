//! Placing the particles, pouring them in and settling them every substep.

use bpe_core::{ResolvedThing, Settings, SlotId, Thing, ThingId};
use bpe_runtime::{Bodies, Outline, ThingSim, Wall, World, keep_inside, state, state_mut};
use glam::Vec2;

use super::pbf::{Pbf, Rect, Scratch, lattice, spacing_for};
use super::{LiquidSettings, LiquidStart};
use crate::modules::bounce::sim::BounceState;
use crate::modules::motion_liquid::Flowing;
use crate::rng::Rng;

pub struct LiquidSim;

/// The video size a liquid fills when there is no wall to fill.
const NO_WALL: Rect = Rect { min: Vec2::new(-540.0, -960.0), max: Vec2::new(540.0, 960.0) };
/// How fast poured particles leave the spout.
const POUR_SPEED: f32 = 200.0;

/// What the liquid keeps between steps.
#[derive(Clone, Debug, PartialEq)]
pub struct LiquidState {
    pub settings: LiquidSettings,
    pub position: Vec2,
    /// From one particle's middle to the next at rest.
    pub spacing: f32,
    /// Whether the graphics card runs this liquid (then it has no bodies here).
    pub gpu: bool,
    /// Where the particles start.
    pub region: Rect,
    /// Particles poured in so far.
    pub poured: u32,
    pub rng: Rng,
    /// The solver and the flowing settings it was built for.
    pbf: Pbf,
    pbf_for: (f32, f32),
    scratch: Scratch,
}

impl LiquidState {
    /// The particle radius.
    pub fn radius(&self) -> f32 {
        self.spacing * 0.5
    }
}

/// The first wall in id order around `position`, else the first wall at all.
pub fn wall_of(world: &World, position: Vec2) -> Option<(ThingId, Wall)> {
    world
        .walls
        .iter()
        .find(|(_, w)| w.outline.inside(position))
        .or_else(|| world.walls.iter().next())
        .map(|(id, w)| (*id, *w))
}

/// The box around an outline.
fn bounds(outline: &Outline) -> Rect {
    match *outline {
        Outline::Ring { center, radius, .. } => {
            Rect { min: center - Vec2::splat(radius), max: center + Vec2::splat(radius) }
        }
        Outline::Box { center, half, angle, .. } | Outline::Rounded { center, half, angle, .. } => {
            let (c, s) = (angle.cos().abs(), angle.sin().abs());
            let extent = Vec2::new(c * half.x + s * half.y, s * half.x + c * half.y);
            Rect { min: center - extent, max: center + extent }
        }
    }
}

/// Where the liquid starts: the lower `fill` of the wall's box (or of the video when there is
/// no wall) for a pool, or a block around the thing.
pub fn region(settings: &LiquidSettings, wall: Option<&Outline>, position: Vec2) -> Rect {
    match settings.start {
        LiquidStart::Pool => {
            let b = wall.map_or(NO_WALL, bounds);
            let fill = settings.fill.clamp(0.0, 1.0);
            Rect { min: Vec2::new(b.min.x, b.max.y - b.size().y * fill), max: b.max }
        }
        LiquidStart::Block => {
            let half = Vec2::new(settings.width, settings.height) * 0.5;
            Rect { min: position - half, max: position + half }
        }
    }
}

/// Whether a particle of `radius` at `p` fits inside the wall without being moved.
fn fits(p: Vec2, radius: f32, wall: Option<&Wall>) -> bool {
    let Some(wall) = wall else { return true };
    let mut q = p;
    keep_inside(&mut q, Vec2::ZERO, radius, wall);
    q.distance_squared(p) < 1e-6
}

/// How much of `region` lies inside the wall, from a grid of samples.
fn kept_fraction(region: &Rect, wall: Option<&Wall>) -> f32 {
    const SAMPLES: u32 = 40;
    let size = region.size();
    let mut kept = 0;
    for y in 0..SAMPLES {
        for x in 0..SAMPLES {
            let p = region.min + size * Vec2::new((x as f32 + 0.5) / SAMPLES as f32, (y as f32 + 0.5) / SAMPLES as f32);
            if fits(p, 0.0, wall) {
                kept += 1;
            }
        }
    }
    (kept as f32 / (SAMPLES * SAMPLES) as f32).max(0.01)
}

/// The spacing that fits `count` particles into the part of `region` inside the wall.
pub fn spacing(settings: &LiquidSettings, region: &Rect, wall: Option<&Wall>) -> f32 {
    spacing_for(settings.count, region.area() * kept_fraction(region, wall)) * 0.98
}

impl ThingSim for LiquidSim {
    fn kind(&self) -> &'static str {
        "liquid"
    }

    fn needs_walls(&self) -> bool {
        true
    }

    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let settings = LiquidSettings::from_params(&resolved.params);
        let position = thing.transform.position;
        let wall = wall_of(world, position).map(|(_, w)| w);
        let region = region(&settings, wall.as_ref().map(|w| &w.outline), position);
        let mut spacing = spacing(&settings, &region, wall.as_ref());
        let gpu = settings.count > settings.cpu_limit;
        if !gpu {
            let count = settings.count as usize;
            let mut points = Vec::new();
            // A region smaller than it looked (an odd wall) gets a tighter lattice until all fit.
            for _ in 0..6 {
                points = lattice(&region, spacing, |p| fits(p, spacing * 0.5, wall.as_ref()), count);
                if points.len() >= count {
                    break;
                }
                spacing *= 0.9;
            }
            let mut bodies = Bodies { silent: true, ..Bodies::default() };
            let pouring = settings.pour_seconds > 0.0;
            for p in points {
                bodies.push(if pouring { position } else { p }, Vec2::ZERO, spacing * 0.5);
            }
            if pouring {
                bodies.alive.iter_mut().for_each(|a| *a = false);
            }
            world.bodies.insert(thing.id, bodies);
        }
        let flowing = Flowing::default();
        let st = LiquidState {
            rng: Rng::new(settings.seed, thing.id.0),
            settings,
            position,
            spacing,
            gpu,
            region,
            poured: 0,
            pbf: Pbf::new(spacing, flowing.stiffness, flowing.viscosity),
            pbf_for: (flowing.stiffness, flowing.viscosity),
            scratch: Scratch::default(),
        };
        world.states.insert(thing.id, Box::new(st));
    }

    fn spawn(&self, me: ThingId, world: &mut World, _log: &mut bpe_runtime::Log) {
        let time = world.time() as f32;
        let World { bodies, states, .. } = world;
        let st = state_mut::<LiquidState>(states.get_mut(&me).unwrap().as_mut());
        if st.gpu || st.settings.pour_seconds <= 0.0 {
            return;
        }
        let Some(bodies) = bodies.get_mut(&me) else { return };
        let due = ((time / st.settings.pour_seconds).min(1.0) * bodies.len() as f32).floor() as u32;
        let spread = st.spacing * 2.0;
        for i in st.poured..due.min(bodies.len() as u32) {
            let i = i as usize;
            let p = st.position + Vec2::new(st.rng.range(-spread, spread), st.rng.range(-spread, spread) * 0.25);
            bodies.position[i] = p;
            bodies.previous[i] = p;
            bodies.velocity[i] = Vec2::new(st.rng.range(-20.0, 20.0), POUR_SPEED);
            bodies.alive[i] = true;
        }
        st.poured = st.poured.max(due);
    }

    fn settle(&self, me: ThingId, world: &mut World, dt: f32) {
        if state::<LiquidState>(world.states[&me].as_ref()).gpu {
            return;
        }
        let flowing = match world.module_state(me, SlotId::Motion) {
            Some(("motion.liquid", st)) => state::<Flowing>(st).clone(),
            _ => Flowing::default(),
        };
        let bounciness = match world.module_state(me, SlotId::Bounce) {
            Some(("bounce.basic", st)) => state::<BounceState>(st).settings.bounciness,
            _ => 0.0,
        };
        let pushers: Vec<(Vec2, f32)> = world
            .bodies
            .iter()
            .filter(|(id, b)| **id != me && !b.silent)
            .flat_map(|(_, b)| (0..b.len()).filter(|&i| b.alive[i]).map(|i| (b.position[i], b.radius[i])))
            .collect();
        let World { bodies, walls, states, .. } = world;
        let st = state_mut::<LiquidState>(states.get_mut(&me).unwrap().as_mut());
        let Some(bodies) = bodies.get_mut(&me) else { return };
        if st.pbf_for != (flowing.stiffness, flowing.viscosity) {
            st.pbf = Pbf::new(st.spacing, flowing.stiffness, flowing.viscosity);
            st.pbf_for = (flowing.stiffness, flowing.viscosity);
        }
        st.pbf.relax(bodies, dt, walls, &pushers, flowing.ball_push, bounciness, &mut st.scratch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{ARENA, BALLS, run, scene_with};
    use bpe_core::{SlotId, Target, ThingId, Value};
    use bpe_runtime::{EventKind, World};
    use glam::Vec2;

    fn pool(
        count: i64,
        f: impl FnOnce(&mut bpe_core::Params),
    ) -> (bpe_core::Scene, bpe_core::Resolved, crate::Kit, ThingId) {
        scene_with("liquid", "Pool", Vec2::ZERO, |p| {
            p.set("count", Value::Int(count));
            f(p);
        })
    }

    #[test]
    fn a_pool_fills_the_bottom_of_the_arena_with_silent_particles() {
        let (scene, resolved, kit, me) = pool(500, |p| p.set("fill", Value::Number(0.3)));
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let w = World::build(&scene, &resolved, &kit.sims);
        let bodies = &w.bodies[&me];
        assert_eq!(bodies.len(), 500);
        assert!(bodies.silent);
        assert!(bodies.position.iter().all(|p| p.length() < 508.0), "all inside the ring");
        let ys: Vec<f32> = bodies.position.iter().map(|p| p.y).collect();
        assert!(ys.iter().cloned().fold(f32::MIN, f32::max) > 400.0, "down at the bottom");
        assert!(ys.iter().cloned().fold(f32::MAX, f32::min) > 150.0, "only the lower part is filled");
        let st = state::<LiquidState>(w.states[&me].as_ref());
        assert!(!st.gpu);
        assert!((bodies.radius[0] - st.spacing * 0.5).abs() < 1e-5);
        assert!(st.spacing > 5.0 && st.spacing < 40.0, "{}", st.spacing);
    }

    #[test]
    fn a_pool_fills_an_arena_added_after_it() {
        // The arena comes after the pool in the tree, so it is built after it; the pool must
        // still find its wall.
        let (mut scene, _, kit, me) = pool(500, |p| p.set("fill", Value::Number(0.3)));
        scene.remove_thing(ARENA).unwrap();
        let later = scene.add_thing(&kit.registry, "arena", ThingId::ROOT).unwrap();
        assert!(later > me);
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let w = World::build(&scene, &resolved, &kit.sims);
        let ps = &w.bodies[&me].position;
        assert_eq!(ps.len(), 500);
        assert!(ps.iter().all(|p| p.length() < 508.0), "all inside the ring");
        assert!(ps.iter().map(|p| p.y).fold(f32::MIN, f32::max) > 400.0, "down at the bottom");
    }

    #[test]
    fn a_pool_fills_a_capsule_arena() {
        // Stage 3's rounded arenas: the pool keeps inside the capsule, not inside its box.
        let (mut scene, _, kit, me) = pool(600, |p| p.set("fill", Value::Number(0.3)));
        let arena = &mut scene.things.get_mut(&ARENA).unwrap().params;
        arena.set("shape", Value::Choice("capsule".into()));
        arena.set("width", Value::Number(600.0));
        arena.set("height", Value::Number(1400.0));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let w = World::build(&scene, &resolved, &kit.sims);
        let outline = w.walls[&ARENA].outline;
        assert!(matches!(outline, bpe_runtime::Outline::Rounded { .. }), "{outline:?}");
        let ps = &w.bodies[&me].position;
        assert_eq!(ps.len(), 600);
        assert!(ps.iter().all(|&p| outline.inside(p)), "every particle starts inside the capsule");
        let lowest = ps.iter().map(|p| p.y).fold(f32::MIN, f32::max);
        assert!(lowest > 500.0, "down in the round end: {lowest}");
        let (w, _) = run(&scene, &resolved, &kit, 1.0);
        assert!(w.bodies[&me].position.iter().all(|&p| outline.inside(p)), "and stays inside after settling");
    }

    #[test]
    fn a_liquid_without_a_wall_fills_a_video_sized_box() {
        let (mut scene, _, kit, me) = pool(300, |_| {});
        scene.remove_thing(ARENA).unwrap();
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let w = World::build(&scene, &resolved, &kit.sims);
        let ps = &w.bodies[&me].position;
        assert_eq!(ps.len(), 300);
        assert!(ps.iter().all(|p| p.x.abs() <= 540.0 && p.y <= 960.0 && p.y >= 960.0 - 0.35 * 1920.0 - 1.0), "{ps:?}");
        let (w, _) = run(&scene, &resolved, &kit, 1.0);
        assert!(w.bodies[&me].position.iter().all(|p| p.is_finite()));
    }

    #[test]
    fn the_liquid_settles_without_exploding_and_stays_inside() {
        let (scene, resolved, kit, me) = pool(800, |_| {});
        let (w1, _) = run(&scene, &resolved, &kit, 2.0);
        let (w2, _) = run(&scene, &resolved, &kit, 2.0);
        let b = &w1.bodies[&me];
        assert!(b.position.iter().all(|p| p.is_finite() && p.length() < 508.0), "stayed inside");
        let fastest = b.velocity.iter().fold(0.0f32, |m, v| m.max(v.length()));
        assert!(fastest < 3000.0, "{fastest}");
        assert_eq!(b.position, w2.bodies[&me].position, "two runs match");
        // Settled into a level pool: the top is roughly flat.
        let top = b.position.iter().map(|p| p.y).fold(f32::MAX, f32::min);
        let bottom = b.position.iter().map(|p| p.y).fold(f32::MIN, f32::max);
        assert!(bottom > 450.0 && top < 400.0, "top {top} bottom {bottom}");
    }

    #[test]
    fn balls_push_liquid_but_liquid_leaves_the_balls_log_alone() {
        let (mut scene, _, kit, me) = pool(600, |_| {});
        scene.things.get_mut(&BALLS).unwrap().params.set("velocity", Value::Vec2([0.0, 0.0]));
        scene.things.get_mut(&BALLS).unwrap().params.set("radius", Value::Number(60.0));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let (w_pushed, log) = run(&scene, &resolved, &kit, 2.5);
        // The same ball with no liquid at all.
        let mut dry = scene.clone();
        dry.remove_thing(me).unwrap();
        let dry_resolved = bpe_core::resolve(&dry, &kit.registry);
        let (_, dry_log) = run(&dry, &dry_resolved, &kit, 2.5);
        assert_eq!(log, dry_log, "the liquid never touches the ball's physics");
        assert!(log.events().iter().all(|e| e.kind == EventKind::Hit && e.a.thing == BALLS));
        // Turning the ball push off changes where the liquid ends up.
        let mut unpushed = scene.clone();
        let m = unpushed.add_module(&kit.registry, "motion.liquid", "Still").unwrap();
        unpushed.library.modules.get_mut(&m).unwrap().params.set("ball_push", Value::Number(0.0));
        unpushed.link(&kit.registry, me, bpe_core::SlotId::Motion, m).unwrap();
        let unpushed_resolved = bpe_core::resolve(&unpushed, &kit.registry);
        let (w_unpushed, _) = run(&unpushed, &unpushed_resolved, &kit, 2.5);
        assert_ne!(w_pushed.bodies[&me].position, w_unpushed.bodies[&me].position);
        // With the push on, nothing sits inside the ball.
        let ball = w_pushed.bodies[&BALLS].position[0];
        let inside = w_pushed.bodies[&me].position.iter().filter(|p| p.distance(ball) < 60.0 - 2.0).count();
        assert!(inside <= 3, "{inside} particles inside the ball");
    }

    #[test]
    fn a_pool_never_bumps_the_balls_even_when_their_bounce_hits_balls() {
        // Stage 3's ball-to-ball stage must leave silent particles out, or a pool would shove
        // the ball through the pair solver instead of only being pushed by it.
        let (mut scene, _, kit, me) = pool(400, |p| p.set("fill", Value::Number(0.5)));
        let bounce = bpe_core::resolve_slot(&scene, &kit.registry, BALLS, SlotId::Bounce, &mut Vec::new())
            .expect("the ball bounces")
            .module;
        scene.library.modules.get_mut(&bounce).unwrap().params.set("hit_balls", Value::Bool(true));
        let ball = scene.things.get_mut(&BALLS).unwrap();
        ball.transform.position = Vec2::new(0.0, -300.0);
        ball.params.set("velocity", Value::Vec2([0.0, 600.0]));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let (with, log) = run(&scene, &resolved, &kit, 1.5);
        assert!(
            log.events().iter().all(|e| e.a.thing != me && e.b.is_none_or(|b| b.thing != me)),
            "the pool logged a hit"
        );
        let mut without = scene.clone();
        without.remove_thing(me).unwrap();
        let resolved = bpe_core::resolve(&without, &kit.registry);
        let (alone, _) = run(&without, &resolved, &kit, 1.5);
        assert_eq!(with.bodies[&BALLS].position, alone.bodies[&BALLS].position, "the pool moved the ball");
    }

    #[test]
    fn a_pour_appears_over_time() {
        let (scene, resolved, kit, me) = pool(400, |p| p.set("pour_seconds", Value::Number(2.0)));
        let (w, _) = run(&scene, &resolved, &kit, 1.0);
        let alive = w.bodies[&me].alive_count();
        assert!((150..=250).contains(&alive), "{alive}");
        assert_eq!(w.bodies[&me].len(), 400, "every particle has its row from the start");
        let (w, _) = run(&scene, &resolved, &kit, 2.5);
        assert_eq!(w.bodies[&me].alive_count(), 400);
        assert!(w.bodies[&me].position.iter().all(|p| p.is_finite() && p.length() < 508.0));
    }

    #[test]
    fn a_big_count_runs_on_the_gpu_and_has_no_bodies() {
        let (scene, resolved, kit, me) = pool(5000, |_| {});
        let w = World::build(&scene, &resolved, &kit.sims);
        assert!(!w.bodies.contains_key(&me));
        let st = state::<LiquidState>(w.states[&me].as_ref());
        assert!(st.gpu);
        assert!(st.spacing > 0.0);
        let (scene, resolved, kit, me) = pool(5000, |p| p.set("cpu_limit", Value::Int(6000)));
        let w = World::build(&scene, &resolved, &kit.sims);
        assert_eq!(w.bodies[&me].len(), 5000);
    }

    #[test]
    fn an_emitter_fills_a_liquid_with_particle_sized_drops() {
        let (mut scene, _, kit, me) = pool(50, |_| {});
        let spout = scene.add_thing(&kit.registry, "emitter", ThingId::ROOT).unwrap();
        let e = scene.things.get_mut(&spout).unwrap();
        e.transform.position = Vec2::new(0.0, -300.0);
        e.params.set("target", Value::Target(Target::Thing(me)));
        e.params.set("direction", Value::Number(90.0));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let (w, log) = run(&scene, &resolved, &kit, 1.0);
        let st = state::<LiquidState>(w.states[&me].as_ref());
        assert_eq!(w.bodies[&me].len(), 50 + 20);
        assert!((w.bodies[&me].radius[60] - st.spacing * 0.5).abs() < 1e-5);
        assert_eq!(log.events().iter().filter(|e| e.kind == EventKind::Spawn).count(), 20);
        assert!(log.events().iter().all(|e| e.kind != EventKind::Hit), "drops are silent too");
    }
}

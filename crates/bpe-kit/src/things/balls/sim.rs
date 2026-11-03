//! Placing the balls, letting them go and keeping their settings.

use bpe_core::{ResolvedThing, Settings, SlotId, Thing, ThingId};
use bpe_runtime::{Bodies, Log, Pose, ThingSim, World, state, state_mut};
use glam::Vec2;

use super::shapes::offsets;
use super::{BallsSettings, Launch, Release, ReleaseOrder};
use crate::rng::Rng;

pub use super::shapes::offsets as group_offsets;

pub struct BallsSim;

/// What the balls keep between steps.
#[derive(Clone, Debug, PartialEq)]
pub struct BallsState {
    pub settings: BallsSettings,
    /// Each body row's ball number: its place in the group's shape. Balls let go one by one
    /// are pushed in the order they go, so a row isn't always its number.
    pub numbers: Vec<u32>,
    /// Each body row's size, as a share of the Size setting (Variety).
    pub size_factor: Vec<f32>,
    /// Where each ball starts from the thing, by number.
    pub offsets: Vec<Vec2>,
    /// The balls still to be let go: the step each goes on and its number, soonest last.
    pub pending: Vec<(u64, u32)>,
}

impl BallsState {
    /// The ball number of body row `row` (the row itself for anything not listed).
    pub fn number(&self, row: usize) -> u32 {
        self.numbers.get(row).copied().unwrap_or(row as u32)
    }
}

/// The ball numbers of `thing`'s body rows, when it is a Balls thing in `world`.
pub fn numbers(world: &World, thing: ThingId) -> Option<&[u32]> {
    let st = world.states.get(&thing)?;
    st.as_any().downcast_ref::<BallsState>().map(|s| s.numbers.as_slice())
}

/// What Variety rolls for: a ball's size, its launch speed, its colour and its turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Roll {
    Size = 1,
    Speed = 2,
    Colour = 3,
    Turn = 4,
    Launch = 5,
    Order = 6,
}

/// Ball `number`'s own random number in 0 to 1 for `roll`, the same every run for one `seed`.
pub fn ball_unit(seed: i32, number: u32, roll: Roll) -> f32 {
    Rng::new(seed as u32, number ^ ((roll as u32) << 24)).unit()
}

/// `amount` of spread (0 to 1) as a factor round 1: 0.5 to 1.5 at full spread.
fn spread(seed: i32, number: u32, roll: Roll, amount: f32) -> f32 {
    if amount == 0.0 { 1.0 } else { 1.0 + amount.clamp(0.0, 1.0) * (ball_unit(seed, number, roll) - 0.5) }
}

/// Ball `number`'s size as a share of the Size setting.
pub fn size_factor(s: &BallsSettings, number: u32) -> f32 {
    if s.group { spread(s.seed, number, Roll::Size, s.vary_size) } else { 1.0 }
}

/// How far round the colour wheel ball `number`'s colours turn, in turns (Variety).
pub fn colour_turn(s: &BallsSettings, number: u32) -> f32 {
    if !s.group || s.vary_colour == 0.0 {
        return 0.0;
    }
    s.vary_colour.clamp(0.0, 1.0) * (ball_unit(s.seed, number, Roll::Colour) - 0.5)
}

/// How far ball `number`'s look is turned, in degrees (Variety).
pub fn look_turn(s: &BallsSettings, number: u32) -> f32 {
    if !s.group || s.vary_turn == 0.0 {
        return 0.0;
    }
    s.vary_turn * (2.0 * ball_unit(s.seed, number, Roll::Turn) - 1.0)
}

/// The ball numbers in the order they are let go.
pub fn release_order(s: &BallsSettings, offsets: &[Vec2]) -> Vec<u32> {
    let n = offsets.len() as u32;
    let mut order: Vec<u32> = (0..n).collect();
    // Let go all at once, the balls keep their numbers' order.
    if !s.group || s.release == Release::AllAtOnce {
        return order;
    }
    match s.release_order {
        ReleaseOrder::FirstToLast => {}
        ReleaseOrder::CentreOut => {
            let middle = offsets.iter().copied().sum::<Vec2>() / n.max(1) as f32;
            let d = |i: &u32| offsets[*i as usize].distance_squared(middle);
            order.sort_by(|a, b| d(a).total_cmp(&d(b)).then(a.cmp(b)));
        }
        ReleaseOrder::Random => {
            let mut rng = Rng::new(s.seed as u32, (Roll::Order as u32) << 24);
            for i in (1..order.len()).rev() {
                let j = (rng.next_u32() as usize) % (i + 1);
                order.swap(i, j);
            }
        }
    }
    order
}

/// The step the `k`th ball let go goes on, at `rate` steps a second.
pub fn release_step(s: &BallsSettings, k: usize, rate: u32) -> u64 {
    if !s.group {
        return 0;
    }
    let slot = match s.release {
        Release::AllAtOnce => return 0,
        Release::OneByOne => k as f64,
        Release::Waves => (k / s.wave_size.max(1) as usize) as f64,
    };
    (slot * s.release_gap.max(0.0) as f64 * rate as f64).round() as u64
}

/// Ball `number`'s launch velocity (before what it rides on adds), `offset` from the middle of a
/// group whose places' centre is `middle`, of `count` balls.
pub fn launch_velocity(s: &BallsSettings, number: u32, count: usize, offset: Vec2, middle: Vec2) -> Vec2 {
    let v = s.velocity;
    if !s.group {
        return v;
    }
    let speed = v.length();
    let way = if speed > 0.0 { v / speed } else { Vec2::X };
    let launched = match s.launch {
        Launch::Same => v,
        Launch::Fan => {
            let t = if count > 1 { number as f32 / (count - 1) as f32 } else { 0.5 };
            Vec2::from_angle((s.fan_angle * (t - 0.5)).to_radians()).rotate(v)
        }
        Launch::Outward => {
            let out = offset - middle;
            let dir = if out.length_squared() > 1e-8 { out.normalize() } else { way };
            dir * speed
        }
        Launch::Random => Vec2::from_angle(std::f32::consts::TAU * ball_unit(s.seed, number, Roll::Launch)) * speed,
    };
    let factor = spread(s.seed, number, Roll::Speed, s.vary_speed);
    if factor == 1.0 { launched } else { launched * factor }
}

/// Pushes ball `number` of the group, `offset` from the thing, as the thing stands at `at`.
fn push_ball(bodies: &mut Bodies, st: &mut BallsState, at: Pose, number: u32) {
    let s = &st.settings;
    let offset = st.offsets[number as usize];
    let turn = Vec2::from_angle(at.angle);
    let position = at.position + turn.rotate(offset);
    let middle = turn.rotate(st.offsets.iter().copied().sum::<Vec2>() / st.offsets.len().max(1) as f32);
    let launched = launch_velocity(s, number, st.offsets.len(), turn.rotate(offset), middle);
    let velocity = launched + at.velocity + (position - at.position).perp() * at.spin;
    let factor = size_factor(s, number);
    let radius = if factor == 1.0 { s.radius } else { s.radius * factor };
    bodies.push(position, velocity, radius);
    st.numbers.push(number);
    st.size_factor.push(factor);
}

impl ThingSim for BallsSim {
    fn kind(&self) -> &'static str {
        "balls"
    }

    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let settings = BallsSettings::from_params(&resolved.params);
        // Riders start where their parent is and move with it; everything else sits where its
        // own transform says.
        let at = world.anchor(thing.id);
        let places = offsets(&settings);
        let order = release_order(&settings, &places);
        let rate = world.steps_per_second;
        let mut st = BallsState {
            settings,
            numbers: Vec::with_capacity(places.len()),
            size_factor: Vec::with_capacity(places.len()),
            offsets: places,
            pending: Vec::new(),
        };
        let mut bodies = Bodies::default();
        for (k, number) in order.into_iter().enumerate() {
            match release_step(&st.settings, k, rate) {
                0 => push_ball(&mut bodies, &mut st, at, number),
                step => st.pending.push((step, number)),
            }
        }
        // Soonest last, so letting one go pops it.
        st.pending.reverse();
        world.bodies.insert(thing.id, bodies);
        world.states.insert(thing.id, Box::new(st));
        // A trail needs to know where the balls have been.
        if resolved.slots.get(&SlotId::Trail).is_some_and(|t| t.kind.starts_with("trail.")) {
            world.keep_history(thing.id);
        }
    }

    /// Lets go the balls whose time has come, where the thing is now.
    fn spawn(&self, thing: ThingId, world: &mut World, _log: &mut Log) {
        let Some(st) = world.states.get(&thing) else { return };
        let due = |st: &BallsState| st.pending.last().is_some_and(|(at, _)| *at <= world.step);
        if !due(state::<BallsState>(st.as_ref())) {
            return;
        }
        let at = world.anchor(thing);
        let step = world.step;
        let (Some(st), Some(bodies)) = (world.states.get_mut(&thing), world.bodies.get_mut(&thing)) else { return };
        let st = state_mut::<BallsState>(st.as_mut());
        while let Some(&(when, number)) = st.pending.last() {
            if when > step {
                break;
            }
            st.pending.pop();
            push_ball(bodies, st, at, number);
        }
    }

    /// A keyframe moved the size: every ball takes it, keeping its own share (Variety).
    /// Placement and launch speed only matter at the start, so they stay.
    fn settings_changed(&self, thing: ThingId, resolved: &ResolvedThing, world: &mut World) {
        let settings = BallsSettings::from_params(&resolved.params);
        let Some(st) = world.states.get_mut(&thing) else { return };
        let st = state_mut::<BallsState>(st.as_mut());
        if let Some(bodies) = world.bodies.get_mut(&thing) {
            for (row, r) in bodies.radius.iter_mut().enumerate() {
                let factor = st.size_factor.get(row).copied().unwrap_or(1.0);
                *r = if factor == 1.0 { settings.radius } else { settings.radius * factor };
            }
        }
        st.settings = settings;
    }

    /// Colour and turn spread only change how the balls look.
    fn affects_physics(&self, key: &str) -> bool {
        !matches!(key, "vary_colour" | "vary_turn")
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{ARENA, BALLS, scene_with_balls};
    use crate::{kit, new_scene};
    use bpe_core::{Address, Attach, Value, resolve};
    use bpe_runtime::{Log, World};
    use glam::Vec2;
    use std::f32::consts::FRAC_PI_2;

    #[test]
    fn balls_riding_a_spinning_arena_start_with_its_rim_speed() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        scene.things.get_mut(&ARENA).unwrap().params.set("spin", Value::Number(90.0));
        scene
            .set_parent(
                BALLS,
                ARENA,
                Attach::Ride { offset: Vec2::new(0.0, -300.0), turn_with_parent: true, item: None },
            )
            .unwrap();
        scene.things.get_mut(&BALLS).unwrap().params.set("velocity", Value::Vec2([0.0, 0.0]));
        let resolved = resolve(&scene, &kit.registry);
        let w = World::build(&scene, &resolved, &kit.sims);
        assert_eq!(w.bodies[&BALLS].position[0], Vec2::new(0.0, -300.0));
        let v = w.bodies[&BALLS].velocity[0];
        // The rim at (0, -300) turning clockwise (y down) moves in +x at 300 * pi/2 px/s.
        assert!((v.x - 300.0 * FRAC_PI_2).abs() < 0.5 && v.y.abs() < 1e-3, "{v:?}");
    }

    #[test]
    fn a_turned_block_turns_its_layout() {
        let (mut scene, _, kit) = scene_with_balls("block", |p| {
            p.set("columns", Value::Int(2));
            p.set("rows", Value::Int(1));
            p.set("spacing", Value::Number(10.0));
        });
        scene.things.get_mut(&BALLS).unwrap().transform.rotation = FRAC_PI_2;
        let resolved = resolve(&scene, &kit.registry);
        let w = World::build(&scene, &resolved, &kit.sims);
        let p = &w.bodies[&BALLS].position;
        assert!((p[0] - Vec2::new(0.0, -5.0)).length() < 1e-4 && (p[1] - Vec2::new(0.0, 5.0)).length() < 1e-4, "{p:?}");
    }

    #[test]
    fn a_single_ball_sits_at_the_thing_with_its_launch_speed() {
        let (scene, resolved, kit) = scene_with_balls("single", |p| p.set("velocity", Value::Vec2([5.0, 6.0])));
        let w = World::build(&scene, &resolved, &kit.sims);
        assert_eq!(w.bodies[&BALLS].position, vec![Vec2::ZERO]);
        assert_eq!(w.bodies[&BALLS].velocity, vec![Vec2::new(5.0, 6.0)]);
        assert_eq!(w.bodies[&BALLS].radius, vec![24.0]);
    }

    #[test]
    fn a_size_key_resizes_the_balls() {
        let (mut scene, _, kit) = scene_with_balls("block", |p| {
            p.set("columns", Value::Int(2));
            p.set("rows", Value::Int(1));
        });
        let size = Address { thing: BALLS, slot: None, key: "radius".into() };
        scene.set_key(&kit.registry, &size, 0.0, Value::Number(24.0)).unwrap();
        scene.set_key(&kit.registry, &size, 1.0, Value::Number(48.0)).unwrap();
        let resolved = resolve(&scene, &kit.registry);
        let mut w = World::build(&scene, &resolved, &kit.sims);
        assert_eq!(w.bodies[&BALLS].radius, vec![24.0, 24.0]);
        w.advance_to(1.5, &kit.sims, &mut Log::default());
        assert_eq!(w.bodies[&BALLS].radius, vec![48.0, 48.0]);
    }

    /// A grid group of `count` balls, at rest unless `f` says otherwise.
    fn group(count: i64, f: impl FnOnce(&mut bpe_core::Params)) -> (bpe_core::Scene, bpe_core::Resolved, crate::Kit) {
        scene_with_balls("single", |p| {
            p.set("group", Value::Bool(true));
            p.set("count", Value::Int(count));
            p.set("columns", Value::Int(4));
            f(p);
        })
    }

    #[test]
    fn group_defaults_leave_every_ball_as_it_was() {
        let (scene, resolved, kit) = group(8, |_| {});
        let w = World::build(&scene, &resolved, &kit.sims);
        let b = &w.bodies[&BALLS];
        assert_eq!(b.position, super::super::shapes::grid(8, 4, 56.0));
        assert!(b.velocity.iter().all(|v| *v == Vec2::new(300.0, -200.0)));
        assert!(b.radius.iter().all(|r| *r == 24.0));
        let st = bpe_runtime::state::<super::BallsState>(w.states[&BALLS].as_ref());
        assert_eq!(st.numbers, (0..8).collect::<Vec<u32>>());
    }

    #[test]
    fn one_by_one_lets_a_ball_go_every_gap_in_order() {
        let (scene, resolved, kit) = group(5, |p| p.set("release", Value::Choice("one_by_one".into())));
        let mut w = World::build(&scene, &resolved, &kit.sims);
        assert_eq!(w.bodies[&BALLS].len(), 1, "the first goes at once");
        let mut log = Log::default();
        let mut counts = Vec::new();
        for _ in 0..100 {
            w.step(&kit.sims, &mut log);
            counts.push(w.bodies[&BALLS].len());
        }
        // A gap of 0.1 s is 24 steps.
        assert_eq!(counts[22], 1);
        assert_eq!(counts[23], 2, "the second goes on step 24");
        assert_eq!(counts[47], 3);
        assert_eq!(counts[99], 5);
        assert_eq!(super::numbers(&w, BALLS).unwrap(), [0, 1, 2, 3, 4]);
    }

    #[test]
    fn centre_out_and_waves_set_who_goes_when() {
        let (scene, resolved, kit) = group(9, |p| {
            p.set("columns", Value::Int(3));
            p.set("release", Value::Choice("waves".into()));
            p.set("wave_size", Value::Int(4));
            p.set("release_order", Value::Choice("centre_out".into()));
        });
        let mut w = World::build(&scene, &resolved, &kit.sims);
        let first = super::numbers(&w, BALLS).unwrap().to_vec();
        assert_eq!(first.len(), 4, "one wave of four");
        assert_eq!(first[0], 4, "the middle of a 3 by 3 grid goes first");
        let mut log = Log::default();
        w.advance_to(0.15, &kit.sims, &mut log);
        assert_eq!(w.bodies[&BALLS].len(), 8);
        w.advance_to(0.25, &kit.sims, &mut log);
        assert_eq!(w.bodies[&BALLS].len(), 9);
        // A wave bigger than the group is one wave.
        let (scene, resolved, kit) = group(5, |p| {
            p.set("release", Value::Choice("waves".into()));
            p.set("wave_size", Value::Int(50));
        });
        assert_eq!(World::build(&scene, &resolved, &kit.sims).bodies[&BALLS].len(), 5);
    }

    #[test]
    fn a_fan_spreads_round_the_launch_and_outward_points_away() {
        let (scene, resolved, kit) = group(5, |p| {
            p.set("launch", Value::Choice("fan".into()));
            p.set("fan_angle", Value::Number(90.0));
            p.set("velocity", Value::Vec2([100.0, 0.0]));
        });
        let w = World::build(&scene, &resolved, &kit.sims);
        let v = &w.bodies[&BALLS].velocity;
        let deg = |v: Vec2| v.to_angle().to_degrees();
        assert!((deg(v[0]) + 45.0).abs() < 1e-3 && (deg(v[4]) - 45.0).abs() < 1e-3, "{v:?}");
        assert!(deg(v[2]).abs() < 1e-3, "the middle ball goes the launch way");
        assert!(v.iter().all(|v| (v.length() - 100.0).abs() < 1e-3));
        let (scene, resolved, kit) = group(4, |p| {
            p.set("shape", Value::Choice("ring".into()));
            p.set("launch", Value::Choice("outward".into()));
            p.set("velocity", Value::Vec2([0.0, 50.0]));
        });
        let w = World::build(&scene, &resolved, &kit.sims);
        let b = &w.bodies[&BALLS];
        for i in 0..4 {
            assert!(b.velocity[i].normalize().dot(b.position[i].normalize()) > 0.999, "ball {i} goes outward");
            assert!((b.velocity[i].length() - 50.0).abs() < 1e-3);
        }
    }

    #[test]
    fn varied_sizes_are_the_same_every_run_and_survive_a_size_key() {
        let (mut scene, _, kit) = group(6, |p| p.set("vary_size", Value::Number(1.0)));
        let size = Address { thing: BALLS, slot: None, key: "radius".into() };
        scene.set_key(&kit.registry, &size, 0.0, Value::Number(24.0)).unwrap();
        scene.set_key(&kit.registry, &size, 1.0, Value::Number(48.0)).unwrap();
        let resolved = resolve(&scene, &kit.registry);
        let mut w = World::build(&scene, &resolved, &kit.sims);
        let before = w.bodies[&BALLS].radius.clone();
        assert_eq!(before, World::build(&scene, &resolved, &kit.sims).bodies[&BALLS].radius);
        assert!(before.iter().any(|r| *r != 24.0) && before.iter().all(|r| (12.0..=36.0).contains(r)), "{before:?}");
        w.advance_to(1.5, &kit.sims, &mut Log::default());
        let after = &w.bodies[&BALLS].radius;
        for (a, b) in after.iter().zip(&before) {
            assert!((a / b - 2.0).abs() < 1e-5, "each keeps its own share: {a} {b}");
        }
    }

    #[test]
    fn seeking_back_replays_late_balls_the_same() {
        let (scene, resolved, kit) = group(12, |p| {
            p.set("release", Value::Choice("one_by_one".into()));
            p.set("release_order", Value::Choice("random".into()));
            p.set("vary_speed", Value::Number(0.8));
        });
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(2.0);
        let bodies = playback.world().bodies[&BALLS].clone();
        let numbers = super::numbers(playback.world(), BALLS).unwrap().to_vec();
        assert_eq!(bodies.len(), 12);
        playback.seek(0.3);
        playback.seek(2.0);
        assert_eq!(playback.world().bodies[&BALLS], bodies);
        assert_eq!(super::numbers(playback.world(), BALLS).unwrap(), numbers);
        let mut sorted = numbers.clone();
        sorted.sort();
        assert_eq!(sorted, (0..12).collect::<Vec<u32>>());
        assert_ne!(numbers, sorted, "a random order");
    }

    #[test]
    fn a_block_is_laid_out_row_by_row_and_a_ring_clockwise_from_the_top() {
        let (scene, resolved, kit) = scene_with_balls("block", |p| {
            p.set("columns", Value::Int(2));
            p.set("rows", Value::Int(2));
            p.set("spacing", Value::Number(10.0));
        });
        let w = World::build(&scene, &resolved, &kit.sims);
        assert_eq!(
            w.bodies[&BALLS].position,
            vec![Vec2::new(-5.0, -5.0), Vec2::new(5.0, -5.0), Vec2::new(-5.0, 5.0), Vec2::new(5.0, 5.0)]
        );
        let (scene, resolved, kit) = scene_with_balls("ring", |p| {
            p.set("count", Value::Int(4));
            p.set("ring_radius", Value::Number(10.0));
        });
        let w = World::build(&scene, &resolved, &kit.sims);
        let p = &w.bodies[&BALLS].position;
        assert_eq!(p.len(), 4);
        assert!((p[0] - Vec2::new(0.0, -10.0)).length() < 1e-4, "{p:?}");
        assert!((p[1] - Vec2::new(10.0, 0.0)).length() < 1e-4, "{p:?}");
    }
}

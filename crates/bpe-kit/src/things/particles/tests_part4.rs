//! Tests of particles part 4: particles that feed other particles, popping, sticking, settling,
//! piles and two-way.

use bpe_core::{Resolved, Scene, Settings, SlotId, ThingId, resolve};
use bpe_runtime::{Log, Playback, World, main_part};
use glam::Vec2;

use super::feeds::FeedKind;
use super::forces::GRAVITY;
use super::looks::{Look, add_particles};
use super::meets::{AtBalls, AtEdges, AtShapes, BallAt, BallGrid, MeetsSettings, kicked};
use super::piles::{self, COLUMN, FLOOR, Pile, Piles, Surfaces};
use super::sim::state_of;
use super::state::{ParticlesState, RESTING, STUCK, Speck};
use super::step::{self, hash_of};
use super::two_way::{self, TwoWayOut};
use super::{BornFrom, Emit, FedWhen, Heading, KIND, ParticlesSettings};
use crate::modules::motion::BasicMotion;
use crate::things::balls::BallsSettings;
use crate::things::scene::SceneSettings;
use crate::{kit, registry};

fn set_source(scene: &mut Scene, id: ThingId, s: &ParticlesSettings) {
    scene.things.get_mut(&id).unwrap().params = s.to_params();
}

fn set_meets(scene: &mut Scene, id: ThingId, m: &MeetsSettings) {
    scene.things.get_mut(&id).unwrap().slots.get_mut(&SlotId::Bounce).unwrap().params = m.to_params();
}

fn meets_of(scene: &Scene, id: ThingId) -> MeetsSettings {
    MeetsSettings::from_params(&scene.things[&id].slots[&SlotId::Bounce].params)
}

fn run(scene: &Scene, time: f64) -> (World, Log, Resolved) {
    let kit = kit();
    let resolved = resolve(scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    let mut world = World::build(scene, &resolved, &kit.sims);
    let mut log = Log::default();
    world.advance_to(time, &kit.sims, &mut log);
    (world, log, resolved)
}

fn particles(world: &World, id: ThingId) -> &ParticlesState {
    state_of(world, id).expect("the thing has particles")
}

/// A blank project with an Empty Particles thing set to `parent` under the root, and an Empty
/// inside it set to `child` (fed by the first).
fn family(parent: ParticlesSettings, child: ParticlesSettings) -> (Scene, ThingId, ThingId) {
    let reg = registry();
    let mut scene = crate::new_studio_scene(&reg);
    let p = add_particles(&mut scene, &reg, Look::Empty, ThingId::ROOT).unwrap();
    set_source(&mut scene, p, &parent);
    let c = add_particles(&mut scene, &reg, Look::Empty, p).unwrap();
    set_source(&mut scene, c, &child);
    (scene, p, c)
}

fn still_parent(count: u32, life: f32) -> ParticlesSettings {
    ParticlesSettings {
        from: BornFrom::AnArea,
        emit: Emit::OnceAtTheStart,
        count,
        life,
        life_spread: 0.0,
        speed: 0.0,
        speed_spread: 0.0,
        ..ParticlesSettings::default()
    }
}

fn fed_child(when: FedWhen, count: u32) -> ParticlesSettings {
    ParticlesSettings {
        from: BornFrom::ItsParentsParticles,
        fed_when: when,
        count,
        life: 100.0,
        speed: 0.0,
        speed_spread: 0.0,
        ..ParticlesSettings::default()
    }
}

/// The test scene's ball and a scene with it and the hollow circle round it.
fn arena() -> (Scene, ThingId) {
    let reg = registry();
    let scene = crate::new_scene(&reg);
    let ball = scene.order.iter().copied().find(|t| scene.things[t].kind == "balls").unwrap();
    (scene, ball)
}

fn ball_settings(scene: &mut Scene, ball: ThingId, f: impl FnOnce(&mut BallsSettings)) {
    let mut b = BallsSettings::from_params(&scene.things[&ball].params);
    f(&mut b);
    scene.things.get_mut(&ball).unwrap().params = b.to_params();
}

fn ball_motion(scene: &mut Scene, ball: ThingId, f: impl FnOnce(&mut BasicMotion)) {
    let card = scene.card_mut(ball, SlotId::Motion).unwrap();
    let mut m = BasicMotion::from_params(&card.params);
    f(&mut m);
    card.params = m.to_params();
}

#[test]
fn a_fed_thing_is_born_where_its_parent_s_particles_die() {
    let (scene, p, c) = family(still_parent(20, 0.5), fed_child(FedWhen::Die, 2));
    let (world, _, _) = run(&scene, 0.4);
    let places: Vec<Vec2> = particles(&world, p).specks.iter().map(|s| s.place).collect();
    assert_eq!(places.len(), 20);
    assert!(particles(&world, c).specks.is_empty());
    let (world, _, _) = run(&scene, 0.6);
    assert!(particles(&world, p).specks.is_empty());
    let born = &particles(&world, c).specks;
    assert_eq!(born.len(), 40, "two at each of the 20 deaths");
    for s in born {
        assert!(places.iter().any(|p| p.distance(s.place) < 1e-3), "born at a death: {:?}", s.place);
    }
    // Each keeps its parent particle's dice, for Colour from: Its parent particle.
    let parent_seeds: Vec<u32> = particles(&run(&scene, 0.4).0, p).specks.iter().map(|s| s.seed).collect();
    assert!(born.iter().all(|s| parent_seeds.contains(&s.kin_seed)));
}

#[test]
fn fed_things_chain_and_step_with_their_parent_at_any_physics_rate() {
    let reg = registry();
    let (mut scene, p, c) = family(still_parent(10, 0.3), fed_child(FedWhen::Die, 1));
    set_source(&mut scene, c, &ParticlesSettings { life: 0.3, life_spread: 0.0, ..fed_child(FedWhen::Die, 1) });
    let g = add_particles(&mut scene, &reg, Look::Empty, c).unwrap();
    set_source(&mut scene, g, &fed_child(FedWhen::Die, 3));
    for rate in [240, 60, 50, 480] {
        let root = scene.things.get_mut(&ThingId::ROOT).unwrap();
        let mut s = SceneSettings::from_params(&root.params);
        s.steps_per_second = rate;
        root.params = s.to_params();
        let (world, _, _) = run(&scene, 1.0);
        assert!(particles(&world, p).specks.is_empty() && particles(&world, c).specks.is_empty());
        assert_eq!(particles(&world, g).specks.len(), 30, "at {rate} Hz every death fed its child");
    }
}

#[test]
fn rockets_burst_once_at_the_top_of_their_climb() {
    let reg = registry();
    let (mut scene, p, c) = family(
        ParticlesSettings {
            from: BornFrom::APoint,
            emit: Emit::OnceAtTheStart,
            count: 10,
            life: 5.0,
            speed: 500.0,
            speed_spread: 0.0,
            angle: -90.0,
            spread: 10.0,
            ..ParticlesSettings::default()
        },
        fed_child(FedWhen::StopRising, 1),
    );
    scene.add_layer(&reg, p, SlotId::Motion, GRAVITY).unwrap();
    let (world, _, _) = run(&scene, 0.5);
    assert!(particles(&world, c).specks.is_empty(), "still climbing");
    let (world, _, _) = run(&scene, 3.0);
    assert_eq!(particles(&world, c).specks.len(), 10);
    let top = particles(&world, c).specks.iter().map(|s| s.place.y).fold(f32::INFINITY, f32::min);
    // 500²/(2·400) = 312 px up.
    assert!((top + 312.0).abs() < 15.0, "{top}");
}

#[test]
fn every_step_feeds_at_the_rate_a_parent_particle() {
    let (scene, _, c) =
        family(still_parent(10, 100.0), ParticlesSettings { rate: 60.0, ..fed_child(FedWhen::EveryStep, 0) });
    let (world, _, _) = run(&scene, 1.0);
    let n = particles(&world, c).specks.len() as i64;
    assert!((n - 600).abs() < 60, "{n}");
}

#[test]
fn rain_splashes_where_it_hits_a_shape_or_a_ball() {
    let reg = registry();
    let (mut scene, _) = arena();
    let rain = add_particles(&mut scene, &reg, Look::Rain, ThingId::ROOT).unwrap();
    let splashes = scene.order.iter().copied().find(|t| scene.things[t].parent == Some(rain)).expect("splashes");
    assert_eq!(scene.things[&splashes].kind, KIND);
    let s = ParticlesSettings::from_params(&scene.things[&splashes].params);
    assert_eq!((s.from, s.fed_when), (BornFrom::ItsParentsParticles, FedWhen::HitAnything));
    let (world, _, _) = run(&scene, 2.0);
    let st = particles(&world, splashes);
    assert!(!st.specks.is_empty(), "the rain splashed");
    // Splashes are born on the circle's line (radius 508) or on the ball.
    let ball = world.bodies.values().next().unwrap();
    for sp in st.specks.iter().filter(|s| s.age(st.step) < 0.02) {
        let on_ring = (sp.place.length() - 508.0).abs() < 12.0;
        let on_ball = sp.place.distance(ball.position[0]) < ball.radius[0] + 40.0;
        assert!(on_ring || on_ball, "{:?}", sp.place);
    }
}

#[test]
fn moments_are_the_same_for_any_number_of_threads() {
    let (mut scene, _) = arena();
    let reg = registry();
    let id = add_particles(&mut scene, &reg, Look::Rain, ThingId::ROOT).unwrap();
    let mut s = ParticlesSettings::from_params(&scene.things[&id].params);
    s.rate = 60_000.0;
    set_source(&mut scene, id, &s);
    let m = MeetsSettings { at_balls: AtBalls::Stick, two_way: true, ..meets_of(&scene, id) };
    set_meets(&mut scene, id, &m);
    let (world, log, _) = run(&scene, 1.0);
    let st = particles(&world, id).clone();
    assert!(st.specks.len() > 3 * step::CHUNK);
    let mut seen = Vec::new();
    for threads in [1, 3, 8] {
        let mut copy = st.clone();
        copy.wants = !0;
        for _ in 0..6 {
            step::step(&mut copy, &world, &log, threads, None);
        }
        seen.push((hash_of(&copy), copy.feeds.clone(), copy.two_way.clone()));
    }
    assert!(!seen[0].1.is_empty());
    assert!(seen.windows(2).all(|w| w[0] == w[1]));
}

#[test]
fn bubbles_pop_on_the_ball() {
    let reg = registry();
    let (mut scene, ball) = arena();
    ball_motion(&mut scene, ball, |m| m.gravity_scale = 0.0);
    ball_settings(&mut scene, ball, |b| b.velocity = Vec2::ZERO);
    let id = add_particles(&mut scene, &reg, Look::Empty, ThingId::ROOT).unwrap();
    set_source(
        &mut scene,
        id,
        &ParticlesSettings {
            from: BornFrom::APoint,
            rate: 200.0,
            speed: 200.0,
            angle: -90.0,
            spread: 0.0,
            life: 5.0,
            ..ParticlesSettings::default()
        },
    );
    set_meets(&mut scene, id, &MeetsSettings { at_balls: AtBalls::Pop, ..MeetsSettings::default() });
    let pops = add_particles(&mut scene, &reg, Look::Empty, id).unwrap();
    set_source(&mut scene, pops, &fed_child(FedWhen::Pop, 1));
    let (world, _, _) = run(&scene, 2.0);
    // Each popped bubble went, and left one pop.
    let (left, popped) = (particles(&world, id).specks.len(), particles(&world, pops).specks.len());
    assert!(popped > 100, "{popped}");
    assert!((left + popped).abs_diff(400) <= 2, "{left} + {popped}");
}

#[test]
fn particles_stick_to_a_ball_ride_it_and_shake_off_when_it_lands() {
    let reg = registry();
    let crowd = |fall_off: f32| {
        let (mut scene, _) = arena();
        let id = add_particles(&mut scene, &reg, Look::Empty, ThingId::ROOT).unwrap();
        set_source(
            &mut scene,
            id,
            &ParticlesSettings {
                from: BornFrom::AnArea,
                width: 900.0,
                height: 900.0,
                emit: Emit::KeepACrowd,
                count: 2000,
                life: 100.0,
                speed: 0.0,
                ..ParticlesSettings::default()
            },
        );
        set_meets(&mut scene, id, &MeetsSettings { at_balls: AtBalls::Stick, fall_off, ..MeetsSettings::default() });
        (scene, id)
    };
    let stuck =
        |world: &World, id: ThingId| particles(world, id).specks.iter().filter(|s| s.flags & STUCK != 0).count();
    let (scene, id) = crowd(0.0);
    let (world, log, _) = run(&scene, 4.0);
    assert!(log.events().iter().any(|e| e.kind == bpe_runtime::EventKind::Hit), "the ball lands");
    let ball = world.bodies.values().find(|b| !b.is_empty()).unwrap();
    let on = stuck(&world, id);
    assert!(on > 5, "{on}");
    for s in particles(&world, id).specks.iter().filter(|s| s.flags & STUCK != 0) {
        assert!((s.place.distance(ball.position[0]) - ball.radius[0]).abs() < 0.5, "on its surface");
        assert_eq!(s.velocity, ball.velocity[0], "moving with it");
    }
    let (scene, id) = crowd(1.0);
    let (shaken, _, _) = run(&scene, 4.0);
    assert!(stuck(&shaken, id) < on, "landing shook them off: {} vs {on}", stuck(&shaken, id));
}

#[test]
fn particles_settle_on_a_shape_and_fade_after() {
    let reg = registry();
    let (mut scene, ball) = arena();
    ball_settings(&mut scene, ball, |b| b.radius = 1.0);
    let id = add_particles(&mut scene, &reg, Look::Empty, ThingId::ROOT).unwrap();
    set_source(
        &mut scene,
        id,
        &ParticlesSettings {
            from: BornFrom::AnArea,
            width: 300.0,
            height: 100.0,
            emit: Emit::OnceAtTheStart,
            count: 100,
            life: 100.0,
            speed: 0.0,
            ..ParticlesSettings::default()
        },
    );
    scene.add_layer(&reg, id, SlotId::Motion, GRAVITY).unwrap();
    set_meets(
        &mut scene,
        id,
        &MeetsSettings { push: 0.0, at_shapes: AtShapes::Settle, fade_after: 2.0, ..MeetsSettings::default() },
    );
    let (world, _, _) = run(&scene, 3.0);
    let st = particles(&world, id);
    assert_eq!(st.specks.len(), 100);
    assert!(st.specks.iter().all(|s| s.flags & RESTING != 0), "all settled");
    assert!(st.specks.iter().all(|s| (s.place.length() - 508.0).abs() < 6.0), "on the circle");
    assert!(st.specks.iter().all(|s| s.velocity == Vec2::ZERO));
    // Their age stands still while they rest.
    assert!(st.specks.iter().all(|s| s.age(st.step) < 2.0));
    let (world, _, _) = run(&scene, 5.0);
    assert!(particles(&world, id).specks.is_empty(), "faded away after 2 s");
    // Most kept: only so many stay.
    set_meets(
        &mut scene,
        id,
        &MeetsSettings { push: 0.0, at_shapes: AtShapes::Settle, most_kept: 30, ..MeetsSettings::default() },
    );
    let (world, _, _) = run(&scene, 4.0);
    let n = particles(&world, id).specks.len();
    assert!((30..=40).contains(&n), "{n}");
}

#[test]
fn a_ball_going_by_kicks_a_settled_particle_up() {
    let mut s = Speck::at(Vec2::new(0.0, 0.0), 1, 7);
    s.flags |= RESTING;
    let m = MeetsSettings::default();
    let ball = |v: Vec2| BallAt { place: Vec2::new(30.0, 0.0), velocity: v, radius: 20.0, thing: ThingId(5), row: 0 };
    assert!(kicked(&s, &BallGrid::new(vec![ball(Vec2::new(200.0, 0.0))], m.reach), &m));
    assert!(!kicked(&s, &BallGrid::new(vec![ball(Vec2::ZERO)], m.reach), &m), "a still ball leaves it");
    assert!(!kicked(&s, &BallGrid::new(vec![ball(Vec2::X * 200.0)], m.reach), &MeetsSettings { push: 0.0, ..m }));
}

/// Surfaces of one flat ledge at y 100 from x 0 to 40, and the floor nowhere.
fn ledge() -> Surfaces {
    let mut s = Surfaces::default();
    s.shapes.insert(
        9,
        vec![vec![Vec2::new(0.0, 100.0), Vec2::new(40.0, 100.0), Vec2::new(40.0, 110.0), Vec2::new(0.0, 110.0)]],
    );
    s
}

#[test]
fn sand_heaps_up_to_its_angle_and_pours_off_a_ledge() {
    let surfaces = ledge();
    let mut piles = Piles::default();
    let i = Pile::column_at(20.0);
    let n = piles::pile_for(&mut piles, 9, i, 100.0, 0.0);
    let repose = 34f32.to_radians();
    let mut spilled = 0;
    for _ in 0..2000 {
        spilled += piles::add_sand(&mut piles, n, i, 0.4, repose, 0.4, &surfaces).len();
    }
    let p = &piles.list[n];
    assert_eq!(p.first, 0);
    assert_eq!(p.base.len(), 20, "the pile covers the ledge and no more");
    let allowed = repose.tan() * COLUMN;
    for k in 1..p.base.len() {
        let step = (p.top(k) - p.top(k - 1)).abs();
        assert!(step <= allowed + 0.05, "column {k}: {step} > {allowed}");
    }
    assert!(spilled > 100, "sand poured off the ends: {spilled}");
    let kept = p.height.iter().sum::<f32>();
    assert!(((kept + spilled as f32 * 0.4) - 800.0).abs() < 1.0, "none lost: {kept} + {spilled}");
    // A ball through the middle cuts a trench.
    let before = piles.sand();
    let cut = piles::cut_by_ball(&mut piles, Vec2::new(20.0, 95.0), 6.0);
    assert!(!cut.is_empty() && piles.sand() < before);
    assert!(piles::sinks(&piles, Vec2::new(20.0, 95.0), 6.0) <= 0.01, "the trench fits it");
}

#[test]
fn a_pour_piles_up_on_the_floor_and_a_moving_shape_crumbles_its_pile() {
    let reg = registry();
    let mut scene = crate::new_studio_scene(&reg);
    let id = add_particles(&mut scene, &reg, Look::SandThatPilesUp, ThingId::ROOT).unwrap();
    let (world, _, _) = run(&scene, 4.0);
    let st = particles(&world, id);
    assert!(!st.piles.list.is_empty(), "a heap on the floor");
    assert!(st.piles.list.iter().all(|p| p.key == FLOOR));
    let peak = st.piles.list.iter().flat_map(|p| p.height.iter()).fold(0f32, |a, b| a.max(*b));
    assert!(peak > 10.0, "{peak}");
    // No surfaces left: it crumbles into grains.
    let mut piles = st.piles.clone();
    let crumbled = piles::crumble_moved(&mut piles, &Surfaces::default());
    assert!(piles.list.is_empty() && !crumbled.is_empty());
}

/// A world of the arena scene with a two-way Particles thing whose state is `st`.
fn arena_world() -> (World, ThingId) {
    let (scene, ball) = arena();
    let kit = kit();
    let world = World::build(&scene, &resolve(&scene, &kit.registry), &kit.sims);
    (world, ball)
}

#[test]
fn balls_rest_on_piles_and_dig_in_when_they_land_hard() {
    let (mut world, ball) = arena_world();
    let r = world.bodies[&ball].radius[0];
    let mut st = ParticlesState::new(ThingId(50), None, ParticlesSettings::default());
    st.meets =
        MeetsSettings { at_shapes: AtShapes::PileUp, two_way: true, pile_bounce: 0.5, ..MeetsSettings::default() };
    let i = Pile::column_at(0.0);
    let n = piles::pile_for(&mut st.piles, FLOOR, i - 40, 300.0, 0.0);
    for k in 0..80 {
        if k > 0 {
            piles::pile_for(&mut st.piles, FLOOR, i - 40 + k, 300.0, 0.0);
        }
        st.piles.list[n].height[k as usize] = 50.0;
    }
    // A ball sunk 10 px into the sand, falling slowly: pushed up onto it, bouncing.
    world.bodies.get_mut(&ball).unwrap().position[0] = Vec2::new(0.0, 250.0 - r + 10.0);
    world.bodies.get_mut(&ball).unwrap().velocity[0] = Vec2::new(0.0, 100.0);
    two_way::hold_up(&mut st, &mut world, 1.0 / 240.0);
    let b = &world.bodies[&ball];
    assert!((b.position[0].y - (250.0 - r)).abs() < 0.1, "{:?}", b.position[0]);
    assert!((b.velocity[0].y + 50.0).abs() < 0.01, "bounced: {:?}", b.velocity[0]);
    // Hard: it digs in and throws sand up.
    world.bodies.get_mut(&ball).unwrap().position[0] = Vec2::new(0.0, 250.0 - r + 10.0);
    world.bodies.get_mut(&ball).unwrap().velocity[0] = Vec2::new(0.0, 900.0);
    let sand = st.piles.sand();
    two_way::hold_up(&mut st, &mut world, 1.0 / 240.0);
    assert!(st.piles.sand() < sand);
    assert!(!st.specks.is_empty(), "sand thrown up");
    assert!(world.bodies[&ball].velocity[0].y > 0.0 && world.bodies[&ball].velocity[0].y < 900.0);
}

#[test]
fn stuck_particles_weigh_a_ball_down_and_pushes_are_spent_once() {
    let (mut world, ball) = arena_world();
    world.gravity = Vec2::new(0.0, 500.0);
    let r = world.bodies[&ball].radius[0];
    let mut stuck = std::collections::BTreeMap::new();
    stuck.insert((ball.0, 0), r * r * 0.5);
    let out = TwoWayOut::of(None, &[], &[], &stuck, &world);
    assert_eq!(out.weigh[&(ball, 0)], Vec2::new(0.0, 250.0), "half its mass more: half gravity more");
    let mut st = ParticlesState::new(ThingId(50), None, ParticlesSettings::default());
    st.two_way = TwoWayOut { pushes: [((ball, 0), Vec2::new(10.0, 0.0))].into(), weigh: out.weigh };
    let v0 = world.bodies[&ball].velocity[0];
    two_way::apply(&mut st, &mut world, 0.01);
    two_way::apply(&mut st, &mut world, 0.01);
    let v = world.bodies[&ball].velocity[0] - v0;
    assert!((v - Vec2::new(10.0, 5.0)).length() < 1e-3, "{v:?}");
}

#[test]
fn two_way_rain_pushes_a_ball_and_fog_slows_one() {
    let reg = registry();
    let still_ball = |two: bool, look: Look, velocity: Vec2| {
        let (mut scene, ball) = arena();
        // No circle round it: the rain falls straight onto the ball.
        let ring = scene.order.iter().copied().find(|t| scene.things[t].kind == "shape").unwrap();
        scene.things.get_mut(&ring).unwrap().enabled = false;
        ball_motion(&mut scene, ball, |m| m.gravity_scale = 0.0);
        ball_settings(&mut scene, ball, |b| b.velocity = velocity);
        let id = add_particles(&mut scene, &reg, look, ThingId::ROOT).unwrap();
        scene.things.get_mut(&id).unwrap().transform.position = Vec2::new(0.0, -200.0);
        let mut m = meets_of(&scene, id);
        m.two_way = two;
        if look == Look::Rain {
            m.weight = 50.0;
        } else {
            m.weight = 1.0;
        }
        set_meets(&mut scene, id, &m);
        let (world, _, _) = run(&scene, 1.5);
        world.bodies[&ball].position[0]
    };
    let alone = still_ball(false, Look::Rain, Vec2::ZERO);
    let pushed = still_ball(true, Look::Rain, Vec2::ZERO);
    assert!(pushed.y > alone.y + 1.0, "rain pushed it down: {pushed:?} vs {alone:?}");
    let v = Vec2::new(300.0, 0.0);
    let free = still_ball(false, Look::ThickFog, v);
    let slowed = still_ball(true, Look::ThickFog, v);
    assert!(slowed.x < free.x - 1.0, "fog slowed it: {slowed:?} vs {free:?}");
}

#[test]
fn a_two_way_thing_steps_with_the_physics_and_counts_in_the_fingerprint() {
    let reg = registry();
    let kit = kit();
    let (mut scene, _) = arena();
    let id = add_particles(&mut scene, &reg, Look::Rain, ThingId::ROOT).unwrap();
    let splashes = scene.order.iter().copied().find(|t| scene.things[t].parent == Some(id)).unwrap();
    let print = |scene: &Scene| bpe_runtime::fingerprint(scene, &resolve(scene, &kit.registry), &kit.sims);
    let decoration = print(&scene);
    let m = MeetsSettings { weight: 2.0, ..meets_of(&scene, id) };
    set_meets(&mut scene, id, &m);
    assert_eq!(print(&scene), decoration, "decoration never changes the physics");
    let m = MeetsSettings { two_way: true, ..meets_of(&scene, id) };
    set_meets(&mut scene, id, &m);
    let two = print(&scene);
    assert_ne!(two, decoration);
    let m = MeetsSettings { weight: 3.0, ..meets_of(&scene, id) };
    set_meets(&mut scene, id, &m);
    assert_ne!(print(&scene), two, "its cards count");
    // A fed thing is decoration whatever its switch says.
    let m = MeetsSettings { two_way: true, ..meets_of(&scene, splashes) };
    set_meets(&mut scene, splashes, &m);
    let resolved = resolve(&scene, &kit.registry);
    let main = main_part(&scene, &resolved, &kit.sims);
    assert!(main.things.contains_key(&id) && !main.things.contains_key(&splashes));
    // The main world with the two-way rain steps the balls as the whole world does.
    let mut whole = Playback::new(&scene, &resolved, kit.sims.clone());
    let mut alone = Playback::new(&scene, &main, kit.sims.clone());
    whole.seek(2.0);
    alone.seek(2.0);
    assert_eq!(whole.log().hash(), alone.log().hash());
    assert_eq!(hash_of(particles(whole.world(), id)), hash_of(particles(alone.world(), id)));
}

#[test]
fn particles_two_way_off_still_never_move_the_balls_with_the_new_meetings() {
    let reg = registry();
    let kit = kit();
    let (mut scene, _) = arena();
    let id = add_particles(&mut scene, &reg, Look::Empty, ThingId::ROOT).unwrap();
    set_source(
        &mut scene,
        id,
        &ParticlesSettings {
            from: BornFrom::APoint,
            rate: 2000.0,
            speed: 900.0,
            heading: Heading::AllRound,
            life: 3.0,
            ..ParticlesSettings::default()
        },
    );
    for (at_balls, at_shapes) in [(AtBalls::Stick, AtShapes::PileUp), (AtBalls::Pop, AtShapes::Settle)] {
        set_meets(
            &mut scene,
            id,
            &MeetsSettings { at_balls, at_shapes, at_edges: AtEdges::Bounce, ..MeetsSettings::default() },
        );
        let resolved = resolve(&scene, &kit.registry);
        let mut full = Playback::new(&scene, &resolved, kit.sims.clone());
        let mut main = Playback::new(&scene, &main_part(&scene, &resolved, &kit.sims), kit.sims.clone());
        full.seek(3.0);
        main.seek(3.0);
        assert_eq!(full.log().hash(), main.log().hash(), "{at_balls:?} {at_shapes:?}");
    }
}

#[test]
fn moments_kinds_cover_what_children_ask_for() {
    assert_eq!(FedWhen::HitAnything.wants(), FeedKind::HitBall.bit() | FeedKind::HitShape.bit());
    assert_eq!(FedWhen::EveryStep.wants(), 0);
}

#[test]
fn two_way_rain_pushes_as_hard_at_any_physics_rate() {
    let reg = registry();
    let push_at = |rate: u32| {
        let fall = |two: bool| {
            let (mut scene, ball) = arena();
            let ring = scene.order.iter().copied().find(|t| scene.things[t].kind == "shape").unwrap();
            scene.things.get_mut(&ring).unwrap().enabled = false;
            ball_motion(&mut scene, ball, |m| m.gravity_scale = 0.0);
            ball_settings(&mut scene, ball, |b| b.velocity = Vec2::ZERO);
            let root = scene.things.get_mut(&ThingId::ROOT).unwrap();
            let mut s = SceneSettings::from_params(&root.params);
            s.steps_per_second = rate;
            root.params = s.to_params();
            let id = add_particles(&mut scene, &reg, Look::Rain, ThingId::ROOT).unwrap();
            scene.things.get_mut(&id).unwrap().transform.position = Vec2::new(0.0, -200.0);
            let m = MeetsSettings { two_way: true, weight: 50.0, ..meets_of(&scene, id) };
            let m = MeetsSettings { two_way: two, ..m };
            set_meets(&mut scene, id, &m);
            let (world, _, _) = run(&scene, 1.5);
            world.bodies[&ball].position[0].y
        };
        fall(true) - fall(false)
    };
    let (fast, slow) = (push_at(240), push_at(60));
    assert!(fast > 1.0, "{fast}");
    assert!((slow / fast - 1.0).abs() < 0.25, "pushed {slow} at 60 Hz, {fast} at 240 Hz");
}

#[test]
fn piles_that_grow_into_each_other_on_one_surface_join() {
    let surfaces = ledge();
    let mut piles = Piles::default();
    let (a, b) = (Pile::column_at(13.0), Pile::column_at(27.0));
    let na = piles::pile_for(&mut piles, 9, a, 100.0, 0.0);
    let nb = piles::pile_for(&mut piles, 9, b, 100.0, 0.0);
    assert_ne!(na, nb, "two pours, two piles");
    let repose = 34f32.to_radians();
    let mut poured = 0.0;
    for _ in 0..1000 {
        for (n, i) in [(na, a), (nb, b)] {
            let spilled = piles::add_sand(&mut piles, n, i, 0.4, repose, 0.4, &surfaces);
            poured += 0.4 - spilled.len() as f32 * 0.4;
        }
    }
    let before = piles.sand();
    let unspilled: f32 = piles.list.iter().map(|p| p.spill).sum();
    let heights = before / COLUMN;
    assert!((heights + unspilled - poured).abs() < 0.05, "{heights} + {unspilled} vs {poured}");
    piles::merge_touching(&mut piles);
    assert_eq!(piles.list.len(), 1, "one heap on the ledge");
    let p = &piles.list[0];
    assert_eq!((p.first, p.base.len()), (0, 20));
    assert!((piles.sand() - before).abs() < 1e-2, "no sand lost joining them");
    // Another surface's pile stays apart.
    let mut two = Piles::default();
    piles::pile_for(&mut two, 9, 0, 100.0, 0.0);
    piles::pile_for(&mut two, 9, 1, 60.0, 0.0);
    piles::merge_touching(&mut two);
    assert_eq!(two.list.len(), 2);
}

#[test]
fn sand_pouring_off_a_ledge_starts_clear_of_it() {
    let surfaces = ledge();
    let mut piles = Piles::default();
    let i = Pile::column_at(20.0);
    let n = piles::pile_for(&mut piles, 9, i, 100.0, 0.0);
    let repose = 34f32.to_radians();
    let mut spilled = Vec::new();
    for _ in 0..2000 {
        spilled.extend(piles::add_sand(&mut piles, n, i, 0.4, repose, 0.4, &surfaces));
    }
    assert!(!spilled.is_empty());
    for (at, side) in spilled {
        assert!(at.x < 0.0 || at.x > 40.0, "off the ledge: {at:?}");
        assert_eq!(side, if at.x < 0.0 { -1.0 } else { 1.0 });
    }
}

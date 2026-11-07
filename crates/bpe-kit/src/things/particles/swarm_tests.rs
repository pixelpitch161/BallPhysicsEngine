//! Tests of Huge swarms (particles part 8) on the software GPU, against the exact engine; each
//! skips without a graphics adapter.

use std::sync::{Arc, OnceLock};

use bpe_core::{Scene, Settings, SlotId, ThingId, Value, layered_key, resolve};
use bpe_gfx::swarm::{SwarmGpu, use_on_this_thread};
use bpe_runtime::{Log, Playback, World};
use glam::Vec2;

use super::forces::{AIR, DRAG, GRAVITY, RISE, SWAY, SWIRL, WANDER, WIND};
use super::look::{Engine, LookCardSettings};
use super::looks::{Look, add_particles};
use super::meets::{AtBalls, AtEdges, AtShapes, MeetsSettings};
use super::more_forces::{FLOCK, FLOW, MAGNETIC, PULL, PULSE, PUSH, RIPPLE, SPIROGRAPH, SPRING};
use super::sim::state_of;
use super::{AreaShape, BornFrom, Emit, GridBy, GridCovers, Heading, ParticlesSettings};
use crate::modules::air::AirSettings;
use crate::{kit, registry};

/// The one software GPU every test steps its swarms on, named on the calling thread.
fn gpu() -> bool {
    static GPU: OnceLock<Option<Arc<SwarmGpu>>> = OnceLock::new();
    let gpu = GPU.get_or_init(|| bpe_gfx::headless_device().ok().map(|(d, q)| SwarmGpu::for_device(&d, &q)));
    use_on_this_thread(gpu.clone());
    if gpu.is_none() {
        eprintln!("no GPU adapter available, skipping");
    }
    gpu.is_some()
}

fn set_engine(scene: &mut Scene, id: ThingId, engine: Engine) {
    let p = LookCardSettings { engine }.to_params();
    let card = scene.card_mut(id, SlotId::Look).unwrap();
    for (k, v) in p.iter() {
        card.params.set(k, v.clone());
    }
}

fn add_force(scene: &mut Scene, id: ThingId, kind: &str, settings: &[(&str, Value)]) {
    let reg = registry();
    let layer = scene.add_layer(&reg, id, SlotId::Motion, kind).unwrap();
    let card = scene.card_mut(id, SlotId::Motion).unwrap();
    for (key, v) in settings {
        card.params.set(&layered_key(layer, key), v.clone());
    }
}

fn set_meets(scene: &mut Scene, id: ThingId, m: &MeetsSettings) {
    scene.card_mut(id, SlotId::Bounce).unwrap().params = m.to_params();
}

fn run(scene: &Scene, time: f64) -> World {
    let kit = kit();
    let resolved = resolve(scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    let mut world = World::build(scene, &resolved, &kit.sims);
    let mut log = Log::default();
    world.advance_to(time, &kit.sims, &mut log);
    world
}

/// Each particle's place and velocity, oldest first: the exact engine's, or the card's.
fn read(world: &World, id: ThingId) -> Vec<(Vec2, Vec2)> {
    let st = state_of(world, id).expect("particles");
    match &st.swarm {
        Some(s) => s
            .buffers
            .as_ref()
            .expect("a card")
            .read()
            .iter()
            .map(|(m, _)| (Vec2::from(m.place), Vec2::from(m.velocity)))
            .collect(),
        None => st.specks.iter().map(|s| (s.place, s.velocity)).collect(),
    }
}

/// `scene` run to `time` exactly and as a swarm: each one's particles.
/// Each particle's place and velocity.
type Specks = Vec<(Vec2, Vec2)>;

fn both(scene: &Scene, id: ThingId, time: f64) -> (Specks, Specks) {
    let exact = read(&run(scene, time), id);
    let mut swarm = scene.clone();
    set_engine(&mut swarm, id, Engine::HugeSwarm);
    (exact, read(&run(&swarm, time), id))
}

/// How far apart the two runs' particles are at most, in place.
fn worst(exact: &[(Vec2, Vec2)], swarm: &[(Vec2, Vec2)]) -> f32 {
    assert_eq!(exact.len(), swarm.len(), "as many particles");
    exact.iter().zip(swarm).map(|(a, b)| a.0.distance(b.0)).fold(0.0, f32::max)
}

/// A blank project with a Particles thing letting `count` particles off from a point all
/// round at `speed`, living long.
fn burst(count: u32, speed: f32) -> (Scene, ThingId) {
    let reg = registry();
    let mut scene = crate::new_studio_scene(&reg);
    let id = add_particles(&mut scene, &reg, Look::Empty, ThingId::ROOT).unwrap();
    let s = ParticlesSettings {
        from: BornFrom::APoint,
        emit: Emit::OnceAtTheStart,
        count,
        speed,
        heading: Heading::AllRound,
        life: 20.0,
        ..ParticlesSettings::default()
    };
    scene.things.get_mut(&id).unwrap().params = s.to_params();
    set_meets(&mut scene, id, &MeetsSettings { push: 0.0, ..MeetsSettings::default() });
    (scene, id)
}

#[test]
fn every_force_moves_a_swarm_as_the_exact_engine_does() {
    if !gpu() {
        return;
    }
    let n = |x: f64| Value::Number(x);
    let cases: Vec<(&str, Vec<(&str, Value)>)> = vec![
        (GRAVITY, vec![("strength", n(500.0)), ("direction", n(70.0))]),
        (WIND, vec![("speed", n(300.0)), ("gusts", n(0.6)), ("gust_every", n(0.7)), ("waves", n(400.0))]),
        (DRAG, vec![("drag", n(2.0))]),
        (SWIRL, vec![("strength", n(600.0)), ("size", n(120.0))]),
        (WANDER, vec![("strength", n(400.0)), ("turns", n(2.0))]),
        (RISE, vec![("strength", n(300.0)), ("cools", n(0.5))]),
        (SWAY, vec![("sway", n(40.0)), ("sway_speed", n(1.5)), ("flutter", n(3.0))]),
    ];
    for (kind, settings) in cases {
        let (mut scene, id) = burst(300, 150.0);
        add_force(&mut scene, id, kind, &settings);
        let (exact, swarm) = both(&scene, id, 1.0);
        let off = worst(&exact, &swarm);
        assert!(off < 1.0, "{kind}: {off} px apart after a second");
    }
    // Only for: the old ones only.
    let (mut scene, id) = burst(100, 0.0);
    add_force(&mut scene, id, GRAVITY, &[("only", Value::Choice("older_than".into())), ("age", n(0.5))]);
    let (exact, swarm) = both(&scene, id, 1.0);
    assert!(worst(&exact, &swarm) < 0.5);
    assert!(swarm.iter().all(|p| p.1.y > 150.0 && p.1.y < 250.0), "pulled for the last half second only");
}

#[test]
fn a_swarm_follows_the_air_as_the_exact_engine_does() {
    if !gpu() {
        return;
    }
    let (mut scene, id) = burst(300, 50.0);
    let reg = registry();
    super::air::ensure_air_card(&mut scene, &reg).unwrap();
    let card = scene.card_mut(ThingId::ROOT, SlotId::Air).unwrap();
    let mut a = AirSettings::from_params(&card.params);
    a.wind_speed = 250.0;
    a.wind_direction = 30.0;
    a.gusts = 0.5;
    card.params = a.to_params();
    add_force(&mut scene, id, AIR, &[("grip", Value::Number(6.0))]);
    let (exact, swarm) = both(&scene, id, 1.0);
    let off = worst(&exact, &swarm);
    assert!(off < 1.0, "{off} px apart");
    let mean = swarm.iter().map(|p| p.1).sum::<Vec2>() / swarm.len() as f32;
    assert!(mean.x > 100.0 && mean.y > 50.0, "carried on the wind: {mean}");
}

#[test]
fn balls_push_and_bounce_a_swarm_and_its_edges_wrap_bounce_and_vanish() {
    if !gpu() {
        return;
    }
    let reg = registry();
    // The test arena: a ball bouncing in a ring, particles round it.
    let mut scene = crate::new_scene(&reg);
    let id = add_particles(&mut scene, &reg, Look::Empty, ThingId::ROOT).unwrap();
    let s = ParticlesSettings {
        from: BornFrom::AnArea,
        area: AreaShape::Circle,
        emit: Emit::OnceAtTheStart,
        count: 2000,
        radius: 500.0,
        speed: 0.0,
        life: 20.0,
        ..ParticlesSettings::default()
    };
    scene.things.get_mut(&id).unwrap().params = s.to_params();
    set_meets(&mut scene, id, &MeetsSettings { push: 0.8, at_balls: AtBalls::BounceOff, ..MeetsSettings::default() });
    let (exact, swarm) = both(&scene, id, 1.0);
    assert_eq!(exact.len(), swarm.len());
    let moved = swarm.iter().filter(|p| p.1.length() > 1.0).count();
    let moved_exact = exact.iter().filter(|p| p.1.length() > 1.0).count();
    assert!(moved > 0, "the ball pushes some: {moved} (exactly {moved_exact})");
    let close = exact.iter().zip(&swarm).filter(|(a, b)| a.0.distance(b.0) < 1.0).count();
    assert!(close * 100 >= exact.len() * 99, "nearly all where the exact engine has them: {close}");
    set_meets(&mut scene, id, &MeetsSettings { push: 0.0, at_balls: AtBalls::Vanish, ..MeetsSettings::default() });
    let (exact, swarm) = both(&scene, id, 1.0);
    assert!(swarm.len() < 2000, "some vanish at the ball");
    assert!(exact.len().abs_diff(swarm.len()) <= 2, "{} against {}", exact.len(), swarm.len());
    // The picture's edges, with particles flying out all round.
    for at_edges in [AtEdges::WrapRound, AtEdges::Bounce, AtEdges::Vanish] {
        let (mut scene, id) = burst(500, 900.0);
        set_meets(&mut scene, id, &MeetsSettings { push: 0.0, at_edges, ..MeetsSettings::default() });
        let (exact, swarm) = both(&scene, id, 2.0);
        assert_eq!(exact.len(), swarm.len(), "{at_edges:?}");
        assert!(worst(&exact, &swarm) < 2.0, "{at_edges:?}: {}", worst(&exact, &swarm));
    }
}

#[test]
fn no_particle_of_a_swarm_gets_through_a_ring_it_bounces_in() {
    if !gpu() {
        return;
    }
    let reg = registry();
    let mut scene = crate::new_scene(&reg);
    let id = add_particles(&mut scene, &reg, Look::Empty, ThingId::ROOT).unwrap();
    let s = ParticlesSettings {
        from: BornFrom::APoint,
        emit: Emit::OnceAtTheStart,
        count: 20_000,
        speed: 900.0,
        heading: Heading::AllRound,
        life: 20.0,
        ..ParticlesSettings::default()
    };
    scene.things.get_mut(&id).unwrap().params = s.to_params();
    set_meets(
        &mut scene,
        id,
        &MeetsSettings {
            push: 0.0,
            at_shapes: AtShapes::Bounce,
            bounciness: 1.0,
            friction: 0.0,
            ..MeetsSettings::default()
        },
    );
    set_engine(&mut scene, id, Engine::HugeSwarm);
    let world = run(&scene, 5.0);
    let wall = world.walls.values().next().unwrap();
    let centre = wall.outline.center();
    let got = read(&world, id);
    assert_eq!(got.len(), 20_000);
    let outside = got.iter().filter(|p| p.0.distance(centre) > 508.0 + 4.0).count();
    assert_eq!(outside, 0, "every one bounced back in");
    set_meets(&mut scene, id, &MeetsSettings { at_shapes: AtShapes::Vanish, ..MeetsSettings::default() });
    assert!(read(&run(&scene, 2.0), id).is_empty(), "with Vanish they go at the wall");
}

#[test]
fn a_swarm_keeps_its_crowd_and_most_particles() {
    if !gpu() {
        return;
    }
    let (mut scene, id) = burst(0, 30.0);
    let s = ParticlesSettings {
        from: BornFrom::WholeScreen,
        emit: Emit::KeepACrowd,
        count: 5000,
        life: 0.5,
        life_spread: 0.5,
        ..ParticlesSettings::default()
    };
    scene.things.get_mut(&id).unwrap().params = s.to_params();
    set_engine(&mut scene, id, Engine::HugeSwarm);
    // As in the exact engine, those that die in a step are made up for at the next.
    let n = read(&run(&scene, 3.0), id).len();
    assert!((4800..=5000).contains(&n), "{n}");
    let mut exact = scene.clone();
    set_engine(&mut exact, id, Engine::Exact);
    let m = read(&run(&exact, 3.0), id).len();
    assert!(n.abs_diff(m) < 60, "{n} against {m} exactly");
    // Most particles caps it.
    let s = ParticlesSettings { most: 1200, ..s };
    scene.things.get_mut(&id).unwrap().params = s.to_params();
    let n = read(&run(&scene, 1.0), id).len();
    assert!((1100..=1200).contains(&n), "{n}");
}

#[test]
fn a_seek_back_and_forth_gives_a_swarm_the_same_particles() {
    if !gpu() {
        return;
    }
    let (mut scene, id) = burst(0, 200.0);
    let s = ParticlesSettings {
        from: BornFrom::APoint,
        rate: 3000.0,
        speed: 300.0,
        heading: Heading::AllRound,
        life: 1.5,
        life_spread: 0.3,
        ..ParticlesSettings::default()
    };
    scene.things.get_mut(&id).unwrap().params = s.to_params();
    add_force(&mut scene, id, SWIRL, &[]);
    set_engine(&mut scene, id, Engine::HugeSwarm);
    let kit = kit();
    let resolved = resolve(&scene, &kit.registry);
    let sims = kit.sims.clone();
    let mut straight = Playback::new(&scene, &resolved, sims.clone());
    straight.seek(2.5);
    let mut jumpy = Playback::new(&scene, &resolved, sims);
    for t in [3.0, 1.0, 2.0, 0.4, 2.5] {
        jumpy.seek(t);
    }
    let (a, b) = (read(straight.world(), id), read(jumpy.world(), id));
    assert!(a.len() > 1000);
    assert_eq!(a, b, "the very same particles");
    assert!(straight.world().gpu_memory() > 0, "its video memory is counted");
}

#[test]
fn a_swarm_with_no_card_has_no_particles_and_draws_nothing() {
    use_on_this_thread(None);
    let (mut scene, id) = burst(1000, 100.0);
    set_engine(&mut scene, id, Engine::HugeSwarm);
    let world = run(&scene, 1.0);
    let st = state_of(&world, id).unwrap();
    assert!(st.swarm.as_ref().is_some_and(|s| s.buffers.is_none()));
    assert!(st.specks.is_empty());
    let kit = kit();
    let resolved = resolve(&scene, &kit.registry);
    let mut frame = bpe_gfx::Frame::default();
    bpe_gfx::build_frame(&scene, &resolved, &world, &Log::default(), 1.0, &kit.draws, &mut frame);
    assert!(frame.swarms.is_empty() && frame.particles.is_empty());
}

#[test]
fn a_swarm_draws_runs_in_the_order_its_pieces_go() {
    if !gpu() {
        return;
    }
    let reg = registry();
    let (mut scene, id) = burst(100, 100.0);
    let look = scene.things[&id].slots[&SlotId::Look].layers.first().map(|l| l.id);
    let layer = match look {
        Some(l) => l,
        None => scene.add_layer(&reg, id, SlotId::Look, super::look::SPRITES).unwrap(),
    };
    for kind in [super::look::SUB_OUTLINE, super::look::SUB_GLOW, super::look::SUB_SHADOW] {
        scene.add_sub_layer(&reg, id, SlotId::Look, layer, kind).unwrap();
    }
    set_engine(&mut scene, id, Engine::HugeSwarm);
    let world = run(&scene, 0.5);
    let kit = kit();
    let resolved = resolve(&scene, &kit.registry);
    let mut frame = bpe_gfx::Frame::default();
    bpe_gfx::build_frame(&scene, &resolved, &world, &Log::default(), 0.5, &kit.draws, &mut frame);
    let roles: Vec<u32> = frame.swarms.iter().chain(&frame.swarms_front).map(|r| r.look.k0[2]).collect();
    use bpe_gfx::swarm::draw::{ROLE_GLOW, ROLE_OUTLINE, ROLE_OWN, ROLE_SHADOW};
    assert_eq!(roles, [ROLE_SHADOW, ROLE_GLOW, ROLE_OWN, ROLE_OUTLINE]);
    assert!(frame.particles.is_empty() && frame.particles_front.is_empty(), "nothing drawn on the processor");
}

#[test]
fn a_swarm_looks_like_the_exact_engine_s_particles() {
    if !gpu() {
        return;
    }
    let gpu = bpe_gfx::swarm::on_this_thread().unwrap();
    let reg = registry();
    let mut scene = crate::new_studio_scene(&reg);
    let id = add_particles(&mut scene, &reg, Look::Glitter, ThingId::ROOT).unwrap();
    let s = ParticlesSettings {
        from: BornFrom::AnArea,
        area: AreaShape::Circle,
        radius: 400.0,
        emit: Emit::OnceAtTheStart,
        count: 3000,
        speed: 20.0,
        heading: Heading::AllRound,
        life: 4.0,
        ..ParticlesSettings::from_params(&scene.things[&id].params)
    };
    scene.things.get_mut(&id).unwrap().params = s.to_params();
    let picture = |scene: &Scene| {
        let kit = kit();
        let resolved = resolve(scene, &kit.registry);
        let mut world = World::build(scene, &resolved, &kit.sims);
        let mut log = Log::default();
        world.advance_to(1.0, &kit.sims, &mut log);
        let mut frame = bpe_gfx::Frame::default();
        bpe_gfx::build_frame(scene, &resolved, &world, &log, 1.0, &kit.draws, &mut frame);
        frame.width = 270;
        frame.height = 480;
        let mut renderer = bpe_gfx::Renderer::new(gpu.device(), 270, 480);
        renderer.render(gpu.device(), gpu.queue(), &frame);
        let px = renderer.read_output(gpu.device(), gpu.queue()).unwrap();
        let mut sum = [0u64; 3];
        for p in px.chunks(4) {
            for k in 0..3 {
                sum[k] += p[k] as u64;
            }
        }
        (sum.map(|x| x as f64 / (270.0 * 480.0)), frame)
    };
    let (exact, f) = picture(&scene);
    assert!(f.swarms.is_empty() && !f.particles.is_empty());
    let mut swarm = scene.clone();
    set_engine(&mut swarm, id, Engine::HugeSwarm);
    let (on_card, f) = picture(&swarm);
    assert!(!f.swarms.is_empty() && f.particles.is_empty());
    assert!(exact.iter().sum::<f64>() > 3.0, "something drawn: {exact:?}");
    for k in 0..3 {
        let off = (exact[k] - on_card[k]).abs() / exact[k].max(1.0);
        assert!(off < 0.05, "channel {k}: {} against {} exactly", on_card[k], exact[k]);
    }
}

#[test]
fn part_3_forces_move_a_swarm_as_the_exact_engine_does() {
    if !gpu() {
        return;
    }
    let n = |x: f64| Value::Number(x);
    let c = |x: &str| Value::Choice(x.into());
    let point = Value::Vec2([300.0, -200.0]);
    let cases: Vec<(&str, Vec<(&str, Value)>)> = vec![
        (PULL, vec![("strength", n(500.0)), ("orbit", n(0.6))]),
        (PULL, vec![("toward", c("a_point")), ("point", point.clone()), ("falloff", c("even")), ("reach", n(900.0))]),
        (PULL, vec![("toward", c("its_ball"))]),
        (PULL, vec![("swallow", n(30.0)), ("strength", n(3000.0)), ("only", c("a_share")), ("share", n(0.4))]),
        (PUSH, vec![("toward", c("its_place")), ("falloff", c("much_weaker_further")), ("only", c("the_rest"))]),
        (SPRING, vec![("stiffness", n(40.0)), ("damping", n(2.0))]),
        (MAGNETIC, vec![("from", c("its_place")), ("size_px", n(200.0)), ("pull", n(300.0))]),
        (FLOW, vec![("pattern", c("noise")), ("changes", n(0.3))]),
        (FLOW, vec![("pattern", c("waves")), ("angle", n(30.0)), ("sway", n(40.0)), ("changes", n(0.5))]),
        (FLOW, vec![("pattern", c("spiral")), ("pull_in", n(25.0))]),
        (FLOW, vec![("pattern", c("vortex")), ("size", n(120.0))]),
        (SPIROGRAPH, vec![("speed", n(0.6)), ("differ", n(0.5))]),
        (SPIROGRAPH, vec![("round", c("a_point")), ("point", point)]),
        (PULSE, vec![("strength", n(400.0)), ("way", c("outwards"))]),
        (PULSE, vec![("way", c("all_round")), ("decay", n(0.0))]),
    ];
    for (kind, settings) in cases {
        let (mut scene, id) = burst(300, 150.0);
        add_force(&mut scene, id, kind, &settings);
        let (exact, swarm) = both(&scene, id, 1.0);
        let off = worst(&exact, &swarm);
        assert!(off < 1.0, "{kind} {settings:?}: {off} px apart after a second");
    }
    // Swallowed ones go, the same ones.
    let (mut scene, id) = burst(300, 60.0);
    add_force(&mut scene, id, PULL, &[("swallow", n(40.0)), ("strength", n(4000.0)), ("falloff", c("even"))]);
    let (exact, swarm) = both(&scene, id, 1.0);
    assert!(exact.len() < 300, "some are swallowed");
    assert!(worst(&exact, &swarm) < 1.0);
}

#[test]
fn a_swarm_springs_home_to_its_grid_and_skips_a_flock() {
    if !gpu() {
        return;
    }
    let reg = registry();
    let mut scene = crate::new_studio_scene(&reg);
    let id = add_particles(&mut scene, &reg, Look::Empty, ThingId::ROOT).unwrap();
    let s = ParticlesSettings {
        from: BornFrom::AGrid,
        grid_covers: GridCovers::ARectangle,
        grid_by: GridBy::ColumnsAndRows,
        width: 300.0,
        height: 200.0,
        columns: 12,
        rows: 8,
        emit: Emit::OnceAtTheStart,
        life: 1000.0,
        speed: 0.0,
        homes_turn: 40.0,
        ..ParticlesSettings::default()
    };
    scene.things.get_mut(&id).unwrap().params = s.to_params();
    let n = |x: f64| Value::Number(x);
    add_force(&mut scene, id, WIND, &[("speed", n(400.0))]);
    add_force(&mut scene, id, SPRING, &[("stiffness", n(60.0))]);
    let (exact, swarm) = both(&scene, id, 1.5);
    assert_eq!(swarm.len(), 96);
    assert!(worst(&exact, &swarm) < 1.0, "every particle springs to its own turning place");
    // A flock moves the exact engine's particles but not a swarm's (it isn't on the card).
    let mut flocking = scene.clone();
    add_force(&mut flocking, id, FLOCK, &[]);
    set_engine(&mut flocking, id, Engine::HugeSwarm);
    let flock = read(&run(&flocking, 1.5), id);
    assert!(worst(&swarm, &flock) < 1e-3, "a flock changes nothing in a swarm");
}

#[test]
fn ripples_ball_pulls_and_magnets_from_a_bouncing_ball_match_the_exact_engine() {
    if !gpu() {
        return;
    }
    let reg = registry();
    let mut scene = crate::new_scene(&reg);
    let id = add_particles(&mut scene, &reg, Look::Empty, ThingId::ROOT).unwrap();
    let s = ParticlesSettings {
        from: BornFrom::AnArea,
        width: 900.0,
        height: 1400.0,
        emit: Emit::KeepACrowd,
        count: 2000,
        life: 100.0,
        speed: 50.0,
        heading: Heading::AllRound,
        ..ParticlesSettings::default()
    };
    scene.things.get_mut(&id).unwrap().params = s.to_params();
    set_meets(&mut scene, id, &MeetsSettings { push: 0.0, ..MeetsSettings::default() });
    let c = |x: &str| Value::Choice(x.into());
    add_force(&mut scene, id, PULL, &[("toward", c("balls_nearby"))]);
    add_force(&mut scene, id, RIPPLE, &[]);
    add_force(&mut scene, id, MAGNETIC, &[]);
    let (exact, swarm) = both(&scene, id, 3.0);
    let off = worst(&exact, &swarm);
    assert!(off < 1.0, "{off} px apart");
    let moved = exact.iter().filter(|p| p.1.length() > 60.0).count();
    assert!(moved > 20, "the ball and its hits moved some: {moved}");
}

#[test]
fn switching_the_engine_restarts_the_particle_playback() {
    let (mut scene, id) = burst(100, 100.0);
    let kit = kit();
    let print = |scene: &Scene| bpe_runtime::beside_fingerprint(scene, &resolve(scene, &kit.registry), &kit.sims);
    let exact = print(&scene);
    set_engine(&mut scene, id, Engine::HugeSwarm);
    assert_ne!(print(&scene), exact, "the particle playback starts over in the other engine");
}

#[test]
fn a_swarm_crowd_on_a_grid_fills_every_place_and_its_bound_shrinks_as_they_go() {
    if !gpu() {
        return;
    }
    let reg = registry();
    let mut scene = crate::new_studio_scene(&reg);
    let id = add_particles(&mut scene, &reg, Look::Empty, ThingId::ROOT).unwrap();
    let s = ParticlesSettings {
        from: BornFrom::AGrid,
        grid_covers: GridCovers::ARectangle,
        grid_by: GridBy::ColumnsAndRows,
        width: 300.0,
        height: 200.0,
        columns: 40,
        rows: 25,
        emit: Emit::KeepACrowd,
        life: 1000.0,
        speed: 0.0,
        ..ParticlesSettings::default()
    };
    scene.things.get_mut(&id).unwrap().params = s.to_params();
    set_engine(&mut scene, id, Engine::HugeSwarm);
    assert_eq!(read(&run(&scene, 0.5), id).len(), 1000, "every place, not Count's 30");
    // A burst that dies young leaves the card's bound (what each step dispatches) small.
    let (mut scene, id) = burst(5000, 100.0);
    let s = ParticlesSettings { life: 0.1, ..ParticlesSettings::from_params(&scene.things[&id].params) };
    scene.things.get_mut(&id).unwrap().params = s.to_params();
    set_engine(&mut scene, id, Engine::HugeSwarm);
    let world = run(&scene, 1.5);
    let buffers = state_of(&world, id).unwrap().swarm.as_ref().unwrap().buffers.clone().unwrap();
    assert!(buffers.read().is_empty());
    assert!(buffers.bound() < 5000, "the bound came down with the count: {}", buffers.bound());
}

/// How long a particle step of a million particles takes on this machine's card (gravity, wind
/// and swirl). `cargo test -p bpe-kit --release bench_a_million -- --ignored --nocapture`.
#[test]
#[ignore]
fn bench_a_million() {
    if !gpu() {
        return;
    }
    let n = |x: f64| Value::Number(x);
    let (mut scene, id) = burst(1_000_000, 200.0);
    let s = ParticlesSettings { most: 1_000_000, ..ParticlesSettings::from_params(&scene.things[&id].params) };
    scene.things.get_mut(&id).unwrap().params = s.to_params();
    add_force(&mut scene, id, GRAVITY, &[("strength", n(300.0))]);
    add_force(&mut scene, id, WIND, &[("speed", n(200.0)), ("gusts", n(0.5))]);
    add_force(&mut scene, id, SWIRL, &[("strength", n(400.0)), ("size", n(150.0))]);
    set_engine(&mut scene, id, Engine::HugeSwarm);
    let kit = kit();
    let resolved = resolve(&scene, &kit.registry);
    let mut world = World::build(&scene, &resolved, &kit.sims);
    let mut log = Log::default();
    world.advance_to(0.25, &kit.sims, &mut log);
    let buffers = state_of(&world, id).unwrap().swarm.as_ref().unwrap().buffers.as_ref().unwrap();
    assert_eq!(buffers.read().len(), 1_000_000);
    let started = std::time::Instant::now();
    world.advance_to(1.25, &kit.sims, &mut log);
    let buffers = state_of(&world, id).unwrap().swarm.as_ref().unwrap().buffers.as_ref().unwrap();
    let alive = buffers.read().len();
    let total = started.elapsed();
    let read_started = std::time::Instant::now();
    let _ = buffers.read();
    let read = read_started.elapsed();
    let steps = 120.0;
    let per = (total.saturating_sub(read)).as_secs_f64() * 1000.0 / steps;
    println!("{alive} particles: {per:.3} ms a particle step (one second {total:?}, a read back {read:?})");
}

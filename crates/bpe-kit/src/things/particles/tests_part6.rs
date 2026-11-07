//! Tests of particles part 6: trails, marks that stay, liquid, and the modifiers.

use bpe_core::{LayerId, Params, Resolved, Scene, Settings, SlotId, ThingId, layered_key, resolve};
use bpe_gfx::{Frame, Particle, TrailVertex, build_frame};
use bpe_runtime::{Log, Playback, World, beside_fingerprint};
use glam::Vec2;

use super::liquid::{LIQUID, LiquidLayer};
use super::looks::{Look, add_particles};
use super::marks::{MARKS, MarksLayer, ON_DEATH, ON_SHAPE};
use super::meets::{AtShapes, MeetsSettings};
use super::modifiers::{Mirror, Modifiers, Tears, glitch, mirror, split_into};
use super::sim::state_of;
use super::state::ParticlesState;
use super::step;
use super::trail::{TRAILS, TrailsLayer};
use super::{BornFrom, Emit, Heading, ParticlesSettings};
use crate::modules::look::layers::around_subs::{OWN, Pieces};
use crate::{kit, registry};

fn set_source(scene: &mut Scene, id: ThingId, s: &ParticlesSettings) {
    scene.things.get_mut(&id).unwrap().params = s.to_params();
}

fn set_meets(scene: &mut Scene, id: ThingId, m: &MeetsSettings) {
    scene.things.get_mut(&id).unwrap().slots.get_mut(&SlotId::Bounce).unwrap().params = m.to_params();
}

/// Sets the Look's only layer to `kind` with `params`.
fn only_layer(scene: &mut Scene, id: ThingId, kind: &str, params: Params) -> LayerId {
    let reg = registry();
    let old: Vec<LayerId> = scene.things[&id].slots[&SlotId::Look].layers.iter().map(|l| l.id).collect();
    let layer = scene.add_layer(&reg, id, SlotId::Look, kind).unwrap();
    for l in old {
        scene.remove_layer(&reg, id, SlotId::Look, l).unwrap();
    }
    put(scene, id, layer, params);
    layer
}

fn put(scene: &mut Scene, id: ThingId, layer: LayerId, params: Params) {
    let card = scene.things.get_mut(&id).unwrap().slots.get_mut(&SlotId::Look).unwrap();
    for (k, v) in params.0 {
        card.params.0.insert(layered_key(layer, &k), v);
    }
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

/// A blank project with a Particles thing dropping `count` particles from a point at the start.
fn falling(count: u32) -> (Scene, ThingId) {
    let reg = registry();
    let mut scene = crate::new_studio_scene(&reg);
    let id = add_particles(&mut scene, &reg, Look::Empty, ThingId::ROOT).unwrap();
    let source = ParticlesSettings {
        from: BornFrom::APoint,
        emit: Emit::OnceAtTheStart,
        count,
        life: 10.0,
        life_spread: 0.0,
        speed: 300.0,
        speed_spread: 0.0,
        angle: 0.0,
        spread: 0.0,
        ..ParticlesSettings::default()
    };
    set_source(&mut scene, id, &source);
    (scene, id)
}

/// The test arena (a hollow circle, one ball) with a Particles thing raining from a point.
fn raining_in_arena(rate: f32) -> (Scene, ThingId) {
    let reg = registry();
    let mut scene = crate::new_scene(&reg);
    let id = add_particles(&mut scene, &reg, Look::Empty, ThingId::ROOT).unwrap();
    let source = ParticlesSettings {
        from: BornFrom::APoint,
        rate,
        life: 4.0,
        speed: 500.0,
        heading: Heading::AllRound,
        ..ParticlesSettings::default()
    };
    set_source(&mut scene, id, &source);
    (scene, id)
}

// ---------- trails ----------

#[test]
fn no_trails_layer_keeps_no_ring() {
    let (scene, id) = falling(50);
    let (world, _, _) = run(&scene, 0.5);
    let st = particles(&world, id);
    assert_eq!(st.trail.points, 0);
    assert!(st.trail.places.is_empty());
    assert!(st.marks.is_empty());
}

#[test]
fn a_trail_holds_each_particles_last_places_newest_first() {
    let (mut scene, id) = falling(3);
    only_layer(&mut scene, id, TRAILS, TrailsLayer { length: 8, every: 2, ..TrailsLayer::default() }.to_params());
    let (world, _, _) = run(&scene, 1.0);
    let st = particles(&world, id);
    assert_eq!(st.trail.points, 15, "(8 - 1) × 2 + 1 places a particle");
    assert_eq!(st.trail.places.len(), 3 * 15);
    let s = st.specks[0];
    let points = st.trail.recent(0, &s, st.step, 2, 8);
    assert_eq!(points.len(), 8);
    assert_eq!(points[0], s.place, "the newest is where it is");
    // Going right at 300 px/s with no forces: each point two steps (1/60 s) behind the last.
    for w in points.windows(2) {
        let gap = w[0].x - w[1].x;
        assert!((gap - 5.0).abs() < 0.01, "{gap}");
        assert!((w[0].y - w[1].y).abs() < 1e-3);
    }
    // Just born, a particle has fewer points.
    let young = st.trail.recent(0, &super::state::Speck { born: st.step - 3, ..s }, st.step, 2, 8);
    assert_eq!(young.len(), 2, "steps 0 and 2 back, not 4");
}

#[test]
fn a_removed_particle_takes_its_trail_row_with_it() {
    let (mut scene, id) = raining_in_arena(400.0);
    only_layer(&mut scene, id, TRAILS, TrailsLayer { length: 4, every: 1, ..TrailsLayer::default() }.to_params());
    set_meets(&mut scene, id, &MeetsSettings { at_shapes: AtShapes::Vanish, ..MeetsSettings::default() });
    let (world, _, _) = run(&scene, 2.0);
    let st = particles(&world, id);
    assert!(st.births as usize > st.specks.len() + 50, "many have gone at the wall");
    assert_eq!(st.trail.places.len(), st.specks.len() * st.trail.points);
    // Each row's newest place is its own particle's place.
    let slot = st.step as usize % st.trail.points;
    for (i, s) in st.specks.iter().enumerate() {
        assert_eq!(st.trail.places[i * st.trail.points + slot], s.place, "row {i}");
    }
}

#[test]
fn trails_are_the_same_on_any_number_of_threads_and_after_a_seek() {
    let (mut scene, id) = falling(20_000);
    let mut s = ParticlesSettings::from_params(&scene.things[&id].params);
    s.heading = Heading::AllRound;
    set_source(&mut scene, id, &s);
    only_layer(&mut scene, id, TRAILS, TrailsLayer { length: 6, every: 1, ..TrailsLayer::default() }.to_params());
    let (world, log, _) = run(&scene, 0.2);
    let st = particles(&world, id).clone();
    let mut rings = Vec::new();
    for threads in [1, 3, 8] {
        let mut copy = st.clone();
        for _ in 0..5 {
            step::step(&mut copy, &world, &log, threads);
        }
        rings.push(copy.trail);
    }
    assert!(rings.windows(2).all(|w| w[0] == w[1]));
    let kit = kit();
    let resolved = resolve(&scene, &kit.registry);
    let mut p = Playback::new(&scene, &resolved, kit.sims.clone());
    p.seek(1.0);
    p.seek(0.3);
    p.seek(1.0);
    let (straight, _, _) = run(&scene, 1.0);
    assert_eq!(particles(p.world(), id).trail, particles(&straight, id).trail);
}

#[test]
fn trails_draw_one_strip_a_particle() {
    let (mut scene, id) = falling(10);
    only_layer(&mut scene, id, TRAILS, TrailsLayer { length: 5, every: 1, ..TrailsLayer::default() }.to_params());
    let frame = frame_of(&scene, 1.0);
    // Four segments, two triangles each.
    assert_eq!(frame.trails.len(), 10 * 4 * 6);
    assert!(frame.particles.is_empty());
}

#[test]
fn the_trail_length_rebuilds_the_particle_playback_but_its_colour_doesnt() {
    let (mut scene, id) = falling(10);
    let layer = only_layer(&mut scene, id, TRAILS, TrailsLayer::default().to_params());
    let kit = kit();
    let print = |scene: &Scene| beside_fingerprint(scene, &resolve(scene, &kit.registry), &kit.sims);
    let before = print(&scene);
    let mut recoloured = scene.clone();
    put(&mut recoloured, id, layer, TrailsLayer { opacity: 0.3, ..TrailsLayer::default() }.to_params());
    assert_eq!(print(&recoloured), before);
    let mut longer = scene.clone();
    put(&mut longer, id, layer, TrailsLayer { length: 40, ..TrailsLayer::default() }.to_params());
    assert_ne!(print(&longer), before);
    let physics = |scene: &Scene| bpe_runtime::fingerprint(scene, &resolve(scene, &kit.registry), &kit.sims);
    assert_eq!(physics(&longer), physics(&scene), "the physics never hears of it");
}

// ---------- marks ----------

#[test]
fn a_particle_leaves_one_mark_where_it_lands_on_a_shape() {
    let (mut scene, id) = raining_in_arena(100.0);
    only_layer(&mut scene, id, MARKS, MarksLayer::default().to_params());
    set_meets(&mut scene, id, &MeetsSettings { at_shapes: AtShapes::Stop, ..MeetsSettings::default() });
    let (world, _, _) = run(&scene, 2.0);
    let st = particles(&world, id);
    assert!(!st.marks.is_empty());
    assert!(st.marks.iter().all(|m| m.why == ON_SHAPE));
    // On the circle's wall (radius 508, thickness 3), each particle once though it stays there.
    for m in &st.marks {
        let r = m.place.length();
        assert!((r - 508.0).abs() < 6.0, "{r}");
    }
    let mut seeds: Vec<u32> = st.marks.iter().map(|m| m.seed).collect();
    seeds.sort_unstable();
    seeds.dedup();
    assert_eq!(seeds.len(), st.marks.len(), "one mark a particle");
}

#[test]
fn marks_where_they_die_and_the_cap_drops_the_oldest() {
    let (mut scene, id) = falling(0);
    let source = ParticlesSettings {
        from: BornFrom::APoint,
        rate: 600.0,
        life: 0.1,
        life_spread: 0.0,
        speed: 0.0,
        ..ParticlesSettings::default()
    };
    set_source(&mut scene, id, &source);
    let marks = MarksLayer { on_shapes: false, on_death: true, most: 30, ..MarksLayer::default() };
    only_layer(&mut scene, id, MARKS, marks.to_params());
    let (world, _, _) = run(&scene, 1.0);
    let st = particles(&world, id);
    assert_eq!(st.marks.len(), 30, "kept to Most marks");
    assert!(st.marks.iter().all(|m| m.why == ON_DEATH));
    assert!(st.marks.windows(2).all(|w| w[0].at <= w[1].at), "oldest first");
    assert!(st.marks[0].at > 60, "the oldest went first");
}

#[test]
fn faded_marks_are_dropped() {
    let (mut scene, id) = falling(0);
    let source = ParticlesSettings {
        from: BornFrom::APoint,
        rate: 120.0,
        life: 0.05,
        life_spread: 0.0,
        speed: 0.0,
        ..ParticlesSettings::default()
    };
    set_source(&mut scene, id, &source);
    let marks =
        MarksLayer { on_shapes: false, on_death: true, fade_after: 0.5, fade_over: 0.25, ..MarksLayer::default() };
    only_layer(&mut scene, id, MARKS, marks.to_params());
    let (world, _, _) = run(&scene, 3.0);
    let st = particles(&world, id);
    assert!(!st.marks.is_empty());
    assert!(st.marks.iter().all(|m| m.age(st.step) <= 0.75 + 1e-3));
    assert!(st.marks.len() < 100, "{}", st.marks.len());
}

#[test]
fn marks_draw_with_their_splats_and_drips_and_stay() {
    let (mut scene, id) = raining_in_arena(60.0);
    let marks = MarksLayer { splats: 3, drip: 40.0, drip_time: 1.0, ..MarksLayer::default() };
    only_layer(&mut scene, id, MARKS, marks.to_params());
    set_meets(&mut scene, id, &MeetsSettings { at_shapes: AtShapes::Vanish, ..MeetsSettings::default() });
    let kit = kit();
    let resolved = resolve(&scene, &kit.registry);
    let mut world = World::build(&scene, &resolved, &kit.sims);
    let mut log = Log::default();
    world.advance_to(3.0, &kit.sims, &mut log);
    let n = particles(&world, id).marks.len();
    assert!(n > 10);
    let mut frame = Frame::default();
    build_frame(&scene, &resolved, &world, &log, 3.0, &kit.draws, &mut frame);
    // The mark, 3 splats, a drip and its drop for the old ones; the newest may not drip yet.
    assert!(frame.particles.len() >= n * 4 && frame.particles.len() <= n * 6, "{} for {n}", frame.particles.len());
}

// ---------- liquid ----------

#[test]
fn a_liquid_layer_makes_one_blob_a_particle() {
    let (mut scene, id) = falling(40);
    let l = LiquidLayer { blob: 3.0, size: 2.0, ..LiquidLayer::default() };
    only_layer(&mut scene, id, LIQUID, l.to_params());
    let frame = frame_of(&scene, 0.5);
    assert_eq!(frame.liquids.len(), 1);
    let liquid = &frame.liquids[0];
    assert_eq!(liquid.blobs.len(), 40);
    assert!(!liquid.front);
    // Size 3 × Size × 2 × Blob size 3.
    assert!((liquid.blobs[0].place[2] - 18.0).abs() < 0.5 * 18.0, "{}", liquid.blobs[0].place[2]);
    assert!(frame.particles.is_empty() && frame.trails.is_empty());
    // A Multiply liquid goes into a layer of its own.
    let card = &scene.things[&id].slots[&SlotId::Look];
    let layer = card.layers[0].id;
    put(&mut scene, id, layer, LiquidLayer { blend: bpe_core::Blend::Multiply, ..l }.to_params());
    let frame = frame_of(&scene, 0.5);
    assert!(frame.liquids.is_empty());
    assert_eq!(frame.layers.iter().map(|b| b.frame.liquids.len()).sum::<usize>(), 1);
}

#[test]
fn a_liquid_takes_no_sub_layers_and_no_colour_split() {
    let reg = registry();
    let kind = reg.layer(LIQUID).unwrap();
    assert!(!kind.takes_subs());
    let get = |_: &str| None;
    let keys: Vec<&str> = kind.dynamic_defs(&get).iter().map(|d| d.key).collect();
    assert!(keys.contains(&"mirror") && keys.contains(&"glitch"));
    assert!(!keys.contains(&"split"));
    let trails = reg.layer(TRAILS).unwrap();
    assert!(trails.dynamic_defs(&get).iter().any(|d| d.key == "split"));
}

// ---------- modifiers ----------

fn sprite(at: Vec2, colour: [f32; 4]) -> Particle {
    Particle::new(6, at, 0.3, 4.0, colour)
}

#[test]
fn mirror_four_ways_makes_four_copies_a_quarter_turn_apart() {
    let mut pieces = Pieces::default();
    pieces.particles[OWN].push(sprite(Vec2::new(10.0, 0.0), [1.0, 1.0, 1.0, 1.0]));
    pieces.strips[OWN].push(TrailVertex { position: [10.0, 0.0], ..TrailVertex::default() });
    let m = Modifiers { mirror: Mirror::FourWays, ..Modifiers::default() };
    mirror(&mut pieces, &m, Vec2::ZERO);
    let at: Vec<Vec2> = pieces.particles[OWN].iter().map(|p| p.at()).collect();
    let want = [Vec2::new(10.0, 0.0), Vec2::new(0.0, 10.0), Vec2::new(-10.0, 0.0), Vec2::new(0.0, -10.0)];
    for (a, w) in at.iter().zip(want) {
        assert!(a.distance(w) < 1e-3, "{a} vs {w}");
    }
    assert!((pieces.particles[OWN][1].place[2] - (0.3 + std::f32::consts::FRAC_PI_2)).abs() < 1e-4);
    assert_eq!(pieces.strips[OWN].len(), 4);
    // Mirrored copies flip every second one.
    let mut pieces = Pieces::default();
    pieces.particles[OWN].push(sprite(Vec2::new(10.0, 2.0), [1.0; 4]));
    mirror(&mut pieces, &Modifiers { flipped: true, ..m }, Vec2::ZERO);
    let flipped = pieces.particles[OWN][1].at();
    // (10, 2) flipped to (10, -2), then a quarter turn: (2, 10).
    assert!(flipped.distance(Vec2::new(2.0, 10.0)) < 1e-3, "{flipped}");
    assert!((pieces.particles[OWN][1].place[2] - (std::f32::consts::FRAC_PI_2 - 0.3)).abs() < 1e-4);
}

#[test]
fn colour_split_makes_three_added_copies_that_sum_to_the_colour() {
    let mut scratch = Pieces::default();
    scratch.particles[OWN].push(sprite(Vec2::ZERO, [0.8, 0.5, 0.2, 0.6]));
    let mut out = Pieces::default();
    split_into(&mut scratch, &mut out, Vec2::new(5.0, 0.0));
    assert!(scratch.particles[OWN].is_empty());
    let copies = &out.particles[OWN];
    assert_eq!(copies.len(), 3);
    assert_eq!(copies[0].at(), Vec2::new(5.0, 0.0), "red ahead");
    assert_eq!(copies[1].at(), Vec2::ZERO);
    assert_eq!(copies[2].at(), Vec2::new(-5.0, 0.0), "blue behind");
    let mut sum = [0.0; 3];
    for c in copies {
        assert!(c.color[3] < 0.0, "added light");
        for (k, total) in sum.iter_mut().enumerate() {
            *total += c.color[k];
        }
    }
    assert_eq!(sum, [0.8, 0.5, 0.2]);
    let m = Modifiers { split: 0.1, ..Modifiers::default() };
    assert_eq!(m.split_of(Vec2::new(0.0, 100.0)), Vec2::new(0.0, 10.0));
}

#[test]
fn glitch_tears_whole_bands_and_changes_over_time() {
    let camera = bpe_runtime::CameraView::default();
    let row: Vec<Vec2> = (0..20).map(|k| Vec2::new(k as f32 * 7.0, 3.0)).collect();
    let pieces_of = || {
        let mut p = Pieces::default();
        p.particles[OWN] = row.iter().map(|at| sprite(*at, [1.0; 4])).collect();
        p
    };
    let off = Modifiers::default();
    let mut p = pieces_of();
    glitch(&mut p, &Tears::new(&off, &camera, 1.0, 1));
    assert!(p.particles[OWN].iter().zip(&row).all(|(s, at)| s.at() == *at), "Glitch 0 tears nothing");
    let on = Modifiers { glitch: 1.0, tear: 40.0, tear_split: 0.0, ..Modifiers::default() };
    let moved = |time: f64| {
        let mut p = pieces_of();
        glitch(&mut p, &Tears::new(&on, &camera, time, 1));
        p.particles[OWN][0].at().x - row[0].x
    };
    let mut p = pieces_of();
    glitch(&mut p, &Tears::new(&on, &camera, 1.0, 1));
    let shift = p.particles[OWN][0].at() - row[0];
    assert!(shift.y == 0.0 && shift.x.abs() <= 40.0);
    assert!(
        p.particles[OWN].iter().zip(&row).all(|(s, at)| (s.at() - *at).distance(shift) < 1e-3),
        "one band, one tear"
    );
    assert_eq!(moved(1.0), moved(1.0), "the same at the same moment");
    let later: Vec<f32> = (0..20).map(|k| moved(1.0 + k as f64 * 0.2)).collect();
    assert!(later.windows(2).any(|w| w[0] != w[1]), "{later:?}");
}

#[test]
fn colour_split_and_mirror_reach_the_frame() {
    let (mut scene, id) = falling(10);
    let mut params = super::look::SpritesLayer::default().to_params();
    params.0.extend(Modifiers { mirror: Mirror::ThreeWays, split: 0.05, ..Modifiers::default() }.to_params().0);
    only_layer(&mut scene, id, super::look::SPRITES, params);
    let frame = frame_of(&scene, 0.5);
    assert_eq!(frame.particles.len(), 10 * 3 * 3);
    assert!(frame.particles.iter().all(|p| p.color[3] < 0.0));
}

#[test]
fn every_part_6_look_shows_what_it_is_for() {
    let reg = registry();
    let mut scene = crate::new_scene(&reg);
    let ball = scene.order.iter().copied().find(|t| scene.things[t].kind == "balls").unwrap();
    let drips = add_particles(&mut scene, &reg, Look::PaintDrips, ball).unwrap();
    let metal = add_particles(&mut scene, &reg, Look::LiquidMetal, ThingId::ROOT).unwrap();
    let splashes = add_particles(&mut scene, &reg, Look::LiquidSplashes, ThingId::ROOT).unwrap();
    let flow = add_particles(&mut scene, &reg, Look::LightStreams, ThingId::ROOT).unwrap();
    let (world, log, resolved) = run(&scene, 4.0);
    assert!(!particles(&world, drips).marks.is_empty(), "paint drips leave marks");
    assert!(particles(&world, flow).trail.points > 0, "flow lines keep trails");
    assert!(!particles(&world, splashes).specks.is_empty(), "the ball's hits splash");
    let kit = kit();
    let mut frame = Frame::default();
    build_frame(&scene, &resolved, &world, &log, 4.0, &kit.draws, &mut frame);
    assert!(!particles(&world, metal).specks.is_empty());
    assert!(frame.liquids.len() >= 2, "liquid metal and the splashes: {}", frame.liquids.len());
}

/// The frame of `scene` at `time`.
fn frame_of(scene: &Scene, time: f64) -> Frame {
    let kit = kit();
    let (world, log, resolved) = run(scene, time);
    let mut frame = Frame::default();
    build_frame(scene, &resolved, &world, &log, time, &kit.draws, &mut frame);
    frame
}

#[test]
fn a_frame_built_again_keeps_only_this_moments_liquid() {
    let (mut scene, id) = falling(20);
    only_layer(&mut scene, id, LIQUID, LiquidLayer::default().to_params());
    let kit = kit();
    let (world, log, resolved) = run(&scene, 0.5);
    let mut frame = Frame::default();
    for _ in 0..3 {
        frame.clear();
        build_frame(&scene, &resolved, &world, &log, 0.5, &kit.draws, &mut frame);
    }
    assert_eq!(frame.liquids.len(), 1, "the editor's reused frame doesn't pile liquids up");
}

#[test]
fn a_big_crowd_keeps_shorter_trails_within_the_ring_budget() {
    let (mut scene, id) = falling(10);
    let source = ParticlesSettings::from_params(&scene.things[&id].params);
    scene.things.get_mut(&id).unwrap().params = ParticlesSettings { most: 1_000_000, ..source }.to_params();
    only_layer(&mut scene, id, TRAILS, TrailsLayer { length: 64, every: 4, ..TrailsLayer::default() }.to_params());
    let (world, _, _) = run(&scene, 0.1);
    let st = particles(&world, id);
    assert_eq!(st.trail.points, super::trail::MOST_RING_PLACES / 1_000_000);
    assert!(st.trail.points * 1_000_000 <= super::trail::MOST_RING_PLACES);
}

#[test]
fn mirror_repeats_a_soft_smoke_field_and_fine_detail_copies_a_liquid() {
    let (mut scene, id) = falling(10);
    let mut params = super::look::SmokeLayer::default().to_params();
    params.0.extend(Modifiers { mirror: Mirror::FourWays, ..Modifiers::default() }.to_params().0);
    only_layer(&mut scene, id, super::look::SMOKE, params);
    let frame = frame_of(&scene, 0.5);
    assert_eq!(frame.fields.iter().map(|f| f.splats.len()).sum::<usize>(), 10 * 4, "every splat four ways");
    let (mut scene, id) = falling(10);
    let l = LiquidLayer { copies: 2, ..LiquidLayer::default() };
    only_layer(&mut scene, id, LIQUID, l.to_params());
    let frame = frame_of(&scene, 0.5);
    assert_eq!(frame.liquids[0].blobs.len(), 10 * 3, "a blob and two copies a particle");
}

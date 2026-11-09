//! Tests of part 3's motion: word, picture and grid places, On the beat, the new forces and the
//! looks they make.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bpe_core::{Scene, Settings, SlotId, ThingId, Value, resolve};
use bpe_runtime::{Log, World};
use glam::Vec2;

use super::beats::Moments;
use super::births::unit;
use super::draw::draws;
use super::forces::{Force, ForceCx, Forces, OnlyFor};
use super::look::{Common, Only};
use super::looks::{Look, add_particles};
use super::meets::{BallAt, BallGrid};
use super::more_forces::*;
use super::ready::{Flockmates, ForceReady, Ready, Ring};
use super::sim::state_of;
use super::spots::{grid_spots, picture_spots, word_spots};
use super::state::{DEAD, HomeFrame, ParticlesState, Speck};
use super::step::{self, DT, hash_of};
use super::{AreaShape, BeatEvery, BornFrom, Emit, GridBy, GridCovers, Heading, KIND, ParticlesSettings, Uses, Weight};
use crate::{kit, registry};

fn speck(place: Vec2, velocity: Vec2) -> Speck {
    Speck { velocity, life: 1000.0, size: 2.0, ..Speck::at(place, 0, 12345) }
}

const CX: ForceCx<'static> = ForceCx { time: 0.0, dt: DT, gravity: Vec2::ZERO, air: None };

/// `force` applied to `s` once with `ready`, then `s` moved by its velocity.
fn once(force: Force, s: &mut Speck, ready: &Ready) {
    let forces = Forces { list: vec![force] };
    forces.apply(s, 0, 1.0, &CX, ready);
    s.place += s.velocity * DT;
}

fn centred(c: Vec2) -> Ready {
    Ready { per_force: vec![ForceReady::Centre(Some((c, Vec2::ZERO)))], ..Ready::default() }
}

/// A blank project with one Particles thing whose Source is `s`, and its forces `forces`.
fn scene_of(s: &ParticlesSettings, forces: &[(&'static str, bpe_core::Params)]) -> (Scene, ThingId) {
    let reg = registry();
    let mut scene = crate::new_studio_scene(&reg);
    let id = add_particles(&mut scene, &reg, Look::Empty, ThingId::ROOT).unwrap();
    scene.things.get_mut(&id).unwrap().params = s.to_params();
    for (kind, params) in forces {
        let layer = scene.add_layer(&reg, id, SlotId::Motion, kind).unwrap();
        let card = scene.card_mut(id, SlotId::Motion).unwrap();
        for (k, v) in params.0.iter() {
            card.params.set(&bpe_core::layered_key(layer, k), v.clone());
        }
    }
    (scene, id)
}

fn run(scene: &Scene, time: f64) -> (World, Log) {
    let kit = kit();
    let resolved = resolve(scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    let mut world = World::build(scene, &resolved, &kit.sims);
    let mut log = Log::default();
    world.advance_to(time, &kit.sims, &mut log);
    (world, log)
}

fn particles(world: &World, id: ThingId) -> &ParticlesState {
    state_of(world, id).expect("the thing has particles")
}

// ---- The tempo ----

#[test]
fn the_tempo_reaches_the_world_and_restarts_only_the_particles() {
    let kit = kit();
    let (mut scene, _) = scene_of(&ParticlesSettings::default(), &[]);
    let resolved = resolve(&scene, &kit.registry);
    let world = World::build(&scene, &resolved, &kit.sims);
    let beat = crate::things::scene::sim::beat_of(&world);
    assert_eq!((beat.beat, beat.per_bar), (0.5, 4), "120 beats a minute, four in a bar");
    let before = (
        bpe_runtime::fingerprint(&scene, &resolved, &kit.sims),
        bpe_runtime::beside_fingerprint(&scene, &resolved, &kit.sims),
    );
    scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("tempo", Value::Number(90.0));
    let resolved = resolve(&scene, &kit.registry);
    let world = World::build(&scene, &resolved, &kit.sims);
    assert!((crate::things::scene::sim::beat_of(&world).beat - 60.0 / 90.0).abs() < 1e-9);
    let after = (
        bpe_runtime::fingerprint(&scene, &resolved, &kit.sims),
        bpe_runtime::beside_fingerprint(&scene, &resolved, &kit.sims),
    );
    assert_eq!(before.0, after.0, "the tempo isn't physics");
    assert_ne!(before.1, after.1, "the particle playback starts over");
}

#[test]
fn beat_moments_count_once_each() {
    let m = Moments { first: 0.25, every: 0.5 };
    assert_eq!(m.count_in(f64::NEG_INFINITY, 0.2), 0);
    assert_eq!(m.count_in(0.2, 0.25), 1, "a moment on a step's end counts in that step");
    assert_eq!(m.count_in(0.25, 0.3), 0, "and not again");
    assert_eq!(m.count_in(0.0, 2.0), 4);
    let b = m.beats_at(1.0).unwrap();
    assert!((b.since - 0.25).abs() < 1e-9 && b.count == 2 && b.every == 0.5);
    assert_eq!(m.beats_at(0.1), None);
}

// ---- Places ----

#[test]
fn a_grid_by_columns_and_rows_has_exactly_that_many_points_inside() {
    let size = Vec2::new(400.0, 300.0);
    let points = grid_spots(size, GridBy::ColumnsAndRows, (8, 6), 0.0, 0.0, false);
    assert_eq!(points.len(), 48);
    assert!(points.iter().all(|p| p.x.abs() < size.x / 2.0 && p.y.abs() < size.y / 2.0));
    let middle = points.iter().copied().sum::<Vec2>() / points.len() as f32;
    assert!(middle.length() < 1e-3, "centred: {middle}");
}

#[test]
fn a_grid_by_spacing_is_spacing_apart_and_staggers_and_jitters() {
    let points = grid_spots(Vec2::new(100.0, 50.0), GridBy::Spacing, (1, 1), 10.0, 0.0, false);
    // 10 × 5 cells, a point in the middle of each.
    assert_eq!(points.len(), 10 * 5);
    assert!((points[1].x - points[0].x - 10.0).abs() < 1e-4);
    let staggered = grid_spots(Vec2::new(100.0, 50.0), GridBy::Spacing, (1, 1), 10.0, 0.0, true);
    assert!((staggered[10].x - points[10].x - 5.0).abs() < 1e-4, "odd rows shift half a cell");
    assert_eq!(staggered[0], points[0]);
    let jittered = grid_spots(Vec2::new(100.0, 50.0), GridBy::Spacing, (1, 1), 10.0, 0.6, false);
    assert_eq!(jittered, grid_spots(Vec2::new(100.0, 50.0), GridBy::Spacing, (1, 1), 10.0, 0.6, false));
    for (j, p) in jittered.iter().zip(&points) {
        assert!((*j - *p).abs().max_element() <= 3.0 + 1e-4, "within its cell");
    }
    assert!(jittered.iter().zip(&points).any(|(j, p)| j != p));
}

#[test]
fn a_word_fills_its_letters_exactly() {
    let spots = word_spots("OK", 120.0, Weight::SemiBold, 4.0);
    assert!(spots.len() > 200, "{}", spots.len());
    // The scanline gives the same points as testing each against the outlines.
    let shapes = crate::things::balls::shapes::letter_outlines_in(crate::things::balls::shapes::FONT, "OK", 120.0);
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for p in shapes.iter().flatten() {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    let middle = (lo + hi) * 0.5;
    let inside = spots.iter().filter(|p| crate::things::balls::shapes::winding(&shapes, **p + middle) != 0).count();
    assert!(inside as f32 >= spots.len() as f32 * 0.99, "{inside} of {}", spots.len());
    // The O has a hole: its middle has no place.
    assert!(word_spots("", 120.0, Weight::SemiBold, 4.0).is_empty());
    assert!(word_spots("   ", 120.0, Weight::Regular, 4.0).is_empty());
    assert_ne!(word_spots("OK", 120.0, Weight::Regular, 4.0).len(), spots.len(), "a lighter weight fills less");
}

#[test]
fn a_typed_word_can_take_a_text_font() {
    use super::spots::{SpotsKey, key_of, spots_for};
    let scene = crate::new_scene(&crate::registry());
    let mut s = ParticlesSettings {
        from: super::BornFrom::AWordOrPicture,
        word_from: super::WordFrom::Text,
        text: "OK".into(),
        letter_height: 120.0,
        spacing: 4.0,
        ..ParticlesSettings::default()
    };
    let video = Vec2::new(1080.0, 1920.0);
    let plex = key_of(&s, video, Some(&scene), ThingId(9), &SpotsKey::None);
    assert!(matches!(&plex, SpotsKey::Word { font: None, .. }), "the built-in font as before");
    assert_eq!(*spots_for(&plex), word_spots("OK", 120.0, Weight::SemiBold, 4.0));
    s.font = "Anton".into();
    let anton = key_of(&s, video, Some(&scene), ThingId(9), &plex);
    assert!(matches!(&anton, SpotsKey::Word { font: Some(f), .. } if f.data.name == "Anton"), "{anton:?}");
    assert_eq!(key_of(&s, video, Some(&scene), ThingId(9), &anton), anton, "kept while it names the same font");
    let spelt = spots_for(&anton);
    assert!(spelt.len() > 200 && *spelt != *spots_for(&plex), "Anton's letters are another shape");
}

#[test]
fn a_picture_fills_its_solid_bright_or_dark_parts() {
    let dir = std::env::temp_dir().join(format!("bpe-particles-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("two.png");
    // 2 × 1: a white opaque pixel and a black opaque one, then a see-through one below each.
    let pixels: [u8; 16] = [255, 255, 255, 255, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0, 0];
    {
        let file = std::fs::File::create(&path).unwrap();
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), 2, 2);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&pixels).unwrap();
    }
    let path = path.to_string_lossy().into_owned();
    // 200 px wide, one place every 10 px: 20 × 20 places, a quarter in each pixel.
    let solid = picture_spots(&path, 200.0, Uses::SolidParts, 10.0);
    let bright = picture_spots(&path, 200.0, Uses::BrightParts, 10.0);
    let dark = picture_spots(&path, 200.0, Uses::DarkParts, 10.0);
    assert!(solid.iter().all(|p| p.y < 0.0), "only the top row is solid");
    assert!(bright.iter().all(|p| p.x < 0.0 && p.y < 0.0), "the white pixel is top left");
    assert!(dark.iter().all(|p| p.x >= 0.0 && p.y < 0.0), "the black pixel is top right");
    assert_eq!(bright.len() + dark.len(), solid.len());
    assert!(!bright.is_empty() && !dark.is_empty());
    assert!(picture_spots(&format!("{path}.missing"), 200.0, Uses::SolidParts, 10.0).is_empty());
    // Kept in the scene (W5), the same picture fills the same places, and the file isn't needed.
    let bytes = std::fs::read(&path).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let reg = crate::kit().registry;
    let mut scene = bpe_core::Scene::empty(&reg);
    let name = scene.add_picture("two", bytes.into());
    let s = ParticlesSettings {
        from: BornFrom::AWordOrPicture,
        word_from: super::WordFrom::Picture,
        picture: name,
        picture_width: 200.0,
        uses: Uses::SolidParts,
        spacing: 10.0,
        ..ParticlesSettings::default()
    };
    let key = super::spots::key_of(
        &s,
        Vec2::new(1080.0, 1920.0),
        Some(&scene),
        bpe_core::ThingId(9),
        &super::spots::SpotsKey::None,
    );
    assert!(matches!(&key, super::spots::SpotsKey::Picture { kept: Some(_), .. }), "{key:?}");
    assert_eq!(*super::spots::spots_for(&key), solid);
    let missing = super::spots::key_of(&s, Vec2::ZERO, None, bpe_core::ThingId(9), &super::spots::SpotsKey::None);
    assert!(super::spots::spots_for(&missing).is_empty(), "not in the scene and no such file");
}

// ---- Births ----

#[test]
fn on_the_beat_births_count_on_every_beat_or_bar() {
    let s = ParticlesSettings {
        from: BornFrom::APoint,
        emit: Emit::OnTheBeat,
        count: 7,
        life: 100.0,
        speed: 0.0,
        ..ParticlesSettings::default()
    };
    let (scene, id) = scene_of(&s, &[]);
    // 120 beats a minute: at 0, 0.5, 1.0, 1.5 and 2.0 s.
    assert_eq!(particles(&run(&scene, 2.0).0, id).specks.len(), 35);
    assert_eq!(particles(&run(&scene, 1.99).0, id).specks.len(), 28);
    let (scene, id) = scene_of(&ParticlesSettings { beat_every: BeatEvery::Bar, ..s.clone() }, &[]);
    assert_eq!(particles(&run(&scene, 4.5).0, id).specks.len(), 21, "bars at 0, 2 and 4 s");
    let (scene, id) = scene_of(&ParticlesSettings { beat_offset: 1.0, ..s }, &[]);
    assert_eq!(particles(&run(&scene, 1.0).0, id).specks.len(), 14, "from half a second in");
}

fn grid_source(columns: u32, rows: u32) -> ParticlesSettings {
    ParticlesSettings {
        from: BornFrom::AGrid,
        grid_covers: GridCovers::ARectangle,
        grid_by: GridBy::ColumnsAndRows,
        width: 300.0,
        height: 200.0,
        columns,
        rows,
        emit: Emit::KeepACrowd,
        life: 1000.0,
        speed: 0.0,
        ..ParticlesSettings::default()
    }
}

#[test]
fn a_crowd_on_a_grid_fills_every_place_and_refills_a_lost_one_there() {
    let (scene, id) = scene_of(&grid_source(6, 4), &[]);
    let (world, log) = run(&scene, 0.5);
    let st = particles(&world, id);
    assert_eq!(st.specks.len(), 24);
    let mut spots: Vec<u32> = st.specks.iter().map(|s| s.spot).collect();
    spots.sort();
    assert_eq!(spots, (1..=24).collect::<Vec<_>>());
    for s in &st.specks {
        assert!(s.place.distance(st.home_frame(Vec2::ZERO, 0.0, Vec2::ZERO).home_of(s)) < 1e-3);
    }
    // One dies: it comes back at its own place.
    let mut st = st.clone();
    let lost = st.specks.remove(5);
    step::step(&mut st, &world, &log, 1, None);
    assert_eq!(st.specks.len(), 24);
    let back = st.specks.last().unwrap();
    assert_eq!(back.spot, lost.spot);
    assert!(back.place.distance(lost.place) < 1e-3);
    // Once at the start fills every place once.
    let (scene, id) = scene_of(&ParticlesSettings { emit: Emit::OnceAtTheStart, ..grid_source(5, 5) }, &[]);
    assert_eq!(particles(&run(&scene, 0.2).0, id).specks.len(), 25);
    // A small cap stops the crowd.
    let (scene, id) = scene_of(&ParticlesSettings { most: 10, ..grid_source(5, 5) }, &[]);
    assert_eq!(particles(&run(&scene, 0.2).0, id).specks.len(), 10);
}

#[test]
fn a_picture_kept_in_the_scene_forms_particles_in_the_world() {
    // 2 × 2, all white and opaque.
    let mut bytes = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut bytes, 2, 2);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&[255; 16]).unwrap();
    }
    let s = ParticlesSettings {
        from: BornFrom::AWordOrPicture,
        word_from: super::WordFrom::Picture,
        picture: "Logo".into(),
        picture_width: 100.0,
        spacing: 10.0,
        emit: Emit::OnceAtTheStart,
        life: 100.0,
        speed: 0.0,
        ..ParticlesSettings::default()
    };
    let (mut scene, id) = scene_of(&s, &[]);
    assert!(particles(&run(&scene, 0.1).0, id).specks.is_empty(), "no picture of that name yet");
    assert_eq!(scene.add_picture("Logo", bytes.into()), "Logo");
    assert!(!particles(&run(&scene, 0.1).0, id).specks.is_empty(), "the kept picture forms them");
}

#[test]
fn a_word_crowd_has_a_particle_at_every_letter_place() {
    let s = ParticlesSettings {
        from: BornFrom::AWordOrPicture,
        text: "HI".into(),
        letter_height: 100.0,
        spacing: 5.0,
        emit: Emit::OnceAtTheStart,
        life: 100.0,
        speed: 0.0,
        ..ParticlesSettings::default()
    };
    let (scene, id) = scene_of(&s, &[]);
    let n = word_spots("HI", 100.0, Weight::SemiBold, 5.0).len();
    assert!(n > 50);
    assert_eq!(particles(&run(&scene, 0.1).0, id).specks.len(), n);
}

#[test]
fn homes_turn_with_homes_turn_and_springs_follow_them() {
    let s = ParticlesSettings { homes_turn: 90.0, ..grid_source(2, 1) };
    let spring = SpringForce { stiffness: 400.0, damping: 30.0, ..Default::default() };
    let (scene, id) = scene_of(&s, &[(SPRING, spring.to_params())]);
    let (world, _) = run(&scene, 1.0);
    let st = particles(&world, id);
    assert!((st.home_turn - FRAC_PI_2).abs() < 1e-3, "{}", st.home_turn);
    // The two places (±75, 0) have turned a quarter: to (0, ±75); the springs have followed.
    for s in &st.specks {
        let home = st.home_frame(Vec2::ZERO, 0.0, Vec2::ZERO).home_of(s);
        assert!(home.x.abs() < 1e-2 && (home.y.abs() - 75.0).abs() < 1e-2, "{home}");
        // A spring trails a moving home by about speed × damping ÷ stiffness (118 × 30 ÷ 400 px).
        assert!(s.place.distance(home) < 12.0, "{} from {home}", s.place);
    }
}

#[test]
fn round_the_middle_flies_sideways() {
    let s = ParticlesSettings {
        from: BornFrom::AnArea,
        area: AreaShape::Ring,
        radius: 200.0,
        inner: 100.0,
        emit: Emit::OnceAtTheStart,
        count: 50,
        speed: 100.0,
        spread: 0.0,
        heading: Heading::RoundTheMiddle,
        ..ParticlesSettings::default()
    };
    let (scene, id) = scene_of(&s, &[]);
    let kit = kit();
    let world = World::build(&scene, &resolve(&scene, &kit.registry), &kit.sims);
    for p in &particles(&world, id).specks {
        assert!(p.place.normalize().dot(p.velocity.normalize()).abs() < 1e-3);
        assert!((p.velocity.length() - 100.0).abs() < 31.0);
    }
}

// ---- Forces, one particle at a time ----

#[test]
fn pull_to_pulls_in_weakens_further_off_and_swallows() {
    let pull = |falloff, swallow| Force::Pull(PullForce { falloff, swallow, at: 100.0, ..Default::default() });
    let ready = centred(Vec2::ZERO);
    let mut near = speck(Vec2::new(100.0, 0.0), Vec2::ZERO);
    once(pull(Falloff::Even, 0.0), &mut near, &ready);
    assert!(near.velocity.x < 0.0 && near.velocity.y.abs() < 1e-6, "towards the middle");
    let speed = |falloff, x: f32| {
        let mut s = speck(Vec2::new(x, 0.0), Vec2::ZERO);
        once(pull(falloff, 0.0), &mut s, &ready);
        s.velocity.length()
    };
    assert!((speed(Falloff::Even, 400.0) - speed(Falloff::Even, 100.0)).abs() < 1e-3);
    assert!((speed(Falloff::WeakerFurther, 400.0) * 4.0 - speed(Falloff::WeakerFurther, 100.0)).abs() < 1e-2);
    assert!((speed(Falloff::MuchWeakerFurther, 400.0) * 16.0 - speed(Falloff::MuchWeakerFurther, 100.0)).abs() < 1e-2);
    let mut close = speck(Vec2::new(5.0, 0.0), Vec2::ZERO);
    once(pull(Falloff::Even, 10.0), &mut close, &ready);
    assert_ne!(close.flags & DEAD, 0, "swallowed");
    // Reach.
    let mut far = speck(Vec2::new(500.0, 0.0), Vec2::ZERO);
    once(Force::Pull(PullForce { reach: 300.0, ..Default::default() }), &mut far, &ready);
    assert_eq!(far.velocity, Vec2::ZERO);
}

#[test]
fn orbit_pulls_sideways() {
    let mut s = speck(Vec2::new(100.0, 0.0), Vec2::ZERO);
    once(
        Force::Pull(PullForce { orbit: 1.0, falloff: Falloff::Even, ..Default::default() }),
        &mut s,
        &centred(Vec2::ZERO),
    );
    assert!(s.velocity.x < 0.0 && (s.velocity.y.abs() - s.velocity.x.abs()).abs() < 1e-3, "{}", s.velocity);
}

#[test]
fn its_ball_is_each_particles_own_and_push_goes_the_other_way() {
    let ball = BallAt { place: Vec2::new(500.0, 0.0), velocity: Vec2::ZERO, radius: 10.0, thing: ThingId(0), row: 0 };
    let ready =
        Ready { parent_balls: vec![None, Some(ball)], per_force: vec![ForceReady::Nothing], ..Ready::default() };
    let mut s = speck(Vec2::new(400.0, 0.0), Vec2::ZERO);
    s.ball = 2;
    once(Force::Pull(PullForce { toward: Toward::ItsBall, ..Default::default() }), &mut s, &ready);
    assert!(s.velocity.x > 0.0, "towards its ball, not the middle");
    let mut s = speck(Vec2::new(100.0, 0.0), Vec2::ZERO);
    once(Force::Push(PushForce { toward: Toward::ItsPlace, ..Default::default() }), &mut s, &centred(Vec2::ZERO));
    assert!(s.velocity.x > 0.0, "pushed out");
}

#[test]
fn balls_nearby_push_only_within_their_reach() {
    let ball = BallAt { place: Vec2::ZERO, velocity: Vec2::ZERO, radius: 10.0, thing: ThingId(0), row: 0 };
    let ready =
        Ready { balls: Some(BallGrid::new(vec![ball], 4.0)), per_force: vec![ForceReady::Nothing], ..Ready::default() };
    let push = || Force::Push(PushForce { ball_reach: 4.0, ..Default::default() });
    let mut near = speck(Vec2::new(30.0, 0.0), Vec2::ZERO);
    once(push(), &mut near, &ready);
    assert!(near.velocity.x > 0.0);
    let mut far = speck(Vec2::new(45.0, 0.0), Vec2::ZERO);
    once(push(), &mut far, &ready);
    assert_eq!(far.velocity, Vec2::ZERO);
}

#[test]
fn spring_home_returns_and_settles() {
    let mut s = speck(Vec2::new(100.0, 50.0), Vec2::new(300.0, 0.0));
    s.home = Vec2::ZERO;
    let spring = SpringForce { stiffness: 30.0, damping: 4.0, ..Default::default() };
    for _ in 0..1200 {
        once(Force::Spring(spring.clone()), &mut s, &Ready::default());
    }
    assert!(s.place.length() < 0.5 && s.velocity.length() < 0.5, "{} {}", s.place, s.velocity);
}

#[test]
fn a_flock_aligns_holds_its_speed_and_skips_itself() {
    let specks = vec![
        speck(Vec2::ZERO, Vec2::new(300.0, 0.0)),
        speck(Vec2::new(50.0, 0.0), Vec2::new(0.0, 300.0)),
        speck(Vec2::new(0.0, 50.0), Vec2::new(0.0, 300.0)),
        speck(Vec2::new(5000.0, 0.0), Vec2::new(-300.0, 0.0)),
    ];
    let mates = Flockmates::new(&specks, 120.0);
    let mut seen = Vec::new();
    mates.near(0, specks[0].place, 120.0, |p, _| seen.push(p));
    assert_eq!(seen.len(), 2, "its two neighbours, not itself nor the far one");
    let ready = Ready { mates: Some(mates), per_force: vec![ForceReady::Nothing], ..Ready::default() };
    let flock = FlockForce { separation: 0.0, cohesion: 0.0, alignment: 5.0, ..Default::default() };
    let mut s = specks[0];
    let forces = Forces { list: vec![Force::Flock(flock)] };
    forces.apply(&mut s, 0, 1.0, &CX, &ready);
    assert!(s.velocity.y > 0.0, "turned towards its neighbours' way");
    let speed = s.velocity.length();
    assert!((200.0 - 1e-3..=440.0 + 1e-3).contains(&speed), "{speed}");
    assert!((s.turn - s.velocity.y.atan2(s.velocity.x)).abs() < 1e-6, "faces the way it flies");
    // Too slow: sped up to Least speed.
    let mut slow = speck(Vec2::new(-9000.0, 0.0), Vec2::new(10.0, 0.0));
    forces.apply(&mut slow, 3, 1.0, &CX, &ready);
    assert!((slow.velocity.length() - 200.0).abs() < 1e-2);
}

#[test]
fn a_magnetic_field_lines_needles_up() {
    // A ball moving right: straight above it the field runs left-right.
    let ball = BallAt { place: Vec2::ZERO, velocity: Vec2::new(100.0, 0.0), radius: 10.0, thing: ThingId(0), row: 0 };
    let ready = Ready {
        balls: Some(BallGrid::new(vec![ball], 12.0)),
        per_force: vec![ForceReady::Nothing],
        ..Ready::default()
    };
    let mut s = speck(Vec2::new(0.0, -40.0), Vec2::ZERO);
    s.turn = 1.2;
    let force = MagneticForce { pull: 0.0, ..Default::default() };
    for _ in 0..240 {
        let forces = Forces { list: vec![Force::Magnetic(force.clone())] };
        forces.apply(&mut s, 0, 1.0, &CX, &ready);
    }
    let off = (s.turn.rem_euclid(PI) - 0.0).min(PI - s.turn.rem_euclid(PI));
    assert!(off < 0.02, "lined up with the field: {}", s.turn);
    // Pull draws it in.
    let mut s = speck(Vec2::new(0.0, -40.0), Vec2::ZERO);
    once(Force::Magnetic(MagneticForce::default()), &mut s, &ready);
    assert!(s.velocity.y > 0.0);
}

#[test]
fn a_flow_field_carries_particles_at_its_speed() {
    for pattern in [Pattern::Noise, Pattern::Waves, Pattern::Spiral, Pattern::Vortex] {
        let flow = FlowForce { pattern, speed: 150.0, grip: 5.0, size: 200.0, ..Default::default() };
        let mut s = speck(Vec2::new(200.0, 30.0), Vec2::ZERO);
        let ready = Ready::default();
        for _ in 0..240 {
            once(Force::Flow(flow.clone()), &mut s, &ready);
        }
        let speed = s.velocity.length();
        assert!(speed > 40.0 && speed < 151.0, "{pattern:?}: {speed}");
    }
}

#[test]
fn a_spirograph_puts_each_particle_on_its_curve() {
    let f = SpirographForce { differ: 0.3, ..Default::default() };
    let c = Vec2::new(10.0, 20.0);
    let mut s = speck(Vec2::ZERO, Vec2::ZERO);
    let ready = centred(c);
    let forces = Forces { list: vec![Force::Spirograph(f.clone())] };
    for k in 0..5 {
        let age = k as f32 * DT;
        forces.apply(&mut s, 0, age, &CX, &ready);
        s.place += s.velocity * DT;
        assert!(s.place.distance(c + pen_at(&s, &f, age + DT)) < 1e-2);
    }
}

#[test]
fn a_pulse_gives_its_strength_over_each_beat() {
    let pulse = PulseForce { strength: 300.0, decay: 0.05, way: PulseWay::Up, ..Default::default() };
    let moments = Moments { first: 0.0, every: 0.5 };
    let mut s = speck(Vec2::ZERO, Vec2::ZERO);
    // From just after one beat to just before the next.
    for k in 1..=59 {
        let time = k as f64 * DT as f64;
        let ready = Ready { time, per_force: vec![ForceReady::Beats(moments.beats_at(time))], ..Ready::default() };
        Forces { list: vec![Force::Pulse(pulse.clone())] }.apply(&mut s, 0, 1.0, &CX, &ready);
    }
    assert!((s.velocity.y + 300.0).abs() < 1.0, "{}", s.velocity);
    // All at once: on the beat's own step.
    let at_once = PulseForce { decay: 0.0, ..pulse };
    let mut s = speck(Vec2::ZERO, Vec2::ZERO);
    let ready = Ready { time: 0.5, per_force: vec![ForceReady::Beats(moments.beats_at(0.5))], ..Ready::default() };
    Forces { list: vec![Force::Pulse(at_once)] }.apply(&mut s, 0, 1.0, &CX, &ready);
    assert!((s.velocity.y + 300.0).abs() < 1e-3);
}

#[test]
fn a_long_pulse_still_gives_every_kick_its_whole_strength() {
    // Decay 1 s on beats every 0.5 s: by 10 s, each beat at t_m has given 1 − e^(−(10 − t_m)).
    let moments = Moments { first: 0.0, every: 0.5 };
    let mut given = 0.0;
    for k in 1..=1200 {
        let b = moments.beats_at(k as f64 * DT as f64).unwrap();
        given += pulse_share(&b, DT as f64, 1.0);
    }
    let end = 1200.0 * DT as f64;
    let owed: f64 = (0..=20).map(|m| 1.0 - (-(end - m as f64 * 0.5)).exp()).sum();
    assert!((given - owed).abs() < 1e-6, "{given} against {owed}");
    // Beats closer together than a step are all counted.
    let fast = Moments { first: 0.0, every: 0.002 };
    let b = fast.beats_at(DT as f64).unwrap();
    let all: f64 = (0..b.count).map(|m| 1.0 - (-(b.since + m as f64 * 0.002) / 0.01_f64).exp()).sum();
    // Everything before the first step's end came in that step.
    let first_step = pulse_share(&b, DT as f64, 0.01);
    assert!((first_step - all).abs() < 1e-9, "{first_step} against {all}");
}

#[test]
fn a_huge_picture_or_word_stays_within_the_lattice_cap() {
    let spots = word_spots("W", 1e8, Weight::SemiBold, 1.0);
    assert!(!spots.is_empty() && spots.len() <= super::spots::MOST_SPOTS);
}

#[test]
fn a_ripple_pushes_only_near_its_ring() {
    let rings = vec![Ring { centre: Vec2::ZERO, age: 0.5, impact: 1.0 }];
    let ready = Ready { per_force: vec![ForceReady::Rings(rings)], ..Ready::default() };
    let ripple = || Force::Ripple(RippleForce { speed: 100.0, width: 20.0, lasts: 1.0, ..Default::default() });
    let mut on = speck(Vec2::new(50.0, 0.0), Vec2::ZERO);
    once(ripple(), &mut on, &ready);
    assert!(on.velocity.x > 0.0, "outwards");
    let mut off = speck(Vec2::new(200.0, 0.0), Vec2::ZERO);
    once(ripple(), &mut off, &ready);
    assert_eq!(off.velocity, Vec2::ZERO);
}

#[test]
fn a_share_of_a_force_is_the_share_a_layer_draws() {
    let common = |only| Common { only, share: 0.3, ..super::look::SpritesLayer::default().common() };
    for seed in 0..500 {
        let s = Speck { seed, ..speck(Vec2::ZERO, Vec2::ZERO) };
        assert_eq!(acts_on(OnlyFor::AShare, 0.0, 0.3, &s, 0.0), draws(&common(Only::AShare), &s, 0));
        assert_eq!(acts_on(OnlyFor::TheRest, 0.0, 0.3, &s, 0.0), draws(&common(Only::TheRest), &s, 0));
    }
    let _ = unit(0, 0);
}

// ---- Whole things ----

#[test]
fn ripples_from_real_hits_and_flocks_are_the_same_on_any_number_of_threads() {
    let s = ParticlesSettings {
        from: BornFrom::AnArea,
        width: 900.0,
        height: 1400.0,
        emit: Emit::KeepACrowd,
        count: 9000,
        life: 100.0,
        speed: 50.0,
        heading: Heading::AllRound,
        ..ParticlesSettings::default()
    };
    let reg = registry();
    let mut scene = crate::new_scene(&reg);
    let id = add_particles(&mut scene, &reg, Look::Empty, ThingId::ROOT).unwrap();
    scene.things.get_mut(&id).unwrap().params = s.to_params();
    for (kind, params) in [
        (FLOCK, FlockForce::default().to_params()),
        (PULL, PullForce { toward: Toward::BallsNearby, ..Default::default() }.to_params()),
        (RIPPLE, RippleForce::default().to_params()),
        (MAGNETIC, MagneticForce::default().to_params()),
    ] {
        let layer = scene.add_layer(&reg, id, SlotId::Motion, kind).unwrap();
        let card = scene.card_mut(id, SlotId::Motion).unwrap();
        for (k, v) in params.0.iter() {
            card.params.set(&bpe_core::layered_key(layer, k), v.clone());
        }
    }
    let (world, log) = run(&scene, 4.0);
    assert!(log.events().iter().any(|e| e.kind == bpe_runtime::EventKind::Hit), "the ball hit something");
    let st = particles(&world, id).clone();
    assert!(st.specks.len() > 2 * step::CHUNK);
    let hashes: Vec<u64> = [1, 3, 8]
        .into_iter()
        .map(|threads| {
            let mut copy = st.clone();
            for _ in 0..6 {
                step::step(&mut copy, &world, &log, threads, None);
            }
            hash_of(&copy)
        })
        .collect();
    assert!(hashes.windows(2).all(|w| w[0] == w[1]), "{hashes:?}");
}

#[test]
fn moire_adds_two_grids_and_every_part_3_look_resolves() {
    let reg = registry();
    let mut scene = crate::new_studio_scene(&reg);
    let first = add_particles(&mut scene, &reg, Look::Moire, ThingId::ROOT).unwrap();
    let grids: Vec<&str> = scene.things.values().filter(|t| t.kind == KIND).map(|t| t.name.as_str()).collect();
    assert_eq!(grids.len(), 2);
    assert_eq!(scene.things[&first].name, "Moiré");
    assert!(grids.contains(&"Moiré 2"));
    let (world, _) = run(&scene, 0.5);
    for t in scene.things.values().filter(|t| t.kind == KIND) {
        assert!(particles(&world, t.id).specks.len() > 1000, "{}", t.name);
    }
    let homes = HomeFrame::default();
    assert_eq!(homes.at(0), None);
    let _ = TAU;
}

//! Moving parts (Water mode part 5, `docs/superpowers/plans/water-mode/part5-moving-parts.md`): the
//! card lays them over the still field where they are, and they push the water.

use std::sync::Arc;

use bpe_water::drops::LEAST_WALL;
use bpe_water::drops::capture::Capture;
use bpe_water::drops::numbers::FRAME;
use bpe_water::take::FrameCodec;
use bpe_water::{
    Drop, Feel, Layout, Mover, MoverShape, Movers, Numbers, Pose, STOPS, Solid, SolidField, WATER, Water, WaterGpu,
    WaterState,
};
use bpe_water::{fill_stacked, probe, scenes};
use glam::Vec2;

fn card() -> Option<Arc<WaterGpu>> {
    let (device, queue) = bpe_gfx::test_device().ok()?;
    Some(WaterGpu::new(&device, &queue))
}

const CELL: f32 = 8.0;

/// An open field 600 px across with nothing in it.
fn open() -> SolidField {
    let layout = Layout::cover(Vec2::splat(-300.0), Vec2::splat(300.0), CELL);
    SolidField::build_least(layout, &[], LEAST_WALL * CELL)
}

/// `solids` as one moving part laid on `still`, its grid aligned with the course's at `place`.
fn mover_of(still: &SolidField, solids: &[Solid], place: Vec2) -> Movers {
    let l = still.layout;
    let shape = MoverShape::of(solids, l.cell, LEAST_WALL * l.cell, l.origin - place).expect("a shape");
    Movers { still: Arc::new(still.clone()), movers: vec![Mover { shape: Arc::new(shape), wetting: 0.5, owner: 3 }] }
}

/// Checks the field on the card against the processor's twin at `pose`: the distances within 0.01
/// cells and the nodes marked as the part's well inside its grid, and the still field's own to the
/// bit far from it and from `was`.
fn check_field(water: &Water, movers: &Movers, pose: Pose, was: Vec2, inside: f32) {
    let card = water.solid_field_now();
    let l = movers.still.layout;
    let n = l.nodes();
    let want = movers.field_at(&[pose]);
    let want_card = want.for_card();
    let still = movers.still.for_card();
    let far = movers.movers[0].shape.radius * pose.scale + 2.0 * l.cell;
    let (mut near, mut marked) = (0, 0);
    for y in 0..l.rows {
        for x in 0..l.cols {
            let k = (y * l.cols + x) as usize;
            let p = l.to_world(Vec2::new(x as f32, y as f32));
            if p.distance(pose.at) < inside {
                near += 1;
                let (got, wanted) = (card[k], want_card[k]);
                assert!((got - wanted).abs() < 0.01, "node {x}, {y}: {got} cells is not {wanted}");
                assert_eq!(card[n + k], want_card[n + k], "node {x}, {y}: its Wetting");
                let mine = want.owner[k] == movers.movers[0].owner;
                assert_eq!(card[2 * n + k], if mine { 1.0 } else { 0.0 }, "node {x}, {y}: whose");
                marked += usize::from(mine);
            } else if p.distance(pose.at) > far && p.distance(was) > far {
                assert_eq!(card[k].to_bits(), still[k].to_bits(), "node {x}, {y}");
                assert_eq!(card[n + k].to_bits(), still[n + k].to_bits(), "node {x}, {y}: its Wetting");
                assert_eq!(card[2 * n + k], 0.0, "node {x}, {y}: whose");
            }
        }
    }
    assert!(near > 300 && marked > 300, "{near} nodes near, {marked} the part's");
}

/// A part set at a pose is there on the card, and one moved is at its new pose after a frame,
/// with drops (each step lays it) and without (the frame's end does).
#[test]
fn a_still_mover_is_where_it_is() {
    let Some(gpu) = card() else { return };
    let still = open();
    let block = [Solid::rect(Vec2::splat(-30.0), Vec2::splat(30.0), true, 0.0)];
    let place = Vec2::new(-40.0, -60.0);
    let movers = mover_of(&still, &block, place);
    let here = Pose::new(place, 0.3, 1.0);
    let there = Pose::new(place + Vec2::new(40.0, 0.0), 0.3, 1.0);
    for with_drops in [true, false] {
        let mut water = Water::new(gpu.clone(), &still, scenes::GRAVITY, Numbers::for_cell(CELL), &[Feel::default()]);
        water.set_movers(movers.clone(), &[here]);
        assert_eq!(water.poses(), &[here]);
        check_field(&water, &movers, here, here.at, 80.0);
        if with_drops {
            let l = water.layout();
            let pool: Vec<Drop> = (0..400)
                .map(|k| {
                    let at = Vec2::new(-200.0 + (k % 40) as f32 * 4.0, 200.0 + (k / 40) as f32 * 4.0);
                    Drop::new(l.to_grid(at), Vec2::ZERO, STOPS, k, WATER)
                })
                .collect();
            water.add(&pool);
        }
        water.move_to(&[there]);
        water.frame();
        assert_eq!(water.poses(), &[there]);
        check_field(&water, &movers, there, here.at, 80.0);
    }
}

/// A closed box of rock 600 x 400 (its wall 3 px, taken as a cell and a half), open inside.
fn course() -> SolidField {
    let half = Vec2::new(300.0, 200.0);
    let layout = Layout::cover(-half - Vec2::splat(40.0), half + Vec2::splat(40.0), CELL);
    SolidField::build_least(layout, &[Solid::rect(-half, half, false, 3.0)], LEAST_WALL * CELL)
}

/// The box's floor's and walls' faces (the rock is 12 px thick to the physics).
const FLOOR: f32 = 194.0;
const SIDE: f32 = 294.0;

/// A `w` x `h` plate centred on its place, as one moving part of `still`.
fn plate(still: &SolidField, w: f32, h: f32, place: Vec2) -> Movers {
    mover_of(still, &[Solid::rect(Vec2::new(-w, -h) * 0.5, Vec2::new(w, h) * 0.5, true, 0.0)], place)
}

/// A body on `movers`' still field with its part at `keys(0)`, and water over `lo` to `hi` (where
/// the part isn't).
fn body(gpu: &Arc<WaterGpu>, movers: &Movers, start: Pose, lo: Vec2, hi: Vec2) -> Water {
    let numbers = Numbers::for_cell(CELL);
    let feels = [Feel { surface_pull: 0.2, ..Feel::default() }];
    let mut water = Water::new(gpu.clone(), &movers.still, scenes::GRAVITY, numbers, &feels);
    water.set_movers(movers.clone(), &[start]);
    let field = movers.field_at(&[start]);
    water.add(&fill_stacked(&water.layout(), &field, lo, hi, STOPS, numbers.wall_off));
    water
}

/// Plays `frames` frames, the part keyed at `keys(seconds)` at each frame's end.
fn play(water: &mut Water, keys: &impl Fn(f64) -> Pose, frames: u32) {
    for _ in 0..frames {
        let t = f64::from(water.frames() + 1) / 60.0;
        water.move_to(&[keys(t)]);
        water.frame();
    }
}

/// How far the deepest drop is inside a solid (pixels; 0 when none is), with the parts at `poses`.
fn deepest_inside(water: &Water, movers: &Movers, poses: &[Pose]) -> f32 {
    let field = movers.field_at(poses);
    let l = water.layout();
    water.read().iter().map(|d| -field.at(l.to_world(Vec2::from(d.x)))).fold(0.0, f32::max)
}

/// The liquid drops in the course (trapped air left out: a part sweeping through water traps some).
fn liquid(water: &Water) -> usize {
    water.read().iter().filter(|d| !d.is_air()).count()
}

fn mean_x(water: &Water) -> f32 {
    let l = water.layout();
    let drops = water.read();
    drops.iter().map(|d| l.to_world(Vec2::from(d.x)).x).sum::<f32>() / drops.len().max(1) as f32
}

/// A smooth key from `a` at `from` to `b` at `to` (seconds).
fn ease(a: f32, b: f32, from: f64, to: f64, t: f64) -> f32 {
    let u = ((t - from) / (to - from)).clamp(0.0, 1.0) as f32;
    a + (b - a) * u * u * (3.0 - 2.0 * u)
}

/// A plate swept through a pool pushes the water along, keeps every drop and lets none into it.
#[test]
fn a_paddle_pushes_the_water() {
    let Some(gpu) = card() else { return };
    let still = course();
    let start = Pose::new(Vec2::new(-200.0, 110.0), 0.0, 1.0);
    let movers = plate(&still, 20.0, 140.0, start.at);
    let keys = |t: f64| Pose::new(Vec2::new(ease(-200.0, 150.0, 0.0, 1.5, t), 110.0), 0.0, 1.0);
    let mut water = body(&gpu, &movers, start, Vec2::new(-SIDE, FLOOR - 100.0), Vec2::new(SIDE, FLOOR));
    let count = liquid(&water);
    let before = mean_x(&water);
    // (Near the end of its sweep: once it stops, the water piled up before it runs back.)
    play(&mut water, &keys, 75);
    let moved = mean_x(&water) - before;
    assert!(moved > 60.0, "the water moved {moved} px");
    play(&mut water, &keys, 45);
    let inside = deepest_inside(&water, &movers, water.poses());
    assert!(inside < 0.5 * CELL, "a drop is {inside} px inside");
    assert_eq!(liquid(&water), count);
}

/// Water held behind a gate goes through once the gate is lifted.
#[test]
fn a_gate_lifted_lets_the_water_through() {
    let Some(gpu) = card() else { return };
    let still = course();
    let start = Pose::new(Vec2::ZERO, 0.0, 1.0);
    let movers = plate(&still, 20.0, 400.0, start.at);
    let keys = |t: f64| Pose::new(Vec2::new(0.0, ease(0.0, -200.0, 1.0, 1.5, t)), 0.0, 1.0);
    let mut water = body(&gpu, &movers, start, Vec2::new(-SIDE, FLOOR - 120.0), Vec2::new(-10.0, FLOOR));
    let right = |water: &Water| {
        let l = water.layout();
        let drops = water.read();
        drops.iter().filter(|d| l.to_world(Vec2::from(d.x)).x > 10.0).count() as f32 / drops.len().max(1) as f32
    };
    play(&mut water, &keys, 60);
    assert!(right(&water) < 0.01, "{} through the shut gate", right(&water));
    play(&mut water, &keys, 180);
    assert!(right(&water) > 0.2, "only {} through the open gate", right(&water));
}

/// A bar turning two turns a second in a pool keeps the water out, and the frames take steps
/// enough for its tips.
#[test]
fn a_fast_turning_bar_keeps_the_water_out() {
    let Some(gpu) = card() else { return };
    let still = course();
    let place = Vec2::new(0.0, 100.0);
    let movers = plate(&still, 160.0, 16.0, place);
    let spin = 4.0 * std::f64::consts::PI;
    let keys = |t: f64| Pose::new(place, (spin * t) as f32, 1.0);
    let mut water = body(&gpu, &movers, keys(0.0), Vec2::new(-SIDE, FLOOR - 100.0), Vec2::new(SIDE, FLOOR));
    let count = liquid(&water);
    play(&mut water, &keys, 120);
    let inside = deepest_inside(&water, &movers, water.poses());
    assert!(inside < 0.5 * CELL, "a drop is {inside} px inside");
    assert_eq!(liquid(&water), count);
    let tip = 80.0 * spin as f32;
    let want = Numbers::for_cell(CELL).steps_for(tip, water.feels(), CELL);
    assert!(water.steps_last_frame() >= want, "{} steps, the tips need {want}", water.steps_last_frame());
}

/// A plate pushed into the rock that holds the pool pushes no water through the rock.
#[test]
fn a_paddle_into_the_rock_lets_no_water_through() {
    let Some(gpu) = card() else { return };
    let still = course();
    let start = Pose::new(Vec2::new(150.0, 110.0), 0.0, 1.0);
    let movers = plate(&still, 20.0, 140.0, start.at);
    let keys = |t: f64| Pose::new(Vec2::new(ease(150.0, 305.0, 0.0, 1.0, t), 110.0), 0.0, 1.0);
    let mut water = body(&gpu, &movers, start, Vec2::new(-SIDE, FLOOR - 100.0), Vec2::new(SIDE, FLOOR));
    let count = liquid(&water);
    for _ in 0..12 {
        play(&mut water, &keys, 10);
        let l = water.layout();
        let drops = water.read();
        assert_eq!(drops.iter().filter(|d| !d.is_air()).count(), count);
        for d in &drops {
            let p = l.to_world(Vec2::from(d.x));
            assert!(p.x < SIDE + 2.0 && p.y < FLOOR + 2.0, "a drop at {p} is through the rock");
        }
    }
}

/// A lid closed down into a cup full of water squeezes it out, every drop whole.
#[test]
fn a_lid_closing_on_water_keeps_every_drop() {
    let Some(gpu) = card() else { return };
    let half = Vec2::new(300.0, 200.0);
    let layout = Layout::cover(-half - Vec2::splat(40.0), half + Vec2::splat(40.0), CELL);
    let cup = Solid::Polyline {
        points: vec![Vec2::new(-60.0, 60.0), Vec2::new(-60.0, 180.0), Vec2::new(60.0, 180.0), Vec2::new(60.0, 60.0)],
        thickness: 3.0,
        square: true,
    };
    let still = SolidField::build_least(layout, &[Solid::rect(-half, half, false, 3.0), cup], LEAST_WALL * CELL);
    let start = Pose::new(Vec2::new(0.0, 40.0), 0.0, 1.0);
    let movers = plate(&still, 100.0, 20.0, start.at);
    let keys = |t: f64| Pose::new(Vec2::new(0.0, ease(40.0, 80.0, 0.2, 0.7, t)), 0.0, 1.0);
    let mut water = body(&gpu, &movers, start, Vec2::new(-54.0, 60.0), Vec2::new(54.0, 174.0));
    let count = liquid(&water);
    assert!(count > 500, "{count}");
    let numbers = Numbers::for_cell(CELL);
    for _ in 0..6 {
        play(&mut water, &keys, 10);
        let drops = water.read();
        assert_eq!(drops.iter().filter(|d| !d.is_air()).count(), count, "every drop is kept, its place a number");
        let cap = numbers.most_move * water.steps_last_frame() as f32 * 60.0 * 1.01;
        for d in &drops {
            let v = Vec2::from(d.v);
            assert!(v.is_finite() && v.length() <= cap, "a drop moves at {v} cells a second (the cap is {cap})");
        }
    }
}

/// Water a part moves repeats: twice from the start, and from a copy made on the way.
#[test]
fn moving_water_repeats() {
    let Some(gpu) = card() else { return };
    let still = course();
    let start = Pose::new(Vec2::new(-200.0, 110.0), 0.0, 1.0);
    let movers = plate(&still, 20.0, 140.0, start.at);
    let keys = |t: f64| Pose::new(Vec2::new(ease(-200.0, 150.0, 0.0, 1.5, t), 110.0), (t * 0.5) as f32, 1.0);
    let run = |copy_at: Option<u32>| {
        let mut water = body(&gpu, &movers, start, Vec2::new(-SIDE, FLOOR - 100.0), Vec2::new(SIDE, FLOOR));
        if let Some(at) = copy_at {
            play(&mut water, &keys, at);
            water = water.copy();
        }
        let left = 100 - water.frames();
        play(&mut water, &keys, left);
        probe::print_of(&water.read())
    };
    let first = run(None);
    assert_eq!(first, run(None), "twice from the start");
    assert_eq!(first, run(Some(40)), "from a copy made at frame 40");
    // From a restart point: a body made from the state, its parts set where the state says.
    let mut water = body(&gpu, &movers, start, Vec2::new(-SIDE, FLOOR - 100.0), Vec2::new(SIDE, FLOOR));
    play(&mut water, &keys, 40);
    let state = WaterState::from_bytes(&water.state().to_bytes()).expect("the state reads back");
    assert_eq!(state.poses, water.poses(), "the state keeps where the parts are");
    let (numbers, feels) = (*water.numbers(), water.feels().to_vec());
    let mut again = Water::from_state(gpu.clone(), &still, scenes::GRAVITY, numbers, &feels, &state).unwrap();
    again.set_movers(movers.clone(), &state.poses);
    play(&mut again, &keys, 60);
    assert_eq!(first, probe::print_of(&again.read()), "from a restart point at frame 40");
}

/// The moments kept inside a frame know where the parts were then, and the take keeps it.
#[test]
fn a_moment_inside_a_frame_keeps_the_parts_between() {
    let Some(gpu) = card() else { return };
    let still = course();
    let a = Pose::new(Vec2::new(-200.0, 110.0), 0.0, 1.0);
    let movers = plate(&still, 20.0, 140.0, a.at);
    let mut water = body(&gpu, &movers, a, Vec2::new(-SIDE, FLOOR - 100.0), Vec2::new(SIDE, FLOOR));
    let b = Pose::new(a.at + Vec2::new(12.0, -3.0), 0.05, 1.1);
    water.move_to(&[b]);
    let mut capture = Capture::new();
    water.frame_keeping(4, &mut capture);
    let kept = capture.take(gpu.device(), true);
    assert_eq!(kept.len(), 4);
    let speed = a.speed_to(b, FRAME);
    for c in &kept {
        let t = if c.moment.steps == 0 { 1.0 } else { c.moment.steps as f32 / c.moment.of as f32 };
        let want = Pose::lerp(a, b, t);
        let (pose, fast) = c.poses[0];
        assert!(
            (pose.at - want.at).length() < 1e-3
                && (pose.turn - want.turn).abs() < 1e-5
                && (pose.scale - want.scale).abs() < 1e-5,
            "at {t} the part is at {pose:?}, not {want:?}"
        );
        assert_eq!(fast, speed);
    }
    assert_eq!(kept[3].poses[0].0, b, "the frame's end is where it was keyed");
    let l = water.layout();
    let mut codec = FrameCodec::new();
    let bytes = codec.encode(&kept[1].drops, (l.cols - 1) * (l.rows - 1), &kept[1].poses);
    let back = codec.decode(&bytes, &l).unwrap();
    assert_eq!(back.poses, kept[1].poses);
    let ahead = back.poses_at(0.004);
    assert_eq!(ahead, vec![kept[1].poses[0].0.moved_on(speed, 0.004)]);
}

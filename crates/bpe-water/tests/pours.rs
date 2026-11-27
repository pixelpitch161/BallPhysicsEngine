//! Rain, drips, bursts, drains and fed pours (Water mode part 4.1,
//! `docs/superpowers/plans/water-mode/part4-more-liquids.md`): drops born at their own moments,
//! drains that take the liquid reaching them and count it, and taps that pour only what the drains
//! took.

use std::sync::Arc;

use bpe_water::drops::LEAST_WALL;
use bpe_water::drops::numbers::FRAME;
use bpe_water::probe;
use bpe_water::scenes::{self, Running};
use bpe_water::{
    Drain, Drop, Feel, Layout, LumpKind, Lumps, MOST_DRAINS, Numbers, Pour, STOPS, SolidField, Throw, Timed, WATER,
    Water, WaterGpu,
};
use glam::Vec2;

fn card() -> Option<Arc<WaterGpu>> {
    let (device, queue) = bpe_gfx::test_device().ok()?;
    Some(WaterGpu::new(&device, &queue))
}

/// An open course 400 x 800 with nothing in it (its edges are the walls), at `cell`-pixel cells.
fn open_course(cell: f32) -> (Layout, SolidField) {
    let layout = Layout::cover(Vec2::new(-200.0, -400.0), Vec2::new(200.0, 400.0), cell);
    (layout, SolidField::build_least(layout, &[], LEAST_WALL * cell))
}

fn liquid(water: &Water) -> usize {
    probe::liquid(&water.read()).len()
}

/// Plays `frames` frames, adding up what each drain took (each frame's report comes two frames
/// later: [`Water::drained`]).
fn play_counting(water: &mut Water, frames: u32, took: &mut [u64; MOST_DRAINS]) {
    for _ in 0..frames {
        water.frame();
        for (t, d) in took.iter_mut().zip(water.drained()) {
            *t += u64::from(d);
        }
    }
}

/// A drop born at its own moment falls as one added between frames at that moment does.
#[test]
fn a_timed_drop_falls_as_an_added_one() {
    let Some(gpu) = card() else { return };
    let (layout, field) = open_course(8.0);
    let at = layout.to_grid(Vec2::new(0.0, -300.0));
    let v = Vec2::new(30.0, 0.0) / layout.cell;
    let mut added = Water::new(gpu.clone(), &field, scenes::GRAVITY, Numbers::for_cell(8.0), &[Feel::default()]);
    let mut timed = Water::new(gpu.clone(), &field, scenes::GRAVITY, Numbers::for_cell(8.0), &[Feel::default()]);
    for _ in 0..3 {
        added.frame();
        timed.frame();
    }
    added.add(&[Drop::new(at, v, STOPS, 0, WATER)]);
    timed.set_timed(&[Timed { at: 3.0 * f64::from(FRAME), drop: Drop::new(at, v, STOPS, 0, WATER) }]);
    for _ in 0..10 {
        added.frame();
        timed.frame();
    }
    let (a, t) = (added.read(), timed.read());
    assert_eq!((a.len(), t.len()), (1, 1), "one drop each: the list is used once");
    let apart = Vec2::from(a[0].x).distance(Vec2::from(t[0].x));
    assert!(apart < 0.1, "the timed drop is {apart} cells from the added one");
}

/// Rain over the open course pours its amount (its lumps' area over the spacing squared), at its
/// moments, and plays the same twice and from a copy made part way.
#[test]
fn rain_pours_its_amount_and_repeats() {
    let Some(gpu) = card() else { return };
    let (layout, field) = open_course(8.0);
    let rain = Lumps {
        kind: LumpKind::Rain,
        place: Vec2::new(0.0, -360.0),
        width: 300.0,
        size: 14.0,
        every: 1.0 / 40.0,
        from: 0.1,
        until: 0.8,
        seed: 3,
        kind_bits: STOPS,
        colour: WATER,
    };
    let throw = |_: f64| Throw { aim: 90.0, speed: 200.0, spread: 20.0 };
    let clear = Numbers::for_cell(8.0).wall_off * layout.cell;
    let play = |water: &mut Water, until: u32| {
        while water.frames() < until {
            let t0 = f64::from(water.frames()) * f64::from(FRAME);
            water.set_timed(&rain.born(&layout, &field, clear, t0, t0 + f64::from(FRAME), throw));
            water.frame();
        }
    };
    let new = || Water::new(gpu.clone(), &field, scenes::GRAVITY, Numbers::for_cell(8.0), &[Feel::default()]);
    let mut a = new();
    play(&mut a, 5);
    assert_eq!(a.count(), 0, "nothing before its Starts");
    play(&mut a, 60);
    let poured = liquid(&a) as f64;
    let amount = rain.amount(&layout, 0.0, 1.0);
    assert!((poured / amount - 1.0).abs() < 0.05, "rain poured {poured} drops of {amount}");
    let mut b = new();
    play(&mut b, 23);
    let mut c = b.copy();
    play(&mut b, 60);
    play(&mut c, 60);
    let first = probe::print_of(&a.read());
    assert_eq!(first, probe::print_of(&b.read()), "twice from the start");
    assert_eq!(first, probe::print_of(&c.read()), "from a copy");
}

/// A leak over the whole pool takes the share a second its pull says (`1 - exp(-1)` in a second
/// at pull 1), the same at 12 and 6 px.
#[test]
fn a_leak_takes_its_share_a_second() {
    let Some(gpu) = card() else { return };
    let mut kept = Vec::new();
    for cell in [12.0, 6.0] {
        let mut run = scenes::still_box(cell, 0.3).start(&gpu, Numbers::for_cell(cell));
        let start = liquid(&run.water);
        run.water.set_drains(&[Drain::disc(Vec2::ZERO, 2000.0, 1.0)]);
        run.run_to(1.0);
        let share = liquid(&run.water) as f64 / start as f64;
        eprintln!("{cell} px: kept {share} of {start}");
        kept.push(share);
    }
    for share in &kept {
        assert!((share - 0.367_879).abs() < 0.04, "a leak at pull 1 keeps about 0.37 in a second: {kept:?}");
    }
}

/// A drain in a pool's floor empties it (but for a few drops clinging to the walls and corners, as
/// real water does), and the liquid never comes back; the report counts what it took to the drop.
#[test]
fn a_drain_empties_a_pool_and_counts_it() {
    let Some(gpu) = card() else { return };
    for cell in [12.0, 6.0] {
        let mut run = scenes::still_box(cell, 0.3).start(&gpu, Numbers::for_cell(cell));
        let start = liquid(&run.water) as u64;
        run.water.set_drains(&[Drain::disc(Vec2::new(0.0, 440.0), 60.0, 30.0)]);
        let mut took = [0; MOST_DRAINS];
        play_counting(&mut run.water, 180, &mut took);
        let left = liquid(&run.water) as u64;
        eprintln!("{cell} px: {left} of {start} left after 3 s");
        assert!(left * 20 < start, "the drain empties the pool: {left} of {start} left after 3 s at {cell} px");
        // (The last two frames' reports, then nothing more.)
        run.water.set_drains(&[]);
        play_counting(&mut run.water, 2, &mut took);
        assert_eq!(took[0] + left, start, "the drain's count is what it took");
        run.run_to(4.0);
        assert_eq!(liquid(&run.water) as u64, left, "nothing comes back");
    }
}

/// Two drains are told apart in the report; air in a drain's reach bursts and is not counted.
#[test]
fn the_report_tells_drains_apart_and_leaves_air_out() {
    let Some(gpu) = card() else { return };
    let mut run = scenes::pool_with_hole(12.0, true).start(&gpu, Numbers { air: 0, ..Numbers::for_cell(12.0) });
    let all = run.water.read();
    let (water, air) = (probe::liquid(&all).len() as u64, probe::air(&all).len());
    assert!(air > 50, "the pool holds a pocket of air: {air}");
    // (Each takes every drop in its reach at once: the first a disc in the pool, the second the rest.)
    let (middle, reach) = (Vec2::new(-150.0, 300.0), 100.0);
    let layout = run.water.layout();
    let within = |d: &&Drop| layout.to_world(Vec2::from(d.x)).distance(middle) < reach;
    let in_first = probe::liquid(&all).iter().filter(within).count() as u64;
    assert!(in_first > 50, "the first drain's reach holds liquid: {in_first}");
    run.water.set_drains(&[Drain::disc(middle, reach, f32::INFINITY), Drain::disc(Vec2::ZERO, 5000.0, f32::INFINITY)]);
    let mut took = [0; MOST_DRAINS];
    play_counting(&mut run.water, 4, &mut took);
    eprintln!("took {} and {} of {water} ({in_first} in the first's reach, {air} air)", took[0], took[1]);
    assert_eq!(run.water.read().len(), 0, "everything left");
    assert_eq!(took[0] + took[1], water, "the drains count the liquid and no air");
    // (A drop at the edge of the first's reach may have moved across it in the step.)
    assert!(took[0].abs_diff(in_first) * 50 <= in_first, "the first drain counts its own: {} of {in_first}", took[0]);
    assert!(took[2..].iter().all(|&t| t == 0));
}

/// An edge drain along the bottom takes a tap's water that reaches it: no pool forms on the floor,
/// and what it took and what is left add up to what the tap poured.
#[test]
fn an_edge_drain_takes_what_reaches_the_edge() {
    let Some(gpu) = card() else { return };
    let (_, field) = open_course(12.0);
    let tap = Pour { place: Vec2::new(0.0, -300.0), width: 48.0, speed: 600.0, until: 1.5, ..Pour::default() };
    let play = |drains: &[Drain]| {
        let mut water = Water::new(gpu.clone(), &field, scenes::GRAVITY, Numbers::for_cell(12.0), &[Feel::default()]);
        water.set_pours(&[tap]);
        water.set_drains(drains);
        let mut took = [0; MOST_DRAINS];
        play_counting(&mut water, 150, &mut took);
        // (The last two frames' reports.)
        water.set_drains(&[]);
        play_counting(&mut water, 2, &mut took);
        (liquid(&water) as u64, took[0])
    };
    let (pooled, _) = play(&[]);
    let (left, took) = play(&[Drain::edge(Drain::BOTTOM, 30.0)]);
    eprintln!("no drain: {pooled}; an edge drain: {left} left, {took} taken");
    assert!(left * 10 < pooled, "no pool forms: {left} drops left of {pooled}");
    assert_eq!(left + took, pooled, "what it took and what is left are what was poured");
}

/// A drain with no pull takes nothing: to the bit as no drain.
#[test]
fn a_drain_with_no_pull_changes_nothing() {
    let Some(gpu) = card() else { return };
    let scene = scenes::thin_pour(12.0, 2.0);
    let mut a = scene.start(&gpu, Numbers::for_cell(12.0));
    let mut b = scene.start(&gpu, Numbers::for_cell(12.0));
    b.water.set_drains(&[Drain::disc(Vec2::ZERO, 400.0, 0.0), Drain::edge(Drain::ALL_EDGES, 0.0)]);
    a.run_to(0.75);
    b.run_to(0.75);
    assert_eq!(probe::print_of(&a.water.read()), probe::print_of(&b.water.read()));
}

/// A pool over a drain that feeds a tap above it: the loop runs, and keeps every drop of its
/// liquid (once the tank has poured what it holds).
fn a_loop(gpu: &Arc<WaterGpu>, cell: f32) -> Running {
    let mut run = scenes::still_box(cell, 0.3).start(gpu, Numbers::for_cell(cell));
    run.water.set_drains(&[Drain::disc(Vec2::new(0.0, 440.0), 40.0, 30.0)]);
    run.water.set_pours(&[Pour {
        place: Vec2::new(-150.0, -300.0),
        width: 60.0,
        speed: scenes::POUR_SPEED,
        fed: true,
        ..Pour::default()
    }]);
    run
}

#[test]
fn a_loop_keeps_its_liquid() {
    let Some(gpu) = card() else { return };
    let mut run = a_loop(&gpu, 12.0);
    let start = liquid(&run.water) as u64;
    let mut took = [0; MOST_DRAINS];
    let mut lowest = start;
    for _ in 0..10 {
        play_counting(&mut run.water, 60, &mut took);
        let now = liquid(&run.water) as u64 + run.water.tank();
        lowest = lowest.min(now);
    }
    eprintln!("start {start}, taken {}, lowest in the course and the tank {lowest}", took[0]);
    assert!(took[0] > start, "the loop runs: {} taken from a pool of {start}", took[0]);
    // (Two frames' takings are on their way to the tank at any moment.)
    assert!(lowest * 100 >= start * 95, "the course and the tank hold the liquid: {lowest} of {start}");
    run.water.set_drains(&[]);
    for _ in 0..600 {
        run.frame();
        if run.water.frames() > 602 && run.water.tank() == 0 {
            break;
        }
    }
    run.frame();
    assert_eq!(liquid(&run.water) as u64, start, "every drop is kept");
}

/// A fed tap with no drain feeding it pours nothing.
#[test]
fn a_fed_tap_with_no_feeding_drain_pours_nothing() {
    let Some(gpu) = card() else { return };
    let mut run = a_loop(&gpu, 12.0);
    run.water.set_drains(&[Drain { feeds: false, ..run.water.drains()[0] }]);
    let start = liquid(&run.water) as u64;
    run.run_to(1.0);
    assert_eq!(run.water.tank(), 0);
    let left = liquid(&run.water) as u64;
    assert!(left < start, "the drain takes {left} of {start}");
    // (Only the drain changes the liquid: the tap pours none.)
    run.run_to(1.5);
    assert!(liquid(&run.water) as u64 <= left);
}

/// A body made from a restart point taken while a fed tap pours from the tank plays on to the bit.
#[test]
fn the_tank_survives_a_restart_point() {
    let Some(gpu) = card() else { return };
    let mut a = a_loop(&gpu, 12.0);
    a.run_to(1.0);
    assert!(a.water.tank() > 0, "the tank holds some");
    let state = a.water.state();
    let bytes = state.to_bytes();
    let back = bpe_water::WaterState::from_bytes(&bytes).expect("the state reads back");
    assert_eq!(back, state);
    let mut b =
        Water::from_state(gpu.clone(), &a.field, scenes::GRAVITY, Numbers::for_cell(12.0), a.water.feels(), &back)
            .expect("a body from the state");
    let mut c = a.copy();
    a.run_to(1.5);
    while b.frames() < a.water.frames() {
        b.frame();
    }
    c.run_to(1.5);
    let first = probe::print_of(&a.water.read());
    assert_eq!(first, probe::print_of(&b.read()), "from the restart point");
    assert_eq!(first, probe::print_of(&c.water.read()), "from a copy");
    assert_eq!(a.water.tank(), b.tank());
}

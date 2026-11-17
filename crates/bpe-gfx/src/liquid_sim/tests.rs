#![allow(clippy::needless_range_loop)] // Colour channels and bands read best by number.

use super::*;

fn card() -> Arc<LiquidCard> {
    let (device, queue) = crate::test_device().expect("a graphics device (install mesa-vulkan-drivers)");
    LiquidCard::for_device(&device, &queue)
}

/// Water's numbers for a liquid in a grid `rows` tall, gravity `g` cells/s², stepped at 240 Hz.
fn water(rows: u32, g: f32, style: u32) -> (StepGpu, LiquidGpu) {
    let c2 = g.abs().max(1.0) * rows as f32 / 0.05;
    let substeps = ((c2.sqrt() / 240.0 / 0.4).ceil() as u32).clamp(1, MOST_SUBSTEPS);
    let dt = 1.0 / (240.0 * substeps as f32);
    let mut step = StepGpu::default();
    step.grid[2] = substeps;
    step.grid[3] = 1;
    step.time = [dt, 0.0, g, 0.45 / dt];
    step.flags[3] = 1;
    step.more = [0.2 / dt, MIX_EVERY as f32 / 240.0, 0.05, 0.08];
    let liquid = LiquidGpu {
        a: [c2, 0.1, 0.1, 0.5],
        b: [1.0, 1.0, 1.0, 1.0],
        c: [style as f32, 0.0, 0.0, LiquidGpu::NO_CUT],
        d: [0; 4],
    };
    step.liquids[0] = liquid;
    step.set_pair(0, 0, MEET_MIX, style, WHEN_ALWAYS, 1.0);
    (step, liquid)
}

/// A two-by-two lattice over cells `x0..x1` × `y0..y1`, coloured by `colour(column, row)`.
fn lattice(x0: u32, x1: u32, y0: u32, y1: u32, colour: impl Fn(u32, u32) -> [f32; 4]) -> Vec<Drop> {
    let mut out = Vec::new();
    let mut serial = 0;
    for r in (y0 * 2)..(y1 * 2) {
        for c in (x0 * 2)..(x1 * 2) {
            let x = [c as f32 * 0.5 + 0.25, r as f32 * 0.5 + 0.25];
            out.push(Drop::new(x, [0.0, 0.0], colour(c, r), STOPS, serial));
            serial += 1;
        }
    }
    out
}

/// Steps `body` `steps` times; the mixing ones are every [`MIX_EVERY`]th counted from the body's
/// first step, as the world counts them.
fn run(body: &mut Body, step: &StepGpu, first: &[Drop], steps: u32) {
    for _ in 0..steps {
        let k = body.steps;
        let mut s = *step;
        s.flags[0] = u32::from(k % MIX_EVERY == MIX_EVERY - 1);
        let births = if k == 0 { first } else { &[] };
        body.step(&StepIn { step: s, births, walls: None, balls: &[] });
    }
}

fn all_finite(drops: &[Drop]) -> bool {
    drops.iter().all(|d| d.x.iter().chain(&d.v).chain(&d.colour).all(|v| v.is_finite()))
}

#[test]
fn paint_back_undoes_the_basis() {
    let (paint, back) = paint_tables();
    for rgb in [[1.0f32, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.3, 0.6, 0.2]] {
        let mut out = [0.0f32; 3];
        for band in 0..8 {
            let r = paint[band][0] * rgb[2] + paint[band][1] * rgb[1] + paint[band][2] * rgb[0];
            for c in 0..3 {
                out[c] += back[band][c] * r;
            }
        }
        for c in 0..3 {
            assert!((out[c] - rgb[c]).abs() < 1e-5, "{rgb:?} came back as {out:?}");
        }
    }
}

#[test]
fn a_box_of_water_settles_level() {
    let mut body = Body::new(card(), 40, 40);
    let (mut step, _) = water(40, 120.0, STYLE_BLEND);
    // Settles fast, so the test needn't wait for the slosh to die down.
    step.liquids[0].a[3] = 1.5;
    // A heap in the left half, three quarters up the box: it must spread across and settle.
    let heap = lattice(3, 20, 10, 38, |_, _| [0.2, 0.4, 0.9, 1.0]);
    let n = heap.len();
    run(&mut body, &step, &heap, 1200);
    let drops = body.read();
    assert!(all_finite(&drops));
    assert_eq!(drops.len(), n, "no drop is lost in a closed box");
    assert!(drops.iter().all(|d| (2.0..=38.0).contains(&d.x[0]) && (2.0..=38.0).contains(&d.x[1])));
    // How deep it stands in each of 8 bands across: the drops there over the band's width (four drops a
    // cell), level within a few cells.
    let mut tops = [0.0f32; 8];
    for d in &drops {
        let band = (((d.x[0] - 2.0) / 36.0 * 8.0) as usize).min(7);
        tops[band] += 1.0 / (36.0 / 8.0 * 4.0);
    }
    let (lo, hi) = tops.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), &t| (a.min(t), b.max(t)));
    if std::env::var_os("LIQUID_PICTURE").is_some() {
        let mut rows = vec![vec![0u32; 40]; 40];
        for d in &drops {
            rows[(d.x[1] as usize).min(39)][(d.x[0] as usize).min(39)] += 1;
        }
        for r in rows {
            eprintln!("{}", r.iter().map(|&n| char::from_digit(n.min(9), 10).unwrap()).collect::<String>());
        }
    }
    assert!(hi - lo < 2.5, "the surface isn't level: {tops:?}");
}

#[test]
fn two_runs_are_identical_and_a_copy_steps_like_the_original() {
    let gpu = card();
    let (step, _) = water(32, 120.0, STYLE_BLEND);
    let heap = lattice(4, 16, 6, 28, |c, _| if c % 2 == 0 { [1.0, 0.0, 0.0, 1.0] } else { [0.0, 0.0, 1.0, 1.0] });
    let mut a = Body::new(gpu.clone(), 32, 32);
    run(&mut a, &step, &heap, 100);
    let mut b = Body::new(gpu.clone(), 32, 32);
    run(&mut b, &step, &heap, 50);
    let mut copy = b.clone();
    run(&mut b, &step, &[], 50);
    run(&mut copy, &step, &[], 50);
    let (ra, rb, rc) = (a.read(), b.read(), copy.read());
    assert!(!ra.is_empty());
    assert!(ra == rb, "two runs differ");
    assert!(ra == rc, "a copy steps differently");
}

/// The mean colour of `drops`.
fn mean(drops: &[Drop]) -> [f32; 3] {
    let mut m = [0.0f32; 3];
    for d in drops {
        for c in 0..3 {
            m[c] += d.colour[c] / drops.len() as f32;
        }
    }
    m
}

/// Interleaved drops of `a` and `b` with no gravity, mixed by `style` for 80 steps.
fn mixed(style: u32, a: [f32; 4], b: [f32; 4]) -> Vec<Drop> {
    let mut body = Body::new(card(), 24, 24);
    let (mut step, _) = water(24, 0.0, style);
    step.time[2] = 0.0;
    let drops = lattice(6, 18, 6, 18, |c, r| if (c + r) % 2 == 0 { a } else { b });
    run(&mut body, &step, &drops, 80);
    body.read()
}

#[test]
fn paint_mix_of_blue_and_yellow_is_green() {
    let drops = mixed(STYLE_PAINT, [0.1, 0.2, 0.9, 1.0], [0.95, 0.85, 0.1, 1.0]);
    let [r, g, b] = mean(&drops);
    assert!(g > r && g > b, "paint mixed blue and yellow into {r} {g} {b}");
}

#[test]
fn light_mix_of_red_and_green_is_yellow() {
    let drops = mixed(STYLE_LIGHT, [1.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 1.0]);
    let [r, g, b] = mean(&drops);
    assert!(r > 0.6 && g > 0.6 && b < 0.2, "light mixed red and green into {r} {g} {b}");
}

#[test]
fn blend_meets_in_the_middle() {
    let drops = mixed(STYLE_BLEND, [1.0, 1.0, 1.0, 1.0], [0.0, 0.0, 0.0, 1.0]);
    let spread = drops.iter().map(|d| d.colour[0]).fold((1.0f32, 0.0f32), |(lo, hi), x| (lo.min(x), hi.max(x)));
    assert!(spread.1 - spread.0 < 0.5, "blend left them apart: {spread:?}");
}

#[test]
fn marble_and_keep_apart_pass_no_colour() {
    for style in [STYLE_MARBLE, STYLE_KEEP_APART] {
        let drops = mixed(style, [1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]);
        for d in &drops {
            let red = (d.colour[0] - 1.0).abs() < 0.01 && d.colour[2] < 0.01;
            let blue = (d.colour[2] - 1.0).abs() < 0.01 && d.colour[0] < 0.01;
            assert!(red || blue, "style {style}: a drop turned {:?}", d.colour);
        }
    }
}

#[test]
fn drops_leave_past_the_area_past_most_and_above_the_cut() {
    let mut body = Body::new(card(), 32, 32);
    let (mut step, _) = water(32, 0.0, STYLE_BLEND);
    step.time[2] = 0.0;
    step.flags[3] = 0;
    // One drop flying off the left edge (it doesn't stop at edges), and a block of still ones.
    let mut drops = lattice(10, 20, 10, 20, |_, _| [1.0; 4]);
    for d in &mut drops {
        d.kind = 0;
    }
    let n = drops.len() as u32;
    drops.push(Drop::new([3.0, 15.0], [-200.0, 0.0], [1.0; 4], 0, n));
    run(&mut body, &step, &drops, 20);
    assert_eq!(body.read().len() as u32, n, "the flying drop is gone");
    // Born before the floor: gone.
    step.liquids[0].d[0] = 100;
    run(&mut body, &step, &[], 2);
    let left = body.read();
    assert!(left.iter().all(|d| d.serial >= 100));
    assert_eq!(left.len() as u32, n - 100);
    // Above the cut line: gone.
    step.liquids[0].c[3] = 15.0;
    run(&mut body, &step, &[], 2);
    assert!(body.read().iter().all(|d| d.x[1] >= 15.0));
}

#[test]
fn nan_numbers_give_no_nan_drops() {
    let mut body = Body::new(card(), 24, 24);
    let (mut step, _) = water(24, 120.0, STYLE_PAINT);
    step.liquids[0].a[0] = f32::NAN;
    step.liquids[0].b[0] = f32::NAN;
    step.time[1] = f32::NAN;
    let drops = lattice(4, 20, 4, 20, |_, _| [0.5, 0.5, 0.5, 1.0]);
    run(&mut body, &step, &drops, 40);
    assert!(all_finite(&body.read()));
}

#[test]
fn walls_hold_the_liquid_and_balls_push_it_aside() {
    let mut body = Body::new(card(), 40, 40);
    let (step, _) = water(40, 120.0, STYLE_BLEND);
    // A floor at y = 30 across the whole grid (normal up), nodes within two cells of it.
    let mut band = Vec::new();
    for y in 26..34u32 {
        for x in 0..40u32 {
            band.push(WallNode { index: y * 40 + x, n: [0.0, -1.0], dist: 30.0 - y as f32, half: -1.0, v: [0.0, 0.0] });
        }
    }
    let drops = lattice(4, 36, 10, 20, |_, _| [1.0; 4]);
    let ball = BallGpu { x: [20.0, 24.0], v: [0.0, 0.0], r: 4.0 };
    for k in 0..600 {
        let walls = if k == 0 { Some(band.as_slice()) } else { None };
        let births = if k == 0 { drops.as_slice() } else { &[] };
        body.step(&StepIn { step, births, walls, balls: &[ball] });
    }
    let out = body.read();
    assert!(out.iter().all(|d| d.x[1] < 30.0 + 0.6), "a drop went through the floor");
    let inside = out.iter().filter(|d| ((d.x[0] - 20.0).powi(2) + (d.x[1] - 24.0).powi(2)).sqrt() < 4.0).count();
    assert_eq!(inside, 0, "drops inside the ball");
}

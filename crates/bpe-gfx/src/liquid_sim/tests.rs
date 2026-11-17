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
        e: [1.0, 3.0, 1.0, 0.0],
        f: [0.0; 4],
        g: [1.0, 1e9, 0.0, 0.0],
        h: [0.0, 3.0, -1.0, 2.0],
        ..Default::default()
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
        body.step(&StepIn { step: s, births, walls: None, balls: &[], feel: false, fresh: false });
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
            band.push(WallNode {
                index: y * 40 + x,
                n: [0.0, -1.0],
                dist: 30.0 - y as f32,
                half: -1.0,
                v: [0.0, 0.0],
                wall: 255,
            });
        }
    }
    let drops = lattice(4, 36, 10, 20, |_, _| [1.0; 4]);
    let ball = BallGpu { x: [20.0, 24.0], v: [0.0, 0.0], r: 4.0, mask: 1 };
    for k in 0..600 {
        let walls = if k == 0 { Some(band.as_slice()) } else { None };
        let births = if k == 0 { drops.as_slice() } else { &[] };
        body.step(&StepIn { step, births, walls, balls: &[ball], feel: false, fresh: false });
    }
    let out = body.read();
    assert!(out.iter().all(|d| d.x[1] < 30.0 + 0.6), "a drop went through the floor");
    let inside = out.iter().filter(|d| ((d.x[0] - 20.0).powi(2) + (d.x[1] - 24.0).powi(2)).sqrt() < 4.0).count();
    assert_eq!(inside, 0, "drops inside the ball");
}

/// A floor at y = `at` across a `cols`-wide grid (normal up).
fn floor(cols: u32, at: u32) -> Vec<WallNode> {
    let mut band = Vec::new();
    for y in (at - 4)..(at + 4) {
        for x in 0..cols {
            band.push(WallNode {
                index: y * cols + x,
                n: [0.0, -1.0],
                dist: at as f32 - y as f32,
                half: -1.0,
                v: [0.0, 0.0],
                wall: 255,
            });
        }
    }
    band
}

/// Water from y = 10 to the floor at 30 settled for `settle` steps, then `feel` steps with
/// `balls` recorded; what they felt.
fn feel_in_water(balls: &[BallGpu], settle: u32, feel: u32) -> Vec<i32> {
    let mut body = Body::new(card(), 40, 40);
    let (mut step, _) = water(40, 120.0, STYLE_BLEND);
    // Calm water: the substeps the Liquid thing takes (a pressure wave 0.15 cells a substep).
    let c = step.liquids[0].a[0].sqrt();
    let substeps = ((c / 240.0 / 0.15).ceil() as u32).clamp(1, MOST_SUBSTEPS);
    let dt = 1.0 / (240.0 * substeps as f32);
    step.grid[2] = substeps;
    step.time[0] = dt;
    step.time[3] = 0.45 / dt;
    step.more[0] = 0.2 / dt;
    let band = floor(40, 30);
    let drops = lattice(2, 38, 10, 30, |_, _| [0.2, 0.4, 0.8, 1.0]);
    for k in 0..settle + feel {
        let walls = if k == 0 { Some(band.as_slice()) } else { None };
        let births = if k == 0 { drops.as_slice() } else { &[] };
        let recording = k >= settle;
        body.step(&StepIn { step, births, walls, balls, feel: recording, fresh: k == settle });
    }
    body.send_feel(balls.len() as u32).expect("some balls").wait().to_vec()
}

/// The same water's stat words after `settle` steps, with no balls.
fn stats_in_water(settle: u32) -> (Vec<u32>, usize) {
    let mut body = Body::new(card(), 40, 40);
    let (step, _) = water(40, 120.0, STYLE_BLEND);
    let band = floor(40, 30);
    let drops = lattice(2, 38, 10, 30, |_, _| [0.2, 0.4, 0.8, 1.0]);
    for k in 0..settle {
        let walls = if k == 0 { Some(band.as_slice()) } else { None };
        let births = if k == 0 { drops.as_slice() } else { &[] };
        body.step(&StepIn { step, births, walls, balls: &[], feel: true, fresh: k == 0 });
    }
    (body.send_feel(0).expect("a stepped body").stats().to_vec(), drops.len())
}

#[test]
fn a_ball_deeper_in_the_water_has_more_above_it() {
    let deep = BallGpu { x: [12.0, 24.0], v: [0.0, 0.0], r: 2.0, mask: 1 };
    let shallow = BallGpu { x: [28.0, 13.0], v: [0.0, 0.0], r: 2.0, mask: 1 };
    let f = feel_in_water(&[deep, shallow], 480, 24);
    let depth = |b: usize| f[b * FEEL_WORDS + feel_at::DEPTH] as f32 / 16.0;
    // Its top (24 - 2 - 1) is about 11 cells under the surface at 10.
    assert!(depth(0) > 6.0 && depth(0) < 15.0, "about 11 cells under: {}", depth(0));
    assert!(depth(1) < 3.0, "at the top: {}", depth(1));
}

#[test]
fn the_stats_count_every_drop_and_their_colour() {
    let (s, n) = stats_in_water(60);
    assert_eq!(s.len(), STATS_LEN);
    assert_eq!(s[stat_at::DROPS] as usize, n, "every drop is liquid 0's");
    assert!(s[STAT_WORDS..].iter().all(|w| *w == 0), "no other liquid");
    let mean = |c: usize| s[stat_at::COLOUR + c] as f32 / n as f32 / 63.0;
    for (c, want) in [0.2f32, 0.4, 0.8].into_iter().enumerate() {
        assert!((mean(c) - want).abs() < 0.02, "channel {c}: {}", mean(c));
    }
    let row = s[stat_at::ROW] as f32 / n as f32;
    assert!(row > 14.0 && row < 26.0, "the water's middle row: {row}");
    assert_eq!(stats_in_water(60).0, s, "the same every time");
}

#[test]
fn a_ball_held_under_still_water_is_pushed_up_and_feels_wet() {
    let ball = BallGpu { x: [20.0, 20.0], v: [0.0, 0.0], r: 3.0, mask: 1 };
    let f = feel_in_water(&[ball], 480, 240);
    let push_y = f[feel_at::PUSH + 1] as f32 / S_PUSH;
    assert!(push_y < 0.0, "pushed up (y is down): {push_y}");
    assert!(f[feel_at::TOUCH] > 0, "touched");
    assert!(f[feel_at::WET] >= RING as i32 - 4, "under all round: {}", f[feel_at::WET]);
    assert!(f[feel_at::LIQUIDS] > 0, "contacts with liquid 0");
    // The upward push is about the water it puts aside: π r² × weight × g × the time.
    // (The water still sloshes a little after the ball was put in it.)
    let expected = std::f32::consts::PI * 3.5 * 3.5 * 1.0 * 120.0 * 240.0 / 240.0;
    assert!(-push_y > expected * 0.2 && -push_y < expected * 2.0, "push {push_y}, about {expected}");
}

#[test]
fn a_ball_out_of_the_water_or_not_in_its_mask_feels_nothing() {
    let dry = BallGpu { x: [20.0, 4.0], v: [0.0, 0.0], r: 2.0, mask: 1 };
    let unmet = BallGpu { x: [20.0, 20.0], v: [0.0, 0.0], r: 3.0, mask: 0b10 };
    let f = feel_in_water(&[dry, unmet], 200, 8);
    assert!(f.iter().all(|w| *w == 0), "{f:?}");
}

#[test]
fn the_feel_is_the_same_every_time() {
    let ball = BallGpu { x: [20.0, 16.0], v: [3.0, 20.0], r: 3.0, mask: 1 };
    assert_eq!(feel_in_water(&[ball], 120, 12), feel_in_water(&[ball], 120, 12));
}

// ---- part 4: stranger liquids ----------------------------------------------------------------

/// The drops' spread: the root mean square distance from their middle.
fn spread(drops: &[Drop]) -> f32 {
    let n = drops.len().max(1) as f32;
    let m = drops.iter().fold([0.0f32; 2], |a, d| [a[0] + d.x[0] / n, a[1] + d.x[1] / n]);
    (drops.iter().map(|d| (d.x[0] - m[0]).powi(2) + (d.x[1] - m[1]).powi(2)).sum::<f32>() / n).sqrt()
}

/// A blob in the middle of a 48 × 48 box with no gravity, pulled out sideways and squeezed
/// up and down (a stretch that keeps its area: only Stretch resists it).
fn thrown_blob() -> Vec<Drop> {
    let mut blob = lattice(18, 30, 18, 30, |_, _| [0.5, 0.5, 0.5, 1.0]);
    for d in &mut blob {
        let off = [d.x[0] - 24.0, d.x[1] - 24.0];
        d.v = [off[0] * 6.0, -off[1] * 6.0];
    }
    blob
}

#[test]
fn stretch_holds_a_blob_together_and_tears_at_its_limit() {
    let gpu = card();
    let (mut step, _) = water(48, 0.0, STYLE_KEEP_APART);
    step.liquids[0].a[1] = 0.0;
    let mut water_body = Body::new(gpu.clone(), 48, 48);
    run(&mut water_body, &step, &thrown_blob(), 60);
    let mut slime = step;
    slime.liquids[0].h = [0.8, 20f32.ln(), -1.0, 2.0];
    let mut slime_body = Body::new(gpu.clone(), 48, 48);
    run(&mut slime_body, &slime, &thrown_blob(), 60);
    let (w, s) = (spread(&water_body.read()), spread(&slime_body.read()));
    assert!(all_finite(&slime_body.read()));
    assert!(s < w * 0.9, "slime ({s}) should spread less than water ({w})");
    // Torn at 1.2 times its length: no drop is stretched further.
    let mut torn = step;
    torn.liquids[0].h = [0.8, 1.2f32.ln(), -1.0, 2.0];
    let mut torn_body = Body::new(gpu, 48, 48);
    run(&mut torn_body, &torn, &thrown_blob(), 60);
    for d in torn_body.read() {
        let f = d.f;
        let j = f[0] * f[3] - f[1] * f[2];
        let (bxx, bxy, byy) =
            ((f[0] * f[0] + f[1] * f[1]) / j, (f[0] * f[2] + f[1] * f[3]) / j, (f[2] * f[2] + f[3] * f[3]) / j);
        let half = 0.5 * (bxx + byy);
        let disc = (half * half - (bxx * byy - bxy * bxy)).max(0.0).sqrt();
        let strain = 0.5 * (half + disc).ln().abs().max((half - disc).max(1e-6).ln().abs());
        assert!(strain <= 1.2f32.ln() + 0.05, "a drop is stretched {strain}");
    }
}

#[test]
fn frozen_liquid_keeps_its_shape_and_water_spreads() {
    let gpu = card();
    let (mut step, _) = water(40, 120.0, STYLE_BLEND);
    step.liquids[0].a[3] = 1.5;
    let heap = || lattice(3, 14, 12, 38, |_, _| [0.2, 0.4, 0.9, 1.0]);
    let mut water_body = Body::new(gpu.clone(), 40, 40);
    run(&mut water_body, &step, &heap(), 600);
    let mut ice = step;
    // Cold (heat 0 at birth) and freezing below 0.5.
    ice.liquids[0].h = [0.0, 3.0, 0.5, 0.6];
    let mut ice_body = Body::new(gpu, 40, 40);
    run(&mut ice_body, &ice, &heap(), 600);
    let top = |d: &[Drop]| d.iter().map(|d| d.x[1]).fold(f32::INFINITY, f32::min);
    let (w, i) = (water_body.read(), ice_body.read());
    assert!(i.iter().all(|d| d.kind & FROZEN != 0), "every drop froze");
    assert!(top(&i) < top(&w) - 8.0, "ice stands taller ({}) than water ({})", top(&i), top(&w));
    assert!(all_finite(&i));
}

#[test]
fn a_drain_takes_the_liquid_and_a_magnet_gathers_it() {
    let gpu = card();
    let (mut step, _) = water(40, 120.0, STYLE_BLEND);
    step.add_force(ForceGpu { a: [20.0, 37.0, 18.0, 60.0], b: [0.0, 0.0, 30.0, 3.0], c: [FORCE_DRAIN, 1, 0, 0] });
    let water_box = lattice(3, 37, 20, 38, |_, _| [0.2, 0.4, 0.9, 1.0]);
    let n = water_box.len();
    let mut body = Body::new(gpu.clone(), 40, 40);
    run(&mut body, &step, &water_box, 480);
    let left = body.read().len();
    assert!(left < n * 3 / 4, "the drain took {} of {n}", n - left);

    let (mut free, _) = water(48, 0.0, STYLE_BLEND);
    let blob = lattice(4, 44, 4, 44, |_, _| [0.5, 0.5, 0.5, 1.0]);
    let mut plain = Body::new(gpu.clone(), 48, 48);
    run(&mut plain, &free, &blob, 240);
    free.add_force(ForceGpu { a: [24.0, 24.0, 40.0, 400.0], b: [0.0, 0.0, 0.5, 0.0], c: [FORCE_MAGNET, 1, 6, 0] });
    let mut pulled = Body::new(gpu, 48, 48);
    run(&mut pulled, &free, &blob, 240);
    let near = |d: &[Drop]| {
        d.iter().map(|d| ((d.x[0] - 24.0).powi(2) + (d.x[1] - 24.0).powi(2)).sqrt()).sum::<f32>() / d.len() as f32
    };
    let (a, b) = (near(&plain.read()), near(&pulled.read()));
    assert!(b < a * 0.8, "the magnet pulled them in ({b} against {a})");
}

#[test]
fn two_liquids_react_into_a_third_and_ink_fades() {
    let gpu = card();
    let (mut step, lq) = water(40, 120.0, STYLE_BLEND);
    step.grid[3] = 3;
    step.liquids[1] = lq;
    step.liquids[2] = LiquidGpu { m: [0.0, 1.0, 0.0, 1.0], ..lq };
    step.set_react(0, 1, 2, 1.0, 0.5, 0.2);
    step.set_react(1, 0, 2, 1.0, 0.5, 0.2);
    let mut a = lattice(3, 20, 20, 38, |_, _| [1.0, 0.0, 0.0, 1.0]);
    let b: Vec<Drop> = lattice(20, 37, 20, 38, |_, _| [0.0, 0.0, 1.0, 1.0])
        .into_iter()
        .map(|d| Drop { kind: STOPS | 1, serial: d.serial + 100_000, ..d })
        .collect();
    a.extend(b);
    let mut body = Body::new(gpu.clone(), 40, 40);
    for k in 0..240u32 {
        let mut s = step;
        s.extra[0] = k;
        s.flags[0] = u32::from(k % MIX_EVERY == MIX_EVERY - 1);
        let births = if k == 0 { &a[..] } else { &[] };
        body.step(&StepIn { step: s, births, walls: None, balls: &[], feel: false, fresh: false });
    }
    let drops = body.read();
    let made: Vec<&Drop> = drops.iter().filter(|d| d.liquid() == 2).collect();
    assert!(made.len() > 20, "only {} drops reacted", made.len());
    assert!(made.iter().all(|d| d.colour[1] > 0.5 && d.spare[0] > 0.0), "made drops take its colour and heat");

    let (mut fading, _) = water(40, 120.0, STYLE_KEEP_APART);
    fading.liquids[0].j = [0.0, 4.0, 0.0, 0.0];
    fading.liquids[0].k = [1.0, 1.0, 0.0, 0.0];
    let ink = lattice(3, 37, 20, 38, |_, _| [0.0, 0.0, 1.0, 1.0]);
    let mut body = Body::new(gpu, 40, 40);
    run(&mut body, &fading, &ink, 480);
    assert!(body.read().iter().all(|d| d.colour[0] > 0.8 && d.colour[2] < 0.2), "the ink faded to yellow");
}

#[test]
fn dried_drops_go_or_leave_stains() {
    let gpu = card();
    let (mut step, _) = water(40, 120.0, STYLE_BLEND);
    step.liquids[0].i = [0.0, 0.0, 0.0, 3.0];
    let puddle = || lattice(3, 37, 34, 38, |_, _| [0.2, 0.4, 0.9, 1.0]);
    let n = puddle().len();
    let mut body = Body::new(gpu.clone(), 40, 40);
    run(&mut body, &step, &puddle(), 480);
    assert!(body.read().len() < n * 3 / 4, "it dried up");
    step.liquids[0].j[0] = 1.0;
    let mut stains = Body::new(gpu, 40, 40);
    run(&mut stains, &step, &puddle(), 480);
    let drops = stains.read();
    assert_eq!(drops.len(), n, "with stains, every dried drop stays");
    let marks: Vec<&Drop> = drops.iter().filter(|d| d.is_stain()).collect();
    assert!(marks.len() > n / 4, "{} stains", marks.len());
    assert!(marks.iter().all(|d| d.liquid() == 0 && d.v == [0.0, 0.0]));
}

#[test]
fn unmix_plays_the_drops_back_to_where_they_were() {
    let gpu = card();
    let (step, _) = water(40, 120.0, STYLE_BLEND);
    let heap = lattice(3, 20, 10, 38, |c, _| if c % 2 == 0 { [1.0, 0.0, 0.0, 1.0] } else { [0.0, 0.0, 1.0, 1.0] });
    let mut body = Body::new(gpu, 40, 40);
    run(&mut body, &step, &heap, 1);
    let start = body.read();
    body.record(100);
    for _ in 0..30 {
        run(&mut body, &step, &[], 4);
        body.record(100);
    }
    assert_eq!(body.samples(), 31);
    let copy = body.clone();
    assert_eq!(copy.samples(), 31);
    assert_eq!(copy.history_bytes(), body.history_bytes(), "a copy shares the samples");
    let moved = body.read();
    assert!(moved.iter().zip(&start).any(|(a, b)| (a.x[0] - b.x[0]).abs() > 2.0), "the heap moved");
    let mut steps = 0;
    while body.rewind_step(&step, &[], 4) {
        steps += 1;
        assert!(steps < 200);
    }
    assert!(steps >= 119, "played back for {steps} steps");
    let back = body.read();
    assert_eq!(back.len(), start.len());
    for (a, b) in back.iter().zip(&start) {
        assert_eq!(a.serial, b.serial);
        assert!((a.x[0] - b.x[0]).abs() < 0.01 && (a.x[1] - b.x[1]).abs() < 0.01, "{:?} came back to {:?}", b.x, a.x);
        assert!((a.colour[0] - b.colour[0]).abs() < 0.02);
    }
    // And it goes on from there.
    run(&mut body, &step, &[], 10);
    assert!(all_finite(&body.read()));
}

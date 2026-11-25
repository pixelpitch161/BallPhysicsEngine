//! Measuring runs for the builder: ignored, run by hand in release, one at a time,
//!
//! `cargo test --release -p bpe-water --test measure -- --ignored --nocapture --test-threads=1 look`
//!
//! - `WATER_SCENES=weird,thin_pour` picks the scenes (see `scene_named`).
//! - `WATER_CELLS=8,6` the cell sizes in pixels.
//! - `WATER_TIMES=1,2,4` when to measure, seconds.
//! - `WATER_OUT=<folder>` where debug pictures go (none without it).
//! - `WATER_DUMP=<folder>` where `look` writes the drops themselves (none without it).
//! - `WATER_SPRAY`, `WATER_SETTLES` the liquid's Spray and Settles in every scene; `WATER_SHIFT`
//!   moves the grid by that part of a cell.
//! - `BPE_WATER_*` replace the solver's numbers (`Numbers::from_env_for`: the steps a second
//!   follow the cell size unless `BPE_WATER_RATE` says otherwise).
//!
//! `pace` times scenes with no read-back until the end (`WATER_SECONDS`): the core's speed.

use std::time::Instant;

use bpe_core::maths::Maths;
use bpe_water::probe::{self, Spacing, Stats, Stream};
use bpe_water::scenes::{self, Scene};
use bpe_water::{Numbers, WaterGpu};
use glam::Vec2;

fn list<T: std::str::FromStr>(name: &str, or: &str) -> Vec<T> {
    let asked = std::env::var(name).unwrap_or_else(|_| or.into());
    asked.split(',').filter_map(|s| s.trim().parse().ok()).collect()
}

fn num<T: std::str::FromStr>(name: &str, or: T) -> T {
    std::env::var(name).ok().and_then(|s| s.parse().ok()).unwrap_or(or)
}

fn scene_named(name: &str, cell: f32) -> Scene {
    let mut scene = scene_plain(name, cell);
    // The liquid's own feel, where asked for (today's Liquid thing is born with Spray 0.4 and
    // Settles 0.1; the scenes here with `Feel::default()`).
    for feel in &mut scene.feels {
        feel.spray = num("WATER_SPRAY", feel.spray);
        feel.settles = num("WATER_SETTLES", feel.settles);
    }
    // The grid moved by a part of a cell (`WATER_SHIFT=0.5`): what depends on where the cells
    // happen to lie shows up as a difference between shifts.
    scene.layout.origin -= Vec2::splat(num("WATER_SHIFT", 0.0) * cell);
    scene
}

fn scene_plain(name: &str, cell: f32) -> Scene {
    match name {
        "box" => scenes::still_box(cell, 0.3),
        "deep" => scenes::still_box(cell, 0.6),
        "course" => scenes::course(cell),
        "weird" => scenes::weird(cell),
        "thin_pour" => scenes::thin_pour(cell, num("WATER_POUR_CELLS", 2.0)),
        "sheet" => scenes::sheet(cell),
        "on_a_disc" => scenes::on_a_disc(cell, num("WATER_PULL", 0.2)),
        other => panic!("no scene called {other}"),
    }
}

/// The air a scene's stream is measured in: a world rectangle.
fn air_of(name: &str) -> Option<(Vec2, Vec2)> {
    match name {
        "thin_pour" => {
            Some((Vec2::new(-250.0, scenes::THIN_POUR.0 + 40.0), Vec2::new(250.0, scenes::THIN_POUR.1 - 80.0)))
        }
        "sheet" => {
            Some((Vec2::new(scenes::SHEET.0 + 4.0, scenes::SHEET.1 - 40.0), Vec2::new(450.0, scenes::SHEET.2 - 80.0)))
        }
        "on_a_disc" => Some((
            Vec2::new(-250.0, scenes::DISC.0.y + scenes::DISC.1 + 40.0),
            Vec2::new(250.0, scenes::THIN_POUR.1 - 80.0),
        )),
        "weird" => Some((Vec2::new(-400.0, 20.0), Vec2::new(400.0, 700.0))),
        _ => None,
    }
}

fn card() -> std::sync::Arc<WaterGpu> {
    let (device, queue) = bpe_gfx::test_device().expect("a graphics device (install mesa-vulkan-drivers)");
    let info = device.adapter_info();
    println!("# card\t{}\t{:?}\t{}", info.name, info.backend, info.driver_info);
    println!("# numbers (the steps a second follow the cell size)\t{:?}", Numbers::from_env());
    WaterGpu::new(&device, &queue)
}

/// Plays the scenes and prints what the drops are doing; saves debug pictures.
#[test]
#[ignore = "a measuring run: by hand"]
fn look() {
    let gpu = card();
    let out = std::env::var("WATER_OUT").ok();
    let tag: String = num("WATER_TAG", String::new());
    println!(
        "# scene\tcell\tseconds\tdrops\trms px/s\tfastest\ttop\tcrowded\tbroken\tin air\tpieces\twhole\tstrays\tgap px\twidths\twall s\tsteps/s\tnearest (cells)\twithin 1/4 cell"
    );
    for name in list::<String>("WATER_SCENES", "weird") {
        for cell in list::<f32>("WATER_CELLS", "8") {
            let scene = scene_named(&name, cell);
            let mut run = scene.start(&gpu, Numbers::from_env_for(cell));
            let began = Instant::now();
            for t in list::<f32>("WATER_TIMES", "1,2,4") {
                run.run_to(t);
                let drops = run.water.read();
                let layout = run.water.layout();
                let s = Stats::of(&drops, &layout);
                let stream = air_of(&name).map(|(lo, hi)| {
                    let air = probe::inside(&drops, &layout, lo, hi);
                    Stream::of(&air, &layout, (lo.y, hi.y), 1.5, 6)
                });
                let st = stream.unwrap_or_default();
                let widths: Vec<String> = st.widths.iter().map(|w| format!("{w:.0}")).collect();
                let spacing = Spacing::of(&drops);
                println!(
                    "{name}\t{cell}\t{t}\t{}\t{:.1}\t{:.0}\t{:.1}\t{}\t{}\t{}\t{}\t{:.3}\t{:.3}\t{:.0}\t{}\t{:.1}\t{}\t{:.3}\t{:.3}",
                    s.drops,
                    s.rms,
                    s.fastest,
                    s.top,
                    s.crowded,
                    s.broken,
                    st.drops,
                    st.pieces,
                    st.whole,
                    st.strays,
                    st.gap,
                    widths.join(" "),
                    began.elapsed().as_secs_f32(),
                    run.water.numbers().rate,
                    spacing.median,
                    spacing.close
                );
                // The Water Weird layout's film over the disc and its thread below: how ragged.
                if name == "weird" {
                    let part = |lo: (f32, f32), hi: (f32, f32)| {
                        let drops = probe::inside(&drops, &layout, Vec2::from(lo), Vec2::from(hi));
                        let (mean, most) = probe::ragged(&drops, &layout, 15.0);
                        format!("{}\t{mean:.2}\t{most:.2}", drops.len())
                    };
                    println!(
                        "ragged\t{cell}\t{t}\t{}\tfilm\t{}\tthread\t{}",
                        run.water.numbers().rate,
                        part((-390.0, -520.0), (-80.0, -235.0)),
                        part((-60.0, -60.0), (330.0, 500.0))
                    );
                }
                // The drops themselves (`WATER_DUMP=<folder>`): 12 numbers a drop as on the card, for
                // a closer look outside the test.
                if let Ok(dir) = std::env::var("WATER_DUMP") {
                    std::fs::create_dir_all(&dir).expect("the dump's folder");
                    let file = format!("{name}-{cell}-{t}{tag}.drops");
                    std::fs::write(std::path::Path::new(&dir).join(file), bytemuck::cast_slice::<_, u8>(&drops))
                        .expect("the dump");
                }
                if let Some(dir) = &out {
                    let tear = bpe_water::Feel::default().gpu(run.water.numbers(), cell).a[2];
                    let (pixels, w, h) = probe::picture(&drops, &run.field, num("WATER_SCALE", 0.5), tear);
                    let file = format!("{name}-{cell}-{t}{tag}.png");
                    probe::save_png(&std::path::Path::new(dir).join(file), &pixels, w, h).expect("a picture");
                }
            }
        }
    }
}

/// The thin pour by height: how many drops, how fast they fall against a free fall, how far they
/// have spread.
#[test]
#[ignore = "a measuring run: by hand"]
fn bands() {
    let gpu = card();
    let cell: f32 = num("WATER_CELL", 8.0);
    let scene = scenes::thin_pour(cell, num("WATER_POUR_CELLS", 2.0));
    let mut run = scene.start(&gpu, Numbers::from_env_for(cell));
    run.run_to(num("WATER_TIME", 1.5));
    let drops = run.water.read();
    let layout = run.water.layout();
    let (top, floor) = scenes::THIN_POUR;
    let step = num("WATER_BAND", 60.0f32);
    println!("# from\tdrops\tfall px/s\tfree fall\tsd fall\tsd across\tmean |vx|\tstretch");
    let mut y = top;
    while y < floor {
        let inside: Vec<_> = drops
            .iter()
            .filter(|d| {
                let w = layout.to_world(Vec2::from(d.x));
                w.y >= y && w.y < y + step
            })
            .collect();
        if !inside.is_empty() {
            let n = inside.len() as f32;
            let vy: Vec<f32> = inside.iter().map(|d| d.v[1] * cell).collect();
            let mean = vy.iter().sum::<f32>() / n;
            let sd = (vy.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / n).sqrt();
            let xs: Vec<f32> = inside.iter().map(|d| layout.to_world(Vec2::from(d.x)).x).collect();
            let mx = xs.iter().sum::<f32>() / n;
            let sx = (xs.iter().map(|x| (x - mx) * (x - mx)).sum::<f32>() / n).sqrt();
            let vx = inside.iter().map(|d| (d.v[0] * cell).abs()).sum::<f32>() / n;
            let e = inside.iter().map(|d| d.e).sum::<f32>() / n;
            let free =
                (scenes::POUR_SPEED * scenes::POUR_SPEED + 2.0 * scenes::GRAVITY.y * (y + step * 0.5 - top)).sqrt();
            println!("{y:.0}\t{}\t{mean:.0}\t{free:.0}\t{sd:.0}\t{sx:.1}\t{vx:.0}\t{e:.2}", inside.len());
        }
        y += step;
    }
}

/// The Water Weird layout's stream off the disc: how wide the falling thread is (the spread of the
/// drops across it in bands of height, a mean over many moments), and how many drops are in it.
/// A steady thread is a few pixels wide; one that leaves in strands or frays is tens.
#[test]
#[ignore = "a measuring run: by hand"]
fn strands() {
    let gpu = card();
    let cell: f32 = num("WATER_CELL", 8.0);
    let mut run = scenes::weird(cell).start(&gpu, Numbers::from_env_for(cell));
    let (lo, hi) = (Vec2::new(-80.0, 60.0), Vec2::new(330.0, 620.0));
    let bands = 8usize;
    let mut spread = vec![0.0f64; bands];
    let mut seen = vec![0u32; bands];
    let mut drops_in = 0usize;
    let mut moments = 0u32;
    let mut t = 1.5f32;
    while t <= num("WATER_UNTIL", 4.0f32) {
        run.run_to(t);
        let drops = run.water.read();
        let layout = run.water.layout();
        let mut by_band: Vec<Vec<f32>> = vec![Vec::new(); bands];
        for d in &drops {
            let w = layout.to_world(Vec2::from(d.x));
            if w.x >= lo.x && w.x < hi.x && w.y >= lo.y && w.y < hi.y {
                let b = (((w.y - lo.y) / (hi.y - lo.y)) * bands as f32) as usize;
                by_band[b.min(bands - 1)].push(w.x);
            }
        }
        for (b, xs) in by_band.iter().enumerate() {
            drops_in += xs.len();
            if xs.len() >= 3 {
                let n = xs.len() as f32;
                let mean = xs.iter().sum::<f32>() / n;
                let sd = (xs.iter().map(|x| (x - mean) * (x - mean)).sum::<f32>() / n).sqrt();
                spread[b] += f64::from(sd);
                seen[b] += 1;
            }
        }
        moments += 1;
        t += 0.1;
    }
    let per: Vec<String> = spread
        .iter()
        .zip(&seen)
        .map(|(s, n)| if *n > 0 { format!("{:.0}", s / f64::from(*n)) } else { "-".into() })
        .collect();
    let all = spread.iter().sum::<f64>() / f64::from(seen.iter().sum::<u32>().max(1));
    println!(
        "strands\tspread px by band\t{}\tmean\t{all:.1}\tdrops a moment\t{:.0}",
        per.join(" "),
        drops_in as f32 / moments as f32
    );
}

/// Where drops by the Water Weird disc are thrown off it: every drop within 3 cells of the disc's
/// upper half whose speed out of the disc jumps in one liquid step.
#[test]
#[ignore = "a measuring run: by hand"]
fn kicks() {
    let gpu = card();
    let cell: f32 = num("WATER_CELL", 6.0);
    let mut run = scenes::weird(cell).start(&gpu, Numbers::from_env_for(cell));
    run.run_to(num("WATER_TIME", 3.0));
    let layout = run.water.layout();
    let (centre, radius) = (Vec2::new(-180.0, -120.0), 120.0);
    let mut before: std::collections::HashMap<u32, (Vec2, Vec2)> = std::collections::HashMap::new();
    println!("# step\tangle\toff px\tout before\tout after\talong before\talong after\tnear drops");
    let mut shown = 0;
    for step in 0..num("WATER_STEPS", 240u32) {
        let drops = run.water.read();
        let mut now = std::collections::HashMap::new();
        let places: Vec<Vec2> = drops.iter().map(|d| layout.to_world(Vec2::from(d.x))).collect();
        let mut events = 0;
        for (d, p) in drops.iter().zip(&places) {
            let v = Vec2::from(d.v) * cell;
            now.insert(d.serial, (*p, v));
            let out = (*p - centre).normalize_or_zero();
            let off = (*p - centre).length() - radius;
            if off > 3.0 * cell || p.y > centre.y {
                continue;
            }
            let Some((_, was)) = before.get(&d.serial) else { continue };
            let (a, b) = (was.dot(out), v.dot(out));
            if b - a > num("WATER_KICK", 120.0) && b > 60.0 {
                events += 1;
                if shown < num("WATER_SHOW", 40) {
                    shown += 1;
                    let near = places.iter().filter(|q| (**q - *p).length() < 1.5 * cell).count();
                    let angle = (p.y - centre.y).det_atan2(p.x - centre.x).to_degrees();
                    let along = out.perp();
                    println!(
                        "{step}\t{angle:.0}\t{off:.1}\t{a:.0}\t{b:.0}\t{:.0}\t{:.0}\t{near}",
                        was.dot(along),
                        v.dot(along)
                    );
                }
            }
        }
        if events > 0 && shown >= num("WATER_SHOW", 40) {
            println!("{step}\t{events} kicks");
        }
        before = now;
        run.step();
    }
}

/// How far drops in the air below the disc move across their neighbours' way: the spread of
/// each drop's speed from the mean of the drops within two cells, across and along that way.
#[test]
#[ignore = "a measuring run: by hand"]
fn across() {
    let gpu = card();
    let cell: f32 = num("WATER_CELL", 6.0);
    let mut run = scenes::weird(cell).start(&gpu, Numbers::from_env_for(cell));
    println!("# time\tdrops\tmean speed\trms along\trms across\tshare > 45 deg");
    for time in list::<f32>("WATER_TIMES", "2,3,4") {
        run.run_to(time);
        let layout = run.water.layout();
        let drops = run.water.read();
        let places: Vec<Vec2> = drops.iter().map(|d| layout.to_world(Vec2::from(d.x))).collect();
        let speeds: Vec<Vec2> = drops.iter().map(|d| Vec2::from(d.v) * cell).collect();
        let (mut n, mut mean_speed, mut along, mut across, mut wide) = (0usize, 0.0f32, 0.0f32, 0.0f32, 0usize);
        for (i, p) in places.iter().enumerate() {
            if p.y < 50.0 || p.y > 750.0 || p.x < -100.0 || p.x > 350.0 {
                continue;
            }
            let mut sum = Vec2::ZERO;
            let mut k = 0;
            for (j, q) in places.iter().enumerate() {
                if (*q - *p).length() < 2.0 * cell {
                    sum += speeds[j];
                    k += 1;
                }
            }
            if k < 2 {
                continue;
            }
            let mean = sum / k as f32;
            let way = mean.normalize_or_zero();
            let off = speeds[i] - mean;
            n += 1;
            mean_speed += mean.length();
            along += off.dot(way).powi(2);
            across += off.dot(way.perp()).powi(2);
            if speeds[i].normalize_or_zero().dot(way) < std::f32::consts::FRAC_1_SQRT_2 {
                wide += 1;
            }
        }
        let m = n.max(1) as f32;
        println!(
            "{time}\t{n}\t{:.0}\t{:.1}\t{:.1}\t{:.3}",
            mean_speed / m,
            (along / m).sqrt(),
            (across / m).sqrt(),
            wide as f32 / m
        );
    }
}

/// How fast the core is: each scene played for `WATER_SECONDS` with no read-back until the end
/// (the drops are read once then, which waits for every step sent). Wall seconds a second of
/// scene, by scene and cell size.
#[test]
#[ignore = "a measuring run: by hand"]
fn pace() {
    let gpu = card();
    let seconds: f32 = num("WATER_SECONDS", 4.0);
    println!("# scene\tcell\tsteps/s\tseconds\tdrops\twall s\twall s a second");
    for name in list::<String>("WATER_SCENES", "weird") {
        for cell in list::<f32>("WATER_CELLS", "8,6,3") {
            let mut run = scene_named(&name, cell).start(&gpu, Numbers::from_env_for(cell));
            let began = Instant::now();
            run.run_to(seconds);
            let drops = run.water.read().len();
            let wall = began.elapsed().as_secs_f32();
            println!(
                "{name}\t{cell}\t{}\t{seconds}\t{drops}\t{wall:.2}\t{:.3}",
                run.water.numbers().rate,
                wall / seconds
            );
        }
    }
}

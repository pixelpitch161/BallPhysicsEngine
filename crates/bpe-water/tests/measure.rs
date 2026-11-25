//! Measuring runs for the builder: ignored, run by hand in release, one at a time,
//!
//! `cargo test --release -p bpe-water --test measure -- --ignored --nocapture --test-threads=1 look`
//!
//! - `WATER_SCENES=weird,thin_pour` picks the scenes (see `scene_named`).
//! - `WATER_CELLS=8,6` the cell sizes in pixels.
//! - `WATER_TIMES=1,2,4` when to measure, seconds.
//! - `WATER_OUT=<folder>` where debug pictures go (none without it).
//! - `BPE_WATER_*` replace the solver's numbers (`Numbers::from_env`).

use std::time::Instant;

use bpe_water::probe::{self, Stats, Stream};
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
        "thin_pour" => Some((Vec2::new(-250.0, scenes::THIN_POUR.0 + 40.0), Vec2::new(250.0, scenes::THIN_POUR.1 - 80.0))),
        "sheet" => Some((Vec2::new(scenes::SHEET.0 + 4.0, scenes::SHEET.1 - 40.0), Vec2::new(450.0, scenes::SHEET.2 - 80.0))),
        "on_a_disc" => Some((Vec2::new(-250.0, scenes::DISC.0.y + scenes::DISC.1 + 40.0), Vec2::new(250.0, scenes::THIN_POUR.1 - 80.0))),
        "weird" => Some((Vec2::new(-400.0, 20.0), Vec2::new(400.0, 700.0))),
        _ => None,
    }
}

fn card() -> std::sync::Arc<WaterGpu> {
    let (device, queue) = bpe_gfx::test_device().expect("a graphics device (install mesa-vulkan-drivers)");
    let info = device.adapter_info();
    println!("# card\t{}\t{:?}\t{}", info.name, info.backend, info.driver_info);
    println!("# numbers\t{:?}", Numbers::from_env());
    WaterGpu::new(&device, &queue)
}

/// Plays the scenes and prints what the drops are doing; saves debug pictures.
#[test]
#[ignore = "a measuring run: by hand"]
fn look() {
    let gpu = card();
    let out = std::env::var("WATER_OUT").ok();
    let tag: String = num("WATER_TAG", String::new());
    println!("# scene\tcell\tseconds\tdrops\trms px/s\tfastest\ttop\tcrowded\tbroken\tin air\tpieces\twhole\tstrays\tgap px\twidths\twall s");
    for name in list::<String>("WATER_SCENES", "weird") {
        for cell in list::<f32>("WATER_CELLS", "8") {
            let scene = scene_named(&name, cell);
            let mut run = scene.start(&gpu, Numbers::from_env());
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
                println!(
                    "{name}\t{cell}\t{t}\t{}\t{:.1}\t{:.0}\t{:.1}\t{}\t{}\t{}\t{}\t{:.3}\t{:.3}\t{:.0}\t{}\t{:.1}",
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
                    began.elapsed().as_secs_f32()
                );
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
    let mut run = scene.start(&gpu, Numbers::from_env());
    run.run_to(num("WATER_TIME", 1.5));
    let drops = run.water.read();
    let layout = run.water.layout();
    let (top, floor) = scenes::THIN_POUR;
    let step = num("WATER_BAND", 60.0f32);
    println!("# from\tdrops\tfall px/s\tfree fall\tsd fall\tsd across\tmean |vx|\tstretch");
    let mut y = top;
    while y < floor {
        let inside: Vec<_> =
            drops.iter().filter(|d| { let w = layout.to_world(Vec2::from(d.x)); w.y >= y && w.y < y + step }).collect();
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
            let free = (scenes::POUR_SPEED * scenes::POUR_SPEED + 2.0 * scenes::GRAVITY.y * (y + step * 0.5 - top)).sqrt();
            println!("{y:.0}\t{}\t{mean:.0}\t{free:.0}\t{sd:.0}\t{sx:.1}\t{vx:.0}\t{e:.2}", inside.len());
        }
        y += step;
    }
}

//! The all-drops engine's shots and measures (`docs/superpowers/plans/water-mode/core-c1-engine.md`,
//! Task 6), the all-drops trial's own on the new engine, so their films and tables line up with the
//! trial's. All are measuring runs, by hand, in release, one at a time:
//!
//! `WATER_OUT=<folder> cargo test --release -p bpe-water --test shots -- --ignored --nocapture --test-threads=1 shots`
//!
//! - `WATER_SHOTS=slope,ledge,pool,course,beads,weird,deep,box,thin` the shots (`scenes::shot`).
//! - `WATER_CELLS=6,3` the layout's cell sizes in pixels (drops are half a cell apart).
//! - `WATER_SECONDS`, `WATER_FPS` (it must divide 60: the water moves on in frames of 1/60 s),
//!   `WATER_STILLS=2,4` (full-size pictures at those seconds), `WATER_TAG` (added to the folder's
//!   name), `WATER_FILM=0` (no film frames, stills only), `WATER_FILM_SCALE=0.5` (a film frame's size
//!   against the screen's), `WATER_DOTS=0` (no pictures of the drops as dots beside the stills).
//! - `WATER_PULL`, `WATER_STICK`, `WATER_THICK`, `WATER_SETTLES` the liquid's feel in every shot;
//!   `WATER_SHIFT` moves the layout by that part of a cell.
//! - `BPE_WATER_*` replace the engine's numbers (`Numbers::from_env_for`).

use std::time::Instant;

use bpe_water::draw::{Plain, PlainGpu, PlainLook, View, sets, stored};
use bpe_water::probe::{self, Stats};
use bpe_water::scenes::{self, Running, Scene};
use bpe_water::{Drop, Layout, Numbers, SolidField, WaterGpu};
use glam::Vec2;

fn list<T: std::str::FromStr>(name: &str, or: &str) -> Vec<T> {
    let asked = std::env::var(name).unwrap_or_else(|_| or.into());
    asked.split(',').filter_map(|s| s.trim().parse().ok()).collect()
}

fn num<T: std::str::FromStr>(name: &str, or: T) -> T {
    std::env::var(name).ok().and_then(|s| s.parse().ok()).unwrap_or(or)
}

fn scene_for(name: &str, cell: f32) -> Scene {
    let mut scene = scenes::shot(name, cell).unwrap_or_else(|| panic!("no shot called {name}"));
    for feel in &mut scene.feels {
        feel.surface_pull = num("WATER_PULL", feel.surface_pull);
        feel.stickiness = num("WATER_STICK", feel.stickiness);
        feel.thickness = num("WATER_THICK", feel.thickness);
        feel.settles = num("WATER_SETTLES", feel.settles);
    }
    scene.layout.origin -= Vec2::splat(num("WATER_SHIFT", 0.0) * cell);
    scene
}

fn card() -> std::sync::Arc<WaterGpu> {
    let (device, queue) = bpe_gfx::test_device().expect("a graphics device (install mesa-vulkan-drivers)");
    let info = device.adapter_info();
    println!("# card\t{}\t{:?}\t{}", info.name, info.backend, info.driver_info);
    WaterGpu::new(&device, &queue)
}

fn start(scene: &Scene, gpu: &std::sync::Arc<WaterGpu>) -> Running {
    scene.start(gpu, Numbers::from_env_for(scene.layout.cell))
}

/// The plain picture of a whole screen (1080 x 1920 world pixels round the middle) at `scale`, drawn
/// on the card straight from the body's drops.
fn screen(plain: &mut Plain, run: &Running, scale: f32) -> (Vec<u8>, u32, u32) {
    let view = View::screen(scale);
    plain.draw(&run.water.drop_buffers(), &view, &PlainLook::default());
    (plain.read(), view.width, view.height)
}

/// The film frames a second: one that divides the 60 frames the water moves on in.
fn fps() -> u32 {
    let fps: u32 = num("WATER_FPS", 30);
    assert!(fps > 0 && 60 % fps == 0, "WATER_FPS must divide 60: {fps}");
    fps
}

/// The shots in the plain drawing, as film frames (numbered as the trial's: frame `i` at `i / fps`
/// seconds) and as full-size stills. `WATER_DUMP=1`: each still's drops also go to a drops file
/// (`<label>-<t>s.drops`, for the plain drawing's `bench_pictures` and the look check).
#[test]
#[ignore = "a measuring run: by hand"]
fn shots() {
    let gpu = card();
    let out = std::path::PathBuf::from(std::env::var("WATER_OUT").expect("WATER_OUT, a folder"));
    let (seconds, fps): (f32, u32) = (num("WATER_SECONDS", 4.0), fps());
    let stills = list::<f32>("WATER_STILLS", "");
    let tag: String = num("WATER_TAG", String::new());
    let film = num("WATER_FILM", 1u32) != 0;
    let dots = num("WATER_DOTS", 1u32) != 0;
    let half: f32 = num("WATER_FILM_SCALE", 0.5);
    let dump = num("WATER_DUMP", 0u32) != 0;
    let drawing = PlainGpu::new(gpu.device(), gpu.queue());
    println!("# shot\tcell\tseconds\tdrops\trms px/s\tfastest\ttop\tbroken\tsteps last frame\twall s");
    for name in list::<String>("WATER_SHOTS", "slope") {
        for cell in list::<f32>("WATER_CELLS", "6") {
            let scene = scene_for(&name, cell);
            let mut run = start(&scene, &gpu);
            // (Drawn as a Water course is made, carved in rock: no light comes in through a thin
            // wall from outside the box.)
            let least = bpe_water::drops::LEAST_WALL * cell;
            let carved = SolidField::build_least(scene.layout, &sets::carved(&scene.solids, least), least);
            let mut plain = Plain::new(drawing.clone(), &carved, run.water.numbers().wall_off);
            let label = format!("{name}-c{cell}{}{tag}", if tag.is_empty() { "" } else { "-" });
            let dir = out.join(&label);
            std::fs::create_dir_all(&dir).expect("the frames' folder");
            let mut wall = 0.0f64;
            let frames = (seconds * fps as f32).round() as u32;
            let mut due = stills.clone();
            for i in 0..=frames {
                let t = i as f32 / fps as f32;
                let still = due.first().is_some_and(|s| t + 1e-4 >= *s);
                let whole = i % fps == 0;
                let at = Instant::now();
                run.run_to(t);
                run.water.wait();
                wall += at.elapsed().as_secs_f64();
                if !(film || still || whole) {
                    continue;
                }
                let drops = if whole || still { run.water.read() } else { Vec::new() };
                if whole || still {
                    let s = Stats::of(&drops, &run.water.layout());
                    println!(
                        "{name}\t{cell}\t{t:.2}\t{}\t{:.1}\t{:.0}\t{:.1}\t{}\t{}\t{wall:.1}",
                        s.drops,
                        s.rms,
                        s.fastest,
                        s.top,
                        s.broken,
                        run.water.steps_last_frame()
                    );
                }
                if film {
                    let (pixels, w, h) = screen(&mut plain, &run, half);
                    probe::save_png(&dir.join(format!("f{i:05}.png")), &pixels, w, h).expect("a frame");
                }
                if still {
                    let (pixels, w, h) = screen(&mut plain, &run, 1.0);
                    probe::save_png(&out.join(format!("{label}-drops-{t:.1}s.png")), &pixels, w, h).expect("a still");
                    if dump {
                        stored::save_drops(&out.join(format!("{label}-{t:.1}s.drops")), &drops).expect("the drops");
                    }
                    if dots {
                        let (pixels, w, h) = probe::picture(&drops, &run.field, 1.0, f32::MAX);
                        probe::save_png(&out.join(format!("{label}-drops-{t:.1}s-dots.png")), &pixels, w, h)
                            .expect("the dots");
                    }
                    due.remove(0);
                }
            }
            println!("# {label}\t{} frames\tphysics alone: {wall:.1} s", frames + 1);
        }
    }
}

/// The engine's speed: wall seconds for a second of a shot, physics alone, with nothing read back
/// in between (`WATER_FROM` the scene second timing starts at, `WATER_SECONDS` how long it is
/// timed for), with the mean steps a frame and the time a step.
#[test]
#[ignore = "a measuring run: by hand"]
fn pace() {
    let gpu = card();
    let (from, seconds): (f32, f32) = (num("WATER_FROM", 1.0), num("WATER_SECONDS", 1.0));
    println!("# shot\tcell\tdrops\tsteps a frame\tms a step\twall s for a scene second");
    for name in list::<String>("WATER_SHOTS", "weird") {
        for cell in list::<f32>("WATER_CELLS", "6") {
            let scene = scene_for(&name, cell);
            let mut run = start(&scene, &gpu);
            run.run_to(from);
            run.water.wait();
            let (frames, steps) = (run.water.frames(), run.water.steps());
            let at = Instant::now();
            run.run_to(from + seconds);
            run.water.wait();
            let took = at.elapsed().as_secs_f64();
            let (frames, steps) = (run.water.frames() - frames, run.water.steps() - steps);
            let per_frame = steps as f64 / f64::from(frames.max(1));
            let per_step = 1000.0 * took / (steps.max(1) as f64);
            let wall = took / f64::from(seconds);
            println!("{name}\t{cell}\t{}\t{per_frame:.1}\t{per_step:.3}\t{wall:.3}", run.water.count());
        }
    }
}

/// The liquid's middle across the screen (world x).
fn middle_x(drops: &[Drop], layout: &Layout) -> f32 {
    let sum: f64 = drops.iter().map(|d| f64::from(layout.to_world(Vec2::from(d.x)).x)).sum();
    (sum / drops.len().max(1) as f64) as f32
}

/// The deep pool at rest (movement left, height kept) and a slosh (how its swings die).
#[test]
#[ignore = "a measuring run: by hand"]
fn pools() {
    let gpu = card();
    let rest: f32 = num("WATER_REST", 20.0);
    println!("# cell\tseconds\tdrops\trms px/s\tfastest\theight kept %\twall s");
    for cell in list::<f32>("WATER_CELLS", "6") {
        let scene = scene_for("deep", cell);
        let floor = 450.0 - 0.5 * cell.max(3.0);
        let mut run = start(&scene, &gpu);
        let layout = run.water.layout();
        let top0 = Stats::of(&run.water.read(), &layout).top;
        let began = Instant::now();
        for t in [0.25 * rest, 0.5 * rest, 0.75 * rest, rest] {
            run.run_to(t);
            let s = Stats::of(&run.water.read(), &layout);
            println!(
                "{cell}\t{t}\t{}\t{:.2}\t{:.1}\t{:.2}\t{:.0}",
                s.drops,
                s.rms,
                s.fastest,
                100.0 * (floor - s.top) / (floor - top0),
                began.elapsed().as_secs_f64()
            );
        }
        // The slosh: the gravity leans for a moment, then the swings of the liquid's middle.
        let lean = scenes::GRAVITY + Vec2::new(0.4 * scenes::GRAVITY.y, 0.0);
        run.water.set_gravity(lean);
        run.run_to(rest + 0.3);
        run.water.set_gravity(scenes::GRAVITY);
        let mut xs = Vec::new();
        let mut t = rest + 0.3;
        while t < rest + 0.3 + num("WATER_SLOSH", 6.0f32) {
            t += 1.0 / 30.0;
            run.run_to(t);
            xs.push(middle_x(&run.water.read(), &layout));
        }
        // The turning points of the middle's swing, as distances from where it ends up.
        let home = xs[xs.len() - 15..].iter().sum::<f32>() / 15.0;
        let mut turns = Vec::new();
        for k in 1..xs.len() - 1 {
            let (a, b, c) = (xs[k - 1] - home, xs[k] - home, xs[k + 1] - home);
            if (b - a) * (c - b) < 0.0 && b.abs() > 0.05 {
                turns.push(b);
            }
        }
        let swings: Vec<String> = turns.iter().take(10).map(|a| format!("{a:.1}")).collect();
        let left = if turns.len() > 4 { 100.0 * turns[4].abs() / turns[0].abs() } else { 0.0 };
        println!("# slosh\t{cell}\tswings px: {}\tleft after four half swings: {left:.0} %", swings.join(" "));
    }
}

/// The course's numbers: where its water is after `WATER_SECONDS`, in shares of all drops.
#[test]
#[ignore = "a measuring run: by hand"]
fn openings() {
    let gpu = card();
    let seconds: f32 = num("WATER_SECONDS", 30.0);
    let (top, bottom, basins) = scenes::COURSE_LINES;
    println!(
        "# cell\tseconds\tdrops\ttop pool %\ton the way %\tbottom pool %\tthrough 12 px %\tthrough 24 px %\tout %\twall s"
    );
    for cell in list::<f32>("WATER_CELLS", "6") {
        let scene = scene_for("course", cell);
        let mut run = start(&scene, &gpu);
        let layout = run.water.layout();
        let began = Instant::now();
        let mut t = 0.0;
        while t < seconds {
            t = (t + 5.0).min(seconds);
            run.run_to(t);
            let drops = run.water.read();
            let mut n = [0usize; 6];
            for d in &drops {
                let p = layout.to_world(Vec2::from(d.x));
                let k = if !p.is_finite() || p.x.abs() > 450.0 || p.y.abs() > 900.0 {
                    5
                } else if p.y < top {
                    0
                } else if p.y < 330.0 {
                    1
                } else if p.y < bottom + 15.0 {
                    2
                } else if p.x < basins {
                    3
                } else {
                    4
                };
                n[k] += 1;
            }
            let share: Vec<String> =
                n.iter().map(|k| format!("{:.1}", 100.0 * *k as f32 / drops.len().max(1) as f32)).collect();
            println!("{cell}\t{t}\t{}\t{}\t{:.0}", drops.len(), share.join("\t"), began.elapsed().as_secs_f64());
        }
    }
}

/// Whether a run repeats on this card: twice from the start, and from a copy made half way.
#[test]
#[ignore = "a measuring run: by hand"]
fn repeats() {
    let gpu = card();
    let seconds: f32 = num("WATER_SECONDS", 2.0);
    println!("# shot\tcell\tdrops\tfirst run\tsecond run\tfrom a copy at half time\trepeats");
    for name in list::<String>("WATER_SHOTS", "weird") {
        for cell in list::<f32>("WATER_CELLS", "6,3") {
            let scene = scene_for(&name, cell);
            let mut a = start(&scene, &gpu);
            a.run_to(seconds);
            let first = a.water.read();
            let mut b = start(&scene, &gpu);
            b.run_to(0.5 * seconds);
            let mut c = b.copy();
            b.run_to(seconds);
            c.run_to(seconds);
            let (pa, pb, pc) =
                (probe::print_of(&first), probe::print_of(&b.water.read()), probe::print_of(&c.water.read()));
            println!(
                "{name}\t{cell}\t{}\t{pa:016x}\t{pb:016x}\t{pc:016x}\t{}",
                first.len(),
                if pa == pb && pb == pc { "yes" } else { "NO" }
            );
        }
    }
}

/// How a hard landing breaks up (part C2, Task 5): the weird shot's stream landing on its block, in
/// pieces (`probe::pieces`: drops within 1.6 spacings are one piece, under 16 px across is small),
/// every 0.25 s, round the block and over everything above the floor's pool. `WATER_PULL=0.2,0.5`,
/// `WATER_CELLS=8,6,3`; `WATER_DOTS=<folder>` also saves a dot picture of the block's surroundings
/// every frame from 0.2 to 0.8 s.
#[test]
#[ignore = "a measuring run: by hand"]
fn landing() {
    let gpu = card();
    let seconds: f32 = num("WATER_SECONDS", 3.0);
    let block = (Vec2::new(-80.0, -420.0), Vec2::new(320.0, -120.0));
    let above_pool = (Vec2::new(-420.0, -840.0), Vec2::new(420.0, 600.0));
    let dots = std::env::var("WATER_DOTS").ok().map(std::path::PathBuf::from);
    println!("# pull\tcell\tseconds\tdrops\tblock: pieces\tsmall %\tbiggest\tabove the pool: pieces\tsmall %\tbiggest");
    for pull in list::<f32>("WATER_PULL", "0.2,0.5") {
        for cell in list::<f32>("WATER_CELLS", "8,6,3") {
            let mut scene = scenes::weird(cell);
            for feel in &mut scene.feels {
                feel.surface_pull = pull;
            }
            let mut run = start(&scene, &gpu);
            let layout = run.water.layout();
            let mut next = 0.25;
            while run.water.frames() < (seconds * 60.0).round() as u32 {
                run.frame();
                let t = run.water.frames() as f32 / 60.0;
                if let Some(dir) = &dots
                    && (0.2..=0.8).contains(&t)
                {
                    let drops = run.water.read();
                    let (pixels, w, _) = probe::picture(&drops, &run.field, 1.0, f32::MAX);
                    let (x0, y0) = ((block.0 - layout.origin).x as u32, (block.0 - layout.origin).y as u32);
                    let (cw, ch) = ((block.1 - block.0).x as u32, (block.1 - block.0).y as u32);
                    let mut crop = Vec::with_capacity((cw * ch * 4) as usize);
                    for y in y0..y0 + ch {
                        crop.extend_from_slice(&pixels[((y * w + x0) * 4) as usize..((y * w + x0 + cw) * 4) as usize]);
                    }
                    let folder = dir.join(format!("landing-pull{pull}-{cell}px"));
                    std::fs::create_dir_all(&folder).expect("a folder");
                    let name = format!("f{:03}.png", run.water.frames());
                    probe::save_png(&folder.join(name), &crop, cw, ch).expect("a picture");
                }
                if t + 1e-4 >= next {
                    next += 0.25;
                    let drops = run.water.read();
                    let a = probe::pieces(&drops, &layout, 1.6, 16.0, block);
                    let b = probe::pieces(&drops, &layout, 1.6, 16.0, above_pool);
                    println!(
                        "{pull}\t{cell}\t{t:.2}\t{}\t{}\t{:.1}\t{}\t{}\t{:.1}\t{}",
                        drops.len(),
                        a.pieces,
                        100.0 * a.small_share,
                        a.biggest,
                        b.pieces,
                        100.0 * b.small_share,
                        b.biggest
                    );
                }
            }
        }
    }
}

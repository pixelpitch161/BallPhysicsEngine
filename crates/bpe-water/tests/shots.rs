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
//!   `WATER_WETTING` every solid's Wetting; `WATER_SHIFT` moves the layout by that part of a cell.
//! - `BPE_WATER_*` replace the engine's numbers (`Numbers::from_env_for`).

use std::time::Instant;

use bpe_core::maths::Maths;
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
    scene.wetting = num("WATER_WETTING", scene.wetting);
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

/// The course's top shelf as it drains (part C2, Task 7): every 10 s up to `WATER_SECONDS` (30),
/// the share of the drops left on it and the height of its water every 10 px along it (the
/// topmost drop joined to the shelf's face, each within a cell of the one under it, counting only
/// a column with at least 3 drops), the tallest and the mean, and the share of the drops above the
/// shelf but not joined to it (drops left hanging from the box's lid, part 2c: on wet rock they
/// stay, as under a real lid). `WATER_STICK` sets the Stickiness (run it at 0 and at 0.5),
/// `WATER_WETTING` the rock's Wetting (part 2c).
#[test]
#[ignore = "a measuring run: by hand"]
fn shelves() {
    let gpu = card();
    let seconds: f32 = num("WATER_SECONDS", 30.0);
    let (shelf, _, _) = scenes::COURSE_LINES;
    println!(
        "# cell\tstickiness\twetting\tseconds\tleft on the shelf %\ttallest px\tmean px\tcolumns\thanging above %\twall s"
    );
    for cell in list::<f32>("WATER_CELLS", "6") {
        let scene = scene_for("course", cell);
        let (stick, wetting) = (scene.feels[0].stickiness, scene.wetting);
        let mut run = start(&scene, &gpu);
        let layout = run.water.layout();
        let began = Instant::now();
        let mut t = 0.0;
        while t < seconds {
            t = (t + 10.0).min(seconds);
            run.run_to(t);
            let drops = run.water.read();
            let liquid = probe::liquid(&drops);
            let mut columns: Vec<Vec<f32>> = vec![Vec::new(); 90];
            let mut above = 0usize;
            for d in liquid.iter() {
                let p = layout.to_world(Vec2::from(d.x));
                if !p.is_finite() || p.y >= shelf || p.x.abs() >= 450.0 {
                    continue;
                }
                above += 1;
                columns[((p.x + 450.0) / 10.0) as usize].push(p.y);
            }
            // (Each column's water up from the shelf, drop by drop, while each is within a cell of
            // the one under it.)
            let (mut on, mut heights) = (0usize, Vec::new());
            for ys in &mut columns {
                ys.sort_by(|a, b| b.total_cmp(a));
                let (mut top, mut joined) = (shelf, 0u32);
                for &y in ys.iter() {
                    if top - y > cell {
                        break;
                    }
                    (top, joined) = (y, joined + 1);
                }
                on += joined as usize;
                if joined >= 3 {
                    heights.push(shelf - top);
                }
            }
            let tallest = heights.iter().copied().fold(0.0f32, f32::max);
            let mean = heights.iter().sum::<f32>() / heights.len().max(1) as f32;
            let share = |n: usize| 100.0 * n as f32 / liquid.len().max(1) as f32;
            println!(
                "{cell}\t{stick}\t{wetting}\t{t}\t{:.1}\t{tallest:.1}\t{mean:.1}\t{}\t{:.1}\t{:.0}",
                share(on),
                heights.len(),
                share(above - on),
                began.elapsed().as_secs_f64()
            );
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
/// every frame from 0.2 to 0.8 s, and one of the whole course at the end.
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
                    if let Some(dir) = &dots
                        && t + 0.25 > seconds
                    {
                        let (pixels, w, h) = probe::picture(&drops, &run.field, 0.5, f32::MAX);
                        let name = format!("whole-pull{pull}-{cell}px-{t:.2}s.png");
                        probe::save_png(&dir.join(name), &pixels, w, h).expect("a picture");
                    }
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

/// Trapped air (part 2c, Task 4): in the pool shot (a thick pour plunging into a pool), every
/// 0.25 s from 1 s to `WATER_SECONDS` (6), the air drops, the trapped room the liquid leaves
/// (`probe::pockets`: its area, the biggest piece and its middle's height) and the wall seconds,
/// then the mean and the most room over the run. `BPE_WATER_AIR=0` gives C2's water beside it.
#[test]
#[ignore = "a measuring run: by hand"]
fn air() {
    let gpu = card();
    let seconds: f32 = num("WATER_SECONDS", 6.0);
    println!("# shot\tcell\ttrapped air\tseconds\tair drops\tpockets px2\tbiggest px2\tits middle y\tliquid\twall s");
    for name in list::<String>("WATER_SHOTS", "pool") {
        for cell in list::<f32>("WATER_CELLS", "6,3,2") {
            let scene = scene_for(&name, cell);
            let mut run = start(&scene, &gpu);
            let on = Numbers::from_env_for(cell).air;
            let began = Instant::now();
            let (mut sum, mut most, mut n) = (0.0f32, 0.0f32, 0u32);
            let mut k = 4;
            while 0.25 * k as f32 <= seconds + 1e-3 {
                let t = 0.25 * k as f32;
                run.run_to(t);
                let drops = run.water.read();
                let p = probe::pockets(&drops, &run.field);
                println!(
                    "{name}\t{cell}\t{on}\t{t}\t{}\t{:.0}\t{:.0}\t{:.1}\t{}\t{:.0}",
                    probe::air(&drops).len(),
                    p.area,
                    p.biggest,
                    p.middle.y,
                    probe::liquid(&drops).len(),
                    began.elapsed().as_secs_f64()
                );
                sum += p.area;
                most = most.max(p.area);
                n += 1;
                k += 1;
            }
            println!("# {name}\t{cell}\ttrapped air {on}\tpockets: mean {:.0} px2, most {most:.0} px2", sum / n as f32);
        }
    }
}

/// Wetting (part 2c, Task 6), at each of `WATER_WETTINGS` (0, 0.25, 0.5, 0.75, 1) and
/// `WATER_CELLS` (6, 3) after `WATER_SECONDS` (3):
/// - a 12, 24 and 48 px blob (`WATER_BLOBS`) on a level floor: width, height (`probe::puddle`) and
///   the angle its edge makes with the floor (a circle's cap: `2 atan(2h / w)`);
/// - a 36 px cap under a tenth of the gravity (`scenes::cap`, by `WATER_SECONDS` or 2 s if sooner):
///   its edge's own angle, which its weight does not flatten (part 3, item 0);
/// - the `beads` shot's tilted plate after `WATER_PLATE_SECONDS` (1): the share of the drops set on
///   it still on it, how far down it they are on average (pixels along it from where they started),
///   and the share of those on it that lie in a film (no more than two spacings deep where they
///   are).
#[test]
#[ignore = "a measuring run: by hand"]
fn wetting() {
    let gpu = card();
    let seconds: f32 = num("WATER_SECONDS", 3.0);
    println!("# cell\twetting\tblob px\twidth px\theight px\tangle deg\twall s");
    println!("# cell\twetting\tplate\ton it %\tdown it px\tfilm %\twall s");
    for cell in list::<f32>("WATER_CELLS", "6,3") {
        let spacing = 0.5 * cell;
        for wetting in list::<f32>("WATER_WETTINGS", "0,0.25,0.5,0.75,1") {
            for side in list::<f32>("WATER_BLOBS", "12,24,48") {
                let began = Instant::now();
                let mut run = start(&scenes::blob(cell, side, wetting), &gpu);
                run.run_to(seconds);
                let (floor, half) = scenes::BLOB;
                let inside = (-half + 2.0 * cell, half - 2.0 * cell);
                let (w, h) = probe::puddle(&run.water.read(), &run.water.layout(), floor, inside);
                let angle = 2.0 * (2.0 * h / w).det_atan().to_degrees();
                println!("{cell}\t{wetting}\t{side}\t{w:.1}\t{h:.1}\t{angle:.0}\t{:.0}", began.elapsed().as_secs_f64());
            }
            // A 36 px cap under a tenth of the gravity: the edge's own angle (part 3, item 0).
            let began = Instant::now();
            let mut run = start(&scenes::cap(cell, 36.0, wetting), &gpu);
            run.run_to(seconds.min(2.0));
            let (floor, half) = scenes::BLOB;
            let inside = (-half + 2.0 * cell, half - 2.0 * cell);
            let (w, h) = probe::puddle(&run.water.read(), &run.water.layout(), floor, inside);
            let angle = 2.0 * (2.0 * h / w).det_atan().to_degrees();
            println!("{cell}\t{wetting}\tcap 36\t{w:.1}\t{h:.1}\t{angle:.0}\t{:.0}", began.elapsed().as_secs_f64());
            // The tilted plate of the beads shot (scenes::beads: from a to b, its face on that line).
            let began = Instant::now();
            let mut run = start(&scenes::blobs(cell, wetting), &gpu);
            let layout = run.water.layout();
            let (a, b) = (Vec2::new(-380.0, -200.0), Vec2::new(380.0, -38.0));
            let u = (b - a).normalize();
            let up = Vec2::new(u.y, -u.x);
            let first: std::collections::HashMap<u32, f32> = run
                .water
                .read()
                .iter()
                .filter_map(|d| {
                    let p = layout.to_world(Vec2::from(d.x));
                    (p.y < 100.0).then_some((d.serial, (p - a).dot(u)))
                })
                .collect();
            run.run_to(num("WATER_PLATE_SECONDS", 1.0));
            let on: Vec<(f32, f32, f32)> = probe::liquid(&run.water.read())
                .iter()
                .filter_map(|d| {
                    let from = *first.get(&d.serial)?;
                    let p = layout.to_world(Vec2::from(d.x));
                    let (along, above) = ((p - a).dot(u), (p - a).dot(up));
                    (p.is_finite() && (0.0..=(b - a).length()).contains(&along) && (-spacing..80.0).contains(&above))
                        .then_some((along, above, along - from))
                })
                .collect();
            let bin = |along: f32| (along / (2.0 * spacing)) as usize;
            let mut deepest = std::collections::HashMap::<usize, f32>::new();
            for &(along, above, _) in &on {
                let e = deepest.entry(bin(along)).or_insert(f32::MIN);
                *e = e.max(above);
            }
            let film = on.iter().filter(|&&(along, _, _)| deepest[&bin(along)] <= 2.0 * spacing).count();
            let left = probe::liquid(&run.water.read()).iter().filter(|d| first.contains_key(&d.serial)).count();
            eprintln!("# plate: {} drops set on it, {left} still in the course, {} on it", first.len(), on.len());
            println!(
                "{cell}\t{wetting}\tplate\t{:.1}\t{:.1}\t{:.1}\t{:.0}",
                100.0 * on.len() as f32 / first.len().max(1) as f32,
                on.iter().map(|o| o.2).sum::<f32>() / on.len().max(1) as f32,
                100.0 * film as f32 / on.len().max(1) as f32,
                began.elapsed().as_secs_f64()
            );
        }
    }
}

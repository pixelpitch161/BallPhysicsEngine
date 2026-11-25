//! The all-drops engine trial (`docs/superpowers/plans/water-mode/all-drops-trial.md`).
//!
//! Two small tests run with the others. The rest are measuring runs, by hand, in release, one at
//! a time:
//!
//! `WATER_OUT=<folder> cargo test --release -p bpe-water --test alldrops -- --ignored --nocapture --test-threads=1 shots`
//!
//! - `WATER_ENGINES=grid,drops` which engines play (the merged grid core, the all-drops trial).
//! - `WATER_SHOTS=slope,ledge,pool,course,beads,weird,deep,box,thin` the shots (`trial::shot`).
//! - `WATER_CELLS=6,3` the layout's cell sizes in pixels (drops are half a cell apart).
//! - `WATER_SECONDS`, `WATER_FPS`, `WATER_STILLS=2,4` (full-size pictures at those seconds),
//!   `WATER_TAG` (added to the folder's name), `WATER_FILM=0` (no film frames, stills only),
//!   `WATER_FILM_SCALE=0.5` (a film frame's size against the screen's), `WATER_DOTS=0` (no pictures
//!   of the drops as dots beside the stills).
//! - `WATER_PULL`, `WATER_STICK`, `WATER_SPRAY`, `WATER_SETTLES` the liquid's feel in every shot;
//!   `WATER_SHIFT` moves the layout by that part of a cell.
//! - `BPE_WATER_*` replace the grid core's numbers, `BPE_DROPS_*` the all-drops engine's.

use std::time::Instant;

use bpe_water::plain::{self, Plain};
use bpe_water::probe::{self, Stats};
use bpe_water::scenes::{self, Scene};
use bpe_water::trial::{self, Cards, Engine, Play};
use bpe_water::{Drop, Layout};
use glam::Vec2;

fn list<T: std::str::FromStr>(name: &str, or: &str) -> Vec<T> {
    let asked = std::env::var(name).unwrap_or_else(|_| or.into());
    asked.split(',').filter_map(|s| s.trim().parse().ok()).collect()
}

fn num<T: std::str::FromStr>(name: &str, or: T) -> T {
    std::env::var(name).ok().and_then(|s| s.parse().ok()).unwrap_or(or)
}

fn engines() -> Vec<Engine> {
    list::<String>("WATER_ENGINES", "grid,drops").iter().filter_map(|n| Engine::named(n)).collect()
}

fn scene_for(name: &str, cell: f32) -> Scene {
    let mut scene = trial::shot(name, cell).unwrap_or_else(|| panic!("no shot called {name}"));
    for feel in &mut scene.feels {
        feel.surface_pull = num("WATER_PULL", feel.surface_pull);
        feel.stickiness = num("WATER_STICK", feel.stickiness);
        feel.spray = num("WATER_SPRAY", feel.spray);
        feel.settles = num("WATER_SETTLES", feel.settles);
    }
    scene.layout.origin -= Vec2::splat(num("WATER_SHIFT", 0.0) * cell);
    scene
}

fn cards() -> Cards {
    let (device, queue) = bpe_gfx::test_device().expect("a graphics device (install mesa-vulkan-drivers)");
    let info = device.adapter_info();
    println!("# card\t{}\t{:?}\t{}", info.name, info.backend, info.driver_info);
    Cards::new(&device, &queue)
}

/// How far off a face an engine's drops rest, in cells.
fn rest_of(engine: Engine) -> f32 {
    match engine {
        Engine::Grid => 0.5,
        Engine::Drops => num("BPE_DROPS_WALL_OFF", 0.25),
    }
}

/// The plain picture of a whole screen (1080 x 1920 world pixels round the middle) at `scale`.
fn screen(drops: &[Drop], play: &Play, scale: f32) -> (Vec<u8>, u32, u32) {
    let (w, h) = ((1080.0 * scale) as u32, (1920.0 * scale) as u32);
    let ask = Plain { lo: Vec2::new(-540.0, -960.0), scale, width: w, height: h, rest: rest_of(play.body.engine()) };
    (plain::draw(drops, &play.field, &ask), w, h)
}

/// Both engines play the shots; the plain drawing of each, side by side (in `WATER_ENGINES`'
/// order), as film frames and as full-size stills.
#[test]
#[ignore = "a measuring run: by hand"]
fn shots() {
    let cards = cards();
    let out = std::path::PathBuf::from(std::env::var("WATER_OUT").expect("WATER_OUT, a folder"));
    let (seconds, fps): (f32, f32) = (num("WATER_SECONDS", 4.0), num("WATER_FPS", 30.0));
    let stills = list::<f32>("WATER_STILLS", "");
    let tag: String = num("WATER_TAG", String::new());
    let film = num("WATER_FILM", 1u32) != 0;
    let dots = num("WATER_DOTS", 1u32) != 0;
    let half: f32 = num("WATER_FILM_SCALE", 0.5);
    println!("# shot\tcell\tengine\tseconds\tdrops\trms px/s\tfastest\ttop\tbroken\twall s");
    for name in list::<String>("WATER_SHOTS", "slope") {
        for cell in list::<f32>("WATER_CELLS", "6") {
            let scene = scene_for(&name, cell);
            let mut plays: Vec<Play> = engines().iter().map(|e| Play::start(&scene, &cards, *e)).collect();
            let label = format!("{name}-c{cell}{}{tag}", if tag.is_empty() { "" } else { "-" });
            let dir = out.join(&label);
            std::fs::create_dir_all(&dir).expect("the frames' folder");
            let mut wall = vec![0.0f64; plays.len()];
            let frames = (seconds * fps).round() as u32;
            let mut due = stills.clone();
            for i in 0..=frames {
                let t = i as f32 / fps;
                let still = due.first().is_some_and(|s| t + 1e-4 >= *s);
                let whole = ((t * fps).round() as u32).is_multiple_of((fps.round() as u32).max(1));
                let mut halves = Vec::new();
                let mut size = (0, 0);
                for (k, play) in plays.iter_mut().enumerate() {
                    let at = Instant::now();
                    play.run_to(t);
                    play.body.wait();
                    wall[k] += at.elapsed().as_secs_f64();
                    if !(film || still || whole) {
                        continue;
                    }
                    let drops = play.body.read();
                    let engine = play.body.engine().name();
                    if whole || still {
                        let s = Stats::of(&drops, &play.body.layout());
                        println!(
                            "{name}\t{cell}\t{engine}\t{t:.2}\t{}\t{:.1}\t{:.0}\t{:.1}\t{}\t{:.1}",
                            s.drops, s.rms, s.fastest, s.top, s.broken, wall[k]
                        );
                    }
                    if film {
                        let (pixels, w, h) = screen(&drops, play, half);
                        size = (w, h);
                        halves.push(pixels);
                    }
                    if still {
                        let (pixels, w, h) = screen(&drops, play, 1.0);
                        probe::save_png(&out.join(format!("{label}-{engine}-{t:.1}s.png")), &pixels, w, h)
                            .expect("a still");
                        if dots {
                            let (pixels, w, h) = probe::picture(&drops, &play.field, 1.0, f32::MAX);
                            probe::save_png(&out.join(format!("{label}-{engine}-{t:.1}s-dots.png")), &pixels, w, h)
                                .expect("the dots");
                        }
                    }
                }
                if still {
                    due.remove(0);
                }
                if film {
                    let (joined, w, h) = plain::beside(&halves, size.0, size.1);
                    probe::save_png(&dir.join(format!("f{i:05}.png")), &joined, w, h).expect("a frame");
                }
            }
            let names: Vec<String> =
                plays.iter().zip(&wall).map(|(p, w)| format!("{} {w:.1} s", p.body.engine().name())).collect();
            println!("# {label}\t{} frames\tphysics alone: {}", frames + 1, names.join(", "));
        }
    }
}

/// The engines' speed: wall seconds for a second of a shot, with nothing read back in between
/// (`WATER_FROM` the scene second timing starts at, `WATER_SECONDS` how long it is timed for).
#[test]
#[ignore = "a measuring run: by hand"]
fn pace() {
    let cards = cards();
    let (from, seconds): (f32, f32) = (num("WATER_FROM", 1.0), num("WATER_SECONDS", 1.0));
    println!("# shot\tcell\tengine\tdrops\tsteps a second\twall s for a scene second");
    for name in list::<String>("WATER_SHOTS", "weird") {
        for cell in list::<f32>("WATER_CELLS", "6") {
            let scene = scene_for(&name, cell);
            for engine in engines() {
                let mut play = Play::start(&scene, &cards, engine);
                play.run_to(from);
                play.body.wait();
                let at = Instant::now();
                play.run_to(from + seconds);
                play.body.wait();
                let wall = at.elapsed().as_secs_f64() / f64::from(seconds);
                println!("{name}\t{cell}\t{}\t{}\t{}\t{wall:.3}", engine.name(), play.body.count(), play.body.rate());
            }
        }
    }
}

/// The liquid's middle across the screen (world x).
fn middle_x(drops: &[Drop], layout: &Layout) -> f32 {
    let sum: f64 = drops.iter().map(|d| f64::from(layout.to_world(Vec2::from(d.x)).x)).sum();
    (sum / drops.len().max(1) as f64) as f32
}

/// Shot 7: the deep pool at rest (movement left, height kept) and a slosh (how its swings die).
#[test]
#[ignore = "a measuring run: by hand"]
fn pools() {
    let cards = cards();
    let rest: f32 = num("WATER_REST", 20.0);
    println!("# engine\tcell\tseconds\tdrops\trms px/s\tfastest\theight kept %\twall s");
    for cell in list::<f32>("WATER_CELLS", "6") {
        let scene = scene_for("deep", cell);
        let floor = 450.0 - 0.5 * cell.max(3.0);
        for engine in engines() {
            let mut play = Play::start(&scene, &cards, engine);
            let layout = play.body.layout();
            let top0 = Stats::of(&play.body.read(), &layout).top;
            let began = Instant::now();
            for t in [0.25 * rest, 0.5 * rest, rest] {
                play.run_to(t);
                let s = Stats::of(&play.body.read(), &layout);
                println!(
                    "{}\t{cell}\t{t}\t{}\t{:.2}\t{:.1}\t{:.2}\t{:.0}",
                    engine.name(),
                    s.drops,
                    s.rms,
                    s.fastest,
                    100.0 * (floor - s.top) / (floor - top0),
                    began.elapsed().as_secs_f64()
                );
            }
            // The slosh: the gravity leans for a moment, then the swings of the liquid's middle.
            let lean = scenes::GRAVITY + Vec2::new(0.4 * scenes::GRAVITY.y, 0.0);
            play.body.set_gravity(lean);
            play.run_to(rest + 0.3);
            play.body.set_gravity(scenes::GRAVITY);
            let mut xs = Vec::new();
            let mut t = rest + 0.3;
            while t < rest + 0.3 + num("WATER_SLOSH", 6.0f32) {
                t += 1.0 / 30.0;
                play.run_to(t);
                xs.push(middle_x(&play.body.read(), &layout));
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
            println!(
                "# slosh\t{}\t{cell}\tswings px: {}\tleft after four half swings: {left:.0} %",
                engine.name(),
                swings.join(" ")
            );
        }
    }
}

/// Shot 4's numbers: where the course's water is after `WATER_SECONDS`, in shares of all drops.
#[test]
#[ignore = "a measuring run: by hand"]
fn openings() {
    let cards = cards();
    let seconds: f32 = num("WATER_SECONDS", 30.0);
    let (top, bottom, basins) = scenes::COURSE_LINES;
    println!(
        "# engine\tcell\tseconds\tdrops\ttop pool %\ton the way %\tbottom pool %\tthrough 12 px %\tthrough 24 px %\tout %\twall s"
    );
    for cell in list::<f32>("WATER_CELLS", "6") {
        let scene = scene_for("course", cell);
        for engine in engines() {
            let mut play = Play::start(&scene, &cards, engine);
            let layout = play.body.layout();
            let began = Instant::now();
            let mut t = 0.0;
            while t < seconds {
                t = (t + 5.0).min(seconds);
                play.run_to(t);
                let drops = play.body.read();
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
                println!(
                    "{}\t{cell}\t{t}\t{}\t{}\t{:.0}",
                    engine.name(),
                    drops.len(),
                    share.join("\t"),
                    began.elapsed().as_secs_f64()
                );
            }
        }
    }
}

/// Whether a run repeats on this card: twice from the start, and from a copy made half way.
#[test]
#[ignore = "a measuring run: by hand"]
fn repeats() {
    let cards = cards();
    let seconds: f32 = num("WATER_SECONDS", 2.0);
    println!("# shot\tcell\tengine\tdrops\tfirst run\tsecond run\tfrom a copy at half time\trepeats");
    for name in list::<String>("WATER_SHOTS", "weird") {
        for cell in list::<f32>("WATER_CELLS", "6") {
            let scene = scene_for(&name, cell);
            for engine in engines() {
                let mut a = Play::start(&scene, &cards, engine);
                a.run_to(seconds);
                let first = a.body.read();
                let mut b = Play::start(&scene, &cards, engine);
                b.run_to(0.5 * seconds);
                let mut c = b.copy();
                b.run_to(seconds);
                c.run_to(seconds);
                let (pa, pb, pc) =
                    (probe::print_of(&first), probe::print_of(&b.body.read()), probe::print_of(&c.body.read()));
                println!(
                    "{name}\t{cell}\t{}\t{}\t{pa:016x}\t{pb:016x}\t{pc:016x}\t{}",
                    engine.name(),
                    first.len(),
                    if pa == pb && pb == pc { "yes" } else { "NO" }
                );
            }
        }
    }
}

fn small_cards() -> Option<Cards> {
    let (device, queue) = bpe_gfx::test_device().ok()?;
    Some(Cards::new(&device, &queue))
}

/// The all-drops engine holds a pool: every drop stays in its box, the pool keeps its height and
/// comes to rest.
#[test]
fn the_all_drops_pool_stays_in_its_box_and_keeps_its_height() {
    let Some(cards) = small_cards() else { return };
    let scene = scenes::still_box(12.0, 0.3);
    let mut play = Play::start(&scene, &cards, Engine::Drops);
    let layout = play.body.layout();
    let before = Stats::of(&play.body.read(), &layout);
    assert!(before.drops > 2000, "{before:?}");
    play.run_to(1.0);
    let drops = play.body.read();
    let after = Stats::of(&drops, &layout);
    assert_eq!(after.broken, 0);
    for d in &drops {
        let p = layout.to_world(Vec2::from(d.x));
        assert!(p.x.abs() < 300.0 && p.y.abs() < 450.0, "a drop left the box: {p}");
    }
    let floor = 450.0 - 6.0;
    let kept = (floor - after.top) / (floor - before.top);
    assert!((0.93..1.05).contains(&kept), "the pool keeps its height: {kept}");
    assert!(after.rms < 60.0, "and comes to rest: {} px/s", after.rms);
}

/// A run repeats, and a copy made half way plays on the same.
#[test]
fn the_all_drops_engine_repeats() {
    let Some(cards) = small_cards() else { return };
    let scene = scenes::weird(12.0);
    let mut a = Play::start(&scene, &cards, Engine::Drops);
    a.run_to(0.5);
    let mut b = Play::start(&scene, &cards, Engine::Drops);
    b.run_to(0.25);
    let mut c = b.copy();
    b.run_to(0.5);
    c.run_to(0.5);
    let first = a.body.read();
    assert!(first.len() > 100, "{}", first.len());
    assert_eq!(probe::print_of(&first), probe::print_of(&b.body.read()), "twice from the start");
    assert_eq!(probe::print_of(&first), probe::print_of(&c.body.read()), "from a copy made half way");
}

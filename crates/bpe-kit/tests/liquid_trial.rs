//! The Water mode's physics trial (throwaway, never merged): today's liquid core against the trial
//! core, which solves the pressure. Plan: `docs/superpowers/plans/water-mode/physics-trial.md`.
//! Everything here is ignored: run by hand, in release, one test at a time,
//!
//! `cargo test --release -p bpe-kit --test liquid_trial -- --ignored --nocapture --test-threads=1 still`
//!
//! - `TRIAL_CORES=old,new` picks the cores (the default is both).
//! - `TRIAL_DETAILS=normal,fine,extreme` picks the Detail levels (the default is normal).
//! - `TRIAL_SECONDS=5` is how long a timing plays.
//! - `TRIAL_SCENE=<a saved scene>` adds that scene to the timings and the frames.
//! - `BPE_TRIAL_*` set the trial core's numbers (`bpe_gfx::liquid_sim::Trial::from_env`).

use std::time::Instant;

use bpe_core::{Registry, Scene, Settings, ThingId, resolve};
use bpe_gfx::liquid_sim::{Drop, Trial, set_trial};
use bpe_gfx::swarm::{SwarmGpu, use_for_now};
use bpe_gfx::{Frame, Renderer, build_frame};
use bpe_kit::things::liquid::state::host_of;
use bpe_kit::things::liquid::{Detail, FillWith, LiquidSettings};
use bpe_kit::things::shape::{ShapeFill, ShapeSettings, ShapeStart};
use bpe_kit::{kit, scenes};
use bpe_runtime::{Log, Playback, World};
use glam::Vec2;

fn cores() -> Vec<bool> {
    let asked = std::env::var("TRIAL_CORES").unwrap_or_else(|_| "old,new".into());
    asked.split(',').map(|c| c.trim() == "new").collect()
}

fn name_of(new: bool) -> &'static str {
    if new { "trial" } else { "today" }
}

fn details() -> Vec<Detail> {
    let asked = std::env::var("TRIAL_DETAILS").unwrap_or_else(|_| "normal".into());
    asked
        .split(',')
        .map(|d| match d.trim() {
            "rough" => Detail::Rough,
            "normal" => Detail::Normal,
            "fine" => Detail::Fine,
            "very_fine" => Detail::VeryFine,
            "extreme" => Detail::Extreme,
            other => panic!("no Detail called {other}"),
        })
        .collect()
}

fn liquid_of(scene: &Scene) -> ThingId {
    scene.order.iter().copied().find(|t| scene.things[t].kind == "liquid").expect("a liquid")
}

fn at_detail(mut scene: Scene, detail: Detail) -> Scene {
    let id = liquid_of(&scene);
    let t = scene.things.get_mut(&id).expect("the liquid");
    let mut s = LiquidSettings::from_params(&t.params);
    s.detail = detail;
    s.export_detail = detail;
    t.params = s.to_params();
    scene
}

/// A world of `scene` on core `new`.
fn build(scene: &Scene, new: bool) -> World {
    set_trial(if new { Trial::from_env() } else { Trial::OFF });
    let kit = kit();
    let resolved = resolve(scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    World::build(scene, &resolved, &kit.sims)
}

fn drops(world: &World) -> Vec<Drop> {
    host_of(world).and_then(|s| s.host.as_ref()).expect("a host").body.as_ref().expect("a card").read()
}

fn cell(world: &World) -> f32 {
    host_of(world).and_then(|s| s.host.as_ref()).expect("a host").layout.cell
}

fn place(world: &World, d: &Drop) -> Vec2 {
    host_of(world).and_then(|s| s.host.as_ref()).expect("a host").layout.to_world(Vec2::from(d.x))
}

/// How many drops are on exactly the same spot as another.
fn stacked(all: &[Drop]) -> usize {
    let mut at: std::collections::HashMap<(u32, u32), usize> = Default::default();
    for d in all {
        *at.entry((d.x[0].to_bits(), d.x[1].to_bits())).or_default() += 1;
    }
    at.values().filter(|n| **n > 1).sum()
}

/// How many drops are within a tenth of a cell of another (clumps the picture would show).
fn crowded(all: &[Drop]) -> usize {
    let mut at: std::collections::HashMap<(i32, i32), usize> = Default::default();
    for d in all {
        *at.entry(((d.x[0] * 10.0) as i32, (d.x[1] * 10.0) as i32)).or_default() += 1;
    }
    at.values().filter(|n| **n > 1).sum()
}

/// The liquid's top (world y is down): the height above which 2 % of the drops are, in pixels.
fn top_of(world: &World) -> f32 {
    let mut ys: Vec<f32> = drops(world).iter().map(|d| place(world, d).y).collect();
    ys.sort_by(f32::total_cmp);
    ys[ys.len() / 50]
}

/// Mean drops in a cell, over the cells with a cell full of liquid on every side (4 at rest).
fn packing(all: &[Drop]) -> f32 {
    let mut at: std::collections::HashMap<(i32, i32), u32> = Default::default();
    for d in all {
        *at.entry((d.x[0].floor() as i32, d.x[1].floor() as i32)).or_default() += 1;
    }
    let inner: Vec<u32> = at
        .iter()
        .filter(|((x, y), _)| (-2..=2).all(|a| (-2..=2).all(|b| at.get(&(x + a, y + b)).is_some_and(|n| *n >= 1))))
        .map(|(_, n)| *n)
        .collect();
    inner.iter().sum::<u32>() as f32 / inner.len().max(1) as f32
}

fn card() -> impl Sized {
    let (device, queue) = bpe_gfx::test_device().expect("a graphics device (install mesa-vulkan-drivers)");
    let info = device.adapter_info();
    println!("# card\t{}\t{:?}\t{}", info.name, info.backend, info.driver_info);
    println!("# trial numbers\t{:?}", Trial::from_env());
    use_for_now(Some(SwarmGpu::for_device(&device, &queue)))
}

/// Still water in boxes whose walls fall at different places between the grid's nodes.
#[test]
#[ignore = "the physics trial: run it by hand"]
fn still() {
    let _card = card();
    let kit = kit();
    let reg = &kit.registry;
    println!(
        "# core\tdetail\tbox\twall\tdrops\trms px/s\tfastest px/s\tthrew px\tsank px\tstacked\tcrowded\tdrops a cell"
    );
    for detail in details() {
        for (width, wall) in [(594.0, 3.0), (604.0, 3.0), (600.0, 36.0), (600.0, 40.0)] {
            for new in cores() {
                let (mut scene, shape) = scenes::liquid_box(reg);
                scene.things.get_mut(&shape).expect("the box").params =
                    bpe_kit::ShapeSettings::rectangle(width, 900.0, wall).to_params();
                scenes::add_liquid(&mut scene, reg, shape, |f| f.level = 0.3, |_| {});
                let mut world = build(&at_detail(scene, detail), new);
                let highest = |world: &World| drops(world).iter().map(|d| place(world, d).y).fold(f32::MAX, f32::min);
                let level = highest(&world);
                let top = top_of(&world);
                let mut threw = level;
                for quarter in 1..=20 {
                    world.advance_to(quarter as f64 * 0.25, &kit.sims, &mut Log::default());
                    threw = threw.min(highest(&world));
                }
                let all = drops(&world);
                let px = cell(&world);
                let v: Vec<f32> = all.iter().map(|d| (Vec2::from(d.v) * px).length()).collect();
                let rms = (v.iter().map(|s| s * s).sum::<f32>() / v.len() as f32).sqrt();
                let fastest = v.iter().copied().fold(0.0, f32::max);
                println!(
                    "{}\t{detail:?}\t{width}\t{wall}\t{}\t{rms:.1}\t{fastest:.1}\t{:.0}\t{:.1}\t{}\t{}\t{:.2}",
                    name_of(new),
                    all.len(),
                    level - threw,
                    top_of(&world) - top,
                    stacked(&all),
                    crowded(&all),
                    packing(&all),
                );
            }
        }
    }
}

/// A slosh in the box: pulled sideways for 0.4 s, then left alone.
#[test]
#[ignore = "the physics trial: run it by hand"]
fn slosh() {
    let _card = card();
    let kit = kit();
    let reg = &kit.registry;
    for detail in details() {
        for new in cores() {
            let (mut scene, shape) = scenes::liquid_box(reg);
            scenes::add_liquid(&mut scene, reg, shape, |f| f.level = 0.3, |_| {});
            let mut world = build(&at_detail(scene, detail), new);
            let down = world.gravity;
            let mut swing = Vec::new();
            for k in 1..=90 {
                let t = k as f64 * 0.05;
                world.gravity = if t <= 0.4 { down + Vec2::new(250.0, 0.0) } else { down };
                world.advance_to(t, &kit.sims, &mut Log::default());
                let all = drops(&world);
                let x = all.iter().map(|d| place(&world, d).x).sum::<f32>() / all.len().max(1) as f32;
                swing.push((t as f32, x));
            }
            let mut turns = Vec::new();
            for w in swing.windows(3) {
                let (a, (t, b), c) = (w[0].1, w[1], w[2].1);
                if t > 0.45 && b.abs() > 0.8 && ((b > a && b >= c) || (b < a && b <= c)) {
                    turns.push((t, b));
                }
            }
            let half = if turns.len() >= 4 { (turns[3].0 - turns[0].0) / 3.0 } else { f32::NAN };
            let kept: Vec<String> =
                turns.windows(2).take(5).map(|p| format!("{:.2}", (p[1].1 / p[0].1).abs())).collect();
            println!(
                "{}\t{detail:?}\tturns {}\thalf swing {half:.2} s (theory 0.73)\tkept {}\t{turns:?}",
                name_of(new),
                turns.len(),
                kept.join(" ")
            );
        }
    }
}

/// A deep still pool over 20 s: does it keep its volume?
#[test]
#[ignore = "the physics trial: run it by hand"]
fn volume() {
    let _card = card();
    let kit = kit();
    let reg = &kit.registry;
    println!("# core\tdetail\tseconds\tdrops\ttop px\tdrops a cell\trms px/s\tcrowded");
    for detail in details() {
        for new in cores() {
            let (mut scene, shape) = scenes::liquid_box(reg);
            scenes::add_liquid(&mut scene, reg, shape, |f| f.level = 0.6, |_| {});
            let mut world = build(&at_detail(scene, detail), new);
            let times = std::env::var("TRIAL_TIMES").unwrap_or_else(|_| "0,1,5,10,20".into());
            for t in times.split(',').map(|t| t.trim().parse::<f64>().expect("a time")) {
                world.advance_to(t, &kit.sims, &mut Log::default());
                let all = drops(&world);
                let px = cell(&world);
                let rms = (all.iter().map(|d| (Vec2::from(d.v) * px).length_squared()).sum::<f32>() / all.len() as f32)
                    .sqrt();
                println!(
                    "{}\t{detail:?}\t{t}\t{}\t{:.1}\t{:.2}\t{rms:.1}\t{}",
                    name_of(new),
                    all.len(),
                    top_of(&world),
                    packing(&all),
                    crowded(&all)
                );
            }
        }
    }
}

fn timed_scenes(reg: &Registry) -> Vec<(String, Scene)> {
    let mut list: Vec<(String, Scene)> = Vec::new();
    if let Ok(path) = std::env::var("TRIAL_SCENE") {
        let text = std::fs::read_to_string(&path).expect("the scene file");
        list.push(("file".into(), bpe_core::load(&text, reg).expect("a scene")));
    }
    let (mut filled, shape) = scenes::liquid_box(reg);
    scenes::add_liquid(&mut filled, reg, shape, |f| f.level = 0.6, |_| {});
    list.push(("box".into(), filled));
    list.push(("pour".into(), scenes::liquid_pour_on_a_shape(reg)));
    list.push(("course".into(), course(reg)));
    list
}

/// A filled block with corners `points` (world pixels, clockwise on the screen).
fn block(scene: &mut Scene, reg: &Registry, points: &[(f32, f32)]) {
    let points: Vec<Vec2> = points.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
    let centre = points.iter().copied().sum::<Vec2>() / points.len() as f32;
    let far = points.iter().map(|p| (*p - centre).length()).fold(1.0, f32::max);
    let size = far.log2().ceil().exp2();
    let path = bpe_core::ShapePath {
        points: points.iter().map(|p| ((*p - centre) / size).to_array()).collect(),
        bends: vec![None; points.len()],
    };
    let id = bpe_kit::add_thing(scene, reg, "shape", ThingId::ROOT).expect("a shape");
    let t = scene.things.get_mut(&id).expect("just added");
    t.params = ShapeSettings {
        fill: ShapeFill::Filled,
        start: ShapeStart::Polygon,
        sides: points.len() as u32,
        size,
        thickness: 3.0,
        path,
        ..ShapeSettings::default()
    }
    .to_params();
    t.transform.position = centre;
}

fn slab(scene: &mut Scene, reg: &Registry, (x0, y0): (f32, f32), (x1, y1): (f32, f32)) {
    block(scene, reg, &[(x0, y0), (x1, y0), (x1, y1), (x0, y1)]);
}

/// The course's lines (world pixels, y down): the top pool's floor, the bottom pool's floor, and
/// the wall between the two catch basins under it.
const TOP_FLOOR: f32 = -640.0;
const BOTTOM_FLOOR: f32 = 640.0;
const BASINS: f32 = -100.0;

/// The course: a pool at the top that drains through a 48-pixel opening in its floor onto a
/// slope, off the slope's end into pegs, down a funnel into a corridor 36 pixels wide, into a
/// bottom pool whose floor has a 12-pixel and a 24-pixel opening, each over its own catch basin.
/// (At Fine a cell is 6 pixels, so the openings are 8, 6, 2 and 4 cells; at Normal 6, 4.5, 1.5
/// and 3; at Extreme 16, 12, 4 and 8.) Everything inside the outer box is a filled block, so a
/// wall is never thinner than a few cells.
fn course(reg: &Registry) -> Scene {
    let (mut scene, shape) = scenes::liquid_box(reg);
    scene.things.get_mut(&shape).expect("the box").params = ShapeSettings::rectangle(900.0, 1800.0, 6.0).to_params();
    let s = &mut scene;
    // The top pool's floor, its opening 48 wide round x = -300.
    slab(s, reg, (-450.0, TOP_FLOOR), (-324.0, TOP_FLOOR + 30.0));
    slab(s, reg, (-276.0, TOP_FLOOR), (450.0, TOP_FLOOR + 30.0));
    // The slope under the opening, down to the right.
    block(s, reg, &[(-450.0, -500.0), (200.0, -380.0), (200.0, -356.0), (-450.0, -476.0)]);
    // Pegs under the slope's end.
    for (row, y) in [-260.0f32, -190.0, -120.0].into_iter().enumerate() {
        let xs: &[f32] = if row % 2 == 0 { &[110.0, 210.0, 310.0, 410.0] } else { &[160.0, 260.0, 360.0] };
        for &x in xs {
            let id = bpe_kit::add_thing(s, reg, "shape", ThingId::ROOT).expect("a shape");
            let t = s.things.get_mut(&id).expect("just added");
            t.params = ShapeSettings { fill: ShapeFill::Filled, ..ShapeSettings::circle(16.0, 3.0) }.to_params();
            t.transform.position = Vec2::new(x, y);
        }
    }
    // The funnel and its corridor, 36 wide round x = 170.
    block(s, reg, &[(-450.0, -60.0), (152.0, 150.0), (152.0, 330.0), (-450.0, 330.0)]);
    block(s, reg, &[(188.0, 150.0), (450.0, 60.0), (450.0, 330.0), (188.0, 330.0)]);
    // The bottom pool's floor: a 12 wide opening round x = -220 and a 24 wide one round x = 220.
    slab(s, reg, (-450.0, BOTTOM_FLOOR), (-226.0, BOTTOM_FLOOR + 30.0));
    slab(s, reg, (-214.0, BOTTOM_FLOOR), (208.0, BOTTOM_FLOOR + 30.0));
    slab(s, reg, (232.0, BOTTOM_FLOOR), (450.0, BOTTOM_FLOOR + 30.0));
    // The wall between the catch basins.
    slab(s, reg, (BASINS - 8.0, BOTTOM_FLOOR + 30.0), (BASINS + 8.0, 900.0));
    // The top pool, 870 x 220 of water: a Fill made of a plain blue picture.
    let picture = std::env::temp_dir().join("bpe-trial-pool.png");
    save(&picture, 87, 22, &[47u8, 127, 224, 255].repeat(87 * 22));
    let id = scenes::add_liquid(
        s,
        reg,
        ThingId::ROOT,
        |f| {
            f.fill_with = FillWith::APicture;
            f.picture = picture.to_string_lossy().into_owned();
            f.picture_width = 870.0;
        },
        |_| {},
    );
    s.things.get_mut(&id).expect("just added").transform.position = Vec2::new(0.0, -762.0);
    scene
}

fn save(path: &std::path::Path, w: u32, h: u32, pixels: &[u8]) {
    let file = std::fs::File::create(path).expect("a picture file");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().expect("a header").write_image_data(pixels).expect("the pixels");
}

fn print_of(all: &[Drop]) -> u64 {
    all.iter()
        .flat_map(|d| d.x.iter().chain(&d.v).map(|f| f.to_bits()))
        .fold(0xcbf2_9ce4_8422_2325u64, |h, w| (h ^ u64::from(w)).wrapping_mul(0x0000_0100_0000_01b3))
}

/// The course: where the liquid is as it runs down, and how much has come through each opening.
#[test]
#[ignore = "the physics trial: run it by hand"]
fn course_run() {
    let _card = card();
    let kit = kit();
    let reg = &kit.registry;
    let times = std::env::var("TRIAL_TIMES").unwrap_or_else(|_| "2,5,10,15,20,30".into());
    println!(
        "# core\tdetail\tseconds\tdrops\ttop pool\ton the way\tbottom pool\tthrough 12 px\tthrough 24 px\toutside\tfastest px/s\tcrowded\twall s"
    );
    for detail in details() {
        for new in cores() {
            let mut world = build(&at_detail(course(reg), detail), new);
            let start = Instant::now();
            for t in times.split(',').map(|t| t.trim().parse::<f64>().expect("a time")) {
                world.advance_to(t, &kit.sims, &mut Log::default());
                let all = drops(&world);
                let px = cell(&world);
                let at: Vec<Vec2> = all.iter().map(|d| place(&world, d)).collect();
                let count = |f: &dyn Fn(Vec2) -> bool| at.iter().filter(|p| f(**p)).count();
                let inside = |p: Vec2| p.x.abs() < 450.0 && p.y.abs() < 900.0;
                let fastest = all.iter().map(|d| (Vec2::from(d.v) * px).length()).fold(0.0, f32::max);
                println!(
                    "{}\t{detail:?}\t{t}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{fastest:.0}\t{}\t{:.1}",
                    name_of(new),
                    all.len(),
                    count(&|p| inside(p) && p.y < TOP_FLOOR),
                    count(&|p| inside(p) && p.y >= TOP_FLOOR && p.y < 330.0),
                    count(&|p| inside(p) && p.y >= 330.0 && p.y < BOTTOM_FLOOR + 15.0),
                    count(&|p| inside(p) && p.y >= BOTTOM_FLOOR + 15.0 && p.x < BASINS),
                    count(&|p| inside(p) && p.y >= BOTTOM_FLOOR + 15.0 && p.x >= BASINS),
                    count(&|p| !inside(p)),
                    crowded(&all),
                    start.elapsed().as_secs_f64(),
                );
            }
        }
    }
}

/// Does the trial core repeat exactly on one machine: two runs, and a run that seeks back and
/// plays again, against a straight run.
#[test]
#[ignore = "the physics trial: run it by hand"]
fn repeats() {
    let _card = card();
    let kit = kit();
    let reg = &kit.registry;
    let seconds: f64 = std::env::var("TRIAL_SECONDS").ok().and_then(|s| s.parse().ok()).unwrap_or(2.0);
    for (name, scene) in [("pour", scenes::liquid_pour_on_a_shape(reg)), ("course", course(reg))] {
        for new in cores() {
            let scene = at_detail(scene.clone(), details()[0]);
            let straight = |_: u32| {
                let mut world = build(&scene, new);
                world.advance_to(seconds, &kit.sims, &mut Log::default());
                print_of(&drops(&world))
            };
            let (a, b) = (straight(0), straight(1));
            set_trial(if new { Trial::from_env() } else { Trial::OFF });
            let resolved = resolve(&scene, &kit.registry);
            let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
            playback.seek(seconds);
            let first = print_of(&drops(playback.world()));
            playback.seek(seconds * 0.5);
            playback.seek(seconds);
            let again = print_of(&drops(playback.world()));
            println!(
                "{name}\t{}\ttwo runs {}\tplayed {}\tsought back and played again {}\t{a:016x} {b:016x} {first:016x} {again:016x}",
                name_of(new),
                if a == b { "the same" } else { "DIFFER" },
                if first == a { "the same" } else { "DIFFERS" },
                if again == a { "the same" } else { "DIFFERS" },
            );
        }
    }
}

/// Frames of one scene on each core, for the side-by-side clips: `TRIAL_OUT/<scene>-<detail>-<core>/f00000.png`
/// and on. `TRIAL_SHOT=course` (or `file` with `TRIAL_SCENE`), `TRIAL_SECONDS`, `TRIAL_FPS` (30),
/// `TRIAL_SIZE=540x960`.
#[test]
#[ignore = "the physics trial: run it by hand"]
fn frames() {
    let (device, queue) = bpe_gfx::test_device().expect("a graphics device (install mesa-vulkan-drivers)");
    let _card = use_for_now(Some(SwarmGpu::for_device(&device, &queue)));
    let kit = kit();
    let reg = &kit.registry;
    let out = std::path::PathBuf::from(std::env::var("TRIAL_OUT").expect("TRIAL_OUT, a folder"));
    let shot = std::env::var("TRIAL_SHOT").unwrap_or_else(|_| "course".into());
    let seconds: f64 = std::env::var("TRIAL_SECONDS").ok().and_then(|s| s.parse().ok()).unwrap_or(5.0);
    let fps: f64 = std::env::var("TRIAL_FPS").ok().and_then(|s| s.parse().ok()).unwrap_or(30.0);
    let size = std::env::var("TRIAL_SIZE").unwrap_or_else(|_| "540x960".into());
    let (w, h) = size.split_once('x').expect("a size like 540x960");
    let (w, h): (u32, u32) = (w.parse().expect("a width"), h.parse().expect("a height"));
    let scene = timed_scenes(reg).into_iter().find(|(n, _)| *n == shot).expect("a scene of that name").1;
    let mut renderer = Renderer::new(&device, w, h);
    for detail in details() {
        for new in cores() {
            let scene = at_detail(scene.clone(), detail);
            set_trial(if new { Trial::from_env() } else { Trial::OFF });
            let resolved = resolve(&scene, &kit.registry);
            let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
            let dir = out.join(format!("{shot}-{detail:?}-{}", name_of(new)).to_lowercase());
            std::fs::create_dir_all(&dir).expect("the frames' folder");
            let start = Instant::now();
            let count = (seconds * fps).round() as u32;
            for i in 0..=count {
                let t = i as f64 / fps;
                playback.seek(t);
                let mut frame = Frame::default();
                build_frame(&scene, &resolved, playback.world(), playback.log(), t, &kit.draws, &mut frame);
                renderer.render(&device, &queue, &frame);
                while renderer.programs_waiting() {
                    renderer.render(&device, &queue, &frame);
                }
                let pixels = renderer.read_output(&device, &queue).expect("the picture");
                save(&dir.join(format!("f{i:05}.png")), w, h, &pixels);
            }
            println!("{}\t{} frames\t{:.0} s", dir.display(), count + 1, start.elapsed().as_secs_f64());
        }
    }
}

/// Seconds of work for a second of liquid, both cores.
#[test]
#[ignore = "the physics trial: run it by hand"]
fn speed() {
    let _card = card();
    let kit = kit();
    let reg = &kit.registry;
    let seconds: f64 = std::env::var("TRIAL_SECONDS").ok().and_then(|s| s.parse().ok()).unwrap_or(5.0);
    let only = std::env::var("TRIAL_ONLY").ok();
    println!("# scene\tdetail\tcore\tgrid\tdrops\tseconds\twall s\twall s a second\tprint\tmean speed² px");
    for (name, scene) in timed_scenes(reg) {
        if only.as_ref().is_some_and(|o| !o.split(',').any(|n| n.trim() == name)) {
            continue;
        }
        for detail in details() {
            for new in cores() {
                let mut world = build(&at_detail(scene.clone(), detail), new);
                let start = Instant::now();
                world.advance_to(seconds, &kit.sims, &mut Log::default());
                let all = drops(&world);
                let wall = start.elapsed().as_secs_f64();
                let print = print_of(&all);
                let px = cell(&world);
                let speed =
                    all.iter().map(|d| (Vec2::from(d.v) * px).length_squared()).sum::<f32>() / all.len().max(1) as f32;
                let host = host_of(&world).and_then(|s| s.host.as_ref()).expect("a host");
                println!(
                    "{name}\t{detail:?}\t{}\t{}x{}\t{}\t{seconds}\t{wall:.2}\t{:.2}\t{print:016x}\t{speed:.0}",
                    name_of(new),
                    host.layout.cols,
                    host.layout.rows,
                    all.len(),
                    wall / seconds,
                );
            }
        }
    }
}

/// A look inside the trial core: the pressure and the speeds down the middle of a deep still box.
#[test]
#[ignore = "the physics trial: run it by hand"]
fn probe() {
    let _card = card();
    let kit = kit();
    let reg = &kit.registry;
    let level: f32 = std::env::var("PROBE_LEVEL").ok().and_then(|s| s.parse().ok()).unwrap_or(0.6);
    let (mut scene, shape) = scenes::liquid_box(reg);
    scenes::add_liquid(&mut scene, reg, shape, |f| f.level = level, |_| {});
    let mut world = build(&at_detail(scene, details()[0]), true);
    println!("gravity {:?}", world.gravity);
    for t in [1.0 / 240.0, 2.0 / 240.0, 5.0 / 240.0, 0.1, 0.25, 0.5, 1.0] {
        world.advance_to(t, &kit.sims, &mut Log::default());
        let host = host_of(&world).and_then(|s| s.host.as_ref()).expect("a host");
        let body = host.body.as_ref().expect("a body");
        let (cols, rows) = (host.layout.cols as usize, host.layout.rows as usize);
        let p = body.read_trial(0);
        let v = body.read_trial(1);
        let x = cols / 2;
        println!("t {t:.4}: row: P want diag solved | u v liquid bits");
        for y in (0..rows).step_by(4) {
            let (a, b) = (p[y * cols + x], v[y * cols + x]);
            if b[2] > 0.0 || a[3] > 0.0 {
                println!(
                    "  {y}: {:.3} {:.3} {:.1} {} | {:.3} {:.3} {:.3} {}",
                    a[0], a[1], a[2], a[3], b[0], b[1], b[2], b[3]
                );
            }
        }
        let c = body.read_trial(2);
        println!("  near the floor: row: open l r t b | P want diag solved | v liquid");
        for y in rows.saturating_sub(75)..rows.saturating_sub(62) {
            let (a, b, o) = (p[y * cols + x], v[y * cols + x], c[y * cols + x]);
            println!("  {y}: {o:?} | {:.3} {:.3} {:.1} {} | {:.3} {:.3}", a[0], a[1], a[2], a[3], b[1], b[2]);
        }
        let y = rows - 80;
        println!("  across row {y}: col: open l r t b | P want diag solved | u v liquid bits");
        for x in (26..38).chain(101..113) {
            let (a, b, o) = (p[y * cols + x], v[y * cols + x], c[y * cols + x]);
            println!(
                "  {x}: {o:?} | {:.3} {:.3} {:.1} {} | {:.3} {:.3} {:.3} {}",
                a[0], a[1], a[2], a[3], b[0], b[1], b[2], b[3]
            );
        }
        let all = drops(&world);
        let (lo, hi) = all.iter().fold((f32::MAX, f32::MIN), |(lo, hi), d| (lo.min(d.x[0]), hi.max(d.x[0])));
        println!("  drops from x {lo:.2} to {hi:.2}");
        println!("  top {:.1}", top_of(&world));
    }
}

/// How well the trial core's pressure is solved in a deep still box: what the cells still lack
/// after a step's sweeps, against what they were asked for.
#[test]
#[ignore = "the physics trial: run it by hand"]
fn solve() {
    let _card = card();
    let kit = kit();
    let reg = &kit.registry;
    let level: f32 = std::env::var("PROBE_LEVEL").ok().and_then(|s| s.parse().ok()).unwrap_or(0.6);
    for detail in details() {
        let (mut scene, shape) = scenes::liquid_box(reg);
        scenes::add_liquid(&mut scene, reg, shape, |f| f.level = level, |_| {});
        let mut world = build(&at_detail(scene, detail), true);
        println!("# {detail:?}\tstep\tasked\tlacks\tlacks most\tmost P\ttop px\trms px/s");
        for step in [1, 2, 3, 5, 8, 12, 24, 48, 120, 240] {
            world.advance_to(step as f64 / 240.0, &kit.sims, &mut Log::default());
            let host = host_of(&world).and_then(|s| s.host.as_ref()).expect("a host");
            let body = host.body.as_ref().expect("a body");
            let (cols, rows) = (host.layout.cols as usize, host.layout.rows as usize);
            let p = body.read_trial(0);
            let c = body.read_trial(2);
            let (mut asked, mut lacks, mut most, mut most_p) = (0.0f64, 0.0f64, 0.0f32, 0.0f32);
            for y in 1..rows - 1 {
                for x in 1..cols - 1 {
                    let k = y * cols + x;
                    let (a, o) = (p[k], c[k]);
                    if a[3] < 0.5 {
                        continue;
                    }
                    let r =
                        a[1] + o[0] * p[k - 1][0] + o[1] * p[k + 1][0] + o[2] * p[k - cols][0] + o[3] * p[k + cols][0]
                            - a[2] * a[0];
                    let r = if a[0] <= 0.0 { r.max(0.0) } else { r.abs() };
                    asked += a[1].abs() as f64;
                    lacks += r as f64;
                    most = most.max(r);
                    most_p = most_p.max(a[0]);
                }
            }
            let all = drops(&world);
            let px = cell(&world);
            let rms =
                (all.iter().map(|d| (Vec2::from(d.v) * px).length_squared()).sum::<f32>() / all.len() as f32).sqrt();
            println!(
                "{detail:?}\t{step}\t{asked:.2}\t{lacks:.3}\t{most:.4}\t{most_p:.3}\t{:.1}\t{rms:.1}",
                top_of(&world)
            );
            if std::env::var("SOLVE_LEVELS").ok().and_then(|s| s.parse::<i32>().ok()) == Some(step) {
                let mut levels = vec![(0usize, cols, rows)];
                while levels.len() < 12 {
                    let (at, c, r) = levels[levels.len() - 1];
                    if c.min(r) <= 4 {
                        break;
                    }
                    levels.push((at + c * r, c.div_ceil(2), r.div_ceil(2)));
                }
                for (n, &(at, c, r)) in levels.iter().enumerate().skip(levels.len().saturating_sub(2)) {
                    println!("level {n}: {c} x {r}: answer / lacks / diag / solved, then faces l r t b");
                    for y in 0..r {
                        let row: Vec<String> = (0..c)
                            .map(|x| {
                                let a = p[at + y * c + x];
                                format!("{:.2}/{:.2}/{:.2}/{}", a[0], a[1], a[2], a[3])
                            })
                            .collect();
                        println!("  {}", row.join("  "));
                    }
                }
            }
        }
    }
}

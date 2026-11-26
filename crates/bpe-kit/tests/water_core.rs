//! Water mode's engines (`bpe-water`: the grid core of part 2a and the all-drops engine of part C1)
//! beside today's Liquid core, drawn by today's liquid drawing (`bpe_water::bridge`). All play the
//! same scene: the same walls and the same grid. The grid core's drops are today's own births; the
//! all-drops engine pours its own even rows from the same spouts.
//!
//! `the_bridge_draws_every_drop` runs with the tests. `side_by_side` is a measuring run, by hand,
//! in release:
//!
//! `WATER_OUT=<folder> cargo test --release -p bpe-kit --test water_core -- --ignored --nocapture side_by_side`
//!
//! - `WATER_SHOTS=weird,thin,sheet,pour` the scenes (`WATER_SCENE=<file>` is the scene "file").
//! - `WATER_DETAILS=normal,fine,extreme` the liquid's Detail.
//! - `WATER_SECONDS`, `WATER_FPS`, `WATER_SIZE=540x960`, `WATER_CORES=old,drops` (any of `old`,
//!   `grid` and `drops`, side by side in that order).
//! - `WATER_MEASURE=1,2,4` when the streams are measured (seconds).
//! - `WATER_FAN=1` the new engines' pours keep the scene's Spread (see `NewCore::step`).
//! - `WATER_PULL`, `WATER_STICK` the all-drops engine's Surface pull (else the scene's) and
//!   Stickiness (else none).
//! - `BPE_GRID_*` replace the grid core's numbers (its steps a second follow the detail's cell
//!   size, `Numbers::for_cell`, unless `BPE_GRID_RATE` says otherwise), `BPE_WATER_*` the
//!   all-drops engine's.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use bpe_core::{Registry, Resolved, Scene, Settings, ThingId, resolve};
use bpe_gfx::swarm::{SwarmGpu, use_for_now};
use bpe_gfx::{Frame, Renderer, build_frame};
use bpe_kit::things::liquid::state::host_of;
use bpe_kit::things::liquid::walls::walls_now;
use bpe_kit::things::liquid::{Detail, LiquidSettings, LiquidState, births};
use bpe_kit::things::shape::{ShapeFill, ShapeSettings, ShapeStart};
use bpe_kit::{Kit, kit, scenes};
use bpe_runtime::{Playback, Wall};
use bpe_water::bridge::Bridge;
use bpe_water::grid::{Feel, Numbers, Water, WaterGpu};
use bpe_water::probe::{self, Stream};
use bpe_water::{Layout, Pour, SolidField};
use bpe_water::{Water as DropsWater, WaterGpu as DropsGpu};
use glam::Vec2;

/// How far `p` is from the nearest of `walls`' faces (pixels, below 0 inside), with hollow walls
/// taken as at least `least` thick. An opening in a hollow wall is no wall.
fn wall_distance(walls: &BTreeMap<ThingId, Wall>, p: Vec2, least: f32) -> f32 {
    let mut best = f32::MAX;
    for w in walls.values() {
        let (q, n) = w.outline.nearest(p);
        if !q.is_finite() || !n.is_finite() {
            continue;
        }
        if !w.solid && w.gaps.as_ref().is_some_and(|g| g.open_at(&w.outline, q).is_some()) {
            continue;
        }
        let d = (p - q).length();
        let d = if w.solid {
            if w.outline.inside(p) { -d } else { d }
        } else {
            let t = w.outline.thickness();
            d - 0.5 * (if t.is_finite() { t.max(0.0) } else { 0.0 }).max(least)
        };
        best = best.min(d);
    }
    best
}

/// Which of Water mode's engines plays. (One of each at most, for a few seconds' run.)
#[allow(clippy::large_enum_variant)]
enum Body {
    /// The grid core: today's births, a step at a time.
    Grid(Water),
    /// The all-drops engine: its own pours, a frame at a time.
    Drops(DropsWater),
}

impl Body {
    fn name(&self) -> &'static str {
        match self {
            Body::Grid(_) => "grid",
            Body::Drops(_) => "drops",
        }
    }

    fn layout(&self) -> Layout {
        match self {
            Body::Grid(w) => w.layout(),
            Body::Drops(w) => w.layout(),
        }
    }

    fn count(&self) -> u32 {
        match self {
            Body::Grid(w) => w.count(),
            Body::Drops(w) => w.count(),
        }
    }

    fn feed(&self) -> bpe_water::bridge::Feed<'_> {
        match self {
            Body::Grid(w) => w.feed(),
            Body::Drops(w) => w.feed(),
        }
    }

    fn read(&self) -> Vec<bpe_water::Drop> {
        match self {
            Body::Grid(w) => w.read(),
            Body::Drops(w) => w.read(),
        }
    }
}

/// Both new engines' pipelines on one device.
struct Cards {
    grid: Arc<WaterGpu>,
    drops: Arc<DropsGpu>,
}

impl Cards {
    fn new(device: &bpe_gfx::wgpu::Device, queue: &bpe_gfx::wgpu::Queue) -> Cards {
        Cards { grid: WaterGpu::new(device, queue), drops: DropsGpu::new(device, queue) }
    }
}

/// A scene's liquid on one of Water mode's engines: today's world at its start gives the walls,
/// the grid and the births; the drops move in `bpe-water`.
struct NewCore {
    /// Today's world, never stepped: where the births read the scene.
    start: Playback,
    liquids: Vec<LiquidState>,
    layout: bpe_kit::things::liquid::state::Layout,
    size: f32,
    water: Body,
    bridge: Bridge,
}

/// A number from the environment (the measuring run's settings), or `or`.
fn num(name: &str, or: f32) -> f32 {
    std::env::var(name).ok().and_then(|s| s.parse().ok()).unwrap_or(or)
}

fn rgba8(c: [f32; 4]) -> u32 {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    u32::from_le_bytes([b(c[0]), b(c[1]), b(c[2]), b(c[3])])
}

fn as_water(born: &[bpe_gfx::liquid_sim::Drop]) -> Vec<bpe_water::Drop> {
    born.iter()
        .map(|d| bpe_water::Drop::new(Vec2::from(d.x), Vec2::from(d.v), d.kind, d.serial, rgba8(d.colour)))
        .collect()
}

impl NewCore {
    /// The scene on `engine` (`grid` or `drops`); the grid core takes `numbers` when they are
    /// given.
    fn new(
        scene: &Scene,
        resolved: &Resolved,
        kit: &Kit,
        cards: &Cards,
        engine: &str,
        numbers: Option<Numbers>,
    ) -> NewCore {
        let mut start = Playback::new(scene, resolved, kit.sims.clone());
        start.seek(0.0);
        let world = start.world();
        let host_st = host_of(world).expect("a liquid");
        let host = host_st.host.as_ref().expect("its host");
        let l = host.layout;
        let layout = Layout { origin: l.origin, cell: l.cell, cols: l.cols, rows: l.rows };
        // (With no numbers given: the liquid's own for this cell size, the steps a second among
        // them, each replaceable by its `BPE_GRID_*`.)
        let numbers = numbers.unwrap_or_else(|| Numbers::from_env_for(l.cell));
        let walls = walls_now(world);
        // Thin walls a cell thick for the grid core, `LEAST_WALL` cells for the all-drops engine
        // (one-cell walls leak drops: see there).
        let least = if engine == "drops" { bpe_water::drops::LEAST_WALL * l.cell } else { l.cell };
        let field = SolidField::from_distance(layout, |p, _| wall_distance(&walls, p, least));
        let mut liquids: Vec<LiquidState> = scene
            .order
            .iter()
            .filter_map(|id| bpe_kit::things::liquid::state_of(world, *id))
            .filter(|s| s.index.is_some())
            .cloned()
            .collect();
        let mut feels = vec![Feel::default(); liquids.len()];
        for st in &liquids {
            let s = &st.settings;
            feels[st.index.expect("kept above")] = Feel {
                gravity: s.gravity,
                surface_pull: s.surface_pull,
                spray: s.spray,
                stickiness: 0.0,
                weight: s.weight,
                settles: s.settles,
            };
        }
        let size = host.size;
        // The first fill: today's own drops at the start.
        let first = as_water(&host.body.as_ref().expect("a card").read());
        let water = match engine {
            "grid" => {
                let mut water = Water::new(cards.grid.clone(), &field, world.gravity * size, numbers, &feels);
                water.add(&first);
                Body::Grid(water)
            }
            "drops" => {
                let feels: Vec<bpe_water::Feel> = feels
                    .iter()
                    .map(|f| bpe_water::Feel {
                        gravity: f.gravity,
                        surface_pull: num("WATER_PULL", f.surface_pull),
                        stickiness: num("WATER_STICK", 0.0),
                        weight: f.weight,
                        settles: f.settles,
                        ..bpe_water::Feel::default()
                    })
                    .collect();
                let numbers = bpe_water::Numbers::from_env_for(l.cell);
                let mut water = DropsWater::new(cards.drops.clone(), &field, world.gravity * size, numbers, &feels);
                // (Today's fill lies against walls a cell thick: the drops inside the thicker
                // walls' band would be thrown out of it, so they are left out.)
                let clear =
                    |d: &bpe_water::Drop| field.at(layout.to_world(Vec2::from(d.x))) >= numbers.wall_off * layout.cell;
                let first: Vec<bpe_water::Drop> = first.into_iter().filter(clear).collect();
                water.add(&first);
                let pours = pours_of(&mut liquids, world, &l, size);
                water.set_pours(&pours);
                Body::Drops(water)
            }
            other => panic!("no engine called {other}"),
        };
        let bridge = Bridge::new(cards.grid.device(), &field);
        NewCore { start, liquids, layout: l, size, water, bridge }
    }

    /// One of the grid core's steps.
    fn step(&mut self) {
        let Body::Grid(water) = &mut self.water else { unreachable!("the grid core steps") };
        let dt = water.numbers().dt();
        let step = water.steps();
        let world = self.start.world();
        for st in &mut self.liquids {
            st.owed.resize(st.pours.len(), 0.0);
            for k in 0..st.pours.len() {
                let born = births::pour(st, k, world, &self.layout, dt, step, self.size);
                let mut born = as_water(&born);
                // The drops are today's (their places, colours, serials); their ways are Water
                // mode's: a pour there does not fan out unless its Spread says so (the Pour card's
                // default is 0, where today's is 4 degrees, each drop's way picked at random), and
                // a Spread fans the stream smoothly across the spout (`bpe_water::Pour::fanned`).
                // `WATER_FAN=1` keeps the scene's own Spread.
                let p = &st.pours[k];
                let anchor = world.anchor(st.id);
                let aim = p.aim + anchor.angle.to_degrees();
                let speed = p.speed * self.size.max(0.0).sqrt();
                let width = births::pour_width(p).max(1e-3);
                let spread = if std::env::var("WATER_FAN").is_ok() { p.spread } else { 0.0 };
                let fan = Pour { aim, spread, ..Pour::default() };
                let spout = anchor.position + Vec2::from_angle(anchor.angle).rotate(p.place);
                let across = fan.fanned(0.0).perp();
                let layout = water.layout();
                for d in &mut born {
                    let u = (layout.to_world(Vec2::from(d.x)) - spout).dot(across) / width;
                    d.v = (fan.fanned(u) * speed / layout.cell).to_array();
                }
                water.add(&born);
            }
        }
        water.step();
    }

    fn run_to(&mut self, seconds: f64) {
        if let Body::Drops(water) = &mut self.water {
            // (It moves on in frames of 1/60 s.)
            let to = (seconds * 60.0).round() as u32;
            while water.frames() < to {
                water.frame();
            }
            return;
        }
        loop {
            let Body::Grid(water) = &self.water else { unreachable!("the grid core") };
            if water.steps() >= (seconds * f64::from(water.numbers().rate)).round() as u32 {
                break;
            }
            self.step();
        }
    }

    /// The scene's picture with the new core's drops where today's would be.
    fn frame(&mut self, scene: &Scene, resolved: &Resolved, kit: &Kit, time: f64) -> Frame {
        self.bridge.update(&self.water.feed());
        let mut frame = Frame::default();
        build_frame(scene, resolved, self.start.world(), self.start.log(), time, &kit.draws, &mut frame);
        let source = self.bridge.source();
        for liquid in &mut frame.liquids {
            if liquid.drops.is_some() {
                liquid.drops = Some(source.clone());
            }
        }
        frame
    }

    fn drops(&self) -> Vec<bpe_water::Drop> {
        self.water.read()
    }
}

/// The scene's pours as the all-drops engine pours them: from today's spouts, Rate x Width wide,
/// each drop's kind and colour those of today's first drop.
fn pours_of(
    liquids: &mut [LiquidState],
    world: &bpe_runtime::World,
    layout: &bpe_kit::things::liquid::state::Layout,
    size: f32,
) -> Vec<Pour> {
    let mut out = Vec::new();
    for st in liquids.iter_mut() {
        st.owed.resize(st.pours.len(), 0.0);
        for k in 0..st.pours.len() {
            let p = st.pours[k].clone();
            let mut probe = st.clone();
            probe.owed[k] = 1.0;
            let Some(first) = births::pour(&mut probe, k, world, layout, 1e-6, 0, size).first().copied() else {
                continue;
            };
            let anchor = world.anchor(st.id);
            let spread = if std::env::var("WATER_FAN").is_ok() { p.spread } else { 0.0 };
            out.push(Pour {
                place: anchor.position + Vec2::from_angle(anchor.angle).rotate(p.place),
                aim: p.aim + anchor.angle.to_degrees(),
                width: births::pour_width(&p),
                speed: p.speed * size.max(0.0).sqrt(),
                spread,
                kind: first.kind,
                colour: rgba8(first.colour),
                ..Pour::default()
            });
        }
    }
    out
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

/// A filled block with corners `points` (world pixels, clockwise on the screen).
fn block(scene: &mut Scene, reg: &Registry, points: &[(f32, f32)]) {
    let points: Vec<Vec2> = points.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
    let centre = points.iter().copied().sum::<Vec2>() / points.len() as f32;
    let far = points.iter().map(|p| (*p - centre).length()).fold(1.0, f32::max);
    // (The next power of two: a shape's Size any corner fits in.)
    let mut size = 1.0f32;
    while size < far {
        size *= 2.0;
    }
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

/// A tall box with one pour `cells` cells wide (at Normal) from near its top, straight down.
fn pour_scene(reg: &Registry, width: f32, at: Vec2) -> Scene {
    let (mut scene, shape) = scenes::liquid_box(reg);
    scene.things.get_mut(&shape).expect("the box").params = ShapeSettings::rectangle(840.0, 1680.0, 3.0).to_params();
    let id = scenes::add_liquid(&mut scene, reg, ThingId::ROOT, |f| f.fill = false, |_| {});
    scene.things.get_mut(&id).expect("just added").transform.position = at;
    scenes::add_pour(&mut scene, reg, id, |p| {
        p.width = width;
        p.rate = 1.0;
    });
    scene
}

/// The scenes by name: `weird` is the owner's Water Weird as built here (its file when
/// `WATER_SCENE` names one is "file"), `thin` a two-cell pour down the whole box, `sheet` a pour
/// onto a ledge it runs off the edge of, `pour` today's pour-on-a-shape test scene.
fn shot(reg: &Registry, name: &str) -> Scene {
    match name {
        "file" => {
            let path = std::env::var("WATER_SCENE").expect("WATER_SCENE, a scene file");
            bpe_core::load(&std::fs::read_to_string(&path).expect("the scene file"), reg).expect("a scene")
        }
        "weird" => {
            let mut scene = pour_scene(reg, 19.2, Vec2::new(120.0, -570.0));
            block(&mut scene, reg, &[(0.0, -240.0), (210.0, -330.0), (240.0, -210.0), (30.0, -120.0)]);
            let id = bpe_kit::add_thing(&mut scene, reg, "shape", ThingId::ROOT).expect("a shape");
            let t = scene.things.get_mut(&id).expect("just added");
            t.params = ShapeSettings { fill: ShapeFill::Filled, ..ShapeSettings::circle(120.0, 3.0) }.to_params();
            t.transform.position = Vec2::new(-180.0, -120.0);
            scene
        }
        "thin" => pour_scene(reg, 16.0, Vec2::new(0.0, -780.0)),
        "sheet" => {
            let mut scene = pour_scene(reg, 48.0, Vec2::new(-250.0, -560.0));
            block(&mut scene, reg, &[(-420.0, -400.0), (0.0, -400.0), (0.0, -370.0), (-420.0, -370.0)]);
            scene
        }
        "pour" => scenes::liquid_pour_on_a_shape(reg),
        other => panic!("no scene called {other}"),
    }
}

fn list(name: &str, or: &str) -> Vec<String> {
    std::env::var(name).unwrap_or_else(|_| or.into()).split(',').map(|s| s.trim().to_lowercase()).collect()
}

fn details() -> Vec<Detail> {
    list("WATER_DETAILS", "normal")
        .iter()
        .map(|d| match d.as_str() {
            "rough" => Detail::Rough,
            "normal" => Detail::Normal,
            "fine" => Detail::Fine,
            "veryfine" => Detail::VeryFine,
            "extreme" => Detail::Extreme,
            other => panic!("no detail called {other}"),
        })
        .collect()
}

fn save(path: &std::path::Path, w: u32, h: u32, pixels: &[u8]) {
    let file = std::fs::File::create(path).expect("a picture file");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().expect("a header").write_image_data(pixels).expect("the pixels");
}

fn picture(
    renderer: &mut Renderer,
    device: &bpe_gfx::wgpu::Device,
    queue: &bpe_gfx::wgpu::Queue,
    frame: &Frame,
) -> Vec<u8> {
    renderer.render(device, queue, frame);
    while renderer.programs_waiting() {
        renderer.render(device, queue, frame);
    }
    renderer.read_output(device, queue).expect("the picture")
}

/// The stream measures of drops (grid units on `layout`) in the air of the box: every drop more
/// than `off` pixels from a wall, between the heights `top` and `bottom`.
fn stream_of(drops: &[bpe_water::Drop], layout: &Layout, field: &SolidField, (top, bottom): (f32, f32)) -> Stream {
    let air: Vec<bpe_water::Drop> = drops
        .iter()
        .filter(|d| {
            let p = layout.to_world(Vec2::from(d.x));
            p.y >= top && p.y < bottom && field.at(p) > 3.0 * layout.cell
        })
        .copied()
        .collect();
    Stream::of(&air, layout, (top, bottom), 1.5, 6)
}

/// Both cores play the scenes; their pictures are saved side by side (today's on the left) and
/// their streams measured.
#[test]
#[ignore = "a measuring run: by hand"]
fn side_by_side() {
    let (device, queue) = bpe_gfx::test_device().expect("a graphics device (install mesa-vulkan-drivers)");
    let info = device.adapter_info();
    println!("# card\t{}\t{:?}\t{}", info.name, info.backend, info.driver_info);
    println!(
        "# numbers (but `rate`: the steps a second follow the cell size, printed by scene)\t{:?}",
        Numbers::from_env()
    );
    let _card = use_for_now(Some(SwarmGpu::for_device(&device, &queue)));
    let cards = Cards::new(&device, &queue);
    let kit = kit();
    let out = std::path::PathBuf::from(std::env::var("WATER_OUT").expect("WATER_OUT, a folder"));
    let num = |name: &str, or: f64| std::env::var(name).ok().and_then(|s| s.parse().ok()).unwrap_or(or);
    let (seconds, fps) = (num("WATER_SECONDS", 4.0), num("WATER_FPS", 30.0));
    let size = std::env::var("WATER_SIZE").unwrap_or_else(|_| "540x960".into());
    let (w, h) = size.split_once('x').expect("a size like 540x960");
    let (w, h): (u32, u32) = (w.parse().expect("a width"), h.parse().expect("a height"));
    let cores = list("WATER_CORES", "old,drops");
    let old = cores.iter().any(|c| c == "old");
    let engines: Vec<String> = cores.iter().filter(|c| *c == "grid" || *c == "drops").cloned().collect();
    let measure: Vec<f64> = list("WATER_MEASURE", "1,2,4").iter().filter_map(|s| s.parse().ok()).collect();
    let mut renderer = Renderer::new(&device, w, h);
    println!("# shot\tdetail\tcore\tseconds\tdrops\tin air\tpieces\twhole\tstrays\tgap px\twidths");
    for name in list("WATER_SHOTS", "weird") {
        for detail in details() {
            let scene = at_detail(shot(&kit.registry, &name), detail);
            let resolved = resolve(&scene, &kit.registry);
            assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
            let mut today = old.then(|| Playback::new(&scene, &resolved, kit.sims.clone()));
            let mut news: Vec<NewCore> =
                engines.iter().map(|e| NewCore::new(&scene, &resolved, &kit, &cards, e, None)).collect();
            for core in &news {
                let cell = core.water.layout().cell;
                match &core.water {
                    Body::Grid(w) => {
                        println!(
                            "# {name}\t{detail:?}\tthe grid core steps {} times a second ({cell} px cells)",
                            w.numbers().rate
                        )
                    }
                    Body::Drops(_) => println!("# {name}\t{detail:?}\tthe all-drops engine ({cell} px cells)"),
                }
            }
            let dir = out.join(format!("{name}-{detail:?}").to_lowercase());
            std::fs::create_dir_all(&dir).expect("the frames' folder");
            let began = Instant::now();
            let (mut old_s, mut new_s) = (0.0, vec![0.0; news.len()]);
            let count = (seconds * fps).round() as u32;
            let mut due = measure.clone();
            for i in 0..=count {
                let t = f64::from(i) / fps;
                let mut halves: Vec<Vec<u8>> = Vec::new();
                let now = due.first().is_some_and(|m| t + 1e-9 >= *m);
                if let Some(p) = today.as_mut() {
                    let at = Instant::now();
                    p.seek(t);
                    old_s += at.elapsed().as_secs_f64();
                    let mut frame = Frame::default();
                    build_frame(&scene, &resolved, p.world(), p.log(), t, &kit.draws, &mut frame);
                    halves.push(picture(&mut renderer, &device, &queue, &frame));
                    if now {
                        let host = host_of(p.world()).and_then(|s| s.host.as_ref()).expect("a host");
                        let l = host.layout;
                        let layout = Layout { origin: l.origin, cell: l.cell, cols: l.cols, rows: l.rows };
                        let walls = walls_now(p.world());
                        let field = SolidField::from_distance(layout, |q, least| wall_distance(&walls, q, least));
                        let drops = as_water(&host.body.as_ref().expect("a card").read());
                        report(&name, detail, "old", t, &drops, &layout, &field);
                    }
                }
                for (k, n) in news.iter_mut().enumerate() {
                    let at = Instant::now();
                    n.run_to(t);
                    let frame = n.frame(&scene, &resolved, &kit, t);
                    halves.push(picture(&mut renderer, &device, &queue, &frame));
                    new_s[k] += at.elapsed().as_secs_f64();
                    if now {
                        let layout = n.water.layout();
                        let walls = walls_now(n.start.world());
                        let field = SolidField::from_distance(layout, |q, least| wall_distance(&walls, q, least));
                        report(&name, detail, n.water.name(), t, &n.drops(), &layout, &field);
                    }
                }
                if now {
                    due.remove(0);
                }
                let wide = w * halves.len() as u32;
                let mut joined = vec![0u8; (wide * h * 4) as usize];
                for (k, half) in halves.iter().enumerate() {
                    for y in 0..h as usize {
                        let from = y * w as usize * 4;
                        let to = (y * wide as usize + k * w as usize) * 4;
                        joined[to..to + w as usize * 4].copy_from_slice(&half[from..from + w as usize * 4]);
                    }
                }
                save(&dir.join(format!("f{i:05}.png")), wide, h, &joined);
            }
            println!(
                "# {}\t{} frames\t{:.0} s in all\ttoday's core {old_s:.1} s\twith their pictures: {}",
                dir.display(),
                count + 1,
                began.elapsed().as_secs_f64(),
                news.iter()
                    .zip(&new_s)
                    .map(|(n, s)| format!("{} {s:.1} s", n.water.name()))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
}

fn report(
    name: &str,
    detail: Detail,
    core: &str,
    t: f64,
    drops: &[bpe_water::Drop],
    layout: &Layout,
    field: &SolidField,
) {
    let lo = layout.to_world(Vec2::splat(2.0));
    let hi = layout.to_world(Vec2::new(layout.cols as f32 - 3.0, layout.rows as f32 - 3.0));
    let st = stream_of(drops, layout, field, (lo.y + 40.0, hi.y - 120.0));
    let widths: Vec<String> = st.widths.iter().map(|w| format!("{w:.0}")).collect();
    println!(
        "{name}\t{detail:?}\t{core}\t{t}\t{}\t{}\t{}\t{:.3}\t{:.3}\t{:.0}\t{}",
        drops.len(),
        st.drops,
        st.pieces,
        st.whole,
        st.strays,
        st.gap,
        widths.join(" ")
    );
    // How thin liquid's drops move across their neighbours' way (`WATER_ACROSS=1`): a thread whose
    // drops fly crosswise is drawn as hairs.
    if std::env::var("WATER_ACROSS").is_ok() {
        across(name, detail, core, t, drops, layout);
    }
    // The drops themselves as dots (`WATER_DOTS=<folder>`): what the physics did, without the look.
    if let Ok(dir) = std::env::var("WATER_DOTS") {
        let (pixels, w, h) = probe::picture(drops, field, 1.0, f32::MAX);
        std::fs::create_dir_all(&dir).expect("the dots' folder");
        let file = format!("{name}-{detail:?}-{core}-{t}.png").to_lowercase();
        probe::save_png(&std::path::Path::new(&dir).join(file), &pixels, w, h).expect("a picture");
    }
}

/// For each drop with fewer than 12 others within two cells (thin liquid), its speed against the
/// mean of those others': the spread along and across their way, as shares of the mean speed, and
/// the share of drops flying more than 45 degrees off it.
fn across(name: &str, detail: Detail, core: &str, t: f64, drops: &[bpe_water::Drop], layout: &Layout) {
    let mut cells: std::collections::HashMap<(i32, i32), Vec<usize>> = std::collections::HashMap::new();
    for (i, d) in drops.iter().enumerate() {
        cells.entry((d.x[0].floor() as i32, d.x[1].floor() as i32)).or_default().push(i);
    }
    let (mut n, mut speed, mut along, mut across, mut wide) = (0usize, 0.0f64, 0.0f64, 0.0f64, 0usize);
    for (i, d) in drops.iter().enumerate() {
        let p = Vec2::from(d.x);
        let (cx, cy) = (p.x.floor() as i32, p.y.floor() as i32);
        let (mut sum, mut k) = (Vec2::ZERO, 0usize);
        for dy in -2..=2 {
            for dx in -2..=2 {
                for &j in cells.get(&(cx + dx, cy + dy)).map(Vec::as_slice).unwrap_or(&[]) {
                    if j != i && (Vec2::from(drops[j].x) - p).length() < 2.0 {
                        sum += Vec2::from(drops[j].v);
                        k += 1;
                    }
                }
            }
        }
        if k == 0 || k >= 12 {
            continue;
        }
        let mean = sum / k as f32;
        if mean.length() * layout.cell < 200.0 {
            continue;
        }
        let way = mean.normalize();
        let off = Vec2::from(d.v) - mean;
        n += 1;
        speed += f64::from(mean.length());
        along += f64::from(off.dot(way) / mean.length()).powi(2);
        across += f64::from(off.dot(way.perp()) / mean.length()).powi(2);
        if Vec2::from(d.v).normalize_or_zero().dot(way) < std::f32::consts::FRAC_1_SQRT_2 {
            wide += 1;
        }
    }
    let m = n.max(1) as f64;
    println!(
        "# across\t{name}\t{detail:?}\t{core}\t{t}\tthin fast drops {n}\tmean {:.0} px/s\talong {:.3}\tacross {:.3}\tover 45 deg {:.3}",
        speed / m * f64::from(layout.cell),
        (along / m).sqrt(),
        (across / m).sqrt(),
        wide as f64 / m
    );
}

/// The bridge hands today's drawing every drop, from either engine: a pool drawn through it covers
/// the pool.
#[test]
fn the_bridge_draws_every_drop() {
    let Ok((device, queue)) = bpe_gfx::test_device() else { return };
    let _card = use_for_now(Some(SwarmGpu::for_device(&device, &queue)));
    let cards = Cards::new(&device, &queue);
    let kit = kit();
    let (mut scene, shape) = scenes::liquid_box(&kit.registry);
    scenes::add_liquid(&mut scene, &kit.registry, shape, |f| f.level = 0.5, |_| {});
    let resolved = resolve(&scene, &kit.registry);
    let mut renderer = Renderer::new(&device, 270, 480);
    for engine in ["grid", "drops"] {
        let mut core = NewCore::new(&scene, &resolved, &kit, &cards, engine, Some(Numbers::default()));
        assert!(core.water.count() > 500, "{engine}: the fill's drops are in: {}", core.water.count());
        core.run_to(0.25);
        let wet = picture(&mut renderer, &device, &queue, &core.frame(&scene, &resolved, &kit, 0.25));
        // The same scene with no drops at all.
        let mut dry = Frame::default();
        build_frame(&scene, &resolved, core.start.world(), core.start.log(), 0.25, &kit.draws, &mut dry);
        dry.liquids.clear();
        let dry = picture(&mut renderer, &device, &queue, &dry);
        let changed = wet
            .chunks(4)
            .zip(dry.chunks(4))
            .filter(|(a, b)| a[..3].iter().zip(&b[..3]).any(|(x, y)| x.abs_diff(*y) > 24))
            .count();
        // Where the drops are: the share of the picture's pixels within a drop's reach of one.
        let layout = core.water.layout();
        let drops = core.drops();
        let lo = drops.iter().map(|d| layout.to_world(Vec2::from(d.x))).fold(Vec2::splat(f32::MAX), Vec2::min);
        let hi = drops.iter().map(|d| layout.to_world(Vec2::from(d.x))).fold(Vec2::splat(f32::MIN), Vec2::max);
        // (The picture is 270 wide of a world 1080 wide: a quarter the size.)
        let covered = ((hi.x - lo.x) * (hi.y - lo.y) / 16.0) as usize;
        assert!(covered > 2000, "{engine}: the pool is big enough to judge: {covered}");
        assert!(
            changed as f32 > covered as f32 * 0.85,
            "{engine}: the drawn liquid covers the pool: {changed} of {covered} pixels"
        );
        assert!(
            (changed as f32) < covered as f32 * 1.25,
            "{engine}: and nothing far outside it: {changed} of {covered} pixels"
        );
    }
}

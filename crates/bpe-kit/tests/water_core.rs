//! Water mode's all-drops engine (`bpe-water`, part C1) beside today's Liquid core: today's drawn
//! by today's renderer, the new engine by Water mode's plain drawing on the card
//! (`bpe_water::draw`, part C3). Both play the same scene: the same walls and the same grid; the
//! all-drops engine pours its own even rows from the same spouts.
//!
//! `the_plain_drawing_draws_every_drop` runs with the tests. `side_by_side` is a measuring run, by
//! hand, in release:
//!
//! `WATER_OUT=<folder> cargo test --release -p bpe-kit --test water_core -- --ignored --nocapture side_by_side`
//!
//! - `WATER_SHOTS=weird,thin,sheet,pour` the scenes (`WATER_SCENE=<file>` is the scene "file").
//! - `WATER_DETAILS=normal,fine,extreme` the liquid's Detail.
//! - `WATER_SECONDS`, `WATER_FPS`, `WATER_SIZE=540x960`, `WATER_CORES=old,drops` (either or both,
//!   side by side in that order).
//! - `WATER_MEASURE=1,2,4` when the streams are measured (seconds).
//! - `WATER_FAN=1` the pours keep the scene's Spread (see `pours_of`).
//! - `WATER_PULL`, `WATER_STICK` the all-drops engine's Surface pull (else the scene's) and
//!   Stickiness (else none); `BPE_WATER_*` its numbers.

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
use bpe_water::draw::{Plain, PlainGpu, PlainLook, StoredDrops, View};
use bpe_water::probe::{self, Stream};
use bpe_water::{Feel, Layout, Numbers, Pour, SolidField, Water, WaterGpu};
use glam::Vec2;

/// How far `p` is from the nearest of `walls`' faces (pixels, below 0 inside), with hollow walls
/// taken as at least `least` thick. An opening in a hollow wall is no wall.
fn wall_distance(walls: &BTreeMap<ThingId, Wall>, p: Vec2, least: f32) -> f32 {
    walls.values().filter_map(|w| one_wall(w, p, least, false)).fold(f32::MAX, f32::min)
}

/// [`wall_distance`] for the drawing: a closed hollow wall round `middle` (the course's box) is
/// carved out of rock, so everything outside it is rock. Water mode's courses are carved out of
/// solid rock, and the drawing's depth lets light in only through the air.
fn drawn_distance(walls: &BTreeMap<ThingId, Wall>, p: Vec2, least: f32, middle: Vec2) -> f32 {
    walls
        .values()
        .filter_map(|w| {
            let container = !w.solid && w.gaps.is_none() && !w.passes && w.outline.inside(middle);
            one_wall(w, p, least, container)
        })
        .fold(f32::MAX, f32::min)
}

/// How far `p` is from one wall's face (below 0 inside it), or `None` where the wall is open there.
/// `carved`: a hollow wall whose outside is all rock.
fn one_wall(w: &Wall, p: Vec2, least: f32, carved: bool) -> Option<f32> {
    let (q, n) = w.outline.nearest(p);
    if !q.is_finite() || !n.is_finite() {
        return None;
    }
    if !w.solid && w.gaps.as_ref().is_some_and(|g| g.open_at(&w.outline, q).is_some()) {
        return None;
    }
    let d = (p - q).length();
    Some(if w.solid {
        if w.outline.inside(p) { -d } else { d }
    } else {
        let t = w.outline.thickness();
        let half = 0.5 * (if t.is_finite() { t.max(0.0) } else { 0.0 }).max(least);
        if carved && !w.outline.inside(p) { -d - half } else { d - half }
    })
}

/// The all-drops engine's and the plain drawing's pipelines on one device.
struct Cards {
    drops: Arc<WaterGpu>,
    plain: Arc<PlainGpu>,
}

impl Cards {
    fn new(device: &bpe_gfx::wgpu::Device, queue: &bpe_gfx::wgpu::Queue) -> Cards {
        Cards { drops: WaterGpu::new(device, queue), plain: PlainGpu::new(device, queue) }
    }
}

/// A scene's liquid on the all-drops engine: today's world at its start gives the walls, the grid,
/// the first fill and the spouts; the drops move in `bpe-water` and are drawn by its plain drawing.
struct NewCore {
    /// Today's world, never stepped: where the scene is read.
    start: Playback,
    water: Water,
    plain: Plain,
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
    fn new(scene: &Scene, resolved: &Resolved, kit: &Kit, cards: &Cards) -> NewCore {
        let mut start = Playback::new(scene, resolved, kit.sims.clone());
        start.seek(0.0);
        let world = start.world();
        let host_st = host_of(world).expect("a liquid");
        let host = host_st.host.as_ref().expect("its host");
        let l = host.layout;
        let layout = Layout { origin: l.origin, cell: l.cell, cols: l.cols, rows: l.rows };
        let walls = walls_now(world);
        // Thin walls `LEAST_WALL` cells thick (one-cell walls leak drops: see there).
        let least = bpe_water::drops::LEAST_WALL * l.cell;
        let field = SolidField::from_distance(layout, |p, _| wall_distance(&walls, p, least));
        let middle = layout.to_world(Vec2::new(l.cols as f32 - 1.0, l.rows as f32 - 1.0) * 0.5);
        let drawn = SolidField::from_distance(layout, |p, _| drawn_distance(&walls, p, least, middle));
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
                surface_pull: num("WATER_PULL", s.surface_pull),
                stickiness: num("WATER_STICK", 0.0),
                weight: s.weight,
                settles: s.settles,
                ..Feel::default()
            };
        }
        let size = host.size;
        let numbers = Numbers::from_env_for(l.cell);
        let mut water = Water::new(cards.drops.clone(), &field, world.gravity * size, numbers, &feels);
        // The first fill: today's own drops at the start. (They lie against walls a cell thick:
        // the drops inside the thicker walls' band would be thrown out of it, so they are left out.)
        let first = as_water(&host.body.as_ref().expect("a card").read());
        let clear = |d: &bpe_water::Drop| field.at(layout.to_world(Vec2::from(d.x))) >= numbers.wall_off * layout.cell;
        let first: Vec<bpe_water::Drop> = first.into_iter().filter(clear).collect();
        water.add(&first);
        let pours = pours_of(&mut liquids, world, &l, size);
        water.set_pours(&pours);
        let plain = Plain::new(cards.plain.clone(), &drawn, numbers.wall_off);
        NewCore { start, water, plain }
    }

    fn run_to(&mut self, seconds: f64) {
        // (It moves on in frames of 1/60 s.)
        let to = (seconds * 60.0).round() as u32;
        while self.water.frames() < to {
            self.water.frame();
        }
    }

    /// The drops in the plain drawing, on `view`.
    fn picture(&mut self, view: &View) -> Vec<u8> {
        self.plain.draw(&self.water.drop_buffers(), view, &PlainLook::default());
        self.plain.read()
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
    let new = cores.iter().any(|c| c == "drops");
    let measure: Vec<f64> = list("WATER_MEASURE", "1,2,4").iter().filter_map(|s| s.parse().ok()).collect();
    let mut renderer = Renderer::new(&device, w, h);
    // The plain drawing of the same screen (1080 x 1920 world pixels round the middle).
    let view = View { lo: Vec2::new(-540.0, -960.0), scale: w as f32 / 1080.0, width: w, height: h };
    println!("# shot\tdetail\tcore\tseconds\tdrops\tin air\tpieces\twhole\tstrays\tgap px\twidths");
    for name in list("WATER_SHOTS", "weird") {
        for detail in details() {
            let scene = at_detail(shot(&kit.registry, &name), detail);
            let resolved = resolve(&scene, &kit.registry);
            assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
            let mut today = old.then(|| Playback::new(&scene, &resolved, kit.sims.clone()));
            let mut news: Vec<NewCore> =
                if new { vec![NewCore::new(&scene, &resolved, &kit, &cards)] } else { Vec::new() };
            for core in &news {
                let cell = core.water.layout().cell;
                println!("# {name}\t{detail:?}\tthe all-drops engine ({cell} px cells)");
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
                    halves.push(n.picture(&view));
                    new_s[k] += at.elapsed().as_secs_f64();
                    if now {
                        let layout = n.water.layout();
                        let walls = walls_now(n.start.world());
                        let field = SolidField::from_distance(layout, |q, least| wall_distance(&walls, q, least));
                        report(&name, detail, "drops", t, &n.drops(), &layout, &field);
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
                new_s.iter().map(|s| format!("drops {s:.1} s")).collect::<Vec<_>>().join(", ")
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
        let (pixels, w, h) = probe::picture(drops, field, 1.0);
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

/// The plain drawing draws every drop of the all-drops engine: a pool drawn by it covers the pool,
/// and nothing far outside it.
#[test]
fn the_plain_drawing_draws_every_drop() {
    let Ok((device, queue)) = bpe_gfx::test_device() else { return };
    let _card = use_for_now(Some(SwarmGpu::for_device(&device, &queue)));
    let cards = Cards::new(&device, &queue);
    let kit = kit();
    let (mut scene, shape) = scenes::liquid_box(&kit.registry);
    scenes::add_liquid(&mut scene, &kit.registry, shape, |f| f.level = 0.5, |_| {});
    let resolved = resolve(&scene, &kit.registry);
    let view = View::screen(0.25);
    let mut core = NewCore::new(&scene, &resolved, &kit, &cards);
    assert!(core.water.count() > 500, "the fill's drops are in: {}", core.water.count());
    core.run_to(0.25);
    let wet = core.picture(&view);
    // The same course with no drops at all.
    let none = StoredDrops::new(&device, &queue, &[]);
    core.plain.draw(&none.buffers(), &view, &PlainLook::default());
    let dry = core.plain.read();
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
    assert!(covered > 2000, "the pool is big enough to judge: {covered}");
    assert!(changed as f32 > covered as f32 * 0.85, "the drawn liquid covers the pool: {changed} of {covered} pixels");
    assert!((changed as f32) < covered as f32 * 1.25, "and nothing far outside it: {changed} of {covered} pixels");
}

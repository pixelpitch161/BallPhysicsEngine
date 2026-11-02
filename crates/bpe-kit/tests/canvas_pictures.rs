//! Picture tests for canvases on the software GPU: the graphics card replays the marks the
//! simulation logs into each canvas's textures, and seeking back redraws them exactly.

use bpe_core::{Attach, Color, Gradient, Params, Resolved, Scene, SlotId, ThingId, Value, resolve};
use bpe_gfx::{Frame, Renderer, build_frame, headless_device};
use bpe_kit::things::canvas::{canvas_area, parts_local, skin_area, wall_local};
use bpe_kit::{Kit, add_thing, fill_slot, flow_source, kit, new_scene};
use bpe_runtime::{Mark, MarkTarget, Outline, Playback, Pose, Solid};
use glam::Vec2;

const W: u32 = 270;
const H: u32 = 480;
const BALLS: ThingId = ThingId(2);

fn gpu() -> Option<(bpe_gfx::wgpu::Device, bpe_gfx::wgpu::Queue)> {
    match headless_device() {
        Ok(pair) => Some(pair),
        Err(_) => {
            eprintln!("no GPU adapter available, skipping");
            None
        }
    }
}

/// Gives `thing` a Marks module and applies `f` to its settings.
fn brushes(scene: &mut Scene, kit: &Kit, thing: ThingId, f: impl FnOnce(&mut Params)) {
    fill_slot(scene, &kit.registry, thing, SlotId::Marks).unwrap();
    let m = bpe_core::resolve_slot(scene, &kit.registry, thing, SlotId::Marks, &mut Vec::new()).unwrap().module;
    f(&mut scene.library.modules.get_mut(&m).unwrap().params);
}

/// The new scene with a canvas fitting the arena (with `canvas`'s changes) and a ball painting
/// a trail on it.
fn painting(canvas: impl FnOnce(&mut Params)) -> (Scene, Resolved, Kit, ThingId) {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let id = add_thing(&mut scene, &kit.registry, "canvas", ThingId::ROOT).unwrap();
    canvas(&mut scene.things.get_mut(&id).unwrap().params);
    brushes(&mut scene, &kit, BALLS, |p| p.set("paint_on", Value::Bool(true)));
    let resolved = resolve(&scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    (scene, resolved, kit, id)
}

/// Draws `scene` at `time` with `renderer`, stepping its canvases on the graphics card first.
/// With `canvases` false, the same frame without its canvases.
#[allow(clippy::too_many_arguments)]
fn draw(
    device: &bpe_gfx::wgpu::Device,
    queue: &bpe_gfx::wgpu::Queue,
    renderer: &mut Renderer,
    scene: &Scene,
    resolved: &Resolved,
    kit: &Kit,
    playback: &mut Playback,
    time: f64,
    canvases: bool,
) -> Vec<u8> {
    playback.seek(time);
    let mut frame = Frame::default();
    build_frame(scene, resolved, playback.world(), playback.log(), time, &kit.draws, &mut frame);
    frame.bloom = 0.0;
    if !canvases {
        frame.canvases.clear();
    }
    let source = flow_source(scene, resolved, &kit.sims, None);
    assert!(renderer.render_with_flow(device, queue, &frame, &source).caught_up);
    let pixels = renderer.read_output(device, queue).unwrap();
    save(&format!("canvas-{time}{}", if canvases { "" } else { "-bare" }), &pixels);
    pixels
}

fn save(name: &str, pixels: &[u8]) {
    save_sized(name, pixels, W, H);
}

fn save_sized(name: &str, pixels: &[u8], width: u32, height: u32) {
    if std::env::var_os("BPE_SAVE_RENDERS").is_none() {
        return;
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pictures");
    std::fs::create_dir_all(&dir).unwrap();
    let file = std::fs::File::create(dir.join(format!("{name}.png"))).unwrap();
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(pixels).unwrap();
}

/// The output pixel at world point `p` of the 1080×1920 video, drawn at a quarter size.
fn pixel(pixels: &[u8], p: Vec2) -> [i32; 3] {
    let (x, y) = (((p.x + 540.0) / 4.0) as usize, ((p.y + 960.0) / 4.0) as usize);
    let i = (y * W as usize + x) * 4;
    [pixels[i] as i32, pixels[i + 1] as i32, pixels[i + 2] as i32]
}

/// How far the picture's tone mapping may move a mark's colour in a channel.
const TONE: i32 = 12;

/// Whether two pixels are within `tolerance` of each other in every channel.
fn near(a: [i32; 3], b: [i32; 3], tolerance: i32) -> bool {
    a.iter().zip(&b).all(|(x, y)| (x - y).abs() <= tolerance)
}

/// A mark's colour as an output pixel.
fn shown(m: &Mark) -> [i32; 3] {
    let c = Color::from_linear(m.colour);
    [c.r as i32, c.g as i32, c.b as i32]
}

/// A canvas point in the world, through the canvas's pose.
fn world_of(pose: Pose, local: Vec2) -> Vec2 {
    pose.position + Vec2::from_angle(pose.angle).rotate(local)
}

/// How far canvas point `p` is from the middle line of mark `m`.
fn distance(m: &Mark, p: Vec2) -> f32 {
    let d = m.to - m.from;
    let t = if d.length_squared() > 0.0 { ((p - m.from).dot(d) / d.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
    (p - (m.from + d * t)).length()
}

/// When mark `m` was made, in seconds.
fn time_of(m: &Mark, playback: &Playback) -> f64 {
    m.step as f64 / playback.world().steps_per_second as f64
}

#[test]
fn painted_stroke_shows_where_the_ball_went() {
    let Some((device, queue)) = gpu() else { return };
    let (scene, resolved, kit, canvas) = painting(|_| {});
    let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
    let mut renderer = Renderer::new(&device, W, H);
    let painted = draw(&device, &queue, &mut renderer, &scene, &resolved, &kit, &mut playback, 1.0, true);
    let bare =
        draw(&device, &queue, &mut Renderer::new(&device, W, H), &scene, &resolved, &kit, &mut playback, 1.0, false);
    let (pose, size) = canvas_area(playback.world(), canvas).unwrap();
    let ball = playback.world().bodies[&BALLS].position[0];
    let radius = playback.world().bodies[&BALLS].radius[0];
    let marks = playback.log().marks();
    assert!(marks.len() > 30, "{} marks", marks.len());
    // On the path, away from the ball: the ball's colour.
    let mut on = 0;
    for m in marks.iter().filter(|m| (0.1..0.8).contains(&time_of(m, &playback))) {
        let p = world_of(pose, (m.from + m.to) * 0.5);
        if (p - ball).length() < radius * 2.0 + 20.0 {
            continue;
        }
        let got = pixel(&painted, p);
        assert!(near(got, shown(m), TONE), "{p}: {got:?}, the stroke is {:?}", shown(m));
        assert!(!near(got, pixel(&bare, p), 20), "{p}: the stroke changes the picture");
        on += 1;
    }
    assert!(on >= 10, "{on} points on the path were away from the ball");
    // Off the path: just what is under the canvas.
    let mut off = 0;
    for gx in -8..=8 {
        for gy in -12..=12 {
            let local = Vec2::new(gx as f32, gy as f32) * size / Vec2::new(18.0, 26.0);
            let p = world_of(pose, local);
            let clear = marks.iter().all(|m| distance(m, local) > m.radius + 12.0);
            if !clear || (p - ball).length() < radius + 20.0 {
                continue;
            }
            assert_eq!(pixel(&painted, p), pixel(&bare, p), "{p}: nothing is painted off the path");
            off += 1;
        }
    }
    assert!(off >= 50, "{off} points off the path");
}

#[test]
fn seek_back_matches_straight_run() {
    let Some((device, queue)) = gpu() else { return };
    let (scene, resolved, kit, _) = painting(|p| {
        p.set("fade", Value::Number(3.0));
    });
    let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
    let mut renderer = Renderer::new(&device, W, H);
    for time in [6.0, 3.0] {
        draw(&device, &queue, &mut renderer, &scene, &resolved, &kit, &mut playback, time, true);
    }
    let seeked = draw(&device, &queue, &mut renderer, &scene, &resolved, &kit, &mut playback, 4.0, true);
    let mut fresh = Renderer::new(&device, W, H);
    let straight = draw(&device, &queue, &mut fresh, &scene, &resolved, &kit, &mut playback, 4.0, true);
    let bare =
        draw(&device, &queue, &mut Renderer::new(&device, W, H), &scene, &resolved, &kit, &mut playback, 4.0, false);
    assert!(straight != bare, "the canvas shows marks");
    let differ = seeked.iter().zip(&straight).filter(|(a, b)| a != b).count();
    assert_eq!(differ, 0, "{differ} bytes differ between seeking back and a straight run");
}

#[test]
fn riding_canvas_turns_its_marks() {
    let Some((device, queue)) = gpu() else { return };
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let obstacle = add_thing(&mut scene, &kit.registry, "obstacle", ThingId::ROOT).unwrap();
    let p = &mut scene.things.get_mut(&obstacle).unwrap().params;
    p.set("shape", Value::Choice("bar".into()));
    p.set("spin", Value::Number(90.0));
    let ball = scene.things.get_mut(&BALLS).unwrap();
    ball.transform.position = Vec2::new(0.0, -300.0);
    ball.params.set("velocity", Value::Vec2([0.0, 0.0]));
    let canvas = add_thing(&mut scene, &kit.registry, "canvas", obstacle).unwrap();
    // Only the bar splats here, not the ball the new canvas gave brushes to.
    scene.things.get_mut(&BALLS).unwrap().slots.remove(&SlotId::Marks);
    scene
        .set_attach(canvas, Attach::Ride { offset: Vec2::new(40.0, 0.0), turn_with_parent: true, item: None })
        .unwrap();
    let p = &mut scene.things.get_mut(&canvas).unwrap().params;
    p.set("fit", Value::Choice("custom".into()));
    p.set("size", Value::Vec2([600.0, 600.0]));
    // On top, so the ball and the bar never hide the splat.
    p.set("layer", Value::Choice("on_top".into()));
    brushes(&mut scene, &kit, obstacle, |p| {
        p.set("paint_on", Value::Bool(false));
        p.set("splat_on", Value::Bool(true));
        p.set("splat_onto", Value::Choice("canvases".into()));
        p.set("splat_min_speed", Value::Number(0.0));
        // Green, unlike the white bar under it.
        p.set("colour_from", Value::Choice("own".into()));
        p.set("colour_colours", Value::Gradient(Gradient::even(&[Color::rgb(40, 200, 90)])));
    });
    let resolved = resolve(&scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);

    let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
    let mut t = 0.0;
    while playback.log().marks().is_empty() && t < 5.0 {
        t += 0.05;
        playback.seek(t);
    }
    let first = *playback.log().marks().first().expect("the ball splats on the bar");
    let mut renderer = Renderer::new(&device, W, H);
    // Once the hit's flash, drawn over the canvas, has gone.
    let t = t + 0.4;
    let before = draw(&device, &queue, &mut renderer, &scene, &resolved, &kit, &mut playback, t, true);
    let pose = canvas_area(playback.world(), canvas).unwrap().0;
    let then = world_of(pose, first.from);
    assert!(near(pixel(&before, then), shown(&first), TONE), "{:?}: the splat is there", pixel(&before, then));

    // A second later the bar has turned a quarter turn, and the splat with it.
    let later = draw(&device, &queue, &mut renderer, &scene, &resolved, &kit, &mut playback, t + 1.0, true);
    let pose_later = canvas_area(playback.world(), canvas).unwrap().0;
    let turned = (pose_later.angle - pose.angle).abs();
    assert!((turned - std::f32::consts::FRAC_PI_2).abs() < 0.05, "turned {turned}");
    let now = world_of(pose_later, first.from);
    assert!((now - then).length() > 80.0, "the splat moved: {then} to {now}");
    assert!(
        near(pixel(&later, now), shown(&first), TONE),
        "{:?}: the splat turned with the canvas",
        pixel(&later, now)
    );
    // Where it was is clear: no later splat landed there.
    let old = Vec2::from_angle(-pose_later.angle).rotate(then - pose_later.position);
    assert!(playback.log().marks().iter().all(|m| distance(m, old) > m.radius * 1.5 + 8.0));
    assert!(!near(pixel(&later, then), shown(&first), 30), "the splat left where it was");
}

#[test]
fn fade_clears_old_marks() {
    let Some((device, queue)) = gpu() else { return };
    let (scene, resolved, kit, canvas) = painting(|p| {
        p.set("fade", Value::Number(1.0));
        p.set("layer", Value::Choice("on_top".into()));
    });
    let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
    playback.seek(2.0);
    let marks = playback.log().marks().to_vec();
    // A stroke from 0.2 s that nothing paints over after 1 s.
    let early = marks
        .iter()
        .filter(|m| (0.19..0.23).contains(&time_of(m, &playback)))
        .find(|e| {
            let p = (e.from + e.to) * 0.5;
            marks.iter().filter(|m| time_of(m, &playback) > 1.0).all(|m| distance(m, p) > m.radius + 12.0)
        })
        .copied()
        .expect("a stroke at 0.2 s left alone after 1 s");
    let pose = canvas_area(playback.world(), canvas).unwrap().0;
    let p = world_of(pose, (early.from + early.to) * 0.5);

    let mut renderer = Renderer::new(&device, W, H);
    let fresh = draw(&device, &queue, &mut renderer, &scene, &resolved, &kit, &mut playback, 0.3, true);
    let bare =
        draw(&device, &queue, &mut Renderer::new(&device, W, H), &scene, &resolved, &kit, &mut playback, 0.3, false);
    assert!(!near(pixel(&fresh, p), pixel(&bare, p), 20), "{p}: the stroke shows at first");
    let faded = draw(&device, &queue, &mut renderer, &scene, &resolved, &kit, &mut playback, 2.0, true);
    let bare =
        draw(&device, &queue, &mut Renderer::new(&device, W, H), &scene, &resolved, &kit, &mut playback, 2.0, false);
    assert_eq!(pixel(&faded, p), pixel(&bare, p), "{p}: the stroke has faded away");
}

#[test]
fn trail_edges_are_smooth() {
    let Some((device, queue)) = gpu() else { return };
    // Two balls side by side fall along the same curve, each painting a stroke twice its width,
    // so the two strokes overlap along the way, painted in the same steps. Where they meet, the
    // later paint lies on top: the line between them runs along the middle, as smooth as the
    // strokes' outer edges.
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let canvas = add_thing(&mut scene, &kit.registry, "canvas", ThingId::ROOT).unwrap();
    // On top, so the balls never hide the paint.
    scene.things.get_mut(&canvas).unwrap().params.set("layer", Value::Choice("on_top".into()));
    let balls = scene.things.get_mut(&BALLS).unwrap();
    balls.transform.position = Vec2::new(-400.0, -300.0);
    balls.params.set("arrangement", Value::Choice("block".into()));
    balls.params.set("columns", Value::Int(1));
    balls.params.set("rows", Value::Int(2));
    balls.params.set("spacing", Value::Number(60.0));
    balls.params.set("velocity", Value::Vec2([700.0, -100.0]));
    let (red, blue) = (Color::rgb(230, 60, 60), Color::rgb(60, 90, 230));
    brushes(&mut scene, &kit, BALLS, |p| {
        p.set("paint_on", Value::Bool(true));
        p.set("paint_width", Value::Number(2.0));
        p.set("colour_from", Value::Choice("own".into()));
        p.set("colour_colours", Value::Gradient(Gradient::even(&[red, blue])));
        p.set("colour_follow", Value::Choice("order".into()));
    });
    let resolved = resolve(&scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());

    let time = 0.7;
    let (w, h) = (1080, 1920);
    playback.seek(time);
    let mut frame = Frame::default();
    build_frame(&scene, &resolved, playback.world(), playback.log(), time, &kit.draws, &mut frame);
    frame.bloom = 0.0;
    let source = flow_source(&scene, &resolved, &kit.sims, None);
    let mut renderer = Renderer::new(&device, w, h);
    assert!(renderer.render_with_flow(&device, &queue, &frame, &source).caught_up);
    let pixels = renderer.read_output(&device, &queue).unwrap();
    save_sized("canvas-trail-edges", &pixels, w, h);

    let pose = canvas_area(playback.world(), canvas).unwrap().0;
    // The picture at world point `p`, blended between the four nearest pixels, so an edge's
    // place is measured finer than a pixel.
    let at = |p: Vec2| {
        let q = p + Vec2::new(540.0, 960.0) - 0.5;
        let (x, y) = (q.x.floor(), q.y.floor());
        let (fx, fy) = (q.x - x, q.y - y);
        let texel =
            |dx: usize, dy: usize, c: usize| pixels[((y as usize + dy) * w as usize + x as usize + dx) * 4 + c] as f32;
        [0, 1, 2].map(|c| {
            let top = texel(0, 0, c) * (1.0 - fx) + texel(1, 0, c) * fx;
            let bottom = texel(0, 1, c) * (1.0 - fx) + texel(1, 1, c) * fx;
            (top * (1.0 - fy) + bottom * fy).round() as i32
        })
    };
    let marks = playback.log().marks();
    let first = shown(&marks[0]);
    // How wide the first ball's stroke shows across it, every quarter of each piece along it.
    let widths: Vec<f32> = marks
        .iter()
        .filter(|m| shown(m) == first && (0.15..time - 0.15).contains(&time_of(m, &playback)))
        .flat_map(|m| (0..4).map(move |q| (m, q as f32 / 4.0)))
        .map(|(m, t)| {
            let across = (m.to - m.from).normalize().perp();
            let middle = m.from.lerp(m.to, t);
            let covered = (-400..=400)
                .filter(|k| near(at(world_of(pose, middle + across * (*k as f32 * 0.25))), first, 40))
                .count();
            covered as f32 * 0.25
        })
        .collect();
    assert!(widths.len() >= 20, "{} samples", widths.len());
    for pair in widths.windows(2) {
        assert!(
            (pair[1] - pair[0]).abs() < 1.5,
            "the stroke's width jumps from {} px to {} px: {widths:?}",
            pair[0],
            pair[1]
        );
    }
}

#[test]
fn ribbon_joints_have_no_beads() {
    let Some((device, queue)) = gpu() else { return };
    // A thin, half-transparent trail along a curve: where its pieces meet, nothing may be
    // painted twice, so the paint is as strong at a joint as halfway between two.
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let canvas = add_thing(&mut scene, &kit.registry, "canvas", ThingId::ROOT).unwrap();
    scene.things.get_mut(&canvas).unwrap().params.set("layer", Value::Choice("on_top".into()));
    scene.things.get_mut(&BALLS).unwrap().transform.position = Vec2::new(-400.0, -300.0);
    scene.things.get_mut(&BALLS).unwrap().params.set("velocity", Value::Vec2([700.0, -300.0]));
    let mut red = Color::rgb(230, 30, 30);
    red.a = 128;
    brushes(&mut scene, &kit, BALLS, |p| {
        p.set("paint_on", Value::Bool(true));
        p.set("paint_width_by", Value::Choice("fixed".into()));
        p.set("paint_width", Value::Number(0.8));
        p.set("colour_from", Value::Choice("own".into()));
        p.set("colour_colours", Value::Gradient(Gradient::even(&[red, red])));
    });
    let resolved = resolve(&scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
    let time = 0.7;
    playback.seek(time);
    let (w, h) = (1080, 1920);
    let mut renderer = Renderer::new(&device, w, h);
    let mut render = |canvases: bool| {
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), time, &kit.draws, &mut frame);
        frame.bloom = 0.0;
        if !canvases {
            frame.canvases.clear();
        }
        let source = flow_source(&scene, &resolved, &kit.sims, None);
        assert!(renderer.render_with_flow(&device, &queue, &frame, &source).caught_up);
        renderer.read_output(&device, &queue).unwrap()
    };
    let (painted, bare) = (render(true), render(false));
    save_sized("canvas-ribbon-beads", &painted, w, h);

    let pose = canvas_area(playback.world(), canvas).unwrap().0;
    // The picture at the pixel under world point `p`.
    let at = |pixels: &[u8], p: Vec2| {
        let (x, y) = ((p.x + 540.0).floor() as usize, (p.y + 960.0).floor() as usize);
        let i = (y * w as usize + x) * 4;
        [pixels[i] as f32, pixels[i + 1] as f32, pixels[i + 2] as f32]
    };
    let marks = playback.log().marks();
    // Along the middle of the ribbon, away from its ends and from the ball at its head.
    let middle = &marks[marks.len() / 5..marks.len() * 3 / 5];
    assert!(middle.len() > 20, "{} pieces", middle.len());
    // The channel the paint changes most gives the strength of the paint: 0 is the picture
    // without it, 1 would be full paint.
    let paint = [red.r as f32, red.g as f32, red.b as f32];
    let samples: Vec<[f32; 3]> = middle.iter().map(|m| at(&painted, world_of(pose, m.from))).collect();
    let bares: Vec<[f32; 3]> = middle.iter().map(|m| at(&bare, world_of(pose, m.from))).collect();
    let strength: Vec<f32> = (0..samples.len())
        .map(|k| {
            let c = (0..3)
                .max_by(|&a, &b| (paint[a] - bares[k][a]).abs().total_cmp(&(paint[b] - bares[k][b]).abs()))
                .unwrap();
            assert!((paint[c] - bares[k][c]).abs() > 60.0, "the paint shows against the ground");
            (samples[k][c] - bares[k][c]) / (paint[c] - bares[k][c])
        })
        .collect();
    let (lo, hi) = strength.iter().fold((f32::MAX, f32::MIN), |(lo, hi), s| (lo.min(*s), hi.max(*s)));
    assert!((0.3..0.9).contains(&lo), "half-transparent paint, not {lo}: {strength:?}");
    assert!(hi - lo < 0.05, "the paint's strength along the ribbon runs from {lo} to {hi}: {strength:?}");
}

// Skins: the paint arenas and obstacles carry on themselves, drawn over their own lines and
// shown only where the thing is.

/// A red that shows against the white walls and the dark background.
const SKIN_RED: Color = Color { r: 220, g: 40, b: 40, a: 255 };

/// The new scene with `things`'s changes, where the balls splat red Prints on what they hit at
/// any speed, with `brush`'s changes to their brush.
fn skin_scene(things: impl FnOnce(&mut Scene, &Kit), brush: impl FnOnce(&mut Params)) -> (Scene, Resolved, Kit) {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    things(&mut scene, &kit);
    brushes(&mut scene, &kit, BALLS, |p| {
        p.set("paint_on", Value::Bool(false));
        p.set("splat_on", Value::Bool(true));
        p.set("splat_min_speed", Value::Number(0.0));
        p.set("colour_from", Value::Choice("own".into()));
        p.set("colour_colours", Value::Gradient(Gradient::even(&[SKIN_RED])));
        brush(p);
    });
    let resolved = resolve(&scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    (scene, resolved, kit)
}

/// A box arena 600 by 800 with a 12 px wall, and six balls flying about in it.
fn box_arena(scene: &mut Scene, _: &Kit) {
    let arena = &mut scene.things.get_mut(&ARENA).unwrap().params;
    arena.set("shape", Value::Choice("box".into()));
    arena.set("width", Value::Number(600.0));
    arena.set("height", Value::Number(800.0));
    arena.set("thickness", Value::Number(12.0));
    many_balls(scene);
}

/// Six balls in a block, thrown sideways.
fn many_balls(scene: &mut Scene) {
    let balls = scene.things.get_mut(&BALLS).unwrap();
    balls.params.set("arrangement", Value::Choice("block".into()));
    balls.params.set("columns", Value::Int(3));
    balls.params.set("rows", Value::Int(2));
    balls.params.set("velocity", Value::Vec2([600.0, -200.0]));
}

const ARENA: ThingId = ThingId(1);

/// `scene` at `time`, `width` by `height` (the whole 1080×1920 video), with or without its
/// canvases and skins.
#[allow(clippy::too_many_arguments)]
fn shot(
    device: &bpe_gfx::wgpu::Device,
    queue: &bpe_gfx::wgpu::Queue,
    renderer: &mut Renderer,
    scene: &Scene,
    resolved: &Resolved,
    kit: &Kit,
    playback: &mut Playback,
    time: f64,
    canvases: bool,
) -> Vec<u8> {
    playback.seek(time);
    let mut frame = Frame::default();
    build_frame(scene, resolved, playback.world(), playback.log(), time, &kit.draws, &mut frame);
    frame.bloom = 0.0;
    if !canvases {
        frame.canvases.clear();
    }
    let source = flow_source(scene, resolved, &kit.sims, None);
    assert!(renderer.render_with_flow(device, queue, &frame, &source).caught_up);
    renderer.read_output(device, queue).unwrap()
}

/// The world points (pixel middles) where two full-size pictures differ by more than a hair.
fn painted_points(painted: &[u8], bare: &[u8]) -> Vec<Vec2> {
    let (w, h) = (1080usize, 1920usize);
    assert_eq!(painted.len(), w * h * 4);
    let mut out = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            if (0..3).any(|c| (painted[i + c] as i32 - bare[i + c] as i32).abs() > 3) {
                out.push(Vec2::new(x as f32 + 0.5 - 540.0, y as f32 + 0.5 - 960.0));
            }
        }
    }
    out
}

/// A world point in a thing's own frame at `pose`.
fn local_of(pose: Pose, p: Vec2) -> Vec2 {
    Vec2::from_angle(-pose.angle).rotate(p - pose.position)
}

/// How far `p` (in the outline's frame) is from the middle of an outline's line.
fn line_distance(outline: &Outline, p: Vec2) -> f32 {
    match *outline {
        Outline::Ring { center, radius, sides: 0, .. } => ((p - center).length() - radius).abs(),
        Outline::Box { center, half, angle, .. } => {
            let q = Vec2::from_angle(-angle).rotate(p - center).abs() - half;
            (q.max(Vec2::ZERO).length() + q.x.max(q.y).min(0.0)).abs()
        }
        other => panic!("no distance for {other:?}"),
    }
}

/// How far paint may reach from the middle of a line `thickness` wide with Skin width 1.
fn reach(thickness: f32) -> f32 {
    thickness * 0.5 * 2.0
}

/// Asserts paint shows on the arena's skin, only within its line's reach (plus a pixel), and
/// gives how many pixels it covers.
fn paint_on_the_line(painted: &[u8], bare: &[u8], playback: &Playback, name: &str) -> usize {
    let world = playback.world();
    let pose = skin_area(world, ARENA).expect("the arena has a skin").0;
    let outline = wall_local(world, ARENA).unwrap();
    let limit = reach(outline.thickness()) + 1.0;
    let points = painted_points(painted, bare);
    for p in &points {
        let d = line_distance(&outline, local_of(pose, *p));
        assert!(d <= limit, "{name}: paint at {p}, {d} px from the line, past {limit}");
    }
    points.len()
}

#[test]
fn skin_paint_stays_on_the_line() {
    let Some((device, queue)) = gpu() else { return };
    let (scene, resolved, kit) = skin_scene(box_arena, |_| {});
    let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
    playback.seek(4.0);
    let marks = playback.log().marks().len();
    assert!(marks >= 10, "{marks} prints");
    assert!(playback.log().marks().iter().all(|m| m.target == MarkTarget::Skin(ARENA)));
    let mut renderer = Renderer::new(&device, 1080, 1920);
    let painted = shot(&device, &queue, &mut renderer, &scene, &resolved, &kit, &mut playback, 4.0, true);
    let bare = shot(&device, &queue, &mut renderer, &scene, &resolved, &kit, &mut playback, 4.0, false);
    save_sized("skin-box", &painted, 1080, 1920);
    let covered = paint_on_the_line(&painted, &bare, &playback, "box");
    assert!(covered > 500, "{covered} pixels painted");
}

#[test]
fn skin_paint_on_a_peg_covers_its_body() {
    let Some((device, queue)) = gpu() else { return };
    let mut peg = ThingId::ROOT;
    let (scene, resolved, kit) = skin_scene(
        |scene, kit| {
            peg = add_thing(scene, &kit.registry, "obstacle", ThingId::ROOT).unwrap();
            scene.things.get_mut(&peg).unwrap().params.set("radius", Value::Number(60.0));
            let ball = scene.things.get_mut(&BALLS).unwrap();
            ball.transform.position = Vec2::new(20.0, -300.0);
            ball.params.set("velocity", Value::Vec2([0.0, 0.0]));
        },
        |_| {},
    );
    let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
    let time = 2.0;
    playback.seek(time);
    let marks: Vec<Mark> =
        playback.log().marks().iter().filter(|m| m.target == MarkTarget::Skin(peg)).copied().collect();
    assert!(!marks.is_empty(), "the ball splats the peg");
    let mut renderer = Renderer::new(&device, 1080, 1920);
    let painted = shot(&device, &queue, &mut renderer, &scene, &resolved, &kit, &mut playback, time, true);
    let bare = shot(&device, &queue, &mut renderer, &scene, &resolved, &kit, &mut playback, time, false);
    save_sized("skin-peg", &painted, 1080, 1920);
    let world = playback.world();
    let pose = skin_area(world, peg).expect("the peg has a skin").0;
    let Solid::Disc { center, radius } = parts_local(world, peg)[0] else { panic!("a peg is a disc") };
    let points = painted_points(&painted, &bare);
    assert!(points.len() > 300, "{} pixels painted", points.len());
    for p in &points {
        let d = (local_of(pose, *p) - center).length() - radius;
        assert!(d <= 1.0, "paint at {p}, {d} px outside the peg");
    }
    // Inside the peg, under the print's middle (pushed into it), the paint is the print's colour.
    let ball = world.bodies[&BALLS].position[0];
    let m = marks.iter().find(|m| (world_of(pose, m.from) - ball).length() > 80.0).expect("a print the ball is off");
    assert!((m.from - center).length() < radius - 5.0, "{}: the print's middle is in the peg", m.from);
    let at = world_of(pose, m.from);
    let i = (((at.y + 960.0) as usize) * 1080 + (at.x + 540.0) as usize) * 4;
    let got = [painted[i] as i32, painted[i + 1] as i32, painted[i + 2] as i32];
    assert!(near(got, shown(m), TONE), "{got:?}: the print shows over the peg");
}

#[test]
fn skin_mask_follows_a_shrinking_ring() {
    let Some((device, queue)) = gpu() else { return };
    let (scene, resolved, kit) = skin_scene(
        |scene, _| {
            let arena = &mut scene.things.get_mut(&ARENA).unwrap().params;
            arena.set("thickness", Value::Number(12.0));
            arena.set("shrink", Value::Number(0.08));
            many_balls(scene);
        },
        |_| {},
    );
    let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
    let mut renderer = Renderer::new(&device, 1080, 1920);
    let mut radii = Vec::new();
    for time in [2.0, 5.0] {
        let painted = shot(&device, &queue, &mut renderer, &scene, &resolved, &kit, &mut playback, time, true);
        let bare = shot(&device, &queue, &mut renderer, &scene, &resolved, &kit, &mut playback, time, false);
        save_sized(&format!("skin-ring-{time}"), &painted, 1080, 1920);
        let covered = paint_on_the_line(&painted, &bare, &playback, &format!("{time} s"));
        assert!(covered > 300, "{time} s: {covered} pixels painted");
        let Outline::Ring { radius, .. } = wall_local(playback.world(), ARENA).unwrap() else { panic!("a ring") };
        radii.push(radius);
    }
    assert!(radii[0] - radii[1] > 60.0, "the ring shrank: {radii:?}");
}

#[test]
fn skin_spins_with_its_bar() {
    let Some((device, queue)) = gpu() else { return };
    let mut bar = ThingId::ROOT;
    let (scene, resolved, kit) = skin_scene(
        |scene, kit| {
            bar = add_thing(scene, &kit.registry, "obstacle", ThingId::ROOT).unwrap();
            let p = &mut scene.things.get_mut(&bar).unwrap().params;
            p.set("shape", Value::Choice("bar".into()));
            p.set("thickness", Value::Number(16.0));
            p.set("spin", Value::Number(90.0));
            // Thrown down at the bar's right end.
            let ball = scene.things.get_mut(&BALLS).unwrap();
            ball.transform.position = Vec2::new(150.0, -100.0);
            ball.params.set("velocity", Value::Vec2([0.0, 1500.0]));
        },
        |_| {},
    );
    let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
    let mut t = 0.0;
    while !playback.log().marks().iter().any(|m| m.target == MarkTarget::Skin(bar)) && t < 2.0 {
        t += 0.02;
        playback.seek(t);
    }
    let first = *playback.log().marks().iter().find(|m| m.target == MarkTarget::Skin(bar)).expect("the bar is hit");
    assert!(first.from.x.abs() > 100.0, "{}: near the bar's tip", first.from);
    // The print's middle is pushed past the bar's edge; its paint shows on the bar's spine.
    let spot = Vec2::new(first.from.x, 0.0);
    let mut renderer = Renderer::new(&device, W, H);
    let mut seen = Vec::new();
    for time in [t + 0.3, t + 0.8] {
        let pixels = draw(&device, &queue, &mut renderer, &scene, &resolved, &kit, &mut playback, time, true);
        let pose = skin_area(playback.world(), bar).unwrap().0;
        let at = world_of(pose, spot);
        let ball = playback.world().bodies[&BALLS].position[0];
        assert!((at - ball).length() > 60.0, "{time} s: the ball is off the spot");
        assert!(near(pixel(&pixels, at), shown(&first), TONE), "{time} s: {:?} at {at}", pixel(&pixels, at));
        seen.push((at, pose, pixels));
    }
    let ((then, _, _), (now, pose, later)) = (&seen[0], &seen[1]);
    assert!((*now - *then).length() > 80.0, "the print turned with the bar: {then} to {now}");
    // Where it was is bare: no other print landed there.
    let old = local_of(*pose, *then);
    if playback.log().marks().iter().all(|m| distance(m, old) > m.radius * 2.6 + 8.0) {
        assert!(!near(pixel(later, *then), shown(&first), 30), "the print left where it was");
    }
}

#[test]
fn skin_fade_and_opacity() {
    let Some((device, queue)) = gpu() else { return };
    // One ball, so prints seldom land on each other.
    let (scene, resolved, kit) = skin_scene(
        |scene, kit| {
            box_arena(scene, kit);
            scene.things.get_mut(&BALLS).unwrap().params.set("arrangement", Value::Choice("single".into()));
        },
        |_| {},
    );
    let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
    playback.seek(4.0);
    let marks = playback.log().marks().to_vec();
    let rate = playback.world().steps_per_second as f64;
    // A print after the first second whose spot on the line nothing painted before, nor paints
    // over for 1.5 s.
    let outline = |w: &bpe_runtime::World| wall_local(w, ARENA).unwrap();
    let Outline::Box { half, .. } = outline(playback.world()) else { panic!("a box") };
    let on_line = |p: Vec2| {
        // The nearest point on the box's line, from inside.
        let gap = half - p.abs();
        if gap.x < gap.y { Vec2::new(half.x.copysign(p.x), p.y) } else { Vec2::new(p.x, half.y.copysign(p.y)) }
    };
    let (mark, spot) = marks
        .iter()
        .filter(|m| m.step as f64 / rate > 1.0)
        .map(|m| (*m, on_line(m.from)))
        .find(|(m, spot)| {
            let t = m.step as f64 / rate;
            marks.iter().all(|o| {
                let other = o.step as f64 / rate;
                o.step == m.step || other > t + 1.5 || distance(o, *spot) > o.radius * 2.6 + 8.0
            })
        })
        .expect("a print left alone");
    let made = mark.step as f64 / rate;
    let mut renderer = Renderer::new(&device, W, H);
    // Draws the scene with the arena's skin fade and opacity.
    let mut at = |fade: f64, opacity: f64, time: f64, canvases: bool| {
        let mut scene = scene.clone();
        let arena = &mut scene.things.get_mut(&ARENA).unwrap().params;
        arena.set("skin_fade", Value::Number(fade));
        arena.set("skin_opacity", Value::Number(opacity));
        let resolved = resolve(&scene, &kit.registry);
        let pixels = draw(&device, &queue, &mut renderer, &scene, &resolved, &kit, &mut playback, time, canvases);
        let pose = skin_area(playback.world(), ARENA).unwrap().0;
        let p = world_of(pose, spot);
        let ball = playback.world().bodies[&BALLS].position.iter().any(|b| (*b - p).length() < 60.0);
        assert!(!ball, "{time} s: a ball is on the spot");
        pixel(&pixels, p)
    };
    let half_way = made + 0.5;
    let bare = at(0.0, 1.0, half_way, false);
    let full = at(0.0, 1.0, half_way, true);
    let faded = at(1.0, 1.0, half_way, true);
    let halved = at(0.0, 0.5, half_way, true);
    assert!(near(full, shown(&mark), TONE), "{full:?}: the print at full strength");
    assert!(!near(full, bare, 60), "{full:?} on {bare:?}");
    // Half a second into a one second fade, and at half opacity, the paint is half way (in
    // linear light, give or take the tone mapping's bend on bright colours).
    let linear = |c: [i32; 3]| Color::rgb(c[0] as u8, c[1] as u8, c[2] as u8).to_linear();
    let (b, f) = (linear(bare), linear(full));
    for (name, got) in [("faded", faded), ("halved", halved)] {
        let g = linear(got);
        for c in 0..3 {
            let want = (b[c] + f[c]) * 0.5;
            assert!((g[c] - want).abs() < 0.06, "{name}: {got:?}, channel {c} is {} not {want}", g[c]);
        }
    }
    assert!(near(faded, halved, 3), "a fade half done is half the paint: {faded:?} and {halved:?}");
    // A fade done is gone.
    let gone = at(1.0, 1.0, made + 1.2, true);
    assert_eq!(gone, at(0.0, 1.0, made + 1.2, false), "the print has faded away");
}

#[test]
fn seek_back_shows_the_same_skin() {
    let Some((device, queue)) = gpu() else { return };
    let (mut scene, _, kit) = skin_scene(box_arena, |_| {});
    scene.things.get_mut(&ARENA).unwrap().params.set("skin_fade", Value::Number(3.0));
    let resolved = resolve(&scene, &kit.registry);
    let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
    let mut renderer = Renderer::new(&device, W, H);
    for time in [6.0, 3.0] {
        draw(&device, &queue, &mut renderer, &scene, &resolved, &kit, &mut playback, time, true);
    }
    let seeked = draw(&device, &queue, &mut renderer, &scene, &resolved, &kit, &mut playback, 4.0, true);
    let mut fresh = Renderer::new(&device, W, H);
    let straight = draw(&device, &queue, &mut fresh, &scene, &resolved, &kit, &mut playback, 4.0, true);
    let bare =
        draw(&device, &queue, &mut Renderer::new(&device, W, H), &scene, &resolved, &kit, &mut playback, 4.0, false);
    assert!(straight != bare, "the skin shows paint");
    let differ = seeked.iter().zip(&straight).filter(|(a, b)| a != b).count();
    assert_eq!(differ, 0, "{differ} bytes differ between seeking back and a straight run");
}

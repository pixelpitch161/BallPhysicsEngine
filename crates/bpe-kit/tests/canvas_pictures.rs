//! Picture tests for canvases on the software GPU: the graphics card replays the marks the
//! simulation logs into each canvas's textures, and seeking back redraws them exactly.

use bpe_core::{Attach, Color, Gradient, Params, Resolved, Scene, SlotId, ThingId, Value, resolve};
use bpe_gfx::{Frame, Renderer, build_frame, headless_device};
use bpe_kit::things::canvas::canvas_area;
use bpe_kit::{Kit, add_thing, fill_slot, flow_source, kit, new_scene};
use bpe_runtime::{Mark, Playback, Pose};
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
    if std::env::var_os("BPE_SAVE_RENDERS").is_none() {
        return;
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pictures");
    std::fs::create_dir_all(&dir).unwrap();
    let file = std::fs::File::create(dir.join(format!("{name}.png"))).unwrap();
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), W, H);
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

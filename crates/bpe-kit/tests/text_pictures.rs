//! The Text thing in pictures (Text part 1): a letter's hole is empty, a screen text stays put
//! when the camera moves, a lit text throws a shadow and glass letters bend what is behind them.
//! Measured on the cloud's software GPU; the checks compare pixels with each other.
//!
//! `BPE_SAVE_RENDERS=1` saves the pictures into `target/pictures/text-*.png`.

use bpe_core::{Choice, Color, Gradient, Scene, Settings, SlotId, ThingId, Value};
use bpe_gfx::{Frame, Renderer, build_frame, headless_device};
use bpe_kit::modules::look::surface::{SurfaceKind, preset_values};
use bpe_kit::paint::Paint;
use bpe_kit::{SceneSettings, ensure_camera, kit};
use bpe_runtime::Playback;

const W: u32 = 270;
const H: u32 = 480;

struct Gpu {
    device: bpe_gfx::wgpu::Device,
    queue: bpe_gfx::wgpu::Queue,
    renderer: Renderer,
}

fn gpu() -> Option<Gpu> {
    let (device, queue) = headless_device().ok()?;
    let renderer = Renderer::new(&device, W, H);
    Some(Gpu { device, queue, renderer })
}

fn draw(gpu: &mut Gpu, scene: &Scene, name: &str) -> Vec<u8> {
    let kit = kit();
    let resolved = bpe_core::resolve(scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    let mut playback = Playback::new(scene, &resolved, kit.sims.clone());
    playback.seek(0.0);
    let mut frame = Frame::default();
    build_frame(scene, &resolved, playback.world(), playback.log(), 0.0, &kit.draws, &mut frame);
    gpu.renderer.render(&gpu.device, &gpu.queue, &frame);
    if gpu.renderer.programs_waiting() {
        gpu.renderer.render(&gpu.device, &gpu.queue, &frame);
    }
    let pixels = gpu.renderer.read_output(&gpu.device, &gpu.queue).unwrap();
    if std::env::var_os("BPE_SAVE_RENDERS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pictures");
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(dir.join(format!("text-{name}.png"))).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), W, H);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(&pixels).unwrap();
    }
    pixels
}

/// The pixel at world point (x, y): the video is 1080 × 1920 round the origin.
fn at(pixels: &[u8], x: f32, y: f32) -> [u8; 4] {
    let px = (((x / 1080.0 + 0.5) * W as f32) as usize).min(W as usize - 1);
    let py = (((y / 1920.0 + 0.5) * H as f32) as usize).min(H as usize - 1);
    let i = (py * W as usize + px) * 4;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

fn brightness(p: [u8; 4]) -> u32 {
    p[0] as u32 + p[1] as u32 + p[2] as u32
}

/// A blank scene on a dark grey background with one Text saying `words` at `size`, its middle at
/// the origin, in the world or on the screen.
fn text_scene(words: &str, size: f64, screen: bool) -> (Scene, ThingId) {
    let reg = kit().registry;
    let mut scene = Scene::empty(&reg);
    ensure_camera(&mut scene, &reg);
    set_background(&mut scene, Paint::solid(Color::rgb(0x20, 0x20, 0x20)));
    let id = bpe_kit::add_thing(&mut scene, &reg, "text", ThingId::ROOT).unwrap();
    let t = scene.things.get_mut(&id).unwrap();
    t.params.set("words", Value::Text(words.into()));
    t.params.set("size", Value::Number(size));
    t.screen = Some(screen);
    (scene, id)
}

fn set_background(scene: &mut Scene, paint: Paint) {
    let root = scene.things.get_mut(&ThingId::ROOT).unwrap();
    let mut s = SceneSettings::from_params(&root.params);
    s.background = paint;
    s.bloom = 0.0;
    root.params = s.to_params();
}

fn set_look(scene: &mut Scene, thing: ThingId, key: &str, v: Value) {
    scene.card_mut(thing, SlotId::Look).unwrap().params.set(key, v);
}

/// The middle of the first letter's box, and a point on its outline's ink, in the world.
fn letter_points(scene: &Scene, id: ThingId) -> Vec<(glam::Vec2, glam::Vec2)> {
    let kit = kit();
    let resolved = bpe_core::resolve(scene, &kit.registry);
    let world = bpe_runtime::World::build(scene, &resolved, &kit.sims);
    let now = bpe_kit::things::text::text_now(&bpe_kit::things::text::TextAt::new(
        scene,
        &resolved.things[&id].params,
        &world,
        id,
        0.0,
    ))
    .unwrap();
    now.layout.letters.iter().map(|l| (now.to_world((l.min + l.max) * 0.5), now.to_world(l.min))).collect()
}

#[test]
fn a_letter_hole_is_empty_on_screen() {
    let Some(mut gpu) = gpu() else { return };
    let (scene, id) = text_scene("O", 700.0, true);
    let pixels = draw(&mut gpu, &scene, "hole");
    let (middle, _) = letter_points(&scene, id)[0];
    let hole = at(&pixels, middle.x, middle.y);
    assert!(brightness(hole) < 3 * 0x30, "the hole shows the background: {hole:?}");
    // The stroke: left of the middle by most of the way to the letter's left edge.
    let (lo, hi) = {
        let kit = kit();
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let world = bpe_runtime::World::build(&scene, &resolved, &kit.sims);
        let now = bpe_kit::things::text::text_now(&bpe_kit::things::text::TextAt::new(
            &scene,
            &resolved.things[&id].params,
            &world,
            id,
            0.0,
        ))
        .unwrap();
        now.bounds().unwrap()
    };
    let stroke_x = lo.x + (hi.x - lo.x) * 0.07;
    let stroke = at(&pixels, stroke_x, middle.y);
    assert!(brightness(stroke) > 3 * 0xe0, "the stroke is white: {stroke:?}");
}

#[test]
fn a_screen_text_does_not_move_with_the_camera() {
    let Some(mut gpu) = gpu() else { return };
    for screen in [true, false] {
        let (mut scene, _) = text_scene("I", 500.0, screen);
        let still = draw(&mut gpu, &scene, "camera-still");
        let reg = kit().registry;
        let cam = ensure_camera(&mut scene, &reg);
        scene.things.get_mut(&cam).unwrap().params.set("position", Value::Vec2([300.0, 0.0]));
        let moved = draw(&mut gpu, &scene, "camera-moved");
        let lit = |p: &[u8]| brightness(at(p, 0.0, -100.0)) > 3 * 0xc0;
        assert!(lit(&still), "the I is drawn in the middle");
        if screen {
            assert!(still == moved, "a screen text stays where it is");
        } else {
            assert!(!lit(&moved), "a world text moves with the world");
        }
    }
}

#[test]
fn a_lit_text_casts_a_shadow() {
    use bpe_kit::things::light::{LightType, add_light};
    let Some(mut gpu) = gpu() else { return };
    let (mut scene, id) = text_scene("I", 400.0, false);
    set_background(&mut scene, Paint::solid(Color::rgb(0x90, 0x90, 0x90)));
    let reg = kit().registry;
    let light = add_light(&mut scene, &reg, LightType::Point, ThingId::ROOT).unwrap();
    let l = scene.things.get_mut(&light).unwrap();
    l.params.set("position", Value::Vec2([-350.0, -50.0]));
    l.params.set("reach", Value::Number(3000.0));
    let pixels = draw(&mut gpu, &scene, "shadow");
    let (_, edge) = letter_points(&scene, id)[0];
    let (middle, _) = letter_points(&scene, id)[0];
    let half = middle.x - edge.x;
    let left = at(&pixels, middle.x - half - 60.0, middle.y);
    let right = at(&pixels, middle.x + half + 60.0, middle.y);
    assert!(brightness(right) + 30 < brightness(left), "the letter's shadow falls right: {left:?} {right:?}");
}

#[test]
fn glass_letters_bend_what_is_behind() {
    let Some(mut gpu) = gpu() else { return };
    let (mut scene, id) = text_scene("O", 900.0, false);
    let (k, w) = (Color::rgb(0x10, 0x10, 0x10), Color::rgb(0xf0, 0xf0, 0xf0));
    let stripes =
        Paint { colours: Gradient::even(&[k, w, k, w, k, w, k, w, k]), angle: 0.0, span: 1080.0, ..Paint::default() };
    set_background(&mut scene, stripes);
    let reg = kit().registry;
    scene.add_slot(&reg, ThingId::ROOT, SlotId::Lighting, bpe_kit::modules::lighting::LIGHTING).unwrap();
    set_look(&mut scene, id, "surface", Value::Choice(SurfaceKind::Matte.name().into()));
    let matte = draw(&mut gpu, &scene, "matte");
    set_look(&mut scene, id, "surface", Value::Choice(SurfaceKind::Glass.name().into()));
    for (k, v) in preset_values(SurfaceKind::Glass) {
        set_look(&mut scene, id, k, v);
    }
    let glass = draw(&mut gpu, &scene, "glass");
    let (middle, edge) = letter_points(&scene, id)[0];
    // Across the O's left stroke.
    let x0 = edge.x + (middle.x - edge.x) * 0.1;
    let differs = (0..12)
        .filter(|k| {
            let y = middle.y - 200.0 + *k as f32 * 36.0;
            let (a, b) = (at(&matte, x0, y), at(&glass, x0, y));
            brightness(a).abs_diff(brightness(b)) > 60
        })
        .count();
    assert!(differs >= 4, "glass shows the stripes behind it: {differs} of 12 differ");
}

/// How many pixels of `a` and `b` differ by more than `by` in brightness, and how many are lit in
/// either.
fn differing(a: &[u8], b: &[u8], by: u32) -> (usize, usize) {
    let (mut differ, mut lit) = (0, 0);
    for (p, q) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
        let (x, y) = (brightness([p[0], p[1], p[2], p[3]]), brightness([q[0], q[1], q[2], q[3]]));
        differ += (x.abs_diff(y) > by) as usize;
        lit += (x.max(y) > 3 * 0x60) as usize;
    }
    (differ, lit)
}

#[test]
fn fast_letters_look_like_drawn_ones() {
    let Some(mut gpu) = gpu() else { return };
    let (mut scene, id) = text_scene("BOG", 300.0, true);
    let reg = kit().registry;
    // An outline reaching past the letters too.
    let outline = scene.add_layer(&reg, id, SlotId::Look, bpe_kit::modules::look::shape::OUTLINE).unwrap();
    set_look(&mut scene, id, &bpe_core::layered_key(outline, "width"), Value::Number(8.0));
    let drawn = draw(&mut gpu, &scene, "drawn-letters");
    scene.things.get_mut(&id).unwrap().params.set("fast", Value::Choice("always".into()));
    let fast = draw(&mut gpu, &scene, "fast-letters");
    let (differ, lit) = differing(&drawn, &fast, 3 * 0x60);
    assert!(lit > 2000, "the letters are drawn: {lit}");
    assert!(differ * 20 < lit, "fast letters match drawn ones but at their edges: {differ} of {lit}");
    // The O's hole is empty in both.
    let (middle, _) = letter_points(&scene, id)[1];
    assert!(brightness(at(&fast, middle.x, middle.y)) < 3 * 0x30);
}

#[test]
fn labels_ride_their_balls() {
    let Some(mut gpu) = gpu() else { return };
    let kit = kit();
    let reg = &kit.registry;
    let mut scene = Scene::empty(reg);
    ensure_camera(&mut scene, reg);
    set_background(&mut scene, Paint::solid(Color::rgb(0x20, 0x20, 0x20)));
    let balls = bpe_kit::add_thing(&mut scene, reg, "balls", ThingId::ROOT).unwrap();
    let mut b = bpe_kit::BallsSettings::from_params(&scene.things[&balls].params);
    b.ring(4, 500.0);
    b.radius = 60.0;
    scene.things.get_mut(&balls).unwrap().params = b.to_params();
    // The balls themselves not drawn: only their labels.
    scene.things.get_mut(&balls).unwrap().slots.remove(&SlotId::Look);
    let id = bpe_kit::things::text::looks::add_text(
        &mut scene,
        reg,
        bpe_kit::things::text::looks::Look::LabelsOnBalls,
        balls,
    )
    .unwrap();
    scene.things.get_mut(&id).unwrap().params.set("size", Value::Number(150.0));
    let pixels = draw(&mut gpu, &scene, "labels");
    let resolved = bpe_core::resolve(&scene, reg);
    let world = bpe_runtime::World::build(&scene, &resolved, &kit.sims);
    let copies = bpe_kit::things::text::text_copies(&bpe_kit::things::text::TextAt::new(
        &scene,
        &resolved.things[&id].params,
        &world,
        id,
        0.0,
    ));
    assert_eq!(copies.len(), 4);
    for c in &copies {
        // Some white ink round each ball's middle.
        let p = c.pose.position;
        let ink = (-6..=6)
            .flat_map(|i| (-6..=6).map(move |j| (i, j)))
            .filter(|(i, j)| brightness(at(&pixels, p.x + *i as f32 * 8.0, p.y + *j as f32 * 8.0)) > 3 * 0xc0)
            .count();
        assert!(ink > 5, "a label on the ball at {p}: {ink}");
    }
}

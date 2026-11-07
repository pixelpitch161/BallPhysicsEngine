//! Glass and light part 2 in pictures: a laser and a light beam through a glass prism fan into
//! colours, red least bent. The expected places come from the kit's own tracer
//! (`laser::trace_split`) on the same world, so the GPU picture is checked against it. Measured on
//! the cloud's software GPU; the checks compare pixels with each other, so they hold on any GPU.
//!
//! `BPE_SAVE_RENDERS=1` saves the pictures into `target/pictures/prism-*.png`.

use bpe_core::{Choice, Color, Scene, Settings, SlotId, ThingId, Value};
use bpe_gfx::{Frame, Renderer, build_frame, headless_device};
use bpe_kit::modules::look::surface::{SurfaceKind, preset_values};
use bpe_kit::paint::Paint;
use bpe_kit::things::light::laser::{self, Meets};
use bpe_kit::{SceneSettings, kit, scenes};
use bpe_runtime::Playback;
use glam::Vec2;

const W: u32 = 270;
const H: u32 = 480;

/// The prism's size (its corners' distance from its middle) and its Rainbow split.
const PRISM: f32 = 160.0;
const RAINBOW: f32 = 2.0;
/// Bends light of the Prism chip.
const INDEX: f32 = 1.6;

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

fn frame_of(scene: &Scene) -> Frame {
    let kit = kit();
    let resolved = bpe_core::resolve(scene, &kit.registry);
    let mut playback = Playback::new(scene, &resolved, kit.sims.clone());
    playback.seek(0.0);
    let mut frame = Frame::default();
    build_frame(scene, &resolved, playback.world(), playback.log(), 0.0, &kit.draws, &mut frame);
    frame
}

fn draw(gpu: &mut Gpu, scene: &Scene, name: &str) -> Vec<u8> {
    let frame = frame_of(scene);
    gpu.renderer.render(&gpu.device, &gpu.queue, &frame);
    if gpu.renderer.programs_waiting() {
        gpu.renderer.render(&gpu.device, &gpu.queue, &frame);
    }
    let pixels = gpu.renderer.read_output(&gpu.device, &gpu.queue).unwrap();
    if std::env::var_os("BPE_SAVE_RENDERS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pictures");
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(dir.join(format!("prism-{name}.png"))).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), W, H);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(&pixels).unwrap();
    }
    pixels
}

/// The output pixel at world point `p`: the video is 1080 × 1920 round the origin.
fn pixel(p: Vec2) -> (usize, usize) {
    let px = (((p.x / 1080.0 + 0.5) * W as f32) as usize).min(W as usize - 1);
    let py = (((p.y / 1920.0 + 0.5) * H as f32) as usize).min(H as usize - 1);
    (px, py)
}

fn at(pixels: &[u8], p: Vec2) -> [i32; 3] {
    let (px, py) = pixel(p);
    let i = (py * W as usize + px) * 4;
    [pixels[i] as i32, pixels[i + 1] as i32, pixels[i + 2] as i32]
}

fn brightness(c: [i32; 3]) -> i32 {
    c[0] + c[1] + c[2]
}

fn thing_of(scene: &Scene, kind: &str) -> ThingId {
    scene.things.values().find(|t| t.kind == kind).map(|t| t.id).unwrap()
}

fn set_light(scene: &mut Scene, key: &str, v: Value) {
    let light = thing_of(scene, "light");
    scene.things.get_mut(&light).unwrap().params.set(key, v);
}

/// The light's aim: in at the prism's left face at its least deviation for the middle colour.
fn aim() -> (Vec2, Vec2) {
    let dir = Vec2::from_angle((30.0 - (INDEX * 0.5).asin().to_degrees()).to_radians());
    // The left face's middle, between the top corner and the bottom left one.
    let mid = (Vec2::new(0.0, -PRISM) + Vec2::new(-0.866 * PRISM, 0.5 * PRISM)) * 0.5;
    (mid - dir * 380.0, dir)
}

/// A dark lit room: a filled glass prism (apex up, the Prism chip with Rainbow split `RAINBOW`) in
/// the middle, the room's ring and ball out of the way, and a light of `kind` aimed into it.
fn prism_room(kind: &str) -> Scene {
    let reg = kit().registry;
    let mut scene = scenes::lit_room(&reg);
    let root = scene.things.get_mut(&ThingId::ROOT).unwrap();
    let mut s = SceneSettings::from_params(&root.params);
    s.background = Paint::solid(Color::rgb(0x18, 0x18, 0x1c));
    root.params = s.to_params();
    let lighting = scene.card_mut(ThingId::ROOT, SlotId::Lighting).unwrap();
    lighting.params.set("ambient", Value::Number(0.2));
    let ring = thing_of(&scene, "shape");
    scene.things.get_mut(&ring).unwrap().params.set("size", Value::Number(3000.0));
    let ball = thing_of(&scene, "balls");
    scene.things.get_mut(&ball).unwrap().transform.position = Vec2::new(0.0, 900.0);
    let prism = bpe_kit::add_thing(&mut scene, &reg, "shape", ThingId::ROOT).unwrap();
    let t = scene.things.get_mut(&prism).unwrap();
    t.transform.position = Vec2::ZERO;
    for (k, v) in [
        ("start", Value::Choice("polygon".into())),
        ("sides", Value::Int(3)),
        ("size", Value::Number(PRISM as f64)),
        ("fill", Value::Choice("filled".into())),
    ] {
        t.params.set(k, v);
    }
    let look = scene.card_mut(prism, SlotId::Look).unwrap();
    look.params.set("surface", Value::Choice(SurfaceKind::Prism.name().into()));
    for (k, v) in preset_values(SurfaceKind::Prism) {
        look.params.set(k, v);
    }
    look.params.set("rainbow", Value::Number(RAINBOW as f64));
    let (from, dir) = aim();
    set_light(&mut scene, "kind", Value::Choice(kind.into()));
    set_light(&mut scene, "position", Value::Vec2([from.x, from.y]));
    set_light(&mut scene, "direction", Value::Number(dir.y.atan2(dir.x).to_degrees() as f64));
    set_light(&mut scene, "strength", Value::Number(3.0));
    set_light(&mut scene, "bounces", Value::Int(8));
    set_light(&mut scene, "beam_length", Value::Number(1400.0));
    set_light(&mut scene, "at_shapes", Value::Choice("pass_through".into()));
    set_light(&mut scene, "at_balls", Value::Choice("pass_through".into()));
    set_light(&mut scene, "laser_width", Value::Number(8.0));
    set_light(&mut scene, "laser_glow", Value::Number(1.0));
    scene
}

/// The beam from `from` along `dir` as the kit traces it in `scene`'s world: the prism bends, the
/// rest passes.
fn traced(scene: &Scene, from: Vec2, dir: Vec2) -> laser::Split {
    let kit = kit();
    let resolved = bpe_core::resolve(scene, &kit.registry);
    let playback = Playback::new(scene, &resolved, kit.sims.clone());
    let world = playback.world();
    let scenery = laser::scenery(world);
    let solid: Vec<ThingId> = world.walls.iter().filter(|(_, w)| w.solid).map(|(id, _)| *id).collect();
    let meets = move |thing: ThingId, _: bool| {
        if solid.contains(&thing) { Meets::Bend { index: INDEX, rainbow: RAINBOW } } else { Meets::Pass }
    };
    laser::trace_split(&scenery, from, dir, 1400.0, 8, None, &meets, 12, laser::MOST_BEAM_SEGMENTS)
}

/// The point `d` along the last segment of `path` from its start.
fn along_last(path: &laser::Path, d: f32) -> Vec2 {
    let n = path.points.len();
    let (a, b) = (path.points[n - 2], path.points[n - 1]);
    a + (b - a).normalize() * d
}

#[test]
fn a_laser_through_a_prism_leaves_a_rainbow_behind_it() {
    let Some(mut gpu) = gpu() else { return };
    let scene = prism_room("laser");
    let (from, dir) = aim();
    let split = traced(&scene, from, dir);
    assert_eq!(split.colours.len(), 12, "the prism splits it");
    let (red, violet) = (&split.colours[0].path, &split.colours[11].path);
    assert!(red.points.len() >= 3 && violet.points.len() >= 3, "both leave the prism");
    let pixels = draw(&mut gpu, &scene, "laser");
    let (r, v) = (along_last(red, 420.0), along_last(violet, 420.0));
    let (rp, vp) = (pixel(r), pixel(v));
    let apart = (rp.0 as f32 - vp.0 as f32).hypot(rp.1 as f32 - vp.1 as f32);
    assert!(apart >= 4.0, "red and violet {apart} pixels apart");
    let (rc, vc) = (at(&pixels, r), at(&pixels, v));
    assert!(rc[0] > rc[2] + 20, "red where red goes: {rc:?}");
    assert!(vc[2] > vc[0] + 20, "violet where violet goes: {vc:?}");
    // Before the prism the beam is white.
    let before = at(&pixels, from + dir * 200.0);
    assert!(brightness(before) > brightness(at(&pixels, from + dir * 200.0 + dir.perp() * 60.0)) + 60, "{before:?}");
    assert!((before[0] - before[2]).abs() < 40, "white before the prism: {before:?}");
}

#[test]
fn a_light_beam_through_a_prism_fans_into_colours() {
    let Some(mut gpu) = gpu() else { return };
    let mut scene = prism_room("beam");
    set_light(&mut scene, "beam_width", Value::Number(80.0));
    set_light(&mut scene, "beam_softness", Value::Number(0.1));
    set_light(&mut scene, "colour", Value::Color(Color::WHITE));
    let frame = frame_of(&scene);
    let ribbons = &frame.light.as_ref().unwrap().ribbons;
    assert!(ribbons.len() > 15 * 12, "white ribbons, then each colour's: {}", ribbons.len());
    let pixels = draw(&mut gpu, &scene, "beam");
    let (from, dir) = aim();
    let bg = brightness(at(&pixels, Vec2::new(-400.0, 600.0)));
    // Before the prism: the band is lit and white, and dark beside it.
    let before = at(&pixels, from + dir * 200.0);
    assert!(brightness(before) > bg + 60, "{before:?} over {bg}");
    assert!((before[0] - before[2]).abs() < 30, "white: {before:?}");
    // Beside it only the light it throws on the background (its middle ray lights as a laser does).
    let beside = brightness(at(&pixels, from + dir * 200.0 + dir.perp() * 90.0));
    assert!(beside < brightness(before) - 150, "dimmer beside it: {beside} vs {before:?}");
    // Behind it: across the fan from its red side to its violet side.
    let split = traced(&scene, from, dir);
    let (red, violet) = (along_last(&split.colours[0].path, 380.0), along_last(&split.colours[11].path, 380.0));
    let toward_violet = (violet - red).normalize();
    let middle = (red + violet) * 0.5;
    let lit: Vec<[i32; 3]> = (-60..=60)
        .map(|k| at(&pixels, middle + toward_violet * (k as f32 * 3.0)))
        .filter(|c| brightness(*c) > 220)
        .collect();
    assert!(lit.len() > 10, "the fan is lit across: {}", lit.len());
    let (first, last) = (lit[0], lit[lit.len() - 1]);
    assert!(first[0] > first[2] + 15, "its red edge: {first:?}");
    assert!(last[2] > last[0] + 15, "its violet edge: {last:?}");
}

#[test]
fn rainbow_split_off_draws_the_laser_of_one_colour() {
    let Some(mut gpu) = gpu() else { return };
    let mut scene = prism_room("laser");
    let prism = scene.things.values().filter(|t| t.kind == "shape").map(|t| t.id).max().unwrap();
    scene.card_mut(prism, SlotId::Look).unwrap().params.set("rainbow", Value::Number(0.0));
    let many = draw(&mut gpu, &scene, "no-rainbow");
    set_light(&mut scene, "colours", Value::Int(1));
    let one = draw(&mut gpu, &scene, "one-colour");
    assert!(many == one, "no Rainbow split: the beam never splits");
    assert_eq!(frame_of(&scene).light.unwrap().lights.len(), 1);
}

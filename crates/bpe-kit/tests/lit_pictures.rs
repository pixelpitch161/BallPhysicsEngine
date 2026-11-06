//! Lighting part 1a in pictures: lamps light what is near them, balls throw shadows away from
//! them, unlit layers keep their colour and so on. Measured on the cloud's software GPU; the
//! checks compare pixels with each other, so they hold on any GPU.
//!
//! `BPE_SAVE_RENDERS=1` saves the pictures into `target/pictures/lit-*.png`.

use bpe_core::{Color, Registry, Scene, Settings, SlotId, ThingId, Value, layered_key};
use bpe_gfx::{Frame, Renderer, build_frame, headless_device};
use bpe_kit::things::light::{LightType, add_light};
use bpe_kit::{kit, scenes};
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

fn frame_of(scene: &Scene, reg: &bpe_kit::Kit, t: f64) -> Frame {
    let resolved = bpe_core::resolve(scene, &reg.registry);
    let mut playback = Playback::new(scene, &resolved, reg.sims.clone());
    playback.seek(t);
    let mut frame = Frame::default();
    build_frame(scene, &resolved, playback.world(), playback.log(), t, &reg.draws, &mut frame);
    frame
}

fn draw(gpu: &mut Gpu, scene: &Scene, name: &str) -> Vec<u8> {
    let kit = kit();
    let frame = frame_of(scene, &kit, 0.0);
    gpu.renderer.render(&gpu.device, &gpu.queue, &frame);
    if gpu.renderer.programs_waiting() {
        gpu.renderer.render(&gpu.device, &gpu.queue, &frame);
    }
    let pixels = gpu.renderer.read_output(&gpu.device, &gpu.queue).unwrap();
    if std::env::var_os("BPE_SAVE_RENDERS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pictures");
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(dir.join(format!("lit-{name}.png"))).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), W, H);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(&pixels).unwrap();
    }
    pixels
}

/// The pixel at world point (x, y): the video is 1080 × 1920 round the origin.
fn at(pixels: &[u8], x: f32, y: f32) -> [u8; 4] {
    let px = ((x / 1080.0 + 0.5) * W as f32) as usize;
    let py = ((y / 1920.0 + 0.5) * H as f32) as usize;
    let i = (py * W as usize + px) * 4;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

fn brightness(p: [u8; 4]) -> u32 {
    p[0] as u32 + p[1] as u32 + p[2] as u32
}

fn light_of(scene: &Scene) -> ThingId {
    scene.things.values().find(|t| t.kind == "light").map(|t| t.id).expect("a light")
}

fn ball_of(scene: &Scene) -> ThingId {
    scene.things.values().find(|t| t.kind == "balls").map(|t| t.id).expect("a ball")
}

fn set_light(scene: &mut Scene, key: &str, v: Value) {
    let light = light_of(scene);
    scene.things.get_mut(&light).unwrap().params.set(key, v);
}

fn set_lighting(scene: &mut Scene, key: &str, v: Value) {
    scene.things.get_mut(&ThingId::ROOT).unwrap().slots.get_mut(&SlotId::Lighting).unwrap().params.set(key, v);
}

fn reg() -> Registry {
    kit().registry
}

#[test]
fn adding_a_light_gives_the_scene_its_lighting_card() {
    let reg = reg();
    let scene = scenes::lit_room(&reg);
    let root = &scene.things[&ThingId::ROOT];
    assert_eq!(root.slots[&SlotId::Lighting].kind.as_str(), "scene.lighting");
    let kit = kit();
    let frame = frame_of(&scene, &kit, 0.0);
    let lit = frame.light.as_ref().expect("a lit frame");
    assert_eq!(lit.lights.len(), 1);
    assert_eq!(lit.lights[0].p0[0..2], [0.0, -400.0]);
}

#[test]
fn a_lamp_lights_what_is_near_it_and_the_ball_throws_a_shadow_away() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let scene = scenes::lit_room(&reg);
    let pixels = draw(&mut gpu, &scene, "room");
    let near = at(&pixels, 0.0, -400.0);
    let far = at(&pixels, 400.0, 400.0);
    assert!(brightness(near) > brightness(far) + 60, "near {near:?} far {far:?}");
    // Under the ball, away from the lamp, against the same distance beside it.
    let behind = at(&pixels, 0.0, 120.0);
    // As far from the lamp (520 px), off to the side and out of the shadow.
    let side = at(&pixels, 216.0, -400.0 + 473.0);
    assert!(brightness(behind) + 30 < brightness(side), "behind {behind:?} beside {side:?}");
}

#[test]
fn no_ambient_and_a_short_reach_leaves_the_far_corner_black() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let mut scene = scenes::lit_room(&reg);
    set_lighting(&mut scene, "ambient", Value::Number(0.0));
    set_light(&mut scene, "reach", Value::Number(300.0));
    let pixels = draw(&mut gpu, &scene, "dark");
    let far = at(&pixels, 400.0, 600.0);
    assert!(brightness(far) <= 3, "{far:?}");
    assert!(brightness(at(&pixels, 0.0, -400.0)) > 150);
}

#[test]
fn a_coloured_light_tints_and_a_lit_0_layer_keeps_its_colour() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let mut scene = scenes::lit_room(&reg);
    set_light(&mut scene, "colour", Value::Color(Color::rgb(0xff, 0x20, 0x20)));
    let red = draw(&mut gpu, &scene, "red");
    let floor = at(&red, 0.0, -300.0);
    assert!(floor[0] as u32 > floor[1] as u32 + 60, "{floor:?}");
    // The ball's body at Lit 0: as it is without any lighting.
    let ball = ball_of(&scene);
    let card = &scene.things[&ball].slots[&SlotId::Look];
    let body = card.layers[0].id;
    scene.card_mut(ball, SlotId::Look).unwrap().params.set(&layered_key(body, "lit"), Value::Number(0.0));
    let unlit_body = draw(&mut gpu, &scene, "lit0");
    let mut plain = scene.clone();
    plain.things.get_mut(&ThingId::ROOT).unwrap().slots.remove(&SlotId::Lighting);
    let without = draw(&mut gpu, &plain, "plain");
    let (a, b) = (at(&unlit_body, 0.0, 0.0), at(&without, 0.0, 0.0));
    for c in 0..3 {
        assert!((a[c] as i32 - b[c] as i32).abs() <= 2, "{a:?} vs {b:?}");
    }
}

#[test]
fn a_spot_lights_only_inside_its_cone() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let mut scene = scenes::lit_room(&reg);
    set_lighting(&mut scene, "ambient", Value::Number(0.0));
    set_light(&mut scene, "kind", Value::Choice("spot".into()));
    set_light(&mut scene, "cone", Value::Number(30.0));
    set_light(&mut scene, "cone_edge", Value::Number(2.0));
    set_light(&mut scene, "direction", Value::Number(90.0));
    let pixels = draw(&mut gpu, &scene, "spot");
    let inside = at(&pixels, 0.0, -250.0);
    let outside = at(&pixels, 250.0, -300.0);
    assert!(brightness(inside) > 100, "{inside:?}");
    assert!(brightness(outside) <= 3, "{outside:?}");
}

#[test]
fn a_light_riding_each_ball_is_one_light_per_ball() {
    let reg = reg();
    let mut scene = scenes::lit_room(&reg);
    let ball = ball_of(&scene);
    let mut balls = bpe_kit::BallsSettings::from_params(&scene.things[&ball].params);
    balls.ring(6, 200.0);
    scene.things.get_mut(&ball).unwrap().params = balls.to_params();
    add_light(&mut scene, &reg, LightType::Point, ball).unwrap();
    let kit = kit();
    let frame = frame_of(&scene, &kit, 0.0);
    assert_eq!(frame.light.as_ref().unwrap().lights.len(), 1 + 6);
}

/// Writes the lighting demo scene to `BPE_DEMO_SCENE` (for the review video), when set.
#[test]
fn write_the_demo_scene() {
    let Some(path) = std::env::var_os("BPE_DEMO_SCENE") else { return };
    let reg = reg();
    std::fs::write(path, bpe_core::save::save(&scenes::lit_demo(&reg))).unwrap();
}

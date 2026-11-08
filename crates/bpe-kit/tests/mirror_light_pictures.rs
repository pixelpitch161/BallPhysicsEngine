//! Glass and light part 4a: mirrors throw light (Aedan approved the glass and light design,
//! 2026-11-07). A chrome wall throws the lamp's light where the reflection says, rough metal
//! throws a softer spot, a mirror ball throws spots that turn with it and shows its tiles, a Stage
//! mirror ball throws spots onto the wall, and a light pipe glows along its outline. Plan:
//! `docs/superpowers/plans/glass-light/part4.md`.
//!
//! Measured on the cloud's software GPU at 270 × 480 (4 world units a pixel), in linear light,
//! against the same scene throwing nothing. `BPE_SAVE_RENDERS=1` saves the pictures into
//! `target/pictures/mirror-*.png`.

use bpe_core::{Choice, Color, Gradient, Registry, Scene, Settings, SlotId, ThingId, Value, layered_key};
use bpe_gfx::{Frame, LightShow, Renderer, build_frame, test_device};
use bpe_kit::modules::look::layers::TURNING;
use bpe_kit::modules::look::surface::{SurfaceKind, preset_values};
use bpe_kit::{ShapeFill, ShapeSettings, kit, scenes};
use bpe_runtime::Playback;
use glam::Vec2;

const W: u32 = 270;
const H: u32 = 480;

struct Gpu {
    device: bpe_gfx::wgpu::Device,
    queue: bpe_gfx::wgpu::Queue,
    renderer: Renderer,
}

fn gpu() -> Option<Gpu> {
    let (device, queue) = test_device().ok()?;
    let renderer = Renderer::new(&device, W, H);
    Some(Gpu { device, queue, renderer })
}

/// The light alone (Light only), so the things' own colours don't count.
fn draw(gpu: &mut Gpu, scene: &Scene, name: &str) -> Vec<u8> {
    draw_as(gpu, scene, name, LightShow::LightOnly)
}

fn draw_as(gpu: &mut Gpu, scene: &Scene, name: &str, show: LightShow) -> Vec<u8> {
    let kit = kit();
    let resolved = bpe_core::resolve(scene, &kit.registry);
    let mut playback = Playback::new(scene, &resolved, kit.sims.clone());
    playback.seek(0.0);
    let mut frame = Frame::default();
    build_frame(scene, &resolved, playback.world(), playback.log(), 0.0, &kit.draws, &mut frame);
    frame.light.as_mut().expect("a lit frame").show = show;
    for _ in 0..2 {
        gpu.renderer.render(&gpu.device, &gpu.queue, &frame);
        if !gpu.renderer.programs_waiting() {
            break;
        }
    }
    let pixels = gpu.renderer.read_output(&gpu.device, &gpu.queue).unwrap();
    if std::env::var_os("BPE_SAVE_RENDERS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pictures");
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(dir.join(format!("mirror-{name}.png"))).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), W, H);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(&pixels).unwrap();
    }
    pixels
}

fn linear(v: u8) -> f32 {
    let c = v as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// The light reaching world point (x, y), grey in linear light, the mean of the 3 × 3 pixels round
/// it: the video is 1080 × 1920 round the origin.
fn light(pixels: &[u8], x: f32, y: f32) -> f32 {
    let px = ((x / 1080.0 + 0.5) * W as f32) as i32;
    let py = ((y / 1920.0 + 0.5) * H as f32) as i32;
    let mut sum = 0.0;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (qx, qy) = ((px + dx).clamp(0, W as i32 - 1), (py + dy).clamp(0, H as i32 - 1));
            let i = (qy as usize * W as usize + qx as usize) * 4;
            for c in 0..3 {
                sum += linear(pixels[i + c]) / 27.0;
            }
        }
    }
    sum
}

/// The one pixel at world point (x, y), grey in linear light.
fn pixel(pixels: &[u8], x: f32, y: f32) -> f32 {
    let px = (((x / 1080.0 + 0.5) * W as f32) as usize).min(W as usize - 1);
    let py = (((y / 1920.0 + 0.5) * H as f32) as usize).min(H as usize - 1);
    let i = (py * W as usize + px) * 4;
    (0..3).map(|c| linear(pixels[i + c])).sum::<f32>() / 3.0
}

fn reg() -> Registry {
    kit().registry
}

fn id_of(scene: &Scene, kind: &str) -> ThingId {
    scene.things.values().find(|t| t.kind == kind).map(|t| t.id).expect("a thing of the kind")
}

fn set_lighting(scene: &mut Scene, key: &str, v: Value) {
    scene.card_mut(ThingId::ROOT, SlotId::Lighting).unwrap().params.set(key, v);
}

fn set_light(scene: &mut Scene, key: &str, v: Value) {
    let light = id_of(scene, "light");
    scene.things.get_mut(&light).unwrap().params.set(key, v);
}

fn set_look(scene: &mut Scene, thing: ThingId, key: &str, v: Value) {
    scene.card_mut(thing, SlotId::Look).unwrap().params.set(key, v);
}

fn set_surface(scene: &mut Scene, thing: ThingId, kind: SurfaceKind) {
    set_look(scene, thing, "surface", Value::Choice(kind.name().into()));
    for (k, v) in preset_values(kind) {
        set_look(scene, thing, k, v);
    }
}

/// The lit room with no ambient light, the ball out of the way, the lamp at `lamp` (size 24,
/// reach 2000), and the room's shape a filled Chrome wall `width` × 40 at the middle that blocks
/// light.
fn mirror_room(reg: &Registry, lamp: Vec2, width: f32) -> Scene {
    let mut scene = scenes::lit_room(reg);
    let ball = id_of(&scene, "balls");
    scene.things.get_mut(&ball).unwrap().transform.position = Vec2::new(0.0, 900.0);
    set_light(&mut scene, "position", Value::Vec2(lamp.into()));
    set_light(&mut scene, "reach", Value::Number(2000.0));
    set_lighting(&mut scene, "ambient", Value::Number(0.0));
    set_lighting(&mut scene, "quality", Value::Choice("good".into()));
    let id = id_of(&scene, "shape");
    let thing = scene.things.get_mut(&id).unwrap();
    thing.params = ShapeSettings { fill: ShapeFill::Filled, ..ShapeSettings::rectangle(width, 40.0, 2.0) }.to_params();
    thing.transform.position = Vec2::ZERO;
    let body = scene.things[&id].slots[&SlotId::Look].layers[0].id;
    set_look(&mut scene, id, &layered_key(body, "blocks_light"), Value::Bool(true));
    set_surface(&mut scene, id, SurfaceKind::Chrome);
    scene
}

/// The scene with the shape's Throws light set to `throws`.
fn throwing(scene: &Scene, throws: f64) -> Scene {
    let mut scene = scene.clone();
    let id = id_of(&scene, "shape");
    set_look(&mut scene, id, "throws", Value::Number(throws));
    scene
}

/// Where the line from `image` (the lamp's mirror image) through `p` crosses the wall's top
/// (y = -20): the reflected light at `p` came off the wall there.
fn crossing(image: Vec2, p: Vec2) -> Option<f32> {
    if p.y >= -20.0 {
        return None;
    }
    let t = (image.y + 20.0) / (image.y - p.y);
    Some(image.x + t * (p.x - image.x))
}

#[test]
fn a_flat_mirror_throws_light_where_the_reflection_says() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let lamp = Vec2::new(-300.0, -400.0);
    let scene = mirror_room(&reg, lamp, 300.0);
    let on = draw(&mut gpu, &scene, "flat");
    assert!(gpu.renderer.bent_photons() > 0, "the mirror sends photons");
    let off = draw(&mut gpu, &throwing(&scene, 0.0), "flat-off");
    // The lamp's image across the wall's top.
    let image = Vec2::new(lamp.x, -40.0 - lamp.y);
    let (mut inside, mut outside) = (0, 0);
    let mut y = -880.0;
    while y <= -80.0 {
        let mut x = -500.0;
        while x <= 500.0 {
            let p = Vec2::new(x, y);
            // Away from the lamp's own bright middle.
            if p.distance(lamp) > 80.0
                && let Some(cross) = crossing(image, p)
            {
                let (a, b) = (light(&on, x, y), light(&off, x, y));
                if cross.abs() < 100.0 && a < 0.9 {
                    // What a lamp at the image would give there (the lamp's strength 1, falloff as
                    // lighting.wgsl's for a point light of reach 2000).
                    let r = p.distance(image) / 2000.0;
                    let image_gives = (1.0 - r.powi(4)).max(0.0).powi(2) / (1.0 + 16.0 * r * r);
                    let thrown = a - b;
                    assert!(
                        thrown >= 0.6 * image_gives && thrown <= 1.5 * image_gives,
                        "inside the cone at ({x}, {y}): {thrown} thrown where the lamp's image gives {image_gives}"
                    );
                    inside += 1;
                } else if cross.abs() > 260.0
                    && p.distance(Vec2::new(-150.0, -20.0)).min(p.distance(Vec2::new(150.0, -20.0))) > 200.0
                {
                    // The wall's corners are rounded by a pixel's antialiasing, so a weak fan of
                    // light leaves each corner past the cone's edges.
                    assert!((a - b).abs() <= 0.08 * b + 0.002, "outside the cone at ({x}, {y}): {a} against {b}");
                    outside += 1;
                }
            }
            x += 40.0;
        }
        y += 40.0;
    }
    assert!(inside >= 12 && outside >= 20, "{inside} inside, {outside} outside");
}

#[test]
fn nothing_throws_with_throws_light_or_mirror_light_zero() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let scene = mirror_room(&reg, Vec2::new(-300.0, -400.0), 300.0);
    let none = draw(&mut gpu, &throwing(&scene, 0.0), "throws-0");
    assert_eq!(gpu.renderer.bent_photons(), 0);
    let mut dark = scene.clone();
    set_lighting(&mut dark, "mirror_light", Value::Number(0.0));
    let unlit = draw(&mut gpu, &dark, "mirror-light-0");
    assert_eq!(gpu.renderer.bent_photons(), 0);
    assert!(none == unlit, "Throws light 0 and Mirror light 0 draw the same");
    // A Matte wall throws nothing at any Throws light.
    let mut matte = scene.clone();
    let id = id_of(&matte, "shape");
    set_surface(&mut matte, id, SurfaceKind::Matte);
    let a = draw(&mut gpu, &throwing(&matte, 1.0), "matte-1");
    assert_eq!(gpu.renderer.bent_photons(), 0);
    let b = draw(&mut gpu, &throwing(&matte, 0.0), "matte-0");
    assert!(a == b, "Matte throws nothing");
}

#[test]
fn rough_metal_throws_a_softer_spot() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let lamp = Vec2::new(-300.0, -400.0);
    // A narrow mirror, so its thrown light is a beam a blur can spread.
    let chrome = mirror_room(&reg, lamp, 60.0);
    let mut rough = chrome.clone();
    let id = id_of(&rough, "shape");
    set_surface(&mut rough, id, SurfaceKind::Metal);
    set_look(&mut rough, id, "roughness", Value::Number(0.6));
    set_look(&mut rough, id, "throws", Value::Number(1.0));
    let off = draw(&mut gpu, &throwing(&chrome, 0.0), "narrow-off");
    // Across the thrown beam 400 past the wall.
    let image = Vec2::new(lamp.x, -40.0 - lamp.y);
    let way = (Vec2::new(0.0, -20.0) - image).normalize();
    let middle = Vec2::new(0.0, -20.0) + way * 400.0;
    let across = way.perp();
    let profile = |pixels: &[u8]| -> Vec<f32> {
        (-60..=60)
            .map(|k| {
                let p = middle + across * (k as f32 * 4.0);
                (light(pixels, p.x, p.y) - light(&off, p.x, p.y)).max(0.0)
            })
            .collect()
    };
    let sharp = profile(&draw(&mut gpu, &chrome, "narrow-chrome"));
    let soft = profile(&draw(&mut gpu, &rough, "narrow-rough"));
    let peak = |p: &[f32]| p.iter().copied().fold(0.0f32, f32::max);
    let width = |p: &[f32]| {
        let half = 0.5 * peak(p);
        p.iter().filter(|v| **v >= half).count()
    };
    assert!(peak(&sharp) > 0.01, "chrome throws a beam ({})", peak(&sharp));
    assert!(peak(&soft) < peak(&sharp), "rough peak {} against chrome {}", peak(&soft), peak(&sharp));
    assert!(width(&soft) > width(&sharp), "rough {} wide against chrome {}", width(&soft), width(&sharp));
}

/// The lit room with a Mirror ball of radius `radius` at the middle (its Body alone), lit by the
/// lamp at (-450, 0), and the room's shape out of the way.
fn ball_room(reg: &Registry, radius: f64, facets: f64) -> Scene {
    let mut scene = scenes::lit_room(reg);
    set_light(&mut scene, "position", Value::Vec2([-450.0, 0.0]));
    set_light(&mut scene, "reach", Value::Number(2000.0));
    set_lighting(&mut scene, "ambient", Value::Number(0.0));
    set_lighting(&mut scene, "quality", Value::Choice("good".into()));
    let shape = id_of(&scene, "shape");
    scene.things.get_mut(&shape).unwrap().params.set("size", Value::Number(3000.0));
    let ball = id_of(&scene, "balls");
    let t = scene.things.get_mut(&ball).unwrap();
    t.params.set("radius", Value::Number(radius));
    t.params.set("velocity", Value::Vec2([0.0, 0.0]));
    t.transform.position = Vec2::ZERO;
    scene.card_mut(ball, SlotId::Look).unwrap().layers.retain(|l| l.kind == "layer.body");
    let body = scene.things[&ball].slots[&SlotId::Look].layers[0].id;
    set_look(&mut scene, ball, &layered_key(body, "blocks_light"), Value::Bool(true));
    let white = Color::rgb(255, 255, 255);
    set_look(&mut scene, ball, &layered_key(body, "colours"), Value::Gradient(Gradient::even(&[white, white])));
    set_surface(&mut scene, ball, SurfaceKind::MirrorBall);
    set_look(&mut scene, ball, "facets", Value::Number(facets));
    scene
}

fn ball_throwing(scene: &Scene, throws: f64) -> Scene {
    let mut scene = scene.clone();
    let id = id_of(&scene, "balls");
    set_look(&mut scene, id, "throws", Value::Number(throws));
    scene
}

/// The thrown light round the circle of radius `r` on the lamp's side (from straight up, round
/// through the lamp, to straight down), a degree a sample.
fn round_the_lamps_side(on: &[u8], off: &[u8], r: f32) -> Vec<f32> {
    (90..=270)
        .map(|deg| {
            let a = (deg as f32).to_radians();
            let (x, y) = (r * a.cos(), -r * a.sin());
            (light(on, x, y) - light(off, x, y)).max(0.0)
        })
        .collect()
}

fn peaks(thrown: &[f32]) -> usize {
    let top = thrown.iter().copied().fold(0.0f32, f32::max);
    let mut count = 0;
    let mut high = false;
    for v in thrown {
        let now = *v > 0.05 * top;
        if now && !high {
            count += 1;
        }
        high = now;
    }
    count
}

#[test]
fn a_mirror_ball_throws_spots_that_turn_with_it() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let scene = ball_room(&reg, 80.0, 24.0);
    let off = draw(&mut gpu, &ball_throwing(&scene, 0.0), "ball-off");
    let on = draw(&mut gpu, &scene, "ball");
    assert!(gpu.renderer.bent_photons() > 0, "the mirror ball sends photons");
    let thrown = round_the_lamps_side(&on, &off, 300.0);
    let n = peaks(&thrown);
    assert!((6..=18).contains(&n), "{n} spots round the lamp's side: {thrown:?}");
    // Turned by half a tile (7.5 degrees), the spots move.
    let mut turned = scene.clone();
    let ball = id_of(&turned, "balls");
    let layer = turned.add_layer(&reg, ball, SlotId::Look, TURNING).unwrap();
    set_look(&mut turned, ball, &layered_key(layer, "turn"), Value::Number(7.5));
    set_look(&mut turned, ball, &layered_key(layer, "spin"), Value::Number(0.0));
    let moved = round_the_lamps_side(&draw(&mut gpu, &turned, "ball-turned"), &off, 300.0);
    let differs = thrown.iter().zip(&moved).filter(|(a, b)| (**a - **b).abs() > 0.3 * a.max(**b)).count();
    assert!(differs >= 20, "{differs} degrees changed when the ball turned");
}

#[test]
fn a_mirror_ball_shows_its_tiles() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let mut scene = ball_room(&reg, 300.0, 12.0);
    let ball = id_of(&scene, "balls");
    set_look(&mut scene, ball, "facet_gaps", Value::Number(0.3));
    set_lighting(&mut scene, "ambient", Value::Number(0.4));
    let pixels = draw_as(&mut gpu, &scene, "tiles", LightShow::Picture);
    // Along a line across the ball (a pixel a sample), count the dips under half their neighbours.
    let row: Vec<f32> = (-70..=70).map(|k| pixel(&pixels, k as f32 * 4.0, -60.0)).collect();
    let mut dips = 0;
    let mut k = 3;
    while k + 3 < row.len() {
        let around = row[k - 3].max(row[k + 3]);
        if row[k] < 0.5 * around && row[k] <= row[k - 1] && row[k] <= row[k + 1] {
            dips += 1;
            k += 3;
        } else {
            k += 1;
        }
    }
    assert!(dips >= 4, "{dips} gaps between tiles: {row:?}");
}

#[test]
fn a_stage_mirror_ball_throws_spots_on_the_wall() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let mut scene = ball_room(&reg, 80.0, 24.0);
    set_lighting(&mut scene, "depth", Value::Choice("stage".into()));
    set_lighting(&mut scene, "floor", Value::Choice("none".into()));
    set_light(&mut scene, "position", Value::Vec2([-300.0, -300.0]));
    set_light(&mut scene, "height", Value::Number(300.0));
    let on = draw(&mut gpu, &scene, "stage-ball");
    let off = draw(&mut gpu, &ball_throwing(&scene, 0.0), "stage-ball-off");
    // Spots: places on the wall at least half as bright again as the bare wall.
    let mut spots = 0;
    let mut y = -900.0f32;
    while y <= 900.0 {
        let mut x = -520.0f32;
        while x <= 520.0 {
            if x.hypot(y) > 120.0 {
                let (a, b) = (light(&on, x, y), light(&off, x, y));
                if b > 0.002 && a >= 1.5 * b {
                    spots += 1;
                }
            }
            x += 8.0;
        }
        y += 8.0;
    }
    assert!(spots >= 6, "{spots} places on the wall lit by the mirror ball's spots");
    // Shadows Off: the ball throws nothing.
    let mut unshadowed = scene.clone();
    set_lighting(&mut unshadowed, "shadow_style", Value::Choice("off".into()));
    let a = draw(&mut gpu, &unshadowed, "stage-ball-no-shadows");
    let b = draw(&mut gpu, &ball_throwing(&unshadowed, 0.0), "stage-ball-no-shadows-off");
    assert!(a == b, "with shadows Off a mirror ball throws nothing");
}

/// A hollow ring shape of radius 200 and band 12 that blocks light, round the lamp at the middle.
fn pipe_room(reg: &Registry, strength: f64) -> Scene {
    let mut scene = scenes::lit_room(reg);
    let ball = id_of(&scene, "balls");
    scene.things.get_mut(&ball).unwrap().transform.position = Vec2::new(0.0, 900.0);
    set_light(&mut scene, "position", Value::Vec2([0.0, 0.0]));
    set_light(&mut scene, "reach", Value::Number(1500.0));
    set_light(&mut scene, "strength", Value::Number(strength));
    set_lighting(&mut scene, "ambient", Value::Number(0.0));
    set_lighting(&mut scene, "quality", Value::Choice("good".into()));
    let id = id_of(&scene, "shape");
    let thing = scene.things.get_mut(&id).unwrap();
    thing.params = ShapeSettings::circle(200.0, 12.0).to_params();
    thing.transform.position = Vec2::ZERO;
    let body = scene.things[&id].slots[&SlotId::Look].layers[0].id;
    set_look(&mut scene, id, &layered_key(body, "blocks_light"), Value::Bool(true));
    set_surface(&mut scene, id, SurfaceKind::Glass);
    set_look(&mut scene, id, "see_through", Value::Number(0.0));
    scene
}

fn piped(scene: &Scene, on: bool) -> Scene {
    let mut scene = scene.clone();
    let id = id_of(&scene, "shape");
    set_look(&mut scene, id, "light_pipe", Value::Bool(on));
    scene
}

#[test]
fn a_light_pipe_glows_along_its_outline() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let scene = pipe_room(&reg, 2.0);
    let on = draw(&mut gpu, &piped(&scene, true), "pipe");
    let off = draw(&mut gpu, &piped(&scene, false), "pipe-off");
    for (x, y) in [(0.0, 300.0), (300.0, 0.0), (-220.0, -220.0)] {
        let (a, b) = (light(&on, x, y), light(&off, x, y));
        assert!(a >= 1.2 * b && a > b + 0.005, "outside the pipe at ({x}, {y}): {a} against {b}");
    }
    // No lamp: the pipe has nothing to carry.
    let dark = pipe_room(&reg, 0.0);
    let a = draw(&mut gpu, &piped(&dark, true), "pipe-dark");
    let b = draw(&mut gpu, &piped(&dark, false), "pipe-dark-off");
    assert!(a == b, "a pipe with no light glows with nothing");
}

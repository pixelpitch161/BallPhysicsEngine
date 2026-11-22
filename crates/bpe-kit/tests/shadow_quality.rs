//! Shadow quality (review W2, plan `docs/superpowers/plans/lighting/shadow-quality.md`): no bright lines inside
//! shadows (V1), Area and Strip lights' shadows from all of the light (V2), even shadows of thin shapes on a Stage's
//! back wall (V3) and shadow edges that move smoothly with a ball (V4). V5 (the preview's field) is
//! `bpe_gfx::lighting::field_divisor`'s own test.
//!
//! Drawn at 270 × 480 (4 world units a pixel), the light alone (Light only) with no ambient light. The checks
//! compare pixels with each other, so they hold on any GPU. `BPE_SAVE_RENDERS=1` saves the pictures into
//! `target/pictures/shadow-*.png`.

use bpe_core::{Registry, Scene, Settings, SlotId, ThingId, Value};
use bpe_gfx::{Frame, LightShow, Renderer, build_frame, test_device};
use bpe_kit::things::balls::BallsSettings;
use bpe_kit::things::light::{LightSettings, LightType};
use bpe_kit::{ShapeSettings, add_thing, kit, scenes};
use bpe_runtime::Playback;
use glam::Vec2;

const W: u32 = 270;
const H: u32 = 480;

struct Gpu {
    device: bpe_gfx::wgpu::Device,
    queue: bpe_gfx::wgpu::Queue,
    renderer: Renderer,
}

/// The picture tests' device (`None` only when `BPE_SKIP_GPU_TESTS` skips them on purpose).
fn gpu() -> Option<Gpu> {
    let (device, queue) = test_device().ok()?;
    let mut renderer = Renderer::new(&device, W, H);
    // As the editor's preview: a field texel covers as much of the scene as in the 1080-wide video (V5), so these
    // small pictures show each Quality's shadows as the video has them.
    renderer.preview_field_like_video(true);
    Some(Gpu { device, queue, renderer })
}

/// The light alone at `time`.
fn draw_at(gpu: &mut Gpu, scene: &Scene, time: f64, name: &str) -> Vec<u8> {
    let kit = kit();
    let resolved = bpe_core::resolve(scene, &kit.registry);
    let mut playback = Playback::new(scene, &resolved, kit.sims.clone());
    playback.seek(time);
    let mut frame = Frame::default();
    build_frame(scene, &resolved, playback.world(), playback.log(), time, &kit.draws, &mut frame);
    frame.light.as_mut().expect("a lit frame").show = LightShow::LightOnly;
    gpu.renderer.render(&gpu.device, &gpu.queue, &frame);
    let pixels = gpu.renderer.read_output(&gpu.device, &gpu.queue).unwrap();
    if std::env::var_os("BPE_SAVE_RENDERS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pictures");
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(dir.join(format!("shadow-{name}.png"))).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), W, H);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(&pixels).unwrap();
    }
    pixels
}

fn draw(gpu: &mut Gpu, scene: &Scene, name: &str) -> Vec<u8> {
    draw_at(gpu, scene, 0.0, name)
}

/// How bright pixel (`px`, `py`) is, 0 to 765.
fn pixel(pixels: &[u8], px: usize, py: usize) -> u32 {
    let i = (py * W as usize + px) * 4;
    pixels[i] as u32 + pixels[i + 1] as u32 + pixels[i + 2] as u32
}

/// The pixel under world point (x, y): the video is 1080 × 1920 round the origin.
fn pixel_at(x: f32, y: f32) -> (usize, usize) {
    (((x / 1080.0 + 0.5) * W as f32) as usize, ((y / 1920.0 + 0.5) * H as f32) as usize)
}

/// How bright the pixel at world point (x, y) is, 0 to 765.
fn at(pixels: &[u8], x: f32, y: f32) -> u32 {
    let (px, py) = pixel_at(x, y);
    pixel(pixels, px, py)
}

fn reg() -> Registry {
    kit().registry
}

fn id_of(scene: &Scene, kind: &str) -> ThingId {
    scene.things.values().find(|t| t.kind == kind).map(|t| t.id).expect("a thing of the kind")
}

/// The lit room, no gravity, no ambient light, at `quality`, its light as `light` says and its ball still at `ball`
/// with `radius`.
fn room(reg: &Registry, quality: &str, light: &LightSettings, ball: Vec2, radius: f32) -> Scene {
    let mut scene = scenes::lit_room(reg);
    scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, 0.0]));
    let id = id_of(&scene, "balls");
    let t = scene.things.get_mut(&id).unwrap();
    let mut s = BallsSettings::from_params(&t.params);
    s.radius = radius;
    s.velocity = Vec2::ZERO;
    t.params = s.to_params();
    t.transform.position = ball;
    let id = id_of(&scene, "light");
    scene.things.get_mut(&id).unwrap().params = light.to_params();
    let lighting = scene.card_mut(ThingId::ROOT, SlotId::Lighting).unwrap();
    lighting.params.set("ambient", Value::Number(0.0));
    lighting.params.set("quality", Value::Choice(quality.into()));
    lighting.params.set("shadow_length", Value::Number(4000.0));
    scene
}

fn lamp(kind: LightType, at: Vec2, size: f32) -> LightSettings {
    let mut p = LightSettings::of_kind(kind);
    p.position = at;
    p.size = size;
    p.reach = 3000.0;
    p.strength = 2.0;
    p
}

/// V1: a block of balls under a low lamp. Rays that skim a ball or run along the gaps used up their steps and
/// counted the rest of the way as open: bright lines inside the shadows on Draft and Good. Ultra had steps enough;
/// the others must now match it (no pixel much brighter than Ultra's).
#[test]
fn rows_of_balls_leave_no_bright_lines_in_their_shadows() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let scene_at = |quality: &str| {
        let mut s = room(&reg, quality, &lamp(LightType::Point, Vec2::new(-100.0, -450.0), 12.0), Vec2::ZERO, 22.0);
        let id = id_of(&s, "balls");
        let t = s.things.get_mut(&id).unwrap();
        let mut b = BallsSettings::from_params(&t.params);
        b.block(6, 6, 90.0);
        t.params = b.to_params();
        t.transform.position = Vec2::new(0.0, 150.0);
        s
    };
    let ultra = draw(&mut gpu, &scene_at("ultra"), "balls-ultra");
    for quality in ["draft", "good"] {
        let pixels = draw(&mut gpu, &scene_at(quality), &format!("balls-{quality}"));
        let mut brighter = 0;
        for py in 0..H as usize {
            for px in 0..W as usize {
                if pixel(&pixels, px, py) > pixel(&ultra, px, py) + 120 {
                    brighter += 1;
                }
            }
        }
        assert!(brighter <= 20, "{quality}: {brighter} pixels much brighter than Ultra's (light leaking into shadows)");
    }
}

/// V2: a ball under a wide light. Its shadow narrows to a point and its soft edge widens with distance, so far
/// below the ball most of the light is back; the light used to be traced to its nearest point as if it were a
/// small bulb, and the shadow ran on as a dark band.
#[test]
fn a_wide_light_casts_a_shadow_that_narrows_to_a_point() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let mut area = lamp(LightType::Area, Vec2::new(0.0, -420.0), 10.0);
    area.width = 360.0;
    area.length = 20.0;
    area.direction = 0.0;
    let mut strip = lamp(LightType::Strip, Vec2::new(0.0, -420.0), 4.0);
    strip.length = 360.0;
    strip.width = 4.0;
    strip.direction = 0.0;
    for (name, light) in [("area", area), ("strip", strip)] {
        for quality in ["draft", "good"] {
            let open = draw(&mut gpu, &room(&reg, quality, &light, Vec2::new(0.0, 900.0), 60.0), "open");
            let shaded = draw(
                &mut gpu,
                &room(&reg, quality, &light, Vec2::new(0.0, -150.0), 60.0),
                &format!("{name}-{quality}"),
            );
            let share = |x: f32, y: f32| at(&shaded, x, y) as f32 / at(&open, x, y).max(1) as f32;
            // Just below the ball every part of the light is hidden.
            assert!(share(0.0, -60.0) < 0.15, "{name} {quality}: under the ball {}", share(0.0, -60.0));
            // Far below it, past the point, most of the light is back.
            assert!(share(0.0, 400.0) > 0.5, "{name} {quality}: far below the ball {}", share(0.0, 400.0));
            // And the shadow there is wider and softer than the ball: a little is still missing well to the side.
            assert!(share(160.0, 400.0) < 0.97, "{name} {quality}: beside the far shadow {}", share(160.0, 400.0));
        }
    }
}

/// V2: a wide light right by the ring: nothing leaks out through the ring (its march used to stop a lamp's Size
/// short of the light's nearest point, and the fans of light got out past thin walls).
#[test]
fn a_wide_light_inside_the_ring_stays_inside() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let mut area = lamp(LightType::Area, Vec2::new(0.0, -420.0), 10.0);
    area.width = 360.0;
    area.length = 20.0;
    area.direction = 0.0;
    for quality in ["draft", "good", "ultra"] {
        let pixels =
            draw(&mut gpu, &room(&reg, quality, &area, Vec2::new(0.0, 900.0), 20.0), &format!("ring-{quality}"));
        for (x, y) in [(0.0, -700.0), (-300.0, -650.0), (300.0, -650.0), (-480.0, -450.0), (480.0, -450.0)] {
            let b = at(&pixels, x, y);
            assert!(b <= 3, "{quality}: light leaks out of the ring to ({x}, {y}): {b}");
        }
        assert!(at(&pixels, 0.0, -300.0) > 300, "{quality}: inside the ring is lit");
    }
}

/// V3: a thin hollow box standing out of a Stage's back wall, lit from above in front. Its shadow on the wall used
/// to come out in stripes (rays through its thin walls let about half the light through, those stepping past them
/// all of it; on a small picture, dotted bright lines). Across the shadow, the light must now change smoothly: along
/// a row, it goes down and up again about once, not up and down many times.
#[test]
fn a_thin_shape_throws_an_even_shadow_on_a_stage() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    // Inside the hollow hexagon's shadow on the back wall (clear of the shape itself and the ring's shadow), each
    // row's brightness goes up and down: stripes make it go up and down much more than Ultra's.
    let travel = |pixels: &[u8]| -> (i64, i64) {
        let (x0, y0) = pixel_at(-200.0, -60.0);
        let (x1, y1) = pixel_at(160.0, 80.0);
        let mut travel = 0;
        let mut range = 0;
        for py in y0..y1 {
            let row: Vec<i64> = (x0..x1).map(|px| pixel(pixels, px, py) as i64).collect();
            travel += row.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<i64>();
            range += row.iter().max().unwrap() - row.iter().min().unwrap();
        }
        (travel, range)
    };
    let mut seen = Vec::new();
    for quality in ["ultra", "draft", "good"] {
        let mut light = lamp(LightType::Point, Vec2::new(-300.0, -550.0), 20.0);
        light.height = 400.0;
        let mut s = room(&reg, quality, &light, Vec2::new(250.0, 450.0), 40.0);
        let lighting = s.card_mut(ThingId::ROOT, SlotId::Lighting).unwrap();
        lighting.params.set("depth", Value::Choice("stage".into()));
        lighting.params.set("looking_down", Value::Number(16.0));
        let id = add_thing(&mut s, &reg, "shape", ThingId::ROOT).unwrap();
        let t = s.things.get_mut(&id).unwrap();
        t.params = ShapeSettings::polygon(6, 120.0, 3.0).to_params();
        t.transform.position = Vec2::new(-120.0, -150.0);
        seen.push((quality, travel(&draw(&mut gpu, &s, &format!("stage-{quality}")))));
    }
    eprintln!("stage (travel, range): {seen:?}");
    for &(quality, (travel, range)) in &seen {
        assert!(range > 5000, "{quality}: the rows cross the shadow: {seen:?}");
        // (Down and up again about once a row: under 3 times the range. With the stripes, 4.6 times.)
        assert!(travel <= 3 * range, "{quality}: stripes in a thin shape's shadow on a stage: {seen:?}");
    }
}

/// V4: a ball moving slowly across, drawn a frame at a time: its shadow's edge far from it moves on evenly, not in
/// jumps of a field texel.
#[test]
fn a_shadow_edge_moves_on_smoothly() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let mut s =
        room(&reg, "good", &lamp(LightType::Point, Vec2::new(-30.0, -330.0), 6.0), Vec2::new(-60.0, -100.0), 50.0);
    let id = id_of(&s, "balls");
    let t = s.things.get_mut(&id).unwrap();
    let mut b = BallsSettings::from_params(&t.params);
    b.velocity = Vec2::new(30.0, 0.0);
    t.params = b.to_params();
    // Where the shadow's left edge crosses a row far below the ball, to a fraction of a pixel.
    let edge = |pixels: &[u8], py: usize| -> f32 {
        let row: Vec<f32> = (0..W as usize / 2).map(|px| pixel(pixels, px, py) as f32).collect();
        let (lo, hi) = row.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
        let mid = 0.5 * (lo + hi);
        let k = row.windows(2).position(|w| w[0] > mid && w[1] <= mid).expect("an edge");
        k as f32 + (row[k] - mid) / (row[k] - row[k + 1]).max(1e-3)
    };
    let (_, py) = pixel_at(0.0, 400.0);
    let edges: Vec<f32> = (0..16).map(|i| edge(&draw_at(&mut gpu, &s, 0.25 + i as f64 / 30.0, "moving"), py)).collect();
    let steps: Vec<f32> = edges.windows(2).map(|w| w[1] - w[0]).collect();
    let mean = steps.iter().sum::<f32>() / steps.len() as f32;
    let spread = (steps.iter().map(|s| (s - mean) * (s - mean)).sum::<f32>() / steps.len() as f32).sqrt();
    assert!(mean > 0.1, "the edge moves on: {edges:?}");
    assert!(steps.iter().all(|&s| s > -0.05), "the edge never steps back: {steps:?}");
    assert!(spread < 0.5 * mean, "the edge moves evenly: steps {steps:?} (mean {mean}, spread {spread})");
}

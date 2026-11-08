//! Thin things that block light block it whole in Flat lighting (Aedan, 2026-11-07: "thin lines
//! leak a lot of light through them"). A hollow ring a fraction of a pixel thick used to let all
//! of a lamp's light through at Good, and one a couple of pixels thick about two thirds of it:
//! the field missed lines thinner than a texel, and the soft shadow's estimate let half the light
//! through anything thinner than its soft edge. Plan: `docs/superpowers/plans/lighting/thin-blockers.md`.
//!
//! Measured on the cloud's software GPU at 270 × 480 (4 world units a pixel). The checks compare
//! pixels with each other, so they hold on any GPU. `BPE_SAVE_RENDERS=1` saves the pictures into
//! `target/pictures/thin-*.png`.

use bpe_core::{Registry, Scene, Settings, SlotId, ThingId, Value, layered_key};
use bpe_gfx::{Frame, LightShow, Renderer, build_frame, test_device};
use bpe_kit::{ShapeSettings, kit, scenes};
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

/// The light alone (Light only), so the shape's own colour doesn't count.
fn draw(gpu: &mut Gpu, scene: &Scene, name: &str) -> Vec<u8> {
    let kit = kit();
    let resolved = bpe_core::resolve(scene, &kit.registry);
    let mut playback = Playback::new(scene, &resolved, kit.sims.clone());
    playback.seek(0.0);
    let mut frame = Frame::default();
    build_frame(scene, &resolved, playback.world(), playback.log(), 0.0, &kit.draws, &mut frame);
    frame.light.as_mut().expect("a lit frame").show = LightShow::LightOnly;
    gpu.renderer.render(&gpu.device, &gpu.queue, &frame);
    let pixels = gpu.renderer.read_output(&gpu.device, &gpu.queue).unwrap();
    if std::env::var_os("BPE_SAVE_RENDERS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pictures");
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(dir.join(format!("thin-{name}.png"))).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), W, H);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(&pixels).unwrap();
    }
    pixels
}

/// How bright the pixel at world point (x, y) is (0 to 765): the video is 1080 × 1920 round the
/// origin.
fn at(pixels: &[u8], x: f32, y: f32) -> u32 {
    let px = ((x / 1080.0 + 0.5) * W as f32) as usize;
    let py = ((y / 1920.0 + 0.5) * H as f32) as usize;
    let i = (py * W as usize + px) * 4;
    pixels[i] as u32 + pixels[i + 1] as u32 + pixels[i + 2] as u32
}

fn reg() -> Registry {
    kit().registry
}

fn id_of(scene: &Scene, kind: &str) -> ThingId {
    scene.things.values().find(|t| t.kind == kind).map(|t| t.id).expect("a thing of the kind")
}

/// The lit room with no ambient light, the ball out of the way, the lamp at `lamp` and the shape
/// given `shape`, blocking light or not.
fn room(reg: &Registry, shape: ShapeSettings, blocks: bool, quality: &str, lamp: [f32; 2]) -> Scene {
    let mut scene = scenes::lit_room(reg);
    let ball = id_of(&scene, "balls");
    scene.things.get_mut(&ball).unwrap().transform.position = Vec2::new(0.0, 900.0);
    let light = id_of(&scene, "light");
    let params = &mut scene.things.get_mut(&light).unwrap().params;
    params.set("position", Value::Vec2(lamp));
    params.set("reach", Value::Number(1500.0));
    let lighting = scene.things.get_mut(&ThingId::ROOT).unwrap().slots.get_mut(&SlotId::Lighting).unwrap();
    lighting.params.set("ambient", Value::Number(0.0));
    lighting.params.set("quality", Value::Choice(quality.into()));
    let id = id_of(&scene, "shape");
    scene.things.get_mut(&id).unwrap().params = shape.to_params();
    let body = scene.things[&id].slots[&SlotId::Look].layers[0].id;
    scene.card_mut(id, SlotId::Look).unwrap().params.set(&layered_key(body, "blocks_light"), Value::Bool(blocks));
    scene
}

/// Round the ring, outside it (radius 300), at all sorts of angles.
const OUTSIDE: [(f32, f32); 8] = [
    (400.0, 0.0),
    (-400.0, 0.0),
    (0.0, -400.0),
    (0.0, 420.0),
    (290.0, 290.0),
    (-300.0, -310.0),
    (360.0, 120.0),
    (-150.0, 380.0),
];

#[test]
fn a_thin_hollow_ring_keeps_a_lamp_inside_it_in() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    for quality in ["draft", "good", "ultra"] {
        let open = draw(&mut gpu, &room(&reg, ShapeSettings::circle(300.0, 2.0), false, quality, [0.0, 0.0]), "open");
        for &(x, y) in &OUTSIDE {
            assert!(at(&open, x, y) > 400, "{quality}: without the ring the lamp lights ({x}, {y})");
        }
        // Half a pixel thick and up.
        for thickness in [2.0, 4.0, 8.0, 16.0] {
            let ring = ShapeSettings::circle(300.0, thickness);
            let name = format!("ring-{quality}-{thickness}");
            let pixels = draw(&mut gpu, &room(&reg, ring, true, quality, [0.0, 0.0]), &name);
            for &(x, y) in &OUTSIDE {
                let b = at(&pixels, x, y);
                assert!(b <= 3, "{quality}, {thickness} thick: light leaks out to ({x}, {y}): {b}");
            }
            // Inside, the lamp still lights right up to the ring.
            assert!(at(&pixels, 0.0, 280.0) > 400, "{quality}, {thickness} thick: inside by the ring");
            assert!(at(&pixels, 270.0, 0.0) > 400, "{quality}, {thickness} thick: inside by the ring");
        }
    }
}

/// A thin wall (a flat box 2 world units thick) under a big lamp: dark behind its middle, lit past
/// its ends, the soft edge between them smooth (no step where the rays start crossing the wall),
/// and the side facing the lamp lit right up to the wall.
#[test]
fn a_thin_wall_throws_a_whole_shadow_with_a_soft_edge() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    for quality in ["good", "ultra"] {
        let mut scene = room(&reg, ShapeSettings::rectangle(400.0, 6.0, 2.0), true, quality, [0.0, -400.0]);
        let light = id_of(&scene, "light");
        scene.things.get_mut(&light).unwrap().params.set("size", Value::Number(120.0));
        let pixels = draw(&mut gpu, &scene, &format!("wall-{quality}"));
        assert!(at(&pixels, 0.0, 300.0) <= 3, "{quality}: behind the wall's middle");
        assert!(at(&pixels, -100.0, 200.0) <= 3, "{quality}: behind the wall");
        assert!(at(&pixels, 520.0, 300.0) > 150, "{quality}: well past the wall's end");
        // The lamp's side, right by the wall: lit (a field texel's coarseness doesn't darken it).
        let open = draw(
            &mut gpu,
            &room(&reg, ShapeSettings::rectangle(400.0, 6.0, 2.0), false, quality, [0.0, -400.0]),
            "wall-open",
        );
        for x in [-150.0, -60.0, 0.0, 90.0] {
            let (lit, bare) = (at(&pixels, x, -12.0), at(&open, x, -12.0));
            assert!(lit + 20 >= bare, "{quality}: the lamp's side by the wall at {x}: {lit} against {bare}");
        }
        // Along a row behind the wall, out past the shadow's edge (at x = 350): from dark to lit,
        // never a jump of more than a soft step from one pixel to the next.
        let row: Vec<u32> = (0..=80).map(|i| at(&pixels, 200.0 + i as f32 * 4.0, 300.0)).collect();
        assert!(row[0] <= 3 && *row.last().unwrap() > 150, "{quality}: {row:?}");
        for w in row.windows(2) {
            assert!(w[0].abs_diff(w[1]) <= 40, "{quality}: a step in the soft edge: {row:?}");
        }
    }
}

/// A thin wall turned to all sorts of slants (so rays meet the field's texels anywhere, not square
/// on), at every Quality: dark all along behind it.
#[test]
fn a_slanted_thin_wall_blocks_wherever_the_rays_meet_it() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let lamp = Vec2::new(0.0, -400.0);
    for quality in ["draft", "good", "ultra"] {
        for degrees in [7.0f32, 23.0, 41.0, 58.0] {
            let mut scene = room(&reg, ShapeSettings::rectangle(500.0, 6.0, 2.0), true, quality, lamp.to_array());
            let shape = id_of(&scene, "shape");
            scene.things.get_mut(&shape).unwrap().transform.rotation = degrees.to_radians();
            let pixels = draw(&mut gpu, &scene, &format!("slant-{quality}-{degrees}"));
            let along = Vec2::from_angle(degrees.to_radians());
            for s in [-150.0, -90.0, -37.0, 0.0, 41.0, 95.0, 160.0] {
                // A point on the wall, and points beyond it seen from the lamp.
                let on = along * s;
                for k in [1.15f32, 1.5, 1.9] {
                    let q = lamp + (on - lamp) * k;
                    let b = at(&pixels, q.x, q.y);
                    assert!(b <= 3, "{quality}, {degrees}°: light leaks through at ({}, {}): {b}", q.x, q.y);
                }
            }
        }
    }
}

/// A lamp right beside a thin wall (40 world units off it, its own size 24: the shadow march
/// stops that far short of the lamp): its own side lit, the far side dark.
#[test]
fn a_lamp_beside_a_thin_wall_lights_only_its_own_side() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    for quality in ["draft", "good", "ultra"] {
        let scene = room(&reg, ShapeSettings::rectangle(700.0, 6.0, 2.0), true, quality, [0.0, -40.0]);
        let pixels = draw(&mut gpu, &scene, &format!("beside-{quality}"));
        for (x, y) in [(0.0, 60.0), (150.0, 80.0), (-220.0, 120.0), (40.0, 300.0)] {
            let b = at(&pixels, x, y);
            assert!(b <= 3, "{quality}: the far side at ({x}, {y}): {b}");
        }
        for (x, y) in [(0.0, -80.0), (150.0, -80.0), (-220.0, -120.0), (60.0, -12.0)] {
            let b = at(&pixels, x, y);
            assert!(b > 150, "{quality}: the lamp's side at ({x}, {y}): {b}");
        }
    }
}

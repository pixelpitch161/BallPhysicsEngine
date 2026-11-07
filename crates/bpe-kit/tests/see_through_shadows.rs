//! See-through things let light through in Flat lighting (Aedan, 2026-11-07: "make it so things
//! let through more the more transparent they are"). A blocker passes the share of a lamp's light
//! it is see-through: a Surface's See-through, or how far from opaque it is drawn; light crossing
//! two passes each one's share in turn, and the shadow keeps its soft edge. Solid things still block
//! whole (`thin_blockers.rs`). Plan: `docs/superpowers/plans/lighting/see-through-blockers.md`.
//!
//! Measured on the cloud's software GPU at 270 × 480 (4 world units a pixel), in linear light
//! against the same scene with the blocker not blocking. `BPE_SAVE_RENDERS=1` saves the pictures
//! into `target/pictures/through-*.png`.

use bpe_core::{Registry, Scene, Settings, SlotId, ThingId, Value, layered_key};
use bpe_gfx::{Frame, LightShow, Renderer, build_frame, headless_device};
use bpe_kit::{ShapeFill, ShapeSettings, add_thing, kit, scenes};
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
    let (device, queue) = headless_device().ok()?;
    let renderer = Renderer::new(&device, W, H);
    Some(Gpu { device, queue, renderer })
}

/// The light alone (Light only), so the things' own colours don't count.
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
        let file = std::fs::File::create(dir.join(format!("through-{name}.png"))).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), W, H);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(&pixels).unwrap();
    }
    pixels
}

/// How much light reaches world point (x, y), in linear light (the mean of its channels, 0 to 1):
/// the video is 1080 × 1920 round the origin.
fn light(pixels: &[u8], x: f32, y: f32) -> f32 {
    let px = ((x / 1080.0 + 0.5) * W as f32) as usize;
    let py = ((y / 1920.0 + 0.5) * H as f32) as usize;
    let i = (py * W as usize + px) * 4;
    let linear = |v: u8| {
        let c = v as f32 / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    (linear(pixels[i]) + linear(pixels[i + 1]) + linear(pixels[i + 2])) / 3.0
}

fn reg() -> Registry {
    kit().registry
}

fn id_of(scene: &Scene, kind: &str) -> ThingId {
    scene.things.values().find(|t| t.kind == kind).map(|t| t.id).expect("a thing of the kind")
}

/// The lit room with no ambient light, the ball out of the way, the lamp at `lamp` (size 24) and
/// the shape given `shape`, blocking light or not, at See-through `see`.
fn room(reg: &Registry, shape: ShapeSettings, blocks: bool, see: f32, quality: &str, lamp: [f32; 2]) -> Scene {
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
    let look = scene.card_mut(id, SlotId::Look).unwrap();
    look.params.set(&layered_key(body, "blocks_light"), Value::Bool(blocks));
    look.params.set("see_through", Value::Number(see.into()));
    scene
}

/// A filled wall `width` wide and 40 thick across the middle.
fn wall(width: f32) -> ShapeSettings {
    ShapeSettings { fill: ShapeFill::Filled, ..ShapeSettings::rectangle(width, 40.0, 2.0) }
}

/// Behind the wall, seen from a lamp at (0, -400).
const BEHIND: [(f32, f32); 4] = [(0.0, 250.0), (-120.0, 300.0), (90.0, 200.0), (150.0, 350.0)];

/// The light at each point against the same scene with nothing blocking, `open`.
fn shares(pixels: &[u8], open: &[u8], points: &[(f32, f32)]) -> Vec<f32> {
    points.iter().map(|&(x, y)| light(pixels, x, y) / light(open, x, y).max(1e-4)).collect()
}

#[test]
fn a_see_through_wall_lets_its_share_of_the_light_through() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    for quality in ["draft", "good", "ultra"] {
        let open = draw(&mut gpu, &room(&reg, wall(600.0), false, 0.0, quality, [0.0, -400.0]), "wall-open");
        for &(x, y) in &BEHIND {
            assert!(light(&open, x, y) > 0.05, "{quality}: the lamp lights ({x}, {y}) with nothing in the way");
        }
        for see in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
            let scene = room(&reg, wall(600.0), true, see, quality, [0.0, -400.0]);
            let pixels = draw(&mut gpu, &scene, &format!("wall-{quality}-{see}"));
            for (k, s) in shares(&pixels, &open, &BEHIND).into_iter().enumerate() {
                assert!((s - see).abs() <= 0.06, "{quality}, see-through {see}: {s} of the light at {:?}", BEHIND[k]);
            }
        }
    }
}

/// A hollow ring 2 world units thick (half a pixel) round the lamp at See-through ½: half the
/// light gets out, all round, at every Quality.
#[test]
fn a_thin_see_through_ring_lets_half_out() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let outside = [(400.0, 0.0), (-400.0, 0.0), (0.0, -400.0), (0.0, 420.0), (290.0, 290.0), (-300.0, -310.0)];
    for quality in ["draft", "good", "ultra"] {
        let ring = || ShapeSettings::circle(300.0, 2.0);
        let open = draw(&mut gpu, &room(&reg, ring(), false, 0.5, quality, [0.0, 0.0]), "ring-open");
        let pixels = draw(&mut gpu, &room(&reg, ring(), true, 0.5, quality, [0.0, 0.0]), &format!("ring-{quality}"));
        for (k, s) in shares(&pixels, &open, &outside).into_iter().enumerate() {
            assert!((s - 0.5).abs() <= 0.06, "{quality}: {s} of the light gets out at {:?}", outside[k]);
        }
    }
}

/// Two half see-through rings, one inside the other: a quarter of the light gets past both.
#[test]
fn light_through_two_see_through_things_takes_each_share() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let outside = [(400.0, 0.0), (-400.0, 0.0), (0.0, 420.0), (290.0, 290.0)];
    for quality in ["good", "ultra"] {
        let two = |blocks: bool| {
            let mut scene = room(&reg, ShapeSettings::circle(300.0, 6.0), blocks, 0.5, quality, [0.0, 0.0]);
            let inner = add_thing(&mut scene, &reg, "shape", ThingId::ROOT).unwrap();
            scene.things.get_mut(&inner).unwrap().params = ShapeSettings::circle(200.0, 6.0).to_params();
            let body = scene.things[&inner].slots[&SlotId::Look].layers[0].id;
            let look = scene.card_mut(inner, SlotId::Look).unwrap();
            look.params.set(&layered_key(body, "blocks_light"), Value::Bool(blocks));
            look.params.set("see_through", Value::Number(0.5));
            scene
        };
        let open = draw(&mut gpu, &two(false), "two-open");
        let pixels = draw(&mut gpu, &two(true), &format!("two-{quality}"));
        for (k, s) in shares(&pixels, &open, &outside).into_iter().enumerate() {
            assert!((s - 0.25).abs() <= 0.05, "{quality}: {s} of the light gets past both at {:?}", outside[k]);
        }
    }
}

/// A big ball whose Body is drawn at half opacity, between the lamp and the floor: about half the
/// light behind it.
#[test]
fn a_half_opaque_ball_lets_half_the_light_through() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    for quality in ["draft", "good", "ultra"] {
        let scene = |blocks: bool| {
            let mut scene = room(&reg, ShapeSettings::circle(30.0, 2.0), false, 0.0, quality, [0.0, -400.0]);
            let shape = id_of(&scene, "shape");
            scene.things.get_mut(&shape).unwrap().transform.position = Vec2::new(0.0, 900.0);
            let ball = id_of(&scene, "balls");
            let thing = scene.things.get_mut(&ball).unwrap();
            thing.transform.position = Vec2::ZERO;
            thing.params.set("radius", Value::Number(120.0));
            let body = scene.things[&ball].slots[&SlotId::Look].layers[0].id;
            let look = scene.card_mut(ball, SlotId::Look).unwrap();
            look.params.set(&layered_key(body, "blocks_light"), Value::Bool(blocks));
            look.params.set(&layered_key(body, "opacity"), Value::Number(0.5));
            scene
        };
        let open = draw(&mut gpu, &scene(false), "ball-open");
        let pixels = draw(&mut gpu, &scene(true), &format!("ball-{quality}"));
        let behind = [(0.0, 300.0), (-40.0, 400.0), (40.0, 250.0)];
        for (k, s) in shares(&pixels, &open, &behind).into_iter().enumerate() {
            assert!((s - 0.5).abs() <= 0.06, "{quality}: {s} of the light behind the ball at {:?}", behind[k]);
        }
    }
}

/// Past the end of a half see-through wall under a big lamp, the shadow's soft edge runs smoothly
/// from half the light to all of it.
#[test]
fn a_see_through_shadow_keeps_a_smooth_soft_edge() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    for quality in ["good", "ultra"] {
        let make = |blocks: bool| {
            let mut scene = room(&reg, wall(300.0), blocks, 0.5, quality, [0.0, -400.0]);
            let light = id_of(&scene, "light");
            scene.things.get_mut(&light).unwrap().params.set("size", Value::Number(120.0));
            scene
        };
        let open = draw(&mut gpu, &make(false), "edge-open");
        let pixels = draw(&mut gpu, &make(true), &format!("edge-{quality}"));
        // Along a row behind the wall, out past its shadow's edge (the wall ends at x = 150, its
        // corner's shadow falls at x = 276; the picture ends at 540).
        let row: Vec<f32> = (0..=90)
            .map(|i| {
                let x = 150.0 + i as f32 * 4.0;
                light(&pixels, x, 300.0) / light(&open, x, 300.0).max(1e-4)
            })
            .collect();
        assert!((row[0] - 0.5).abs() <= 0.06 && *row.last().unwrap() > 0.95, "{quality}: {row:?}");
        for w in row.windows(2) {
            assert!((w[0] - w[1]).abs() <= 0.06, "{quality}: a step in the soft edge: {row:?}");
        }
    }
}

/// A thing drawn through a blend (a shape with a Blend layer goes through its own layer picture)
/// is lit on its own pixels as one drawn straight in is: it doesn't shadow itself.
#[test]
fn a_blended_thing_doesnt_shadow_itself() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    for quality in ["draft", "good"] {
        let plain = room(&reg, wall(600.0), true, 0.0, quality, [0.0, -400.0]);
        let mut blended = plain.clone();
        let shape = id_of(&blended, "shape");
        blended.add_layer(&reg, shape, SlotId::Look, "shape.blend").unwrap();
        let a = draw(&mut gpu, &plain, "self-plain");
        let b = draw(&mut gpu, &blended, &format!("self-blended-{quality}"));
        for x in [-200.0, -50.0, 0.0, 120.0, 250.0] {
            let (lit, want) = (light(&b, x, 0.0), light(&a, x, 0.0));
            assert!(want > 0.05, "{quality}: the wall's own pixels are lit at {x}");
            assert!((lit - want).abs() <= 0.1 * want, "{quality}: blended wall at {x}: {lit} against {want}");
        }
    }
}

/// On a Stage with Coloured light through glass off, a see-through thing still blocks whole (the
/// Stage's shadows have maths of their own), clear glass too. With it on (the default) a
/// see-through shape lets its share through in its colour (`coloured_shadows.rs`).
#[test]
fn a_stage_keeps_see_through_blockers_whole() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let stage = |see: f32| {
        let mut scene = room(&reg, wall(600.0), true, see, "good", [0.0, -400.0]);
        let lighting = scene.things.get_mut(&ThingId::ROOT).unwrap().slots.get_mut(&SlotId::Lighting).unwrap();
        lighting.params.set("depth", Value::Choice("stage".into()));
        lighting.params.set("glass_colour", Value::Bool(false));
        scene
    };
    let solid = draw(&mut gpu, &stage(0.0), "stage-solid");
    for see in [0.5f32, 0.99, 1.0] {
        let pixels = draw(&mut gpu, &stage(see), &format!("stage-{see}"));
        assert!(pixels == solid, "see-through {see} draws the Stage as a solid wall does");
    }
}

//! Glass and light part 3: lamp light bent by glass shapes (Aedan approved the glass and light
//! design, 2026-11-07). Glass that bends next to nothing gives the light part 1 gave, one pane
//! behind another isn't counted twice, a glass circle focuses light to a bright line and a
//! biconcave lens spreads it, Rainbow split gives coloured edges, things that don't bend send no
//! photons, and a lens on a Stage focuses on the wall. Plan:
//! `docs/superpowers/plans/glass-light/part3.md`.
//!
//! Measured on the cloud's software GPU at 270 × 480 (4 world units a pixel), in linear light,
//! against the same scene with the glass not there (or not bending). `BPE_SAVE_RENDERS=1` saves
//! the pictures into `target/pictures/bent-*.png`.

use bpe_core::{Params, Registry, Scene, Settings, ShapePath, SlotId, ThingId, Value, layered_key};
use bpe_gfx::{Frame, LightShow, Renderer, build_frame, test_device};
use bpe_kit::modules::look::surface::SurfaceKind as Surface;
use bpe_kit::things::particles::liquid::{LIQUID, LiquidLayer, params_of};
use bpe_kit::things::particles::look::{SPRITES, SpritesLayer};
use bpe_kit::things::particles::looks::{Look, add_particles};
use bpe_kit::things::particles::{BornFrom, Emit, ParticlesSettings};
use bpe_kit::{ShapeFill, ShapeSettings, ShapeStart, add_thing, kit, scenes};
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
    draw_at(gpu, scene, name, 0.0)
}

/// [`draw`] at `t` seconds.
fn draw_at(gpu: &mut Gpu, scene: &Scene, name: &str, t: f64) -> Vec<u8> {
    let kit = kit();
    let resolved = bpe_core::resolve(scene, &kit.registry);
    let mut playback = Playback::new(scene, &resolved, kit.sims.clone());
    playback.seek(t);
    let mut frame = Frame::default();
    build_frame(scene, &resolved, playback.world(), playback.log(), t, &kit.draws, &mut frame);
    frame.light.as_mut().expect("a lit frame").show = LightShow::LightOnly;
    gpu.renderer.render(&gpu.device, &gpu.queue, &frame);
    let pixels = gpu.renderer.read_output(&gpu.device, &gpu.queue).unwrap();
    if std::env::var_os("BPE_SAVE_RENDERS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pictures");
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(dir.join(format!("bent-{name}.png"))).unwrap();
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

/// The light reaching world point (x, y), per channel in linear light, the mean of the 3 × 3
/// pixels round it: the video is 1080 × 1920 round the origin.
fn light(pixels: &[u8], x: f32, y: f32) -> [f32; 3] {
    let px = ((x / 1080.0 + 0.5) * W as f32) as i32;
    let py = ((y / 1920.0 + 0.5) * H as f32) as i32;
    let mut sum = [0.0; 3];
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (qx, qy) = ((px + dx).clamp(0, W as i32 - 1), (py + dy).clamp(0, H as i32 - 1));
            let i = (qy as usize * W as usize + qx as usize) * 4;
            for c in 0..3 {
                sum[c] += linear(pixels[i + c]) / 9.0;
            }
        }
    }
    sum
}

fn grey(c: [f32; 3]) -> f32 {
    (c[0] + c[1] + c[2]) / 3.0
}

fn reg() -> Registry {
    kit().registry
}

fn id_of(scene: &Scene, kind: &str) -> ThingId {
    scene.things.values().find(|t| t.kind == kind).map(|t| t.id).expect("a thing of the kind")
}

fn set_lighting(scene: &mut Scene, key: &str, v: Value) {
    let lighting = scene.things.get_mut(&ThingId::ROOT).unwrap().slots.get_mut(&SlotId::Lighting).unwrap();
    lighting.params.set(key, v);
}

/// A filled wall `width` wide and 40 thick.
fn wall(width: f32) -> ShapeSettings {
    ShapeSettings { fill: ShapeFill::Filled, ..ShapeSettings::rectangle(width, 40.0, 2.0) }
}

/// A filled glass circle of radius `r`.
fn disc(r: f32) -> ShapeSettings {
    ShapeSettings { fill: ShapeFill::Filled, ..ShapeSettings::circle(r, 2.0) }
}

/// A filled outline through `points` (world units round the shape's place).
fn outline(points: &[[f32; 2]]) -> ShapeSettings {
    let far = points.iter().map(|p| p[0].hypot(p[1])).fold(1.0f32, f32::max);
    let size = far.log2().ceil().exp2();
    let path = ShapePath {
        points: points.iter().map(|p| [p[0] / size, p[1] / size]).collect(),
        bends: vec![None; points.len()],
    };
    ShapeSettings {
        start: ShapeStart::Polygon,
        sides: points.len() as u32,
        size,
        thickness: 2.0,
        path,
        fill: ShapeFill::Filled,
        ..ShapeSettings::default()
    }
}

/// A biconcave lens: thin in the middle (30) and thick at its sides (80), 200 across.
fn biconcave() -> ShapeSettings {
    outline(&[[-100.0, -40.0], [0.0, -15.0], [100.0, -40.0], [100.0, 40.0], [0.0, 15.0], [-100.0, 40.0]])
}

/// Shape `id` as `shape` at `at`, blocking light, See-through `see` and Bends light `bends`.
fn set_glass(scene: &mut Scene, id: ThingId, shape: ShapeSettings, at: Vec2, see: f32, bends: f32) {
    let thing = scene.things.get_mut(&id).unwrap();
    thing.params = shape.to_params();
    thing.transform.position = at;
    let body = scene.things[&id].slots[&SlotId::Look].layers[0].id;
    let look = scene.card_mut(id, SlotId::Look).unwrap();
    look.params.set(&layered_key(body, "blocks_light"), Value::Bool(true));
    look.params.set("see_through", Value::Number(see.into()));
    look.params.set("bends_light", Value::Number(bends.into()));
}

/// The lit room with no ambient light, the ball out of the way, the lamp at (0, -400) (size 24,
/// reach 1500) and the room's shape as glass: `shape` at `at`.
fn room(reg: &Registry, shape: ShapeSettings, at: Vec2, see: f32, bends: f32, quality: &str) -> Scene {
    let mut scene = scenes::lit_room(reg);
    let ball = id_of(&scene, "balls");
    scene.things.get_mut(&ball).unwrap().transform.position = Vec2::new(0.0, 900.0);
    let light = id_of(&scene, "light");
    let params = &mut scene.things.get_mut(&light).unwrap().params;
    params.set("position", Value::Vec2([0.0, -400.0]));
    params.set("reach", Value::Number(1500.0));
    set_lighting(&mut scene, "ambient", Value::Number(0.0));
    set_lighting(&mut scene, "quality", Value::Choice(quality.into()));
    let id = id_of(&scene, "shape");
    set_glass(&mut scene, id, shape, at, see, bends);
    scene
}

/// The same scene with its glass (the first shape) taken out of the light's way.
fn open(scene: &Scene) -> Scene {
    let mut scene = scene.clone();
    let id = id_of(&scene, "shape");
    scene.things.get_mut(&id).unwrap().transform.position = Vec2::new(0.0, 5000.0);
    scene
}

/// The lamp made a sun shining straight down.
fn sunny(scene: &mut Scene) {
    let light = id_of(scene, "light");
    let params = &mut scene.things.get_mut(&light).unwrap().params;
    params.set("kind", Value::Choice("sun".into()));
    params.set("direction", Value::Number(90.0));
}

/// The brightest light (grey, linear) over a grid of points from `lo` to `hi`, and where.
fn brightest(pixels: &[u8], lo: Vec2, hi: Vec2) -> (f32, Vec2) {
    let mut best = (0.0, lo);
    let mut y = lo.y;
    while y <= hi.y {
        let mut x = lo.x;
        while x <= hi.x {
            let g = grey(light(pixels, x, y));
            if g > best.0 {
                best = (g, Vec2::new(x, y));
            }
            x += 8.0;
        }
        y += 8.0;
    }
    best
}

/// Behind the wall, seen from the lamp at (0, -400).
const BEHIND: [(f32, f32); 4] = [(0.0, 250.0), (-120.0, 300.0), (90.0, 200.0), (150.0, 350.0)];

fn caustics(scene: &Scene, on: f32) -> Scene {
    let mut scene = scene.clone();
    set_lighting(&mut scene, "glass_caustics", Value::Number(on.into()));
    scene
}

#[test]
fn nearly_straight_glass_gives_the_light_it_replaced() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    for quality in ["fast", "good"] {
        for sun in [false, true] {
            let mut scene = room(&reg, wall(600.0), Vec2::ZERO, 0.85, 1.001, quality);
            if sun {
                sunny(&mut scene);
            }
            let name = format!("straight-{quality}-{}", if sun { "sun" } else { "lamp" });
            let bent = draw(&mut gpu, &caustics(&scene, 1.0), &name);
            assert!(gpu.renderer.bent_photons() > 0, "{name}: the glass bends");
            let before = draw(&mut gpu, &caustics(&scene, 0.0), &format!("{name}-part1"));
            assert_eq!(gpu.renderer.bent_photons(), 0, "{name}: Glass caustics 0 sends none");
            for (x, y) in BEHIND {
                let (a, b) = (grey(light(&bent, x, y)), grey(light(&before, x, y)));
                assert!(b > 0.05, "{name} at ({x}, {y}): lit behind the pane ({b})");
                assert!((a / b - 1.0).abs() <= 0.1, "{name} at ({x}, {y}): bent {a} against part 1's {b}");
            }
        }
    }
    // Lit from the side, the light meeting the pane at up to 76°: an edge that hardly changes
    // the index reflects next to nothing.
    let mut scene = room(&reg, wall(600.0), Vec2::ZERO, 0.85, 1.001, "good");
    let lamp = id_of(&scene, "light");
    scene.things.get_mut(&lamp).unwrap().params.set("position", Value::Vec2([-400.0, -100.0]));
    let bent = draw(&mut gpu, &caustics(&scene, 1.0), "straight-oblique");
    let before = draw(&mut gpu, &caustics(&scene, 0.0), "straight-oblique-part1");
    for (x, y) in [(300.0, 150.0), (200.0, 300.0), (400.0, 100.0)] {
        let (a, b) = (grey(light(&bent, x, y)), grey(light(&before, x, y)));
        assert!(b > 0.02, "oblique at ({x}, {y}): lit behind the pane ({b})");
        assert!((a / b - 1.0).abs() <= 0.12, "oblique at ({x}, {y}): bent {a} against part 1's {b}");
    }
}

#[test]
fn one_glass_behind_another_isnt_counted_twice() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let mut scene = room(&reg, wall(600.0), Vec2::ZERO, 0.85, 1.001, "good");
    let second = add_thing(&mut scene, &reg, "shape", ThingId::ROOT).unwrap();
    set_glass(&mut scene, second, wall(600.0), Vec2::new(0.0, 150.0), 0.85, 1.001);
    let bent = draw(&mut gpu, &caustics(&scene, 1.0), "two-panes");
    let before = draw(&mut gpu, &caustics(&scene, 0.0), "two-panes-part1");
    for (x, y) in [(0.0, 300.0), (-100.0, 350.0), (80.0, 260.0)] {
        let (a, b) = (grey(light(&bent, x, y)), grey(light(&before, x, y)));
        assert!(b > 0.03, "lit behind both panes at ({x}, {y}): {b}");
        assert!((a / b - 1.0).abs() <= 0.1, "at ({x}, {y}): bent {a} against part 1's {b}");
    }
}

#[test]
fn a_convex_lens_focuses_and_a_concave_one_spreads() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let mut lens = room(&reg, disc(90.0), Vec2::ZERO, 0.95, 1.5, "good");
    // A dimmer lamp: at full strength the focus is brighter than white, which shows as plain white
    // (since 2026-11-08), so it can't read as much brighter than the open floor.
    let lamp = id_of(&lens, "light");
    lens.things.get_mut(&lamp).unwrap().params.set("strength", Value::Number(0.4));
    let focused = draw(&mut gpu, &lens, "convex");
    let bare = draw(&mut gpu, &open(&lens), "convex-open");
    let (top, at) = brightest(&focused, Vec2::new(-60.0, 100.0), Vec2::new(60.0, 600.0));
    let there = grey(light(&bare, at.x, at.y));
    assert!(top >= 2.5 * there, "focused {top} at {at} against {there} open");

    let concave = room(&reg, biconcave(), Vec2::ZERO, 0.95, 1.5, "good");
    let spread = draw(&mut gpu, &concave, "concave");
    let bare = draw(&mut gpu, &open(&concave), "concave-open");
    for y in [200.0, 300.0] {
        let (a, b) = (grey(light(&spread, 0.0, y)), grey(light(&bare, 0.0, y)));
        assert!(a < 0.8 * b, "behind the middle at y {y}: {a} against {b} open");
    }
}

#[test]
fn rainbow_split_gives_coloured_rims() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let mut lens = room(&reg, disc(90.0), Vec2::ZERO, 0.95, 1.5, "good");
    let id = id_of(&lens, "shape");
    lens.card_mut(id, SlotId::Look).unwrap().params.set("rainbow", Value::Number(1.0));
    let pixels = draw(&mut gpu, &lens, "rainbow");
    let (mut reddest, mut bluest) = (0.0f32, 0.0f32);
    let mut y = 100.0;
    while y <= 600.0 {
        let mut x = -120.0;
        while x <= 120.0 {
            let c = light(&pixels, x, y);
            if grey(c) > 0.02 {
                reddest = reddest.max(c[0] / c[2].max(1e-3));
                bluest = bluest.max(c[2] / c[0].max(1e-3));
            }
            x += 4.0;
        }
        y += 4.0;
    }
    assert!(reddest >= 1.3 && bluest >= 1.3, "red over blue {reddest}, blue over red {bluest}");
}

#[test]
fn nothing_bending_runs_no_photons() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    // Matte see-through glass bends nothing.
    draw(&mut gpu, &room(&reg, wall(600.0), Vec2::ZERO, 0.85, 1.0, "good"), "matte");
    assert_eq!(gpu.renderer.bent_photons(), 0);
    // A glass ball keeps the sphere caustics.
    let mut balls = room(&reg, wall(600.0), Vec2::ZERO, 0.85, 1.0, "good");
    let ball = id_of(&balls, "balls");
    balls.things.get_mut(&ball).unwrap().transform.position = Vec2::ZERO;
    let look = balls.card_mut(ball, SlotId::Look).unwrap();
    look.params.set("see_through", Value::Number(0.9));
    look.params.set("bends_light", Value::Number(1.5));
    draw(&mut gpu, &balls, "glass-ball");
    assert_eq!(gpu.renderer.bent_photons(), 0);
    // Glass caustics 0.
    let lens = room(&reg, disc(90.0), Vec2::ZERO, 0.95, 1.5, "good");
    draw(&mut gpu, &caustics(&lens, 0.0), "lens-off");
    assert_eq!(gpu.renderer.bent_photons(), 0);
    draw(&mut gpu, &lens, "lens-on");
    assert!(gpu.renderer.bent_photons() > 0);
}

#[test]
fn a_stage_lens_focuses_on_the_wall() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let mut lens = room(&reg, disc(90.0), Vec2::ZERO, 0.95, 1.5, "good");
    set_lighting(&mut lens, "depth", Value::Choice("stage".into()));
    let focused = draw(&mut gpu, &lens, "stage");
    assert!(gpu.renderer.bent_photons() > 0);
    let bare = draw(&mut gpu, &open(&lens), "stage-open");
    let (top, at) = brightest(&focused, Vec2::new(-60.0, 110.0), Vec2::new(60.0, 600.0));
    let there = grey(light(&bare, at.x, at.y));
    assert!(top >= 1.8 * there, "focused {top} at {at} against {there} open");
}

/// The lit room's glass moved away, and a Particles thing at the middle giving `count` still
/// particles of `size` at the start, its only layer `kind` with `params`.
fn particles_room(reg: &Registry, count: u32, size: f32, kind: &str, params: Params) -> Scene {
    let mut scene = open(&room(reg, disc(90.0), Vec2::ZERO, 0.95, 1.5, "good"));
    let id = add_particles(&mut scene, reg, Look::Empty, ThingId::ROOT).unwrap();
    let source = ParticlesSettings {
        from: BornFrom::APoint,
        emit: Emit::OnceAtTheStart,
        count,
        life: 100.0,
        life_spread: 0.0,
        speed: 0.0,
        speed_spread: 0.0,
        size,
        size_spread: 0.0,
        ..ParticlesSettings::default()
    };
    scene.things.get_mut(&id).unwrap().params = source.to_params();
    let old: Vec<_> = scene.things[&id].slots[&SlotId::Look].layers.iter().map(|l| l.id).collect();
    let layer = scene.add_layer(reg, id, SlotId::Look, kind).unwrap();
    for l in old {
        scene.remove_layer(reg, id, SlotId::Look, l).unwrap();
    }
    let card = scene.things.get_mut(&id).unwrap().slots.get_mut(&SlotId::Look).unwrap();
    for (k, v) in params.0 {
        card.params.0.insert(layered_key(layer, &k), v);
    }
    scene
}

#[test]
fn a_bubble_that_bends_light_focuses_it() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let mut sprite = SpritesLayer::default().to_params();
    sprite.set("bends_light", Value::Number(1.5));
    let bubble = particles_room(&reg, 1, 120.0, SPRITES, sprite);
    let focused = draw_at(&mut gpu, &bubble, "bubble", 0.5);
    assert!(gpu.renderer.bent_photons() > 0, "a bending sprite sends photons");
    let bare = draw_at(&mut gpu, &caustics(&bubble, 0.0), "bubble-off", 0.5);
    let (top, at) = brightest(&focused, Vec2::new(-60.0, 40.0), Vec2::new(60.0, 500.0));
    let (before, _) = brightest(&bare, Vec2::new(-60.0, 40.0), Vec2::new(60.0, 500.0));
    assert!(top >= 1.3 * before, "focused {top} at {at} against {before} without bent light");
    // At Bends light 1 the sprite bends nothing.
    let plain = particles_room(&reg, 1, 120.0, SPRITES, SpritesLayer::default().to_params());
    draw_at(&mut gpu, &plain, "bubble-plain", 0.5);
    assert_eq!(gpu.renderer.bent_photons(), 0);
}

#[test]
fn a_glass_liquid_bends_light_and_a_plastic_one_doesnt() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    // Lit by the real lights, a glass liquid bends lamp light as a glass shape does: when it blocks
    // light.
    let liquid = |surface| {
        let mut p = params_of(&LiquidLayer { surface, size: 2.0, ..LiquidLayer::default() });
        p.set("blocks_light", Value::Bool(true));
        p
    };
    let glass = particles_room(&reg, 60, 20.0, LIQUID, liquid(Surface::Glass));
    draw_at(&mut gpu, &glass, "liquid-glass", 0.5);
    assert!(gpu.renderer.bent_photons() > 0, "a glass liquid sends photons");
    draw_at(&mut gpu, &caustics(&glass, 0.0), "liquid-glass-off", 0.5);
    let plastic = particles_room(&reg, 60, 20.0, LIQUID, liquid(Surface::Plastic));
    draw_at(&mut gpu, &plastic, "liquid-plastic", 0.5);
    assert_eq!(gpu.renderer.bent_photons(), 0);
}

#[test]
fn free_glass_leaves_what_is_under_it_as_it_is() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    // With Glass caustics 0 a glass liquid that bends light draws just as one that doesn't (kept to
    // its own look by Lit 0, where how much it bends changes nothing else).
    let liquid = |bends: f32| {
        let mut p = params_of(&LiquidLayer { surface: Surface::Glass, size: 2.0, ..LiquidLayer::default() });
        p.set("bends_light", Value::Number(bends.into()));
        p.set("lit", Value::Number(0.0));
        p
    };
    let bending = caustics(&particles_room(&reg, 60, 20.0, LIQUID, liquid(1.33)), 0.0);
    let flat = caustics(&particles_room(&reg, 60, 20.0, LIQUID, liquid(1.0)), 0.0);
    let a = draw_at(&mut gpu, &bending, "liquid-unbent", 0.5);
    let b = draw_at(&mut gpu, &flat, "liquid-flat", 0.5);
    assert!(a == b, "Glass caustics 0: a bending glass liquid changes nothing");
    // A bubble over a solid wall leaves the wall's shadow whole.
    let mut sprite = SpritesLayer::default().to_params();
    sprite.set("bends_light", Value::Number(1.33));
    let mut scene = particles_room(&reg, 1, 120.0, SPRITES, sprite);
    let id = id_of(&scene, "shape");
    set_glass(&mut scene, id, wall(600.0), Vec2::ZERO, 0.0, 1.0);
    let with = draw_at(&mut gpu, &scene, "bubble-on-wall", 0.5);
    assert!(gpu.renderer.bent_photons() > 0, "the bubble sends photons");
    let without = draw_at(&mut gpu, &caustics(&scene, 0.0), "bubble-on-wall-off", 0.5);
    for (x, y) in BEHIND {
        let (a, b) = (grey(light(&with, x, y)), grey(light(&without, x, y)));
        assert!(b < 0.01, "dark behind the wall at ({x}, {y}): {b}");
        assert!(a < b + 0.01, "the bubble lets no light through the wall at ({x}, {y}): {a} against {b}");
    }
}

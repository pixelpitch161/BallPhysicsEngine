//! Glass and light part 1: light through see-through things takes their colour (Aedan approved
//! the glass and light design, 2026-11-07). Red glass throws red light, red over blue glass
//! purple, a two-colour pane both colours; Tints light 0 and Coloured light through glass off
//! throw white light exactly as before; Deeper when thick deepens the colour through a glass
//! ball's middle; a Stage's see-through shape lets its coloured share through; light from a
//! glowing thing comes out of red glass red. Plan: `docs/superpowers/plans/glass-light/part1.md`.
//!
//! Measured on the cloud's software GPU at 270 × 480 (4 world units a pixel), in linear light,
//! per channel, against the same scene with the blocker not blocking. `BPE_SAVE_RENDERS=1` saves
//! the pictures into `target/pictures/tint-*.png`.

use bpe_core::{Color, Gradient, LayerId, Registry, Scene, Settings, SlotId, ThingId, Value, layered_key};
use bpe_gfx::{Frame, LightShow, Renderer, build_frame, test_device};
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
    let (device, queue) = test_device().ok()?;
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
        let file = std::fs::File::create(dir.join(format!("tint-{name}.png"))).unwrap();
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

/// The light reaching world point (x, y), per channel in linear light: the video is 1080 × 1920
/// round the origin.
fn light(pixels: &[u8], x: f32, y: f32) -> [f32; 3] {
    let px = ((x / 1080.0 + 0.5) * W as f32) as usize;
    let py = ((y / 1920.0 + 0.5) * H as f32) as usize;
    let i = (py * W as usize + px) * 4;
    [linear(pixels[i]), linear(pixels[i + 1]), linear(pixels[i + 2])]
}

/// Each channel's share of the light at (x, y) against `open`.
fn share(pixels: &[u8], open: &[u8], (x, y): (f32, f32)) -> [f32; 3] {
    let (a, b) = (light(pixels, x, y), light(open, x, y));
    [0, 1, 2].map(|c| a[c] / b[c].max(1e-4))
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

/// A filled wall `width` wide and 40 thick across the middle.
fn wall(width: f32) -> ShapeSettings {
    ShapeSettings { fill: ShapeFill::Filled, ..ShapeSettings::rectangle(width, 40.0, 2.0) }
}

/// The lit room with no ambient light, the ball out of the way, the lamp at (0, -400) (size 24)
/// and the shape a `wall(600)`, blocking light or not, at See-through `see`.
fn room(reg: &Registry, blocks: bool, see: f32, quality: &str) -> Scene {
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
    set_shape(&mut scene, id, wall(600.0), blocks, see);
    scene
}

fn set_shape(scene: &mut Scene, id: ThingId, shape: ShapeSettings, blocks: bool, see: f32) {
    scene.things.get_mut(&id).unwrap().params = shape.to_params();
    let body = scene.things[&id].slots[&SlotId::Look].layers[0].id;
    let look = scene.card_mut(id, SlotId::Look).unwrap();
    look.params.set(&layered_key(body, "blocks_light"), Value::Bool(blocks));
    look.params.set("see_through", Value::Number(see.into()));
}

/// Paints shape `id` with a Colour and gradient layer of `colours`.
fn paint(scene: &mut Scene, reg: &Registry, id: ThingId, colours: Gradient) -> LayerId {
    let layer = scene.add_layer(reg, id, SlotId::Look, "shape.paint").unwrap();
    scene.card_mut(id, SlotId::Look).unwrap().params.set(&layered_key(layer, "colours"), Value::Gradient(colours));
    layer
}

fn set_surface(scene: &mut Scene, id: ThingId, key: &str, v: f32) {
    scene.card_mut(id, SlotId::Look).unwrap().params.set(key, Value::Number(v.into()));
}

const PINK: Color = Color::rgb(255, 128, 128);
const LIGHT_RED: Color = Color::rgb(255, 102, 102);
const LIGHT_BLUE: Color = Color::rgb(102, 102, 255);

/// Behind the wall, seen from the lamp at (0, -400).
const BEHIND: [(f32, f32); 4] = [(0.0, 250.0), (-120.0, 300.0), (90.0, 200.0), (150.0, 350.0)];

fn near(got: f32, want: f32, by: f32) -> bool {
    (got - want).abs() <= by
}

#[test]
fn red_glass_throws_red_light() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let t = linear(128);
    for quality in ["good", "ultra"] {
        let open = draw(&mut gpu, &room(&reg, false, 0.85, quality), "open");
        let mut scene = room(&reg, true, 0.85, quality);
        let id = id_of(&scene, "shape");
        paint(&mut scene, &reg, id, Gradient::solid(PINK));
        let pixels = draw(&mut gpu, &scene, &format!("red-{quality}"));
        for p in BEHIND {
            let s = share(&pixels, &open, p);
            assert!(near(s[0], 0.85, 0.06), "{quality} {p:?}: red {s:?}");
            assert!(near(s[1], 0.85 * t, 0.06) && near(s[2], 0.85 * t, 0.06), "{quality} {p:?}: {s:?}");
        }
    }
}

#[test]
fn red_over_blue_gives_purple() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let t = linear(102);
    let two = |blocks: bool| {
        let mut scene = room(&reg, blocks, 1.0, "good");
        let red = id_of(&scene, "shape");
        let blue = add_thing(&mut scene, &reg, "shape", ThingId::ROOT).unwrap();
        set_shape(&mut scene, blue, wall(600.0), blocks, 1.0);
        scene.things.get_mut(&blue).unwrap().transform.position = Vec2::new(0.0, 80.0);
        paint(&mut scene, &reg, red, Gradient::solid(LIGHT_RED));
        paint(&mut scene, &reg, blue, Gradient::solid(LIGHT_BLUE));
        scene
    };
    let open = draw(&mut gpu, &two(false), "two-open");
    let pixels = draw(&mut gpu, &two(true), "red-over-blue");
    for p in [(0.0, 300.0), (-100.0, 350.0), (80.0, 260.0)] {
        let s = share(&pixels, &open, p);
        // Clear glass passes its colour whole: red's (1, t, t) times blue's (t, t, 1).
        let want = [t, t * t, t];
        for c in 0..3 {
            assert!(near(s[c], want[c], 0.05), "{p:?}: {s:?} against {want:?}");
        }
        assert!(s[0] > 3.0 * s[1] && s[2] > 3.0 * s[1], "purple at {p:?}: {s:?}");
    }
}

#[test]
fn a_two_colour_pane_throws_both_colours() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let open = draw(&mut gpu, &room(&reg, false, 1.0, "good"), "pane-open");
    let mut scene = room(&reg, true, 1.0, "good");
    let id = id_of(&scene, "shape");
    let mut colours = Gradient::solid(Color::rgb(255, 0, 0));
    colours.stops.push(bpe_core::Stop { at: 1.0, ..colours.stops[0] });
    colours.stops[1].color = Color::rgb(0, 0, 255);
    let layer = paint(&mut scene, &reg, id, colours);
    // Left to right (the default Turn 90 runs top to bottom, through the wall's thickness).
    scene.card_mut(id, SlotId::Look).unwrap().params.set(&layered_key(layer, "colours~turn"), Value::Number(0.0));
    let pixels = draw(&mut gpu, &scene, "pane");
    let left = share(&pixels, &open, (-330.0, 250.0));
    let right = share(&pixels, &open, (330.0, 250.0));
    assert!(left[0] > 2.0 * left[2], "red on the left: {left:?}");
    assert!(right[2] > 2.0 * right[0], "blue on the right: {right:?}");
}

#[test]
fn tints_light_0_is_colourless() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let white = draw(&mut gpu, &room(&reg, true, 0.85, "good"), "white");
    let mut scene = room(&reg, true, 0.85, "good");
    let id = id_of(&scene, "shape");
    paint(&mut scene, &reg, id, Gradient::solid(PINK));
    set_surface(&mut scene, id, "tints", 0.0);
    let pixels = draw(&mut gpu, &scene, "tints-0");
    assert!(pixels == white, "Tints light 0 throws white light as white glass does");
    let open = draw(&mut gpu, &room(&reg, false, 0.85, "good"), "open");
    for p in BEHIND {
        let s = share(&pixels, &open, p);
        assert!(s.iter().all(|c| near(*c, 0.85, 0.06)), "{p:?}: {s:?}");
    }
}

#[test]
fn glass_colour_off_is_exactly_colourless() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    for quality in ["draft", "good", "ultra"] {
        let white_on = draw(&mut gpu, &room(&reg, true, 0.85, quality), "white-on");
        let mut white_off = room(&reg, true, 0.85, quality);
        set_lighting(&mut white_off, "glass_colour", Value::Bool(false));
        let mut red_off = white_off.clone();
        let id = id_of(&red_off, "shape");
        paint(&mut red_off, &reg, id, Gradient::solid(PINK));
        let white_off = draw(&mut gpu, &white_off, "white-off");
        let red_off = draw(&mut gpu, &red_off, "red-off");
        assert!(red_off == white_off, "{quality}: off, red glass throws white light");
        assert!(white_on == white_off, "{quality}: white glass throws the same light either way");
    }
}

/// A red glass ball between the lamp and the floor: its middle, where it is thickest, deepens the
/// colour by Deeper when thick; nearer its edge less.
#[test]
fn deeper_when_thick_deepens_the_middle() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let t = linear(128);
    let scene = |blocks: bool, deeper: f32| {
        let mut scene = room(&reg, false, 0.0, "ultra");
        let shape = id_of(&scene, "shape");
        scene.things.get_mut(&shape).unwrap().transform.position = Vec2::new(0.0, 900.0);
        let ball = id_of(&scene, "balls");
        let thing = scene.things.get_mut(&ball).unwrap();
        thing.transform.position = Vec2::ZERO;
        thing.params.set("radius", Value::Number(160.0));
        let body = scene.things[&ball].slots[&SlotId::Look].layers[0].id;
        let look = scene.card_mut(ball, SlotId::Look).unwrap();
        look.params.set(&layered_key(body, "blocks_light"), Value::Bool(blocks));
        look.params.set(&layered_key(body, "colours"), Value::Gradient(Gradient::solid(PINK)));
        look.params.set("see_through", Value::Number(1.0));
        look.params.set("thick_tint", Value::Number(deeper.into()));
        scene
    };
    let open = draw(&mut gpu, &scene(false, 0.0), "ball-open");
    let flat = draw(&mut gpu, &scene(true, 0.0), "ball-0");
    let deep = draw(&mut gpu, &scene(true, 0.5), "ball-half");
    // Straight behind the middle: the ray crosses the ball through its middle.
    let middle = (0.0, 300.0);
    let ratio = |pixels: &[u8], p| {
        let s = share(pixels, &open, p);
        s[1] / s[0].max(1e-4)
    };
    let (r0, r1) = (ratio(&flat, middle), ratio(&deep, middle));
    assert!(near(r0, t, 0.03 + 0.15 * t), "Deeper 0: {r0} against {t}");
    let want = t.powf(1.0 + 2.0 * 0.5);
    assert!(near(r1, want, 0.02 + 0.25 * want), "Deeper ½ through the middle: {r1} against {want}");
    // At (270, 300) the ray from the lamp passes 0.9 radii (144) from the middle:
    // 400 x / sqrt(x² + 700²) = 144.
    let edge = ratio(&deep, (270.0, 300.0));
    assert!(edge > r1 + 0.01, "thinner near the edge: {edge} against the middle's {r1}");
}

#[test]
fn a_blended_glass_shape_still_tints() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let t = linear(128);
    let open = draw(&mut gpu, &room(&reg, false, 0.85, "good"), "open");
    let mut scene = room(&reg, true, 0.85, "good");
    let id = id_of(&scene, "shape");
    paint(&mut scene, &reg, id, Gradient::solid(PINK));
    scene.add_layer(&reg, id, SlotId::Look, "shape.blend").unwrap();
    let pixels = draw(&mut gpu, &scene, "blended");
    for p in BEHIND {
        let s = share(&pixels, &open, p);
        assert!(near(s[0], 0.85, 0.06) && near(s[1], 0.85 * t, 0.06), "{p:?}: {s:?}");
    }
}

#[test]
fn a_shape_that_doesnt_block_throws_no_colour() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let open = draw(&mut gpu, &room(&reg, false, 0.85, "good"), "open");
    let mut scene = room(&reg, false, 0.85, "good");
    let id = id_of(&scene, "shape");
    paint(&mut scene, &reg, id, Gradient::solid(PINK));
    let pixels = draw(&mut gpu, &scene, "not-blocking");
    for p in BEHIND {
        let s = share(&pixels, &open, p);
        assert!(s.iter().all(|c| near(*c, 1.0, 0.03)), "{p:?}: {s:?}");
    }
}

/// On a Stage a see-through shape lets its share through in its colour; a solid one still blocks
/// whole, with the colour on or off.
#[test]
fn stage_glass_slab_lets_coloured_light_through() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let stage = |blocks: bool, see: f32, glass_colour: bool| {
        let mut scene = room(&reg, blocks, see, "good");
        set_lighting(&mut scene, "depth", Value::Choice("stage".into()));
        set_lighting(&mut scene, "glass_colour", Value::Bool(glass_colour));
        let id = id_of(&scene, "shape");
        paint(&mut scene, &reg, id, Gradient::solid(PINK));
        scene
    };
    let open = draw(&mut gpu, &stage(false, 0.0, true), "stage-open");
    let solid = draw(&mut gpu, &stage(true, 0.0, true), "stage-solid");
    let solid_off = draw(&mut gpu, &stage(true, 0.0, false), "stage-solid-off");
    let glass = draw(&mut gpu, &stage(true, 0.85, true), "stage-glass");
    let mean = |pixels: &[u8]| {
        let mut sum = [0.0f32; 3];
        for p in BEHIND {
            let s = share(pixels, &open, p);
            (0..3).for_each(|c| sum[c] += s[c] / BEHIND.len() as f32);
        }
        sum
    };
    let (s, g, o) = (mean(&solid), mean(&glass), mean(&solid_off));
    assert!(s.iter().zip(o).all(|(a, b)| near(*a, b, 0.02)), "a solid wall: {s:?} against off {o:?}");
    assert!(g[0] > s[0] + 0.15, "the glass's shadow is lighter: {g:?} against {s:?}");
    assert!(g[0] > 1.5 * g[1].max(1e-3), "and red: {g:?}");
}

/// A glowing white ball behind red glass, no lamps: the light field comes out of the glass red.
#[test]
fn glow_through_red_glass_comes_out_red() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let scene = |red: bool| {
        let mut scene = room(&reg, true, 0.85, "good");
        let light = id_of(&scene, "light");
        scene.things.get_mut(&light).unwrap().enabled = false;
        set_lighting(&mut scene, "glow_reach", Value::Number(1500.0));
        let ball = id_of(&scene, "balls");
        let thing = scene.things.get_mut(&ball).unwrap();
        thing.transform.position = Vec2::new(0.0, -300.0);
        thing.params.set("radius", Value::Number(40.0));
        let body = scene.things[&ball].slots[&SlotId::Look].layers[0].id;
        let sub = scene.add_sub_layer(&reg, ball, SlotId::Look, body, "sub.light").unwrap();
        scene.card_mut(ball, SlotId::Look).unwrap().params.set(&layered_key(sub, "strength"), Value::Number(3.0));
        if red {
            let id = id_of(&scene, "shape");
            paint(&mut scene, &reg, id, Gradient::solid(PINK));
        }
        scene
    };
    let white = draw(&mut gpu, &scene(false), "glow-white");
    let red = draw(&mut gpu, &scene(true), "glow-red");
    for p in BEHIND {
        let (w, r) = (light(&white, p.0, p.1), light(&red, p.0, p.1));
        assert!(w[0] > 0.01 && w[1] > 0.002, "the glow reaches {p:?}: {w:?}");
        // The ball's glow is its own colour: what red glass lets through, against white glass.
        let s = [0, 1, 2].map(|c| r[c] / w[c].max(1e-4));
        assert!(s[0] > 1.5 * s[1] && s[0] > 1.5 * s[2], "red glass: red light at {p:?}: {s:?}");
    }
}

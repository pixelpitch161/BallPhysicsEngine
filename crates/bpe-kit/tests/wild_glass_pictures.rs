//! Glass and light part 4b: dichroic glass, diamond fire and rainbows (Aedan approved the glass
//! and light design, 2026-11-07). A dichroic see-through wall colours its shadow with the hue it
//! lets through, and bending dichroic glass colours the light it focuses; a dichroic ball shines
//! the opposite hue; Fire splits a highlight into colours; the rainbow's red ring lies outside its
//! violet one at the size set, in haze or on rain that makes rainbows. Plan:
//! `docs/superpowers/plans/glass-light/part4.md`.
//!
//! Measured on the cloud's software GPU at 270 × 480 (4 world units a pixel), in linear light.
//! `BPE_SAVE_RENDERS=1` saves the pictures into `target/pictures/wild-*.png`.

use bpe_core::{Choice, Color, Gradient, Registry, Scene, Settings, SlotId, ThingId, Value, layered_key};
use bpe_gfx::{Frame, LightShow, Renderer, build_frame, test_device};
use bpe_kit::modules::look::surface::{SurfaceKind, preset_values};
use bpe_kit::things::particles::look::SPRITES;
use bpe_kit::things::particles::looks::{Look, add_particles};
use bpe_kit::things::particles::{BornFrom, Emit, ParticlesSettings};
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

fn draw_as(gpu: &mut Gpu, scene: &Scene, name: &str, show: LightShow) -> Vec<u8> {
    let kit = kit();
    let resolved = bpe_core::resolve(scene, &kit.registry);
    let mut playback = Playback::new(scene, &resolved, kit.sims.clone());
    let t = 0.25;
    playback.seek(t);
    let mut frame = Frame::default();
    build_frame(scene, &resolved, playback.world(), playback.log(), t, &kit.draws, &mut frame);
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
        let file = std::fs::File::create(dir.join(format!("wild-{name}.png"))).unwrap();
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

/// The colour at world point (x, y) in linear light, the mean of the 3 × 3 pixels round it: the
/// video is 1080 × 1920 round the origin.
fn rgb(pixels: &[u8], x: f32, y: f32) -> [f32; 3] {
    let px = ((x / 1080.0 + 0.5) * W as f32) as i32;
    let py = ((y / 1920.0 + 0.5) * H as f32) as i32;
    let mut sum = [0.0; 3];
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (qx, qy) = ((px + dx).clamp(0, W as i32 - 1), (py + dy).clamp(0, H as i32 - 1));
            let i = (qy as usize * W as usize + qx as usize) * 4;
            for (c, s) in sum.iter_mut().enumerate() {
                *s += linear(pixels[i + c]) / 9.0;
            }
        }
    }
    sum
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

fn white_body(scene: &mut Scene, thing: ThingId, key: &str) {
    let body = scene.things[&thing].slots[&SlotId::Look].layers[0].id;
    let white = Color::rgb(255, 255, 255);
    set_look(scene, thing, &layered_key(body, key), Value::Gradient(Gradient::even(&[white, white])));
    set_look(scene, thing, &layered_key(body, "blocks_light"), Value::Bool(true));
}

/// The lit room with no ambient light, the ball out of the way, the lamp above at (0, -400), and
/// the room's shape a filled white Glass wall 400 × 40 at the middle that blocks light; bending
/// light as Glass does when `bends`, else straight through.
fn glass_room(reg: &Registry, bends: bool) -> Scene {
    let mut scene = scenes::lit_room(reg);
    let ball = id_of(&scene, "balls");
    scene.things.get_mut(&ball).unwrap().transform.position = Vec2::new(0.0, 900.0);
    set_light(&mut scene, "reach", Value::Number(2000.0));
    set_light(&mut scene, "colour", Value::Color(Color::rgb(255, 255, 255)));
    set_lighting(&mut scene, "ambient", Value::Number(0.0));
    set_lighting(&mut scene, "quality", Value::Choice("good".into()));
    let id = id_of(&scene, "shape");
    let thing = scene.things.get_mut(&id).unwrap();
    thing.params = ShapeSettings { fill: ShapeFill::Filled, ..ShapeSettings::rectangle(400.0, 40.0, 2.0) }.to_params();
    thing.transform.position = Vec2::ZERO;
    white_body(&mut scene, id, "colours");
    set_surface(&mut scene, id, SurfaceKind::Glass);
    set_look(&mut scene, id, "see_through", Value::Number(0.7));
    if !bends {
        set_look(&mut scene, id, "bends_light", Value::Number(1.0));
    }
    scene
}

fn dichroic(scene: &Scene, thing: &str, amount: f64, hue: f64, shift: f64) -> Scene {
    let mut scene = scene.clone();
    let id = id_of(&scene, thing);
    set_look(&mut scene, id, "dichroic", Value::Number(amount));
    set_look(&mut scene, id, "dichroic_hue", Value::Number(hue));
    set_look(&mut scene, id, "dichroic_shift", Value::Number(shift));
    scene
}

#[test]
fn dichroic_zero_changes_nothing_whatever_its_hue() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    for bends in [false, true] {
        let scene = glass_room(&reg, bends);
        let plain = draw_as(&mut gpu, &scene, "plain", LightShow::Picture);
        let other = draw_as(&mut gpu, &dichroic(&scene, "shape", 0.0, 40.0, -200.0), "d0", LightShow::Picture);
        assert!(plain == other, "Dichroic 0 draws as before (bends {bends})");
        let mut fire = scene.clone();
        let id = id_of(&fire, "shape");
        set_look(&mut fire, id, "fire", Value::Number(0.0));
        set_lighting(&mut fire, "rainbow", Value::Number(0.0));
        set_lighting(&mut fire, "rainbow_size", Value::Number(300.0));
        assert!(plain == draw_as(&mut gpu, &fire, "fire0", LightShow::Picture), "Fire 0 and Rainbow 0 change nothing");
    }
}

#[test]
fn a_dichroic_wall_colours_its_shadow_with_the_hue_it_lets_through() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let scene = glass_room(&reg, false);
    let grey = draw_as(&mut gpu, &scene, "shadow-grey", LightShow::LightOnly);
    // Hue 190° (cyan-blue) through, straight on and turning no further.
    let blue = draw_as(&mut gpu, &dichroic(&scene, "shape", 1.0, 190.0, 0.0), "shadow-blue", LightShow::LightOnly);
    let mut checked = 0;
    for x in [-120.0, -60.0, 0.0, 60.0, 120.0] {
        let (g, b) = (rgb(&grey, x, 120.0), rgb(&blue, x, 120.0));
        assert!(g[2] > 0.01, "light gets through the see-through wall at {x}: {g:?}");
        assert!((g[0] - g[2]).abs() <= 0.1 * g[2] + 0.002, "a plain wall's shadow is grey at {x}: {g:?}");
        assert!(b[2] > 0.6 * g[2] && b[0] < 0.2 * b[2], "the dichroic wall's shadow is blue at {x}: {b:?}");
        checked += 1;
    }
    assert_eq!(checked, 5);
    // In front of the wall, where the lamp's light comes straight, nothing changed.
    let (g, b) = (rgb(&grey, 200.0, -300.0), rgb(&blue, 200.0, -300.0));
    assert!((g[0] - b[0]).abs() < 0.01 && (g[2] - b[2]).abs() < 0.01, "{g:?} against {b:?}");
}

#[test]
fn bending_dichroic_glass_colours_the_light_it_carries() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let scene = glass_room(&reg, true);
    let plain = draw_as(&mut gpu, &scene, "bent-plain", LightShow::LightOnly);
    assert!(gpu.renderer.bent_photons() > 0, "the glass bends lamp light");
    // Hue 0 (red) through.
    let red = draw_as(&mut gpu, &dichroic(&scene, "shape", 1.0, 0.0, 0.0), "bent-red", LightShow::LightOnly);
    let (mut redder, mut seen) = (0, 0);
    for x in [-120.0, -60.0, 0.0, 60.0, 120.0] {
        for y in [80.0, 160.0, 240.0] {
            let (p, r) = (rgb(&plain, x, y), rgb(&red, x, y));
            if p[0] > 0.01 {
                seen += 1;
                if r[0] > 4.0 * r[2] && r[0] > 0.3 * p[0] {
                    redder += 1;
                }
            }
        }
    }
    assert!(seen >= 8 && redder * 4 >= seen * 3, "{redder} of {seen} places under the glass lit red");
}

/// The lit room with a white ball of radius 200 at the middle (its Body alone), its surface
/// `kind`, under the lamp at (-300, -400), on a dark background.
fn ball_room(reg: &Registry, kind: SurfaceKind) -> Scene {
    let mut scene = scenes::lit_room(reg);
    set_light(&mut scene, "position", Value::Vec2([-300.0, -400.0]));
    set_light(&mut scene, "reach", Value::Number(2000.0));
    set_light(&mut scene, "height", Value::Number(300.0));
    set_light(&mut scene, "strength", Value::Number(0.5));
    set_light(&mut scene, "colour", Value::Color(Color::rgb(255, 255, 255)));
    set_lighting(&mut scene, "ambient", Value::Number(0.05));
    let shape = id_of(&scene, "shape");
    scene.things.get_mut(&shape).unwrap().params.set("size", Value::Number(3000.0));
    let ball = id_of(&scene, "balls");
    let t = scene.things.get_mut(&ball).unwrap();
    t.params.set("radius", Value::Number(200.0));
    t.params.set("velocity", Value::Vec2([0.0, 0.0]));
    t.transform.position = Vec2::ZERO;
    scene.card_mut(ball, SlotId::Look).unwrap().layers.retain(|l| l.kind == "layer.body");
    white_body(&mut scene, ball, "colours");
    set_surface(&mut scene, ball, kind);
    scene
}

/// Over the ball (radius 200 round the middle), the most any pixel's channels differ: how
/// coloured its most coloured place is, and where.
fn most_colour(pixels: &[u8]) -> (f32, [f32; 3]) {
    let mut best = (0.0, [0.0; 3]);
    let mut y = -190.0;
    while y <= 190.0 {
        let mut x = -190.0;
        while x <= 190.0 {
            if x * x + y * y < 185.0 * 185.0 {
                let c = rgb(pixels, x, y);
                let spread = c.iter().copied().fold(0.0f32, f32::max) - c.iter().copied().fold(f32::MAX, f32::min);
                if spread > best.0 {
                    best = (spread, c);
                }
            }
            x += 4.0;
        }
        y += 4.0;
    }
    best
}

/// The ball's brightest place's colour.
fn brightest(pixels: &[u8]) -> [f32; 3] {
    let mut best = [0.0f32; 3];
    let mut y = -190.0;
    while y <= 190.0 {
        let mut x = -190.0;
        while x <= 190.0 {
            if x * x + y * y < 185.0 * 185.0 {
                let c = rgb(pixels, x, y);
                if c.iter().sum::<f32>() > best.iter().sum::<f32>() {
                    best = c;
                }
            }
            x += 4.0;
        }
        y += 4.0;
    }
    best
}

#[test]
fn a_dichroic_ball_shines_the_opposite_hue() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let scene = ball_room(&reg, SurfaceKind::Plastic);
    let white = brightest(&draw_as(&mut gpu, &scene, "shine-white", LightShow::Picture));
    assert!((white[0] - white[2]).abs() < 0.05 * white[0] + 0.01, "a white ball's highlight is white: {white:?}");
    // Cyan through, so red bounces off.
    let shine =
        brightest(&draw_as(&mut gpu, &dichroic(&scene, "balls", 1.0, 180.0, 0.0), "shine-red", LightShow::Picture));
    assert!(shine[0] > 1.3 * shine[2] && shine[0] > 1.3 * shine[1], "the dichroic ball's highlight is red: {shine:?}");
}

#[test]
fn fire_splits_a_highlight_into_colours() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let mut scene = ball_room(&reg, SurfaceKind::Plastic);
    let ball = id_of(&scene, "balls");
    set_look(&mut scene, ball, "roughness", Value::Number(0.15));
    let (plain, _) = most_colour(&draw_as(&mut gpu, &scene, "fire-0", LightShow::Picture));
    set_look(&mut scene, ball, "fire", Value::Number(1.0));
    let (fire, at) = most_colour(&draw_as(&mut gpu, &scene, "fire-1", LightShow::Picture));
    assert!(plain < 0.02, "a white ball shows no colours: {plain}");
    assert!(fire > 0.08 && fire > 4.0 * plain, "Fire 1 splits the highlight: {fire} at {at:?} against {plain}");
}

/// The lit room, the ball and shape out of the way, the lamp at the top lighting the whole
/// picture, and the Lighting card's Rainbow on, its middle put at (0, 700) with size 700, so the
/// bow passes through the middle of the picture.
fn rainbow_room(reg: &Registry) -> Scene {
    let mut scene = scenes::lit_room(reg);
    let ball = id_of(&scene, "balls");
    scene.things.get_mut(&ball).unwrap().transform.position = Vec2::new(0.0, 900.0);
    let shape = id_of(&scene, "shape");
    scene.things.get_mut(&shape).unwrap().params.set("size", Value::Number(3000.0));
    set_light(&mut scene, "position", Value::Vec2([0.0, -900.0]));
    set_light(&mut scene, "reach", Value::Number(4000.0));
    set_lighting(&mut scene, "rainbow", Value::Number(1.0));
    set_lighting(&mut scene, "rainbow_from", Value::Choice("a_point".into()));
    set_lighting(&mut scene, "rainbow_centre", Value::Vec2([0.0, 700.0]));
    set_lighting(&mut scene, "rainbow_size", Value::Number(700.0));
    scene
}

#[test]
fn the_rainbow_shows_in_haze_red_outside_violet_at_its_size() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let mut scene = rainbow_room(&reg);
    set_lighting(&mut scene, "haze", Value::Number(0.6));
    let bow = draw_as(&mut gpu, &scene, "bow", LightShow::Picture);
    let mut none = scene.clone();
    set_lighting(&mut none, "rainbow", Value::Number(0.0));
    let plain = draw_as(&mut gpu, &none, "bow-off", LightShow::Picture);
    // Up from the middle of the bow: radius r is at y = 700 − r.
    let (mut red_at, mut red) = (0.0, f32::MIN);
    let (mut blue_at, mut blue) = (0.0, f32::MIN);
    let mut r = 500.0;
    while r <= 900.0 {
        let (b, p) = (rgb(&bow, 0.0, 700.0 - r), rgb(&plain, 0.0, 700.0 - r));
        let warm = (b[0] - p[0]) - (b[2] - p[2]);
        if warm > red {
            (red, red_at) = (warm, r);
        }
        if -warm > blue {
            (blue, blue_at) = (-warm, r);
        }
        r += 4.0;
    }
    assert!(red > 0.01 && blue > 0.01, "the bow shows: red {red}, blue {blue}");
    assert!(red_at > blue_at, "red at {red_at} lies outside blue at {blue_at}");
    assert!((red_at - 700.0).abs() < 40.0 && (blue_at - 700.0).abs() < 50.0, "red {red_at}, blue {blue_at}, size 700");
    // Where nothing catches it (no haze, no rain), no rainbow.
    let mut clear = scene.clone();
    set_lighting(&mut clear, "haze", Value::Number(0.0));
    let mut clear_off = clear.clone();
    set_lighting(&mut clear_off, "rainbow", Value::Number(0.0));
    let (a, b) = (
        draw_as(&mut gpu, &clear, "bow-clear", LightShow::Picture),
        draw_as(&mut gpu, &clear_off, "bow-clear-off", LightShow::Picture),
    );
    assert!(a == b, "clear air shows no rainbow");
}

#[test]
fn rain_that_makes_rainbows_shows_the_bow() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let mut scene = rainbow_room(&reg);
    // A still clump of white particles on the bow, at (0, 0).
    let id = add_particles(&mut scene, &reg, Look::Empty, ThingId::ROOT).unwrap();
    let source = ParticlesSettings {
        from: BornFrom::APoint,
        emit: Emit::OnceAtTheStart,
        count: 60,
        speed: 0.0,
        life: 100.0,
        size: 40.0,
        ..ParticlesSettings::default()
    };
    scene.things.get_mut(&id).unwrap().params = source.to_params();
    let sprites = scene.things[&id].slots[&SlotId::Look].layers.iter().find(|l| l.kind == SPRITES).unwrap().id;
    let dry = draw_as(&mut gpu, &scene, "rain-dry", LightShow::Picture);
    let mut rain = scene.clone();
    set_look(&mut rain, id, &layered_key(sprites, "makes_rainbows"), Value::Number(1.0));
    let wet = draw_as(&mut gpu, &rain, "rain-wet", LightShow::Picture);
    let mut off = rain.clone();
    set_lighting(&mut off, "rainbow", Value::Number(0.0));
    let unlit = draw_as(&mut gpu, &off, "rain-off", LightShow::Picture);
    assert!(dry == unlit, "rain that doesn't make rainbows, and Rainbow 0, show none");
    let (d, w) = (rgb(&dry, 0.0, 0.0), rgb(&wet, 0.0, 0.0));
    let diff: f32 = (0..3).map(|c| (w[c] - d[c]).abs()).sum();
    assert!(diff > 0.03, "the bow shows on the rain: {w:?} against {d:?}");
    // Away from the rain, nothing changed.
    let (d, w) = (rgb(&dry, 300.0, -600.0), rgb(&wet, 300.0, -600.0));
    assert!((0..3).all(|c| (w[c] - d[c]).abs() < 0.005), "{w:?} against {d:?}");
}

#[test]
fn a_dichroic_mirror_throws_the_opposite_hue() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    // The glass room's wall made a Chrome mirror 300 wide, lit from (-300, -400).
    let mut scene = glass_room(&reg, false);
    set_light(&mut scene, "position", Value::Vec2([-300.0, -400.0]));
    let id = id_of(&scene, "shape");
    scene.things.get_mut(&id).unwrap().params =
        ShapeSettings { fill: ShapeFill::Filled, ..ShapeSettings::rectangle(300.0, 40.0, 2.0) }.to_params();
    set_surface(&mut scene, id, SurfaceKind::Chrome);
    let mut off = scene.clone();
    set_look(&mut off, id, "throws", Value::Number(0.0));
    let none = draw_as(&mut gpu, &off, "mirror-off", LightShow::LightOnly);
    let white = draw_as(&mut gpu, &scene, "mirror-white", LightShow::LightOnly);
    // Cyan goes through, so red is thrown.
    let red = draw_as(&mut gpu, &dichroic(&scene, "shape", 1.0, 180.0, 0.0), "mirror-red", LightShow::LightOnly);
    // Places the reflection off the wall's middle reaches (the lamp's image is at (-300, 360)).
    for (x, y) in [(200.0, -400.0), (300.0, -600.0), (250.0, -500.0)] {
        let (n, w, r) = (rgb(&none, x, y), rgb(&white, x, y), rgb(&red, x, y));
        let thrown_white = [w[0] - n[0], w[1] - n[1], w[2] - n[2]];
        let thrown_red = [r[0] - n[0], r[1] - n[1], r[2] - n[2]];
        assert!(
            thrown_white[0] > 0.01 && thrown_white[2] > 0.5 * thrown_white[0],
            "white thrown at ({x}, {y}): {thrown_white:?}"
        );
        assert!(
            thrown_red[0] > 0.5 * thrown_white[0] && thrown_red[2].abs() < 0.2 * thrown_red[0],
            "red thrown at ({x}, {y}): {thrown_red:?} against white {thrown_white:?}"
        );
    }
}

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
    draw_showing(gpu, scene, name, bpe_gfx::LightShow::Picture)
}

/// [`draw`] with the light pass showing `show` (the editor's views of the light).
fn draw_showing(gpu: &mut Gpu, scene: &Scene, name: &str, show: bpe_gfx::LightShow) -> Vec<u8> {
    let kit = kit();
    let mut frame = frame_of(scene, &kit, 0.0);
    if let Some(light) = frame.light.as_mut() {
        light.show = show;
    }
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
fn light_only_shows_the_light_falling_with_its_shadows() {
    use bpe_gfx::LightShow;
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let mut scene = scenes::lit_room(&reg);
    let picture = draw(&mut gpu, &scene, "show-picture");
    let shown = draw_showing(&mut gpu, &scene, "show-picture", LightShow::Picture);
    assert!(picture == shown, "showing the picture is the picture");
    let light = draw_showing(&mut gpu, &scene, "show-light", LightShow::LightOnly);
    assert!(light != picture);
    let near = at(&light, 0.0, -300.0);
    let far = at(&light, 400.0, 700.0);
    assert!(brightness(near) > brightness(far) + 60, "near {near:?} far {far:?}");
    // Grey: the light is white, whatever the background's colour.
    assert!(near[0].abs_diff(near[2]) <= 12, "{near:?}");
    let behind = at(&light, 0.0, 120.0);
    let side = at(&light, 216.0, -400.0 + 473.0);
    assert!(brightness(behind) + 30 < brightness(side), "the ball's shadow: behind {behind:?} beside {side:?}");
    // With the lamp off, only the ambient light: the same grey everywhere.
    set_light(&mut scene, "strength", Value::Number(0.0));
    let ambient = draw_showing(&mut gpu, &scene, "show-ambient", LightShow::LightOnly);
    let (a, b) = (at(&ambient, -300.0, -600.0), at(&ambient, 300.0, 700.0));
    assert!(brightness(a).abs_diff(brightness(b)) <= 6, "{a:?} {b:?}");
    assert!(brightness(a) > 100 && brightness(a) < 600, "ambient 0.25 is a mid grey: {a:?}");
}

#[test]
fn the_heat_map_runs_from_dark_blue_to_hot() {
    use bpe_gfx::LightShow;
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let mut scene = scenes::lit_room(&reg);
    set_lighting(&mut scene, "ambient", Value::Number(0.0));
    set_light(&mut scene, "reach", Value::Number(500.0));
    set_light(&mut scene, "strength", Value::Number(4.0));
    let heat = draw_showing(&mut gpu, &scene, "show-heat", LightShow::HeatMap);
    let dark = at(&heat, 400.0, 800.0);
    assert!(brightness(dark) <= 6, "no light is black: {dark:?}");
    let hot = at(&heat, 30.0, -400.0);
    assert!(hot[0] > 200 && hot[0] >= hot[1] && hot[0] >= hot[2], "a bright pool is red or white: {hot:?}");
    // As unlit (light 1) is green, halfway between.
    let mid = (0..=500)
        .step_by(10)
        .map(|d| at(&heat, d as f32, -400.0))
        .find(|p| p[1] as u32 > p[0] as u32 + 40 && p[1] as u32 > p[2] as u32 + 40);
    assert!(mid.is_some(), "somewhere in the pool the light is about as unlit: green");
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

/// Writes lighting part 1b's demo scene to `BPE_GLOW_SCENE` (for the review video), when set.
#[test]
fn write_the_glow_demo_scene() {
    let Some(path) = std::env::var_os("BPE_GLOW_SCENE") else { return };
    let reg = reg();
    std::fs::write(path, bpe_core::save::save(&scenes::glow_demo(&reg))).unwrap();
}

// Lighting part 1b: things that give off light, the light field, haze and Light blooms.

/// The ball's Gives off light sub-layer in [`scenes::glow_room`].
fn glow_of(scene: &Scene) -> (ThingId, bpe_core::LayerId) {
    let ball = ball_of(scene);
    let card = &scene.things[&ball].slots[&SlotId::Look];
    let sub = card.layers.iter().find(|l| l.kind.as_str() == "sub.light").expect("a Gives off light").id;
    (ball, sub)
}

fn set_glow(scene: &mut Scene, key: &str, v: Value) {
    let (ball, sub) = glow_of(scene);
    scene.card_mut(ball, SlotId::Look).unwrap().params.set(&layered_key(sub, key), v);
}

/// The glow room's ball without its Gives off light: an ordinary lit ball again.
fn drop_glow(scene: &mut Scene) {
    let (ball, sub) = glow_of(scene);
    scene.remove_layer(&reg(), ball, SlotId::Look, sub).unwrap();
}

fn set_root(scene: &mut Scene, key: &str, v: Value) {
    scene.things.get_mut(&ThingId::ROOT).unwrap().params.set(key, v);
}

fn redness(p: [u8; 4]) -> i32 {
    p[0] as i32 - p[2] as i32
}

#[test]
fn a_glowing_ball_lights_what_is_round_it_and_shows_its_own_colour() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let scene = scenes::glow_room(&reg);
    let lit = draw(&mut gpu, &scene, "glow");
    let mut off = scene.clone();
    set_glow(&mut off, "strength", Value::Number(0.0));
    let dark = draw(&mut gpu, &off, "glow-off");
    // Beside the ball the floor is lit warm; with the glow off, the room is black.
    let near = at(&lit, 120.0, 0.0);
    assert!(brightness(near) > brightness(at(&dark, 120.0, 0.0)) + 60, "near {near:?}");
    assert!(redness(near) > 20, "warm: {near:?}");
    // It fades with distance.
    let far = at(&lit, 400.0, 700.0);
    assert!(brightness(near) > brightness(far) + 40, "near {near:?} far {far:?}");
    // The ball itself shows its colours, unlit: as it is without any lighting.
    let mut plain = scene.clone();
    plain.things.get_mut(&ThingId::ROOT).unwrap().slots.remove(&SlotId::Lighting);
    let without = draw(&mut gpu, &plain, "glow-plain");
    let (a, b) = (at(&lit, 0.0, 0.0), at(&without, 0.0, 0.0));
    for c in 0..3 {
        assert!((a[c] as i32 - b[c] as i32).abs() <= 3, "{a:?} vs {b:?}");
    }
    // Without a Lighting card a Gives off light changes nothing.
    let mut plain_off = plain.clone();
    set_glow(&mut plain_off, "strength", Value::Number(0.0));
    assert_eq!(without, draw(&mut gpu, &plain_off, "glow-plain-off"));
}

#[test]
fn something_in_the_way_throws_a_shadow_from_a_glow() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let mut scene = scenes::glow_room(&reg);
    // A second ball, not glowing, between the glow and the floor below it.
    let other = bpe_kit::add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
    let t = scene.things.get_mut(&other).unwrap();
    t.transform.position = glam::Vec2::new(0.0, 200.0);
    t.params.set("radius", Value::Number(50.0));
    let pixels = draw(&mut gpu, &scene, "glow-shadow");
    // Below the blocker, against the same distance from the glow off to the side.
    let behind = at(&pixels, 0.0, 330.0);
    let side = at(&pixels, 330.0, 0.0);
    assert!(brightness(behind) + 30 < brightness(side), "behind {behind:?} side {side:?}");
}

#[test]
fn negative_light_takes_light_away() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let mut scene = scenes::glow_room(&reg);
    set_lighting(&mut scene, "ambient", Value::Number(1.0));
    set_glow(&mut scene, "strength", Value::Number(0.0));
    let plain = draw(&mut gpu, &scene, "neg-off");
    set_glow(&mut scene, "strength", Value::Number(3.0));
    set_glow(&mut scene, "negative", Value::Bool(true));
    let dark = draw(&mut gpu, &scene, "neg");
    let (a, b) = (at(&dark, 120.0, 0.0), at(&plain, 120.0, 0.0));
    assert!(brightness(a) + 30 < brightness(b), "negative {a:?} plain {b:?}");
}

#[test]
fn bounce_light_spills_a_lit_balls_colour() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let mut scene = scenes::lit_room(&reg);
    set_lighting(&mut scene, "ambient", Value::Number(0.0));
    // A big red ball lit by the lamp above it.
    let ball = ball_of(&scene);
    scene.things.get_mut(&ball).unwrap().params.set("radius", Value::Number(100.0));
    let body = scene.things[&ball].slots[&SlotId::Look].layers[0].id;
    let card = scene.card_mut(ball, SlotId::Look).unwrap();
    let b = bpe_kit::modules::look::layers::BodyLayer {
        fill: bpe_kit::paint::Paint::solid(Color::rgb(0xff, 0x10, 0x10)),
        ..Default::default()
    };
    for (k, v) in b.to_params().iter() {
        card.params.set(&layered_key(body, k), v.clone());
    }
    let before = draw(&mut gpu, &scene, "bounce-off");
    set_lighting(&mut scene, "bounce", Value::Number(2.0));
    let after = draw(&mut gpu, &scene, "bounce");
    // Beside the ball, near its lit top.
    let (a, b) = (at(&after, 120.0, -60.0), at(&before, 120.0, -60.0));
    assert!(redness(a) > redness(b) + 8, "bounce {a:?} without {b:?}");
}

#[test]
fn haze_shows_a_spots_beam_and_nothing_at_haze_0() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let mut scene = scenes::lit_room(&reg);
    let root = scene.things.get_mut(&ThingId::ROOT).unwrap();
    let mut s = bpe_kit::SceneSettings::from_params(&root.params);
    s.background = bpe_kit::paint::Paint::solid(Color::rgb(0, 0, 0));
    root.params = s.to_params();
    set_lighting(&mut scene, "ambient", Value::Number(0.0));
    set_light(&mut scene, "kind", Value::Choice("spot".into()));
    set_light(&mut scene, "cone", Value::Number(30.0));
    set_light(&mut scene, "cone_edge", Value::Number(2.0));
    set_light(&mut scene, "direction", Value::Number(90.0));
    let clear = draw(&mut gpu, &scene, "haze-off");
    set_lighting(&mut scene, "haze", Value::Number(0.0));
    assert_eq!(clear, draw(&mut gpu, &scene, "haze-0"));
    set_lighting(&mut scene, "haze", Value::Number(1.0));
    let hazy = draw(&mut gpu, &scene, "haze");
    let (inside, outside) = (at(&hazy, 0.0, -250.0), at(&hazy, 250.0, -300.0));
    assert!(brightness(inside) > brightness(at(&clear, 0.0, -250.0)) + 20, "{inside:?}");
    assert!(brightness(outside) <= 6, "{outside:?}");
    // A beam of 0 shows nothing in the haze.
    set_light(&mut scene, "beam", Value::Number(0.0));
    let no_beam = draw(&mut gpu, &scene, "haze-nobeam");
    let p = at(&no_beam, 0.0, -250.0);
    assert!(brightness(p) + 10 < brightness(inside), "{p:?}");
}

#[test]
fn light_blooms_brighten_round_a_lamp() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let mut scene = scenes::glow_room(&reg);
    let before = draw(&mut gpu, &scene, "blooms-off");
    set_lighting(&mut scene, "light_blooms", Value::Number(1.5));
    let after = draw(&mut gpu, &scene, "blooms");
    let (a, b) = (at(&after, 80.0, 0.0), at(&before, 80.0, 0.0));
    assert!(brightness(a) > brightness(b) + 15, "blooms {a:?} without {b:?}");
}

#[test]
fn a_glowing_background_lights_the_room() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let mut scene = scenes::glow_room(&reg);
    drop_glow(&mut scene);
    let dark = draw(&mut gpu, &scene, "bg-off");
    set_root(&mut scene, "bg_glow", Value::Number(1.0));
    set_root(&mut scene, "bg_glow_from", Value::Choice("own".into()));
    set_root(&mut scene, "bg_glow_colour", Value::Color(Color::rgb(0x40, 0x80, 0xff)));
    let lit = draw(&mut gpu, &scene, "bg");
    // The background shows its own colour, a lamp; the ball takes its blue light.
    let (a, b) = (at(&lit, 0.0, 30.0), at(&dark, 0.0, 30.0));
    assert!(brightness(a) > brightness(b) + 40, "{a:?} vs {b:?}");
    assert!(a[2] as i32 - a[0] as i32 > b[2] as i32 - b[0] as i32 + 20, "blue: {a:?} vs {b:?}");
    let (a, b) = (at(&lit, 300.0, 600.0), at(&dark, 300.0, 600.0));
    assert!(brightness(a) > brightness(b) + 40, "the background shows: {a:?} vs {b:?}");
}

#[test]
fn a_glowing_shape_lights_the_ball_inside_it() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let mut scene = scenes::glow_room(&reg);
    drop_glow(&mut scene);
    let before = draw(&mut gpu, &scene, "shape-off");
    let shape = scene.things.values().find(|t| t.kind == "shape").map(|t| t.id).unwrap();
    scene.things.get_mut(&shape).unwrap().params.set("thickness", Value::Number(30.0));
    let sbody = scene.things[&shape].slots[&SlotId::Look].layers[0].id;
    let sub = scene.add_sub_layer(&reg, shape, SlotId::Look, sbody, "shape.sub.light").unwrap();
    scene.card_mut(shape, SlotId::Look).unwrap().params.set(&layered_key(sub, "strength"), Value::Number(4.0));
    let after = draw(&mut gpu, &scene, "shape");
    // The floor near the ring's edge, inside it.
    let (a, b) = (at(&after, 0.0, 440.0), at(&before, 0.0, 440.0));
    assert!(brightness(a) > brightness(b) + 40, "{a:?} vs {b:?}");
    // And the ball in the middle takes some of it.
    let (a, b) = (at(&after, 0.0, 0.0), at(&before, 0.0, 0.0));
    assert!(brightness(a) > brightness(b) + 10, "ball {a:?} vs {b:?}");
}

#[test]
fn a_frame_knows_when_something_gives_off_light() {
    let reg = reg();
    let kit = kit();
    let scene = scenes::glow_room(&reg);
    let frame = frame_of(&scene, &kit, 0.0);
    assert!(frame.light.as_ref().unwrap().glows);
    let mut off = scene.clone();
    let (ball, sub) = glow_of(&off);
    off.remove_layer(&reg, ball, SlotId::Look, sub).unwrap();
    let frame = frame_of(&off, &kit, 0.0);
    assert!(!frame.light.as_ref().unwrap().glows);
}

#[test]
fn a_frame_with_nothing_glowing_keeps_no_glow_from_the_one_before() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let mut scene = scenes::glow_room(&reg);
    set_lighting(&mut scene, "light_blooms", Value::Number(1.0));
    set_lighting(&mut scene, "bounce", Value::Number(1.0));
    set_light(&mut scene, "strength", Value::Number(1.0));
    draw(&mut gpu, &scene, "stale-before");
    drop_glow(&mut scene);
    let after = draw(&mut gpu, &scene, "stale-after");
    let Some(mut fresh) = self::gpu() else { return };
    assert!(after == draw(&mut fresh, &scene, "stale-fresh"), "the same as drawn afresh");
}

#[test]
fn a_shine_keeps_its_white_on_a_ball_in_the_dark() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let mut scene = scenes::glow_room(&reg);
    drop_glow(&mut scene);
    // A big red ball with the classic shine, in a room with no light at all.
    let ball = ball_of(&scene);
    scene.things.get_mut(&ball).unwrap().params.set("radius", Value::Number(200.0));
    let body = scene.things[&ball].slots[&SlotId::Look].layers[0].id;
    let b = bpe_kit::modules::look::layers::BodyLayer {
        fill: bpe_kit::paint::Paint::solid(Color::rgb(0xff, 0x10, 0x10)),
        ..Default::default()
    };
    let card = scene.card_mut(ball, SlotId::Look).unwrap();
    for (k, v) in b.to_params().iter() {
        card.params.set(&layered_key(body, k), v.clone());
    }
    let pixels = draw(&mut gpu, &scene, "shine-dark");
    let mut brightest = [0u8; 4];
    let mut reddest = 0;
    for y in -180..180 {
        for x in -180..180 {
            if x * x + y * y > 170 * 170 {
                continue;
            }
            let p = at(&pixels, x as f32, y as f32);
            if brightness(p) > brightness(brightest) {
                brightest = p;
            }
            reddest = reddest.max(redness(p));
        }
    }
    // Its highlight shows white (never cyan), and the red body takes no light: dark.
    assert!(brightness(brightest) > 300, "a highlight: {brightest:?}");
    assert!((brightest[0] as i32 - brightest[1] as i32).abs() < 40, "white, not cyan: {brightest:?}");
    let middle_low = at(&pixels, 0.0, 150.0);
    assert!(brightness(middle_low) < 60, "the body in the dark: {middle_low:?}");
    let _ = reddest;
}

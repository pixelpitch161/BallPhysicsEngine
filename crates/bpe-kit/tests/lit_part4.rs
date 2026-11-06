//! Lighting part 4 in pictures: lasers, gobos, ultraviolet, Lights up, Picked colour, moving haze,
//! underwater light, glow in the dark and heat glow. Measured on the cloud's software GPU; the
//! checks compare pixels with each other, so they hold on any GPU.
//!
//! `BPE_SAVE_RENDERS=1` saves the pictures into `target/pictures/lit4-*.png`.

use bpe_core::{Color, Registry, Scene, SlotId, Target, ThingId, Value, layered_key};
use bpe_gfx::{Frame, Renderer, build_frame, headless_device};
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

fn frame_of(scene: &Scene, t: f64) -> Frame {
    let kit = kit();
    let resolved = bpe_core::resolve(scene, &kit.registry);
    let mut playback = Playback::new(scene, &resolved, kit.sims.clone());
    playback.seek(t);
    let mut frame = Frame::default();
    build_frame(scene, &resolved, playback.world(), playback.log(), t, &kit.draws, &mut frame);
    frame
}

fn render(gpu: &mut Gpu, frame: &Frame, name: &str) -> Vec<u8> {
    gpu.renderer.render(&gpu.device, &gpu.queue, frame);
    if gpu.renderer.programs_waiting() {
        gpu.renderer.render(&gpu.device, &gpu.queue, frame);
    }
    let pixels = gpu.renderer.read_output(&gpu.device, &gpu.queue).unwrap();
    if std::env::var_os("BPE_SAVE_RENDERS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pictures");
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(dir.join(format!("lit4-{name}.png"))).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), W, H);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(&pixels).unwrap();
    }
    pixels
}

fn draw_at(gpu: &mut Gpu, scene: &Scene, t: f64, name: &str) -> Vec<u8> {
    let frame = frame_of(scene, t);
    render(gpu, &frame, name)
}

fn draw(gpu: &mut Gpu, scene: &Scene, name: &str) -> Vec<u8> {
    draw_at(gpu, scene, 0.0, name)
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

fn n(v: f32) -> Value {
    Value::Number(v as f64)
}

fn choice(c: &str) -> Value {
    Value::Choice(c.into())
}

/// The lit room in the dark: no ambient light, so only the lamp lights anything.
fn dark_room() -> Scene {
    let mut scene = scenes::lit_room(&reg());
    set_lighting(&mut scene, "ambient", n(0.0));
    scene
}

/// The ball's Body layer id.
fn body_of(scene: &Scene) -> (ThingId, bpe_core::LayerId) {
    let ball = ball_of(scene);
    let card = &scene.things[&ball].slots[&SlotId::Look];
    (ball, card.layers.iter().find(|l| l.kind.as_str() == "layer.body").expect("a Body").id)
}

/// Sets `key` on every layer of the ball's Look that has it (Body, Shine and Glow in the lit room).
fn set_layers(scene: &mut Scene, key: &str, v: Value) {
    let ball = ball_of(scene);
    let ids: Vec<_> = scene.things[&ball].slots[&SlotId::Look].layers.iter().map(|l| l.id).collect();
    let card = scene.card_mut(ball, SlotId::Look).unwrap();
    for id in ids {
        card.params.set(&layered_key(id, key), v.clone());
    }
}

fn set_body(scene: &mut Scene, key: &str, v: Value) {
    let (ball, body) = body_of(scene);
    scene.card_mut(ball, SlotId::Look).unwrap().params.set(&layered_key(body, key), v);
}

// ---------- lasers ----------

#[test]
fn a_laser_draws_a_line_along_its_direction_and_nothing_beside_it() {
    let Some(mut gpu) = gpu() else { return };
    let mut scene = dark_room();
    set_light(&mut scene, "kind", choice("laser"));
    set_light(&mut scene, "position", Value::Vec2([-300.0, -300.0]));
    set_light(&mut scene, "direction", n(0.0));
    set_light(&mut scene, "bounces", Value::Int(0));
    let pixels = draw(&mut gpu, &scene, "laser");
    let on = at(&pixels, 0.0, -300.0);
    let beside = at(&pixels, 0.0, -180.0);
    assert!(brightness(on) > brightness(beside) + 80, "on {on:?} beside {beside:?}");
    // Its colour: the laser's red.
    assert!(on[0] > on[1] + 40, "{on:?}");
}

#[test]
fn a_laser_stops_at_a_ball_or_passes_through_it() {
    let Some(mut gpu) = gpu() else { return };
    let mut scene = dark_room();
    set_light(&mut scene, "kind", choice("laser"));
    set_light(&mut scene, "position", Value::Vec2([0.0, -450.0]));
    set_light(&mut scene, "direction", n(90.0));
    set_light(&mut scene, "bounces", Value::Int(0));
    set_light(&mut scene, "at_balls", choice("stop"));
    let stops = draw(&mut gpu, &scene, "laser-stop");
    set_light(&mut scene, "at_balls", choice("pass_through"));
    let passes = draw(&mut gpu, &scene, "laser-pass");
    let (a, b) = (at(&stops, 0.0, 300.0), at(&passes, 0.0, 300.0));
    assert!(brightness(b) > brightness(a) + 80, "stopped {a:?} passed {b:?}");
    // Above the ball the beam is there either way.
    assert!(brightness(at(&stops, 0.0, -300.0)) > 120);
}

#[test]
fn a_laser_bounces_off_the_shape() {
    let reg = reg();
    let mut scene = dark_room();
    set_light(&mut scene, "kind", choice("laser"));
    set_light(&mut scene, "position", Value::Vec2([-300.0, -300.0]));
    set_light(&mut scene, "direction", n(0.0));
    set_light(&mut scene, "bounces", Value::Int(3));
    let _ = reg;
    let frame = frame_of(&scene, 0.0);
    let lit = frame.light.as_ref().unwrap();
    let l = &lit.lights[0];
    assert_eq!(l.p0[3], 5.0, "a laser");
    assert_eq!(l.p4[3], 4.0, "three bounces: four pieces of beam");
    // The first piece ends on the circle (radius 508, its band 1.5 px inside).
    let seg = lit.segments[l.p4[2] as usize];
    let end = glam::Vec2::new(seg[2], seg[3]);
    assert!((end.length() - 506.5).abs() < 1.0, "{end:?}");
}

// ---------- gobos and flicker ----------

#[test]
fn a_gobo_breaks_the_light_into_its_pattern() {
    let Some(mut gpu) = gpu() else { return };
    let mut scene = dark_room();
    set_light(&mut scene, "reach", n(2000.0));
    let plain = draw(&mut gpu, &scene, "gobo-off");
    set_light(&mut scene, "gobo", choice("stripes"));
    set_light(&mut scene, "gobo_size", n(80.0));
    set_light(&mut scene, "gobo_sharpness", n(0.9));
    let striped = draw(&mut gpu, &scene, "gobo-stripes");
    // Along a line under the lamp, the light falls in stripes: some spots keep it, some lose it.
    let ratios: Vec<f32> = (0..24)
        .map(|k| {
            let x = -400.0 + 800.0 * k as f32 / 23.0;
            let (s, p) = (at(&striped, x, -200.0), at(&plain, x, -200.0));
            brightness(s) as f32 / brightness(p).max(1) as f32
        })
        .collect();
    let lo = ratios.iter().copied().fold(f32::MAX, f32::min);
    let hi = ratios.iter().copied().fold(0.0, f32::max);
    assert!(lo < 0.4 && hi > 0.8, "{ratios:?}");
}

#[test]
fn a_flickering_light_changes_from_frame_to_frame_the_same_every_time() {
    let mut scene = dark_room();
    set_light(&mut scene, "flicker", n(0.8));
    set_light(&mut scene, "flicker_style", choice("faulty"));
    let strengths: Vec<f32> =
        (0..60).map(|k| frame_of(&scene, k as f64 / 30.0).light.unwrap().lights[0].p1[0]).collect();
    let again: Vec<f32> = (0..60).map(|k| frame_of(&scene, k as f64 / 30.0).light.unwrap().lights[0].p1[0]).collect();
    assert_eq!(strengths, again, "pure in time: scrubbing and export agree");
    let lo = strengths.iter().copied().fold(f32::MAX, f32::min);
    let hi = strengths.iter().copied().fold(0.0, f32::max);
    assert!(lo < 0.6 * hi, "{strengths:?}");
}

// ---------- ultraviolet ----------

#[test]
fn only_under_uv_hides_a_layer_until_uv_light_falls_on_it() {
    let Some(mut gpu) = gpu() else { return };
    let mut scene = dark_room();
    set_lighting(&mut scene, "ambient", n(0.3));
    set_layers(&mut scene, "under_uv", choice("only_under_uv"));
    let hidden = draw(&mut gpu, &scene, "uv-hidden");
    set_light(&mut scene, "ultraviolet", Value::Bool(true));
    let shown = draw(&mut gpu, &scene, "uv-shown");
    // The ball's middle: the floor shows through it in ordinary light, and it glows pink in UV.
    let (h, s) = (at(&hidden, 0.0, 0.0), at(&shown, 0.0, 0.0));
    assert!(brightness(s) > brightness(h) + 60, "hidden {h:?} shown {s:?}");
    assert!(s[0] > s[1] + 30, "pink: {s:?}");
    // Without a Lighting card the layer shows as it always did.
    let mut plain = scenes::lit_room(&reg());
    plain.things.get_mut(&ThingId::ROOT).unwrap().slots.remove(&SlotId::Lighting);
    set_layers(&mut plain, "under_uv", choice("only_under_uv"));
    let frame = frame_of(&plain, 0.0);
    assert!(frame.light.is_none());
    assert!(frame.look_layers.iter().all(|r| r.more == [0.0; 4]), "no part 4 data without the card");
}

#[test]
fn uv_light_makes_a_glows_layer_glow_and_an_ordinary_one_stay_dark() {
    let Some(mut gpu) = gpu() else { return };
    let mut scene = dark_room();
    set_light(&mut scene, "ultraviolet", Value::Bool(true));
    set_light(&mut scene, "uv_visible", n(0.0));
    let plain = draw(&mut gpu, &scene, "uv-plain");
    set_body(&mut scene, "under_uv", choice("glows"));
    let glowing = draw(&mut gpu, &scene, "uv-glows");
    let (p, g) = (at(&plain, 0.0, 0.0), at(&glowing, 0.0, 0.0));
    assert!(brightness(g) > brightness(p) + 80, "plain {p:?} glowing {g:?}");
}

// ---------- Lights up ----------

#[test]
fn a_light_that_lights_only_the_ball_leaves_the_floor_dark() {
    let Some(mut gpu) = gpu() else { return };
    let mut scene = dark_room();
    let all = draw(&mut gpu, &scene, "links-all");
    let ball = ball_of(&scene);
    set_light(&mut scene, "lights_up", choice("only_these"));
    set_light(&mut scene, "link_things", Value::Target(Target::Thing(ball)));
    set_light(&mut scene, "link_background", Value::Bool(false));
    let only = draw(&mut gpu, &scene, "links-only");
    let floor = (at(&all, 150.0, -400.0), at(&only, 150.0, -400.0));
    assert!(brightness(floor.0) > brightness(floor.1) + 60, "floor {floor:?}");
    let on_ball = (at(&all, 0.0, -20.0), at(&only, 0.0, -20.0));
    assert!(brightness(on_ball.1) + 20 >= brightness(on_ball.0), "ball {on_ball:?}");
    // Everything but the ball: the floor lit, the ball dark.
    set_light(&mut scene, "lights_up", choice("everything_but_these"));
    set_light(&mut scene, "link_background", Value::Bool(true));
    let but = draw(&mut gpu, &scene, "links-but");
    assert!(brightness(at(&but, 150.0, -400.0)) + 20 >= brightness(floor.0));
    assert!(brightness(at(&but, 0.0, -20.0)) + 60 < brightness(on_ball.0));
}

// ---------- Picked colour ----------

#[test]
fn picked_colour_takes_the_balls_colour() {
    let mut scene = dark_room();
    let ball = ball_of(&scene);
    set_light(&mut scene, "colour_from", choice("picked"));
    set_light(&mut scene, "pick_from", Value::Target(Target::Thing(ball)));
    set_light(&mut scene, "colour", Value::Color(Color::WHITE));
    let frame = frame_of(&scene, 0.0);
    let c = frame.light.unwrap().lights[0].p1;
    // The ball is pink (#FF5CA8): red over blue over green.
    assert!(c[0] > c[2] && c[2] > c[1], "{c:?}");
}

// ---------- the air ----------

#[test]
fn drifting_haze_is_uneven_and_moves() {
    let Some(mut gpu) = gpu() else { return };
    let mut scene = dark_room();
    set_lighting(&mut scene, "haze", n(1.0));
    set_light(&mut scene, "reach", n(1500.0));
    let still = draw(&mut gpu, &scene, "haze-still");
    set_lighting(&mut scene, "haze_moves", choice("drifting"));
    set_lighting(&mut scene, "patchiness", n(1.0));
    set_lighting(&mut scene, "haze_size", n(150.0));
    let a = draw_at(&mut gpu, &scene, 0.0, "haze-drift-0");
    let b = draw_at(&mut gpu, &scene, 1.0, "haze-drift-1");
    assert_ne!(still, a, "moving haze differs from still haze");
    assert_ne!(a, b, "and moves");
}

#[test]
fn underwater_light_ripples_and_turns_the_waters_colour_deeper_down() {
    let Some(mut gpu) = gpu() else { return };
    let mut scene = dark_room();
    set_light(&mut scene, "reach", n(3000.0));
    let dry = draw(&mut gpu, &scene, "water-off");
    set_lighting(&mut scene, "underwater", n(1.0));
    set_lighting(&mut scene, "water_top", n(-900.0));
    set_lighting(&mut scene, "depth_fade", n(600.0));
    set_lighting(&mut scene, "water_colour", Value::Color(Color::rgb(0x10, 0x60, 0xff)));
    let wet = draw(&mut gpu, &scene, "water");
    assert_ne!(dry, wet);
    let deep = at(&wet, 300.0, 300.0);
    assert!(deep[2] > deep[0] + 20, "deep water is blue: {deep:?}");
}

#[test]
fn glow_in_the_dark_keeps_the_light_after_it_goes() {
    let Some(mut gpu) = gpu() else { return };
    let mut scene = dark_room();
    set_lighting(&mut scene, "glow_dark", n(1.0));
    set_lighting(&mut scene, "charge", n(10.0));
    set_lighting(&mut scene, "glow_fade", n(5.0));
    // Half a second with the lamp on charges the floor.
    for k in 0..15 {
        let frame = frame_of(&scene, k as f64 / 30.0);
        gpu.renderer.render(&gpu.device, &gpu.queue, &frame);
    }
    set_light(&mut scene, "strength", n(0.0));
    let frame = frame_of(&scene, 0.5);
    let after = render(&mut gpu, &frame, "memory-after");
    assert!(!gpu.renderer.memory_cold());
    // The same dark frame drawn after a seek: nothing remembered.
    let mut fresh = self::gpu().unwrap();
    let cold = render(&mut fresh, &frame, "memory-cold");
    assert!(fresh.renderer.memory_cold());
    let near = (at(&after, 150.0, -400.0), at(&cold, 150.0, -400.0));
    assert!(brightness(near.0) > brightness(near.1) + 30, "{near:?}");
    // It glows green, its own Glow colour.
    assert!(near.0[1] > near.0[0], "{near:?}");
}

// ---------- heat glow ----------

/// The lit room with a Heat glow on the ball's Body.
fn hot_ball() -> Scene {
    let reg = reg();
    let mut scene = dark_room();
    let (ball, body) = body_of(&scene);
    let sub = scene.add_sub_layer(&reg, ball, SlotId::Look, body, "sub.heat").expect("the Body takes Heat glow");
    let card = scene.card_mut(ball, SlotId::Look).unwrap();
    card.params.set(&layered_key(sub, "per_hit"), n(2.0));
    card.params.set(&layered_key(sub, "cool_down"), n(5.0));
    scene
}

#[test]
fn a_ball_heats_up_as_it_hits_the_wall() {
    let scene = hot_ball();
    let frame = frame_of(&scene, 0.0);
    assert_eq!(frame.layered[0].hit[3], 0.0, "cold before any hit");
    let frame = frame_of(&scene, 4.0);
    assert!(
        frame.layered.iter().any(|b| b.hit[3] > 0.1),
        "{:?}",
        frame.layered.iter().map(|b| b.hit).collect::<Vec<_>>()
    );
}

#[test]
fn a_hot_ball_glows_and_lights_the_room() {
    let Some(mut gpu) = gpu() else { return };
    let mut scene = hot_ball();
    set_light(&mut scene, "strength", n(0.0));
    let t = 4.0;
    let frame = frame_of(&scene, t);
    let ball = frame.layered.iter().max_by(|a, b| a.hit[3].total_cmp(&b.hit[3])).unwrap();
    let centre = ball.center;
    let hot = render(&mut gpu, &frame, "heat");
    let mut cold = hot_ball();
    set_light(&mut cold, "strength", n(0.0));
    let (b, sub) = {
        let card = &cold.things[&ball_of(&cold)].slots[&SlotId::Look];
        (ball_of(&cold), card.layers.iter().find(|l| l.kind.as_str() == "sub.heat").unwrap().id)
    };
    cold.card_mut(b, SlotId::Look).unwrap().params.set(&layered_key(sub, "per_hit"), n(0.0));
    let dark = render(&mut gpu, &frame_of(&cold, t), "heat-cold");
    let (h, c) = (at(&hot, centre[0], centre[1]), at(&dark, centre[0], centre[1]));
    assert!(brightness(h) > brightness(c) + 40, "hot {h:?} cold {c:?}");
    assert!(h[0] >= h[2], "red to white hot: {h:?}");
}

// ---------- smoke ----------

#[test]
fn a_trail_that_catches_beams_asks_for_the_tags_picture() {
    let reg = reg();
    let mut scene = dark_room();
    let ball = ball_of(&scene);
    let trail = scene.add_layer(&reg, ball, SlotId::Look, "layer.trail").unwrap();
    let frame = frame_of(&scene, 1.0);
    assert!(!frame.light.as_ref().unwrap().smoke);
    scene.card_mut(ball, SlotId::Look).unwrap().params.set(&layered_key(trail, "catches_beams"), n(1.0));
    let frame = frame_of(&scene, 1.0);
    assert!(frame.light.as_ref().unwrap().smoke);
    assert!(frame.trails.iter().chain(&frame.trails_front).any(|v| v.extra[2] == 1.0));
}

/// Writes part 4's demo scenes to `BPE_CLUB_SCENE` and `BPE_WATER_SCENE` (for the review video),
/// when set.
#[test]
fn write_the_demo_scenes() {
    let reg = reg();
    if let Some(path) = std::env::var_os("BPE_CLUB_SCENE") {
        std::fs::write(path, bpe_core::save::save(&scenes::club_demo(&reg))).unwrap();
    }
    if let Some(path) = std::env::var_os("BPE_WATER_SCENE") {
        std::fs::write(path, bpe_core::save::save(&scenes::water_demo(&reg))).unwrap();
    }
}

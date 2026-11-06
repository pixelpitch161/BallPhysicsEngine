//! Lighting part 2a in pictures: the 3D stage (spheres before a back wall, a floor, shapes standing
//! out of the wall) and the base surfaces. Measured on the cloud's software GPU; the checks compare
//! pixels with each other, so they hold on any GPU.
//!
//! `BPE_SAVE_RENDERS=1` saves the pictures into `target/pictures/stage-*.png`.

use bpe_core::{Choice, Registry, Scene, Settings, SlotId, ThingId, Value};
use bpe_gfx::{Frame, Renderer, build_frame, headless_device};
use bpe_kit::modules::look::surface::{SurfaceKind, preset_values};
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

fn draw(gpu: &mut Gpu, scene: &Scene, name: &str) -> Vec<u8> {
    let frame = frame_of(scene, 0.0);
    gpu.renderer.render(&gpu.device, &gpu.queue, &frame);
    if gpu.renderer.programs_waiting() {
        gpu.renderer.render(&gpu.device, &gpu.queue, &frame);
    }
    let pixels = gpu.renderer.read_output(&gpu.device, &gpu.queue).unwrap();
    if std::env::var_os("BPE_SAVE_RENDERS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pictures");
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(dir.join(format!("stage-{name}.png"))).unwrap();
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

fn reg() -> Registry {
    kit().registry
}

fn thing_of(scene: &Scene, kind: &str) -> ThingId {
    scene.things.values().find(|t| t.kind == kind).map(|t| t.id).expect("a thing of the kind")
}

fn set_light(scene: &mut Scene, key: &str, v: Value) {
    let light = thing_of(scene, "light");
    scene.things.get_mut(&light).unwrap().params.set(key, v);
}

fn set_lighting(scene: &mut Scene, key: &str, v: Value) {
    scene.things.get_mut(&ThingId::ROOT).unwrap().slots.get_mut(&SlotId::Lighting).unwrap().params.set(key, v);
}

fn set_surface(scene: &mut Scene, thing: ThingId, kind: SurfaceKind) {
    let card = scene.card_mut(thing, SlotId::Look).unwrap();
    card.params.set("surface", Value::Choice(kind.name().into()));
    for (k, v) in preset_values(kind) {
        card.params.set(k, v);
    }
}

/// The lit room on a stage with no floor: the ball (at the middle) before a wall 240 px behind,
/// one lamp up and to the left, 300 px in front.
fn stage_room() -> (Scene, f32) {
    let reg = reg();
    let mut scene = scenes::lit_room(&reg);
    set_lighting(&mut scene, "depth", Value::Choice("stage".into()));
    set_lighting(&mut scene, "floor", Value::Choice("none".into()));
    set_lighting(&mut scene, "ambient", Value::Number(0.1));
    set_light(&mut scene, "position", Value::Vec2([-300.0, -400.0]));
    set_light(&mut scene, "height", Value::Number(300.0));
    set_light(&mut scene, "reach", Value::Number(3000.0));
    // A big plain ball: its Body alone (the test ball's Shine and Glow are lights of their own).
    let ball = thing_of(&scene, "balls");
    scene.things.get_mut(&ball).unwrap().params.set("radius", Value::Number(100.0));
    let look = scene.card_mut(ball, SlotId::Look).unwrap();
    look.layers.retain(|l| l.kind == "layer.body");
    let radius = frame_of(&scene, 0.0).light.unwrap().spheres[0].a[2];
    (scene, radius)
}

#[test]
fn a_stage_frame_has_a_sphere_per_ball_and_a_flat_one_none() {
    let (scene, radius) = stage_room();
    assert!(radius > 5.0, "{radius}");
    let lit = frame_of(&scene, 0.0).light.unwrap();
    assert_eq!(lit.spheres.len(), 1);
    assert_eq!(lit.settings.stage[0], 1.0);
    // Two things have a Look: the ball and the shape, surfaces 1 and 2.
    assert!(lit.surfaces.len() >= 3, "{:?}", lit.surfaces);
    let flat = frame_of(&scenes::lit_room(&reg()), 0.0).light.unwrap();
    assert!(flat.spheres.is_empty());
    assert_eq!(flat.settings.stage[0], 0.0);
}

#[test]
fn a_ball_throws_its_shadow_back_onto_the_wall() {
    let Some(mut gpu) = gpu() else { return };
    let (scene, _) = stage_room();
    let pixels = draw(&mut gpu, &scene, "wall-shadow");
    // From the lamp (-300, -400, 300) through the ball's middle to the wall at z = -240.
    let shadow = at(&pixels, 240.0, 320.0);
    let beside = at(&pixels, 440.0, 170.0);
    assert!(brightness(shadow) + 25 < brightness(beside), "shadow {shadow:?} beside {beside:?}");
}

#[test]
fn the_ball_is_lit_on_the_side_facing_the_lamp_and_not_shadowed_by_itself() {
    let Some(mut gpu) = gpu() else { return };
    let (scene, r) = stage_room();
    let pixels = draw(&mut gpu, &scene, "ball-sides");
    let toward = at(&pixels, -0.4 * r, -0.5 * r);
    let away = at(&pixels, 0.5 * r, 0.5 * r);
    assert!(brightness(toward) > brightness(away) + 40, "toward {toward:?} away {away:?}");
}

#[test]
fn a_floor_under_a_resting_ball_is_dark_where_it_touches() {
    let Some(mut gpu) = gpu() else { return };
    let (mut scene, r) = stage_room();
    set_lighting(&mut scene, "floor", Value::Choice("at_a_height".into()));
    set_lighting(&mut scene, "floor_at", Value::Number(r as f64));
    set_lighting(&mut scene, "floor_surface", Value::Choice("matte".into()));
    set_lighting(&mut scene, "contact", Value::Number(1.0));
    // Only the room's even light, so the floor is as bright everywhere but where the ball shades it.
    set_light(&mut scene, "strength", Value::Number(0.0));
    set_lighting(&mut scene, "ambient", Value::Number(0.6));
    let pixels = draw(&mut gpu, &scene, "floor-contact");
    let under = at(&pixels, 0.0, r + 6.0);
    let away = at(&pixels, 300.0, r + 6.0);
    assert!(brightness(under) + 20 < brightness(away), "under {under:?} away {away:?}");
}

#[test]
fn a_mirror_floor_shows_the_ball_standing_on_it() {
    let Some(mut gpu) = gpu() else { return };
    let (mut scene, r) = stage_room();
    set_lighting(&mut scene, "floor", Value::Choice("at_a_height".into()));
    set_lighting(&mut scene, "floor_at", Value::Number(r as f64));
    set_lighting(&mut scene, "ambient", Value::Number(0.6));
    let pinkness = |p: [u8; 4]| p[0] as i32 - p[1] as i32;
    let mut looks = Vec::new();
    for surface in ["matte", "glossy", "mirror"] {
        set_lighting(&mut scene, "floor_surface", Value::Choice(surface.into()));
        let pixels = draw(&mut gpu, &scene, &format!("floor-{surface}"));
        // Below the floor line, mirroring the ball's lower half.
        looks.push(pinkness(at(&pixels, 0.0, 1.5 * r)));
    }
    assert!(looks[1] > looks[0] + 8, "glossy {looks:?}");
    assert!(looks[2] > looks[1] + 20, "mirror {looks:?}");
}

/// The brightest of a few points across the ball.
fn brightest_on_ball(pixels: &[u8], r: f32) -> u32 {
    let mut best = 0;
    for i in -4..=4 {
        for j in -4..=4 {
            let (x, y) = (i as f32 * 0.2 * r, j as f32 * 0.2 * r);
            if x * x + y * y < 0.8 * r * r {
                best = best.max(brightness(at(pixels, x, y)));
            }
        }
    }
    best
}

#[test]
fn plastic_shines_where_matte_does_not() {
    let Some(mut gpu) = gpu() else { return };
    for depth in ["flat", "stage"] {
        let (mut scene, r) = stage_room();
        set_lighting(&mut scene, "depth", Value::Choice(depth.into()));
        let ball = thing_of(&scene, "balls");
        let matte = brightest_on_ball(&draw(&mut gpu, &scene, &format!("matte-{depth}")), r);
        set_surface(&mut scene, ball, SurfaceKind::Plastic);
        let plastic = brightest_on_ball(&draw(&mut gpu, &scene, &format!("plastic-{depth}")), r);
        assert!(plastic > matte + 20, "{depth}: plastic {plastic} matte {matte}");
    }
}

#[test]
fn chrome_mirrors_the_sky_bright_above_and_dark_below() {
    let Some(mut gpu) = gpu() else { return };
    let (mut scene, r) = stage_room();
    set_light(&mut scene, "strength", Value::Number(0.0));
    set_lighting(&mut scene, "sky_light", Value::Number(1.0));
    let ball = thing_of(&scene, "balls");
    set_surface(&mut scene, ball, SurfaceKind::Chrome);
    let pixels = draw(&mut gpu, &scene, "chrome");
    let top = at(&pixels, 0.0, -0.75 * r);
    let bottom = at(&pixels, 0.0, 0.75 * r);
    assert!(brightness(top) > brightness(bottom) + 60, "top {top:?} bottom {bottom:?}");
}

#[test]
fn toon_light_falls_in_a_few_bands() {
    let Some(mut gpu) = gpu() else { return };
    let (mut scene, r) = stage_room();
    let ball = thing_of(&scene, "balls");
    // Steps across the ball, corner to corner, where the light hardly changes: in bands most do.
    let flat_steps = |pixels: &[u8]| {
        let b: Vec<u32> = (-6..=6).map(|k| brightness(at(pixels, k as f32 * 0.1 * r, k as f32 * 0.1 * r))).collect();
        b.windows(2).filter(|w| w[0].abs_diff(w[1]) <= 2).count()
    };
    let matte = flat_steps(&draw(&mut gpu, &scene, "toon-before"));
    set_surface(&mut scene, ball, SurfaceKind::Toon);
    let toon = flat_steps(&draw(&mut gpu, &scene, "toon"));
    assert!(toon >= matte + 4, "toon {toon} flat steps of 12, matte {matte}");
}

#[test]
fn a_glass_ball_lets_light_through_its_shadow() {
    let Some(mut gpu) = gpu() else { return };
    let (mut scene, _) = stage_room();
    let ball = thing_of(&scene, "balls");
    let matte = brightness(at(&draw(&mut gpu, &scene, "glass-before"), 240.0, 320.0));
    set_surface(&mut scene, ball, SurfaceKind::Glass);
    let glass = brightness(at(&draw(&mut gpu, &scene, "glass"), 240.0, 320.0));
    assert!(glass > matte + 15, "glass {glass} matte {matte}");
}

#[test]
fn a_shape_standing_out_of_the_wall_shadows_it() {
    let Some(mut gpu) = gpu() else { return };
    let (mut scene, _) = stage_room();
    let shape = thing_of(&scene, "shape");
    scene.things.get_mut(&shape).unwrap().params.set("thickness", Value::Number(40.0));
    set_light(&mut scene, "position", Value::Vec2([0.0, -400.0]));
    let pixels = draw(&mut gpu, &scene, "shape-shadow");
    // As far from the lamp each way: out past the ring's top, and in toward the ball.
    let outside = at(&pixels, 0.0, -560.0);
    let inside = at(&pixels, 0.0, -240.0);
    assert!(brightness(outside) + 25 < brightness(inside), "outside {outside:?} inside {inside:?}");
}

#[test]
fn a_light_behind_the_balls_lights_their_rims() {
    let Some(mut gpu) = gpu() else { return };
    let (mut scene, r) = stage_room();
    set_lighting(&mut scene, "ambient", Value::Number(0.0));
    set_light(&mut scene, "position", Value::Vec2([0.0, -200.0]));
    set_light(&mut scene, "height", Value::Number(-100.0));
    let pixels = draw(&mut gpu, &scene, "rim");
    let rim = at(&pixels, 0.0, -0.93 * r);
    let middle = at(&pixels, 0.0, 0.1 * r);
    assert!(brightness(rim) > brightness(middle) + 30, "rim {rim:?} middle {middle:?}");
}

/// Writes the stage demo scene to `BPE_STAGE_DEMO` (for the review video), when set.
#[test]
fn write_the_stage_demo_scene() {
    let Some(path) = std::env::var_os("BPE_STAGE_DEMO") else { return };
    std::fs::write(path, bpe_core::save::save(&scenes::stage_demo(&reg()))).unwrap();
}

#[test]
fn the_stage_demo_draws() {
    let scene = scenes::stage_demo(&reg());
    let lit = frame_of(&scene, 1.0).light.unwrap();
    assert_eq!(lit.settings.stage[0], 1.0);
    assert!(lit.spheres.len() >= 5);
    // Every Look's surface is in the table, a ball's as a ball.
    assert!(lit.surfaces.iter().any(|s| s.b[3] == bpe_gfx::SurfaceGpu::BALL && s.a[1] == 1.0), "chrome");
    let _ = bpe_kit::modules::look::surface::SurfaceSettings::defs();
}

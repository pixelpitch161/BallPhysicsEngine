//! Lighting part 2b in pictures: glass that bends what is behind it, live reflections, mirror
//! walls, depth of field, 2a's gaps and the advanced surfaces. Measured on the cloud's software
//! GPU; the checks compare pixels with each other, so they hold on any GPU.
//!
//! `BPE_SAVE_RENDERS=1` saves the pictures into `target/pictures/glass-*.png`.

use bpe_core::{Choice, Color, Gradient, Registry, Scene, Settings, SlotId, ThingId, Value, layered_key};
use bpe_gfx::{Frame, Renderer, build_frame, headless_device};
use bpe_kit::modules::look::surface::{SurfaceKind, preset_values};
use bpe_kit::paint::Paint;
use bpe_kit::{SceneSettings, kit, scenes};
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
        let file = std::fs::File::create(dir.join(format!("glass-{name}.png"))).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), W, H);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(&pixels).unwrap();
    }
    pixels
}

/// The pixel at world point (x, y): the video is 1080 × 1920 round the origin.
fn at(pixels: &[u8], x: f32, y: f32) -> [u8; 4] {
    let px = (((x / 1080.0 + 0.5) * W as f32) as usize).min(W as usize - 1);
    let py = (((y / 1920.0 + 0.5) * H as f32) as usize).min(H as usize - 1);
    let i = (py * W as usize + px) * 4;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

fn brightness(p: [u8; 4]) -> u32 {
    p[0] as u32 + p[1] as u32 + p[2] as u32
}

fn redness(p: [u8; 4]) -> i32 {
    p[0] as i32 - (p[1] as i32 + p[2] as i32) / 2
}

fn reg() -> Registry {
    kit().registry
}

fn things_of(scene: &Scene, kind: &str) -> Vec<ThingId> {
    scene.things.values().filter(|t| t.kind == kind).map(|t| t.id).collect()
}

fn thing_of(scene: &Scene, kind: &str) -> ThingId {
    things_of(scene, kind)[0]
}

fn set_light(scene: &mut Scene, key: &str, v: Value) {
    let light = thing_of(scene, "light");
    scene.things.get_mut(&light).unwrap().params.set(key, v);
}

fn set_lighting(scene: &mut Scene, key: &str, v: Value) {
    scene.card_mut(ThingId::ROOT, SlotId::Lighting).unwrap().params.set(key, v);
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

fn set_background(scene: &mut Scene, paint: Paint) {
    let root = scene.things.get_mut(&ThingId::ROOT).unwrap();
    let mut s = SceneSettings::from_params(&root.params);
    s.background = paint;
    root.params = s.to_params();
}

/// Black and white stripes running up the picture.
fn stripes() -> Paint {
    let (k, w) = (Color::rgb(0x10, 0x10, 0x10), Color::rgb(0xf0, 0xf0, 0xf0));
    Paint { colours: Gradient::even(&[k, w, k, w, k, w, k, w, k]), angle: 0.0, span: 1080.0, ..Paint::default() }
}

/// Ball `ball`'s Body colour.
fn set_colour(scene: &mut Scene, ball: ThingId, colour: Color) {
    let card = scene.card_mut(ball, SlotId::Look).unwrap();
    let body = card.layers.iter().find(|l| l.kind == "layer.body").unwrap().id;
    card.params.set(&layered_key(body, "colours"), Value::Gradient(Gradient::even(&[colour, colour])));
}

/// A second ball, a copy of the first, at `at` (drawn over it).
fn add_ball(scene: &mut Scene, at: [f32; 2], radius: f32) -> ThingId {
    let reg = reg();
    let first = thing_of(scene, "balls");
    let id = bpe_kit::add_thing(scene, &reg, "balls", ThingId::ROOT).unwrap();
    let look = scene.card(first, SlotId::Look).unwrap().clone();
    let params = scene.things[&first].params.clone();
    let t = scene.things.get_mut(&id).unwrap();
    t.params = params;
    t.params.set("radius", Value::Number(radius as f64));
    t.transform.position = glam::Vec2::from(at);
    t.slots.insert(SlotId::Look, look);
    id
}

/// The lit room (Flat or Stage, no floor): a ball of radius `radius` at `at` with its Body
/// alone, before a wall 240 px behind, one lamp up and to the left, 300 px in front, and the
/// room's shape out of the way.
fn room(stage: bool, at: [f32; 2], radius: f32) -> Scene {
    let reg = reg();
    let mut scene = scenes::lit_room(&reg);
    set_lighting(&mut scene, "depth", Value::Choice(if stage { "stage" } else { "flat" }.into()));
    set_lighting(&mut scene, "floor", Value::Choice("none".into()));
    set_lighting(&mut scene, "ambient", Value::Number(0.3));
    set_light(&mut scene, "position", Value::Vec2([-300.0, -400.0]));
    set_light(&mut scene, "height", Value::Number(300.0));
    set_light(&mut scene, "reach", Value::Number(3000.0));
    let ball = thing_of(&scene, "balls");
    let t = scene.things.get_mut(&ball).unwrap();
    t.params.set("radius", Value::Number(radius as f64));
    t.transform.position = glam::Vec2::from(at);
    let look = scene.card_mut(ball, SlotId::Look).unwrap();
    look.layers.retain(|l| l.kind == "layer.body");
    let shape = thing_of(&scene, "shape");
    scene.things.get_mut(&shape).unwrap().params.set("size", Value::Number(2000.0));
    scene
}

/// The spread of brightness along a row across a ball at `centre` of radius `r`.
fn spread_across(pixels: &[u8], centre: [f32; 2], r: f32) -> (u32, u32) {
    let b: Vec<u32> = (-8..=8).map(|k| brightness(at(pixels, centre[0] + k as f32 * 0.09 * r, centre[1]))).collect();
    (*b.iter().min().unwrap(), *b.iter().max().unwrap())
}

#[test]
fn a_glass_ball_shows_the_stripes_behind_it_bent() {
    let Some(mut gpu) = gpu() else { return };
    for stage in [false, true] {
        let mut scene = room(stage, [0.0, 0.0], 160.0);
        set_background(&mut scene, stripes());
        let ball = thing_of(&scene, "balls");
        set_colour(&mut scene, ball, Color::rgb(0xff, 0xff, 0xff));
        let name = if stage { "stripes-stage" } else { "stripes-flat" };
        let (lo, hi) = spread_across(&draw(&mut gpu, &scene, &format!("{name}-matte")), [0.0, 0.0], 160.0);
        set_surface(&mut scene, ball, SurfaceKind::Glass);
        let (glo, ghi) = spread_across(&draw(&mut gpu, &scene, name), [0.0, 0.0], 160.0);
        assert!(scene_frame_refracts(&scene), "{name}");
        // A white matte ball is light all across; through glass the stripes show, dark and light.
        assert!(lo > 200, "{name}: matte {lo}..{hi}");
        assert!(glo + 150 < ghi, "{name}: glass {glo}..{ghi}");
    }
}

fn scene_frame_refracts(scene: &Scene) -> bool {
    frame_of(scene, 0.0).light.unwrap().refracts
}

#[test]
fn a_red_ball_shows_through_a_clear_glass_ball() {
    let Some(mut gpu) = gpu() else { return };
    let mut scene = room(false, [60.0, 0.0], 70.0);
    let red = thing_of(&scene, "balls");
    set_colour(&mut scene, red, Color::rgb(0xff, 0x20, 0x20));
    let glass = add_ball(&mut scene, [0.0, 0.0], 160.0);
    set_colour(&mut scene, glass, Color::rgb(0xff, 0xff, 0xff));
    let opaque = at(&draw(&mut gpu, &scene, "red-behind-before"), 60.0, 0.0);
    set_surface(&mut scene, glass, SurfaceKind::Glass);
    let pixels = draw(&mut gpu, &scene, "red-behind");
    // Somewhere across the glass the red ball shows (bent, so not just where it is).
    let reddest = (-8..=8).map(|k| redness(at(&pixels, k as f32 * 18.0, 0.0))).max().unwrap();
    assert!(redness(opaque) < 20, "{opaque:?}");
    assert!(reddest > 60, "reddest {reddest}");
}

#[test]
fn a_chrome_ball_mirrors_the_red_ball_beside_it() {
    let Some(mut gpu) = gpu() else { return };
    for stage in [false, true] {
        let mut scene = room(stage, [-150.0, 0.0], 100.0);
        let chrome = thing_of(&scene, "balls");
        set_colour(&mut scene, chrome, Color::rgb(0xff, 0xff, 0xff));
        set_surface(&mut scene, chrome, SurfaceKind::Chrome);
        let red = add_ball(&mut scene, [150.0, 0.0], 100.0);
        set_colour(&mut scene, red, Color::rgb(0xff, 0x20, 0x20));
        set_surface(&mut scene, red, SurfaceKind::Matte);
        let name = if stage { "chrome-red-stage" } else { "chrome-red-flat" };
        let pixels = draw(&mut gpu, &scene, name);
        let facing = redness(at(&pixels, -150.0 + 80.0, 0.0));
        let away = redness(at(&pixels, -150.0 - 80.0, 0.0));
        assert!(facing > away + 30, "{name}: facing {facing} away {away}");
        // A sky gradient alone shows no red.
        set_look(&mut scene, chrome, "reflects", Value::Choice("a_sky_gradient".into()));
        let sky = redness(at(&draw(&mut gpu, &scene, &format!("{name}-sky")), -150.0 + 80.0, 0.0));
        assert!(facing > sky + 30, "{name}: facing {facing} sky {sky}");
    }
}

#[test]
fn a_mirror_circle_shows_the_ball_inside_it_outside_its_edge() {
    let Some(mut gpu) = gpu() else { return };
    let mut scene = room(false, [150.0, 0.0], 50.0);
    let ball = thing_of(&scene, "balls");
    set_colour(&mut scene, ball, Color::rgb(0xff, 0x20, 0x20));
    let shape = thing_of(&scene, "shape");
    scene.things.get_mut(&shape).unwrap().params.set("size", Value::Number(300.0));
    scene.things.get_mut(&shape).unwrap().params.set("thickness", Value::Number(4.0));
    let before = draw(&mut gpu, &scene, "mirror-before");
    set_look(&mut scene, shape, "mirror", Value::Choice("outside".into()));
    set_look(&mut scene, shape, "mirror_strength", Value::Number(1.0));
    let lit = frame_of(&scene, 0.0).light.unwrap();
    assert_eq!(lit.mirrors.len(), 1);
    let pixels = draw(&mut gpu, &scene, "mirror");
    // 170 px outside the edge: squeezed, it shows 108 px inside, on the ball.
    let outside = at(&pixels, 470.0, 0.0);
    assert!(redness(at(&before, 470.0, 0.0)) < 15, "before {:?}", at(&before, 470.0, 0.0));
    assert!(redness(outside) > 60, "outside {outside:?}");
    // Inside is untouched.
    let inside = (at(&pixels, 0.0, 100.0), at(&before, 0.0, 100.0));
    assert!(brightness(inside.0).abs_diff(brightness(inside.1)) <= 6, "{inside:?}");
}

#[test]
fn depth_of_field_blurs_the_wall_and_keeps_the_ball_sharp() {
    let Some(mut gpu) = gpu() else { return };
    let mut scene = room(true, [0.0, 0.0], 140.0);
    set_background(&mut scene, stripes());
    let ball = thing_of(&scene, "balls");
    set_colour(&mut scene, ball, Color::rgb(0xff, 0x20, 0x20));
    let row = |pixels: &[u8]| {
        let b: Vec<u32> = (-20..=20).map(|k| brightness(at(pixels, k as f32 * 24.0, -600.0))).collect();
        b.iter().max().unwrap() - b.iter().min().unwrap()
    };
    let sharp = draw(&mut gpu, &scene, "dof-off");
    set_lighting(&mut scene, "depth_of_field", Value::Bool(true));
    set_lighting(&mut scene, "focus_at", Value::Number(140.0));
    set_lighting(&mut scene, "dof_blur", Value::Number(0.2));
    let blurred = draw(&mut gpu, &scene, "dof-on");
    assert!(row(&blurred) * 10 < row(&sharp) * 7, "wall contrast {} -> {}", row(&sharp), row(&blurred));
    // The ball's front, at the focus, keeps its colour.
    let (a, b) = (at(&sharp, 0.0, 0.0), at(&blurred, 0.0, 0.0));
    assert!(brightness(a).abs_diff(brightness(b)) <= 12, "{a:?} vs {b:?}");
}

#[test]
fn velvet_is_brighter_at_its_rim_than_its_middle() {
    let Some(mut gpu) = gpu() else { return };
    let mut scene = room(true, [0.0, 0.0], 160.0);
    // Lit from straight ahead, so a matte ball is brightest in its middle.
    set_light(&mut scene, "position", Value::Vec2([0.0, 0.0]));
    set_light(&mut scene, "height", Value::Number(900.0));
    let ball = thing_of(&scene, "balls");
    let pixels = draw(&mut gpu, &scene, "velvet-before");
    let (m_mid, m_rim) = (brightness(at(&pixels, 0.0, 0.0)), brightness(at(&pixels, 0.93 * 160.0, 0.0)));
    set_surface(&mut scene, ball, SurfaceKind::Velvet);
    let pixels = draw(&mut gpu, &scene, "velvet");
    let (v_mid, v_rim) = (brightness(at(&pixels, 0.0, 0.0)), brightness(at(&pixels, 0.93 * 160.0, 0.0)));
    assert!(m_rim < m_mid, "matte rim {m_rim} middle {m_mid}");
    assert!(v_rim > v_mid + 20, "velvet rim {v_rim} middle {v_mid}");
}

#[test]
fn lava_glows_in_a_dark_room() {
    let Some(mut gpu) = gpu() else { return };
    let mut scene = room(true, [0.0, 0.0], 160.0);
    set_light(&mut scene, "strength", Value::Number(0.0));
    set_lighting(&mut scene, "ambient", Value::Number(0.0));
    let ball = thing_of(&scene, "balls");
    let dark = brightness(at(&draw(&mut gpu, &scene, "lava-before"), 0.0, 0.0));
    set_surface(&mut scene, ball, SurfaceKind::Lava);
    let lava = brightness(at(&draw(&mut gpu, &scene, "lava"), 0.0, 0.0));
    assert!(dark < 20, "dark {dark}");
    assert!(lava > dark + 60, "lava {lava} dark {dark}");
}

#[test]
fn pearl_and_flake_shine_their_own_ways() {
    let Some(mut gpu) = gpu() else { return };
    let mut scene = room(true, [0.0, 0.0], 200.0);
    let ball = thing_of(&scene, "balls");
    set_colour(&mut scene, ball, Color::rgb(0xc0, 0xc0, 0xc0));
    // How far the colour's channels are apart, and how often the light jumps, over the ball.
    let measure = |pixels: &[u8]| {
        let (mut tint, mut jumps) = (0i32, 0);
        for j in -6..=6 {
            let mut last = None;
            for i in -6..=6 {
                let (x, y) = (i as f32 * 25.0, j as f32 * 25.0);
                if x * x + y * y > 0.8 * 200.0 * 200.0 {
                    continue;
                }
                let p = at(pixels, x, y);
                let (lo, hi) = (p[0].min(p[1]).min(p[2]) as i32, p[0].max(p[1]).max(p[2]) as i32);
                tint = tint.max(hi - lo);
                let b = brightness(p) as i32;
                if let Some(l) = last
                    && (b - l as i32).abs() > 24
                {
                    jumps += 1;
                }
                last = Some(b as u32);
            }
        }
        (tint, jumps)
    };
    set_surface(&mut scene, ball, SurfaceKind::Plastic);
    let plastic = measure(&draw(&mut gpu, &scene, "plastic"));
    set_surface(&mut scene, ball, SurfaceKind::Pearl);
    let pearl = measure(&draw(&mut gpu, &scene, "pearl"));
    set_surface(&mut scene, ball, SurfaceKind::Flake);
    let flake = measure(&draw(&mut gpu, &scene, "flake"));
    assert!(pearl.0 > plastic.0 + 20, "pearl {pearl:?} plastic {plastic:?}");
    assert!(flake.1 > plastic.1 + 6, "flake {flake:?} plastic {plastic:?}");
}

#[test]
fn brushed_metal_stretches_its_highlight_across_its_lines() {
    let Some(mut gpu) = gpu() else { return };
    let mut scene = room(true, [0.0, 0.0], 200.0);
    let ball = thing_of(&scene, "balls");
    set_colour(&mut scene, ball, Color::rgb(0xc0, 0xc0, 0xc0));
    let bright_count = |pixels: &[u8], vertical: bool| {
        let best = (-20..=20)
            .flat_map(|j| (-20..=20).map(move |i| (i, j)))
            .map(|(i, j)| brightness(at(pixels, i as f32 * 9.0, j as f32 * 9.0)))
            .max()
            .unwrap();
        let (mut longest, mut cx, mut cy) = (0, 0.0, 0.0);
        for j in -20..=20 {
            for i in -20..=20 {
                if brightness(at(pixels, i as f32 * 9.0, j as f32 * 9.0)) == best {
                    (cx, cy) = (i as f32 * 9.0, j as f32 * 9.0);
                }
            }
        }
        for k in -20..=20 {
            let (x, y) = if vertical { (cx, k as f32 * 9.0) } else { (k as f32 * 9.0, cy) };
            if brightness(at(pixels, x, y)) as f32 > best as f32 * 0.8 {
                longest += 1;
            }
        }
        longest
    };
    set_surface(&mut scene, ball, SurfaceKind::Brushed);
    set_look(&mut scene, ball, "brush_lines", Value::Choice("straight".into()));
    set_look(&mut scene, ball, "brush_angle", Value::Number(0.0));
    let pixels = draw(&mut gpu, &scene, "brushed");
    // Lines along x: the highlight runs across them (up and down), as on real brushed metal.
    let along = bright_count(&pixels, false);
    let across = bright_count(&pixels, true);
    assert!(across > along + 3, "along {along} across {across}");
}

#[test]
fn in_stage_a_ball_stops_the_light_of_a_glowing_ball_behind_it() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let mut scene = scenes::glow_room(&reg);
    set_lighting(&mut scene, "depth", Value::Choice("stage".into()));
    set_lighting(&mut scene, "floor", Value::Choice("none".into()));
    let glow = thing_of(&scene, "balls");
    let at_glow = scene.things[&glow].transform.position;
    // A plain ball 160 px right of the glowing one.
    let blocker = add_ball(&mut scene, [at_glow.x + 160.0, at_glow.y], 50.0);
    scene.card_mut(blocker, SlotId::Look).unwrap().layers.retain(|l| l.kind == "layer.body");
    let pixels = draw(&mut gpu, &scene, "stage-glow-blocked");
    let behind = at(&pixels, at_glow.x + 300.0, at_glow.y);
    let beside = at(&pixels, at_glow.x, at_glow.y + 300.0);
    assert!(brightness(behind) + 15 < brightness(beside), "behind {behind:?} beside {beside:?}");
}

#[test]
fn a_surface_sub_layer_gives_its_layer_its_own_surface() {
    let reg = reg();
    let mut scene = room(true, [0.0, 0.0], 160.0);
    let ball = thing_of(&scene, "balls");
    let pattern = scene.add_layer(&reg, ball, SlotId::Look, "layer.pattern").unwrap();
    let sub = scene.add_sub_layer(&reg, ball, SlotId::Look, pattern, "sub.surface").unwrap();
    let card = scene.card_mut(ball, SlotId::Look).unwrap();
    for (k, v) in preset_values(SurfaceKind::Chrome) {
        card.params.set(&layered_key(sub, k), v);
    }
    card.params.set(&layered_key(sub, "surface"), Value::Choice("chrome".into()));
    let lit = frame_of(&scene, 0.0).light.unwrap();
    // The ball's own (Matte) and its Pattern's (Chrome) after the shape's.
    let chrome = lit.surfaces.iter().filter(|s| s.a[1] >= 0.99).count();
    assert_eq!(chrome, 1, "{:?}", lit.surfaces);
}

/// Writes the surfaces demo scene to `BPE_SURFACES_DEMO` (for the review video), when set.
#[test]
fn write_the_surfaces_demo_scene() {
    let Some(path) = std::env::var_os("BPE_SURFACES_DEMO") else { return };
    std::fs::write(path, bpe_core::save::save(&scenes::surfaces_demo(&reg()))).unwrap();
}

#[test]
fn the_surfaces_demo_draws() {
    let Some(mut gpu) = gpu() else { return };
    let scene = scenes::surfaces_demo(&reg());
    let lit = frame_of(&scene, 1.0).light.unwrap();
    assert_eq!(lit.spheres.len(), 9);
    assert_eq!(lit.mirrors.len(), 1);
    assert!(lit.refracts);
    assert!(lit.settings.dof[0] > 0.5);
    let pixels = draw(&mut gpu, &scene, "surfaces-demo");
    assert!(pixels.chunks(4).any(|p| brightness([p[0], p[1], p[2], p[3]]) > 200));
}

#[test]
fn a_surface_sub_layer_shows_on_a_layer_that_blocks_no_light() {
    let Some(mut gpu) = gpu() else { return };
    let reg = reg();
    let mut scene = room(true, [0.0, 0.0], 180.0);
    let ball = thing_of(&scene, "balls");
    let pattern = scene.add_layer(&reg, ball, SlotId::Look, "layer.pattern").unwrap();
    let before = draw(&mut gpu, &scene, "sub-surface-before");
    let sub = scene.add_sub_layer(&reg, ball, SlotId::Look, pattern, "sub.surface").unwrap();
    let card = scene.card_mut(ball, SlotId::Look).unwrap();
    for (k, v) in preset_values(SurfaceKind::Chrome) {
        card.params.set(&layered_key(sub, k), v);
    }
    let after = draw(&mut gpu, &scene, "sub-surface");
    // Somewhere on the ball (where the pattern is) the chrome shows.
    let changed = (-8..=8)
        .flat_map(|j| (-8..=8).map(move |i| (i as f32 * 18.0, j as f32 * 18.0)))
        .filter(|(x, y)| x * x + y * y < 160.0 * 160.0)
        .filter(|(x, y)| brightness(at(&before, *x, *y)).abs_diff(brightness(at(&after, *x, *y))) > 30)
        .count();
    assert!(changed >= 5, "{changed} points changed");
}

#[test]
fn a_laser_bends_through_a_glass_ball() {
    let mut scene = room(false, [0.0, 0.0], 160.0);
    set_light(&mut scene, "kind", Value::Choice("laser".into()));
    set_light(&mut scene, "position", Value::Vec2([-450.0, 60.0]));
    set_light(&mut scene, "direction", Value::Number(0.0));
    // Going in and coming out each count as a bounce.
    set_light(&mut scene, "bounces", Value::Int(2));
    set_light(&mut scene, "at_balls", Value::Choice("pass_through".into()));
    set_light(&mut scene, "at_shapes", Value::Choice("stop".into()));
    // One colour: Glass's Rainbow split doesn't fan it (glass and light part 2).
    set_light(&mut scene, "colours", Value::Int(1));
    let end_y = |scene: &Scene| {
        let lit = frame_of(scene, 0.0).light.unwrap();
        let l = lit.lights.iter().find(|l| l.p0[3] == 5.0).expect("a laser");
        let last = lit.segments[(l.p4[2] + l.p4[3] - 1.0) as usize];
        (last[3], l.p4[3])
    };
    let (straight, pieces) = end_y(&scene);
    assert!((straight - 60.0).abs() < 1.0, "{straight}");
    assert_eq!(pieces, 1.0);
    let ball = thing_of(&scene, "balls");
    set_surface(&mut scene, ball, SurfaceKind::Glass);
    let (bent, pieces) = end_y(&scene);
    assert!(pieces >= 3.0, "in, through and out: {pieces}");
    assert!((bent - 60.0).abs() > 10.0, "bent to {bent}");
}

#[test]
fn a_laser_through_rainbow_glass_splits_into_colours() {
    let mut scene = room(false, [0.0, 0.0], 160.0);
    set_light(&mut scene, "kind", Value::Choice("laser".into()));
    set_light(&mut scene, "position", Value::Vec2([-450.0, 60.0]));
    set_light(&mut scene, "direction", Value::Number(0.0));
    set_light(&mut scene, "bounces", Value::Int(4));
    set_light(&mut scene, "at_shapes", Value::Choice("stop".into()));
    let ball = thing_of(&scene, "balls");
    set_surface(&mut scene, ball, SurfaceKind::Prism);
    let lit = frame_of(&scene, 0.0).light.unwrap();
    let lasers: Vec<_> = lit.lights.iter().filter(|l| l.p0[3] == 5.0).collect();
    assert_eq!(lasers.len(), 13, "the white beam to the glass, then 12 colours");
    let white = lasers[0];
    for c in 0..3 {
        let sum: f32 = lasers[1..].iter().map(|l| l.p1[c]).sum();
        assert!((sum - white.p1[c]).abs() < 1e-4 * white.p1[c].abs().max(1.0), "channel {c}: {sum} vs {}", white.p1[c]);
    }
    let split_at = lit.segments[(white.p4[2] + white.p4[3] - 1.0) as usize];
    for l in &lasers[1..] {
        let first = lit.segments[l.p4[2] as usize];
        assert_eq!([first[0], first[1]], [split_at[2], split_at[3]], "each colour starts where the white ends");
    }
    let ends: Vec<f32> = lasers[1..].iter().map(|l| lit.segments[(l.p4[2] + l.p4[3] - 1.0) as usize][3]).collect();
    assert!(ends.first() != ends.last(), "red and violet end apart: {ends:?}");
    // Colours 1: one laser, as before.
    set_light(&mut scene, "colours", Value::Int(1));
    let lit = frame_of(&scene, 0.0).light.unwrap();
    assert_eq!(lit.lights.iter().filter(|l| l.p0[3] == 5.0).count(), 1);
    assert!(lit.ribbons.is_empty());
}

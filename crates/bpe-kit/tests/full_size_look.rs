//! The full-size look check: test scenes and every ready-made look rendered at the real 1080 x 1920, to be opened
//! and looked at before a pull request that changes what is drawn. The picture tests draw at 270 x 480 and check
//! numbers, so jagged edges, light leaks, banding and seams only show here.
//!
//! Ignored, so `cargo test` only compiles it. Run it with
//! `cargo test --release -p bpe-kit --test full_size_look -- --ignored --nocapture`.
//!
//! For each picture it writes `<name>.png` (full size) and `<name>-q1.png` .. `-q4.png`, the four quarters at 1:1
//! (top left, top right, bottom left, bottom right), so edges can be seen at their real size; each group also gets a
//! `sheet-<group>.png` at quarter size with `sheet-<group>.txt` listing its pictures in order (rows of 6).
//!
//! Environment:
//! - `LOOK_OUT`: the folder (default `target/full-size-look` in the workspace).
//! - `LOOK_ONLY`: a comma list; only pictures whose name contains one of them.
//! - `LOOK_QUALITIES`: the lighting qualities for the lighting scenes (default `good,ultra`; also `draft`, `high`).
//! - `LOOK_SHOW=light`: lit scenes show their light only.
//! - `LOOK_SET`: `key=value;key=value` written onto every Lighting card (a number, or a choice).

use bpe_core::{Registry, Scene, Settings, SlotId, ThingId, Value};
use bpe_gfx::{Frame, LightShow, Renderer, build_frame, test_device};
use bpe_kit::things::balls::BallsSettings;
use bpe_kit::things::light::{LightSettings, LightType};
use bpe_kit::things::shape::ShapeSettings;
use bpe_kit::{add_thing, kit, new_scene, scenes};
use bpe_runtime::Playback;
use glam::Vec2;
use std::path::PathBuf;

const W: u32 = 1080;
const H: u32 = 1920;
/// Thumbnails in a contact sheet are a quarter of the picture each way.
const THUMB: u32 = 4;
const SHEET_COLUMNS: u32 = 6;

/// A picture to make: its name, the scene and the moments drawn (more than one gets `-f00`, `-f01`, … names).
struct Shot {
    name: String,
    scene: Scene,
    times: Vec<f64>,
}

fn shot(name: impl Into<String>, scene: Scene, times: Vec<f64>) -> Shot {
    Shot { name: name.into(), scene, times }
}

fn out_dir() -> PathBuf {
    let dir = std::env::var_os("LOOK_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/full-size-look"));
    std::fs::create_dir_all(&dir).expect("the output folder");
    dir
}

fn save(path: &std::path::Path, w: u32, h: u32, pixels: &[u8]) {
    let file = std::fs::File::create(path).expect("a picture file");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(pixels).unwrap();
}

/// The part of a `w`-wide RGBA picture from (`x`, `y`), `cw` x `ch`.
fn crop(pixels: &[u8], w: u32, x: u32, y: u32, cw: u32, ch: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((cw * ch * 4) as usize);
    for row in y..y + ch {
        let start = ((row * w + x) * 4) as usize;
        out.extend_from_slice(&pixels[start..start + (cw * 4) as usize]);
    }
    out
}

/// The picture shrunk by `k` each way, each pixel the mean of its k x k block.
fn shrink(pixels: &[u8], w: u32, h: u32, k: u32) -> Vec<u8> {
    let (sw, sh) = (w / k, h / k);
    let mut out = vec![0u8; (sw * sh * 4) as usize];
    for y in 0..sh {
        for x in 0..sw {
            let mut sum = [0u32; 4];
            for dy in 0..k {
                for dx in 0..k {
                    let i = (((y * k + dy) * w + x * k + dx) * 4) as usize;
                    for (c, s) in sum.iter_mut().enumerate() {
                        *s += pixels[i + c] as u32;
                    }
                }
            }
            let o = ((y * sw + x) * 4) as usize;
            for c in 0..4 {
                out[o + c] = (sum[c] / (k * k)) as u8;
            }
        }
    }
    out
}

/// Saves the full picture and its four 1:1 quarters; gives back the thumbnail for the sheet.
fn save_all(dir: &std::path::Path, name: &str, pixels: &[u8]) -> Vec<u8> {
    save(&dir.join(format!("{name}.png")), W, H, pixels);
    let (hw, hh) = (W / 2, H / 2);
    for (q, (x, y)) in [(0, 0), (hw, 0), (0, hh), (hw, hh)].into_iter().enumerate() {
        save(&dir.join(format!("{name}-q{}.png", q + 1)), hw, hh, &crop(pixels, W, x, y, hw, hh));
    }
    shrink(pixels, W, H, THUMB)
}

/// One sheet of thumbnails, rows of [`SHEET_COLUMNS`], with a text file naming them in order.
fn save_sheet(dir: &std::path::Path, group: &str, thumbs: &[(String, Vec<u8>)]) {
    if thumbs.is_empty() {
        return;
    }
    let (tw, th) = (W / THUMB, H / THUMB);
    let columns = SHEET_COLUMNS.min(thumbs.len() as u32);
    let rows = (thumbs.len() as u32).div_ceil(columns);
    let (sw, sh) = (columns * tw, rows * th);
    let mut sheet = vec![0u8; (sw * sh * 4) as usize];
    for (i, (_, thumb)) in thumbs.iter().enumerate() {
        let (cx, cy) = ((i as u32 % columns) * tw, (i as u32 / columns) * th);
        for row in 0..th {
            let from = (row * tw * 4) as usize;
            let to = (((cy + row) * sw + cx) * 4) as usize;
            sheet[to..to + (tw * 4) as usize].copy_from_slice(&thumb[from..from + (tw * 4) as usize]);
        }
    }
    save(&dir.join(format!("sheet-{group}.png")), sw, sh, &sheet);
    let names: Vec<&str> = thumbs.iter().map(|(n, _)| n.as_str()).collect();
    std::fs::write(dir.join(format!("sheet-{group}.txt")), names.join("\n") + "\n").expect("the sheet's list");
}

struct Gpu {
    device: bpe_gfx::wgpu::Device,
    queue: bpe_gfx::wgpu::Queue,
    renderer: Renderer,
}

/// Draws `shot` at each of its moments; gives back (name, pixels) a moment.
fn render(gpu: &mut Gpu, shot: &Shot, show: LightShow) -> Vec<(String, Vec<u8>)> {
    let kit = bpe_kit::kit();
    let resolved = bpe_core::resolve(&shot.scene, &kit.registry);
    let mut playback = Playback::new(&shot.scene, &resolved, kit.sims.clone());
    let mut out = Vec::new();
    for (i, &t) in shot.times.iter().enumerate() {
        playback.seek(t);
        let mut frame = Frame::default();
        build_frame(&shot.scene, &resolved, playback.world(), playback.log(), t, &kit.draws, &mut frame);
        if let Some(light) = frame.light.as_mut() {
            light.show = show;
        }
        let start = std::time::Instant::now();
        gpu.renderer.render(&gpu.device, &gpu.queue, &frame);
        // In-ball programs are built on first use; draw again once they are ready.
        while gpu.renderer.programs_waiting() {
            gpu.renderer.render(&gpu.device, &gpu.queue, &frame);
        }
        let pixels = gpu.renderer.read_output(&gpu.device, &gpu.queue).expect("the picture");
        let name = if shot.times.len() > 1 { format!("{}-f{i:02}", shot.name) } else { shot.name.clone() };
        eprintln!("{name}: {:.2} s", start.elapsed().as_secs_f32());
        out.push((name, pixels));
    }
    out
}

fn lighting(scene: &mut Scene, key: &str, v: Value) {
    if let Some(card) = scene.card_mut(ThingId::ROOT, SlotId::Lighting) {
        card.params.set(key, v);
    }
}

fn first_of(scene: &Scene, kind: &str) -> ThingId {
    scene.things.values().find(|t| t.kind == kind).map(|t| t.id).expect("a thing of that kind")
}

fn set_light(scene: &mut Scene, s: &LightSettings) {
    let id = first_of(scene, "light");
    scene.things.get_mut(&id).unwrap().params = s.to_params();
}

fn light(kind: LightType, at: Vec2, size: f32, strength: f32) -> LightSettings {
    let mut p = LightSettings::of_kind(kind);
    p.position = at;
    p.size = size;
    p.reach = 3000.0;
    p.strength = strength;
    p
}

/// A still copy of the first ball at `at` with `radius`.
fn add_ball(scene: &mut Scene, reg: &Registry, at: Vec2, radius: f32) {
    let template = scene.things[&first_of(scene, "balls")].clone();
    let id = add_thing(scene, reg, "balls", ThingId::ROOT).unwrap();
    let t = scene.things.get_mut(&id).unwrap();
    let mut s = BallsSettings::from_params(&template.params);
    s.radius = radius;
    s.velocity = Vec2::ZERO;
    t.params = s.to_params();
    t.slots.insert(SlotId::Look, template.slots[&SlotId::Look].clone());
    t.transform.position = at;
}

fn add_shape(scene: &mut Scene, reg: &Registry, at: Vec2, s: ShapeSettings) {
    let id = add_thing(scene, reg, "shape", ThingId::ROOT).unwrap();
    let t = scene.things.get_mut(&id).unwrap();
    t.params = s.to_params();
    t.transform.position = at;
}

/// The first ball of `balls` set as `edit` says (its settings) and placed at `at`.
fn first_ball(scene: &mut Scene, at: Vec2, edit: impl FnOnce(&mut BallsSettings)) {
    let b = first_of(scene, "balls");
    let t = scene.things.get_mut(&b).unwrap();
    let mut s = BallsSettings::from_params(&t.params);
    s.velocity = Vec2::ZERO;
    edit(&mut s);
    t.params = s.to_params();
    t.transform.position = at;
}

fn no_gravity(scene: &mut Scene) {
    scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, 0.0]));
}

/// The lit room with a still ball of `radius` at `at` and no gravity.
fn room(reg: &Registry, at: Vec2, radius: f32) -> Scene {
    let mut s = scenes::lit_room(reg);
    first_ball(&mut s, at, |b| b.radius = radius);
    no_gravity(&mut s);
    s
}

/// Lighting stress scenes (from the 2026-11-08 visual review): small and large lamps, long shadows, thin walls,
/// many balls, contact, a moving ball and a Stage. Each is drawn at every quality asked for.
fn lighting_shots(reg: &Registry) -> Vec<Shot> {
    let mut out = Vec::new();
    out.push(shot("room", room(reg, Vec2::ZERO, 60.0), vec![0.0]));
    {
        let mut s = room(reg, Vec2::ZERO, 80.0);
        let mut p = light(LightType::Point, Vec2::new(-60.0, -200.0), 4.0, 2.0);
        p.height = 60.0;
        p.reach = 2400.0;
        set_light(&mut s, &p);
        add_ball(&mut s, reg, Vec2::new(220.0, 300.0), 30.0);
        add_ball(&mut s, reg, Vec2::new(-260.0, 200.0), 14.0);
        out.push(shot("near-point", s, vec![0.0]));
    }
    {
        let mut s = room(reg, Vec2::new(-200.0, 100.0), 50.0);
        let mut p = light(LightType::Spot, Vec2::new(-480.0, -500.0), 10.0, 2.5);
        p.direction = 55.0;
        p.cone = 80.0;
        p.height = 120.0;
        set_light(&mut s, &p);
        add_ball(&mut s, reg, Vec2::new(60.0, 400.0), 40.0);
        add_ball(&mut s, reg, Vec2::new(-80.0, -150.0), 20.0);
        lighting(&mut s, "shadow_length", Value::Number(4000.0));
        out.push(shot("spot-long", s, vec![0.0]));
    }
    {
        let mut s = room(reg, Vec2::new(0.0, -300.0), 60.0);
        let mut p = LightSettings::of_kind(LightType::Sun);
        p.direction = 60.0;
        p.elevation = 20.0;
        p.sun_size = 1.0;
        p.strength = 1.5;
        set_light(&mut s, &p);
        add_ball(&mut s, reg, Vec2::new(-250.0, 200.0), 30.0);
        add_shape(&mut s, reg, Vec2::new(200.0, 300.0), ShapeSettings::polygon(5, 90.0, 4.0));
        lighting(&mut s, "shadow_length", Value::Number(4000.0));
        out.push(shot("sun-low", s, vec![0.0]));
    }
    {
        let mut s = room(reg, Vec2::new(0.0, -150.0), 50.0);
        let mut p = light(LightType::Area, Vec2::new(0.0, -450.0), 10.0, 2.0);
        p.width = 200.0;
        p.length = 40.0;
        // Across the picture (an area light's Direction is where its width runs; 90, down, would poke it out of
        // the ring).
        p.direction = 0.0;
        p.height = 120.0;
        set_light(&mut s, &p);
        add_ball(&mut s, reg, Vec2::new(-260.0, -250.0), 25.0);
        lighting(&mut s, "shadow_length", Value::Number(4000.0));
        out.push(shot("area", s, vec![0.0]));
    }
    {
        let mut s = scenes::lit_room(reg);
        first_ball(&mut s, Vec2::new(0.0, 150.0), |b| {
            b.block(6, 6, 90.0);
            b.radius = 22.0;
        });
        no_gravity(&mut s);
        set_light(&mut s, &light(LightType::Point, Vec2::new(-100.0, -450.0), 12.0, 2.0));
        out.push(shot("many", s, vec![0.0]));
    }
    {
        let mut s = room(reg, Vec2::new(250.0, 500.0), 30.0);
        set_light(&mut s, &light(LightType::Point, Vec2::new(0.0, -500.0), 6.0, 2.0));
        add_shape(&mut s, reg, Vec2::new(-150.0, -250.0), ShapeSettings::polygon(6, 110.0, 2.0));
        add_shape(&mut s, reg, Vec2::new(170.0, -200.0), ShapeSettings::rectangle(160.0, 90.0, 3.0));
        let mut e = ShapeSettings::ellipse(260.0, 60.0, 4.0);
        e.turn = 20.0;
        add_shape(&mut s, reg, Vec2::new(0.0, 50.0), e);
        lighting(&mut s, "shadow_length", Value::Number(4000.0));
        out.push(shot("thin-walls", s, vec![0.0]));
    }
    {
        let mut s = scenes::lit_room(reg);
        first_ball(&mut s, Vec2::new(0.0, 300.0), |b| b.radius = 70.0);
        set_light(&mut s, &light(LightType::Point, Vec2::new(-150.0, 0.0), 20.0, 2.0));
        lighting(&mut s, "contact", Value::Number(0.5));
        out.push(shot("contact", s, vec![6.0]));
    }
    {
        let mut s = scenes::lit_room(reg);
        first_ball(&mut s, Vec2::new(-60.0, -100.0), |b| {
            b.radius = 50.0;
            b.velocity = Vec2::new(90.0, 40.0);
        });
        no_gravity(&mut s);
        set_light(&mut s, &light(LightType::Point, Vec2::new(-30.0, -330.0), 6.0, 2.0));
        // Four frames in a row at 60 a second, for shimmer and crawling edges.
        out.push(shot("moving", s, (0..4).map(|i| 1.0 + i as f64 / 60.0).collect()));
    }
    {
        let mut s = room(reg, Vec2::new(60.0, 200.0), 70.0);
        add_ball(&mut s, reg, Vec2::new(-250.0, 380.0), 40.0);
        let mut p = light(LightType::Point, Vec2::new(-350.0, -500.0), 30.0, 2.5);
        p.height = 400.0;
        p.reach = 3500.0;
        set_light(&mut s, &p);
        lighting(&mut s, "depth", Value::Choice("stage".into()));
        lighting(&mut s, "contact", Value::Number(0.5));
        lighting(&mut s, "looking_down", Value::Number(16.0));
        out.push(shot("stage-one", s, vec![0.0]));
    }
    {
        // A wide area panel right by the ring's top: nothing may leak out past the ring, and the balls' shadows
        // narrow to a point.
        let mut s = room(reg, Vec2::new(0.0, -100.0), 60.0);
        let mut p = light(LightType::Area, Vec2::new(0.0, -420.0), 10.0, 2.0);
        p.width = 360.0;
        p.length = 30.0;
        p.direction = 0.0;
        p.height = 80.0;
        set_light(&mut s, &p);
        add_ball(&mut s, reg, Vec2::new(-220.0, -50.0), 25.0);
        let mut bar = ShapeSettings::rectangle(160.0, 20.0, 4.0);
        bar.fill = bpe_kit::things::shape::ShapeFill::Filled;
        add_shape(&mut s, reg, Vec2::new(150.0, 150.0), bar);
        lighting(&mut s, "shadow_length", Value::Number(4000.0));
        out.push(shot("area-wide", s, vec![0.0]));
    }
    {
        // A strip light along a line across the top: shadows from the whole strip, not from its nearest point.
        let mut s = room(reg, Vec2::new(-60.0, 0.0), 50.0);
        let mut p = light(LightType::Strip, Vec2::new(0.0, -380.0), 6.0, 2.0);
        p.length = 500.0;
        p.width = 8.0;
        p.direction = 0.0;
        set_light(&mut s, &p);
        add_ball(&mut s, reg, Vec2::new(200.0, -150.0), 20.0);
        add_shape(&mut s, reg, Vec2::new(-150.0, 300.0), ShapeSettings::polygon(6, 70.0, 3.0));
        lighting(&mut s, "shadow_length", Value::Number(4000.0));
        out.push(shot("strip", s, vec![0.0]));
    }
    {
        // Thin hollow shapes on a Stage: their shadows on the back wall must be even inside, not striped.
        let mut s = room(reg, Vec2::new(250.0, 450.0), 40.0);
        let mut p = light(LightType::Point, Vec2::new(-300.0, -550.0), 20.0, 2.5);
        p.height = 400.0;
        p.reach = 3500.0;
        set_light(&mut s, &p);
        add_shape(&mut s, reg, Vec2::new(-120.0, -150.0), ShapeSettings::polygon(6, 120.0, 3.0));
        let mut e = ShapeSettings::ellipse(240.0, 70.0, 4.0);
        e.turn = 20.0;
        add_shape(&mut s, reg, Vec2::new(60.0, 150.0), e);
        lighting(&mut s, "depth", Value::Choice("stage".into()));
        lighting(&mut s, "looking_down", Value::Number(16.0));
        out.push(shot("stage-thin", s, vec![0.0]));
    }
    out
}

/// The test scenes' demos: lit, stage, surfaces, light shows, glow, colour.
fn demo_shots(reg: &Registry) -> Vec<Shot> {
    let at = |name: &str, scene: Scene| shot(name, scene, vec![1.0]);
    vec![
        at("lit-demo", scenes::lit_demo(reg)),
        at("stage-demo", scenes::stage_demo(reg)),
        at("surfaces-demo", scenes::surfaces_demo(reg)),
        at("bake-demo-live", scenes::bake_demo(reg)),
        at("light-show", scenes::light_show(reg)),
        at("glow-demo", scenes::glow_demo(reg)),
        at("club-demo", scenes::club_demo(reg)),
        shot("club-haze-with-the-air", haze_with_the_air(reg), vec![1.0, 1.05, 3.0]),
        at("water-demo", scenes::water_demo(reg)),
        at("colour-demo", scenes::colour_demo(reg)),
        at("blend-demo", scenes::blend_demo(reg)),
        at("graded-sunset", scenes::graded_sunset(reg)),
        at("stage-camera-orbit", stage_camera(reg, 25.0)),
        at("stage-camera-orbit-far", stage_camera(reg, -40.0)),
        at("stage-depth-of-field", stage_depth_of_field(reg)),
        at("gradient-seams", gradient_seams(reg)),
        at("lit-liquid-smoke", lit_liquid_smoke(reg, true)),
        at("lit-liquid-smoke-own-light", lit_liquid_smoke(reg, false)),
    ]
}

/// A Particles thing at `at` whose only Look layer is `kind` with `params`: a still crowd of `w` x `h`, one
/// particle of `size` every `room` square pixels.
struct Crowd {
    at: Vec2,
    w: f32,
    h: f32,
    size: f32,
    room: f32,
}

fn add_crowd(scene: &mut Scene, reg: &Registry, c: Crowd, kind: &str, params: bpe_core::Params) {
    let Crowd { at, w, h, size, room } = c;
    use bpe_kit::things::particles::looks::{Look, add_particles};
    use bpe_kit::things::particles::{BornFrom, Emit, ParticlesSettings};
    let id = add_particles(scene, reg, Look::Empty, ThingId::ROOT).unwrap();
    let source = ParticlesSettings {
        from: BornFrom::AnArea,
        emit: Emit::OnceAtTheStart,
        count: (w * h / room) as u32,
        speed: 0.0,
        life: 100.0,
        size,
        size_spread: 0.0,
        width: w,
        height: h,
        ..ParticlesSettings::default()
    };
    let t = scene.things.get_mut(&id).unwrap();
    t.params = source.to_params();
    t.transform.position = at;
    let old: Vec<bpe_core::LayerId> = scene.things[&id].slots[&SlotId::Look].layers.iter().map(|l| l.id).collect();
    let layer = scene.add_layer(reg, id, SlotId::Look, kind).unwrap();
    for l in old {
        scene.remove_layer(reg, id, SlotId::Look, l).unwrap();
    }
    let card = scene.card_mut(id, SlotId::Look).unwrap();
    for (k, v) in params.0 {
        card.params.0.insert(bpe_core::layered_key(layer, &k), v);
    }
}

/// W15: liquid and soft smoke lit by the real lights. A Plastic and a Glass puddle (over stripes, so
/// the glass shows what is behind it, and blocking light, so it bends the lamp's light), Soft smoke
/// above, a warm lamp low on the left and a ball casting its shadow over the Plastic puddle. `real` false sets
/// Lit 0 on all three: their own light, to compare.
fn lit_liquid_smoke(reg: &Registry, real: bool) -> Scene {
    use bpe_kit::modules::look::surface::SurfaceKind;
    use bpe_kit::paint::Paint;
    use bpe_kit::things::particles::liquid::{LIQUID, LiquidLayer, params_of};
    use bpe_kit::things::particles::look::{SMOKE, SmokeLayer};
    let mut s = room(reg, Vec2::new(-120.0, 120.0), 50.0);
    let root = s.things.get_mut(&ThingId::ROOT).unwrap();
    let mut settings = bpe_kit::SceneSettings::from_params(&root.params);
    let (k, w) = (bpe_core::Color::rgb(0x1c, 0x1c, 0x24), bpe_core::Color::rgb(0xb0, 0xb0, 0xb8));
    let colours: Vec<bpe_core::Color> = (0..19).map(|i| if i % 2 == 0 { k } else { w }).collect();
    settings.background = Paint { colours: bpe_core::Gradient::even(&colours), span: 1080.0, ..Paint::default() };
    root.params = settings.to_params();
    let mut lamp = light(LightType::Point, Vec2::new(-420.0, -60.0), 8.0, 1.6);
    lamp.height = 60.0;
    lamp.colour = bpe_core::Color::rgb(0xff, 0xb0, 0x60);
    set_light(&mut s, &lamp);
    let puddle = |surface, colour| {
        let mut p = params_of(&LiquidLayer {
            surface,
            roundness: 40.0,
            colour: Paint::solid(colour),
            ..LiquidLayer::default()
        });
        p.set("blocks_light", Value::Bool(true));
        p.set("lit", Value::Number(if real { 1.0 } else { 0.0 }));
        p
    };
    let plastic = puddle(SurfaceKind::Plastic, bpe_core::Color::rgb(0x30, 0x70, 0xd0));
    let puddle_at = |at, w, h| Crowd { at, w, h, size: 6.0, room: 40.0 };
    add_crowd(&mut s, reg, puddle_at(Vec2::new(-60.0, 300.0), 260.0, 160.0), LIQUID, plastic);
    let glass = puddle(SurfaceKind::Glass, bpe_core::Color::rgb(0xe0, 0xf0, 0xff));
    add_crowd(&mut s, reg, puddle_at(Vec2::new(220.0, 60.0), 200.0, 200.0), LIQUID, glass);
    let smoke = SmokeLayer {
        shading: 0.8,
        opacity: 0.9,
        puff: 4.0,
        colour: Paint::solid(bpe_core::Color::rgb(0xe0, 0xe0, 0xe0)),
        ..SmokeLayer::default()
    };
    let mut smoke = smoke.to_params();
    smoke.set("lit", Value::Number(if real { 1.0 } else { 0.0 }));
    let cloud = Crowd { at: Vec2::new(0.0, -360.0), w: 320.0, h: 180.0, size: 16.0, room: 200.0 };
    add_crowd(&mut s, reg, cloud, SMOKE, smoke);
    s
}

/// The stage demo seen through its Stage camera moved `orbit` degrees round.
fn stage_camera(reg: &Registry, orbit: f64) -> Scene {
    let mut s = scenes::stage_demo(reg);
    let camera = bpe_kit::ensure_camera(&mut s, reg);
    let params = &mut s.things.get_mut(&camera).unwrap().params;
    params.set("stage_camera", Value::Bool(true));
    params.set("orbit", Value::Number(orbit));
    s
}

/// The stage demo with depth of field, focused on the balls: the wall and floor well blurred.
fn stage_depth_of_field(reg: &Registry) -> Scene {
    let mut s = scenes::stage_demo(reg);
    lighting(&mut s, "depth_of_field", Value::Bool(true));
    lighting(&mut s, "focus_at", Value::Number(40.0));
    lighting(&mut s, "dof_blur", Value::Number(0.3));
    s
}

/// Two big still balls painted inside by a Sweep and a Spiral gradient that doesn't loop, so their last colour meets
/// their first in a seam (which should be as smooth as any edge).
fn gradient_seams(reg: &Registry) -> Scene {
    use bpe_core::{Color, Gradient, Mix, layered_key};
    let mut s = new_scene(reg);
    no_gravity(&mut s);
    first_ball(&mut s, Vec2::new(0.0, -420.0), |b| b.radius = 330.0);
    let first = first_of(&s, "balls");
    add_ball(&mut s, reg, Vec2::new(0.0, 420.0), 330.0);
    let second = s.things.values().filter(|t| t.kind == "balls").map(|t| t.id).max().expect("the copy");
    let colours = Gradient::spread(
        &[Color::rgb(240, 80, 120), Color::rgb(250, 200, 80), Color::rgb(160, 60, 250)],
        Mix::Even,
        false,
    );
    for (ball, form) in [(first, "sweep"), (second, "spiral")] {
        let layers = &s.card(ball, SlotId::Look).unwrap().layers;
        let glow = layers.iter().find(|l| l.kind == "layer.glow").map(|l| l.id);
        if let Some(glow) = glow {
            s.remove_layer(reg, ball, SlotId::Look, glow).unwrap();
        }
        let pattern = s.add_layer(reg, ball, SlotId::Look, "layer.pattern").unwrap();
        s.move_layer(reg, ball, SlotId::Look, pattern, 1).unwrap();
        let p = &mut s.card_mut(ball, SlotId::Look).unwrap().params;
        p.set(&layered_key(pattern, "in_pattern"), Value::Choice("gradient".into()));
        p.set(&layered_key(pattern, "in_from"), Value::Choice("own_gradient".into()));
        p.set(&layered_key(pattern, "in_colours"), Value::Gradient(colours.clone()));
        p.set(&layered_key(pattern, "in_colours~form"), Value::Choice(form.into()));
    }
    s
}

/// The club demo with its haze carried by a gusty Air card wind, and the balls following the air
/// (W13): the haze drifts with the wind, the balls with it.
fn haze_with_the_air(reg: &Registry) -> Scene {
    let mut s = scenes::club_demo(reg);
    lighting(&mut s, "haze_moves", Value::Choice("with_the_air".into()));
    bpe_kit::things::particles::air::ensure_air_card(&mut s, reg).unwrap();
    let air = s.card_mut(ThingId::ROOT, SlotId::Air).unwrap();
    for (k, v) in [("wind_speed", 220.0), ("wind_direction", -30.0), ("gusts", 0.6), ("gust_every", 1.5)] {
        air.params.set(k, Value::Number(v));
    }
    let balls: Vec<ThingId> = s.things.values().filter(|t| t.kind == "balls").map(|t| t.id).collect();
    for b in balls {
        if let Some(m) = s.card_mut(b, SlotId::Motion) {
            m.params.set("follow_air", Value::Number(1.5));
        }
    }
    s
}

/// The physics test-scene families that draw something worth seeing: text, particles, emitters.
fn family_shots(reg: &Registry) -> Vec<Shot> {
    let mut out = Vec::new();
    let named = |prefix: &str, name: &str| format!("{prefix}-{}", name.to_lowercase().replace([' ', '/', ','], "-"));
    for (name, scene) in scenes::text_scenes(reg) {
        out.push(shot(named("text", &name), scene, vec![2.0]));
    }
    for (name, scene) in scenes::particle_scenes(reg).into_iter().chain(scenes::particle_motion_scenes(reg)) {
        out.push(shot(named("particles", &name), scene, vec![2.0]));
    }
    for (name, scene) in scenes::emitter_scenes(reg) {
        out.push(shot(named("emitter", &name), scene, vec![2.0]));
    }
    out
}

/// Every ready-made Particles and Text look, each in the test scenes' starting room (a ring and one ball); looks
/// that ride ride the ball.
fn look_shots(reg: &Registry) -> Vec<Shot> {
    use bpe_kit::things::{particles, text};
    let mut out = Vec::new();
    let slug = |name: &str| name.to_lowercase().replace([' ', '/', ','], "-");
    for look in particles::looks::Look::all().into_iter().skip(1) {
        let mut scene = new_scene(reg);
        let parent = if look.rides() { first_of(&scene, "balls") } else { ThingId::ROOT };
        particles::looks::add_particles(&mut scene, reg, look, parent).expect("a particles look");
        out.push(shot(format!("particle-look-{}", slug(look.name())), scene, vec![2.0]));
    }
    for look in text::looks::LOOKS {
        let mut scene = new_scene(reg);
        text::looks::add_text(&mut scene, reg, look, ThingId::ROOT).expect("a text look");
        out.push(shot(format!("text-look-{}", slug(look.name())), scene, vec![2.0]));
    }
    out
}

/// Every ready-made look for balls (a grid of 30, still and moving sideways so trails show) and shapes (a grid of
/// 18 rounded rectangles), on a plain scene with no gravity.
fn ready_shots(reg: &Registry) -> Vec<Shot> {
    use bpe_kit::ready_looks::{Wears, add_with_look, all};
    let mut balls = Scene::empty(reg);
    no_gravity(&mut balls);
    for (i, look) in all().iter().filter(|l| l.wears == Wears::Balls).enumerate() {
        let id = add_with_look(&mut balls, reg, "balls", ThingId::ROOT, look).expect("a ball look");
        let t = balls.things.get_mut(&id).unwrap();
        let mut s = BallsSettings::from_params(&t.params);
        s.radius = 44.0;
        s.velocity = Vec2::new(260.0, 0.0);
        t.params = s.to_params();
        let (col, row) = ((i % 5) as f32, (i / 5) as f32);
        t.transform.position = Vec2::new(-400.0 + col * 200.0, -750.0 + row * 300.0);
    }
    let mut still = balls.clone();
    for t in still.things.values_mut().filter(|t| t.kind == "balls") {
        let mut s = BallsSettings::from_params(&t.params);
        s.velocity = Vec2::ZERO;
        t.params = s.to_params();
    }
    let mut shapes = Scene::empty(reg);
    for (i, look) in all().iter().filter(|l| l.wears == Wears::Shape).enumerate() {
        let id = add_with_look(&mut shapes, reg, "shape", ThingId::ROOT, look).expect("a shape look");
        let t = shapes.things.get_mut(&id).unwrap();
        let mut s = ShapeSettings::from_params(&t.params);
        s.start = bpe_kit::ShapeStart::Ellipse;
        s.width = 220.0;
        s.height = 150.0;
        s.fill = bpe_kit::ShapeFill::Filled;
        t.params = s.to_params();
        let (col, row) = ((i % 3) as f32, (i / 3) as f32);
        t.transform.position = Vec2::new(-340.0 + col * 340.0, -760.0 + row * 300.0);
    }
    vec![
        shot("ready-balls-still", still, vec![0.0]),
        shot("ready-balls-moving", balls, vec![0.7]),
        shot("ready-shapes", shapes, vec![0.5]),
    ]
}

fn wanted(name: &str, only: &Option<String>) -> bool {
    only.as_ref().is_none_or(|o| o.split(',').any(|o| !o.is_empty() && name.contains(o)))
}

/// The `LOOK_SET` pairs written onto the scene's Lighting card.
fn apply_set(scene: &mut Scene) {
    let Ok(set) = std::env::var("LOOK_SET") else { return };
    for pair in set.split(';').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').expect("LOOK_SET pairs are key=value");
        let v = v.parse::<f64>().map(Value::Number).unwrap_or_else(|_| Value::Choice(v.into()));
        lighting(scene, k, v);
    }
}

#[test]
#[ignore = "renders full-size pictures to look at; run with --ignored"]
fn render_at_full_size() {
    let reg = kit().registry;
    let dir = out_dir();
    let only = std::env::var("LOOK_ONLY").ok();
    let show = match std::env::var("LOOK_SHOW").as_deref() {
        Ok("light") => LightShow::LightOnly,
        _ => LightShow::Picture,
    };
    let qualities: Vec<String> = std::env::var("LOOK_QUALITIES")
        .unwrap_or_else(|_| "good,ultra".into())
        .split(',')
        .map(str::to_string)
        .collect();
    let (device, queue) = test_device().expect("a graphics device (set up as for the picture tests)");
    let renderer = Renderer::new(&device, W, H);
    let mut gpu = Gpu { device, queue, renderer };

    let mut lit = Vec::new();
    for base in lighting_shots(&reg) {
        for q in &qualities {
            let mut scene = base.scene.clone();
            lighting(&mut scene, "quality", Value::Choice(q.clone()));
            lit.push(shot(format!("{}-{q}", base.name), scene, base.times.clone()));
        }
    }
    let groups = [
        ("lighting", lit),
        ("demos", demo_shots(&reg)),
        ("families", family_shots(&reg)),
        ("looks", look_shots(&reg)),
        ("ready", ready_shots(&reg)),
    ];
    let mut count = 0;
    for (group, shots) in groups {
        let mut thumbs = Vec::new();
        for mut s in shots.into_iter().filter(|s| wanted(&s.name, &only)) {
            apply_set(&mut s.scene);
            for (name, pixels) in render(&mut gpu, &s, show) {
                thumbs.push((name.clone(), save_all(&dir, &name, &pixels)));
                count += 1;
            }
        }
        save_sheet(&dir, group, &thumbs);
    }
    eprintln!("{count} pictures in {}", dir.display());
    assert!(count > 0, "LOOK_ONLY matched no picture");
}

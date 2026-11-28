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
//! - `LOOK_FILE`: a saved scene file drawn as well (pictures named `file-...`), at `LOOK_TIMES` (a comma list of
//!   seconds, default `1,3,5`). With `LOOK_ONLY=file` only it is drawn.
//! - Water mode's plain drawing has its own group, `water` (`LOOK_ONLY=water`), with the look drawing's pictures
//!   (`LOOK_ONLY=water-look`), a final made and drawn from its take (`LOOK_ONLY=water-final`), and
//!   the extras and wet marks on a final (`LOOK_ONLY=water-extras`), and the seven ready-made looks on a final
//!   (`LOOK_ONLY=water-looks`; `LOOK_SCENES=<folder>` saves each look's scene file there, to render), and the
//!   ready-made liquids in nine basins (`LOOK_ONLY=water-liquids`).
//! - `LOOK_TAKES`: a folder the extras' and the looks' finals are kept in, so a change to the look draws again
//!   without making it again (about 20 minutes on the cloud's software card).

use bpe_core::maths::Maths;
use bpe_core::{Registry, Scene, Settings, SlotId, ThingId, Value};
use bpe_gfx::{Frame, LightShow, Renderer, build_frame, test_device};
use bpe_kit::things::balls::BallsSettings;
use bpe_kit::things::light::{LightSettings, LightType};
use bpe_kit::things::shape::ShapeSettings;
use bpe_kit::{add_thing, kit, new_scene, scenes};
use bpe_runtime::Playback;
use glam::Vec2;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

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
        shot("gaps", gaps(reg), vec![0.0, 2.0]),
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
        at("grading-basic", grading_basic(reg)),
        at("grading-curves", grading_curves(reg)),
        at("grading-colour", grading_colour(reg)),
        at("grading-detail", grading_detail(reg)),
        shot("grading-masks", grading_masks(reg), vec![1.0, 2.5]),
        shot("grading-things", grading_things(reg), vec![1.0, 2.5]),
        at("grading-stage-follow", grading_stage_follow(reg)),
        at("stage-camera-orbit", stage_camera(reg, 25.0)),
        at("stage-camera-orbit-far", stage_camera(reg, -40.0)),
        at("stage-depth-of-field", stage_depth_of_field(reg)),
        at("gradient-seams", gradient_seams(reg)),
        at("lit-liquid-smoke", lit_liquid_smoke(reg, true)),
        at("lit-liquid-smoke-own-light", lit_liquid_smoke(reg, false)),
        // Liquid part 1: water in the ring with its ball dropping in, and blue paint poured into
        // yellow paint.
        shot("liquid-water", liquid_scene(reg, "Liquid water"), vec![0.5, 2.0]),
        shot("liquid-mixing", liquid_scene(reg, "Liquid paint mixing"), vec![1.5, 3.0]),
        // Liquid part 3: every Look layer in its own light, under a lamp, and glowing.
        shot("liquid-looks", liquid_look(reg, "Liquid looks"), vec![1.0, 2.5, 2.55]),
        shot("liquid-lit", liquid_look(reg, "Liquid lit pool"), vec![1.0, 2.5]),
        shot("liquid-glow", liquid_look(reg, "Liquid glow"), vec![2.0]),
        // Liquid fixes part B: lots of clear water, lit from inside the ring, with balls dropped in
        // (self shadows, depth rings and columns, ripples over the body, the gap at the wall).
        shot("liquid-deep-lit", liquid_deep_lit(reg), vec![1.0, 1.05, 2.5]),
        // Spray and bouncing fixed: water poured onto a shape in a box (it sprayed everywhere; look
        // for whole sheets off the shape, a pool that fills level, and streams on the walls drawn).
        shot("liquid-pour-on-shape", scenes::liquid_pour_on_a_shape(reg), vec![1.5, 3.0, 5.0]),
    ]
}

/// Liquid fixes part B: a ring of clear water at Level 0.7 under a Point light inside the ring,
/// five balls dropped into it.
fn liquid_deep_lit(reg: &Registry) -> Scene {
    use bpe_kit::paint::Paint;
    use bpe_kit::things::scene::SceneSettings;
    let mut s = new_scene(reg);
    let ring = first_of(&s, "shape");
    scenes::add_liquid(&mut s, reg, ring, |f| f.level = 0.7, |_| {});
    let root = s.things.get_mut(&ThingId::ROOT).unwrap();
    let mut r = SceneSettings::from_params(&root.params);
    r.background = Paint::solid(bpe_core::Color::rgb(0x30, 0x34, 0x40));
    root.params = r.to_params();
    let lamp = bpe_kit::things::light::add_light(&mut s, reg, LightType::Point, ThingId::ROOT).unwrap();
    s.things.get_mut(&lamp).unwrap().params.set("position", Value::Vec2([-150.0, -400.0]));
    for i in 0..5 {
        let at = Vec2::new(-300.0 + 150.0 * i as f32, -350.0 - 40.0 * i as f32);
        add_ball(&mut s, reg, at, 40.0);
    }
    s
}

/// One of liquid part 3's look scenes.
fn liquid_look(reg: &Registry, name: &str) -> Scene {
    scenes::liquid_look_scenes(reg).into_iter().find(|(n, _)| n == name).map(|(_, s)| s).expect("a liquid look scene")
}

/// One of liquid part 1's test scenes.
fn liquid_scene(reg: &Registry, name: &str) -> Scene {
    scenes::liquid_scenes(reg).into_iter().find(|(n, _)| n == name).map(|(_, s)| s).expect("a liquid scene")
}

/// `scene` given a Grade card, with `basic` and `curve` setting its Basic and Tone Curve panels.
fn graded(mut scene: Scene, reg: &Registry, basic: &[(&str, Value)], curve: &[(&str, Value)]) -> Scene {
    use bpe_kit::modules::grade;
    grade::ensure_grade(&mut scene, reg);
    let card = scene.card(ThingId::ROOT, SlotId::Grade).expect("made above");
    let panels: Vec<_> =
        grade::panels_of(card, grade::whole_of(card).expect("made above")).iter().map(|l| l.id).collect();
    let params = &mut scene.card_mut(ThingId::ROOT, SlotId::Grade).expect("made above").params;
    for (panel, settings) in [(panels[0], basic), (panels[1], curve)] {
        for (key, v) in settings {
            params.set(&bpe_core::layered_key(panel, key), v.clone());
        }
    }
    scene
}

/// The lit demo through a strong Basic panel: cooler, more contrast, its glows pulled back under
/// white, its shadows opened and more colour (look for banding in the smooth falloffs).
fn grading_basic(reg: &Registry) -> Scene {
    let n = Value::Number;
    graded(
        scenes::lit_demo(reg),
        reg,
        &[
            ("temp", n(-35.0)),
            ("tint", n(10.0)),
            ("exposure", n(0.3)),
            ("contrast", n(40.0)),
            ("highlights", n(-100.0)),
            ("shadows", n(60.0)),
            ("whites", n(-20.0)),
            ("blacks", n(-30.0)),
            ("vibrance", n(50.0)),
            ("saturation", n(10.0)),
        ],
        &[],
    )
}

/// The colour demo through the Tone Curve: a lifted parametric darks and lights, an S point
/// curve and a cross-processed look from the red and blue curves.
fn grading_curves(reg: &Registry) -> Scene {
    use bpe_core::CurvePoint;
    let n = Value::Number;
    let curve = |points: &[(f32, f32)]| Value::Curve(points.iter().map(|&(x, y)| CurvePoint::new(x, y, 0.0)).collect());
    graded(
        scenes::colour_demo(reg),
        reg,
        &[],
        &[
            ("darks", n(25.0)),
            ("lights", n(-15.0)),
            ("rgb", curve(&[(0.0, 0.05), (0.25, 0.18), (0.75, 0.85), (1.0, 1.0)])),
            ("red", curve(&[(0.0, 0.0), (0.5, 0.58), (1.0, 1.0)])),
            ("blue", curve(&[(0.0, 0.12), (0.5, 0.45), (1.0, 0.9)])),
        ],
    )
}

/// The colour demo through the colour panels: HSL turning and greying some bands, a Point Colour
/// pick, a teal-and-orange split tone from the Colour Grading wheels and Calibration's primaries
/// (look for banding where hues meet, odd hues near greys and clipped colours).
fn grading_colour(reg: &Registry) -> Scene {
    use bpe_kit::modules::grade;
    let mut scene = scenes::colour_demo(reg);
    grade::ensure_grade(&mut scene, reg);
    let card = scene.card(ThingId::ROOT, SlotId::Grade).expect("made above");
    let panels: Vec<_> =
        grade::panels_of(card, grade::whole_of(card).expect("made above")).iter().map(|l| l.id).collect();
    let n = Value::Number;
    let settings: [(usize, &str, Value); 22] = [
        (2, "red_hue", n(30.0)),
        (2, "orange_saturation", n(40.0)),
        (2, "yellow_luminance", n(-30.0)),
        (2, "green_saturation", n(-80.0)),
        (2, "aqua_hue", n(-40.0)),
        (2, "blue_saturation", n(60.0)),
        (2, "blue_luminance", n(-40.0)),
        (2, "purple_hue", n(50.0)),
        (2, "magenta_saturation", n(-50.0)),
        (3, "pick1_on", Value::Bool(true)),
        (3, "pick1_colour", Value::Color(bpe_core::Color::rgb(255, 120, 40))),
        (3, "pick1_hue", n(-30.0)),
        (3, "pick1_luminance", n(30.0)),
        (3, "pick1_range", n(70.0)),
        (4, "shadows_hue", n(195.0)),
        (4, "shadows_saturation", n(45.0)),
        (4, "highlights_hue", n(35.0)),
        (4, "highlights_saturation", n(35.0)),
        (4, "midtones_luminance", n(10.0)),
        (4, "balance", n(-20.0)),
        (7, "shadows_tint", n(25.0)),
        (7, "blue_hue", n(-40.0)),
    ];
    let params = &mut scene.card_mut(ThingId::ROOT, SlotId::Grade).expect("made above").params;
    for (panel, key, v) in settings {
        params.set(&bpe_core::layered_key(panels[panel], key), v);
    }
    scene
}

/// The lit demo through the area steps and effects: Texture, Clarity and Dehaze, Sharpening, a
/// dark vignette whose Highlights keep the lamps, grain and a light gradient map (look for halos at
/// edges, blocks or rings in the blurs, the grain's look and banding in the vignette).
fn grading_detail(reg: &Registry) -> Scene {
    use bpe_kit::modules::grade;
    let mut scene = scenes::lit_demo(reg);
    grade::ensure_grade(&mut scene, reg);
    let card = scene.card(ThingId::ROOT, SlotId::Grade).expect("made above");
    let panels: Vec<_> =
        grade::panels_of(card, grade::whole_of(card).expect("made above")).iter().map(|l| l.id).collect();
    let n = Value::Number;
    let map = bpe_core::Gradient::even(&[
        bpe_core::Color::rgb(20, 10, 60),
        bpe_core::Color::rgb(200, 90, 60),
        bpe_core::Color::rgb(255, 240, 200),
    ]);
    let settings: [(usize, &str, Value); 14] = [
        (0, "texture", n(40.0)),
        (0, "clarity", n(45.0)),
        (0, "dehaze", n(25.0)),
        (5, "amount", n(70.0)),
        (5, "radius", n(1.2)),
        (5, "detail", n(40.0)),
        (6, "vignette_amount", n(-60.0)),
        (6, "vignette_feather", n(70.0)),
        (6, "vignette_highlights", n(60.0)),
        (6, "grain_amount", n(30.0)),
        (6, "grain_size", n(30.0)),
        (6, "grain_roughness", n(60.0)),
        (6, "map", Value::Gradient(map)),
        (6, "map_amount", n(20.0)),
    ];
    let params = &mut scene.card_mut(ThingId::ROOT, SlotId::Grade).expect("made above").params;
    for (panel, key, v) in settings {
        params.set(&bpe_core::layered_key(panels[panel], key), v);
    }
    scene
}

/// The lit demo through masks: a glow on every ball (a Radial Gradient following each, growing
/// with it), a darker, cooler top (a Linear Gradient), and warmer highlights only inside a soft
/// oval in the middle (a Luminance Range intersected with a Radial Gradient). Look for hard
/// edges at the feathers, seams between ball copies and banding in the fades.
fn grading_masks(reg: &Registry) -> Scene {
    use bpe_kit::modules::grade;
    let mut scene = scenes::lit_demo(reg);
    let n = Value::Number;
    let choice = |s: &str| Value::Choice(s.into());
    let masked = |scene: &mut Scene, first: &str, part: &[(&str, Value)], basic: &[(&str, Value)]| {
        let mask = grade::add_mask(scene, reg, first).expect("a mask");
        let card = scene.card(ThingId::ROOT, SlotId::Grade).expect("made with the mask");
        let (p, b) = (grade::parts_of(card, mask)[0].id, grade::panels_of(card, mask)[0].id);
        let params = &mut scene.card_mut(ThingId::ROOT, SlotId::Grade).expect("made above").params;
        for (id, settings) in [(p, part), (b, basic)] {
            for (key, v) in settings {
                params.set(&bpe_core::layered_key(id, key), v.clone());
            }
        }
        mask
    };
    masked(
        &mut scene,
        grade::RADIAL,
        &[
            ("place", choice("on_every_ball")),
            ("grows", Value::Bool(true)),
            ("width", n(150.0)),
            ("height", n(150.0)),
            ("feather", n(70.0)),
        ],
        &[("exposure", n(1.2)), ("temp", n(30.0)), ("saturation", n(40.0))],
    );
    masked(
        &mut scene,
        grade::LINEAR,
        &[("y", n(-500.0)), ("angle", n(80.0)), ("feather", n(700.0))],
        &[("exposure", n(-1.0)), ("temp", n(-40.0))],
    );
    let oval = masked(
        &mut scene,
        grade::LUMINANCE_RANGE,
        &[("low", n(45.0)), ("high", n(100.0)), ("smoothness", n(60.0))],
        &[("temp", n(60.0)), ("tint", n(20.0)), ("exposure", n(0.3))],
    );
    let part = grade::add_part(&mut scene, reg, oval, grade::RADIAL, grade::Combine::Intersect).expect("a part");
    let params = &mut scene.card_mut(ThingId::ROOT, SlotId::Grade).expect("made above").params;
    for (key, v) in [("y", n(200.0)), ("width", n(900.0)), ("height", n(1300.0)), ("feather", n(80.0))] {
        params.set(&bpe_core::layered_key(part, key), v);
    }
    scene
}

/// A mask after the others whose first part is `first` with `part`'s settings, and whose Basic
/// panel has `basic`'s.
fn mask_with(
    scene: &mut Scene,
    reg: &Registry,
    first: &str,
    part: &[(&str, Value)],
    basic: &[(&str, Value)],
) -> bpe_core::LayerId {
    use bpe_kit::modules::grade;
    let mask = grade::add_mask(scene, reg, first).expect("a mask");
    let card = scene.card(ThingId::ROOT, SlotId::Grade).expect("made with the mask");
    let (p, b) = (grade::parts_of(card, mask)[0].id, grade::panels_of(card, mask)[0].id);
    let params = &mut scene.card_mut(ThingId::ROOT, SlotId::Grade).expect("made above").params;
    for (id, settings) in [(p, part), (b, basic)] {
        for (key, v) in settings {
            params.set(&bpe_core::layered_key(id, key), v.clone());
        }
    }
    mask
}

/// The lit demo with Things masks and a Brush (colour grading part 5): the balls brighter and
/// warmer, grown out to take in their glow; the background where no thing is cooler and darker;
/// a soft painted swoosh across the middle more colourful. Look at the edges of the balls' masks
/// (no jagged steps, no gap where a ball overlaps a shape) and the brush's soft edge (no banding).
fn grading_things(reg: &Registry) -> Scene {
    use bpe_kit::modules::grade;
    let mut scene = scenes::lit_demo(reg);
    let n = Value::Number;
    let choice = |s: &str| Value::Choice(s.into());
    mask_with(
        &mut scene,
        reg,
        grade::THINGS,
        &[("only", choice("balls")), ("expand", n(14.0)), ("feather", n(40.0))],
        &[("exposure", n(0.8)), ("temp", n(40.0)), ("saturation", n(30.0))],
    );
    mask_with(
        &mut scene,
        reg,
        grade::THINGS,
        &[("only", choice("background"))],
        &[("exposure", n(-0.7)), ("temp", n(-40.0)), ("saturation", n(-30.0))],
    );
    let swoosh = bpe_core::Stroke {
        points: (0..=40)
            .map(|i| {
                let t = i as f32 / 40.0;
                [-450.0 + 900.0 * t, 250.0 * (t * std::f32::consts::PI * 2.0).det_sin()]
            })
            .collect(),
        size: 140.0,
        softness: 0.8,
        strength: 0.8,
        ..Default::default()
    };
    let strokes = Value::Strokes(bpe_core::Strokes { list: vec![swoosh] });
    mask_with(&mut scene, reg, grade::BRUSH, &[("strokes", strokes)], &[("saturation", n(60.0)), ("tint", n(30.0))]);
    scene
}

/// The Stage camera swung round with a radial mask on every ball: each mask sits on its ball as
/// the moved camera shows it (part 4's drift, fixed in part 5).
fn grading_stage_follow(reg: &Registry) -> Scene {
    use bpe_kit::modules::grade;
    let mut scene = stage_camera(reg, 25.0);
    let n = Value::Number;
    mask_with(
        &mut scene,
        reg,
        grade::RADIAL,
        &[
            ("place", Value::Choice("on_every_ball".into())),
            ("grows", Value::Bool(true)),
            ("width", n(60.0)),
            ("height", n(60.0)),
            ("feather", n(10.0)),
        ],
        &[("exposure", n(2.0)), ("temp", n(80.0))],
    );
    scene
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
/// Gaps in shapes: a glowing ring with two gaps round a hexagon with one, a ball inside each, so the
/// caps' round ends and the glow at the gaps are seen.
fn gaps(reg: &Registry) -> Scene {
    let mut scene = bpe_kit::new_studio_scene(reg);
    let place = |scene: &mut Scene, settings: ShapeSettings, look: &str| {
        let id = add_thing(scene, reg, "shape", ThingId::ROOT).unwrap();
        let t = scene.things.get_mut(&id).unwrap();
        t.params = settings.to_params();
        t.transform.position = Vec2::ZERO;
        let ready = bpe_kit::ready_looks::find(look).unwrap();
        let (mine, how) = (bpe_kit::ready_looks::Colours::Look, bpe_kit::ready_looks::How::Replace);
        bpe_kit::ready_looks::apply(scene, reg, id, ready, mine, how).unwrap();
    };
    let ring = ShapeSettings { gaps: 2, gap_size: 0.08, gap_at: 0.125, ..ShapeSettings::circle(470.0, 14.0) };
    place(&mut scene, ring, "shape.glow_ring");
    let hexagon =
        ShapeSettings { gaps: 1, gap_size: 0.1, gap_at: 0.5, spin: 25.0, ..ShapeSettings::polygon(6, 300.0, 10.0) };
    place(&mut scene, hexagon, "shape.neon_tube");
    let ball = scene.things.values().find(|t| t.kind == "balls").map(|t| t.id);
    if let Some(ball) = ball {
        let t = scene.things.get_mut(&ball).unwrap();
        t.transform.position = Vec2::new(0.0, -100.0);
        let mut b = BallsSettings::from_params(&t.params);
        b.radius = 24.0;
        b.velocity = Vec2::new(300.0, 500.0);
        t.params = b.to_params();
    }
    scene
}

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

/// Small links between features (W12): a shape wearing letter moves, a spot aimed at a ball, letters as Ripples
/// sources, a word of balls and a word of particles in a Text font.
fn link_shots(reg: &Registry) -> Vec<Shot> {
    use bpe_core::{Color, Gradient, Mix, layered_key};
    let mut out = Vec::new();
    {
        let mut s = new_scene(reg);
        let shape = first_of(&s, "shape");
        for (kind, key, v) in [("text.move.spin", "speed", 40.0), ("text.move.wave", "height", 60.0)] {
            let id = s.add_layer(reg, shape, SlotId::Look, kind).unwrap();
            s.card_mut(shape, SlotId::Look).unwrap().params.set(&layered_key(id, key), Value::Number(v));
        }
        out.push(shot("links-shape-moves", s, vec![0.5, 0.5 + 1.0 / 60.0]));
    }
    {
        let mut s = room(reg, Vec2::new(200.0, 300.0), 60.0);
        // Inside the ring, aimed at the first ball (its Direction would point it straight up).
        let mut p = light(LightType::Spot, Vec2::new(-250.0, -300.0), 10.0, 2.5);
        p.direction = -90.0;
        p.cone = 30.0;
        p.height = 120.0;
        p.points = bpe_kit::things::light::Points::AtAThing;
        set_light(&mut s, &p);
        add_ball(&mut s, reg, Vec2::new(-250.0, 200.0), 30.0);
        out.push(shot("links-aimed-spot", s, vec![0.0]));
    }
    {
        let mut s = new_scene(reg);
        let text = add_thing(&mut s, reg, "text", ThingId::ROOT).unwrap();
        let t = s.things.get_mut(&text).unwrap();
        t.screen = Some(false);
        t.params.set("words", Value::Text("RIPPLE".into()));
        let colours = [Color::rgb(20, 30, 90), Color::rgb(40, 200, 220), Color::rgb(240, 80, 160)];
        let root = &mut s.things.get_mut(&ThingId::ROOT).unwrap().params;
        root.set("background_colours", Value::Gradient(Gradient::spread(&colours, Mix::Light, false)));
        root.set(
            "background_colours~form",
            Value::Choice(bpe_core::Choice::name(bpe_core::GradientForm::Ripples).into()),
        );
        out.push(shot("links-text-ripples", s, vec![0.5]));
    }
    {
        let mut s = new_scene(reg);
        first_ball(&mut s, Vec2::new(0.0, -500.0), |b| {
            b.group = true;
            b.shape = bpe_kit::things::balls::GroupShape::Word;
            b.word = "POP".into();
            b.word_font = "Anton".into();
            b.radius = 8.0;
        });
        no_gravity(&mut s);
        let id = bpe_kit::things::particles::looks::add_particles(
            &mut s,
            reg,
            bpe_kit::things::particles::looks::Look::FormAWord,
            ThingId::ROOT,
        )
        .unwrap();
        s.things.get_mut(&id).unwrap().params.set("font", Value::Text("Bungee".into()));
        out.push(shot("links-word-fonts", s, vec![0.0, 3.0]));
    }
    out
}

/// One way to ride a parent (W14): small lights on each letter of a Text with sparks off its outline and a
/// light on its pen, a light on a ball's rim and a label riding round a shape's edge, in a dim lit room.
fn riding_shots(reg: &Registry) -> Vec<Shot> {
    use bpe_kit::things::light::{LightType, add_light};
    use bpe_kit::things::particles::looks::{Look, add_particles};
    let mut s = room(reg, Vec2::new(-250.0, 450.0), 70.0);
    lighting(&mut s, "ambient", Value::Number(0.15));
    let light = first_of(&s, "light");
    s.things.get_mut(&light).unwrap().params.set("strength", Value::Number(0.3));
    let text = add_thing(&mut s, reg, "text", ThingId::ROOT).unwrap();
    let t = s.things.get_mut(&text).unwrap();
    t.screen = Some(false);
    t.transform.position = Vec2::new(0.0, -350.0);
    t.params.set("words", Value::Text("RIDE".into()));
    t.params.set("size", Value::Number(260.0));
    let on = |s: &mut Scene, id: ThingId, pairs: &[(&str, Value)]| {
        for (k, v) in pairs {
            s.things.get_mut(&id).unwrap().params.set(k, v.clone());
        }
    };
    let each = add_light(&mut s, reg, LightType::Point, text).unwrap();
    let warm = Value::Color(bpe_core::Color::rgb(0xff, 0xa0, 0x50));
    on(
        &mut s,
        each,
        &[
            ("rides_on", Value::Choice("each_letter".into())),
            ("size", Value::Number(6.0)),
            ("reach", Value::Number(320.0)),
            ("strength", Value::Number(1.4)),
            ("colour", warm),
        ],
    );
    let sparks = add_particles(&mut s, reg, Look::Empty, text).unwrap();
    on(
        &mut s,
        sparks,
        &[
            ("rides_on", Value::Choice("the_edge".into())),
            ("rate", Value::Number(300.0)),
            ("speed", Value::Number(60.0)),
        ],
    );
    let ball = first_of(&s, "balls");
    let rim = add_light(&mut s, reg, LightType::Point, ball).unwrap();
    let cool = Value::Color(bpe_core::Color::rgb(0x60, 0xc0, 0xff));
    on(
        &mut s,
        rim,
        &[
            ("rides_on", Value::Choice("the_edge".into())),
            ("edge_at", Value::Number(0.125)),
            ("size", Value::Number(5.0)),
            ("reach", Value::Number(400.0)),
            ("strength", Value::Number(1.6)),
            ("colour", cool),
        ],
    );
    add_shape(&mut s, reg, Vec2::new(250.0, 450.0), ShapeSettings::default());
    let shape = s.order.iter().copied().rfind(|id| s.things[id].kind == "shape").unwrap();
    let label = add_thing(&mut s, reg, "text", shape).unwrap();
    on(
        &mut s,
        label,
        &[
            ("words", Value::Text("EDGE".into())),
            ("size", Value::Number(60.0)),
            ("rides_on", Value::Choice("the_edge".into())),
            ("edge_at", Value::Number(0.625)),
        ],
    );
    vec![shot("riding", s, vec![0.0, 1.0])]
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

/// Every built-in scene (`bpe_kit::library`) at its poster moment and two more, named `library-<id>`.
fn library_shots(reg: &Registry) -> Vec<Shot> {
    bpe_kit::library::all()
        .iter()
        .map(|e| {
            let i = &e.info;
            shot(format!("library-{}", i.id), e.scene(reg), vec![i.poster_seconds, i.seconds * 0.4, i.seconds * 0.8])
        })
        .collect()
}

/// The scene file `LOOK_FILE` names (if any), at the `LOOK_TIMES` moments.
fn file_shots(reg: &Registry) -> Vec<Shot> {
    let Some(path) = std::env::var_os("LOOK_FILE") else { return Vec::new() };
    let text = std::fs::read_to_string(&path).expect("LOOK_FILE can be read");
    let scene = bpe_core::save::load(&text, reg).expect("LOOK_FILE is a scene file");
    let times = std::env::var("LOOK_TIMES").unwrap_or_else(|_| "1,3,5".into());
    let times = times.split(',').map(|t| t.trim().parse().expect("LOOK_TIMES are seconds")).collect();
    vec![shot("file", scene, times)]
}

/// Water mode's plain drawing (`bpe_water::draw`, its own drawing on the card, not the renderer's) of drops laid out
/// by hand: the bench at 8, 3 and 2 px cells, a screen two thirds full at 2 px (1.4 million drops), and part 3's
/// pieces in their colours at 8 and 3 px. Drops of real
/// runs are looked at with `bpe-water`'s `bench_pictures` (tests/plain.rs).
fn water_pictures(gpu: &Gpu, only: &Option<String>) -> Vec<(String, Vec<u8>)> {
    use bpe_water::draw::{Plain, PlainGpu, StoredDrops, View, sets};
    type Make = fn() -> sets::Bench;
    type Look = fn() -> bpe_water::draw::PlainLook;
    let all: [(&str, Make, Look); 7] = [
        ("water-bench-c8", || sets::bench(8.0), sets::look),
        ("water-bench-c3", || sets::bench(3.0), sets::look),
        ("water-bench-c2", || sets::bench(2.0), sets::look),
        ("water-pool-c2", || sets::screen_pool(2.0, 2.0 / 3.0), sets::look),
        // Part 3: pieces in their own colours, at the draft's and Fine's detail.
        ("water-pieces-c8", || sets::pieces(8.0), sets::pieces_look),
        ("water-pieces-c3", || sets::pieces(3.0), sets::pieces_look),
        // Part 4.2: two liquids' colours running together, and a blob carrying half of each.
        ("water-mixed-c3", || sets::mixed(3.0), sets::look),
    ];
    let card = PlainGpu::new(&gpu.device, &gpu.queue);
    let mut out = Vec::new();
    for (name, make, look) in all.into_iter().filter(|(name, ..)| wanted(name, only)) {
        let set = make();
        let stored = StoredDrops::new(&gpu.device, &gpu.queue, &set.drops);
        let mut plain = Plain::new(card.clone(), &set.field, sets::REST);
        let view = View::screen(1.0);
        assert_eq!((view.width, view.height), (W, H));
        plain.draw(&stored.buffers(), &view, &look());
        out.push((name.to_string(), plain.read()));
    }
    out
}

/// Water mode part 6a's look drawing at full size (`LOOK_ONLY=water-look`), of drops laid out by hand: the bench and
/// the pieces at 3 px under two warm Spots, each Material on the pieces, the Back's four kinds, caustics under a
/// waved pool lit by a Sun, and glow in the dark with bloom.
fn water_look_pictures(gpu: &Gpu, only: &Option<String>) -> Vec<(String, Vec<u8>)> {
    use bpe_water::draw::look::{
        BackLook, BackPicture, FinishLook, GlowFrom, Lamp, LampKind, LightLook, LiquidLook, Look, MaterialKind,
        MaterialLook,
    };
    use bpe_water::draw::{LookDraw, LookGpu, StoredDrops, View, sets};
    // (`LOOK_ONLY=water-look-pieces` picks one of them.)
    if !wanted("water-look", only) && !only.as_deref().is_some_and(|o| o.contains("water-look")) {
        return Vec::new();
    }
    let warm = [1.0, 0.82, 0.63];
    let spot = |x: f32, aim: f32| Lamp {
        kind: LampKind::Spot,
        place: Vec2::new(x, -820.0),
        aim,
        colour: warm,
        brightness: 1.6,
        spread: 70.0,
        softness: 0.4,
        ..Lamp::default()
    };
    let water = LiquidLook {
        colour: [0.31, 0.71, 0.91],
        deep: [0.02, 0.13, 0.23],
        clarity: 320.0,
        bend: 16.0,
        shine: 0.6,
        edge_sheen: 0.4,
        ripples: 0.5,
        ..LiquidLook::default()
    };
    let orange = LiquidLook { colour: [1.0, 0.54, 0.24], deep: [0.35, 0.1, 0.02], clarity: 120.0, ..water.clone() };
    let rock = MaterialLook {
        colour: [0.48, 0.36, 0.27],
        second: [0.29, 0.21, 0.15],
        texture: 160.0,
        depth: 14.0,
        roughness: 0.7,
        ..MaterialLook::default()
    };
    let lit = Look {
        liquids: vec![water.clone(), orange.clone()],
        rock: rock.clone(),
        back: BackLook::DarkerRock(0.65),
        light: LightLook { ambient: 0.12, bounce: 0.4, caustics: 1.2, shafts: 0.3, ..LightLook::default() },
        lamps: vec![spot(-350.0, 70.0), spot(350.0, 110.0)],
        time: 1.0,
        finish: FinishLook { bloom: 0.25, exposure: 1.0, ..FinishLook::default() },
        ..Look::default()
    };
    let material = |kind: MaterialKind, colour: [f32; 3], second: [f32; 3]| MaterialLook {
        kind,
        colour,
        second,
        texture: 90.0,
        depth: 10.0,
        roughness: 0.4,
        ..MaterialLook::default()
    };
    let pieces = Look {
        solids: vec![
            material(MaterialKind::Rock, [0.6, 0.45, 0.35], [0.35, 0.25, 0.2]),
            material(MaterialKind::Glass, [0.79, 0.84, 0.89], [0.5, 0.6, 0.7]),
            material(MaterialKind::Paper, [0.93, 0.9, 0.84], [0.8, 0.76, 0.7]),
            material(MaterialKind::Basalt, [0.14, 0.11, 0.1], [0.05, 0.04, 0.04]),
            material(MaterialKind::Flat, [0.92, 0.78, 0.25], [0.0; 3]),
            MaterialLook {
                outline: 4.0,
                outline_colour: [0.05, 0.05, 0.08],
                ..material(MaterialKind::Flat, [0.4, 0.7, 0.5], [0.0; 3])
            },
        ],
        ..lit.clone()
    };
    // A picture behind: stripes and a sun, written for the run.
    let picture = std::env::temp_dir().join(format!("bpe-look-back-{}.png", std::process::id()));
    let (pw, ph) = (540u32, 960u32);
    let mut pixels = Vec::with_capacity((pw * ph * 4) as usize);
    for y in 0..ph {
        for x in 0..pw {
            let d = Vec2::new(x as f32 - 270.0, y as f32 - 300.0).length();
            let band = ((y / 60) % 2) as u8;
            let c = if d < 90.0 { [255, 210, 90] } else { [40 + 60 * band, 70 + 40 * band, 140 + 50 * band] };
            pixels.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    save(&picture, pw, ph, &pixels);
    let gradient: Vec<[f32; 3]> = (0..64)
        .map(|i| {
            let t = i as f32 / 63.0;
            [0.02 + 0.3 * t, 0.03 + 0.05 * t, 0.12 * (1.0 - t) + 0.02]
        })
        .collect();
    let back = |b: BackLook| Look { back: b, ..lit.clone() };
    let glow = Look {
        liquids: vec![
            LiquidLook {
                colour: [0.42, 0.11, 1.0],
                clarity: 120.0,
                glow: 2.5,
                glow_colour: [1.0, 0.24, 0.78],
                glow_from: GlowFrom::Always,
                ..water.clone()
            },
            LiquidLook { glow: 2.0, glow_colour: [1.0, 0.54, 0.19], ..orange.clone() },
        ],
        rock: material(MaterialKind::Flat, [0.07, 0.06, 0.12], [0.0; 3]),
        back: BackLook::Colour([0.03, 0.02, 0.05]),
        light: LightLook { ambient: 0.05, bounce: 0.5, ..LightLook::default() },
        lamps: vec![],
        finish: FinishLook { bloom: 0.8, exposure: 1.0, ..FinishLook::default() },
        ..lit.clone()
    };
    let sun = Look {
        liquids: vec![LiquidLook { clarity: 600.0, ripples: 1.0, ..water.clone() }],
        rock: MaterialLook { colour: [0.85, 0.8, 0.7], second: [0.7, 0.62, 0.5], ..rock.clone() },
        back: BackLook::Colour([0.55, 0.75, 0.9]),
        light: LightLook { ambient: 0.35, caustics: 1.5, shafts: 0.4, ..LightLook::default() },
        lamps: vec![Lamp {
            kind: LampKind::Sun,
            aim: 80.0,
            colour: [1.0, 0.97, 0.9],
            brightness: 1.2,
            softness: 0.3,
            ..Lamp::default()
        }],
        ..lit.clone()
    };
    type Make = Box<dyn Fn() -> sets::Bench>;
    let all: Vec<(&str, Make, Look)> = vec![
        ("water-look-bench-c3", Box::new(|| sets::bench(3.0)), lit.clone()),
        ("water-look-bench-c8", Box::new(|| sets::bench(8.0)), lit.clone()),
        ("water-look-pieces-c3", Box::new(|| sets::pieces(3.0)), pieces),
        ("water-look-mixed-c3", Box::new(|| sets::mixed(3.0)), lit.clone()),
        ("water-look-back-darker", Box::new(|| sets::bench(3.0)), back(BackLook::DarkerRock(0.3))),
        ("water-look-back-colour", Box::new(|| sets::bench(3.0)), back(BackLook::Colour([0.1, 0.12, 0.2]))),
        (
            "water-look-back-gradient",
            Box::new(|| sets::bench(3.0)),
            back(BackLook::Gradient { samples: gradient, angle: 90.0 }),
        ),
        (
            "water-look-back-picture",
            Box::new(|| sets::bench(3.0)),
            back(BackLook::Picture(BackPicture::file(picture.display().to_string()))),
        ),
        ("water-look-caustics", Box::new(|| sets::wavy_pool(3.0, 500.0, 14.0, 110.0)), sun),
        ("water-look-glow", Box::new(|| sets::bench(3.0)), glow),
    ];
    let card = LookGpu::new(&gpu.device, &gpu.queue);
    let mut out = Vec::new();
    for (name, make, look) in all.into_iter().filter(|(name, ..)| wanted(name, only)) {
        let set = make();
        let stored = StoredDrops::new(&gpu.device, &gpu.queue, &set.drops);
        let mut drawing = LookDraw::new(card.clone(), &set.field, sets::REST);
        let view = View::screen(1.0);
        let started = std::time::Instant::now();
        drawing.draw(&stored.buffers(), &view, &look);
        let pixels = drawing.read();
        eprintln!(
            "{name}: {} drops, {:.0} ms {:?}",
            set.drops.len(),
            started.elapsed().as_secs_f64() * 1000.0,
            drawing.notes()
        );
        out.push((name.to_string(), pixels));
    }
    std::fs::remove_file(&picture).ok();
    out
}

/// Water mode part 8: a final made at Coarse detail (3 px) of one screen with a pool a third full
/// and a tap pouring into it, its moment at 1 s drawn from the take as the export draws it (in a new scene's look,
/// part 6), and the
/// same moment moved on half a frame by the drops' speeds (`water-final`, `water-final-ahead`).
fn water_final_pictures(gpu: &Gpu, only: &Option<String>) -> Vec<(String, Vec<u8>)> {
    use bpe_kit::things::water::finals::Finals;
    use bpe_kit::things::water::make::{MakeFinal, MakeProgress};
    use bpe_kit::things::water::{LIQUID, POUR, course_of, new_water_scene};
    use bpe_water::draw::{LookDraw, LookGpu, StoredDrops, View};
    use bpe_water::take::{Chunk, FrameCodec};
    if !wanted("water-final", only) {
        return Vec::new();
    }
    let reg = kit().registry;
    let mut scene = new_water_scene(&reg, "");
    let water = scene.card_mut(ThingId::ROOT, SlotId::Water).unwrap();
    water.params.set("final_detail", Value::Choice("coarse".into()));
    let course = course_of(&scene).unwrap();
    let r = 540.0 - 24.0;
    let chamber = bpe_core::Stroke {
        points: vec![[0.0, -960.0 + 24.0 + r], [0.0, 960.0 - 24.0 - r]],
        size: r,
        strength: 1.0,
        square: true,
        ..Default::default()
    };
    let carved = Value::Strokes(bpe_core::Strokes { list: vec![chamber] });
    scene.card_mut(course, SlotId::Form).unwrap().params.set("carved", carved);
    bpe_kit::add_thing(&mut scene, &reg, LIQUID, ThingId::ROOT).unwrap();
    let top = 936.0 - 1872.0 / 3.0;
    let fill = bpe_kit::add_thing(&mut scene, &reg, POUR, ThingId::ROOT).unwrap();
    scene.things.get_mut(&fill).unwrap().transform.position = Vec2::new(0.0, 0.5 * (top + 936.0));
    let main = &mut scene.card_mut(fill, SlotId::Main).unwrap().params;
    main.set("kind", Value::Choice("fill_at_the_start".into()));
    main.set("fill_size", Value::Vec2([2.0 * r, 936.0 - top]));
    let tap = bpe_kit::add_thing(&mut scene, &reg, POUR, ThingId::ROOT).unwrap();
    scene.things.get_mut(&tap).unwrap().transform.position = Vec2::new(-200.0, -800.0);
    let resolved = bpe_core::resolve(&scene, &reg);
    let make = MakeFinal::of(&scene, &resolved, 1.0);
    let finals = Finals::at(std::env::temp_dir().join(format!("bpe-look-final-{}", std::process::id())));
    std::fs::remove_dir_all(finals.dir()).ok();
    let water_gpu = bpe_water::WaterGpu::new(&gpu.device, &gpu.queue);
    let head = make.make(&water_gpu, &finals, &MakeProgress::default(), &AtomicBool::new(false)).unwrap();
    let mut chunk = Chunk::open(&finals.take_dir(head.print), 1).unwrap();
    let frame = FrameCodec::new().decode(&chunk.bytes(0).unwrap(), &head.layout()).unwrap();
    let mut stored = StoredDrops::new(&gpu.device, &gpu.queue, &[]);
    stored.set(&gpu.device, &gpu.queue, &frame.xv, &frame.kinds, &frame.mixes);
    let field = make.setup.field();
    let mut drawing = LookDraw::new(LookGpu::new(&gpu.device, &gpu.queue), &field, make.setup.numbers().wall_off);
    // (A new scene's look: the cards' defaults, no lamps.)
    let look = bpe_kit::things::water::look::look_of(&scene, &resolved, 1.0);
    let view = View::screen(1.0);
    let mut out = Vec::new();
    for (name, ahead) in [("water-final", 0.0), ("water-final-ahead", 0.5 / 60.0)] {
        drawing.draw_at(&stored.buffers(), &view, &look, ahead);
        out.push((name.to_string(), drawing.read()));
    }
    eprintln!("water-final: {} drops at {} px", head.drops, make.setup.cell);
    std::fs::remove_dir_all(finals.dir()).ok();
    out
}

/// Water mode part 6b's extras at full size (`LOOK_ONLY=water-extras`): a final made at Coarse
/// detail (3 px) of one screen with a pool a quarter full and a fast tap thrown at its right wall,
/// drawn from the take with its wet marks: foam, bubbles and mist on the pool under a Spot
/// (`water-extras-pool`, and `water-extras-none` with them off), sparks on the fast stream over a
/// dark back (`water-extras-sparks`) and the wet marks the stream leaves down the wall
/// (`water-extras-wet`).
fn water_extras_pictures(gpu: &Gpu, only: &Option<String>) -> Vec<(String, Vec<u8>)> {
    use bpe_kit::things::water::finals::Finals;
    use bpe_kit::things::water::make::{MakeFinal, MakeProgress};
    use bpe_kit::things::water::{LIQUID, POUR, course_of, new_water_scene};
    use bpe_water::draw::look::{BackLook, Lamp, LampKind};
    use bpe_water::draw::wet::HeldWet;
    use bpe_water::draw::{LookDraw, LookGpu, StoredDrops, View};
    use bpe_water::take::{Chunk, FrameCodec, chunk};
    if !wanted("water-extras", only) && !only.as_deref().is_some_and(|o| o.contains("water-extras")) {
        return Vec::new();
    }
    let reg = kit().registry;
    let mut scene = new_water_scene(&reg, "");
    let water = scene.card_mut(ThingId::ROOT, SlotId::Water).unwrap();
    water.params.set("final_detail", Value::Choice("coarse".into()));
    let course = course_of(&scene).unwrap();
    let r = 540.0 - 24.0;
    let chamber = bpe_core::Stroke {
        points: vec![[0.0, -960.0 + 24.0 + r], [0.0, 960.0 - 24.0 - r]],
        size: r,
        strength: 1.0,
        square: true,
        ..Default::default()
    };
    let carved = Value::Strokes(bpe_core::Strokes { list: vec![chamber] });
    scene.card_mut(course, SlotId::Form).unwrap().params.set("carved", carved);
    bpe_kit::add_thing(&mut scene, &reg, LIQUID, ThingId::ROOT).unwrap();
    let top = 936.0 - 1872.0 / 4.0;
    let fill = bpe_kit::add_thing(&mut scene, &reg, POUR, ThingId::ROOT).unwrap();
    scene.things.get_mut(&fill).unwrap().transform.position = Vec2::new(0.0, 0.5 * (top + 936.0));
    let main = &mut scene.card_mut(fill, SlotId::Main).unwrap().params;
    main.set("kind", Value::Choice("fill_at_the_start".into()));
    main.set("fill_size", Value::Vec2([2.0 * r, 936.0 - top]));
    let tap = bpe_kit::add_thing(&mut scene, &reg, POUR, ThingId::ROOT).unwrap();
    scene.things.get_mut(&tap).unwrap().transform.position = Vec2::new(-300.0, -600.0);
    let main = &mut scene.card_mut(tap, SlotId::Main).unwrap().params;
    main.set("speed", Value::Number(1600.0));
    main.set("aim", Value::Number(35.0));
    main.set("rate", Value::Number(2.0));
    main.set("width", Value::Number(40.0));
    let resolved = bpe_core::resolve(&scene, &reg);
    let make = MakeFinal::of(&scene, &resolved, 2.0);
    // (LOOK_TAKES=<folder> keeps the final there, so a look change draws again without making it again.)
    let kept = std::env::var_os("LOOK_TAKES").map(std::path::PathBuf::from);
    let finals = Finals::at(
        kept.clone().unwrap_or_else(|| std::env::temp_dir().join(format!("bpe-look-extras-{}", std::process::id()))),
    );
    if kept.is_none() {
        std::fs::remove_dir_all(finals.dir()).ok();
    }
    let water_gpu = bpe_water::WaterGpu::new(&gpu.device, &gpu.queue);
    let head = make.make(&water_gpu, &finals, &MakeProgress::default(), &AtomicBool::new(false)).unwrap();
    let dir = finals.take_dir(head.print);
    let layout = head.layout();
    let field = make.setup.field();
    let mut drawing = LookDraw::new(LookGpu::new(&gpu.device, &gpu.queue), &field, make.setup.numbers().wall_off);
    let mut stored = StoredDrops::new(&gpu.device, &gpu.queue, &[]);
    let mut wet = HeldWet::default();
    let base = bpe_kit::things::water::look::look_of(&scene, &resolved, 1.0);
    let spot = Lamp { kind: LampKind::Spot, place: Vec2::new(100.0, -900.0), spread: 90.0, ..Lamp::default() };
    let view = View::screen(1.0);
    let mut out = Vec::new();
    for (name, second) in
        [("water-extras-none", 2), ("water-extras-pool", 2), ("water-extras-sparks", 1), ("water-extras-wet", 2)]
    {
        let mut c = Chunk::open(&dir, second).unwrap();
        let frame = FrameCodec::new().decode(&c.bytes(0).unwrap(), &layout).unwrap();
        stored.set(&gpu.device, &gpu.queue, &frame.xv, &frame.kinds, &frame.mixes);
        let marks = std::sync::Arc::new(chunk::read_wet(&dir, second, layout.nodes()).unwrap());
        let mut look = base.clone();
        look.time = second as f32;
        look.lamps = vec![spot.clone()];
        look.light.ambient = 0.35;
        let l = &mut look.liquids[0];
        match name {
            "water-extras-pool" => {
                (l.foam, l.bubbles, l.mist, l.sparks) = (0.8, 0.8, 0.7, 0.0);
            }
            "water-extras-sparks" => {
                (l.foam, l.bubbles, l.mist, l.sparks) = (0.0, 0.0, 0.0, 1.0);
                look.back = BackLook::Colour([0.01, 0.01, 0.02]);
                look.light.ambient = 0.15;
            }
            "water-extras-wet" => look.rock.wet_marks = 1.0,
            _ => look.rock.wet_marks = 0.0,
        }
        wet.give(&gpu.device, &gpu.queue, layout, Some(&(marks, 60 * second)), &mut drawing);
        drawing.draw(&stored.buffers(), &view, &look);
        out.push((name.to_string(), drawing.read()));
    }
    eprintln!("water-extras: {} drops at {} px", head.drops, make.setup.cell);
    if kept.is_none() {
        std::fs::remove_dir_all(finals.dir()).ok();
    }
    out
}

/// Water mode part 6c's ready-made looks at full size (`LOOK_ONLY=water-looks`): one demo course (a carved chamber,
/// a pool a quarter full, a tap pouring onto a Ramp that spills into a Basin over the pool), made once at Coarse
/// detail for 3 s and drawn at 2 s from its take in each look, set with `apply_look` (`water-looks-<look>`).
/// `LOOK_SCENES=<folder>` also saves each look's scene file there, to render with `bpe-render-next --finals` and the
/// `LOOK_TAKES` folder (the looks share one final: a look never changes the physics).
fn water_looks_pictures(gpu: &Gpu, only: &Option<String>) -> Vec<(String, Vec<u8>)> {
    use bpe_kit::modules::water::{Keep, ReadyLook, apply_look};
    use bpe_kit::things::water::finals::Finals;
    use bpe_kit::things::water::make::{MakeFinal, MakeProgress};
    use bpe_kit::things::water::{LIQUID, PIECE, POUR, course_of, new_water_scene};
    use bpe_water::draw::wet::HeldWet;
    use bpe_water::draw::{LookDraw, LookGpu, StoredDrops, View};
    use bpe_water::take::{Chunk, FrameCodec, chunk};
    if !wanted("water-looks", only) && !only.as_deref().is_some_and(|o| o.contains("water-looks")) {
        return Vec::new();
    }
    let reg = kit().registry;
    let mut scene = new_water_scene(&reg, "Looks course");
    scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("duration", Value::Number(3.0));
    let water = scene.card_mut(ThingId::ROOT, SlotId::Water).unwrap();
    water.params.set("final_detail", Value::Choice("coarse".into()));
    let course = course_of(&scene).unwrap();
    let r = 540.0 - 24.0;
    let chamber = bpe_core::Stroke {
        points: vec![[0.0, -960.0 + 24.0 + r], [0.0, 960.0 - 24.0 - r]],
        size: r,
        strength: 1.0,
        square: true,
        ..Default::default()
    };
    let carved = Value::Strokes(bpe_core::Strokes { list: vec![chamber] });
    scene.card_mut(course, SlotId::Form).unwrap().params.set("carved", carved);
    bpe_kit::add_thing(&mut scene, &reg, LIQUID, ThingId::ROOT).unwrap();
    let top = 936.0 - 1872.0 / 4.0;
    let fill = bpe_kit::add_thing(&mut scene, &reg, POUR, ThingId::ROOT).unwrap();
    scene.things.get_mut(&fill).unwrap().transform.position = Vec2::new(0.0, 0.5 * (top + 936.0));
    let main = &mut scene.card_mut(fill, SlotId::Main).unwrap().params;
    main.set("kind", Value::Choice("fill_at_the_start".into()));
    main.set("fill_size", Value::Vec2([2.0 * r, 936.0 - top]));
    let tap = bpe_kit::add_thing(&mut scene, &reg, POUR, ThingId::ROOT).unwrap();
    scene.things.get_mut(&tap).unwrap().transform.position = Vec2::new(300.0, -830.0);
    let main = &mut scene.card_mut(tap, SlotId::Main).unwrap().params;
    main.set("rate", Value::Number(1.5));
    main.set("width", Value::Number(36.0));
    // A Ramp high at its right under the tap, and a Basin under its low end.
    for (kind, at, size) in
        [("ramp", Vec2::new(150.0, -330.0), [520.0, 170.0]), ("basin", Vec2::new(-210.0, 170.0), [340.0, 170.0])]
    {
        let piece = bpe_kit::add_thing(&mut scene, &reg, PIECE, ThingId::ROOT).unwrap();
        scene.things.get_mut(&piece).unwrap().transform.position = at;
        let form = &mut scene.card_mut(piece, SlotId::Form).unwrap().params;
        form.set("piece", Value::Choice(kind.into()));
        form.set("size", Value::Vec2(size));
        form.set("thickness", Value::Number(18.0));
    }
    let resolved = bpe_core::resolve(&scene, &reg);
    let make = MakeFinal::of(&scene, &resolved, 3.0);
    let kept = std::env::var_os("LOOK_TAKES").map(std::path::PathBuf::from);
    let finals = Finals::at(
        kept.clone().unwrap_or_else(|| std::env::temp_dir().join(format!("bpe-look-looks-{}", std::process::id()))),
    );
    if kept.is_none() {
        std::fs::remove_dir_all(finals.dir()).ok();
    }
    let water_gpu = bpe_water::WaterGpu::new(&gpu.device, &gpu.queue);
    let head = make.make(&water_gpu, &finals, &MakeProgress::default(), &AtomicBool::new(false)).unwrap();
    let dir = finals.take_dir(head.print);
    let layout = head.layout();
    let field = make.setup.field();
    let mut drawing = LookDraw::new(LookGpu::new(&gpu.device, &gpu.queue), &field, make.setup.numbers().wall_off);
    let mut stored = StoredDrops::new(&gpu.device, &gpu.queue, &[]);
    let second = 2;
    let mut c = Chunk::open(&dir, second).unwrap();
    let frame = FrameCodec::new().decode(&c.bytes(0).unwrap(), &layout).unwrap();
    stored.set(&gpu.device, &gpu.queue, &frame.xv, &frame.kinds, &frame.mixes);
    let marks = std::sync::Arc::new(chunk::read_wet(&dir, second, layout.nodes()).unwrap());
    let mut wet = HeldWet::default();
    wet.give(&gpu.device, &gpu.queue, layout, Some(&(marks, 60 * second)), &mut drawing);
    let scenes = std::env::var_os("LOOK_SCENES").map(std::path::PathBuf::from);
    let view = View::screen(1.0);
    let mut out = Vec::new();
    for look in ReadyLook::ALL {
        let slug = look.title().to_lowercase().replace(' ', "-");
        let name = format!("water-looks-{slug}");
        if !wanted(&name, only) {
            continue;
        }
        let mut scene = scene.clone();
        apply_look(&mut scene, &reg, look, Keep::default());
        if let Some(scenes) = &scenes {
            std::fs::create_dir_all(scenes).unwrap();
            std::fs::write(scenes.join(format!("{name}.json")), bpe_core::save::save(&scene)).unwrap();
        }
        let resolved = bpe_core::resolve(&scene, &reg);
        let l = bpe_kit::things::water::look::look_of(&scene, &resolved, second as f64);
        drawing.draw(&stored.buffers(), &view, &l);
        out.push((name, drawing.read()));
    }
    // Part 6e: Glass lab with a Things mask on the Liquids grown round them, a stop and a half darker (the
    // mask's edge, round the drops and the stream, at full size), and with one on the Back.
    for (name, choice) in [("water-looks-masked-liquids", "liquids"), ("water-looks-masked-back", "back")] {
        use bpe_kit::modules::grade;
        if !wanted(name, only) {
            continue;
        }
        let mut scene = scene.clone();
        apply_look(&mut scene, &reg, ReadyLook::GlassLab, Keep::default());
        let mask = grade::add_mask(&mut scene, &reg, grade::THINGS).unwrap();
        let card = scene.card(ThingId::ROOT, SlotId::Grade).unwrap();
        let (part, basic) = (grade::parts_of(card, mask)[0].id, grade::panels_of(card, mask)[0].id);
        let params = &mut scene.card_mut(ThingId::ROOT, SlotId::Grade).unwrap().params;
        params.set(&bpe_core::layered_key(part, "only"), Value::Choice(choice.into()));
        params.set(&bpe_core::layered_key(part, "expand"), Value::Number(24.0));
        params.set(&bpe_core::layered_key(part, "feather"), Value::Number(24.0));
        params.set(&bpe_core::layered_key(basic, "exposure"), Value::Number(-1.5));
        let resolved = bpe_core::resolve(&scene, &reg);
        let l = bpe_kit::things::water::look::look_of(&scene, &resolved, second as f64);
        drawing.draw(&stored.buffers(), &view, &l);
        out.push((name.to_string(), drawing.read()));
    }
    eprintln!("water-looks: {} drops at {} px, take {}", head.drops, make.setup.cell, dir.display());
    if kept.is_none() {
        std::fs::remove_dir_all(finals.dir()).ok();
    }
    out
}

/// Water mode part 4.3a's ready-made liquids at full size (`LOOK_ONLY=water-liquids`): nine Basins, three rows of
/// three, each with a lump of one ready liquid standing in it at the start (Water, Oil, Honey; Syrup, Slime, Milk;
/// Paint, Mercury, Ferrofluid), made at Coarse detail for 3 s and drawn at 2 s from its take in the Plain look.
fn water_liquids_pictures(gpu: &Gpu, only: &Option<String>) -> Vec<(String, Vec<u8>)> {
    use bpe_kit::modules::water::{ReadyLiquid, apply_liquid};
    use bpe_kit::things::water::finals::Finals;
    use bpe_kit::things::water::make::{MakeFinal, MakeProgress};
    use bpe_kit::things::water::{LIQUID, PIECE, POUR, course_of, new_water_scene};
    use bpe_water::draw::wet::HeldWet;
    use bpe_water::draw::{LookDraw, LookGpu, StoredDrops, View};
    use bpe_water::take::{Chunk, FrameCodec, chunk};
    let name = "water-liquids";
    if !wanted(name, only) {
        return Vec::new();
    }
    let reg = kit().registry;
    let mut scene = new_water_scene(&reg, "Liquids");
    scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("duration", Value::Number(3.0));
    let water = scene.card_mut(ThingId::ROOT, SlotId::Water).unwrap();
    water.params.set("final_detail", Value::Choice("coarse".into()));
    let course = course_of(&scene).unwrap();
    let r = 540.0 - 24.0;
    let chamber = bpe_core::Stroke {
        points: vec![[0.0, -960.0 + 24.0 + r], [0.0, 960.0 - 24.0 - r]],
        size: r,
        strength: 1.0,
        square: true,
        ..Default::default()
    };
    let carved = Value::Strokes(bpe_core::Strokes { list: vec![chamber] });
    scene.card_mut(course, SlotId::Form).unwrap().params.set("carved", carved);
    for (k, liquid) in ReadyLiquid::ALL.into_iter().enumerate() {
        let at = Vec2::new(-330.0 + 330.0 * (k % 3) as f32, -600.0 + 600.0 * (k / 3) as f32);
        let id = bpe_kit::add_thing(&mut scene, &reg, LIQUID, ThingId::ROOT).unwrap();
        assert!(apply_liquid(&mut scene, &reg, id, liquid));
        let basin = bpe_kit::add_thing(&mut scene, &reg, PIECE, ThingId::ROOT).unwrap();
        scene.things.get_mut(&basin).unwrap().transform.position = at;
        let form = &mut scene.card_mut(basin, SlotId::Form).unwrap().params;
        form.set("piece", Value::Choice("basin".into()));
        form.set("size", Value::Vec2([240.0, 300.0]));
        form.set("thickness", Value::Number(18.0));
        // A lump standing on the basin's floor, narrower than the basin, that slumps out to its walls (one
        // dropped from higher splashes out of the basins: a flat lump meeting a flat floor throws jets at several
        // times the speed it lands at).
        let lump = bpe_kit::add_thing(&mut scene, &reg, POUR, ThingId::ROOT).unwrap();
        scene.things.get_mut(&lump).unwrap().transform.position = at + Vec2::new(0.0, 150.0 - 18.0 - 60.0);
        let main = &mut scene.card_mut(lump, SlotId::Main).unwrap().params;
        main.set("kind", Value::Choice("fill_at_the_start".into()));
        main.set("fill_size", Value::Vec2([140.0, 120.0]));
        main.set("liquid", Value::Target(bpe_core::Target::Thing(id)));
    }
    let resolved = bpe_core::resolve(&scene, &reg);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    let make = MakeFinal::of(&scene, &resolved, 3.0);
    let finals = Finals::at(std::env::temp_dir().join(format!("bpe-look-liquids-{}", std::process::id())));
    std::fs::remove_dir_all(finals.dir()).ok();
    let water_gpu = bpe_water::WaterGpu::new(&gpu.device, &gpu.queue);
    let head = make.make(&water_gpu, &finals, &MakeProgress::default(), &AtomicBool::new(false)).unwrap();
    let dir = finals.take_dir(head.print);
    let layout = head.layout();
    let field = make.setup.field();
    let mut drawing = LookDraw::new(LookGpu::new(&gpu.device, &gpu.queue), &field, make.setup.numbers().wall_off);
    let mut stored = StoredDrops::new(&gpu.device, &gpu.queue, &[]);
    let second = 2;
    let mut c = Chunk::open(&dir, second).unwrap();
    let frame = FrameCodec::new().decode(&c.bytes(0).unwrap(), &layout).unwrap();
    stored.set(&gpu.device, &gpu.queue, &frame.xv, &frame.kinds, &frame.mixes);
    let marks = std::sync::Arc::new(chunk::read_wet(&dir, second, layout.nodes()).unwrap());
    let mut wet = HeldWet::default();
    wet.give(&gpu.device, &gpu.queue, layout, Some(&(marks, 60 * second)), &mut drawing);
    let l = bpe_kit::things::water::look::look_of(&scene, &resolved, second as f64);
    drawing.draw(&stored.buffers(), &View::screen(1.0), &l);
    eprintln!("water-liquids: {} drops at {} px", head.drops, make.setup.cell);
    std::fs::remove_dir_all(finals.dir()).ok();
    vec![(name.to_string(), drawing.read())]
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
    // A Huge swarm moves on the graphics card named for this thread (One to a million's sparks).
    let _card = bpe_gfx::swarm::use_for_now(Some(bpe_gfx::swarm::SwarmGpu::for_device(&gpu.device, &gpu.queue)));

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
        ("links", link_shots(&reg)),
        ("riding", riding_shots(&reg)),
        ("library", library_shots(&reg)),
        ("file", file_shots(&reg)),
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
    let mut thumbs = Vec::new();
    let water = water_pictures(&gpu, &only).into_iter().chain(water_look_pictures(&gpu, &only));
    let finals = water_final_pictures(&gpu, &only)
        .into_iter()
        .chain(water_extras_pictures(&gpu, &only))
        .chain(water_looks_pictures(&gpu, &only))
        .chain(water_liquids_pictures(&gpu, &only));
    for (name, pixels) in water.chain(finals) {
        thumbs.push((name.clone(), save_all(&dir, &name, &pixels)));
        count += 1;
    }
    save_sheet(&dir, "water", &thumbs);
    eprintln!("{count} pictures in {}", dir.display());
    assert!(count > 0, "LOOK_ONLY matched no picture");
}

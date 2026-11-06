//! Gradients part 2 in pictures, on the software GPU: every gradient form drawn across the
//! background (checked against the editor's own CPU layout, `area_colour`, at points over the
//! picture), inside a ball (a Pattern layer) and on a shape (a Colour and gradient layer), each
//! of those checked to change the picture from Straight's.
//!
//! `BPE_SAVE_RENDERS=1` saves the pictures into `target/pictures/form-*.png`.

use bpe_core::gradient_form::{Layout, area_colour};
use bpe_core::{Choice, Color, Gradient, GradientForm, Mix, Scene, SettingValue, SlotId, ThingId, Value, layered_key};
use bpe_gfx::{Frame, Renderer, build_frame, headless_device, wgpu};
use bpe_kit::{Kit, kit, new_scene};
use bpe_runtime::Playback;
use glam::Vec2;

const W: u32 = 270;
const H: u32 = 480;
/// The video's size, in the screen units the background measures in.
const VIDEO: Vec2 = Vec2::new(1080.0, 1920.0);
const SHAPE: ThingId = ThingId(1);
const BALL: ThingId = ThingId(2);

fn colours() -> Gradient {
    Gradient::spread(&[Color::rgb(220, 40, 40), Color::rgb(40, 200, 60), Color::rgb(40, 60, 220)], Mix::Light, false)
}

fn draw(device: &wgpu::Device, queue: &wgpu::Queue, renderer: &mut Renderer, kit: &Kit, scene: &Scene) -> Vec<u8> {
    let resolved = bpe_kit::resolve_scene(scene, &kit.registry);
    let mut playback = Playback::new(scene, &resolved, kit.sims.clone());
    playback.seek(0.0);
    let mut frame = Frame::default();
    build_frame(scene, &resolved, playback.world(), playback.log(), 0.0, &kit.draws, &mut frame);
    renderer.render(device, queue, &frame);
    renderer.read_output(device, queue).unwrap()
}

fn save(name: &str, pixels: &[u8]) {
    if std::env::var_os("BPE_SAVE_RENDERS").is_none() {
        return;
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pictures");
    std::fs::create_dir_all(&dir).unwrap();
    let file = std::fs::File::create(dir.join(format!("form-{name}.png"))).unwrap();
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), W, H);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(pixels).unwrap();
}

fn px(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    let i = ((y * W + x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2]]
}

fn off(a: [u8; 3], b: [u8; 3]) -> u8 {
    (0..3).map(|i| a[i].abs_diff(b[i])).max().unwrap()
}

/// The mean difference, per channel out of 255, between two pictures inside `area`
/// (x0, y0, x1, y1 in pixels).
fn differ(a: &[u8], b: &[u8], area: (u32, u32, u32, u32)) -> f32 {
    let (mut sum, mut n) = (0.0, 0.0);
    for y in area.1..area.3 {
        for x in area.0..area.2 {
            let (p, q) = (px(a, x, y), px(b, x, y));
            sum += (0..3).map(|i| p[i].abs_diff(q[i]) as f32).sum::<f32>() / 3.0;
            n += 1.0;
        }
    }
    sum / n
}

/// How much the picture changes from pixel to pixel inside `area`: 0 for one flat colour.
fn spread(a: &[u8], area: (u32, u32, u32, u32)) -> f32 {
    let first = px(a, area.0, area.1);
    let mut most = 0u8;
    for y in (area.1..area.3).step_by(3) {
        for x in (area.0..area.2).step_by(3) {
            most = most.max(off(px(a, x, y), first));
        }
    }
    most as f32
}

/// The new scene with its ball and shape off, no bloom, and the background laid out by `form`
/// over the place.
fn background(kit: &Kit, form: GradientForm) -> Scene {
    let mut scene = new_scene(&kit.registry);
    scene.things.get_mut(&SHAPE).unwrap().enabled = false;
    scene.things.get_mut(&BALL).unwrap().enabled = false;
    let root = &mut scene.things.get_mut(&ThingId::ROOT).unwrap().params;
    root.set("bloom", Value::Number(0.0));
    root.set("background_follow", Value::Choice("place".into()));
    root.set("background_angle", Value::Number(30.0));
    root.set("background_span", Value::Number(500.0));
    root.set("background_colours", Value::Gradient(colours()));
    root.set("background_colours~form", form.to_value());
    scene
}

/// Where the ball and the shape sit in the picture: round the middle.
const MIDDLE: (u32, u32, u32, u32) = (W / 2 - 50, H / 2 - 50, W / 2 + 50, H / 2 + 50);

/// The new scene with a big still ball in the middle wearing a Pattern laid out by `form`.
fn ball(kit: &Kit, form: GradientForm) -> Scene {
    let reg = &kit.registry;
    let mut scene = new_scene(reg);
    // Ripples run out from the other things, so the shape stays round the ball for them.
    scene.things.get_mut(&SHAPE).unwrap().enabled = form == GradientForm::Ripples;
    scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("bloom", Value::Number(0.0));
    let b = scene.things.get_mut(&BALL).unwrap();
    b.transform.position = Vec2::ZERO;
    b.params.set("radius", Value::Number(300.0));
    b.params.set("velocity", Value::Vec2([0.0, 0.0]));
    let layers = &scene.card(BALL, SlotId::Look).unwrap().layers;
    let glow = layers.iter().find(|l| l.kind == "layer.glow").map(|l| l.id);
    if let Some(glow) = glow {
        scene.remove_layer(reg, BALL, SlotId::Look, glow).unwrap();
    }
    let pattern = scene.add_layer(reg, BALL, SlotId::Look, "layer.pattern").unwrap();
    scene.move_layer(reg, BALL, SlotId::Look, pattern, 1).unwrap();
    let p = &mut scene.card_mut(BALL, SlotId::Look).unwrap().params;
    p.set(&layered_key(pattern, "in_pattern"), Value::Choice("gradient".into()));
    p.set(&layered_key(pattern, "in_from"), Value::Choice("own_gradient".into()));
    p.set(&layered_key(pattern, "in_colours"), Value::Gradient(colours()));
    p.set(&layered_key(pattern, "in_colours~form"), form.to_value());
    scene
}

/// The new scene with its ball off and the shape filled, painted by a Colour and gradient layer
/// laid out by `form`.
fn shape(kit: &Kit, form: GradientForm) -> Scene {
    let reg = &kit.registry;
    let mut scene = new_scene(reg);
    // Ripples run out from the other things (never a thing's own outline): the ball stays.
    scene.things.get_mut(&BALL).unwrap().enabled = form == GradientForm::Ripples;
    let s = scene.things.get_mut(&SHAPE).unwrap();
    s.params.set("fill", Value::Choice("filled".into()));
    scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("bloom", Value::Number(0.0));
    let paint = scene.add_layer(reg, SHAPE, SlotId::Look, bpe_kit::modules::look::shape::PAINT).unwrap();
    let p = &mut scene.card_mut(SHAPE, SlotId::Look).unwrap().params;
    p.set(&layered_key(paint, "colours"), Value::Gradient(colours()));
    p.set(&layered_key(paint, "colours~form"), form.to_value());
    scene
}

#[test]
fn every_form_draws_on_the_background_balls_and_shapes() {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    let kit = kit();
    let mut renderer = Renderer::new(&device, W, H);
    let forms = GradientForm::ALL;

    // The background, against the CPU's layout at a grid of points. A point on a sharp edge of
    // a form (a star's point, a kaleidoscope's seam) may land a pixel off, so a few may miss.
    for form in forms.into_iter().filter(|f| *f != GradientForm::Ripples) {
        let pixels = draw(&device, &queue, &mut renderer, &kit, &background(&kit, form));
        save(&format!("background-{}", form.name()), &pixels);
        let layout = Layout { turn: Some(30.0), centre: Some([0.0; 2]), size: Some(1.0), ..Layout::default() };
        let g = colours();
        let mut misses = Vec::new();
        let mut points = 0;
        for j in 0..9 {
            for i in 0..6 {
                let (x, y) = (12 + i * (W - 24) / 5, 12 + j * (H - 24) / 8);
                let screen = (Vec2::new(x as f32 + 0.5, y as f32 + 0.5) / Vec2::new(W as f32, H as f32) - 0.5) * VIDEO;
                // The background's Straight runs once across a Span (an area's runs across two
                // units); every other form measures in Spans from the centre.
                let units = if form == GradientForm::Straight { 2.0 } else { 1.0 };
                let want = Color::from_linear(area_colour(&g, form, &layout, units * screen / 500.0, 0.0));
                let drawn = px(&pixels, x, y);
                points += 1;
                if off(drawn, [want.r, want.g, want.b]) > 14 {
                    misses.push((x, y, drawn, [want.r, want.g, want.b]));
                }
            }
        }
        assert!(
            misses.len() * 10 <= points,
            "{form:?}: {} of {points} points off the CPU layout: {misses:?}",
            misses.len()
        );
    }

    // Inside a ball and on a shape: each form changes the picture from Straight's.
    let sites: [(&str, fn(&Kit, GradientForm) -> Scene); 2] = [("ball", ball), ("shape", shape)];
    for (site, make) in sites {
        let straight = draw(&device, &queue, &mut renderer, &kit, &make(&kit, GradientForm::Straight));
        save(&format!("{site}-straight"), &straight);
        assert!(spread(&straight, MIDDLE) > 30.0, "{site}: Straight runs through the colours");
        for form in forms.into_iter().filter(|f| *f != GradientForm::Straight) {
            let pixels = draw(&device, &queue, &mut renderer, &kit, &make(&kit, form));
            save(&format!("{site}-{}", form.name()), &pixels);
            let changed = differ(&pixels, &straight, MIDDLE);
            assert!(changed > 4.0, "{site}: {form:?} draws like Straight (off by {changed:.1} on average)");
            assert!(spread(&pixels, MIDDLE) > 10.0, "{site}: {form:?} draws one flat colour");
        }
    }
}

//! Gradients part 1 in pictures, on the software GPU: each Mix drawn across the background,
//! Loop wrapping it round with no cut, Bands cutting it into flat bands and Grain dithering it.
//! The checks compare pixels with each other and with the gradient's own sampling (within a few
//! steps: the picture's highlights roll off a little near full brightness), so they hold on any
//! GPU.
//!
//! `BPE_SAVE_RENDERS=1` saves the pictures into `target/pictures/gradient-*.png`.

use bpe_core::{Color, Gradient, Mix, Scene, ThingId, Value};
use bpe_gfx::{Frame, Renderer, build_frame, headless_device, wgpu};
use bpe_kit::{Kit, kit, new_scene};
use bpe_runtime::Playback;

const W: u32 = 270;
const H: u32 = 480;
const RED: Color = Color { r: 255, g: 0, b: 0, a: 255 };
const BLUE: Color = Color { r: 0, g: 0, b: 255, a: 255 };

fn draw(device: &wgpu::Device, queue: &wgpu::Queue, kit: &Kit, scene: &Scene, name: &str) -> Vec<u8> {
    let resolved = bpe_kit::resolve_scene(scene, &kit.registry);
    let mut playback = Playback::new(scene, &resolved, kit.sims.clone());
    playback.seek(0.0);
    let mut frame = Frame::default();
    build_frame(scene, &resolved, playback.world(), playback.log(), 0.0, &kit.draws, &mut frame);
    let mut renderer = Renderer::new(device, W, H);
    renderer.render(device, queue, &frame);
    let pixels = renderer.read_output(device, queue).unwrap();
    if std::env::var_os("BPE_SAVE_RENDERS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pictures");
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(dir.join(format!("gradient-{name}.png"))).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), W, H);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(&pixels).unwrap();
    }
    pixels
}

/// The new scene with nothing in it but `colours` across the background, left to right.
fn across(kit: &Kit, colours: Gradient) -> Scene {
    let mut scene = new_scene(&kit.registry);
    scene.things.get_mut(&ThingId(1)).unwrap().enabled = false;
    scene.things.get_mut(&ThingId(2)).unwrap().enabled = false;
    set(&mut scene, "bloom", Value::Number(0.0));
    set(&mut scene, "background_angle", Value::Number(0.0));
    set(&mut scene, "background_span", Value::Number(1080.0));
    set(&mut scene, "background_colours", Value::Gradient(colours));
    scene
}

fn set(scene: &mut Scene, key: &str, value: Value) {
    scene.things.get_mut(&ThingId::ROOT).unwrap().params.set(key, value);
}

fn px(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    let i = ((y * W + x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2]]
}

/// How far a drawn colour may sit from the gradient's own sampling: the output rolls bright
/// channels off a little (255 draws as about 246).
const CLOSE: u8 = 12;

/// What the gradient's own sampling gives at column `x`, as 8-bit sRGB.
fn sampled(g: &Gradient, x: u32) -> [u8; 3] {
    let c = Color::from_linear(g.sample((x as f32 + 0.5) / W as f32));
    [c.r, c.g, c.b]
}

fn off(a: [u8; 3], b: [u8; 3]) -> u8 {
    (0..3).map(|i| a[i].abs_diff(b[i])).max().unwrap()
}

fn luma(p: [u8; 3]) -> f32 {
    0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32
}

#[test]
fn each_mix_draws_as_the_gradient_samples() {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    let kit = kit();
    let mut middles = Vec::new();
    for mix in [Mix::Light, Mix::Plain, Mix::Even, Mix::RoundTheWheel, Mix::LongWayRound] {
        let g = Gradient::spread(&[RED, BLUE], mix, false);
        let pixels = draw(&device, &queue, &kit, &across(&kit, g.clone()), &format!("{mix:?}"));
        for x in [10, W / 4, W / 2, 3 * W / 4, W - 10] {
            let (drawn, want) = (px(&pixels, x, H / 2), sampled(&g, x));
            assert!(off(drawn, want) <= CLOSE, "{mix:?} at column {x}: drawn {drawn:?}, sampled {want:?}");
        }
        middles.push((mix, px(&pixels, W / 2, H / 2)));
    }
    let middle = |m: Mix| middles.iter().find(|(x, _)| *x == m).unwrap().1;
    assert!(luma(middle(Mix::Even)) > luma(middle(Mix::Light)), "Even doesn't dip dark: {middles:?}");
    let long = middle(Mix::LongWayRound);
    assert!(long[1] > long[0] && long[1] > long[2], "the long way round from red to blue is through green: {long:?}");
    let short = middle(Mix::RoundTheWheel);
    assert!(short[1] < 60 && short[0] > 100 && short[2] > 100, "the short way is through purple: {short:?}");
}

#[test]
fn a_looped_gradient_has_no_cut_where_it_wraps() {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    let kit = kit();
    let plain = draw(&device, &queue, &kit, &across(&kit, Gradient::spread(&[RED, BLUE], Mix::Light, false)), "cut");
    let (left, right) = (px(&plain, 1, H / 2), px(&plain, W - 2, H / 2));
    assert!(off(left, right) > 200, "unlooped, the ends are red and blue: {left:?} {right:?}");
    let looped = Gradient::spread(&[RED, BLUE], Mix::Light, true);
    let pixels = draw(&device, &queue, &kit, &across(&kit, looped.clone()), "looped");
    let (left, right) = (px(&pixels, 0, H / 2), px(&pixels, W - 1, H / 2));
    assert!(off(left, right) <= 8, "looped, the two ends meet: {left:?} {right:?}");
    for x in [0, W / 4, W / 2, 3 * W / 4, W - 1] {
        assert!(off(px(&pixels, x, H / 2), sampled(&looped, x)) <= CLOSE, "column {x}");
    }
}

#[test]
fn bands_cut_the_background_into_flat_colours() {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    let kit = kit();
    let g = Gradient::spread(&[RED, BLUE], Mix::Even, false);
    let mut scene = across(&kit, g.clone());
    set(&mut scene, "background_colours~bands", Value::Number(5.0));
    let pixels = draw(&device, &queue, &kit, &scene, "bands");
    // Runs of one colour along the middle row; a pixel on a band's edge may blend the two.
    let row: Vec<[u8; 3]> = (0..W).map(|x| px(&pixels, x, H / 2)).collect();
    let mut runs: Vec<([u8; 3], u32)> = Vec::new();
    for p in row {
        match runs.last_mut() {
            Some((c, n)) if off(*c, p) <= 1 => *n += 1,
            _ => runs.push((p, 1)),
        }
    }
    let bands: Vec<[u8; 3]> = runs.iter().filter(|(_, n)| *n >= 8).map(|(c, _)| *c).collect();
    assert_eq!(bands.len(), 5, "five flat bands: {runs:?}");
    for (k, band) in bands.iter().enumerate() {
        let c = Color::from_linear(g.sample((k as f32 + 0.5) / 5.0));
        assert!(off(*band, [c.r, c.g, c.b]) <= CLOSE, "band {k} is the colour at its middle: {band:?} vs {c:?}");
    }
}

#[test]
fn grain_dithers_without_shifting_the_colours() {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    let kit = kit();
    let dark = Gradient::spread(&[Color::rgb(10, 12, 30), Color::rgb(30, 20, 60)], Mix::Even, false);
    let scene = across(&kit, dark.clone());
    let smooth = draw(&device, &queue, &kit, &scene, "no-grain");
    let mut grainy = scene.clone();
    set(&mut grainy, "background_colours~grain", Value::Number(4.0));
    let grained = draw(&device, &queue, &kit, &grainy, "grain");
    // Neighbouring pixels of one column (where the gradient doesn't change) differ by much more
    // with grain than the picture's own fine dither.
    let wobble = |p: &[u8]| {
        (0..W)
            .step_by(9)
            .map(|x| (100..300).map(|y| off(px(p, x, y), px(p, x, y + 1)) as u32).sum::<u32>())
            .sum::<u32>()
    };
    let (still, noisy) = (wobble(&smooth), wobble(&grained));
    assert!(noisy > still * 2, "grain: neighbours differ {noisy} vs {still} without");
    // On average it is the same picture.
    let mean = |p: &[u8]| {
        let mut sum = [0f64; 3];
        for y in 0..H {
            for x in 0..W {
                let c = px(p, x, y);
                for i in 0..3 {
                    sum[i] += c[i] as f64;
                }
            }
        }
        sum.map(|s| s / (W * H) as f64)
    };
    let (a, b) = (mean(&smooth), mean(&grained));
    for i in 0..3 {
        assert!((a[i] - b[i]).abs() <= 1.0, "channel {i}: {a:?} vs {b:?}");
    }
}

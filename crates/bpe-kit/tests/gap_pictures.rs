//! Gaps in a shape, in pictures: the band is cut where the outline is open, with round ends, and
//! its layers (a glow) follow the cut. Measured on the cloud's software GPU; the checks compare
//! pixels with each other, so they hold on any GPU.
//!
//! `BPE_SAVE_RENDERS=1` saves the pictures into `target/pictures/gap-*.png`.

use bpe_core::{Scene, Settings, ThingId};
use bpe_gfx::{Frame, Renderer, build_frame, test_device};
use bpe_kit::{ShapeSettings, add_thing, kit, new_studio_scene};
use bpe_runtime::Playback;

const W: u32 = 270;
const H: u32 = 480;

fn scene_with(settings: ShapeSettings, look: Option<&str>) -> Scene {
    let kit = kit();
    let mut scene = new_studio_scene(&kit.registry);
    let shape = add_thing(&mut scene, &kit.registry, "shape", ThingId::ROOT).unwrap();
    let t = scene.things.get_mut(&shape).unwrap();
    t.params = settings.to_params();
    t.transform.position = glam::Vec2::ZERO;
    if let Some(look) = look {
        let ready = bpe_kit::ready_looks::find(look).unwrap();
        bpe_kit::ready_looks::apply(
            &mut scene,
            &kit.registry,
            shape,
            ready,
            bpe_kit::ready_looks::Colours::Look,
            bpe_kit::ready_looks::How::Replace,
        )
        .unwrap();
    }
    scene
}

fn draw(scene: &Scene, name: &str) -> Vec<u8> {
    let (device, queue) = test_device().expect("a graphics device");
    let mut renderer = Renderer::new(&device, W, H);
    let kit = kit();
    let resolved = bpe_core::resolve(scene, &kit.registry);
    let mut playback = Playback::new(scene, &resolved, kit.sims.clone());
    playback.seek(0.0);
    let mut frame = Frame::default();
    build_frame(scene, &resolved, playback.world(), playback.log(), 0.0, &kit.draws, &mut frame);
    renderer.render(&device, &queue, &frame);
    if renderer.programs_waiting() {
        renderer.render(&device, &queue, &frame);
    }
    let pixels = renderer.read_output(&device, &queue).unwrap();
    if std::env::var_os("BPE_SAVE_RENDERS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pictures");
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(dir.join(format!("gap-{name}.png"))).unwrap();
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

fn off(p: [u8; 4], from: [u8; 4]) -> u32 {
    (0..3).map(|k| (p[k] as i32 - from[k] as i32).unsigned_abs()).sum()
}

/// The band is drawn at `closed` and cut at `open` (both on the outline): the open point looks like
/// the background far from the shape, the closed one doesn't.
fn check(pixels: &[u8], open: (f32, f32), closed: (f32, f32), what: &str) {
    let background = at(pixels, -500.0, -900.0);
    let o = at(pixels, open.0, open.1);
    let c = at(pixels, closed.0, closed.1);
    assert!(off(c, background) > 60, "{what}: the closed band is drawn ({c:?} on {background:?})");
    assert!(off(o, background) < 12, "{what}: the opening shows what is behind ({o:?} on {background:?})");
}

#[test]
fn a_gap_cuts_the_band_of_every_outline() {
    let thick = |s: ShapeSettings| ShapeSettings { thickness: 30.0, gaps: 1, gap_size: 0.15, ..s };
    let ring = ShapeSettings { gap_at: 0.25, ..thick(ShapeSettings::circle(400.0, 30.0)) };
    check(&draw(&scene_with(ring.clone(), None), "ring"), (400.0, 0.0), (-400.0, 0.0), "ring");
    // A closed ring is drawn there.
    let closed = draw(&scene_with(ShapeSettings { gaps: 0, ..ring }, None), "closed");
    let background = at(&closed, -500.0, -900.0);
    assert!(off(at(&closed, 400.0, 0.0), background) > 60, "a ring with no gaps is whole");
    // A flat-topped hexagon: the middle of its bottom side is 3.5 sides round from the top left corner.
    let apothem = 400.0 * (std::f32::consts::PI / 6.0).cos();
    let hexagon = ShapeSettings { gap_at: 3.5 / 6.0, ..thick(ShapeSettings::polygon(6, 400.0, 30.0)) };
    check(&draw(&scene_with(hexagon, None), "hexagon"), (0.0, apothem), (0.0, -apothem), "hexagon");
    let oval = ShapeSettings { gap_at: 0.25, ..thick(ShapeSettings::ellipse(700.0, 1100.0, 30.0)) };
    check(&draw(&scene_with(oval, None), "ellipse"), (350.0, 0.0), (-350.0, 0.0), "ellipse");
    let rect = ShapeSettings { gap_at: 0.125, ..thick(ShapeSettings::rectangle(700.0, 700.0, 30.0)) };
    check(&draw(&scene_with(rect, None), "box"), (0.0, -350.0), (0.0, 350.0), "box");
}

#[test]
fn a_glowing_ring_glows_less_at_its_gap() {
    let ring =
        ShapeSettings { gaps: 1, gap_size: 0.15, gap_at: 0.25, thickness: 20.0, ..ShapeSettings::circle(400.0, 20.0) };
    let pixels = draw(&scene_with(ring, Some("shape.glow_ring")), "glow");
    let background = at(&pixels, -500.0, -900.0);
    let open = at(&pixels, 400.0, 0.0);
    let closed = at(&pixels, -400.0, 0.0);
    assert!(off(closed, background) > off(open, background) + 60, "{open:?} {closed:?} on {background:?}");
}

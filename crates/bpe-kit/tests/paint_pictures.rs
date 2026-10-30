//! Picture tests for the balls' Colours paint on the software GPU.

use bpe_core::{Color, Gradient, SlotId, ThingId, Value};
use bpe_gfx::{Frame, Renderer, build_frame, headless_device};
use bpe_kit::{flow_source, kit, scenes};
use bpe_runtime::Playback;

const W: u32 = 270;
const H: u32 = 480;

/// The RGBA pixels of a PNG in tests/data.
fn saved(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name);
    let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&path).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    assert_eq!((info.width, info.height, info.color_type), (W, H, png::ColorType::Rgba));
    pixels.truncate(info.buffer_size());
    pixels
}

#[test]
fn solid_paint_matches_plain_colour() {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    let kit = kit();
    let (_, scene) = scenes::all(&kit.registry).into_iter().find(|(name, _)| name == "One ball").unwrap();
    let resolved = bpe_core::resolve(&scene, &kit.registry);
    let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
    playback.seek(2.0);
    let mut frame = Frame::default();
    build_frame(&scene, &resolved, playback.world(), playback.log(), 2.0, &kit.draws, &mut frame);
    let mut renderer = Renderer::new(&device, W, H);
    let source = flow_source(&scene, &resolved, &kit.sims, None);
    assert!(renderer.render_with_flow(&device, &queue, &frame, &source).caught_up);
    let pixels = renderer.read_output(&device, &queue).unwrap();
    // Drawn by the commit before the ball's colour became a paint (one-stop Colours).
    let before = saved("one-ball.png");
    let differ = pixels.iter().zip(&before).filter(|(a, b)| a != b).count();
    assert_eq!(differ, 0, "{differ} bytes differ from the picture before Colours");
}

#[test]
fn inside_rings_draw_on_classic_balls() {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    let kit = kit();
    let mut scene = bpe_kit::new_scene(&kit.registry);
    let ball = scene.things.get_mut(&ThingId(2)).unwrap();
    ball.transform.position = glam::Vec2::ZERO;
    ball.params.set("radius", Value::Number(300.0));
    ball.params.set("velocity", Value::Vec2([0.0, 0.0]));
    let m = bpe_core::resolve_slot(&scene, &kit.registry, ThingId(2), SlotId::Look, &mut Vec::new()).unwrap().module;
    let params = &mut scene.library.modules.get_mut(&m).unwrap().params;
    let (red, blue) = (Color::rgb(255, 0, 0), Color::rgb(0, 0, 255));
    params.set("colours", Value::Gradient(Gradient::even(&[red, blue])));
    params.set("glow", Value::Number(0.0));
    let resolved = bpe_core::resolve(&scene, &kit.registry);
    let playback = Playback::new(&scene, &resolved, kit.sims.clone());
    let mut renderer = Renderer::new(&device, W, H);
    let mut draw = |scene: &bpe_core::Scene| {
        let resolved = bpe_core::resolve(scene, &kit.registry);
        let mut frame = Frame { bloom: 0.0, ..Frame::default() };
        build_frame(scene, &resolved, playback.world(), playback.log(), 0.0, &kit.draws, &mut frame);
        renderer.render(&device, &queue, &frame);
        renderer.read_output(&device, &queue).unwrap()
    };
    // The pixel `r` ball radii right of the middle.
    let at = |pixels: &[u8], r: f32| {
        let x = (W as f32 / 2.0 + r * 300.0 / 4.0) as u32;
        let i = ((H / 2 * W + x) * 4) as usize;
        [pixels[i] as i32, pixels[i + 1] as i32, pixels[i + 2] as i32]
    };
    // Without a pattern the ball is one colour: the gradient at its place, the middle of the
    // screen, half way from red to blue.
    let plain = draw(&scene);
    let (middle, edge) = (at(&plain, 0.1), at(&plain, 0.85));
    assert!(middle[0] > 40 && middle[2] > 40, "{middle:?}");
    assert!(edge[0] > 20 && edge[2] > 20, "{edge:?}: the same colour, lit darker");

    let params = &mut scene.library.modules.get_mut(&m).unwrap().params;
    params.set("inside", Value::Choice("rings".into()));
    // Starting from red at the middle whatever the ball's place.
    params.set("follow", Value::Choice("order".into()));
    let rings = draw(&scene);
    if std::env::var_os("BPE_SAVE_RENDERS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pictures");
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(dir.join("inside-rings.png")).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), W, H);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(&rings).unwrap();
    }
    let (middle, edge) = (at(&rings, 0.1), at(&rings, 0.85));
    // The light stays on top, so the colours are compared, not matched.
    assert!(middle[0] > middle[2] + 80, "{middle:?}: red at the middle");
    assert!(edge[2] > edge[0] + 30, "{edge:?}: blue towards the edge");
    let (plain_middle, plain_edge) = (at(&plain, 0.1), at(&plain, 0.85));
    assert!(middle[0] > plain_middle[0] && edge[2] > plain_edge[2], "the rings change the ball's colours");
}

//! Picture tests: every stage scene draws something on the software GPU.

use bpe_gfx::{Frame, Renderer, build_frame, headless_device};
use bpe_kit::{kit, scenes};
use bpe_runtime::Playback;

const W: u32 = 270;
const H: u32 = 480;

#[test]
fn every_stage_scene_draws_a_picture() {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    let kit = kit();
    let mut renderer = Renderer::new(&device, W, H);
    let mut bright = std::collections::BTreeMap::new();
    for (name, scene) in scenes::all(&kit.registry) {
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(2.0);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 2.0, &kit.draws, &mut frame);
        assert_eq!(renderer.pixel_size(&frame), 1080.0 / W as f32);
        renderer.render(&device, &queue, &frame);
        let pixels = renderer.read_output(&device, &queue).unwrap();
        assert_eq!(pixels.len(), (W * H * 4) as usize, "{name}");
        let count = pixels.chunks(4).filter(|p| p[0] > 100 || p[1] > 100 || p[2] > 100).count();
        assert!(count > 0, "{name}: nothing bright was drawn");
        bright.insert(name.clone(), count);
        if std::env::var_os("BPE_SAVE_RENDERS").is_some() {
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pictures");
            std::fs::create_dir_all(&dir).unwrap();
            let file =
                std::fs::File::create(dir.join(format!("{}.png", name.to_lowercase().replace(' ', "-")))).unwrap();
            let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), W, H);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header().unwrap().write_image_data(&pixels).unwrap();
        }
    }
    assert!(bright["Reel block"] > bright["One ball"], "{bright:?}");
    assert!(bright["Lava ball"] > bright["One ball"], "a halo, a streak and lava light up more: {bright:?}");
    assert!(bright["Neon ring"] > bright["Shrinking ring"], "{bright:?}");
    assert_eq!(renderer.shader_problems(), Vec::<String>::new());
}

/// The brightest channel in the 3 x 3 pixels around (x, y).
fn brightest_around(pixels: &[u8], x: u32, y: u32) -> u8 {
    let mut best = 0;
    for dy in -1..=1i32 {
        for dx in -1..=1i32 {
            let (px, py) = ((x as i32 + dx) as u32, (y as i32 + dy) as u32);
            let i = ((py * W + px) * 4) as usize;
            best = best.max(pixels[i]).max(pixels[i + 1]).max(pixels[i + 2]);
        }
    }
    best
}

#[test]
fn a_rectangle_shape_draws_its_sharp_corners_in_plain_white() {
    use bpe_core::Settings;
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    let kit = kit();
    let mut scene = bpe_kit::new_scene(&kit.registry);
    scene.things.get_mut(&bpe_core::ThingId(1)).unwrap().params =
        bpe_kit::ShapeSettings::rectangle(900.0, 1500.0, 16.0).to_params();
    let resolved = bpe_core::resolve(&scene, &kit.registry);
    let playback = Playback::new(&scene, &resolved, kit.sims.clone());
    let mut frame = Frame::default();
    build_frame(&scene, &resolved, playback.world(), playback.log(), 0.0, &kit.draws, &mut frame);
    let mut renderer = Renderer::new(&device, W, H);
    renderer.render(&device, &queue, &frame);
    let pixels = renderer.read_output(&device, &queue).unwrap();
    // World (450, 0) is on the right wall and (450, 750) its corner; (300, 300) is inside, clear
    // of the ball. Four world pixels per output pixel.
    let at = |x: i32, y: i32| ((135 + x / 4) as u32, (240 + y / 4) as u32);
    let (wx, wy) = at(450, 0);
    let (cx, cy) = at(450, 750);
    let (ix, iy) = at(300, 300);
    assert!(brightest_around(&pixels, wx, wy) > 200, "the wall is drawn white");
    assert!(brightest_around(&pixels, cx, cy) > 200, "the sharp corner is drawn");
    assert!(brightest_around(&pixels, ix, iy) < 60, "the middle is empty");
}

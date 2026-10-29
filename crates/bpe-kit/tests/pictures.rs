//! Picture tests: every stage scene draws something on the software GPU.

use bpe_gfx::{Frame, Renderer, build_frame, headless_device};
use bpe_kit::{flow_source, kit, scenes};
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
        // Liquids and swarms on the graphics card are stepped to the frame's time first.
        let source = flow_source(&scene, &resolved, &kit.sims, None);
        assert!(renderer.render_with_flow(&device, &queue, &frame, &source).caught_up, "{name}");
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
fn a_rounded_box_arena_draws_round_corners() {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    let kit = kit();
    let mut scene = bpe_kit::new_scene(&kit.registry);
    let arena = scene.things.get_mut(&bpe_core::ThingId(1)).unwrap();
    arena.params.set("shape", bpe_core::Value::Choice("box".into()));
    arena.params.set("corner", bpe_core::Value::Number(300.0));
    arena.params.set("thickness", bpe_core::Value::Number(16.0));
    let resolved = bpe_core::resolve(&scene, &kit.registry);
    let playback = Playback::new(&scene, &resolved, kit.sims.clone());
    let mut frame = Frame::default();
    build_frame(&scene, &resolved, playback.world(), playback.log(), 0.0, &kit.draws, &mut frame);
    let mut renderer = Renderer::new(&device, W, H);
    renderer.render(&device, &queue, &frame);
    let pixels = renderer.read_output(&device, &queue).unwrap();
    // World (450, 0) is on the straight right wall; world (450, 750) is where the sharp corner
    // would be, well outside the rounded one. Four world pixels per output pixel.
    let (wall_x, wall_y) = (135 + 450 / 4, 240);
    let (corner_x, corner_y) = (135 + 450 / 4, 240 + 750 / 4 - 2);
    assert!(brightest_around(&pixels, wall_x, wall_y) > 100, "the straight wall is drawn");
    assert!(brightest_around(&pixels, corner_x, corner_y) < 40, "the sharp corner is empty");
    // The rounded corner passes through the arc's 45° point: (150 + 212, 450 + 212).
    assert!(brightest_around(&pixels, 135 + 362 / 4, 240 + 662 / 4) > 100, "the round corner is drawn");
}

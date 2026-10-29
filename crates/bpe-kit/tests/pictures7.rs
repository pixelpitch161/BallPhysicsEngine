//! Picture tests for the stage 7 scenes: every scene draws something on the software GPU, the
//! captions have glyphs and the jelly has soft triangles.

use bpe_gfx::{Frame, Renderer, build_frame, headless_device};
use bpe_kit::{kit, scenes};
use bpe_runtime::Playback;

const W: u32 = 270;
const H: u32 = 480;

#[test]
fn every_stage7_scene_draws_a_picture() {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    let kit = kit();
    let mut renderer = Renderer::new(&device, W, H);
    let mut bright = std::collections::BTreeMap::new();
    for (name, scene) in scenes::stage7(&kit.registry) {
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
        match name.as_str() {
            "Captioned reel" | "Drum beat" => assert!(!frame.glyphs.is_empty(), "{name}: no caption glyphs"),
            "Jelly drop" => assert!(frame.soft.len() >= 3, "{name}: no soft triangles"),
            _ => {}
        }
        if std::env::var_os("BPE_SAVE_RENDERS").is_some() {
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pictures7");
            std::fs::create_dir_all(&dir).unwrap();
            let file =
                std::fs::File::create(dir.join(format!("{}.png", name.to_lowercase().replace(' ', "-")))).unwrap();
            let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), W, H);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header().unwrap().write_image_data(&pixels).unwrap();
        }
    }
    assert!(bright["Captioned reel"] > bright["Jelly drop"], "{bright:?}");
}

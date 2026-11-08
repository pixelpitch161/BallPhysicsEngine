//! Particles part 7 in pictures: a Particles thing's Gives off light sub-layer lights a dark lit
//! scene round its particles, and its particles take the scene's light. The checks compare
//! pixels with each other, so they hold on any GPU.
//!
//! `BPE_SAVE_RENDERS=1` saves the pictures into `target/pictures/particles7-*.png`.

use bpe_core::{Scene, Settings, SlotId, ThingId};
use bpe_gfx::{Frame, Renderer, build_frame, test_device};
use bpe_kit::kit;
use bpe_kit::things::light::{LightType, add_light};
use bpe_kit::things::particles::look::{SPRITES, SUB_LIGHT};
use bpe_kit::things::particles::looks::{Look, add_particles};
use bpe_kit::things::particles::{BornFrom, Emit, ParticlesSettings};
use bpe_runtime::Playback;

const W: u32 = 270;
const H: u32 = 480;

/// A blank lit project (its one light switched off, so only the ambient light is left) with a
/// still clump of particles in the middle, given off light when `glows`.
fn scene_of(glows: bool) -> Scene {
    let kit = kit();
    let reg = &kit.registry;
    let mut scene = bpe_kit::new_studio_scene(reg);
    let light = add_light(&mut scene, reg, LightType::Point, ThingId::ROOT).unwrap();
    scene.things.get_mut(&light).unwrap().enabled = false;
    let id = add_particles(&mut scene, reg, Look::Empty, ThingId::ROOT).unwrap();
    let source = ParticlesSettings {
        from: BornFrom::APoint,
        emit: Emit::OnceAtTheStart,
        count: 40,
        speed: 0.0,
        life: 100.0,
        size: 12.0,
        ..ParticlesSettings::default()
    };
    scene.things.get_mut(&id).unwrap().params = source.to_params();
    if glows {
        let card = &scene.things[&id].slots[&SlotId::Look];
        let base = card.layers.iter().find(|l| l.kind == SPRITES).unwrap().id;
        scene.add_sub_layer(reg, id, SlotId::Look, base, SUB_LIGHT).unwrap();
    }
    scene
}

fn picture(scene: &Scene, name: &str) -> Option<Vec<u8>> {
    let (device, queue) = test_device().ok()?;
    let mut renderer = Renderer::new(&device, W, H);
    let kit = kit();
    let resolved = bpe_core::resolve(scene, &kit.registry);
    let mut playback = Playback::new(scene, &resolved, kit.sims.clone());
    let t = 0.5;
    playback.seek(t);
    let mut frame = Frame::default();
    build_frame(scene, &resolved, playback.world(), playback.log(), t, &kit.draws, &mut frame);
    assert!(frame.light.is_some(), "the scene is lit");
    renderer.render(&device, &queue, &frame);
    if renderer.programs_waiting() {
        renderer.render(&device, &queue, &frame);
    }
    let pixels = renderer.read_output(&device, &queue).unwrap();
    if std::env::var_os("BPE_SAVE_RENDERS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pictures");
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(dir.join(format!("particles7-{name}.png"))).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), W, H);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(&pixels).unwrap();
    }
    Some(pixels)
}

/// The summed brightness of the pixels in a ring `from`..`to` world px round the middle.
fn ring(pixels: &[u8], from: f32, to: f32) -> u64 {
    let mut sum = 0;
    for py in 0..H as usize {
        for px in 0..W as usize {
            let x = (px as f32 + 0.5) / W as f32 * 1080.0 - 540.0;
            let y = (py as f32 + 0.5) / H as f32 * 1920.0 - 960.0;
            let d = (x * x + y * y).sqrt();
            if d >= from && d < to {
                let i = (py * W as usize + px) * 4;
                sum += pixels[i] as u64 + pixels[i + 1] as u64 + pixels[i + 2] as u64;
            }
        }
    }
    sum
}

#[test]
fn particles_that_give_off_light_light_the_scene_round_them() {
    let Some(plain) = picture(&scene_of(false), "plain") else {
        eprintln!("no graphics adapter: skipped");
        return;
    };
    let glowing = picture(&scene_of(true), "glowing").unwrap();
    // Round the clump (which itself spans some 20 px), the glowing particles' light shows.
    let (a, b) = (ring(&plain, 60.0, 200.0), ring(&glowing, 60.0, 200.0));
    assert!(b > a + a / 10 + 500, "the light round them: {a} plain, {b} glowing");
    // Far away the picture is the same dark.
    let (far_a, far_b) = (ring(&plain, 800.0, 900.0), ring(&glowing, 800.0, 900.0));
    assert!(far_b <= far_a + far_a / 5 + 2000, "{far_a} vs {far_b}");
}

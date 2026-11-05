//! Picture tests for the emitter: its marker and a wavefront put light where they should.

use bpe_core::{Scene, Settings, SlotId, ThingId, Value, layered_key};
use bpe_gfx::{Frame, Renderer, build_frame, headless_device};
use bpe_kit::things::emitter::{EmitterSettings, KIND, Marker, StartFrom};
use bpe_kit::{kit, new_scene};
use bpe_runtime::Playback;
use glam::Vec2;

const W: u32 = 270;
const H: u32 = 480;

/// The brightest channel in the 3 x 3 pixels around world point `p` (the camera at rest: 4
/// world pixels to a picture pixel, the origin in the middle).
fn brightest_at(pixels: &[u8], p: Vec2) -> u8 {
    let (x, y) = ((W as f32 * 0.5 + p.x / 4.0).round() as i32, (H as f32 * 0.5 + p.y / 4.0).round() as i32);
    let mut best = 0;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let i = (((y + dy) as u32 * W + (x + dx) as u32) * 4) as usize;
            best = best.max(pixels[i]).max(pixels[i + 1]).max(pixels[i + 2]);
        }
    }
    best
}

/// The new scene without its ball or shape, with an emitter set by `s`; and its id.
fn scene_with(s: EmitterSettings) -> (Scene, ThingId) {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    scene.remove_thing(ThingId(2)).unwrap();
    scene.remove_thing(ThingId(1)).unwrap();
    let id = bpe_kit::add_thing(&mut scene, &kit.registry, KIND, ThingId::ROOT).unwrap();
    scene.things.get_mut(&id).unwrap().params = s.to_params();
    (scene, id)
}

fn render(scene: &Scene) -> Option<Vec<u8>> {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return None;
    };
    let kit = kit();
    let resolved = bpe_core::resolve(scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    let playback = Playback::new(scene, &resolved, kit.sims.clone());
    let mut frame = Frame::default();
    build_frame(scene, &resolved, playback.world(), playback.log(), 0.0, &kit.draws, &mut frame);
    let mut renderer = Renderer::new(&device, W, H);
    renderer.render(&device, &queue, &frame);
    assert_eq!(renderer.shader_problems(), Vec::<String>::new());
    Some(renderer.read_output(&device, &queue).unwrap())
}

#[test]
fn an_emitters_marker_draws_its_ring_and_dot() {
    let (scene, _) = scene_with(EmitterSettings {
        emits: false,
        marker: Marker::RingWithDot,
        marker_size: 80.0,
        marker_glow: 0.0,
        ripple: false,
        ..EmitterSettings::default()
    });
    let Some(pixels) = render(&scene) else { return };
    let ring = brightest_at(&pixels, Vec2::new(72.0, 0.0));
    let dot = brightest_at(&pixels, Vec2::ZERO);
    let between = brightest_at(&pixels, Vec2::new(44.0, 0.0));
    let outside = brightest_at(&pixels, Vec2::new(0.0, 160.0));
    assert!(ring > 150 && dot > 150, "ring {ring}, dot {dot}");
    assert!(between + 60 < ring && outside + 60 < ring, "between {between}, outside {outside}, ring {ring}");
}

#[test]
fn a_wavefront_lights_the_line_between_the_balls_of_a_round() {
    let s = EmitterSettings {
        count: 24,
        start_from: StartFrom::Ring,
        start_radius: 400.0,
        speed: 0.0,
        radius: 6.0,
        marker: Marker::None,
        ripple: false,
        ..EmitterSettings::default()
    };
    let (mut scene, id) = scene_with(s);
    let shot = |scene: &Scene| render(scene);
    // Halfway between two balls of the ring: on the line, off both balls.
    let between = Vec2::from_angle((-90.0f32 + 7.5).to_radians()) * 400.0 * 7.5f32.to_radians().cos();
    let Some(plain) = shot(&scene) else { return };
    let reg = bpe_kit::registry();
    let layer = scene.add_layer(&reg, id, SlotId::Look, bpe_kit::modules::look::layers::wavefront::WAVEFRONT).unwrap();
    scene.card_mut(id, SlotId::Look).unwrap().params.set(&layered_key(layer, "width"), Value::Number(16.0));
    let Some(lined) = shot(&scene) else { return };
    let (before, after) = (brightest_at(&plain, between), brightest_at(&lined, between));
    assert!(after > 150 && before + 100 < after, "before {before}, after {after}");
}

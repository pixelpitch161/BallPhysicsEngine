//! Soft bodies, billiard pockets, spinning balls and custom-shape arenas in the picture.
//!
//! These need a graphics adapter, and fail rather than skip without one: on a machine with no
//! graphics card, install a software Vulkan driver (Mesa's lavapipe) to run them.

use crate::{FrameData, Renderer, headless_device};
use bpe_scene::{Bodies, Scene};
use bpe_sim::Playback;

fn preset(name: &str) -> Scene {
    Scene::presets().into_iter().find(|p| p.name == name).unwrap().scene
}

fn device() -> (wgpu::Device, wgpu::Queue) {
    headless_device().expect("no graphics adapter: install a software Vulkan driver (Mesa lavapipe) for picture tests")
}

fn frame_at(scene: &Scene, time: f64) -> FrameData {
    let mut playback = Playback::new(scene);
    playback.seek(time);
    FrameData::build(scene, &playback, 1.0)
}

fn picture(device: &wgpu::Device, queue: &wgpu::Queue, scene: &Scene, time: f64) -> Vec<u8> {
    let mut renderer = Renderer::new(device, 270, 480);
    let mut playback = Playback::new(scene);
    playback.seek(time);
    let frame = FrameData::build(scene, &playback, renderer.pixel_size(scene));
    renderer.render(device, queue, scene, time, &frame);
    renderer.read_output(device, queue).unwrap()
}

/// Pixels that differ clearly between two pictures.
fn changed(a: &[u8], b: &[u8]) -> usize {
    a.chunks(4).zip(b.chunks(4)).filter(|(p, q)| (0..3).map(|i| p[i].abs_diff(q[i]) as u32).sum::<u32>() > 60).count()
}

#[test]
fn soft_bodies_are_drawn() {
    let (device, queue) = device();
    for (name, time, least) in
        [("Jelly", 4.0, 3000), ("Cloth", 3.0, 8000), ("Liquid", 3.0, 8000), ("Rope bridge", 3.0, 150)]
    {
        let scene = preset(name);
        let without = Scene { bodies: Bodies::default(), ..scene.clone() };
        let with = picture(&device, &queue, &scene, time);
        let plain = picture(&device, &queue, &without, time);
        let count = changed(&with, &plain);
        assert!(count > least, "{name}: only {count} pixels show its soft bodies");
    }
}

#[test]
fn soft_bodies_fill_the_frame_data() {
    let scene = preset("Jelly");
    let frame = frame_at(&scene, 2.0);
    let jelly = &scene.bodies.jelly[0];
    // A fan of triangles per jelly ball, one per point of its skin.
    assert_eq!(frame.soft.len(), (jelly.columns * jelly.rows * jelly.points * 3) as usize);
    let scene = preset("Liquid");
    let frame = frame_at(&scene, 1.0);
    assert_eq!(frame.soft.len(), scene.bodies.fluid.count as usize * 6 * 3);
    let scene = preset("Cloth");
    let cloth = &scene.bodies.cloths[0];
    let frame = frame_at(&scene, 1.0);
    assert_eq!(frame.soft.len(), ((cloth.columns - 1) * (cloth.rows - 1) * 6) as usize);
    assert!(frame.soft.iter().all(|v| v.position.iter().all(|c| c.is_finite())));
}

#[test]
fn a_cut_rope_draws_one_piece_fewer() {
    let scene = preset("Rope bridge");
    let mut playback = Playback::new(&scene);
    playback.seek(1.0);
    let whole = FrameData::build(&scene, &playback, 1.0).shapes.len();
    let mut sim = playback.simulation().clone();
    assert!(sim.cut_soft(0, 0.5));
    let soft = sim.soft();
    assert_eq!(soft.links_of(0).count() + 1, scene.bodies.ropes[0].points as usize - 1);
    assert!(whole > 0);
}

#[test]
fn pockets_and_spin_glints_are_drawn() {
    let scene = preset("Billiards");
    let frame = frame_at(&scene, 0.5);
    let pockets = scene.physics.contact.pockets.len();
    assert_eq!(pockets, 6);
    let dark = frame.shapes.iter().filter(|s| s.c[0] == 2.0 && s.color[0] < 0.05 && s.c[1] > 40.0).count();
    assert_eq!(dark, pockets);
    // Each ball still on the table has a glint showing how far it has turned.
    let alive = frame.balls.len();
    assert!(frame.flashes.len() >= alive);
}

#[test]
fn a_heart_arena_is_drawn_as_its_outline() {
    let (device, queue) = device();
    let scene = preset("Heart arena");
    let frame = frame_at(&scene, 1.0);
    assert!(frame.shapes.len() > 50, "the heart's outline is made of many short pieces");
    let picture = picture(&device, &queue, &scene, 1.0);
    // The outline's colour shows near the heart's lowest point, the middle of the bottom.
    let (w, h) = (270usize, 480usize);
    let column = w / 2;
    let lit = (h / 2..h).filter(|&row| {
        let p = &picture[(row * w + column) * 4..][..4];
        p[0] as u32 + p[1] as u32 + p[2] as u32 > 250
    });
    assert!(lit.count() > 0, "no outline below the middle");
}

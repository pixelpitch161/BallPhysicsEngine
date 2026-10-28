//! Picture tests for the GPU layers on whatever adapter is around (software Vulkan in the cloud).
//!
//! Set `BPE_REQUIRE_GPU=1` to fail instead of skipping when there is no adapter, and
//! `BPE_SAVE_RENDERS=<folder>` to save each test's pictures as PNG files for a look.

use super::*;
use crate::{FrameData, Renderer, headless_device};
use bpe_scene::{EventMatch, GpuFluid, ParticlePreset, Swarm};

pub(crate) fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
    match headless_device() {
        Ok(pair) => Some(pair),
        Err(error) if std::env::var("BPE_REQUIRE_GPU").is_ok_and(|v| v == "1") => {
            panic!("BPE_REQUIRE_GPU is set but there is no GPU adapter: {error}")
        }
        Err(_) => {
            eprintln!("no GPU adapter available, skipping");
            None
        }
    }
}

const W: u32 = 135;
const H: u32 = 240;

/// Renders `scene` at each of `times` in turn with one renderer, returning the last picture.
fn render(device: &wgpu::Device, queue: &wgpu::Queue, renderer: &mut Renderer, scene: &Scene, times: &[f64]) -> Vec<u8> {
    let mut playback = Playback::new(scene);
    let mut frame = FrameData::default();
    for &time in times {
        playback.seek(time);
        frame.fill(scene, &playback, renderer.pixel_size(scene));
        renderer.render(device, queue, scene, time, &frame);
    }
    renderer.read_output(device, queue).unwrap()
}

fn fresh(device: &wgpu::Device) -> Renderer {
    Renderer::new(device, W, H)
}

pub(crate) fn save(name: &str, pixels: &[u8], width: u32, height: u32) {
    let Ok(folder) = std::env::var("BPE_SAVE_RENDERS") else { return };
    let _ = std::fs::create_dir_all(&folder);
    let file = std::fs::File::create(std::path::Path::new(&folder).join(format!("{name}.png"))).unwrap();
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.write_header().unwrap().write_image_data(pixels).unwrap();
}

fn brightness(pixels: &[u8]) -> u64 {
    pixels.chunks(4).map(|p| p[0] as u64 + p[1] as u64 + p[2] as u64).sum()
}

/// Largest difference of any colour channel between two pictures.
fn max_difference(a: &[u8], b: &[u8]) -> u8 {
    a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0)
}

pub(crate) fn swarm_scene(count: u32) -> Scene {
    let mut scene = Scene::default();
    scene.gpu.swarm = Swarm { enabled: true, count, brightness: 0.8, size: 2.0, ..Swarm::default() };
    scene
}

pub(crate) fn fluid_scene(count: u32) -> Scene {
    let mut scene = Scene::rainbow_trails();
    scene.gpu.fluid = GpuFluid { enabled: true, count, fill: 0.35, ..GpuFluid::default() };
    scene
}

#[test]
fn particles_draw_and_scrub_exactly() {
    let Some((device, queue)) = gpu() else { return };
    let mut scene = Scene::default();
    let plain = render(&device, &queue, &mut fresh(&device), &scene, &[3.0]);
    for preset in ParticlePreset::ALL {
        scene.particles = vec![preset.emitter(EventMatch::AnyHit)];
        let straight = render(&device, &queue, &mut fresh(&device), &scene, &[3.0]);
        save(&format!("particles-{}", preset.name().to_lowercase()), &straight, W, H);
        assert_ne!(straight, plain, "{} drew nothing", preset.name());
        let mut renderer = fresh(&device);
        let scrubbed = render(&device, &queue, &mut renderer, &scene, &[5.0, 1.0, 3.0]);
        assert!(straight == scrubbed, "{} differs after scrubbing", preset.name());
    }
}

#[test]
fn bouncing_particles_stay_inside() {
    let Some((device, queue)) = gpu() else { return };
    let mut scene = Scene::default();
    let mut sparks = ParticlePreset::Sparks.emitter(EventMatch::Wall);
    sparks.bounce = true;
    sparks.speed = 900.0;
    sparks.life = 2.0;
    sparks.color = bpe_scene::ParticleColor::Fixed(bpe_scene::Color::WHITE);
    scene.particles = vec![sparks];
    scene.look.flash_strength = 0.0;
    scene.look.bloom_strength = 0.0;
    let pixels = render(&device, &queue, &mut fresh(&device), &scene, &[4.0]);
    save("particles-bounce", &pixels, W, H);
    // Nothing bright well outside the ring (radius 508 of a 1080 wide frame).
    let mut outside = 0;
    for y in 0..H {
        for x in 0..W {
            let wx = (x as f32 + 0.5) / W as f32 * 1080.0 - 540.0;
            let wy = (y as f32 + 0.5) / H as f32 * 1920.0 - 960.0;
            let p = &pixels[((y * W + x) * 4) as usize..][..4];
            if (wx * wx + wy * wy).sqrt() > 560.0 && p[0] > 120 {
                outside += 1;
            }
        }
    }
    assert!(outside < 3, "{outside} bright pixels outside the ring");
}

#[test]
fn swarm_is_the_same_however_you_get_there() {
    let Some((device, queue)) = gpu() else { return };
    let scene = swarm_scene(4000);
    let straight = render(&device, &queue, &mut fresh(&device), &scene, &[2.5]);
    save("swarm", &straight, W, H);
    let plain = render(&device, &queue, &mut fresh(&device), &Scene::default(), &[2.5]);
    assert!(brightness(&straight) > brightness(&plain) + 5000, "the swarm should add light");
    // Playing at 30 frames a second gets to the same picture.
    let frames: Vec<f64> = (0..=75).map(|i| i as f64 / 30.0).collect();
    let played = render(&device, &queue, &mut fresh(&device), &scene, &frames);
    assert!(played == straight, "playing through differs by {}", max_difference(&played, &straight));
    // Seeking back to an earlier time restores a checkpoint and steps forward again.
    let mut renderer = fresh(&device);
    let back = render(&device, &queue, &mut renderer, &scene, &[4.0, 2.5]);
    assert!(back == straight, "seeking back differs by {}", max_difference(&back, &straight));
}

#[test]
fn liquid_is_the_same_however_you_get_there() {
    let Some((device, queue)) = gpu() else { return };
    let scene = fluid_scene(3000);
    let straight = render(&device, &queue, &mut fresh(&device), &scene, &[1.5]);
    save("liquid", &straight, W, H);
    let plain = render(&device, &queue, &mut fresh(&device), &Scene::rainbow_trails(), &[1.5]);
    assert_ne!(straight, plain);
    let frames: Vec<f64> = (0..=45).map(|i| i as f64 / 30.0).collect();
    let played = render(&device, &queue, &mut fresh(&device), &scene, &frames);
    assert!(played == straight, "playing through differs by {}", max_difference(&played, &straight));
    let back = render(&device, &queue, &mut fresh(&device), &scene, &[2.5, 1.5]);
    assert!(back == straight, "seeking back differs by {}", max_difference(&back, &straight));
}

#[test]
fn looks_change_without_restarting() {
    let Some((device, queue)) = gpu() else { return };
    let mut scene = swarm_scene(2000);
    let mut renderer = fresh(&device);
    render(&device, &queue, &mut renderer, &scene, &[1.0]);
    let steps = renderer.gpu.steps_run();
    scene.gpu.swarm.brightness = 0.3;
    scene.gpu.swarm.colors = vec![bpe_scene::Color::rgb(255, 0, 0)];
    scene.look.bloom_strength = 0.9;
    render(&device, &queue, &mut renderer, &scene, &[1.0]);
    assert_eq!(renderer.gpu.steps_run(), steps, "a colour change should not start the swarm again");
    scene.gpu.swarm.flow = 2.0;
    render(&device, &queue, &mut renderer, &scene, &[1.0]);
    assert!(renderer.gpu.steps_run() > steps, "a movement change starts again from the beginning");
}

#[test]
fn switched_off_layers_change_nothing() {
    let Some((device, queue)) = gpu() else { return };
    let plain = render(&device, &queue, &mut fresh(&device), &Scene::default(), &[2.0]);
    let mut scene = Scene::default();
    scene.gpu.swarm.count = 1000;
    scene.gpu.fluid.count = 9000;
    scene.particles = vec![bpe_scene::Emitter { enabled: false, ..Default::default() }];
    let mut renderer = fresh(&device);
    let pixels = render(&device, &queue, &mut renderer, &scene, &[2.0]);
    assert!(pixels == plain);
    assert_eq!(renderer.gpu.steps_run(), 0);
}

#[test]
fn every_wall_kind_keeps_the_swarm_out() {
    let Some((device, queue)) = gpu() else { return };
    for (name, mut scene) in [
        ("ring", Scene::default()),
        ("box", Scene::box_arena()),
        ("hexagon", Scene::spinning_hexagon()),
        ("rings", Scene::escape_rings()),
        ("plinko", Scene::plinko()),
    ] {
        scene.gpu.swarm = Swarm { enabled: true, count: 3000, avoid_walls: 1.0, ..Swarm::default() };
        let mut renderer = fresh(&device);
        let pixels = render(&device, &queue, &mut renderer, &scene, &[2.0]);
        save(&format!("swarm-{name}"), &pixels, W, H);
        let points = renderer.gpu.swarm.as_ref().unwrap().read_points(&device, &queue);
        let walls = Walls::of(&{
            let mut p = Playback::new(&scene);
            p.seek(2.0);
            p.simulation().clone()
        });
        let half = Vec2::new(540.0, 960.0);
        let mut inside_walls = 0;
        for p in &points {
            assert!(p.is_finite(), "{name}: a point went to {p}");
            assert!(p.x.abs() <= half.x && p.y.abs() <= half.y, "{name}: {p} left the frame");
            if walls.distance(*p) < -2.0 {
                inside_walls += 1;
            }
        }
        // Walls move between steps, so allow a handful caught on a moving edge.
        assert!(inside_walls <= points.len() / 200, "{name}: {inside_walls} points inside walls");
    }
}

/// Bigger pictures of each layer for a human to look at: run with `--ignored` and
/// `BPE_SAVE_RENDERS` set.
#[test]
#[ignore]
fn showcase() {
    let Some((device, queue)) = gpu() else { return };
    let (w, h) = (540, 960);
    let mut liquid = fluid_scene(40_000);
    liquid.gpu.fluid.pour_seconds = 0.0;
    let mut poured = fluid_scene(40_000);
    poured.gpu.fluid.pour_seconds = 3.0;
    let mut confetti = Scene::escape_rings();
    confetti.particles = vec![
        ParticlePreset::Confetti.emitter(EventMatch::Escape),
        ParticlePreset::Sparks.emitter(EventMatch::AnyHit),
        ParticlePreset::Rings.emitter(EventMatch::AnyHit),
    ];
    let mut smoke = Scene::plinko();
    smoke.particles = vec![ParticlePreset::Smoke.emitter(EventMatch::Obstacle), ParticlePreset::Stars.emitter(EventMatch::Obstacle)];
    for (name, scene, time) in [
        ("show-swarm", swarm_scene(60_000), 4.0),
        ("show-liquid", liquid, 3.0),
        ("show-pour", poured, 2.5),
        ("show-confetti", confetti, 9.0),
        ("show-smoke", smoke, 6.0),
    ] {
        if std::env::var("BPE_SHOW").is_ok_and(|only| !name.contains(&only)) {
            continue;
        }
        let mut renderer = Renderer::new(&device, w, h);
        let frames: Vec<f64> = (0..=(time * 10.0) as usize).map(|i| i as f64 / 10.0).collect();
        let pixels = render(&device, &queue, &mut renderer, &scene, &frames);
        save(name, &pixels, w, h);
    }
}

/// Prints how the liquid settles, for tuning: run with `--ignored --nocapture`.
#[test]
#[ignore]
fn liquid_report() {
    let Some((device, queue)) = gpu() else { return };
    let mut scene = fluid_scene(20_000);
    scene.spawners.clear();
    let mut renderer = Renderer::new(&device, 270, 480);
    for t in [0.0, 0.5, 1.0, 2.0, 3.0, 4.0] {
        render(&device, &queue, &mut renderer, &scene, &[t]);
        let fluid = renderer.gpu.fluid.as_ref().unwrap();
        let state = fluid.read_state(&device, &queue);
        let s = fluid.spacing();
        let ys: Vec<f32> = state.iter().map(|p| p[1]).collect();
        let top = ys.iter().cloned().fold(f32::MAX, f32::min);
        let speed = state.iter().map(|p| (p[2] * p[2] + p[3] * p[3]).sqrt()).sum::<f32>() / state.len() as f32;
        // Nearest neighbour distances on a sample.
        let mut near = Vec::new();
        for i in (0..state.len()).step_by(97) {
            let mut best = f32::MAX;
            for (j, q) in state.iter().enumerate() {
                if j != i {
                    let d = ((q[0] - state[i][0]).powi(2) + (q[1] - state[i][1]).powi(2)).sqrt();
                    best = best.min(d);
                }
            }
            near.push(best / s);
        }
        near.sort_by(f32::total_cmp);
        println!(
            "t={t}: spacing {s:.2}, top {top:.0}, mean speed {speed:.1}, nearest/spacing p10 {:.2} p50 {:.2} p90 {:.2}",
            near[near.len() / 10],
            near[near.len() / 2],
            near[near.len() * 9 / 10]
        );
    }
}

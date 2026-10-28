//! Picture tests for additive colours (the colour picker's "Additive" blending).

use super::*;
use bpe_scene::{BallStyle, Color, Container, Spawner, Vec2};
use bpe_sim::Playback;

const W: u32 = 540;
const H: u32 = 960;
const RED: Color = Color::rgb(0xff, 0x20, 0x20);
const GREEN: Color = Color::rgb(0x20, 0xff, 0x20);

/// A red ball at the centre with a green ball over its right half, no gravity, walls or bloom.
fn overlap(green: Color, style: Option<BallStyle>) -> Scene {
    let mut scene = Scene::default();
    scene.physics.gravity = Vec2::ZERO;
    scene.physics.ball_collisions = false;
    scene.container = Container::Circle { center: Vec2::ZERO, radius: 5000.0, thickness: 0.0, color: Color::WHITE };
    scene.look.bloom_strength = 0.0;
    scene.look.flash_strength = 0.0;
    scene.look.ball_glow = 0.0;
    scene.effects.clear();
    let ball = |x: f32, color: Color| Spawner::Single {
        position: Vec2::new(x, 0.0),
        velocity: Vec2::ZERO,
        radius: 150.0,
        color,
        depth: 0.0,
        depth_velocity: 0.0,
        style: style.clone(),
    };
    scene.spawners = vec![ball(0.0, RED), ball(150.0, green)];
    scene
}

fn render(scene: &Scene) -> Option<Vec<u8>> {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return None;
    };
    let mut renderer = Renderer::new(&device, W, H);
    let playback = Playback::new(scene);
    let frame = FrameData::build(scene, &playback, renderer.pixel_size(scene));
    renderer.render(&device, &queue, scene, 0.0, &frame);
    Some(renderer.read_output(&device, &queue).unwrap())
}

/// The pixel at a world position.
fn pixel(pixels: &[u8], at: Vec2) -> [u8; 4] {
    let x = ((at.x / 1080.0 + 0.5) * W as f32) as usize;
    let y = ((at.y / 1920.0 + 0.5) * H as f32) as usize;
    let i = (y * W as usize + x) * 4;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

fn save(name: &str, pixels: &[u8]) {
    crate::gpu::tests::save(name, pixels, W, H);
}

/// Where the two balls overlap, a normal green ball hides the red one and an additive one adds
/// to it, for both classic and styled balls.
#[test]
fn additive_balls_add_their_light() {
    let glow = Color::additive(GREEN.r, GREEN.g, GREEN.b);
    for (name, style) in [("classic", None), ("styled", Some(BallStyle::default()))] {
        let (Some(normal), Some(added)) =
            (render(&overlap(GREEN, style.clone())), render(&overlap(glow, style.clone())))
        else {
            return;
        };
        save(&format!("additive-{name}-normal"), &normal);
        save(&format!("additive-{name}"), &added);
        let both = Vec2::new(75.0, 30.0);
        let only_green = Vec2::new(260.0, 30.0);
        let n = pixel(&normal, both);
        let a = pixel(&added, both);
        assert!(n[0] < 120 && n[1] > 150, "{name}: a normal ball covers the red one, got {n:?}");
        assert!(a[0] > 150 && a[1] > 150, "{name}: an additive ball adds to the red one, got {a:?}");
        // On the dark background alone, the additive ball still shows its own colour.
        let g = pixel(&added, only_green);
        assert!(g[1] > 120 && g[0] < 120, "{name}: additive ball on its own, got {g:?}");
    }
}

/// Walls, pegs and lines in an additive colour are drawn as added light; others cover.
#[test]
fn additive_shapes_cover_nothing() {
    let glow = Color::additive(GREEN.r, GREEN.g, GREEN.b).to_linear();
    let added = ShapeInstance::capsule(Vec2::ZERO, Vec2::X, 4.0, glow, 0.0);
    assert!(added.color[3] < 0.0);
    let normal = ShapeInstance::capsule(Vec2::ZERO, Vec2::X, 4.0, GREEN.to_linear(), 0.0);
    assert_eq!(normal.color[3], 1.0);
    // Fully transparent black stays transparent, not additive.
    assert_eq!(crate::frame::blend_alpha([0.0; 4]), [0.0; 4]);
}

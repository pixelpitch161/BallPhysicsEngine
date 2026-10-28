//! Picture tests for effects on things: shader materials on ball groups, walls and obstacles.

use super::*;
use bpe_scene::{Color, Container, Effect, EffectTarget, Obstacle, Spawner, Vec2, View};
use bpe_sim::Playback;
use effects::graph::Builder;

const W: u32 = 270;
const H: u32 = 480;

const LEFT_BALL: Vec2 = Vec2::new(-200.0, 0.0);
const RIGHT_BALL: Vec2 = Vec2::new(200.0, 0.0);
const WALL: Vec2 = Vec2::new(0.0, -450.0);
const PEG: Vec2 = Vec2::new(0.0, 250.0);
const EMPTY: Vec2 = Vec2::new(0.0, -200.0);

/// Two still balls (groups 0 and 1) in a ring with a peg below them, and a node effect that
/// paints everything it touches plain green.
fn scene(target: EffectTarget) -> Scene {
    let mut scene = Scene::default();
    scene.physics.gravity = Vec2::ZERO;
    scene.container = Container::Circle { center: Vec2::ZERO, radius: 450.0, thickness: 24.0, color: Color::WHITE };
    scene.obstacles = vec![Obstacle::Peg { position: PEG, radius: 40.0, color: Color::WHITE }];
    scene.look.bloom_strength = 0.0;
    scene.look.flash_strength = 0.0;
    scene.look.trail_seconds = 0.0;
    let ball = |position| Spawner::Single {
        position,
        velocity: Vec2::ZERO,
        radius: 70.0,
        color: Color::rgb(0xff, 0x40, 0x40),
        depth: 0.0,
        depth_velocity: 0.0,
        style: None,
    };
    scene.spawners = vec![ball(LEFT_BALL), ball(RIGHT_BALL)];
    let mut b = Builder::new("Green");
    let colour = b.add("colour");
    b.set_color(colour, "color", [0.0, 1.0, 0.0]);
    let out = b.add("output");
    b.wire(colour, "out", out, "color");
    scene.shader_graphs = vec![b.finish()];
    scene.effects = vec![Effect::new("graph:Green").on(target)];
    scene
}

fn render(scene: &Scene) -> Option<Vec<u8>> {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return None;
    };
    let mut renderer = Renderer::new(&device, W, H);
    let mut playback = Playback::new(scene);
    playback.seek(0.5);
    let frame = FrameData::build(scene, &playback, renderer.pixel_size(scene));
    renderer.render(&device, &queue, scene, 0.5, &frame);
    Some(renderer.read_output(&device, &queue).unwrap())
}

/// The pixel at a world position.
fn pixel(pixels: &[u8], at: Vec2) -> [u8; 3] {
    let x = ((at.x / 1080.0 + 0.5) * W as f32) as usize;
    let y = ((at.y / 1920.0 + 0.5) * H as f32) as usize;
    let i = (y * W as usize + x) * 4;
    [pixels[i], pixels[i + 1], pixels[i + 2]]
}

fn green(p: [u8; 3]) -> bool {
    p[1] > 200 && p[0] < 40 && p[2] < 40
}

#[test]
fn effects_on_things_change_only_those_things() {
    let mut plain = scene(EffectTarget::Picture);
    plain.effects.clear();
    let Some(before) = render(&plain) else { return };
    let places = [("left ball", LEFT_BALL), ("right ball", RIGHT_BALL), ("wall", WALL), ("peg", PEG), ("empty", EMPTY)];
    let cases = [
        (EffectTarget::Picture, vec!["left ball", "right ball", "wall", "peg", "empty"]),
        (EffectTarget::Balls, vec!["left ball", "right ball"]),
        (EffectTarget::Group(1), vec!["right ball"]),
        (EffectTarget::Walls, vec!["wall"]),
        (EffectTarget::Obstacles, vec!["peg"]),
        (EffectTarget::Group(7), vec![]),
    ];
    for (target, painted) in cases {
        let after = render(&scene(target)).unwrap();
        for (name, at) in places {
            if painted.contains(&name) {
                assert!(green(pixel(&after, at)), "{target:?} should paint the {name}: {:?}", pixel(&after, at));
            } else {
                assert_eq!(pixel(&after, at), pixel(&before, at), "{target:?} changed the {name}");
            }
        }
    }
}

#[test]
fn effects_on_balls_follow_styled_shapes() {
    // A square ball is painted over its corners, which a round ball doesn't reach.
    let mut scene = scene(EffectTarget::Group(0));
    if let Spawner::Single { style, .. } = &mut scene.spawners[0] {
        *style = Some(bpe_scene::BallStyle { shape: bpe_scene::BallShape::Square, ..Default::default() });
    }
    let Some(after) = render(&scene) else { return };
    let corner = LEFT_BALL + Vec2::new(52.0, 52.0);
    assert!(green(pixel(&after, corner)), "{:?}", pixel(&after, corner));
    assert!(!green(pixel(&after, LEFT_BALL + Vec2::new(100.0, 0.0))));
}

#[test]
fn effects_on_balls_work_in_3d() {
    let mut scene = scene(EffectTarget::Balls);
    scene.obstacles.clear();
    scene.view = View::Space(Box::default());
    let mut plain = scene.clone();
    plain.effects.clear();
    let Some(before) = render(&plain) else { return };
    let after = render(&scene).unwrap();
    let changed: Vec<usize> =
        (0..before.len() / 4).filter(|&i| before[i * 4..i * 4 + 3] != after[i * 4..i * 4 + 3]).collect();
    assert!(changed.len() > 200, "only {} pixels changed", changed.len());
    // Only the balls change, towards green.
    assert!(changed.iter().all(|&i| after[i * 4] <= before[i * 4] && after[i * 4 + 2] <= before[i * 4 + 2]));
    // On the walls instead, the balls stay as they were.
    scene.effects[0].target = EffectTarget::Walls;
    let walls = render(&scene).unwrap();
    let ball_pixels = changed.iter().filter(|&&i| walls[i * 4..i * 4 + 3] == before[i * 4..i * 4 + 3]).count();
    assert!(ball_pixels * 10 > changed.len() * 9, "{ball_pixels} of {}", changed.len());
}

#[test]
fn a_switched_off_effect_on_things_changes_nothing() {
    let mut plain = scene(EffectTarget::Picture);
    plain.effects.clear();
    let Some(before) = render(&plain) else { return };
    let mut off = scene(EffectTarget::Group(0));
    off.effects[0].enabled = false;
    assert_eq!(render(&off).unwrap(), before);
}

#[test]
fn object_block_sees_balls_from_any_effect() {
    // A full-picture effect that shows a ball group's seed as brightness: different balls get
    // different numbers, and empty space gets none.
    let mut scene = scene(EffectTarget::Picture);
    let mut b = Builder::new("Seeds");
    let object = b.add("object");
    let out = b.add("output");
    b.wire(object, "seed", out, "color");
    scene.shader_graphs = vec![b.finish()];
    scene.effects = vec![Effect::new("graph:Seeds")];
    let Some(after) = render(&scene) else { return };
    let (left, right) = (pixel(&after, LEFT_BALL), pixel(&after, RIGHT_BALL));
    assert_ne!(left, right);
    assert_eq!(pixel(&after, EMPTY), [0, 0, 0]);
}

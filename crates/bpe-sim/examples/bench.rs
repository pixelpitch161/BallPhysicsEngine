//! Times the physics on big scenes, in milliseconds per video frame (a 60 fps frame has 16.7 ms),
//! and counts memory allocations per step once a scene is running.
//! Run with `cargo run --release -p bpe-sim --example bench [case name]`.

use bpe_scene::{
    BallContact, Bodies, BounceRules, Cloth, ClothPin, Color, Container, Fluid, Jelly, Physics, Scene, Spawner, Vec2,
};
use bpe_sim::Simulation;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static GLOBAL: Counting = Counting;

fn room(size: Vec2) -> Container {
    Container::Rect { center: Vec2::ZERO, size, thickness: 4.0, color: Color::WHITE }
}

fn grid(columns: u32, rows: u32, spacing: f32, radius: f32, center: Vec2) -> Spawner {
    Spawner::Grid {
        center,
        columns,
        rows,
        spacing,
        radius,
        velocity: Vec2::new(120.0, -60.0),
        gradient: vec![Color::WHITE],
        depth: 0.0,
        depth_velocity: 0.0,
    }
}

fn bumping() -> Physics {
    Physics {
        gravity: Vec2::new(0.0, 1400.0),
        restitution: 0.4,
        ball_collisions: true,
        contact: BallContact { quality: 8, friction: 0.4, sleep: true, ..BallContact::default() },
        ..Physics::default()
    }
}

fn scene(container: Container, spawners: Vec<Spawner>, bodies: Bodies, physics: Physics, max_balls: u32) -> Scene {
    Scene {
        container,
        spawners,
        bodies,
        physics,
        rules: BounceRules { max_balls, ..BounceRules::default() },
        ..Scene::default()
    }
}

fn cases() -> Vec<(&'static str, Scene)> {
    let tall = room(Vec2::new(1000.0, 1800.0));
    let falling = Physics { gravity: Vec2::new(0.0, 1400.0), ..Physics::default() };
    let jelly =
        Jelly { columns: 5, rows: 10, spacing: 130.0, radius: 55.0, points: 32, squishiness: 0.5, ..Jelly::default() };
    let cloth =
        Cloth { size: Vec2::new(700.0, 700.0), columns: 40, rows: 40, pin: ClothPin::TopCorners, ..Cloth::default() };
    let fluid = Fluid { count: 8000, size: Vec2::new(900.0, 900.0), radius: 5.0, ..Fluid::default() };
    vec![
        (
            "3000 balls bumping",
            scene(
                tall.clone(),
                vec![grid(60, 50, 15.0, 6.0, Vec2::new(0.0, -300.0))],
                Bodies::default(),
                bumping(),
                20_000,
            ),
        ),
        (
            "500 ball stack",
            scene(
                room(Vec2::new(800.0, 1500.0)),
                vec![grid(20, 25, 34.0, 16.0, Vec2::new(0.0, -100.0))],
                Bodies::default(),
                bumping(),
                1000,
            ),
        ),
        (
            "50 jelly balls x 32",
            scene(tall.clone(), Vec::new(), Bodies { jelly: vec![jelly], ..Bodies::default() }, falling.clone(), 1),
        ),
        (
            "40x40 cloth",
            scene(tall.clone(), Vec::new(), Bodies { cloths: vec![cloth], ..Bodies::default() }, falling.clone(), 1),
        ),
        (
            "8000 drops of liquid",
            scene(tall.clone(), Vec::new(), Bodies { fluid, ..Bodies::default() }, falling.clone(), 1),
        ),
        (
            "10000 balls, no bumping",
            scene(tall, vec![grid(100, 100, 9.0, 3.0, Vec2::ZERO)], Bodies::default(), falling, 20_000),
        ),
    ]
}

fn main() {
    let only = std::env::args().nth(1).unwrap_or_default().to_lowercase();
    let seconds = 10.0;
    for (name, mut scene) in cases() {
        if !only.is_empty() && !name.to_lowercase().contains(&only) {
            continue;
        }
        scene.video.duration = seconds as f32;
        let mut sim = Simulation::new(&scene);
        // Room for any frame's events, so only the physics' own allocations are counted.
        let mut events = Vec::with_capacity(1 << 16);
        // One second to settle in and grow its buffers, then time the rest.
        sim.advance_to(1.0, &mut events);
        let frames = ((seconds - 1.0) * 60.0) as u32;
        let mut worst = 0.0f64;
        let start = std::time::Instant::now();
        let before = ALLOCATIONS.load(Ordering::Relaxed);
        for frame in 1..=frames {
            events.clear();
            let t = std::time::Instant::now();
            sim.advance_to(1.0 + frame as f64 / 60.0, &mut events);
            worst = worst.max(t.elapsed().as_secs_f64());
        }
        let allocations = ALLOCATIONS.load(Ordering::Relaxed) - before;
        let mean = start.elapsed().as_secs_f64() / frames as f64;
        let fastest = sim.balls().iter().filter(|b| b.alive).map(|b| b.velocity.length()).fold(0.0f32, f32::max);
        let balls = sim.balls().iter().filter(|b| b.alive).count();
        let asleep = (0..sim.balls().len()).filter(|&i| sim.ball_asleep(i)).count();
        println!(
            "{name}: {:.2} ms a frame (worst {:.2}), {:.2} allocations a frame, {balls} balls ({asleep} asleep), fastest ball at the end {fastest:.1} px/s",
            mean * 1000.0,
            worst * 1000.0,
            allocations as f64 / frames as f64,
        );
    }
}

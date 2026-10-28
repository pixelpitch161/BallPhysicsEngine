//! Prints a line per second of how each preset's balls and soft bodies are doing, for tuning.
//! Run with `cargo run --release -p bpe-sim --example probe [preset name]`.

use bpe_scene::Scene;
use bpe_sim::{EventKind, Simulation};

fn main() {
    let name = std::env::args().nth(1).unwrap_or_default().to_lowercase();
    for preset in Scene::presets() {
        if !name.is_empty() && !preset.name.to_lowercase().contains(&name) {
            continue;
        }
        let scene = preset.scene;
        let mut sim = Simulation::new(&scene);
        let mut events = Vec::new();
        let start = std::time::Instant::now();
        for second in 1..=scene.video.duration as u32 {
            sim.advance_to(second as f64, &mut events);
            let soft = sim.soft();
            // Height over width of the first few soft bodies: 1 is round, less is squashed flat.
            let shape: Vec<String> = soft
                .bodies
                .iter()
                .take(3)
                .map(|b| {
                    let pts = &soft.points[b.points.start as usize..b.points.end as usize];
                    let (lo, hi) = pts.iter().fold((bpe_scene::Vec2::MAX, bpe_scene::Vec2::MIN), |(lo, hi), p| {
                        (lo.min(p.position), hi.max(p.position))
                    });
                    let size = hi - lo;
                    format!("{:.2}", size.y / size.x.max(1e-3))
                })
                .collect();
            let fastest = sim.balls().iter().map(|b| b.velocity.length()).fold(0.0f32, f32::max);
            let asleep = (0..sim.balls().len()).filter(|&i| sim.ball_asleep(i)).count();
            let nan = sim.balls().iter().any(|b| !b.position.is_finite())
                || soft.points.iter().any(|p| !p.position.is_finite());
            let lowest = soft.points.iter().map(|p| p.position.y).fold(f32::MIN, f32::max);
            let count = |f: fn(&EventKind) -> bool| events.iter().filter(|e| f(&e.kind)).count();
            println!(
                "{} t={second} balls={} asleep={asleep} fastest={fastest:.0} shape={shape:?} lowest={lowest:.0} nan={nan} walls={} ball={} soft={} escapes={} alive={}",
                preset.name,
                sim.balls().len(),
                count(|k| matches!(k, EventKind::Wall)),
                count(|k| matches!(k, EventKind::Ball(_))),
                count(|k| matches!(k, EventKind::Soft { .. })),
                count(|k| matches!(k, EventKind::Escape(_))),
                sim.balls().iter().filter(|b| b.alive).count(),
            );
        }
        println!("{}: {:.2?} for {} s", preset.name, start.elapsed(), scene.video.duration);
    }
}

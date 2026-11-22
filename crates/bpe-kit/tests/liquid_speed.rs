//! How long the liquid takes on the graphics card: each liquid test scene played for a few seconds,
//! the time per second of video and the drops at the end printed. A timing to read, not a pass/fail
//! test; CI prints it on the owner's card on every run (job "Pictures (owner's graphics card)").
//!
//! Ignored, so `cargo test` only compiles it. Run it with
//! `cargo test --release -p bpe-kit --test liquid_speed -- --ignored --nocapture`.

use std::time::Instant;

use bpe_gfx::swarm::{SwarmGpu, use_for_now};
use bpe_kit::things::liquid::state_of;
use bpe_kit::{kit, scenes};
use bpe_runtime::Playback;

const SECONDS: u32 = 4;

#[test]
#[ignore = "a timing, run on purpose"]
fn liquid_time_per_second_of_video() {
    let Ok((device, queue)) = bpe_gfx::test_device() else { return };
    let _card = use_for_now(Some(SwarmGpu::for_device(&device, &queue)));
    println!("card: {}", bpe_gfx::test_device_name());
    let kit = kit();
    for (name, scene) in scenes::liquid_scenes(&kit.registry) {
        let resolved = bpe_kit::resolve_scene(&scene, &kit.registry);
        let id = scene.order.iter().copied().find(|t| scene.things[t].kind == "liquid").expect("a liquid");
        let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
        // The first second warms up (programs build, drops are born); the rest is timed.
        playback.seek(1.0);
        let start = Instant::now();
        playback.seek(SECONDS as f64);
        let ms = start.elapsed().as_secs_f64() * 1000.0 / (SECONDS - 1) as f64;
        let drops = state_of(playback.world(), id)
            .and_then(|st| st.host.as_ref().and_then(|h| h.body.as_ref()).map(|b| b.read().len()))
            .unwrap_or(0);
        println!("{name}: {ms:.0} ms per second of video, {drops} drops");
    }
}

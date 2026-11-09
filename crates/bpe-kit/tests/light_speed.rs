//! How long lit frames take to draw: a few lighting scenes drawn again and again at a set size, the
//! mean time a frame printed. For measuring the lighting's speed before and after a change (review
//! workstream W8), not a pass/fail test.
//!
//! Ignored, so `cargo test` only compiles it. Run it with
//! `cargo test --release -p bpe-kit --test light_speed -- --ignored --nocapture`.
//!
//! Environment: `SPEED_SIZE` (`WxH`, default `540x960`), `SPEED_FRAMES` (frames timed after two to
//! warm up, default 6), `SPEED_ONLY` (a comma list of scene names), `SPEED_DUMP` (a folder: each
//! scene's last picture is written there as raw RGBA, `<name>.rgba`, to compare two builds byte for
//! byte).

use bpe_core::{Registry, Scene, Settings, SlotId, ThingId, Value};
use bpe_gfx::{Frame, Renderer, build_frame, test_device};
use bpe_kit::things::balls::BallsSettings;
use bpe_kit::{kit, scenes};
use bpe_runtime::Playback;

/// The lighting demo with `n` balls in a ring, each carrying a light that reaches `reach`.
fn many_lights(reg: &Registry, n: u32, reach: f64) -> Scene {
    let mut scene = scenes::lit_demo(reg);
    let ids: Vec<ThingId> = scene.things.values().filter(|t| t.kind == "light").map(|t| t.id).collect();
    // The second light is the one on each ball.
    scene.things.get_mut(&ids[1]).expect("light").params.set("reach", Value::Number(reach));
    let ball = scene.things.values().find(|t| t.kind == "balls").map(|t| t.id).expect("a ball");
    let mut balls = BallsSettings::from_params(&scene.things[&ball].params);
    balls.ring(n, 300.0);
    balls.radius = 12.0;
    scene.things.get_mut(&ball).expect("ball").params = balls.to_params();
    scene
}

fn stage_of(scene: Scene) -> Scene {
    with(scene, &[("depth", Value::Choice("stage".into()))])
}

/// `scene` with its Lighting card's `values` set.
fn with(mut scene: Scene, values: &[(&str, Value)]) -> Scene {
    if let Some(card) = scene.card_mut(ThingId::ROOT, SlotId::Lighting) {
        for (k, v) in values {
            card.params.set(k, v.clone());
        }
    }
    scene
}

#[test]
#[ignore = "measures drawing speed; run with --ignored"]
fn lit_frame_speed() {
    let reg = kit().registry;
    let (w, h) = std::env::var("SPEED_SIZE")
        .ok()
        .and_then(|s| s.split_once('x').and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?))))
        .unwrap_or((540u32, 960u32));
    let frames: usize = std::env::var("SPEED_FRAMES").ok().and_then(|s| s.parse().ok()).unwrap_or(6);
    let only = std::env::var("SPEED_ONLY").ok();
    let shots: Vec<(&str, Scene)> = vec![
        ("room", scenes::lit_room(&reg)),
        ("demo", scenes::lit_demo(&reg)),
        ("lights-120", many_lights(&reg, 120, 320.0)),
        ("lights-120-near", many_lights(&reg, 120, 90.0)),
        ("lights-120-stage", stage_of(many_lights(&reg, 120, 320.0))),
        ("stage", scenes::stage_demo(&reg)),
        ("surfaces", scenes::surfaces_demo(&reg)),
        ("bake", scenes::bake_demo(&reg)),
        ("light-show", scenes::light_show(&reg)),
        ("stage-haze", with(scenes::stage_demo(&reg), &[("haze", Value::Number(0.4))])),
        (
            "stage-glow",
            with(scenes::stage_demo(&reg), &[("glow_dark", Value::Number(1.0)), ("bounce", Value::Number(0.6))]),
        ),
        (
            "demo-rainbow",
            with(scenes::lit_demo(&reg), &[("rainbow", Value::Number(1.0)), ("haze", Value::Number(0.3))]),
        ),
    ];
    let (device, queue) = test_device().expect("a graphics device");
    let k = kit();
    for (name, scene) in shots {
        if only.as_ref().is_some_and(|o| !o.split(',').any(|n| n == name)) {
            continue;
        }
        let resolved = bpe_core::resolve(&scene, &k.registry);
        let mut playback = Playback::new(&scene, &resolved, k.sims.clone());
        playback.seek(0.5);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 0.5, &k.draws, &mut frame);
        let lights = frame.light.as_ref().map_or(0, |l| l.lights.len());
        let mut renderer = Renderer::new(&device, w, h);
        let draw = |r: &mut Renderer| {
            r.render(&device, &queue, &frame);
            let _ = device.poll(bpe_gfx::wgpu::PollType::wait_indefinitely());
        };
        for _ in 0..2 {
            draw(&mut renderer);
            while renderer.programs_waiting() {
                draw(&mut renderer);
            }
        }
        let start = std::time::Instant::now();
        for _ in 0..frames {
            draw(&mut renderer);
        }
        let ms = start.elapsed().as_secs_f64() * 1000.0 / frames as f64;
        if let Ok(dir) = std::env::var("SPEED_DUMP") {
            let pixels = renderer.read_output(&device, &queue).expect("the picture");
            std::fs::create_dir_all(&dir).expect("the dump folder");
            std::fs::write(std::path::Path::new(&dir).join(format!("{name}.rgba")), pixels).expect("written");
        }
        eprintln!("{name:>18}: {ms:8.1} ms a frame ({lights} lights, {w}x{h})");
    }
}

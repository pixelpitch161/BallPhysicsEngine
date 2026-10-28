//! Timing and fingerprinting of every stage of making a video, for `bpe-render --profile`.
//!
//! Each scene is simulated, mixed and drawn the way the editor and the exporter do it, and the
//! time each stage takes is printed. A fingerprint of the hits, the soundtrack and the frames is
//! printed with it, so a speed-up can be checked to give exactly the same video as before.

use bpe_render::{EffectLibrary, FrameData, Renderer, wgpu};
use bpe_scene::{Color, Ease, Effect, Key, Property, Scene, Spawner, Track, Value};
use bpe_sim::{Event, Playback};
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

/// Built-in presets plus a few scenes that push one part of the engine hard.
pub fn heavy_scenes() -> Vec<(String, Scene)> {
    let mut scenes: Vec<(String, Scene)> =
        Scene::presets().into_iter().map(|preset| (preset.name.to_string(), preset.scene)).collect();

    let mut swarm = Scene::default();
    if let Spawner::Grid { columns, rows, spacing, .. } = &mut swarm.spawners[0] {
        (*columns, *rows, *spacing) = (50, 50, 11.0);
    }
    scenes.push(("Swarm of 2500".into(), swarm));

    let mut crowd = Scene::default();
    crowd.physics.ball_collisions = true;
    scenes.push(("Reference reel, balls collide".into(), crowd));

    let mut plinko = Scene::plinko();
    plinko.rules.limit_speed = false;
    plinko.rules.speed_gain = 1.05;
    scenes.push(("Plinko, no speed limit".into(), plinko));

    let mut rings = Scene::escape_and_multiply();
    rings.rules.limit_speed = false;
    rings.rules.speed_gain = 1.04;
    scenes.push(("Escape and multiply, no speed limit".into(), rings));

    let effects = Scene {
        effects: ["echo", "kaleidoscope", "shockwave", "color_split", "film", "glitch"].map(Effect::new).to_vec(),
        ..Scene::default()
    };
    scenes.push(("Reference reel, 6 effects".into(), effects));

    let mut keyed = Scene::multiplying_balls();
    let key = |time, value| Key { time, value: Value::Number(value), ease: Ease::Smooth };
    keyed.animation.push(Track { property: Property::Gravity, keys: vec![key(0.0, 600.0), key(10.0, -300.0)] });
    keyed
        .animation
        .push(Track { property: Property::ArenaSize, keys: vec![key(0.0, 1.0), key(8.0, 0.6), key(16.0, 1.1)] });
    keyed.animation.push(Track {
        property: Property::BackgroundTop,
        keys: vec![
            Key { time: 0.0, value: Value::Color(Color::rgb(10, 10, 40)), ease: Ease::Linear },
            Key { time: 20.0, value: Value::Color(Color::rgb(90, 10, 60)), ease: Ease::Linear },
        ],
    });
    scenes.push(("Multiplying balls, keyframes".into(), keyed));
    scenes
}

/// How long each stage took for one scene, and what it made.
pub struct Report {
    pub name: String,
    pub balls: usize,
    pub hits: usize,
    pub simulate: Duration,
    pub sound: Duration,
    /// Stepping the simulation and building every frame's shapes, as the exporter does.
    pub frames: Duration,
    pub frame_count: u32,
    /// The slowest single frame of `frames`.
    pub slowest_frame: Duration,
    /// Jumping back 10% from the end, as when scrubbing the timeline backwards.
    pub scrub_back: Duration,
    /// Drawing and reading back `gpu_frames` frames.
    pub gpu: Option<Duration>,
    pub gpu_frames: u32,
    pub fingerprint: Fingerprint,
}

/// Hashes of everything a scene produces, to check that two builds make the same video.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fingerprint {
    pub hits: u64,
    pub sound: u64,
    pub frames: u64,
    pub pixels: Option<u64>,
}

/// Profiles one scene. With a device, `gpu_frames` evenly spaced frames are also drawn at
/// `scale` and read back.
pub fn profile_scene(
    name: &str,
    scene: &Scene,
    gpu: Option<(&wgpu::Device, &wgpu::Queue)>,
    scale: f32,
    gpu_frames: u32,
) -> anyhow::Result<Report> {
    let started = Instant::now();
    let hits = bpe_sim::collect_wall_hits(scene);
    let simulate = started.elapsed();

    let started = Instant::now();
    let soundtrack = bpe_audio::render_soundtrack(scene, &hits, None)?;
    let sound = started.elapsed();

    let fps = scene.video.fps.max(1) as f64;
    let frame_count = scene.frame_count();
    let pixel = scene.video.width as f32 / crate::output_size(scene, scale).0 as f32;
    let mut playback = Playback::new(scene);
    let mut frame = FrameData::default();
    let mut frames_hash = hasher();
    let mut slowest_frame = Duration::ZERO;
    let started = Instant::now();
    for index in 0..frame_count {
        let begun = Instant::now();
        playback.seek(index as f64 / fps);
        frame.fill(scene, &playback, pixel);
        slowest_frame = slowest_frame.max(begun.elapsed());
        if index % 10 == 0 {
            hash_frame(&frame, &mut frames_hash);
        }
    }
    let frames = started.elapsed();
    let balls = playback.space().map_or(playback.simulation().balls().len(), |space| space.balls().len()); // 3D (WS4)

    let started = Instant::now();
    playback.seek(scene.video.duration as f64 * 0.9);
    let scrub_back = started.elapsed();

    let mut pixels_hash = None;
    let mut gpu_time = None;
    if let Some((device, queue)) = gpu {
        let (width, height) = crate::output_size(scene, scale);
        let mut renderer = Renderer::new(device, width, height);
        let library = EffectLibrary::new();
        library.sync_graphs(&scene.shader_graphs);
        renderer.set_effect_library(library);
        let mut playback = Playback::new(scene);
        let mut hash = hasher();
        let started = Instant::now();
        let step = (frame_count / gpu_frames.max(1)).max(1);
        for index in (0..frame_count).step_by(step as usize).take(gpu_frames as usize) {
            let time = index as f64 / fps;
            playback.seek(time);
            frame.fill(scene, &playback, renderer.pixel_size(scene));
            renderer.render(device, queue, scene, time, &frame);
            renderer.read_output(device, queue)?.hash(&mut hash);
        }
        gpu_time = Some(started.elapsed());
        pixels_hash = Some(hash.finish());
    }

    Ok(Report {
        name: name.to_string(),
        balls,
        hits: hits.iter().filter(|e| e.kind.is_bounce()).count(),
        simulate,
        sound,
        frames,
        frame_count,
        slowest_frame,
        scrub_back,
        gpu: gpu_time,
        gpu_frames,
        fingerprint: Fingerprint {
            hits: hash_hits(&hits),
            sound: hash_floats(&soundtrack.samples),
            frames: frames_hash.finish(),
            pixels: pixels_hash,
        },
    })
}

impl Report {
    pub fn header() -> String {
        format!(
            "{:38} {:>5} {:>7} {:>9} {:>9} {:>9} {:>9} {:>9} {:>11}  fingerprint",
            "scene", "balls", "hits", "sim ms", "sound ms", "frames ms", "worst ms", "scrub ms", "gpu ms/frame"
        )
    }

    pub fn row(&self) -> String {
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        let gpu = self.gpu.map_or("-".to_string(), |d| format!("{:.1}", ms(d) / self.gpu_frames.max(1) as f64));
        let f = &self.fingerprint;
        format!(
            "{:38} {:>5} {:>7} {:>9.0} {:>9.0} {:>9.0} {:>9.1} {:>9.0} {:>11}  {:016x}-{:016x}-{:016x}-{}",
            self.name,
            self.balls,
            self.hits,
            ms(self.simulate),
            ms(self.sound),
            ms(self.frames),
            ms(self.slowest_frame),
            ms(self.scrub_back),
            gpu,
            f.hits,
            f.sound,
            f.frames,
            f.pixels.map_or("-".to_string(), |p| format!("{p:016x}")),
        )
    }
}

fn hasher() -> std::collections::hash_map::DefaultHasher {
    std::collections::hash_map::DefaultHasher::new()
}

/// Hashes the bounces only, so fingerprints stay comparable with logs from before other events
/// (spawns, beats, rule actions) were recorded.
fn hash_hits(hits: &[Event]) -> u64 {
    let mut hash = hasher();
    for hit in hits.iter().filter(|e| e.kind.is_bounce()) {
        hit.time.to_bits().hash(&mut hash);
        hit.ball.hash(&mut hash);
        hit.position.x.to_bits().hash(&mut hash);
        hit.position.y.to_bits().hash(&mut hash);
        hit.speed.to_bits().hash(&mut hash);
        format!("{:?}", hit.kind).hash(&mut hash);
    }
    hash.finish()
}

fn hash_floats(values: &[f32]) -> u64 {
    let mut hash = hasher();
    values.iter().for_each(|v| v.to_bits().hash(&mut hash));
    hash.finish()
}

fn hash_frame(frame: &FrameData, hash: &mut impl Hasher) {
    bytemuck::cast_slice::<_, u8>(&frame.balls).hash(hash);
    bytemuck::cast_slice::<_, u8>(&frame.trails).hash(hash);
    bytemuck::cast_slice::<_, u8>(&frame.flashes).hash(hash);
    bytemuck::cast_slice::<_, u8>(&frame.shapes).hash(hash);
    for hit in &frame.hits {
        hit.position.iter().chain([&hit.age, &hit.strength]).chain(&hit.color).for_each(|v| v.to_bits().hash(hash));
    }
    frame.pulse.to_bits().hash(hash);
}

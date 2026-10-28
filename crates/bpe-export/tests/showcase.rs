//! The integration pass: two showcase scenes that use every feature at once, and tests that
//! each feature still does its job when all of them run together.
//!
//! "Everything at once" is flat: a heart arena with a star inside, balls that bump into each
//! other, jelly, a rope and a pool of liquid, a GPU swarm, hit particles, rules, a node shader
//! that pulses with the music, a MIDI melody on a marimba over a drum kit locked to the beat,
//! and animated captions. "Everything in 3D" does the same in a glass cube, minus the flat-only
//! physics.
//!
//! The scenes are saved in `scenes/showcase/` and shown in the preset gallery. After changing
//! them here, run with `BPE_BLESS=1` to rewrite the files.

use bpe_render::{FrameData, Renderer};
use bpe_scene::logic::{Graph, Link, Node, Param};
use bpe_scene::*;
use bpe_sim::rules::RuleOutput;
use bpe_sim::{EventKind, Playback};
use std::path::PathBuf;

fn showcase_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scenes/showcase")
}

/// A rule graph: blocks as (id, kind, settings), wired output `then` to input `does`.
/// A block: its id, kind and settings.
type Block<'a> = (u32, &'a str, &'a [(&'a str, Param)]);

fn rule(name: &str, blocks: &[Block], wires: &[(u32, u32)]) -> Graph {
    let mut graph = Graph::new(name);
    graph.nodes = blocks
        .iter()
        .enumerate()
        .map(|(column, (id, kind, params))| Node {
            id: *id,
            kind: (*kind).into(),
            pos: [column as f32 * 240.0, 0.0],
            params: params.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(),
            enabled: true,
            ..Node::default()
        })
        .collect();
    graph.links = wires.iter().map(|&(from, to)| Link { from: (from, 0), to: (to, 0) }).collect();
    graph
}

fn num(value: f32) -> Param {
    Param::Number(value)
}

fn colour(c: Color) -> Param {
    Param::Color(c.to_linear())
}

/// Rules shared by both showcase scenes.
fn rules() -> Vec<Graph> {
    vec![
        // Every 5th bounce off the wall splits the ball and bursts confetti.
        rule(
            "Split and celebrate",
            &[
                (1, "wall", &[]),
                (2, "every_nth", &[("times", num(5.0))]),
                (3, "spawn", &[("count", num(1.0)), ("spread", num(60.0))]),
                (4, "confetti", &[("count", num(80.0)), ("color", colour(Color::rgb(0xff, 0xd8, 0x2e)))]),
            ],
            &[(1, 2), (2, 3), (3, 4)],
        ),
        // Every wall hit adds to the score shown in a caption.
        rule(
            "Keep score",
            &[(1, "wall", &[]), (2, "counter_add", &[("counter", Param::Text("score".into()))])],
            &[(1, 2)],
        ),
        // A pink flash on the first beat of each bar.
        rule(
            "Flash on the bar",
            &[
                (1, "beat", &[("downbeats", Param::Bool(true))]),
                (2, "flash", &[("color", colour(Color::rgb(0xff, 0x3e, 0x8e))), ("strength", num(0.35))]),
            ],
            &[(1, 2)],
        ),
        // Halfway through, gravity flips sideways for a moment of chaos.
        rule(
            "Halfway twist",
            &[
                (1, "at", &[("time", num(10.0))]),
                (2, "caption", &[("text", Param::Text("CHAOS!".into())), ("seconds", num(1.5))]),
                (3, "gravity", &[("mode", Param::Choice("add".into())), ("amount", num(-250.0))]),
                (4, "confetti", &[("count", num(300.0)), ("color", colour(Color::rgb(0x2e, 0xb8, 0xff)))]),
            ],
            &[(1, 2), (2, 3), (3, 4)],
        ),
    ]
}

/// A node shader made from a built-in one: kaleidoscope slices that kick on the beat.
fn shader_graph() -> Graph {
    let mut graph = bpe_render::effects::templates::graphs()
        .into_iter()
        .find(|g| g.name == "Beat kaleido")
        .expect("Beat kaleido comes with the program");
    graph.name = "Showcase kaleido".into();
    graph
}

fn effects() -> Vec<Effect> {
    vec![
        Effect { amount: 0.2, ..Effect::new("graph:Showcase kaleido") },
        Effect { amount: 0.6, ..Effect::new("graph:Treble sparkle") },
    ]
}

fn modulators(extra: Vec<Modulator>) -> Vec<Modulator> {
    let mut list = vec![
        Modulator { property: Property::Bloom, source: AudioBand::Bass, amount: 0.4, ..Modulator::default() },
        Modulator { property: Property::BallGlow, source: AudioBand::Treble, amount: 0.5, ..Modulator::default() },
    ];
    list.extend(extra);
    list
}

fn sound() -> Sound {
    Sound {
        instrument: Instrument::library("Marimba"),
        full_volume_speed: 1100.0,
        voices: vec![
            VoiceMap {
                on: NoteTrigger::Beats,
                instrument: Instrument::library("Drum kit"),
                gain: -12.0,
                pitch: PitchMode::Fixed(42),
                enabled: true,
            },
            VoiceMap {
                on: NoteTrigger::Downbeats,
                instrument: Instrument::library("Drum kit"),
                gain: -6.0,
                pitch: PitchMode::Fixed(36),
                enabled: true,
            },
        ],
        ..Sound::default()
    }
}

fn music() -> Music {
    Music {
        bpm: 120.0,
        beat_sync: BeatSync { mode: SyncMode::Lock, subdivisions: 2, ..BeatSync::default() },
        melody: Some(Melody { path: "builtin:ode-to-joy".into(), ..Melody::default() }),
        ..Music::default()
    }
}

fn captions() -> Vec<Caption> {
    let mut hook = Caption::new("Can the heart hold them all?", CaptionStyle::Hook);
    hook.anim_in = Anim::Pop;
    hook.show_to = 6.0;
    let mut balls = Caption::new("{balls} balls  ·  score {counter:score}", CaptionStyle::Counter);
    balls.react = React::PulseOnHit;
    let mut timer = Caption::new("{countdown}", CaptionStyle::Timer);
    timer.react = React::PulseOnBeat;
    vec![hook, balls, timer]
}

/// Every flat feature in one heart-shaped arena.
pub fn everything() -> Scene {
    let heart = Shape::heart();
    let star = Shape::star(5, 0.45);
    let mut sparks = ParticlePreset::Sparks.emitter(EventMatch::AnyHit);
    sparks.count = 14;
    let mut swarm =
        Swarm { enabled: true, count: 200_000, brightness: 0.05, color: SwarmColor::NearestBall, ..Swarm::default() };
    swarm.attraction *= 0.5;
    Scene {
        name: "Everything at once".into(),
        container: Container::Custom {
            shape: heart.name.clone(),
            center: Vec2::new(0.0, 60.0),
            radius: 500.0,
            rotation: 0.0,
            thickness: 5.0,
            color: Color::rgb(0xff, 0x6a, 0x9c),
        },
        obstacles: vec![Obstacle::Shape {
            shape: star.name.clone(),
            position: Vec2::new(0.0, 40.0),
            rotation: 0.0,
            scale: 70.0,
            thickness: 4.0,
            color: Color::rgb(0xff, 0xd8, 0x2e),
        }],
        shapes: vec![heart, star],
        spawners: vec![Spawner::Grid {
            center: Vec2::new(0.0, -170.0),
            columns: 4,
            rows: 3,
            spacing: 40.0,
            radius: 13.0,
            velocity: Vec2::new(-520.0, -380.0),
            gradient: vec![Color::rgb(0xff, 0x3e, 0x8e), Color::rgb(0xff, 0xb8, 0x2e), Color::rgb(0x2e, 0xb8, 0xff)],
            depth: 0.0,
            depth_velocity: 0.0,
        }],
        physics: Physics {
            gravity: Vec2::new(0.0, 500.0),
            ball_collisions: true,
            contact: BallContact { quality: 4, friction: 0.2, spin: true, ..BallContact::default() },
            ..Physics::default()
        },
        // A little speed on every wall hit keeps the balls lively among the soft things.
        rules: BounceRules { max_balls: 80, speed_gain: 1.03, max_speed: 1600.0, ..BounceRules::default() },
        bodies: Bodies {
            jelly: vec![Jelly {
                center: Vec2::new(-160.0, 150.0),
                columns: 2,
                rows: 1,
                spacing: 120.0,
                radius: 45.0,
                velocity: Vec2::new(150.0, -200.0),
                points: 20,
                squishiness: 0.6,
                gradient: vec![Color::rgb(0x5c, 0xe6, 0x5c), Color::rgb(0x9a, 0x5c, 0xff)],
            }],
            ropes: vec![Rope {
                from: Vec2::new(-260.0, 280.0),
                to: Vec2::new(260.0, 280.0),
                points: 30,
                slack: 1.0,
                stiffness: 1.0,
                color: Color::rgb(0xd8, 0xdc, 0xd0),
                ..Rope::default()
            }],
            fluid: Fluid {
                count: 350,
                center: Vec2::new(0.0, 380.0),
                size: Vec2::new(200.0, 60.0),
                radius: 6.0,
                ..Fluid::default()
            },
            ..Bodies::default()
        },
        gpu: GpuLayers { swarm, ..GpuLayers::default() },
        particles: vec![sparks, ParticlePreset::Rings.emitter(EventMatch::Soft)],
        look: Look {
            background_top: Color::rgb(0x2a, 0x02, 0x1a),
            background_bottom: Color::rgb(0x06, 0x00, 0x04),
            trail_seconds: 0.35,
            ball_glow: 0.3,
            bloom_strength: 0.3,
            ..Look::default()
        },
        logic: rules(),
        effects: effects(),
        shader_graphs: vec![shader_graph()],
        modulators: modulators(Vec::new()),
        sound: sound(),
        music: music(),
        captions: captions(),
        video: Video { duration: 20.0, ..Video::default() },
        ..Scene::default()
    }
}

/// The same ideas in a glass cube: rules, particles, node shaders, music and captions in 3D.
pub fn everything_3d() -> Scene {
    let mut scene = Scene::neon_cube();
    scene.name = "Everything in 3D".into();
    scene.physics.gravity = Vec2::new(0.0, 250.0);
    scene.rules.max_balls = 60;
    scene.logic = rules();
    scene.effects = effects();
    scene.shader_graphs = vec![shader_graph()];
    scene.modulators = modulators(vec![Modulator {
        property: Property::EdgeGlow,
        source: AudioBand::Bass,
        amount: 2.0,
        ..Modulator::default()
    }]);
    let mut sparks = ParticlePreset::Sparks.emitter(EventMatch::Wall);
    sparks.count = 12;
    scene.particles = vec![sparks];
    scene.sound = sound();
    scene.music = music();
    scene.captions = captions();
    scene.captions[0].text = "Can the cube hold them all?".into();
    scene.video.duration = 20.0;
    scene
}

fn showcases() -> [(&'static str, Scene); 2] {
    [("everything.ron", everything()), ("everything-3d.ron", everything_3d())]
}

#[test]
fn showcase_files_are_current() {
    for (file, scene) in showcases() {
        let path = showcase_dir().join(file);
        let text = scene.to_ron().unwrap();
        if std::env::var_os("BPE_BLESS").is_some() {
            std::fs::create_dir_all(showcase_dir()).unwrap();
            std::fs::write(&path, &text).unwrap();
            continue;
        }
        let saved = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
        assert!(saved == text, "{file} is out of date: run this test with BPE_BLESS=1");
        assert_eq!(Scene::from_ron(&saved).unwrap(), scene, "{file} doesn't load back the same");
    }
}

#[test]
fn showcases_are_in_the_gallery() {
    let presets = Scene::presets();
    for (_, scene) in showcases() {
        let preset = presets.iter().find(|p| p.name == scene.name).unwrap_or_else(|| panic!("{} missing", scene.name));
        assert_eq!(preset.scene, scene);
    }
}

/// What a whole run of a scene did, by kind of event and rule action.
#[derive(Debug, Default)]
struct Tally {
    walls: usize,
    obstacles: usize,
    balls: usize,
    soft: usize,
    spawns: usize,
    beats: usize,
    flashes: usize,
    confetti: usize,
    captions: usize,
    counters: usize,
    most_balls: usize,
}

fn run(scene: &Scene) -> (Playback, Tally) {
    let mut playback = Playback::with_inputs(scene, &bpe_sim::beat_sync::inputs(scene, None));
    let mut tally = Tally::default();
    let mut t = 0.0;
    while t < scene.video.duration as f64 {
        t += 0.25;
        playback.seek(t);
        let alive = match playback.space() {
            Some(space) => space.alive_count(),
            None => playback.simulation().balls().iter().filter(|b| b.alive).count(),
        };
        tally.most_balls = tally.most_balls.max(alive);
    }
    let rules = playback.simulation().rules();
    for event in playback.events() {
        match event.kind {
            EventKind::Wall => tally.walls += 1,
            EventKind::Obstacle(_) => tally.obstacles += 1,
            EventKind::Ball(_) => tally.balls += 1,
            EventKind::Soft { .. } => tally.soft += 1,
            EventKind::Spawn => tally.spawns += 1,
            EventKind::Beat { .. } => tally.beats += 1,
            EventKind::Action { .. } => match rules.output(event) {
                Some(RuleOutput::Flash { .. }) => tally.flashes += 1,
                Some(RuleOutput::Confetti { .. }) => tally.confetti += 1,
                Some(RuleOutput::Caption { .. }) => tally.captions += 1,
                Some(RuleOutput::Counter { .. }) => tally.counters += 1,
                _ => {}
            },
            _ => {}
        }
    }
    (playback, tally)
}

#[test]
fn every_flat_feature_takes_part() {
    let scene = everything();
    let (playback, tally) = run(&scene);
    eprintln!("{tally:?}");
    assert!(tally.walls > 20 && tally.obstacles > 0, "walls and the star are hit: {tally:?}");
    assert!(tally.balls > 0, "balls bump into each other: {tally:?}");
    assert!(tally.soft > 0, "jelly, rope or liquid is touched: {tally:?}");
    assert!(tally.spawns > 0 && tally.most_balls > 12, "the split rule adds balls: {tally:?}");
    assert!(tally.most_balls <= 80, "the ball limit holds: {tally:?}");
    assert!(tally.beats >= 35, "beats at 120 bpm: {tally:?}");
    assert!(tally.flashes >= 8, "a flash every bar: {tally:?}");
    assert!(tally.confetti > 1 && tally.captions == 1, "confetti and the CHAOS caption: {tally:?}");
    assert!(tally.counters > 20, "the score counts: {tally:?}");
    let rules = playback.simulation().rules();
    let score = rules.counter_names().iter().position(|n| n == "score").map(|i| rules.counters()[i]);
    assert_eq!(score, Some(tally.counters as f64));
}

#[test]
fn every_3d_feature_takes_part() {
    let scene = everything_3d();
    let (playback, tally) = run(&scene);
    eprintln!("{tally:?}");
    assert!(playback.space().is_some());
    assert!(tally.walls > 20, "balls hit the cube: {tally:?}");
    assert!(tally.spawns > 0 && tally.most_balls > 6, "rules fire on 3D bounces: {tally:?}");
    assert!(tally.most_balls <= 60, "the ball limit holds in 3D: {tally:?}");
    assert!(tally.flashes >= 8 && tally.confetti > 1 && tally.captions == 1, "{tally:?}");
}

#[test]
fn showcases_sound_right() {
    for (_, scene) in showcases() {
        let (playback, _) = run(&scene);
        let samples = bpe_audio::render_soundtrack(&scene, playback.events(), None).unwrap().samples;
        let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
        eprintln!("{}: peak {peak:.3}, rms {rms:.4}", scene.name);
        assert!(samples.iter().all(|s| s.is_finite()));
        assert!(peak <= 1.0 && rms > 0.01, "{}: peak {peak}, rms {rms}", scene.name);
    }
}

#[test]
fn showcases_score_and_vary() {
    for (_, scene) in showcases() {
        let score = bpe_export::score::score_scene(&scene, &Default::default());
        eprintln!("{}: {score:?}", scene.name);
        assert!(score.total.is_finite() && score.total > 0.0);
        let library = bpe_render::EffectLibrary::new();
        let vary = bpe_export::batch::Vary::parse("all").unwrap();
        for seed in 1..4 {
            let (variation, _) = bpe_export::batch::variation(&scene, &vary, seed, 0.5, &library, &[]);
            let mut playback = Playback::new(&variation);
            playback.seek(3.0);
        }
    }
}

fn gpu() -> Option<(bpe_render::wgpu::Device, bpe_render::wgpu::Queue)> {
    match bpe_render::headless_device() {
        Ok(device) => Some(device),
        Err(error) if std::env::var_os("BPE_REQUIRE_GPU").is_some() => panic!("no GPU adapter: {error}"),
        Err(_) => {
            eprintln!("no GPU adapter available, skipping");
            None
        }
    }
}

/// Draws a few moments of each showcase and checks the pictures are lively and that the
/// rule's confetti and the captions are in them.
#[test]
fn showcases_draw() {
    let Some((device, queue)) = gpu() else { return };
    for (_, scene) in showcases() {
        let mut renderer = Renderer::new(&device, 270, 480);
        let mut playback = Playback::with_inputs(&scene, &bpe_sim::beat_sync::inputs(&scene, None));
        let mut confetti = 0;
        let mut captions = 0;
        for time in [0.5, 4.0, 10.2, 15.0] {
            playback.seek(time);
            let frame = FrameData::build(&scene, &playback, renderer.pixel_size(&scene));
            confetti += frame.particles.bursts.len();
            captions += frame.text.rule_captions.len();
            renderer.render(&device, &queue, &scene, playback.simulation().time(), &frame);
            let pixels = renderer.read_output(&device, &queue).unwrap();
            let lit = pixels.chunks(4).filter(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 120).count();
            assert!(lit > pixels.len() / 4 / 200, "{} at {time}s is nearly black", scene.name);
        }
        assert!(confetti > 0, "{} draws particles", scene.name);
        assert!(captions > 0, "{} shows the rule's CHAOS caption", scene.name);
    }
}

/// No frame of either showcase holds up the preview. Beat lock used to move the jelly and the
/// liquid along with every guess it made, which took a quarter of a second per bounce.
#[test]
fn showcases_play_without_stutters() {
    for (_, scene) in showcases() {
        let mut playback = Playback::with_inputs(&scene, &bpe_sim::beat_sync::inputs(&scene, None));
        let mut frame = FrameData::default();
        let fps = scene.video.fps as f64;
        let mut times = Vec::new();
        for index in 0..scene.frame_count() {
            let begun = std::time::Instant::now();
            playback.seek(index as f64 / fps);
            let stepped = begun.elapsed();
            frame.fill(&scene, &playback, 1.0);
            times.push((begun.elapsed(), stepped, index));
        }
        times.sort_by_key(|t| std::cmp::Reverse(t.0));
        eprintln!("{}: slowest frames (total, stepping, frame) {:?}", scene.name, &times[..4]);
        if !cfg!(debug_assertions) {
            assert!(times[0].0.as_millis() < 100, "{} stutters: {:?}", scene.name, times[0]);
        }
    }
}

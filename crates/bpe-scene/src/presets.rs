//! Presets for the newer scene types: escape rings, spinning shapes, multiplying balls,
//! a shrinking arena, string art and a peg board.

use crate::*;

const RAINBOW: [Color; 6] = [
    Color::rgb(0xff, 0x3e, 0x5e),
    Color::rgb(0xff, 0x8a, 0x2e),
    Color::rgb(0xff, 0xd8, 0x2e),
    Color::rgb(0x5c, 0xe6, 0x5c),
    Color::rgb(0x2e, 0xb8, 0xff),
    Color::rgb(0x9a, 0x5c, 0xff),
];

fn chords(names: &[&str], seconds: f32) -> Vec<ChordStep> {
    names.iter().map(|&chord| ChordStep { chord: chord.into(), seconds }).collect()
}

impl Scene {
    /// One ball inside ten spinning rings, each with a gap. Every escape shatters a ring.
    pub fn escape_rings() -> Self {
        Self {
            name: "Escape rings".into(),
            container: Container::Rings {
                center: Vec2::ZERO,
                count: 10,
                inner_radius: 110.0,
                spacing: 38.0,
                sides: 0,
                gap: 50.0,
                spin: 70.0,
                spin_step: 9.0,
                gap_offset: 20.0,
                thickness: 6.0,
                gradient: RAINBOW.to_vec(),
            },
            spawners: vec![Spawner::Single {
                position: Vec2::new(0.0, -20.0),
                velocity: Vec2::new(260.0, -340.0),
                radius: 13.0,
                color: Color::WHITE,
                depth: 0.0,
                depth_velocity: 0.0,
            }],
            physics: Physics { gravity: Vec2::new(0.0, 800.0), ..Physics::default() },
            rules: BounceRules { speed_gain: 1.01, max_speed: 1500.0, ..BounceRules::default() },
            look: Look {
                trail_seconds: 0.35,
                trail_width: 0.8,
                trail_opacity: 0.5,
                ball_glow: 0.4,
                flash_size: 34.0,
                bloom_strength: 0.25,
                ..Look::default()
            },
            sound: Sound {
                progression: chords(&["Am", "F", "C", "G"], 2.5),
                low_note: 57,
                high_note: 84,
                instrument: Instrument::Pluck { decay: 1.2, brightness: 0.55 },
                full_volume_speed: 900.0,
                ..Sound::default()
            },
            video: Video { duration: 24.0, ..Video::default() },
            ..Self::default()
        }
    }

    /// Many hexagon rings, where every escape adds two new balls in the middle.
    pub fn escape_and_multiply() -> Self {
        let mut scene = Self::escape_rings();
        scene.name = "Escape and multiply".into();
        if let Container::Rings {
            sides,
            count,
            inner_radius,
            spacing,
            thickness,
            gap,
            spin,
            spin_step,
            gap_offset,
            ..
        } = &mut scene.container
        {
            *sides = 6;
            *count = 18;
            *inner_radius = 90.0;
            *spacing = 23.0;
            *thickness = 4.0;
            *gap = 30.0;
            *spin = 45.0;
            *spin_step = 6.0;
            *gap_offset = 25.0;
        }
        scene.spawners = vec![Spawner::Single {
            position: Vec2::new(0.0, -20.0),
            velocity: Vec2::new(-280.0, -300.0),
            radius: 10.0,
            color: Color::rgb(0x2e, 0xd8, 0xe6),
            depth: 0.0,
            depth_velocity: 0.0,
        }];
        scene.rules = BounceRules { spawn_per_escape: 2, max_balls: 200, spawn_spread: 40.0, ..scene.rules };
        scene.sound.progression = chords(&["Dm", "Bb", "F", "C"], 2.5);
        scene.video.duration = 18.0;
        scene
    }

    /// A spinning hexagon with six balls bouncing inside.
    pub fn spinning_hexagon() -> Self {
        let spawners = RAINBOW
            .iter()
            .enumerate()
            .map(|(i, &color)| Spawner::Single {
                position: Vec2::new(-200.0 + i as f32 * 80.0, -120.0),
                velocity: Vec2::new(150.0 - i as f32 * 60.0, -200.0),
                radius: 16.0,
                color,
                depth: 0.0,
                depth_velocity: 0.0,
            })
            .collect();
        Self {
            name: "Spinning hexagon".into(),
            container: Container::Polygon {
                center: Vec2::ZERO,
                radius: 470.0,
                sides: 6,
                rotation: 0.0,
                thickness: 5.0,
                color: Color::rgb(0xd8, 0xdc, 0xd0),
            },
            arena_motion: ArenaMotion { spin: 40.0, ..ArenaMotion::default() },
            spawners,
            physics: Physics { gravity: Vec2::new(0.0, 900.0), ..Physics::default() },
            rules: BounceRules { max_speed: 1800.0, ..BounceRules::default() },
            look: Look { trail_seconds: 0.5, trail_width: 0.9, trail_opacity: 0.5, ball_glow: 0.3, ..Look::default() },
            sound: Sound {
                progression: chords(&["Fmaj7", "Em7", "Dm7", "Cmaj7"], 3.0),
                low_note: 55,
                high_note: 84,
                ..Sound::default()
            },
            video: Video { duration: 25.0, ..Video::default() },
            ..Self::default()
        }
    }

    /// One ball that splits off a new ball on every bounce.
    pub fn multiplying_balls() -> Self {
        Self {
            name: "Multiplying balls".into(),
            spawners: vec![Spawner::Single {
                position: Vec2::new(0.0, -200.0),
                velocity: Vec2::new(320.0, 0.0),
                radius: 11.0,
                color: Color::rgb(0xff, 0x3e, 0x5e),
                depth: 0.0,
                depth_velocity: 0.0,
            }],
            physics: Physics { gravity: Vec2::new(0.0, 600.0), ..Physics::default() },
            rules: BounceRules { spawn_per_hit: 1, max_balls: 300, spawn_spread: 25.0, ..BounceRules::default() },
            look: Look { trail_seconds: 0.3, trail_width: 0.8, trail_opacity: 0.5, ball_glow: 0.3, ..Look::default() },
            sound: Sound {
                progression: chords(&["C", "Am", "F", "G"], 3.0),
                low_note: 60,
                high_note: 88,
                instrument: Instrument::Pluck { decay: 0.7, brightness: 0.5 },
                ..Sound::default()
            },
            video: Video { duration: 20.0, ..Video::default() },
            ..Self::default()
        }
    }

    /// A ring that slowly closes in, so the bounces get faster and the notes denser.
    pub fn shrinking_ring() -> Self {
        Self {
            name: "Shrinking ring".into(),
            container: Container::Circle {
                center: Vec2::ZERO,
                radius: 500.0,
                thickness: 5.0,
                color: Color::rgb(0xff, 0xb8, 0x2e),
            },
            arena_motion: ArenaMotion { shrink_speed: 13.0, min_size: 130.0, ..ArenaMotion::default() },
            spawners: vec![Spawner::Single {
                position: Vec2::new(0.0, -150.0),
                velocity: Vec2::new(380.0, 0.0),
                radius: 22.0,
                color: Color::rgb(0x2e, 0xd8, 0xe6),
                depth: 0.0,
                depth_velocity: 0.0,
            }],
            physics: Physics { gravity: Vec2::new(0.0, 700.0), ..Physics::default() },
            rules: BounceRules {
                max_speed: 2600.0,
                recolor: vec![Color::rgb(0x2e, 0xd8, 0xe6), Color::rgb(0xff, 0x3e, 0xc8), Color::rgb(0xff, 0xd8, 0x2e)],
                ..BounceRules::default()
            },
            look: Look { trail_seconds: 0.4, trail_width: 0.9, trail_opacity: 0.5, ball_glow: 0.3, ..Look::default() },
            sound: Sound {
                progression: chords(&["Em", "C", "G", "D"], 3.0),
                low_note: 52,
                high_note: 86,
                instrument: Instrument::Pluck { decay: 0.8, brightness: 0.55 },
                full_volume_speed: 1200.0,
                ..Sound::default()
            },
            video: Video { duration: 30.0, ..Video::default() },
            ..Self::default()
        }
    }

    /// One ball that draws a line to every point it has hit, building up a web.
    pub fn string_art() -> Self {
        Self {
            name: "String art".into(),
            container: Container::Circle {
                center: Vec2::ZERO,
                radius: 440.0,
                thickness: 4.0,
                color: Color::rgb(0xd8, 0xdc, 0xd0),
            },
            spawners: vec![Spawner::Single {
                position: Vec2::new(-60.0, -220.0),
                velocity: Vec2::new(650.0, 240.0),
                radius: 14.0,
                color: Color::rgb(0xff, 0x5c, 0xa8),
                depth: 0.0,
                depth_velocity: 0.0,
            }],
            physics: Physics { gravity: Vec2::new(0.0, 600.0), ..Physics::default() },
            rules: BounceRules {
                speed_gain: 1.04,
                max_speed: 2400.0,
                recolor: RAINBOW.to_vec(),
                ..BounceRules::default()
            },
            look: Look {
                trail_seconds: 0.2,
                trail_width: 0.8,
                ball_glow: 0.4,
                string_lines: 400,
                string_opacity: 0.3,
                string_width: 1.5,
                ..Look::default()
            },
            sound: Sound {
                progression: chords(&["Cmaj7", "Am7", "Fmaj7", "G"], 3.0),
                low_note: 60,
                high_note: 88,
                instrument: Instrument::Pluck { decay: 1.5, brightness: 0.45 },
                ..Sound::default()
            },
            video: Video { duration: 25.0, ..Video::default() },
            ..Self::default()
        }
    }

    /// Balls dropped one by one through a board of pegs into bins.
    pub fn plinko() -> Self {
        let wall = Color::rgb(0xd8, 0xdc, 0xd0);
        let bottom = 760.0;
        let mut obstacles = vec![Obstacle::PegGrid {
            center: Vec2::new(0.0, -60.0),
            columns: 11,
            rows: 12,
            spacing: Vec2::new(84.0, 76.0),
            radius: 7.0,
            stagger: true,
            color: Color::rgb(0xc8, 0xd8, 0xff),
        }];
        for i in -5..=5 {
            let x = i as f32 * 84.0;
            obstacles.push(Obstacle::Line {
                from: Vec2::new(x, 560.0),
                to: Vec2::new(x, bottom),
                thickness: 4.0,
                color: wall,
            });
        }
        Self {
            name: "Plinko".into(),
            container: Container::Rect {
                center: Vec2::new(0.0, 0.0),
                size: Vec2::new(940.0, 2.0 * bottom),
                thickness: 4.0,
                color: wall,
            },
            obstacles,
            spawners: vec![Spawner::Stream {
                position: Vec2::new(0.0, -700.0),
                velocity: Vec2::ZERO,
                radius: 10.0,
                count: 160,
                interval: 0.12,
                jitter: 40.0,
                gradient: RAINBOW.to_vec(),
                depth: 0.0,
                depth_velocity: 0.0,
            }],
            physics: Physics {
                gravity: Vec2::new(0.0, 1400.0),
                restitution: 0.5,
                ball_collisions: true,
                ..Physics::default()
            },
            look: Look { trail_seconds: 0.15, trail_width: 0.7, ball_glow: 0.3, flash_size: 20.0, ..Look::default() },
            sound: Sound {
                progression: chords(&["C", "G", "Am", "F"], 3.0),
                low_note: 64,
                high_note: 96,
                instrument: Instrument::Pluck { decay: 0.5, brightness: 0.6 },
                min_impact_speed: 120.0,
                full_volume_speed: 1400.0,
                ..Sound::default()
            },
            video: Video { duration: 30.0, ..Video::default() },
            ..Self::default()
        }
    }

    /// Keyframes in action: the ring breathes in and out while the colours and glow change.
    pub fn pulsing_ring() -> Self {
        let number = |time, v| Key { time, value: Value::Number(v), ease: Ease::Smooth };
        let colour = |time, c| Key { time, value: Value::Color(c), ease: Ease::Smooth };
        let spawners = RAINBOW
            .iter()
            .enumerate()
            .map(|(i, &color)| {
                let angle = i as f32 / RAINBOW.len() as f32 * std::f32::consts::TAU;
                Spawner::Single {
                    position: Vec2::from_angle(angle) * 120.0,
                    velocity: Vec2::from_angle(angle + 1.2) * 420.0,
                    radius: 14.0,
                    color,
                    depth: 0.0,
                    depth_velocity: 0.0,
                }
            })
            .collect();
        Self {
            name: "Pulsing ring".into(),
            physics: Physics { gravity: Vec2::new(0.0, 300.0), ..Physics::default() },
            spawners,
            look: Look { trail_seconds: 0.6, trail_width: 0.8, trail_opacity: 0.55, ball_glow: 0.3, ..Look::default() },
            animation: vec![
                Track {
                    property: Property::ArenaSize,
                    keys: vec![
                        number(0.0, 1.0),
                        number(4.0, 0.55),
                        number(8.0, 1.0),
                        number(12.0, 0.45),
                        number(16.0, 1.0),
                    ],
                },
                Track {
                    property: Property::BackgroundTop,
                    keys: vec![
                        colour(0.0, Color::rgb(0x02, 0x02, 0x2a)),
                        colour(8.0, Color::rgb(0x2a, 0x05, 0x3a)),
                        colour(16.0, Color::rgb(0x06, 0x12, 0x48)),
                    ],
                },
                Track {
                    property: Property::Bloom,
                    keys: vec![number(0.0, 0.15), number(12.0, 0.45), number(16.0, 0.25)],
                },
            ],
            sound: Sound {
                progression: chords(&["Am", "F", "C", "G"], 4.0),
                low_note: 57,
                high_note: 88,
                instrument: Instrument::Pluck { decay: 1.2, brightness: 0.55 },
                ..Sound::default()
            },
            video: Video { duration: 16.0, ..Video::default() },
            ..Self::default()
        }
    }
}

// ---- 3D (WS4) ----

/// 3D settings around `container`, with the camera framing it for a 9:16 video.
fn space(container: Container3d, change: impl FnOnce(&mut Space)) -> View {
    let mut space = Space { camera: Camera::fit(&container, 9.0 / 16.0), container, ..Space::default() };
    change(&mut space);
    View::Space(Box::new(space))
}

impl Scene {
    /// A block of balls tumbling inside a glass sphere while the camera circles it.
    pub fn glass_sphere() -> Self {
        Self {
            name: "Glass sphere".into(),
            view: space(Container3d::Sphere { radius: 380.0 }, |space| {
                space.camera.mode = CameraMode::Orbit { speed: 10.0 };
                space.material.glass = 0.45;
            }),
            spawners: vec![Spawner::Grid {
                center: Vec2::new(0.0, -120.0),
                columns: 12,
                rows: 12,
                spacing: 26.0,
                radius: 9.0,
                velocity: Vec2::new(180.0, 0.0),
                gradient: RAINBOW.to_vec(),
                depth: 0.0,
                depth_velocity: 0.0,
            }],
            physics: Physics { gravity: Vec2::new(0.0, 900.0), ball_collisions: true, ..Physics::default() },
            look: Look { trail_seconds: 0.25, trail_opacity: 0.4, flash_size: 24.0, ..Look::default() },
            sound: Sound {
                progression: chords(&["Am", "F", "C", "G"], 2.5),
                instrument: Instrument::Pluck { decay: 0.8, brightness: 0.5 },
                ..Sound::default()
            },
            video: Video { duration: 30.0, ..Video::default() },
            ..Self::default()
        }
    }

    /// A few fast balls with long trails inside a turning cube with glowing edges.
    pub fn neon_cube() -> Self {
        let spawners = RAINBOW
            .iter()
            .enumerate()
            .map(|(i, &color)| {
                let angle = i as f32 / RAINBOW.len() as f32 * std::f32::consts::TAU;
                Spawner::Single {
                    position: Vec2::from_angle(angle) * 80.0,
                    velocity: Vec2::from_angle(angle + 0.9) * 700.0,
                    radius: 16.0,
                    color,
                    depth: (i as f32 - 2.5) * 40.0,
                    depth_velocity: if i % 2 == 0 { 450.0 } else { -450.0 },
                }
            })
            .collect();
        Self {
            name: "Neon cube".into(),
            view: space(Container3d::Cube { size: 600.0 }, |space| {
                space.material.glass = 0.15;
                space.material.glass_color = Color::rgb(0xff, 0x4d, 0xd8);
                space.material.edge_glow = 2.5;
                space.material.roughness = 0.15;
                space.lights.ambient_top = Color::rgb(0x6a, 0x3d, 0xff);
                space.lights.ambient_bottom = Color::rgb(0x10, 0x04, 0x28);
                space.scatter = 0.0;
            }),
            spawners,
            arena_motion: ArenaMotion { spin: 18.0, ..ArenaMotion::default() },
            physics: Physics { gravity: Vec2::ZERO, ..Physics::default() },
            look: Look {
                trail_seconds: 0.8,
                trail_width: 0.7,
                trail_opacity: 0.7,
                flash_size: 40.0,
                bloom_strength: 0.35,
                ..Look::default()
            },
            sound: Sound {
                progression: chords(&["Em", "C", "G", "D"], 2.0),
                low_note: 60,
                high_note: 88,
                instrument: Instrument::Pluck { decay: 0.6, brightness: 0.7 },
                ..Sound::default()
            },
            video: Video { duration: 24.0, ..Video::default() },
            ..Self::default()
        }
    }

    /// Balls pushed down an endless tunnel, breaking each ring when one finds its gap.
    pub fn endless_tunnel() -> Self {
        Self {
            name: "Endless tunnel".into(),
            view: space(Container3d::tunnel(330.0), |space| {
                space.material.edge_glow = 1.6;
                space.scatter = 0.3;
            }),
            spawners: vec![Spawner::Grid {
                center: Vec2::ZERO,
                columns: 4,
                rows: 4,
                spacing: 60.0,
                radius: 14.0,
                velocity: Vec2::new(260.0, -340.0),
                gradient: RAINBOW.to_vec(),
                depth: 0.0,
                depth_velocity: 300.0,
            }],
            physics: Physics { gravity: Vec2::new(0.0, 500.0), ball_collisions: true, ..Physics::default() },
            rules: BounceRules { speed_gain: 1.01, max_speed: 1500.0, ..BounceRules::default() },
            look: Look { trail_seconds: 0.4, trail_opacity: 0.5, flash_size: 30.0, ..Look::default() },
            sound: Sound {
                progression: chords(&["Dm", "Bb", "F", "C"], 2.5),
                instrument: Instrument::Pluck { decay: 1.0, brightness: 0.6 },
                ..Sound::default()
            },
            video: Video { duration: 30.0, ..Video::default() },
            ..Self::default()
        }
    }
}

/// Presets that show off swarms, liquid and hit particles.
impl Scene {
    /// Every scene preset added with the GPU layers, for the gallery.
    pub fn gpu_presets() -> Vec<Preset> {
        vec![
            Preset {
                name: "Spark storm",
                description: "Six balls race around a ring, throwing sparks and stars on every bounce.",
                scene: Scene::spark_storm(),
            },
            Preset {
                name: "Confetti escape",
                description: "Each ring a ball breaks out of bursts into confetti.",
                scene: Scene::confetti_escape(),
            },
            Preset {
                name: "Smoke rings",
                description: "Balls fall through pegs, leaving puffs of smoke and rings of light.",
                scene: Scene::smoke_rings(),
            },
            Preset {
                name: "Liquid fill",
                description: "Water pours into a ring while balls splash through it.",
                scene: Scene::liquid_fill(),
            },
            Preset {
                name: "Million swarm",
                description: "A million glowing points swirl and flock around three balls.",
                scene: Scene::million_swarm(),
            },
        ]
    }

    pub fn spark_storm() -> Self {
        let spawners = RAINBOW
            .iter()
            .enumerate()
            .map(|(i, &color)| {
                let angle = i as f32 / RAINBOW.len() as f32 * std::f32::consts::TAU;
                Spawner::Single {
                    position: Vec2::from_angle(angle) * 150.0,
                    velocity: Vec2::from_angle(angle + 1.4) * 700.0,
                    radius: 16.0,
                    color,
                    depth: 0.0,
                    depth_velocity: 0.0,
                }
            })
            .collect();
        let mut sparks = ParticlePreset::Sparks.emitter(EventMatch::AnyHit);
        sparks.count = 28;
        sparks.bounce = true;
        Self {
            name: "Spark storm".into(),
            physics: Physics { gravity: Vec2::new(0.0, 500.0), ..Physics::default() },
            spawners,
            look: Look { trail_seconds: 0.3, trail_width: 0.7, ball_glow: 0.4, bloom_strength: 0.35, ..Look::default() },
            particles: vec![sparks, ParticlePreset::Stars.emitter(EventMatch::AnyHit)],
            sound: Sound {
                progression: chords(&["Dm", "Bb", "F", "C"], 3.0),
                instrument: Instrument::Pluck { decay: 0.6, brightness: 0.7 },
                ..Sound::default()
            },
            video: Video { duration: 20.0, ..Video::default() },
            ..Self::default()
        }
    }

    pub fn confetti_escape() -> Self {
        let mut confetti = ParticlePreset::Confetti.emitter(EventMatch::Escape);
        confetti.count = 160;
        Self {
            name: "Confetti escape".into(),
            particles: vec![confetti, ParticlePreset::Sparks.emitter(EventMatch::AnyHit)],
            ..Self::escape_rings()
        }
    }

    pub fn smoke_rings() -> Self {
        Self {
            name: "Smoke rings".into(),
            particles: vec![
                ParticlePreset::Smoke.emitter(EventMatch::Obstacle),
                ParticlePreset::Rings.emitter(EventMatch::Obstacle),
            ],
            ..Self::plinko()
        }
    }

    pub fn liquid_fill() -> Self {
        let spawners = RAINBOW[..3]
            .iter()
            .enumerate()
            .map(|(i, &color)| Spawner::Single {
                position: Vec2::new(-200.0 + i as f32 * 200.0, -300.0),
                velocity: Vec2::new(300.0 - i as f32 * 250.0, 0.0),
                radius: 26.0,
                color,
                depth: 0.0,
                depth_velocity: 0.0,
            })
            .collect();
        Self {
            name: "Liquid fill".into(),
            physics: Physics { gravity: Vec2::new(0.0, 900.0), restitution: 1.0, ..Physics::default() },
            spawners,
            gpu: GpuLayers {
                fluid: GpuFluid { enabled: true, count: 80_000, fill: 0.4, pour_seconds: 8.0, ..GpuFluid::default() },
                ..GpuLayers::default()
            },
            particles: vec![ParticlePreset::Rings.emitter(EventMatch::Wall)],
            look: Look { trail_seconds: 0.2, ball_glow: 0.3, ..Look::default() },
            video: Video { duration: 20.0, ..Video::default() },
            ..Self::default()
        }
    }

    pub fn million_swarm() -> Self {
        let spawners = RAINBOW[..3]
            .iter()
            .enumerate()
            .map(|(i, &color)| {
                let angle = i as f32 / 3.0 * std::f32::consts::TAU;
                Spawner::Single {
                    position: Vec2::from_angle(angle) * 200.0,
                    velocity: Vec2::from_angle(angle + 1.6) * 450.0,
                    radius: 20.0,
                    color,
                    depth: 0.0,
                    depth_velocity: 0.0,
                }
            })
            .collect();
        Self {
            name: "Million swarm".into(),
            physics: Physics { gravity: Vec2::ZERO, ..Physics::default() },
            spawners,
            gpu: GpuLayers {
                swarm: Swarm {
                    enabled: true,
                    count: MAX_SWARM_POINTS,
                    brightness: 0.12,
                    color: SwarmColor::NearestBall,
                    ..Swarm::default()
                },
                ..GpuLayers::default()
            },
            look: Look { trail_seconds: 0.0, ball_glow: 0.5, bloom_strength: 0.3, ..Look::default() },
            video: Video { duration: 20.0, ..Video::default() },
            ..Self::default()
        }
    }
}

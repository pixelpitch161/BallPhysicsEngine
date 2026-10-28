//! Scene description: everything that defines one video.
//!
//! A scene is plain data, saved as RON. The simulation, renderer, audio mixer and
//! exporter all read from it, and nothing else. World coordinates are pixels of the
//! output video, with the origin at the centre of the frame and y pointing down.

mod animation;
mod bodies;
mod captions;
mod color;
mod effect;
mod gpu;
pub mod logic;
mod music;
mod particles;
mod presets;
mod random;
mod shapes;
mod view;

pub use animation::{
    Audio, AudioBand, AudioFrom, Curve, Ease, Key, Levels, Listener, Modulator, Property, SAME_TIME, Track, Value,
    remap_effect_modulators,
};
pub use bodies::*;
pub use bpe_graph::Graph;
pub use captions::*;
pub use color::Color;
pub use effect::Effect;
pub use glam::Vec2;
pub use glam::Vec3; // 3D (WS4)
pub use gpu::*;
pub use music::*;
pub use particles::*;
pub use random::{Dice, Group, palette};
pub use shapes::*;
pub use view::*;

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Scene {
    pub name: String,
    pub video: Video,
    pub physics: Physics,
    pub container: Container,
    /// How the arena turns and shrinks over time.
    pub arena_motion: ArenaMotion,
    /// Pegs and walls inside the arena.
    pub obstacles: Vec<Obstacle>,
    pub spawners: Vec<Spawner>,
    pub rules: BounceRules,
    pub look: Look,
    /// Shader effects applied to the finished picture, in order.
    pub effects: Vec<Effect>,
    pub sound: Sound,
    /// Settings that change over time, one track of keyframes each.
    pub animation: Vec<Track>,
    /// Rules: "when this happens, do that" graphs run inside the simulation.
    pub logic: Vec<Graph>,
    /// Named outlines that a custom arena or shaped obstacles use.
    pub shapes: Vec<Shape>,
    /// Jelly balls, ropes, cloth and small fluids.
    pub bodies: Bodies,
    /// Swarms and big fluids drawn on the graphics card.
    pub gpu: GpuLayers,
    /// Particles that burst out on hits and rule actions.
    pub particles: Vec<Emitter>,
    /// Flat 2D or 3D space.
    pub view: View,
    /// Shader effects built from blocks, used by effects named "graph:<name>".
    pub shader_graphs: Vec<Graph>,
    /// Settings that pulse with the music.
    pub modulators: Vec<Modulator>,
    /// The song, beat sync and MIDI melody.
    pub music: Music,
    /// Text drawn over the video.
    pub captions: Vec<Caption>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Video {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Length of the clip in seconds.
    pub duration: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Physics {
    /// Pixels per second squared. Positive y pulls down.
    pub gravity: Vec2,
    /// Fixed simulation rate. Higher is more precise; 240 is plenty for most scenes.
    pub steps_per_second: u32,
    /// Fraction of speed kept along the wall normal on each bounce. 1.0 is perfectly elastic.
    pub restitution: f32,
    /// Fraction of velocity lost per second to air resistance.
    pub air_drag: f32,
    /// Whether balls bounce off each other. Off means they pass through one another.
    pub ball_collisions: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Container {
    Circle {
        center: Vec2,
        radius: f32,
        thickness: f32,
        color: Color,
    },
    Rect {
        center: Vec2,
        size: Vec2,
        thickness: f32,
        color: Color,
    },
    /// A regular polygon such as a triangle, square or hexagon.
    Polygon {
        center: Vec2,
        /// Distance from the centre to each corner.
        radius: f32,
        sides: u32,
        /// Degrees. At 0 the polygon has a flat side at the bottom.
        rotation: f32,
        thickness: f32,
        color: Color,
    },
    /// Rings nested inside each other, each with a gap. When a ball escapes through a ring's
    /// gap, that ring shatters. Balls start inside the smallest ring.
    Rings {
        center: Vec2,
        count: u32,
        /// Radius of the smallest ring.
        inner_radius: f32,
        /// Distance from one ring to the next.
        spacing: f32,
        /// 0 for round rings, or 3 and up for polygon rings.
        sides: u32,
        /// Width of each ring's gap, in degrees.
        gap: f32,
        /// Degrees per second the smallest ring turns. Positive is clockwise.
        spin: f32,
        /// Added to the spin of each ring further out, so the rings drift apart.
        spin_step: f32,
        /// Degrees each ring's gap is turned from the one inside it at the start.
        gap_offset: f32,
        thickness: f32,
        /// Colours from the smallest ring to the biggest, spread evenly.
        gradient: Vec<Color>,
    },
    /// An arena with any outline, from [`Scene::shapes`] by name.
    Custom {
        /// Where the shape's origin sits.
        #[serde(default)]
        center: Vec2,
        shape: String,
        thickness: f32,
        color: Color,
    },
}

/// Turning and shrinking applied to the whole arena.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ArenaMotion {
    /// Degrees per second the arena turns. Positive is clockwise.
    pub spin: f32,
    /// Pixels per second the arena's radius shrinks. Negative values make it grow.
    pub shrink_speed: f32,
    /// Pixels the arena's radius shrinks each time a ball hits it.
    pub shrink_per_hit: f32,
    /// The arena never shrinks below this radius in pixels.
    pub min_size: f32,
    /// Size of the arena compared to its shape's size, 1 being as set. Mostly for animating.
    pub size: f32,
}

/// Something fixed inside the arena that balls bounce off, from either side.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Obstacle {
    Peg {
        position: Vec2,
        radius: f32,
        color: Color,
    },
    /// Rows of pegs, every other row shifted half a step when `stagger` is on, like a Plinko board.
    PegGrid {
        center: Vec2,
        columns: u32,
        rows: u32,
        spacing: Vec2,
        radius: f32,
        stagger: bool,
        color: Color,
    },
    /// A straight wall between two points.
    Line {
        from: Vec2,
        to: Vec2,
        thickness: f32,
        color: Color,
    },
    /// An outline from [`Scene::shapes`] by name, placed, turned (degrees) and scaled.
    Shape {
        shape: String,
        position: Vec2,
        rotation: f32,
        scale: f32,
    },
}

/// One peg or wall piece, with grids expanded into single pegs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ObstaclePart {
    Disc { center: Vec2, radius: f32, color: Color },
    Segment { from: Vec2, to: Vec2, thickness: f32, color: Color },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Spawner {
    /// A rectangular block of balls, coloured top to bottom by a gradient.
    Grid {
        center: Vec2,
        columns: u32,
        rows: u32,
        spacing: f32,
        radius: f32,
        velocity: Vec2,
        /// Colours from the top row to the bottom row, spread evenly.
        gradient: Vec<Color>,
        /// Distance into the screen in 3D, in pixels. Ignored in 2D.
        #[serde(default)]
        depth: f32,
        /// Speed into the screen in 3D, in pixels per second. Ignored in 2D.
        #[serde(default)]
        depth_velocity: f32,
    },
    Single {
        position: Vec2,
        velocity: Vec2,
        radius: f32,
        color: Color,
        #[serde(default)]
        depth: f32,
        #[serde(default)]
        depth_velocity: f32,
    },
    /// Balls dropped one after another from the same spot.
    Stream {
        position: Vec2,
        velocity: Vec2,
        radius: f32,
        count: u32,
        /// Seconds between one ball and the next.
        interval: f32,
        /// Each ball starts up to this many pixels left or right of `position`, so they don't
        /// all follow the same path.
        jitter: f32,
        /// Colours from the first ball to the last, spread evenly.
        gradient: Vec<Color>,
        #[serde(default)]
        depth: f32,
        #[serde(default)]
        depth_velocity: f32,
    },
}

/// Changes applied to a ball each time it hits the wall.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BounceRules {
    /// Speed multiplier per wall hit. 1.0 leaves speed unchanged.
    pub speed_gain: f32,
    /// Whether `max_speed` applies. With it off, balls can keep speeding up until they reach
    /// the simulation's own ceiling (about 245,000 px/s at 240 steps a second).
    pub limit_speed: bool,
    /// Speed cap in pixels per second, so speed gain cannot run away.
    pub max_speed: f32,
    /// Pixels added to the radius per wall hit.
    pub grow: f32,
    pub max_radius: f32,
    /// Colours a ball steps through on each wall hit. Empty keeps its colour.
    pub recolor: Vec<Color>,
    /// New balls that split off a ball each time it hits the wall or a peg.
    pub spawn_per_hit: u32,
    /// New balls that split off a ball each time it escapes a ring.
    pub spawn_per_escape: u32,
    /// No more balls are added once there are this many.
    pub max_balls: u32,
    /// New balls fly off up to this many degrees either side of their parent's direction.
    pub spawn_spread: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Look {
    pub background_top: Color,
    pub background_bottom: Color,
    /// How far back in time each ball's streak reaches, in seconds. 0 disables trails.
    pub trail_seconds: f32,
    /// Streak width at the ball, as a fraction of the ball diameter.
    pub trail_width: f32,
    /// Streak brightness at the ball, 0 to 1.
    pub trail_opacity: f32,
    /// Brightness of the soft halo around each ball.
    pub ball_glow: f32,
    /// Strength of the light flash where a ball hits the wall.
    pub flash_strength: f32,
    pub flash_seconds: f32,
    pub flash_size: f32,
    /// How much bright areas bleed light around them.
    pub bloom_strength: f32,
    pub exposure: f32,
    /// String art: lines from each ball to the points where it hit things. This is how many of
    /// its most recent hits get a line. 0 turns it off.
    pub string_lines: u32,
    pub string_opacity: f32,
    /// Line width in pixels.
    pub string_width: f32,
    /// How many pieces a ring breaks into when a ball escapes it.
    pub shatter_pieces: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sound {
    pub enabled: bool,
    /// Chords played in order, then repeated. Notes for each wall hit come from the current chord.
    pub progression: Vec<ChordStep>,
    /// Lowest and highest MIDI note to pick from (60 is middle C).
    pub low_note: u8,
    pub high_note: u8,
    pub instrument: Instrument,
    /// Instruments for particular kinds of event. `instrument` plays every hit and escape that
    /// no enabled row plays.
    pub voices: Vec<VoiceMap>,
    /// Wall hits slower than this (pixels per second along the wall normal) make no sound.
    pub min_impact_speed: f32,
    /// Impact speed that plays at full note volume.
    pub full_volume_speed: f32,
    /// How far notes are panned left and right by where the hit happens. 0 is mono.
    pub stereo_width: f32,
    /// Overall loudness target in LUFS-like dB (about -14 for social media).
    pub target_loudness: f32,
    /// Seed for picking notes, so the same scene always sounds the same.
    pub seed: u64,
}

/// A built-in starting point for a new scene.
#[derive(Clone, Debug)]
pub struct Preset {
    pub name: &'static str,
    pub description: &'static str,
    pub scene: Scene,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChordStep {
    /// Chord name such as "C", "Am", "G7", "Dsus4", "Fmaj7".
    pub chord: String,
    pub seconds: f32,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            name: "Reference reel".into(),
            video: Video::default(),
            physics: Physics::default(),
            container: Container::Circle {
                center: Vec2::ZERO,
                radius: 508.0,
                thickness: 3.0,
                color: Color::rgb(0xd8, 0xdc, 0xd0),
            },
            arena_motion: ArenaMotion::default(),
            obstacles: Vec::new(),
            spawners: vec![Spawner::Grid {
                center: Vec2::new(37.0, -127.0),
                columns: 28,
                rows: 28,
                spacing: 19.0,
                radius: 4.5,
                velocity: Vec2::new(-480.0, 265.0),
                gradient: vec![
                    Color::rgb(0x3e, 0x9b, 0xff),
                    Color::rgb(0x8a, 0x4d, 0xff),
                    Color::rgb(0xff, 0x3e, 0xc8),
                    Color::rgb(0xff, 0x6a, 0x5c),
                    Color::rgb(0xff, 0xb8, 0x2e),
                ],
                depth: 0.0,
                depth_velocity: 0.0,
            }],
            rules: BounceRules::default(),
            look: Look::default(),
            effects: Vec::new(),
            sound: Sound::default(),
            animation: Vec::new(),
            logic: Vec::new(),
            shapes: Vec::new(),
            bodies: Bodies::default(),
            gpu: GpuLayers::default(),
            particles: Vec::new(),
            view: View::default(),
            shader_graphs: Vec::new(),
            modulators: Vec::new(),
            music: Music::default(),
            captions: Vec::new(),
        }
    }
}

impl Default for Video {
    fn default() -> Self {
        Self { width: 1080, height: 1920, fps: 60, duration: 23.0 }
    }
}

impl Default for Physics {
    fn default() -> Self {
        Self {
            gravity: Vec2::new(0.0, 160.0),
            steps_per_second: 240,
            restitution: 1.0,
            air_drag: 0.0,
            ball_collisions: false,
        }
    }
}

impl Default for BounceRules {
    fn default() -> Self {
        Self {
            speed_gain: 1.0,
            limit_speed: true,
            max_speed: 3000.0,
            grow: 0.0,
            max_radius: 60.0,
            recolor: Vec::new(),
            spawn_per_hit: 0,
            spawn_per_escape: 0,
            max_balls: 500,
            spawn_spread: 30.0,
        }
    }
}

impl Default for ArenaMotion {
    fn default() -> Self {
        Self { spin: 0.0, shrink_speed: 0.0, shrink_per_hit: 0.0, min_size: 60.0, size: 1.0 }
    }
}

impl Default for Look {
    fn default() -> Self {
        Self {
            background_top: Color::rgb(0x02, 0x02, 0x2a),
            background_bottom: Color::rgb(0x01, 0x00, 0x06),
            trail_seconds: 0.3,
            trail_width: 0.3,
            trail_opacity: 0.4,
            ball_glow: 0.12,
            flash_strength: 0.4,
            flash_seconds: 0.25,
            flash_size: 26.0,
            bloom_strength: 0.15,
            exposure: 1.0,
            string_lines: 0,
            string_opacity: 0.35,
            string_width: 1.5,
            shatter_pieces: 24,
        }
    }
}

impl Default for Sound {
    fn default() -> Self {
        let step = |chord: &str, seconds| ChordStep { chord: chord.into(), seconds };
        Self {
            enabled: true,
            progression: vec![
                step("C", 4.2),
                step("Dsus4", 4.0),
                step("Am", 5.0),
                step("C", 4.0),
                step("Am", 3.1),
                step("Dsus4", 3.0),
            ],
            low_note: 48,
            high_note: 84,
            instrument: Instrument::Pluck { decay: 0.9, brightness: 0.5 },
            voices: Vec::new(),
            min_impact_speed: 20.0,
            full_volume_speed: 600.0,
            stereo_width: 0.6,
            target_loudness: -14.0,
            seed: 1,
        }
    }
}

impl Container {
    pub fn center(&self) -> Vec2 {
        match *self {
            Container::Circle { center, .. }
            | Container::Rect { center, .. }
            | Container::Polygon { center, .. }
            | Container::Rings { center, .. }
            | Container::Custom { center, .. } => center,
        }
    }

    pub fn center_mut(&mut self) -> &mut Vec2 {
        match self {
            Container::Circle { center, .. }
            | Container::Rect { center, .. }
            | Container::Polygon { center, .. }
            | Container::Rings { center, .. }
            | Container::Custom { center, .. } => center,
        }
    }

    pub fn thickness(&self) -> f32 {
        match *self {
            Container::Circle { thickness, .. }
            | Container::Rect { thickness, .. }
            | Container::Polygon { thickness, .. }
            | Container::Rings { thickness, .. }
            | Container::Custom { thickness, .. } => thickness,
        }
    }

    /// The arena's main colour (the smallest ring's colour for rings).
    pub fn color(&self) -> Color {
        match self {
            Container::Circle { color, .. }
            | Container::Rect { color, .. }
            | Container::Polygon { color, .. }
            | Container::Custom { color, .. } => *color,
            Container::Rings { gradient, .. } => gradient.first().copied().unwrap_or(Color::WHITE),
        }
    }

    /// The arena's radius at its starting size, which `ArenaMotion` shrinks from. For a box it
    /// is half its longer side, and for rings the biggest ring's radius.
    pub fn size(&self) -> f32 {
        match *self {
            Container::Circle { radius, .. } | Container::Polygon { radius, .. } => radius,
            Container::Rect { size, .. } => size.x.max(size.y) * 0.5,
            Container::Rings { count, inner_radius, spacing, .. } => {
                ring_radius(inner_radius, spacing, count.max(1) - 1)
            }
            // Until custom shapes are measured, treat them like the default ring.
            Container::Custom { .. } => 508.0,
        }
    }

    /// Half the arena's width, used to pan sounds left and right.
    pub fn half_width(&self) -> f32 {
        match *self {
            Container::Rect { size, .. } => size.x * 0.5,
            _ => self.size(),
        }
    }
}

/// Radius of ring number `index` (0 is the smallest).
pub fn ring_radius(inner_radius: f32, spacing: f32, index: u32) -> f32 {
    inner_radius + spacing * index as f32
}

impl Obstacle {
    /// Appends this obstacle's pegs and wall pieces to `out`.
    pub fn parts(&self, out: &mut Vec<ObstaclePart>) {
        match *self {
            Obstacle::Peg { position, radius, color } => {
                out.push(ObstaclePart::Disc { center: position, radius, color })
            }
            Obstacle::PegGrid { center, columns, rows, spacing, radius, stagger, color } => {
                let size = Vec2::new(columns.saturating_sub(1) as f32, rows.saturating_sub(1) as f32) * spacing;
                let origin = center - size * 0.5;
                for row in 0..rows {
                    let shifted = stagger && row % 2 == 1;
                    // A shifted row has one peg fewer so it stays inside the grid's outline.
                    let count = if shifted { columns.saturating_sub(1) } else { columns };
                    let shift = if shifted { spacing.x * 0.5 } else { 0.0 };
                    for column in 0..count {
                        let position = origin + Vec2::new(column as f32 * spacing.x + shift, row as f32 * spacing.y);
                        out.push(ObstaclePart::Disc { center: position, radius, color });
                    }
                }
            }
            Obstacle::Line { from, to, thickness, color } => {
                out.push(ObstaclePart::Segment { from, to, thickness, color })
            }
            // Shaped obstacles are flattened into wall pieces once custom shapes are in.
            Obstacle::Shape { .. } => {}
        }
    }
}

impl Scene {
    /// Every peg and wall piece, in a fixed order that the simulation and renderer share.
    pub fn obstacle_parts(&self) -> Vec<ObstaclePart> {
        let mut parts = Vec::new();
        for obstacle in &self.obstacles {
            obstacle.parts(&mut parts);
        }
        parts
    }

    /// Built-in starting points shown in the editor's preset gallery, each with a one-line description.
    pub fn presets() -> Vec<Preset> {
        vec![
            Preset {
                name: "Reference reel",
                description: "784 glowing balls fall into a ring and play chords.",
                scene: Scene::default(),
            },
            Preset {
                name: "Growing ball",
                description: "One ball that grows and speeds up on every bounce.",
                scene: Scene::growing_ball(),
            },
            Preset {
                name: "Rainbow trails",
                description: "Eight balls burst out from the middle with long trails.",
                scene: Scene::rainbow_trails(),
            },
            Preset {
                name: "Box arena",
                description: "A block of balls in a square box that change colour as they bounce.",
                scene: Scene::box_arena(),
            },
            Preset {
                name: "One big ball",
                description: "A single large ball bouncing with a deep, slow sound.",
                scene: Scene::big_ball(),
            },
            Preset {
                name: "Escape rings",
                description: "A ball breaks out of ten spinning rings, shattering each one.",
                scene: Scene::escape_rings(),
            },
            Preset {
                name: "Escape and multiply",
                description: "Every escape through the hexagons adds two new balls in the middle.",
                scene: Scene::escape_and_multiply(),
            },
            Preset {
                name: "Spinning hexagon",
                description: "Six balls tumble inside a turning hexagon.",
                scene: Scene::spinning_hexagon(),
            },
            Preset {
                name: "Multiplying balls",
                description: "One ball splits off a new ball on every bounce.",
                scene: Scene::multiplying_balls(),
            },
            Preset {
                name: "Shrinking ring",
                description: "The ring closes in, so the bounces get faster and faster.",
                scene: Scene::shrinking_ring(),
            },
            Preset {
                name: "String art",
                description: "Lines from the ball to every point it hit build up a web.",
                scene: Scene::string_art(),
            },
            Preset {
                name: "Plinko",
                description: "Balls drop one by one through a board of pegs into bins.",
                scene: Scene::plinko(),
            },
            Preset {
                name: "Pulsing ring",
                description: "Keyframes make the ring breathe in and out while the colours change.",
                scene: Scene::pulsing_ring(),
            },
            // ---- 3D (WS4) ----
            Preset {
                name: "Glass sphere",
                description: "A block of balls tumbles inside a glass sphere while the camera circles it.",
                scene: Scene::glass_sphere(),
            },
            Preset {
                name: "Neon cube",
                description: "Six fast balls leave long trails inside a turning cube with glowing edges.",
                scene: Scene::neon_cube(),
            },
            Preset {
                name: "Endless tunnel",
                description: "Balls are pushed down a tunnel and break each ring when one finds its gap.",
                scene: Scene::endless_tunnel(),
            },
            // Music workstream presets.
            Preset {
                name: "Marimba melody",
                description: "Three balls play Ode to Joy on a marimba, one note per bounce.",
                scene: Scene::marimba_melody(),
            },
            Preset {
                name: "Beat-locked bounce",
                description: "One ball steered so every bounce lands on the beat.",
                scene: Scene::beat_locked_bounce(),
            },
            Preset {
                name: "Drum machine",
                description: "Balls in rings play a drum kit: kick, snare and a crash on every escape.",
                scene: Scene::drum_machine(),
            },
        ]
        .into_iter()
        .chain(Scene::gpu_presets())
        .collect()
    }

    /// Eight balls fired outwards from the centre, each its own colour, with long trails.
    pub fn rainbow_trails() -> Self {
        let colors = [
            Color::rgb(0xff, 0x3e, 0x5e),
            Color::rgb(0xff, 0x8a, 0x2e),
            Color::rgb(0xff, 0xd8, 0x2e),
            Color::rgb(0x5c, 0xe6, 0x5c),
            Color::rgb(0x2e, 0xd8, 0xe6),
            Color::rgb(0x3e, 0x7b, 0xff),
            Color::rgb(0x8a, 0x4d, 0xff),
            Color::rgb(0xff, 0x4d, 0xd8),
        ];
        let spawners = colors
            .iter()
            .enumerate()
            .map(|(i, &color)| {
                let angle = i as f32 / colors.len() as f32 * std::f32::consts::TAU - 1.3;
                Spawner::Single {
                    position: Vec2::ZERO,
                    velocity: Vec2::new(angle.cos(), angle.sin()) * 520.0,
                    radius: 12.0,
                    color,
                    depth: 0.0,
                    depth_velocity: 0.0,
                }
            })
            .collect();
        Self {
            name: "Rainbow trails".into(),
            physics: Physics { gravity: Vec2::new(0.0, 250.0), ..Physics::default() },
            spawners,
            look: Look { trail_seconds: 1.2, trail_width: 0.8, trail_opacity: 0.6, ball_glow: 0.35, ..Look::default() },
            sound: Sound {
                progression: vec![
                    ChordStep { chord: "Fmaj7".into(), seconds: 3.0 },
                    ChordStep { chord: "Em7".into(), seconds: 3.0 },
                    ChordStep { chord: "Dm7".into(), seconds: 3.0 },
                    ChordStep { chord: "Cmaj7".into(), seconds: 3.0 },
                ],
                low_note: 60,
                high_note: 88,
                ..Sound::default()
            },
            video: Video { duration: 20.0, ..Video::default() },
            ..Self::default()
        }
    }

    /// A block of balls in a square box, stepping through colours on every wall hit.
    pub fn box_arena() -> Self {
        Self {
            name: "Box arena".into(),
            container: Container::Rect {
                center: Vec2::ZERO,
                size: Vec2::new(900.0, 900.0),
                thickness: 4.0,
                color: Color::rgb(0xd8, 0xdc, 0xd0),
            },
            spawners: vec![Spawner::Grid {
                center: Vec2::new(-150.0, -200.0),
                columns: 12,
                rows: 12,
                spacing: 26.0,
                radius: 8.0,
                velocity: Vec2::new(420.0, -150.0),
                gradient: vec![Color::rgb(0x2e, 0xd8, 0xe6)],
                depth: 0.0,
                depth_velocity: 0.0,
            }],
            physics: Physics { gravity: Vec2::new(0.0, 400.0), ..Physics::default() },
            rules: BounceRules {
                recolor: vec![
                    Color::rgb(0xff, 0x3e, 0xc8),
                    Color::rgb(0xff, 0xb8, 0x2e),
                    Color::rgb(0x5c, 0xe6, 0x5c),
                    Color::rgb(0x2e, 0xd8, 0xe6),
                ],
                ..BounceRules::default()
            },
            look: Look { trail_seconds: 0.25, trail_width: 0.6, ..Look::default() },
            video: Video { duration: 20.0, ..Video::default() },
            ..Self::default()
        }
    }

    /// One large ball with heavy gravity and a low, slow pluck.
    pub fn big_ball() -> Self {
        Self {
            name: "One big ball".into(),
            physics: Physics { gravity: Vec2::new(0.0, 1200.0), ..Physics::default() },
            spawners: vec![Spawner::Single {
                position: Vec2::new(0.0, -250.0),
                velocity: Vec2::new(380.0, 0.0),
                radius: 110.0,
                color: Color::rgb(0x3e, 0x9b, 0xff),
                depth: 0.0,
                depth_velocity: 0.0,
            }],
            look: Look {
                trail_seconds: 0.4,
                trail_width: 0.9,
                trail_opacity: 0.35,
                ball_glow: 0.25,
                ..Look::default()
            },
            sound: Sound {
                progression: vec![
                    ChordStep { chord: "Am".into(), seconds: 4.0 },
                    ChordStep { chord: "F".into(), seconds: 4.0 },
                ],
                low_note: 40,
                high_note: 64,
                instrument: Instrument::Pluck { decay: 2.5, brightness: 0.3 },
                full_volume_speed: 1500.0,
                ..Sound::default()
            },
            video: Video { duration: 15.0, ..Video::default() },
            ..Self::default()
        }
    }

    /// One ball in a ring that grows and speeds up a little on every bounce.
    pub fn growing_ball() -> Self {
        Self {
            name: "Growing ball".into(),
            physics: Physics { gravity: Vec2::new(0.0, 900.0), ..Physics::default() },
            spawners: vec![Spawner::Single {
                position: Vec2::new(-120.0, -200.0),
                velocity: Vec2::new(420.0, 0.0),
                radius: 18.0,
                color: Color::rgb(0xff, 0x5c, 0xa8),
                depth: 0.0,
                depth_velocity: 0.0,
            }],
            rules: BounceRules { speed_gain: 1.02, grow: 2.5, max_radius: 300.0, ..BounceRules::default() },
            look: Look { trail_seconds: 0.5, trail_width: 0.9, trail_opacity: 0.5, ball_glow: 0.3, ..Look::default() },
            sound: Sound {
                progression: vec![
                    ChordStep { chord: "Am".into(), seconds: 2.0 },
                    ChordStep { chord: "F".into(), seconds: 2.0 },
                    ChordStep { chord: "C".into(), seconds: 2.0 },
                    ChordStep { chord: "G".into(), seconds: 2.0 },
                ],
                low_note: 57,
                high_note: 81,
                instrument: Instrument::Pluck { decay: 1.4, brightness: 0.45 },
                full_volume_speed: 1200.0,
                ..Sound::default()
            },
            video: Video { duration: 20.0, ..Video::default() },
            ..Self::default()
        }
    }

    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path.as_ref())?;
        Self::from_ron(&text)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        std::fs::write(path.as_ref(), self.to_ron()?)?;
        Ok(())
    }

    pub fn from_ron(text: &str) -> anyhow::Result<Self> {
        let mut scene: Self = ron::from_str(text)?;
        for track in &mut scene.animation {
            track.sort();
        }
        Ok(scene)
    }

    pub fn to_ron(&self) -> anyhow::Result<String> {
        let pretty = ron::ser::PrettyConfig::new().struct_names(true);
        Ok(ron::ser::to_string_pretty(self, pretty)?)
    }

    /// Number of video frames in the clip.
    pub fn frame_count(&self) -> u32 {
        (self.video.duration * self.video.fps as f32).round().max(1.0) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ron_round_trip() {
        let scene = Scene::default();
        let text = scene.to_ron().unwrap();
        assert_eq!(Scene::from_ron(&text).unwrap(), scene);
    }

    #[test]
    fn presets_save_and_load() {
        for preset in Scene::presets() {
            let text = preset.scene.to_ron().unwrap();
            assert_eq!(Scene::from_ron(&text).unwrap(), preset.scene, "{}", preset.name);
            assert_eq!(preset.scene.name, preset.name);
        }
    }

    #[test]
    fn missing_fields_use_defaults() {
        let scene = Scene::from_ron("(name: \"Sparse\")").unwrap();
        assert_eq!(scene.name, "Sparse");
        assert_eq!(scene.video, Video::default());
    }

    #[test]
    fn scene_files_from_before_new_scene_types_still_load() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenes");
        for name in ["reference-reel.ron", "growing-ball.ron"] {
            let scene = Scene::load(dir.join(name)).unwrap();
            assert_eq!(scene.arena_motion, ArenaMotion::default(), "{name}");
            assert!(scene.obstacles.is_empty());
            assert_eq!(scene.rules.spawn_per_hit, 0);
            assert_eq!(scene.look.string_lines, 0);
        }
    }

    #[test]
    fn staggered_peg_rows_have_one_peg_fewer() {
        let grid = Obstacle::PegGrid {
            center: Vec2::ZERO,
            columns: 5,
            rows: 3,
            spacing: Vec2::new(10.0, 10.0),
            radius: 1.0,
            stagger: true,
            color: Color::WHITE,
        };
        let mut parts = Vec::new();
        grid.parts(&mut parts);
        assert_eq!(parts.len(), 5 + 4 + 5);
        let ObstaclePart::Disc { center, .. } = parts[5] else { panic!() };
        assert_eq!(center, Vec2::new(-15.0, 0.0));
    }
}

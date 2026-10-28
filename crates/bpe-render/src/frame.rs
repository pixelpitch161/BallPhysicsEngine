//! What to draw for one video frame, built on the CPU from the simulation.

use bpe_scene::{Color, Container, ObstaclePart, Scene, Vec2};
use bpe_sim::{HitKind, Outline, Playback};
use bytemuck::{Pod, Zeroable};
use std::f32::consts::TAU;

/// Glow strength around arena walls.
const WALL_GLOW: f32 = 0.015;
const PEG_GLOW: f32 = 0.04;
/// How long the pieces of a shattered ring stay on screen, in seconds.
const SHATTER_SECONDS: f32 = 1.8;
/// Most string-art lines drawn in one frame.
const MAX_STRING_LINES: usize = 20_000;
/// Most recent hits searched for string-art lines in one frame.
const MAX_STRING_SCAN: usize = 100_000;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct BallInstance {
    pub center: [f32; 2],
    pub radius: f32,
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct TrailVertex {
    pub position: [f32; 2],
    pub color: [f32; 4],
    /// Signed distance from the centre line, and half width at this point.
    pub edge: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct FlashInstance {
    pub center: [f32; 2],
    pub size: f32,
    pub color: [f32; 4],
}

/// A wall hit that shader effects can react to.
#[derive(Clone, Copy, Debug)]
pub struct RecentHit {
    /// World position of the hit.
    pub position: [f32; 2],
    /// Seconds since the hit.
    pub age: f32,
    /// Impact speed relative to the scene's full-volume speed, 0 to 1.5.
    pub strength: f32,
    /// Linear colour of the ball that hit.
    pub color: [f32; 4],
}

/// How far back effects can see wall hits, in seconds.
const HIT_MEMORY: f64 = 3.0;
/// A wall, peg, line or shard, drawn as an instanced quad with a distance function.
/// `c.x` picks the shape: 0 ring or polygon outline, 1 box outline, 2 filled capsule.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct ShapeInstance {
    /// Outlines: centre, angle. Capsules: both end points.
    pub a: [f32; 4],
    /// Ring: radius, sides, half gap. Box: half width, half height.
    pub b: [f32; 4],
    /// Kind, half line width (or capsule radius), glow strength.
    pub c: [f32; 4],
    /// Linear-light colour. Alpha scales the line, not the glow.
    pub color: [f32; 4],
}

impl ShapeInstance {
    pub fn outline(outline: &Outline, color: [f32; 4], glow: f32) -> Self {
        match *outline {
            Outline::Ring { center, radius, sides, angle, gap_half, thickness } => Self {
                a: [center.x, center.y, angle, 0.0],
                b: [radius, sides as f32, gap_half, 0.0],
                c: [0.0, thickness * 0.5, glow, 0.0],
                color,
            },
            Outline::Box { center, half, angle, thickness } => Self {
                a: [center.x, center.y, angle, 0.0],
                b: [half.x, half.y, 0.0, 0.0],
                c: [1.0, thickness * 0.5, glow, 0.0],
                color,
            },
        }
    }

    pub fn capsule(from: Vec2, to: Vec2, radius: f32, color: [f32; 4], glow: f32) -> Self {
        Self { a: [from.x, from.y, to.x, to.y], b: [0.0; 4], c: [2.0, radius, glow, 0.0], color }
    }
}

#[derive(Clone, Debug, Default)]
pub struct FrameData {
    pub balls: Vec<BallInstance>,
    pub trails: Vec<TrailVertex>,
    pub flashes: Vec<FlashInstance>,
    /// The newest wall hits, newest first, at most [`crate::MAX_EFFECT_HITS`].
    pub hits: Vec<RecentHit>,
    /// 0 to 1: jumps when balls hit the wall and fades over a fraction of a second.
    pub pulse: f32,
    /// Walls, pegs, string-art lines and ring shards, drawn under the trails and balls.
    pub shapes: Vec<ShapeInstance>,
}

impl FrameData {
    /// Builds the frame for the playback's current time. `pixel` is the size of one output
    /// pixel in world units, so streaks keep at least a pixel of anti-aliasing margin.
    pub fn build(scene: &Scene, playback: &Playback, pixel: f32) -> Self {
        let mut frame = Self::default();
        frame.fill(scene, playback, pixel);
        frame
    }

    pub fn fill(&mut self, scene: &Scene, playback: &Playback, pixel: f32) {
        self.balls.clear();
        self.trails.clear();
        self.flashes.clear();
        self.hits.clear();
        self.shapes.clear();
        let sim = playback.simulation();
        let look = &scene.look;

        self.push_walls(scene, playback);
        self.push_strings(scene, playback);

        for ball in sim.balls() {
            self.balls.push(BallInstance { center: ball.position.into(), radius: ball.radius, color: ball.color });
        }

        if look.trail_seconds > 0.0 && look.trail_opacity > 0.0 {
            let mut points = Vec::new();
            for (index, ball) in sim.balls().iter().enumerate() {
                points.clear();
                points.push(ball.position);
                points.extend(sim.trail(index));
                let half_width = ball.radius * look.trail_width;
                push_streak(&mut self.trails, &points, half_width, ball.color, pixel);
            }
        }

        if look.flash_strength > 0.0 && look.flash_seconds > 0.0 {
            let now = sim.time();
            let full_speed = scene.sound.full_volume_speed.max(1.0);
            for hit in playback.recent_hits(look.flash_seconds as f64) {
                let age = (now - hit.time) as f32 / look.flash_seconds;
                let intensity = (hit.impact_speed / full_speed).min(1.5) * (-3.0 * age).exp() * 0.35;
                if intensity < 0.002 {
                    continue;
                }
                let c = sim.balls()[hit.ball as usize].color;
                let color = [
                    (c[0] + 1.0) * 0.5 * intensity,
                    (c[1] + 1.0) * 0.5 * intensity,
                    (c[2] + 1.0) * 0.5 * intensity,
                    1.0,
                ];
                self.flashes.push(FlashInstance { center: hit.position.into(), size: look.flash_size, color });
            }
        }

        if !scene.effects.is_empty() {
            let now = sim.time();
            let full_speed = scene.sound.full_volume_speed.max(1.0);
            let mut energy = 0.0;
            for hit in playback.recent_hits(HIT_MEMORY).iter().rev() {
                let age = (now - hit.time).max(0.0) as f32;
                let strength = (hit.impact_speed / full_speed).min(1.5);
                energy += strength * (-6.0 * age).exp();
                if self.hits.len() < crate::MAX_EFFECT_HITS {
                    let color = sim.balls().get(hit.ball as usize).map_or([1.0; 4], |b| b.color);
                    self.hits.push(RecentHit { position: hit.position.into(), age, strength, color });
                }
            }
            self.pulse = 1.0 - (-energy).exp();
        } else {
            self.pulse = 0.0;
        }
    }
}

impl FrameData {
    /// Arena outlines, pegs (lit up when hit) and the shards of rings that broke recently.
    fn push_walls(&mut self, scene: &Scene, playback: &Playback) {
        let sim = playback.simulation();
        let look = &scene.look;
        for (ring, outline) in sim.outlines() {
            if outline.thickness() > 0.0 {
                let color = wall_color(&scene.container, ring);
                self.shapes.push(ShapeInstance::outline(&outline, color, WALL_GLOW));
            }
        }

        let now = sim.time();
        let lit_seconds = look.flash_seconds.max(0.3) as f64;
        let mut last_hit = vec![f64::NEG_INFINITY; sim.obstacles().len()];
        for hit in playback.recent_hits(lit_seconds) {
            if let HitKind::Obstacle(i) = hit.kind {
                last_hit[i as usize] = hit.time;
            }
        }
        for (part, last) in sim.obstacles().iter().zip(last_hit) {
            let age = ((now - last) / lit_seconds).clamp(0.0, 1.0) as f32;
            let boost = 1.0 + 4.0 * look.flash_strength.min(2.0) * (1.0 - age) * (1.0 - age);
            match *part {
                ObstaclePart::Disc { center, radius, color } => {
                    let color = scaled(color.to_linear(), boost);
                    self.shapes.push(ShapeInstance::capsule(center, center, radius, color, PEG_GLOW * boost));
                }
                ObstaclePart::Segment { from, to, thickness, color } if thickness > 0.0 => {
                    self.shapes.push(ShapeInstance::capsule(from, to, thickness * 0.5, color.to_linear(), WALL_GLOW));
                }
                ObstaclePart::Segment { .. } => {}
            }
        }

        let pieces = look.shatter_pieces.min(400);
        let gravity = scene.physics.gravity;
        for (ring, broken) in sim.ring_breaks().iter().enumerate() {
            let Some(broken) = broken else { continue };
            let age = (now - broken.time) as f32;
            if pieces == 0 || !(0.0..SHATTER_SECONDS).contains(&age) {
                continue;
            }
            let Outline::Ring { gap_half, thickness, .. } = broken.outline else { continue };
            let fade = 1.0 - age / SHATTER_SECONDS;
            let color = scaled(wall_color(&scene.container, Some(ring as u32)), 1.0 + 3.0 * (-6.0 * age).exp());
            let color = [color[0], color[1], color[2], color[3] * fade * fade];
            let center = broken.outline.center();
            let span = TAU - 2.0 * gap_half;
            for j in 0..pieces {
                let seed = (ring as u64) << 32 | j as u64;
                let (u0, u1) =
                    (gap_half + span * j as f32 / pieces as f32, gap_half + span * (j + 1) as f32 / pieces as f32);
                let (from, to) = (broken.outline.point(u0), broken.outline.point(u1));
                let middle = (from + to) * 0.5;
                let outward = (middle - center).normalize_or_zero();
                let speed = 160.0 + 260.0 * bpe_sim::noise(seed * 3);
                let velocity = outward * speed + outward.perp() * 120.0 * (bpe_sim::noise(seed * 3 + 1) - 0.5);
                let spin = 8.0 * (bpe_sim::noise(seed * 3 + 2) - 0.5) * age;
                let moved = middle + velocity * age + gravity * (0.5 * age * age);
                let half = Vec2::from_angle(spin).rotate((to - from) * 0.5);
                let radius = (thickness * 0.5).max(1.0);
                let glow = WALL_GLOW * 2.0 * fade * fade;
                self.shapes.push(ShapeInstance::capsule(moved - half, moved + half, radius, color, glow));
            }
        }
    }

    /// String art: a line from each ball to each of its most recent hit points.
    fn push_strings(&mut self, scene: &Scene, playback: &Playback) {
        let look = &scene.look;
        if look.string_lines == 0 || look.string_opacity <= 0.0 || look.string_width <= 0.0 {
            return;
        }
        let balls = playback.simulation().balls();
        let per_ball = look.string_lines as usize;
        let mut counts = vec![0usize; balls.len()];
        let mut full = 0;
        let mut total = 0;
        // Only look this far back, so long clips with many balls stay quick to draw.
        for hit in playback.hits().iter().rev().take(MAX_STRING_SCAN) {
            if matches!(hit.kind, HitKind::Escape(_)) {
                continue;
            }
            let count = &mut counts[hit.ball as usize];
            if *count >= per_ball {
                continue;
            }
            *count += 1;
            if *count == per_ball {
                full += 1;
            }
            let ball = &balls[hit.ball as usize];
            let c = ball.color;
            let color = [c[0], c[1], c[2], look.string_opacity];
            self.shapes.push(ShapeInstance::capsule(hit.position, ball.position, look.string_width * 0.5, color, 0.0));
            total += 1;
            if full == balls.len() || total >= MAX_STRING_LINES {
                break;
            }
        }
    }
}

/// Colour of the arena wall, or of ring `ring` for escape rings.
fn wall_color(container: &Container, ring: Option<u32>) -> [f32; 4] {
    match (container, ring) {
        (Container::Rings { gradient, count, .. }, Some(k)) => {
            let t = if *count > 1 { k as f32 / (*count - 1) as f32 } else { 0.0 };
            Color::sample_gradient(gradient, t)
        }
        _ => container.color().to_linear(),
    }
}

fn scaled(c: [f32; 4], k: f32) -> [f32; 4] {
    [c[0] * k, c[1] * k, c[2] * k, c[3]]
}

/// Appends a tapered, fading streak through `points` (newest first) as a triangle list.
fn push_streak(out: &mut Vec<TrailVertex>, points: &[Vec2], half_width: f32, color: [f32; 4], pixel: f32) {
    let n = points.len();
    if n < 2 || half_width <= 0.0 {
        return;
    }
    let direction = |i: usize| (points[i + 1] - points[i]).try_normalize();
    // At a gentle bend the two segments share a side vector so the strip has no gaps. At a sharp
    // bend (a bounce) each segment keeps its own, which avoids a folded spike at the corner.
    let side_at = |i: usize, own: Vec2| {
        let neighbour = if i == 0 || i + 1 >= n {
            None
        } else if own == direction(i).unwrap_or(own) {
            direction(i - 1)
        } else {
            direction(i)
        };
        match neighbour {
            Some(other) if other.dot(own) > 0.5 => (other + own).normalize().perp(),
            _ => own.perp(),
        }
    };
    let vertex = |i: usize, side: Vec2, sign: f32| {
        let t = i as f32 / (n - 1) as f32;
        let width = half_width * (1.0 - t);
        let reach = width + pixel;
        TrailVertex {
            position: (points[i] + side * reach * sign).into(),
            color: [color[0], color[1], color[2], (1.0 - t) * (1.0 - t)],
            edge: [reach * sign, width],
        }
    };
    for i in 0..n - 1 {
        let Some(own) = direction(i) else { continue };
        let (s0, s1) = (side_at(i, own), side_at(i + 1, own));
        let (a, b) = (vertex(i, s0, 1.0), vertex(i, s0, -1.0));
        let (c, d) = (vertex(i + 1, s1, 1.0), vertex(i + 1, s1, -1.0));
        out.extend_from_slice(&[a, b, c, b, d, c]);
    }
}

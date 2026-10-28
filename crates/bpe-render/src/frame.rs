//! What to draw for one video frame, built on the CPU from the simulation.

use bpe_scene::{Scene, Vec2};
use bpe_sim::Playback;
use bytemuck::{Pod, Zeroable};

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

#[derive(Clone, Debug, Default)]
pub struct FrameData {
    pub balls: Vec<BallInstance>,
    pub trails: Vec<TrailVertex>,
    pub flashes: Vec<FlashInstance>,
    /// The newest wall hits, newest first, at most [`crate::MAX_EFFECT_HITS`].
    pub hits: Vec<RecentHit>,
    /// 0 to 1: jumps when balls hit the wall and fades over a fraction of a second.
    pub pulse: f32,
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
        let sim = playback.simulation();
        let look = &scene.look;

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

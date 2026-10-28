//! What to draw for one video frame, built on the CPU from the simulation.

use bpe_scene::{Color, Container, ObstaclePart, Scene, Vec2};
use bpe_sim::{Counts, Event, EventKind, Outline, Playback, Simulation};
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
    /// Every event in the last [`EVENT_MEMORY`] seconds, oldest first, for particles, shaders
    /// and captions.
    pub events: Vec<Event>,
    /// The song's bass, mid and treble (0 to 1) and how far through the current beat it is.
    /// All zero with no song.
    pub audio: [f32; 4],
    /// The song's overall loudness, 0 to 1.
    pub level: f32,
    /// Running totals now: balls, hits, escapes, rule counters.
    pub counts: Counts,
    /// Swarms and big fluids (GPU workstream).
    pub gpu: crate::gpu::GpuFrame,
    /// Hit particles (GPU workstream).
    pub particles: crate::particles::ParticleFrame,
    /// The 3D picture (3D workstream).
    pub space: crate::space::SpaceFrame,
    /// Captions (captions workstream).
    pub text: crate::text::TextFrame,
    /// Jelly balls, cloth and liquid as filled triangles, drawn over trails and under balls
    /// (physics workstream). `edge.x` is each vertex's distance inside the shape's edge.
    pub soft: Vec<TrailVertex>,
    // ---- Audio-reactive visuals (WS5) ----
    /// Analysis of the finished soundtrack, the song plus the bounce sounds, set by whoever mixed
    /// it. When present, `audio` and `level` follow it, and so do modulators on visual settings.
    /// The simulation never sees it.
    pub mix: Option<std::sync::Arc<bpe_music::AudioFeatures>>,
    /// The song's analysis, taken from the playback by [`FrameData::fill`].
    pub song: Option<std::sync::Arc<bpe_music::AudioFeatures>>,
}

/// The music at one moment, as modulators read it.
pub fn levels(features: &bpe_music::AudioFeatures, time: f64) -> bpe_scene::Levels {
    bpe_sim::music_levels(features, time)
}

/// How far through the current beat `time` is; see [`bpe_sim::resting_beat_phase`].
pub fn beat_phase(features: Option<&bpe_music::AudioFeatures>, time: f64) -> f32 {
    bpe_sim::resting_beat_phase(features, time)
}

/// How long a rule's screen flash lasts.
const RULE_FLASH_SECONDS: f64 = 0.8;

/// How far back [`FrameData::events`] reaches, in seconds.
pub const EVENT_MEMORY: f64 = 3.0;

impl FrameData {
    /// Runs `f` with the song and mix as modulators read them.
    pub fn with_audio<R>(&self, f: impl FnOnce(&bpe_scene::Audio) -> R) -> R {
        let song = self.song.as_deref().map(|s| move |t: f64| levels(s, t));
        let mix = self.mix.as_deref().map(|m| move |t: f64| levels(m, t));
        let audio = bpe_scene::Audio {
            song: song.as_ref().map(|f| f as bpe_scene::Listener),
            mix: mix.as_ref().map(|f| f as bpe_scene::Listener),
        };
        f(&audio)
    }

    /// Builds the frame for the playback's current time. `pixel` is the size of one output
    /// pixel in world units, so streaks keep at least a pixel of anti-aliasing margin.
    pub fn build(scene: &Scene, playback: &Playback, pixel: f32) -> Self {
        let mut frame = Self::default();
        frame.fill(scene, playback, pixel);
        frame
    }

    pub fn fill(&mut self, scene: &Scene, playback: &Playback, pixel: f32) {
        self.song = playback.simulation().inputs().music.clone();
        let animated;
        let scene = if scene.animation.is_empty() && scene.modulators.is_empty() {
            scene
        } else {
            animated = self.with_audio(|audio| scene.at_time_with(playback.simulation().time() as f32, audio));
            &animated
        };
        self.balls.clear();
        self.trails.clear();
        self.flashes.clear();
        self.hits.clear();
        self.shapes.clear();
        self.soft.clear();
        let sim = playback.simulation();
        let look = &scene.look;

        // 3D (WS4): the 3D picture replaces walls, strings, balls, trails and flashes, and the
        // flat simulation has no balls for them.
        if playback.space().is_some() {
            self.pulse = 0.0;
        } else {
            self.push_walls(scene, playback);
            self.push_strings(scene, playback);

            for ball in sim.balls().iter().filter(|b| b.alive) {
                self.balls.push(BallInstance { center: ball.position.into(), radius: ball.radius, color: ball.color });
            }
            // Soft bodies, billiard pockets and spinning balls' glints (physics workstream).
            self.push_soft(scene, sim, pixel);

            if look.trail_seconds > 0.0 && look.trail_opacity > 0.0 {
                let mut points = Vec::new();
                for (index, ball) in sim.balls().iter().enumerate() {
                    if !ball.alive {
                        continue;
                    }
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
                for hit in playback.recent_events(look.flash_seconds as f64).iter().filter(|e| e.kind.is_bounce()) {
                    let age = (now - hit.time) as f32 / look.flash_seconds;
                    let intensity = (hit.speed / full_speed).min(1.5) * (-3.0 * age).exp() * 0.35;
                    if intensity < 0.002 {
                        continue;
                    }
                    let c = event_color(sim, hit);
                    let color = [
                        (c[0] + 1.0) * 0.5 * intensity,
                        (c[1] + 1.0) * 0.5 * intensity,
                        (c[2] + 1.0) * 0.5 * intensity,
                        1.0,
                    ];
                    self.flashes.push(FlashInstance { center: hit.position.into(), size: look.flash_size, color });
                }
            }

            // Rule flashes cover the whole picture and fade over about half a second. They are added
            // up into one full-screen flash, so a rule firing hundreds of times is still one quad.
            let rules = sim.rules();
            if !rules.is_empty() {
                let now = sim.time();
                let mut sum = [0.0f32; 3];
                for event in playback.recent_events(RULE_FLASH_SECONDS).iter() {
                    let Some(bpe_sim::rules::RuleOutput::Flash { color, strength }) = rules.output(event) else {
                        continue;
                    };
                    let fade = (-6.0 * (now - event.time).max(0.0) as f32).exp() * strength * 0.5;
                    for (total, c) in sum.iter_mut().zip(color) {
                        *total += c * fade;
                    }
                }
                if sum.iter().any(|&c| c > 1e-3) {
                    let size = 3.0 * (scene.video.width + scene.video.height) as f32;
                    // Alpha above 1 tells the shader this flash ignores the scene's own flash strength.
                    let color = [sum[0].min(1.0), sum[1].min(1.0), sum[2].min(1.0), 2.0];
                    self.flashes.push(FlashInstance { center: scene.container.center().into(), size, color });
                }
            }

            if !scene.effects.is_empty() {
                let now = sim.time();
                let full_speed = scene.sound.full_volume_speed.max(1.0);
                let mut energy = 0.0;
                for hit in playback.recent_events(HIT_MEMORY).iter().rev().filter(|e| e.kind.is_bounce()) {
                    let age = (now - hit.time).max(0.0) as f32;
                    let strength = (hit.speed / full_speed).min(1.5);
                    energy += strength * (-6.0 * age).exp();
                    if self.hits.len() < crate::MAX_EFFECT_HITS {
                        let color = event_color(sim, hit);
                        self.hits.push(RecentHit { position: hit.position.into(), age, strength, color });
                    }
                }
                self.pulse = 1.0 - (-energy).exp();
            } else {
                self.pulse = 0.0;
            }
        }

        let now = sim.time();
        self.events.clear();
        self.events.extend_from_slice(playback.recent_events(EVENT_MEMORY));
        (self.audio, self.level) = match &sim.inputs().music {
            Some(music) => {
                let at = music.at(now);
                ([at.bass, at.mid, at.treble, 0.0], at.level)
            }
            None => ([0.0; 4], 0.0),
        };
        // Visuals follow everything you hear when the mixed soundtrack is known; beats still come
        // from the song, whose beat is steadier than the bounces'.
        if let Some(mix) = &self.mix {
            let at = mix.at(now);
            (self.audio, self.level) = ([at.bass, at.mid, at.treble, 0.0], at.level);
        }
        self.audio[3] = beat_phase(self.song.as_deref().or(self.mix.as_deref()), now);
        self.counts = playback.counts_at(now);
        self.gpu.fill(scene, playback);
        self.space.fill(scene, playback);
        self.particles.fill(scene, playback, self.space.active.then_some(&self.space));
        if self.space.active {
            // 3D (WS4): effects react to hits where they show on screen.
            self.pulse = self.space.effect_hits(scene, now, playback.events(), &mut self.hits);
        }
        self.text.fill(scene, playback, &self.counts);
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
        if let (Some(place), Container::Custom { shape, thickness, color, .. }) =
            (sim.arena_placement(), &scene.container)
            && let Some(index) = scene.shape_index(shape)
        {
            self.push_shape_outline(sim, index as usize, place, *thickness, color.to_linear(), WALL_GLOW);
        }

        let now = sim.time();
        let lit_seconds = look.flash_seconds.max(0.3) as f64;
        let mut last_hit = vec![f64::NEG_INFINITY; sim.obstacles().len()];
        for hit in playback.recent_hits(lit_seconds) {
            if let EventKind::Obstacle(i) = hit.kind {
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
                ObstaclePart::Shape { shape, place, thickness, color } => {
                    let color = scaled(color.to_linear(), boost);
                    self.push_shape_outline(sim, shape as usize, place, thickness, color, WALL_GLOW * boost);
                }
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
            let Some(index) = hit.ball_index().filter(|&i| i < balls.len() && hit.kind.is_hit()) else {
                continue;
            };
            let count = &mut counts[index];
            if *count >= per_ball {
                continue;
            }
            *count += 1;
            if *count == per_ball {
                full += 1;
            }
            let ball = &balls[index];
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
    // Each segment's direction, worked out once: every segment's corners need its neighbours' too.
    let directions: Vec<Option<Vec2>> = points.windows(2).map(|w| (w[1] - w[0]).try_normalize()).collect();
    let direction = |i: usize| directions[i];
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

impl FrameData {
    /// A custom shape's outline at `place`, as a chain of rounded line pieces.
    pub fn push_shape_outline(
        &mut self,
        sim: &bpe_sim::Simulation,
        shape: usize,
        place: bpe_scene::Placement,
        thickness: f32,
        color: [f32; 4],
        glow: f32,
    ) {
        let Some(lines) = sim.shape_lines(shape) else { return };
        // A hairline still shows, so a shape drawn with no thickness doesn't vanish.
        let radius = (thickness * 0.5).max(0.75);
        for line in lines {
            for (from, to) in line.pieces() {
                self.shapes.push(ShapeInstance::capsule(place.apply(from), place.apply(to), radius, color, glow));
            }
        }
    }
}

/// Lighter version of a linear colour, for the middle of a jelly ball.
fn lighter(c: [f32; 4], by: f32) -> [f32; 4] {
    [c[0] + (1.0 - c[0]) * by, c[1] + (1.0 - c[1]) * by, c[2] + (1.0 - c[2]) * by, c[3]]
}

fn soft_vertex(position: Vec2, color: [f32; 4], inside: f32) -> TrailVertex {
    TrailVertex { position: position.into(), color, edge: [inside, 0.0] }
}

/// Points around a circle used for drawing liquid drops: few, as there are thousands.
const DROP_SIDES: usize = 6;

impl FrameData {
    /// Jelly balls (filled, lighter in the middle, with a rim), ropes and cloth edges as
    /// rounded lines, cloth as shaded triangles, liquid as overlapping drops, billiard pockets,
    /// and a glint on each spinning ball.
    pub fn push_soft(&mut self, scene: &Scene, sim: &Simulation, pixel: f32) {
        for pocket in &scene.physics.contact.pockets {
            let color = [0.01, 0.01, 0.012, 1.0];
            self.shapes.push(ShapeInstance::capsule(pocket.position, pocket.position, pocket.radius, color, 0.0));
        }
        if scene.physics.ball_collisions && scene.physics.contact.spin && scene.physics.contact.quality > 0 {
            for (i, ball) in sim.balls().iter().enumerate().filter(|(_, b)| b.alive) {
                let at = ball.position + Vec2::from_angle(sim.ball_angle(i)) * ball.radius * 0.55;
                let color = [0.35, 0.35, 0.35, 1.0];
                self.flashes.push(FlashInstance { center: at.into(), size: ball.radius * 0.45, color });
            }
        }
        let world = sim.soft();
        let points = &world.points;
        for (index, body) in world.bodies.iter().enumerate() {
            let range = body.points.start as usize..body.points.end as usize;
            let color = body.color;
            match body.kind {
                bpe_sim::bodies::BodyKind::Jelly => {
                    let ring = &points[range];
                    let n = ring.len();
                    if n < 3 {
                        continue;
                    }
                    // The skin's outer edge: each point's middle pushed out by its radius.
                    let middle = ring.iter().map(|p| p.position).sum::<Vec2>() / n as f32;
                    let edge = |k: usize| {
                        let p = &ring[k % n];
                        p.position + (p.position - middle).normalize_or_zero() * p.radius
                    };
                    let reach = (0..n).map(|k| edge(k).distance(middle)).fold(0.0f32, f32::max);
                    let inner = lighter(color, 0.35);
                    for k in 0..n {
                        let (a, b) = (edge(k), edge(k + 1));
                        let out = |p: Vec2| p + (p - middle).normalize_or_zero() * pixel;
                        self.soft.extend_from_slice(&[
                            soft_vertex(middle, inner, reach),
                            soft_vertex(out(a), color, -pixel),
                            soft_vertex(out(b), color, -pixel),
                        ]);
                    }
                    let rim = lighter(color, 0.6);
                    let width = (reach * 0.03).max(pixel);
                    for k in 0..n {
                        let (a, b) = (edge(k), edge(k + 1));
                        let inset = |p: Vec2| p + (middle - p).normalize_or_zero() * width;
                        self.shapes.push(ShapeInstance::capsule(inset(a), inset(b), width, rim, 0.0));
                    }
                }
                bpe_sim::bodies::BodyKind::Rope => {
                    let radius = body.thickness * 0.5;
                    for (a, b) in world.links_of(index) {
                        let (a, b) = (points[a as usize].position, points[b as usize].position);
                        self.shapes.push(ShapeInstance::capsule(a, b, radius, color, 0.0));
                    }
                }
                bpe_sim::bodies::BodyKind::Cloth => self.push_cloth(world, index, pixel),
                bpe_sim::bodies::BodyKind::Fluid => {
                    for p in &points[range] {
                        // Drops drawn bigger than they are, so neighbours merge into one surface.
                        let r = p.radius * 1.45;
                        let corner =
                            |k: usize| p.position + Vec2::from_angle(k as f32 * TAU / DROP_SIDES as f32) * (r + pixel);
                        for k in 0..DROP_SIDES {
                            self.soft.extend_from_slice(&[
                                soft_vertex(p.position, color, r),
                                soft_vertex(corner(k), color, -pixel),
                                soft_vertex(corner(k + 1), color, -pixel),
                            ]);
                        }
                    }
                }
            }
        }
    }

    /// A cloth as triangles between its points, darker where it is squashed and lighter where
    /// it is stretched, so folds show. Torn cells are left out.
    fn push_cloth(&mut self, world: &bpe_sim::bodies::SoftWorld, index: usize, pixel: f32) {
        let body = &world.bodies[index];
        let (columns, rows) = (body.columns as usize, body.rows as usize);
        if columns < 2 || rows < 2 {
            return;
        }
        let first = body.points.start;
        let at = |c: usize, r: usize| world.points[first as usize + r * columns + c].position;
        // Which links between grid neighbours still hold.
        let mut across = vec![false; columns * rows];
        let mut down = vec![false; columns * rows];
        for (a, b) in world.links_of(index) {
            let (a, b) = ((a.min(b) - first) as usize, (a.max(b) - first) as usize);
            if b == a + 1 && b % columns != 0 {
                across[a] = true;
            } else if b == a + columns {
                down[a] = true;
            }
        }
        // The spacing of points along the pinned top edge, which keeps its length.
        let rest = ((at(columns - 1, 0) - at(0, 0)).length() / (columns - 1) as f32).max(1e-3);
        let shade = |area: f32| {
            let stretch = (area.abs().sqrt() / rest).clamp(0.5, 1.5);
            0.55 + 0.45 * stretch.min(1.2) / 1.2
        };
        for r in 0..rows - 1 {
            for c in 0..columns - 1 {
                let k = r * columns + c;
                if !(across[k] && down[k] && across[k + columns] && down[k + 1]) {
                    continue;
                }
                let (p00, p10, p01, p11) = (at(c, r), at(c + 1, r), at(c, r + 1), at(c + 1, r + 1));
                let area = (p10 - p00).perp_dot(p01 - p00);
                let s = shade(area);
                let checker = if (c + r) % 2 == 0 { 1.0 } else { 0.93 };
                let color =
                    [body.color[0] * s * checker, body.color[1] * s * checker, body.color[2] * s * checker, 1.0];
                // Large distance inside: no smoothing across the shared edges of cells.
                let deep = pixel * 4.0;
                self.soft.extend_from_slice(&[
                    soft_vertex(p00, color, deep),
                    soft_vertex(p10, color, deep),
                    soft_vertex(p11, color, deep),
                    soft_vertex(p00, color, deep),
                    soft_vertex(p11, color, deep),
                    soft_vertex(p01, color, deep),
                ]);
            }
        }
    }
}

/// Colour of whatever made an event: its ball, or the soft body that hit a wall.
fn event_color(sim: &Simulation, event: &Event) -> [f32; 4] {
    if let Some(ball) = event.ball_index().and_then(|b| sim.balls().get(b)) {
        return ball.color;
    }
    match event.kind {
        EventKind::Soft { body } => sim.soft().bodies.get(body as usize).map_or([1.0; 4], |b| b.color),
        _ => [1.0; 4],
    }
}

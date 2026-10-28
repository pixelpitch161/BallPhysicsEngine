//! The 3D view: lit balls inside a glass sphere, a glass cube or an endless tunnel.
//!
//! [`SpaceFrame`] is built on the CPU from the 3D simulation, like [`crate::FrameData`] is for 2D.
//! [`SpaceRenderer`] draws it into the same HDR picture the 2D passes use, so shader effects, bloom
//! and captions work unchanged. It draws in three passes:
//!
//! 1. Shadows: the balls from the key light, into a depth map.
//! 2. The scene: the container's inside walls and the sky are ray traced in one full-screen pass,
//!    then the balls as ray-traced sphere impostors with correct depth, then trails as ribbons
//!    facing the camera and glows for hits and shards.
//! 3. Glass: the container's front surface, its edges and the tunnel's rings, ray traced again and
//!    blended over the picture wherever they are in front of what's already there.

mod renderer;

pub use renderer::SpaceRenderer;

use bpe_scene::{BallStyle, CameraMode, Color, Container3d, Scene, Space, Vec2, Vec3};
use bpe_sim::space::SpaceSimulation;
use bytemuck::{Pod, Zeroable};
use glam::camera::rh::{proj::directx, view};
use glam::{Mat4, Vec4};

/// Closest distance the camera draws, in world pixels.
pub const NEAR: f32 = 4.0;
/// Most tunnel rings, and recent hits, the shaders see at once.
pub const MAX_GATES: usize = 24;
pub const MAX_GLOW_HITS: usize = 24;
/// Most hit sparks drawn at once, newest first, so a crowd hitting together doesn't blow out.
const MAX_SPARKS: usize = 48;
/// How long the pieces of a broken tunnel ring stay on screen, in seconds.
const SHATTER_SECONDS: f32 = 1.8;
/// How long a hit lights up the wall, in seconds, at the scene's flash length of 1.
const HIT_GLOW_SECONDS: f64 = 0.6;
/// How far back effects can see hits, in seconds, as in 2D.
const EFFECT_HIT_MEMORY: f64 = 3.0;
/// Seconds of a followed ball's past positions averaged to steady the camera.
const FOLLOW_SMOOTHING: usize = 24;

/// A ball as the shaders see it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct SpaceBall {
    pub center: [f32; 3],
    pub radius: f32,
    /// Linear-light RGBA. A negative alpha adds light (see [`crate::frame::blend_alpha`]).
    pub color: [f32; 4],
    /// A look's second colour (patterns and gradients), linear light.
    pub color2: [f32; 4],
    pub outline_color: [f32; 4],
    /// Shape, fill (-1 draws the classic ball lit by the 3D material), pattern size, roundness.
    pub form: [f32; 4],
    /// Outline width in pixels, unused, opacity, turn around the up axis in radians.
    pub look: [f32; 4],
    /// For effects on things: the ball's group plus one (0 for none), and a random number from
    /// 0 to 1.
    pub tag: [f32; 4],
}

impl SpaceBall {
    /// A ball drawn the classic way, lit by the scene's 3D material.
    pub fn classic(center: Vec3, radius: f32, color: [f32; 4]) -> Self {
        let color = crate::frame::blend_alpha(color);
        Self {
            center: center.into(),
            radius,
            color,
            color2: color,
            outline_color: color,
            form: [0.0, -1.0, 1.0, 0.0],
            look: [0.0, 0.0, 1.0, 0.0],
            tag: [0.0; 4],
        }
    }

    /// The same ball, marked as one of group `group` for effects on things.
    pub fn tagged(mut self, group: u32, index: usize) -> Self {
        self.tag = [group as f32 + 1.0, crate::frame::ball_seed(index), 0.0, 0.0];
        self
    }

    /// A ball with its own look at `time` seconds. Shapes other than circles are drawn as puffy
    /// stickers that face the camera; patterns wrap around the ball and turn with it.
    pub fn styled(center: Vec3, radius: f32, color: [f32; 4], style: &BallStyle, time: f32) -> Self {
        use crate::frame::{blend_alpha, fill_code, shape_code, style_colors};
        let (color, color2, outline) = style_colors(style, color, time);
        Self {
            center: center.into(),
            radius,
            color: blend_alpha(color),
            color2: blend_alpha(color2),
            outline_color: blend_alpha(outline),
            form: [
                shape_code(style.shape),
                fill_code(style.fill),
                style.pattern_size.max(0.1),
                style.roundness.clamp(0.0, 1.0),
            ],
            look: [
                style.outline.max(0.0),
                0.0,
                style.opacity.clamp(0.0, 1.0),
                (style.turn + style.spin * time).to_radians(),
            ],
            tag: [0.0; 4],
        }
    }
}

/// One corner of a trail ribbon.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct RibbonVertex {
    pub position: [f32; 3],
    /// Signed distance from the ribbon's middle, divided by its half width: -1 to 1.
    pub across: f32,
    pub color: [f32; 4],
}

/// A soft glowing dot: a spark where a ball hit, or a piece of a broken ring.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct Glow {
    pub center: [f32; 3],
    pub size: f32,
    pub color: [f32; 4],
}

/// The camera for one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraFrame {
    pub position: Vec3,
    pub target: Vec3,
    pub view: Mat4,
    pub projection: Mat4,
    pub view_projection: Mat4,
    /// Vertical field of view in radians.
    pub fov: f32,
    pub aspect: f32,
}

impl Default for CameraFrame {
    fn default() -> Self {
        Self::new(Vec3::new(0.0, 0.0, -1000.0), Vec3::ZERO, 40.0, 9.0 / 16.0)
    }
}

impl CameraFrame {
    /// A camera at `position` looking at `target`, with "up" on screen being -y in the world.
    pub fn new(position: Vec3, target: Vec3, fov_degrees: f32, aspect: f32) -> Self {
        let target = if (target - position).length_squared() < 1e-6 { position + Vec3::Z } else { target };
        let forward = (target - position).normalize();
        // Looking straight up or down, pick another "up" so the view stays defined.
        let up = if forward.cross(Vec3::NEG_Y).length_squared() < 1e-6 { Vec3::Z } else { Vec3::NEG_Y };
        let fov = fov_degrees.clamp(5.0, 150.0).to_radians();
        let view = view::look_at_mat4(position, target, up);
        // Reversed depth (near is 1, far is 0) keeps depth precise across a long tunnel.
        let projection = directx::perspective_infinite_reverse(fov, aspect.max(0.01), NEAR);
        Self { position, target, view, projection, view_projection: projection * view, fov, aspect }
    }

    /// Where a world point lands on screen, in 2D video pixels (origin in the middle, y down),
    /// or None if it is behind the camera.
    pub fn to_screen(&self, point: Vec3, video: Vec2) -> Option<Vec2> {
        let clip = self.view_projection * point.extend(1.0);
        if clip.w <= 0.0 {
            return None;
        }
        Some(Vec2::new(clip.x / clip.w * video.x * 0.5, -clip.y / clip.w * video.y * 0.5))
    }

    /// Size in world pixels of one picture pixel at distance `distance` from the camera, for a
    /// picture `height` pixels tall.
    pub fn pixel_at(&self, distance: f32, height: f32) -> f32 {
        2.0 * distance * (self.fov * 0.5).tan() / height.max(1.0)
    }
}

/// The key light's camera, which the shadow map is drawn from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightFrame {
    pub view_projection: Mat4,
    /// Depth units (0 to 1) per world pixel, for ray-traced depth in the shadow pass.
    pub depth_scale: f32,
    /// World pixels per shadow map texel.
    pub texel: f32,
}

impl Default for LightFrame {
    fn default() -> Self {
        Self { view_projection: Mat4::IDENTITY, depth_scale: 0.0, texel: 1.0 }
    }
}

/// What the container looks like at one moment.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ContainerFrame {
    /// 0 sphere, 1 cube, 2 tunnel.
    pub kind: u32,
    /// Radius, or half the cube's side, after shrinking.
    pub size: f32,
    /// Radians the container has turned around the up axis.
    pub angle: f32,
    /// Where the tunnel's back wall is.
    pub back: f32,
}

/// One tunnel ring as the glass pass sees it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GateFrame {
    pub z: f32,
    pub angle: f32,
    pub gap_half: f32,
    /// Index of the ring, for its colour.
    pub index: u32,
}

/// A recent hit that lights up the container wall.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WallGlow {
    pub position: Vec3,
    /// 0 to about 1.5, fading with age.
    pub strength: f32,
    pub color: [f32; 4],
}

/// Everything the 3D view draws for one frame. Empty (and `active` false) for a flat scene.
#[derive(Clone, Debug, Default)]
pub struct SpaceFrame {
    pub active: bool,
    pub camera: CameraFrame,
    pub light: LightFrame,
    pub container: ContainerFrame,
    pub balls: Vec<SpaceBall>,
    pub ribbons: Vec<RibbonVertex>,
    pub glows: Vec<Glow>,
    /// Halos around balls whose look has a glow. `color.a` is the ball's radius as a fraction
    /// of `size`; the halo is drawn outside it.
    pub halos: Vec<Glow>,
    /// Unbroken tunnel rings, nearest first, at most [`MAX_GATES`].
    pub gates: Vec<GateFrame>,
    /// Newest first, at most [`MAX_GLOW_HITS`].
    pub wall_glows: Vec<WallGlow>,
    /// The 3D settings, at their animated values.
    pub space: Space,
}

/// What other parts of the renderer (particles) need to draw into the 3D view: the camera and the
/// depth of what's already drawn.
pub struct SpaceView<'a> {
    pub view_projection: Mat4,
    pub camera_position: Vec3,
    /// Reversed depth (1 near, 0 far) of the 3D scene, `Depth32Float`, the size of the picture.
    pub depth: &'a wgpu::TextureView,
}

impl SpaceFrame {
    /// Builds the frame for the playback's current time. `scene` gives the animated look.
    pub fn fill(&mut self, scene: &Scene, playback: &bpe_sim::Playback) {
        match playback.space() {
            Some(sim) => self.fill_from(scene, sim, playback.events()),
            None => self.clear(),
        }
    }

    /// Builds the frame from a 3D simulation at its current time and the events so far.
    pub fn fill_from(&mut self, scene: &Scene, sim: &SpaceSimulation, events: &[bpe_sim::Event]) {
        let Some(space) = scene.space() else {
            self.clear();
            return;
        };
        self.active = true;
        self.space = space.clone();
        self.balls.clear();
        self.ribbons.clear();
        self.glows.clear();
        self.gates.clear();
        self.wall_glows.clear();
        let video = &scene.video;
        let aspect = video.width as f32 / video.height.max(1) as f32;
        let arena = sim.arena();
        let time = sim.time();

        self.container = ContainerFrame {
            kind: match space.container {
                Container3d::Sphere { .. } => 0,
                Container3d::Cube { .. } => 1,
                Container3d::Tunnel { .. } => 2,
            },
            size: space.container.size() * arena.scale,
            angle: arena.angle,
            back: arena.back,
        };
        self.camera = camera_for(space, sim, time as f32, aspect);

        self.halos.clear();
        let now = time as f32;
        for (index, ball) in sim.balls().iter().enumerate() {
            // Removed balls keep their slot, so ball numbers in events still line up, but draw nothing.
            let radius = if ball.alive { ball.radius } else { 0.0 };
            match style_of(scene, ball.group) {
                None => {
                    self.balls.push(SpaceBall::classic(ball.position, radius, ball.color).tagged(ball.group, index))
                }
                Some(style) => {
                    let drawn =
                        SpaceBall::styled(ball.position, radius, ball.color, style, now).tagged(ball.group, index);
                    if style.glow > 0.0 && radius > 0.0 && style.opacity > 0.0 {
                        // The halo reaches out like the 2D one: about 2.5 glow sizes past the edge.
                        let size = radius * (1.0 + style.glow_size.max(0.05) * 2.5);
                        let c = drawn.color;
                        let k = style.glow * style.opacity.clamp(0.0, 1.0) * if c[3] < 0.0 { -c[3] } else { 1.0 };
                        let c = if style.outline > 0.0 { drawn.outline_color } else { c };
                        let color = [c[0] * k, c[1] * k, c[2] * k, radius / size];
                        self.halos.push(Glow { center: ball.position.into(), size, color });
                    }
                    self.balls.push(drawn);
                }
            }
        }
        self.gates.extend(sim.gates().take(MAX_GATES).map(|g| GateFrame {
            z: g.z,
            angle: g.angle,
            gap_half: g.gap_half,
            index: g.index,
        }));
        self.light = light_for(space, &self.container, &self.gates);
        self.push_ribbons(scene, sim);
        self.push_hits(scene, sim, events);
        self.push_shards(scene, sim);
    }

    pub fn clear(&mut self) {
        self.active = false;
        self.balls.clear();
        self.halos.clear();
        self.ribbons.clear();
        self.glows.clear();
        self.gates.clear();
        self.wall_glows.clear();
    }

    /// Trails as ribbons that always face the camera, tapering and fading towards the tail. Balls
    /// with their own look use its trail settings instead of the scene's.
    fn push_ribbons(&mut self, scene: &Scene, sim: &SpaceSimulation) {
        let look = &scene.look;
        let classic = look.trail_seconds > 0.0 && look.trail_opacity > 0.0 && look.trail_width > 0.0;
        // The simulation keeps enough trail for the longest one drawn; the scene's own trails
        // stop where they always did.
        let classic_samples = sim.trail_samples(look.trail_seconds);
        let time = sim.time() as f32;
        let eye = self.camera.position;
        let mut points = Vec::new();
        for (index, ball) in sim.balls().iter().enumerate() {
            if !ball.alive {
                continue;
            }
            let (samples, half_width, c, opacity) = match style_of(scene, ball.group) {
                None if classic => (classic_samples, ball.radius * look.trail_width, ball.color, look.trail_opacity),
                Some(style) if style.trail_seconds > 0.0 && style.trail_opacity > 0.0 && style.trail_width > 0.0 => {
                    let (fill, second, outline) = crate::frame::style_colors(style, ball.color, time);
                    let c = match style.trail_color {
                        bpe_scene::TrailColor::Ball => fill,
                        bpe_scene::TrailColor::Second => second,
                        bpe_scene::TrailColor::Outline => outline,
                    };
                    let opacity = style.trail_opacity.clamp(0.0, 1.0) * style.opacity.clamp(0.0, 1.0);
                    (sim.trail_samples(style.trail_seconds), ball.radius * style.trail_width, c, opacity)
                }
                _ => continue,
            };
            points.clear();
            points.push(ball.position);
            points.extend(sim.trail(index).take(samples));
            let n = points.len();
            if n < 2 {
                continue;
            }
            let mut previous: Option<(RibbonVertex, RibbonVertex)> = None;
            for i in 0..n {
                let along = if i + 1 < n { points[i + 1] - points[i] } else { points[i] - points[i - 1] };
                let toward = eye - points[i];
                let Some(side) = along.cross(toward).try_normalize() else {
                    previous = None;
                    continue;
                };
                let t = i as f32 / (n - 1) as f32;
                let width = half_width * (1.0 - t);
                let alpha = (1.0 - t) * (1.0 - t) * opacity;
                let color = [c[0] * alpha, c[1] * alpha, c[2] * alpha, alpha];
                let left = RibbonVertex { position: (points[i] + side * width).into(), across: 1.0, color };
                let right = RibbonVertex { position: (points[i] - side * width).into(), across: -1.0, color };
                if let Some((pl, pr)) = previous {
                    self.ribbons.extend_from_slice(&[pl, pr, left, pr, right, left]);
                }
                previous = Some((left, right));
            }
        }
    }

    /// Glows on the wall and sparks where balls hit, fading over the scene's flash length.
    fn push_hits(&mut self, scene: &Scene, sim: &SpaceSimulation, events: &[bpe_sim::Event]) {
        let look = &scene.look;
        if look.flash_strength <= 0.0 || look.flash_seconds <= 0.0 {
            return;
        }
        let now = sim.time();
        let memory = HIT_GLOW_SECONDS * (look.flash_seconds as f64 / 0.25).max(0.2);
        let full_speed = scene.sound.full_volume_speed.max(1.0);
        let start = events.partition_point(|e| e.time <= now - memory);
        for event in events[start..].iter().rev() {
            if !matches!(event.kind, bpe_sim::EventKind::Wall | bpe_sim::EventKind::Ball(_)) {
                continue;
            }
            let age = ((now - event.time) / memory).max(0.0) as f32;
            let strength = (event.speed / full_speed).min(1.5) * (-3.0 * age).exp() * look.flash_strength;
            if strength < 0.004 {
                continue;
            }
            let color = sim.balls().get(event.ball as usize).map_or([1.0; 4], |b| b.color);
            let position = event.position.extend(event.depth);
            if self.wall_glows.len() < MAX_GLOW_HITS && event.kind == bpe_sim::EventKind::Wall {
                self.wall_glows.push(WallGlow { position, strength, color });
            }
            if self.glows.len() >= MAX_SPARKS {
                continue;
            }
            let k = strength * 0.3;
            let spark = [(color[0] + 1.0) * k, (color[1] + 1.0) * k, (color[2] + 1.0) * k, 1.0];
            self.glows.push(Glow { center: position.into(), size: look.flash_size, color: spark });
        }
    }

    /// Pieces of tunnel rings that broke recently, flying out and on.
    fn push_shards(&mut self, scene: &Scene, sim: &SpaceSimulation) {
        let Container3d::Tunnel { radius, rings, .. } = sim.space().container else { return };
        let pieces = scene.look.shatter_pieces.min(400);
        if pieces == 0 {
            return;
        }
        let now = sim.time();
        let radius = radius * sim.arena().scale;
        let gravity = scene.physics.gravity;
        for broken in sim.gate_breaks().iter().flatten() {
            let age = (now - broken.time) as f32;
            if !(0.0..SHATTER_SECONDS).contains(&age) {
                continue;
            }
            let gate = broken.gate;
            let fade = (1.0 - age / SHATTER_SECONDS).powi(2);
            let color = Vec4::from(ring_color(&self.space, gate.index, rings));
            let color = (color * (1.0 + 3.0 * (-6.0 * age).exp()) * fade).with_w(1.0);
            let span = std::f32::consts::TAU - 2.0 * gate.gap_half;
            for j in 0..pieces {
                let seed = (gate.index as u64) << 32 | j as u64;
                let u = gate.angle + gate.gap_half + span * (j as f32 + 0.5) / pieces as f32;
                let outward = Vec3::new(u.cos(), u.sin(), 0.0);
                let speed = 160.0 + 260.0 * bpe_sim::noise(seed * 3);
                let forward = 200.0 * (bpe_sim::noise(seed * 3 + 1) - 0.3);
                let start = outward * radius * (0.6 + 0.4 * bpe_sim::noise(seed * 3 + 2)) + Vec3::Z * gate.z;
                let moved = start
                    + (outward * -speed + Vec3::Z * forward) * age
                    + Vec3::new(gravity.x, gravity.y, 0.0) * (0.5 * age * age);
                self.glows.push(Glow { center: moved.into(), size: 10.0 + 8.0 * fade, color: color.into() });
            }
        }
    }

    /// Recent hits for shader effects, newest first, placed where they show on screen. Returns
    /// the pulse: 0 to 1, jumping up with hits and fading.
    pub fn effect_hits(
        &self,
        scene: &Scene,
        now: f64,
        events: &[bpe_sim::Event],
        out: &mut Vec<crate::RecentHit>,
    ) -> f32 {
        if scene.effects.is_empty() {
            return 0.0;
        }
        let video = Vec2::new(scene.video.width as f32, scene.video.height as f32);
        let full_speed = scene.sound.full_volume_speed.max(1.0);
        let start = events.partition_point(|e| e.time <= now - EFFECT_HIT_MEMORY);
        let mut energy = 0.0;
        for event in events[start..].iter().rev() {
            if !matches!(event.kind, bpe_sim::EventKind::Wall | bpe_sim::EventKind::Ball(_)) {
                continue;
            }
            let age = (now - event.time).max(0.0) as f32;
            let strength = (event.speed / full_speed).min(1.5);
            energy += strength * (-6.0 * age).exp();
            if out.len() < crate::MAX_EFFECT_HITS
                && let Some(position) = self.to_screen(event.position.extend(event.depth), video)
            {
                let color = self.balls.get(event.ball as usize).map_or([1.0; 4], |b| b.color);
                out.push(crate::RecentHit { position: position.into(), age, strength, color });
            }
        }
        1.0 - (-energy).exp()
    }

    /// Projects a 3D point to 2D video pixels, for effects that react to where hits happen.
    pub fn to_screen(&self, point: Vec3, video: Vec2) -> Option<Vec2> {
        self.camera.to_screen(point, video)
    }
}

/// The look of balls from spawner `group`, or None for the classic look.
fn style_of(scene: &Scene, group: u32) -> Option<&BallStyle> {
    scene.spawners.get(group as usize).and_then(|s| s.style())
}

/// The colour of tunnel ring `index` of `count`: the glass colour, turning slowly around the
/// colour wheel along the tunnel.
pub fn ring_color(space: &Space, index: u32, count: u32) -> [f32; 4] {
    let t = if count > 1 { index as f32 / (count - 1) as f32 } else { 0.0 };
    space.material.glass_color.shift_hue(t * 0.6).to_linear()
}

/// Where the camera is and what it looks at for the 3D settings at `time`.
pub fn camera_for(space: &Space, sim: &SpaceSimulation, time: f32, aspect: f32) -> CameraFrame {
    let camera = &space.camera;
    let (mut position, mut target) = (camera.position, camera.target);
    if let Container3d::Tunnel { .. } = space.container {
        let back = Vec3::Z * sim.arena().back;
        position += back;
        target += back;
    }
    match camera.mode {
        CameraMode::Fixed => {}
        CameraMode::Orbit { speed } => {
            let turn = glam::Quat::from_rotation_y(-(speed * time).to_radians());
            position = target + turn * (position - target);
        }
        CameraMode::Follow { ball, distance } => {
            if let Some(b) = sim.balls().get(ball as usize) {
                // Average the ball's recent positions so the camera glides instead of jolting.
                let mut sum = b.position;
                let mut count = 1.0;
                for p in sim.trail(ball as usize).take(FOLLOW_SMOOTHING) {
                    sum += p;
                    count += 1.0;
                }
                let followed = sum / count;
                let direction = (position - target).normalize_or(Vec3::NEG_Z);
                target = followed;
                position = followed + direction * distance.max(NEAR * 4.0);
            }
        }
    }
    CameraFrame::new(position, target, camera.fov, aspect)
}

/// The key light's view, fitted around the container (or the stretch of tunnel the balls are in)
/// so the shadow map's texels cover only what can cast or catch a shadow.
fn light_for(space: &Space, container: &ContainerFrame, gates: &[GateFrame]) -> LightFrame {
    let (center, reach) = match space.container {
        Container3d::Sphere { .. } => (Vec3::ZERO, container.size * 1.02),
        Container3d::Cube { .. } => (Vec3::ZERO, container.size * 3f32.sqrt() * 1.02),
        Container3d::Tunnel { spacing, .. } => {
            let far = gates.first().map_or(container.back + spacing * 2.0, |g| g.z);
            let half = ((far - container.back) * 0.5).max(container.size);
            let center = Vec3::new(0.0, 0.0, container.back + half);
            (center, (half * half + container.size * container.size).sqrt() * 1.02)
        }
    };
    let reach = reach.max(1.0);
    let direction = space.key_direction();
    let eye = center + direction * reach * 2.0;
    let up = if direction.cross(Vec3::NEG_Y).length_squared() < 1e-4 { Vec3::Z } else { Vec3::NEG_Y };
    let view = view::look_at_mat4(eye, center, up);
    let (near, far) = (reach * 0.5, reach * 3.5);
    let projection = directx::orthographic(-reach, reach, -reach, reach, near, far);
    LightFrame {
        view_projection: projection * view,
        depth_scale: 1.0 / (far - near),
        texel: 2.0 * reach / renderer::SHADOW_SIZE as f32,
    }
}

/// Linear-light colour with its alpha as a [`Vec4`].
pub(crate) fn linear(color: Color) -> Vec4 {
    Vec4::from(color.to_linear())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_scene::View;

    fn space_scene(container: Container3d) -> Scene {
        Scene { view: View::Space(Box::new(Space { container, ..Space::default() })), ..Scene::default() }
    }

    fn frame_at(scene: &Scene, time: f64) -> SpaceFrame {
        let mut sim = SpaceSimulation::new(scene);
        let mut events = Vec::new();
        sim.advance_to(time, &mut events);
        let mut frame = SpaceFrame::default();
        frame.fill_from(scene, &sim, &events);
        frame
    }

    #[test]
    fn camera_sees_the_middle_of_the_container_in_the_middle_of_the_picture() {
        let camera = CameraFrame::new(Vec3::new(0.0, -200.0, -1500.0), Vec3::ZERO, 40.0, 9.0 / 16.0);
        let video = Vec2::new(1080.0, 1920.0);
        let middle = camera.to_screen(Vec3::ZERO, video).unwrap();
        assert!(middle.length() < 0.5, "{middle}");
        // Right in the world is right on screen, and down is down.
        assert!(camera.to_screen(Vec3::new(100.0, 0.0, 0.0), video).unwrap().x > 10.0);
        assert!(camera.to_screen(Vec3::new(0.0, 100.0, 0.0), video).unwrap().y > 10.0);
        assert!(camera.to_screen(Vec3::new(0.0, 0.0, -3000.0), video).is_none());
    }

    #[test]
    fn flat_scenes_make_an_empty_frame() {
        let scene = Scene::default();
        let sim = SpaceSimulation::new(&space_scene(Container3d::default()));
        let mut frame = SpaceFrame::default();
        frame.fill_from(&scene, &sim, &[]);
        assert!(!frame.active && frame.balls.is_empty());
    }

    #[test]
    fn a_frame_has_balls_trails_and_glows() {
        let scene = space_scene(Container3d::Sphere { radius: 420.0 });
        let frame = frame_at(&scene, 2.0);
        assert!(frame.active);
        assert_eq!(frame.balls.len(), 784);
        assert!(!frame.ribbons.is_empty());
        assert!(!frame.wall_glows.is_empty() && !frame.glows.is_empty());
        assert_eq!(frame.ribbons.len() % 6, 0);
        assert!(frame.ribbons.iter().all(|v| v.position.iter().all(|c| c.is_finite())));
    }

    #[test]
    fn tunnel_frames_have_rings_and_a_camera_that_moves_on() {
        let scene = space_scene(Container3d::tunnel(320.0));
        let early = frame_at(&scene, 0.5);
        let later = frame_at(&scene, 6.0);
        assert!(!early.gates.is_empty());
        assert!(early.gates.windows(2).all(|w| w[0].z < w[1].z));
        assert!(later.camera.position.z > early.camera.position.z + 100.0);
    }

    #[test]
    fn orbiting_and_following_cameras_move() {
        let mut scene = space_scene(Container3d::Cube { size: 600.0 });
        scene.space_mut().unwrap().camera.mode = CameraMode::Orbit { speed: 30.0 };
        let a = frame_at(&scene, 0.0).camera.position;
        let b = frame_at(&scene, 3.0).camera.position;
        assert!((a - b).length() > 100.0);
        assert!((a.length() - b.length()).abs() < 1.0);
        scene.space_mut().unwrap().camera.mode = CameraMode::Follow { ball: 3, distance: 500.0 };
        let frame = frame_at(&scene, 2.0);
        assert!(((frame.camera.position - frame.camera.target).length() - 500.0).abs() < 1.0);
        // Following a ball that doesn't exist leaves the camera where it was put.
        scene.space_mut().unwrap().camera.mode = CameraMode::Follow { ball: 99_999, distance: 500.0 };
        let frame = frame_at(&scene, 2.0);
        assert_eq!(frame.camera.position, scene.space().unwrap().camera.position);
    }

    #[test]
    fn the_light_covers_the_whole_container() {
        let scene = space_scene(Container3d::Cube { size: 600.0 });
        let frame = frame_at(&scene, 0.1);
        for corner in [Vec3::splat(300.0), Vec3::splat(-300.0), Vec3::new(300.0, -300.0, 300.0)] {
            let p = frame.light.view_projection.project_point3(corner);
            assert!(p.x.abs() <= 1.0 && p.y.abs() <= 1.0 && (0.0..=1.0).contains(&p.z), "{p}");
        }
    }
}

/// Random scenes in 3D: every preset rolled with every group at full strength, put in each 3D
/// container with a rolled camera and lights, then simulated and turned into frames.
#[cfg(test)]
mod fuzz {
    use super::*;
    use bpe_scene::{Dice, Group, View};
    use bpe_sim::Playback;

    const GROUPS: [Group; 11] = [
        Group::Shape,
        Group::Movement,
        Group::World,
        Group::Bounces,
        Group::NewBalls,
        Group::Background,
        Group::Trails,
        Group::Glow,
        Group::Strings,
        Group::Chords,
        Group::Notes,
    ];

    pub(crate) fn rolled(seed: u64) -> Scene {
        let presets = Scene::presets();
        let base = &presets[seed as usize % presets.len()].scene;
        let groups: Vec<Group> = (0..base.spawners.len()).map(Group::Balls).chain(GROUPS).collect();
        let mut scene = base.variation(&groups, seed, 1.0);
        let mut dice = Dice::mixed(seed, 0x3d);
        let container = match seed % 3 {
            0 => Container3d::Sphere { radius: dice.range(150.0, 900.0) },
            1 => Container3d::Cube { size: dice.range(200.0, 1500.0) },
            _ => Container3d::tunnel(dice.range(150.0, 700.0)),
        };
        let mut space = scene.space().cloned().unwrap_or_default();
        space.container = container;
        space.scatter = dice.unit();
        space.gravity_depth = dice.range(-1500.0, 1500.0);
        space.roll_camera(&mut dice, 9.0 / 16.0);
        space.roll_lights(&mut dice);
        scene.view = View::Space(Box::new(space));
        scene
    }

    #[test]
    fn five_hundred_rolled_scenes_run_in_3d() {
        let seeds: u64 = std::env::var("BPE_FUZZ_SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(500);
        for seed in 0..seeds {
            let scene = rolled(seed);
            let mut playback = Playback::new(&scene);
            let mut frame = crate::FrameData::default();
            for time in [0.0, 0.7, 1.5] {
                playback.seek(time);
                frame.fill(&scene, &playback, 1.0);
                let sim = playback.space().expect("3D scene");
                let reach = scene.space().unwrap().container.size() * sim.arena().scale.max(1.0) * 2.0 + 100.0;
                for ball in sim.balls() {
                    assert!(ball.position.is_finite() && ball.velocity.is_finite(), "seed {seed}: {ball:?}");
                    if !matches!(scene.space().unwrap().container, Container3d::Tunnel { .. }) {
                        assert!(
                            ball.position.length() <= reach,
                            "seed {seed} at {time}: ball escaped to {} (container {:?}, arena {:?}, radius {}, motion {:?})",
                            ball.position,
                            scene.space().unwrap().container,
                            sim.arena(),
                            ball.radius,
                            scene.arena_motion
                        );
                    }
                }
                assert!(frame.space.active && frame.space.camera.view_projection.is_finite(), "seed {seed}");
                assert!(frame.space.balls.len() == sim.balls().len(), "seed {seed}");
            }
            assert!(playback.events().iter().all(|e| e.position.is_finite() && e.depth.is_finite()), "seed {seed}");
            // Going back gives the same events as the first time.
            let count = playback.events_between(0.0, 1.0).len();
            playback.seek(0.3);
            playback.seek(1.5);
            assert_eq!(playback.events_between(0.0, 1.0).len(), count, "seed {seed}");
        }
    }
}

#[cfg(test)]
mod gpu_tests {
    use super::*;
    use crate::{FrameData, Renderer, headless_device};
    use bpe_scene::View;
    use bpe_sim::Playback;

    /// Small pictures of each container, averaged over blocks, kept next to this file. Set
    /// BPE_BLESS to write them again after a deliberate change to the look.
    const REFERENCE: &str = "src/space/reference.txt";
    const GRID: (usize, usize) = (9, 16);

    /// A GPU, or a panic: without one these tests would pass without checking anything. Set
    /// BPE_ALLOW_NO_GPU to skip them on a machine that has none.
    fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
        match headless_device() {
            Ok(device) => Some(device),
            Err(_) if std::env::var_os("BPE_ALLOW_NO_GPU").is_some() => None,
            Err(error) => panic!("no GPU adapter ({error}); install mesa-vulkan-drivers or set BPE_ALLOW_NO_GPU"),
        }
    }

    /// Renders a 3D scene at `time` through a playback, as the app does, and returns the
    /// picture. Set BPE_DUMP to a folder to also save it there as raw RGBA, for looking at.
    pub(crate) fn render_space(
        device: &(wgpu::Device, wgpu::Queue),
        scene: &Scene,
        time: f64,
        (width, height): (u32, u32),
        name: &str,
    ) -> Vec<u8> {
        let (device, queue) = device;
        let mut renderer = Renderer::new(device, width, height);
        let mut playback = Playback::new(scene);
        playback.seek(time);
        let frame = FrameData::build(scene, &playback, renderer.pixel_size(scene));
        renderer.render(device, queue, scene, time, &frame);
        let pixels = renderer.read_output(device, queue).unwrap();
        if let Ok(dir) = std::env::var("BPE_DUMP") {
            std::fs::write(format!("{dir}/{name}-{width}x{height}.rgba"), &pixels).unwrap();
        }
        pixels
    }

    fn space_scene(container: Container3d) -> Scene {
        let camera = bpe_scene::Camera::fit(&container, 9.0 / 16.0);
        Scene { view: View::Space(Box::new(Space { container, camera, ..Space::default() })), ..Scene::default() }
    }

    /// Average brightness of each block of the picture, 0 to 255.
    fn blocks(pixels: &[u8], width: u32, height: u32) -> Vec<f32> {
        let (columns, rows) = GRID;
        let mut sums = vec![0.0f32; columns * rows];
        for (i, p) in pixels.chunks(4).enumerate() {
            let (x, y) = (i as u32 % width, i as u32 / width);
            let cell = (y as usize * rows / height as usize) * columns + x as usize * columns / width as usize;
            sums[cell] += (p[0] as f32 + p[1] as f32 + p[2] as f32) / 3.0;
        }
        let per = (width * height) as f32 / (columns * rows) as f32;
        sums.iter().map(|sum| sum / per).collect()
    }

    #[test]
    fn renders_each_container_like_the_reference() {
        let Some(device) = device() else { return };
        let scale: u32 = std::env::var("BPE_DUMP_SCALE").ok().and_then(|s| s.parse().ok()).unwrap_or(1);
        let (width, height) = (270 * scale, 480 * scale);
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(REFERENCE);
        let reference = std::fs::read_to_string(&path).unwrap_or_default();
        let mut written = String::new();
        let mut failures = Vec::new();
        for container in
            [Container3d::Sphere { radius: 380.0 }, Container3d::Cube { size: 600.0 }, Container3d::tunnel(330.0)]
        {
            let name = container.name();
            let pixels = render_space(&device, &space_scene(container), 3.0, (width, height), name);
            let bright = pixels.chunks(4).filter(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 200).count();
            assert!(bright > 200 * (scale * scale) as usize, "{name}: only {bright} bright pixels");
            let now = blocks(&pixels, width, height);
            written += &format!("{name}\t{}\n", now.iter().map(|v| format!("{v:.0}")).collect::<Vec<_>>().join(" "));
            if scale != 1 {
                continue; // The references are at the normal size.
            }
            let Some(line) = reference.lines().find(|l| l.split('\t').next() == Some(name)) else {
                failures.push(format!("{name}: no reference picture"));
                continue;
            };
            let was: Vec<f32> = line.split('\t').nth(1).unwrap().split(' ').map(|v| v.parse().unwrap()).collect();
            let worst = was.iter().zip(&now).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
            let mean = was.iter().zip(&now).map(|(a, b)| (a - b).abs()).sum::<f32>() / was.len() as f32;
            if worst > 24.0 || mean > 4.0 {
                failures.push(format!("{name}: differs from the reference (worst block {worst:.0}, mean {mean:.1})"));
            }
        }
        if std::env::var_os("BPE_BLESS").is_some() && scale == 1 {
            std::fs::write(&path, written).unwrap();
        } else {
            assert!(failures.is_empty(), "{}", failures.join("\n"));
        }
    }

    #[test]
    fn the_3d_presets_render() {
        let Some(device) = device() else { return };
        let scale: u32 = std::env::var("BPE_DUMP_SCALE").ok().and_then(|s| s.parse().ok()).unwrap_or(1);
        for preset in Scene::presets().into_iter().filter(|p| p.scene.space().is_some()) {
            let pixels = render_space(&device, &preset.scene, 6.0, (270 * scale, 480 * scale), preset.name);
            let lit = pixels.chunks(4).filter(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 200).count();
            assert!(lit > 500 * (scale * scale) as usize, "{}: only {lit} bright pixels", preset.name);
        }
    }

    /// A flat scene never touches the 3D renderer, and a 3D scene draws nothing flat.
    #[test]
    fn flat_and_space_pictures_differ() {
        let Some(device) = device() else { return };
        let flat = render_space(&device, &Scene::default(), 2.0, (90, 160), "flat");
        let space = render_space(&device, &space_scene(Container3d::default()), 2.0, (90, 160), "space");
        let differing = flat.chunks(4).zip(space.chunks(4)).filter(|(a, b)| a != b).count();
        assert!(differing > 90 * 160 / 4, "only {differing} pixels differ");
    }
}

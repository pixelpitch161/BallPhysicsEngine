//! Hit particles: sparks, smoke, confetti, stars, shards and rings that burst out of events.
//!
//! The CPU only lists the recent events each emitter answers ("bursts"). The vertex shader works
//! out every particle from its burst and age, so nothing is kept between frames, scrubbing is
//! exact and the cost is one draw call per blend mode.

use crate::frame::FrameData;
use crate::gpu::GpuLayers;
use crate::gpu::util::{self, StorageBuffer};
use bpe_scene::{
    Blend, Color, Container, Emitter, EventMatch, ObstaclePart, ParticleColor, ParticlePreset, Scene, Vec2,
    particle_hash,
};
use bpe_sim::{Event, EventKind, Playback};
use bytemuck::{Pod, Zeroable};

/// Most particles on screen at once. Beyond this the oldest bursts are left out.
pub const MAX_LIVE_PARTICLES: u32 = 200_000;
/// Most gradient colours an emitter passes to the shader.
const MAX_STOPS: usize = 6;

/// One event's burst. Matches `struct Burst` in particles.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct BurstGpu {
    pub place: [f32; 4],
    pub timing: [f32; 4],
    pub color: [f32; 4],
    pub ids: [u32; 4],
}

impl BurstGpu {
    pub fn count(&self) -> u32 {
        self.timing[3] as u32
    }
}

/// Matches `struct Emitter` in particles.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct EmitterGpu {
    kind: [u32; 4],
    motion: [f32; 4],
    more: [f32; 4],
    gradient: [[f32; 4]; MAX_STOPS],
}

impl EmitterGpu {
    fn new(e: &Emitter, index: usize) -> Self {
        let mut gradient = [[0.0; 4]; MAX_STOPS];
        let mut stops = 0;
        if let ParticleColor::Gradient(colors) = &e.color {
            for (slot, c) in gradient.iter_mut().zip(colors) {
                *slot = c.to_linear();
                stops += 1;
            }
        }
        let seed = (e.seed ^ (e.seed >> 32)) as u32 ^ (index as u32).wrapping_mul(0x632b_e5ab);
        Self {
            kind: [e.preset.shader_index(), (stops > 0) as u32, stops as u32, seed],
            motion: [e.speed.max(0.0), e.spread.clamp(0.0, 360.0).to_radians(), e.size.max(0.0), e.life.max(0.0)],
            more: [e.gravity, e.drag.max(0.0), e.follow_impact as u32 as f32, e.bounce as u32 as f32],
            gradient,
        }
    }
}

/// What the particles need from one frame, built on the CPU.
#[derive(Clone, Debug, Default)]
pub struct ParticleFrame {
    /// Glowing bursts first, then covering ones, each oldest first.
    pub bursts: Vec<BurstGpu>,
    pub emitters: Vec<EmitterGpu>,
    /// Particles in glowing bursts, then in covering ones.
    pub additive: u32,
    pub covering: u32,
}

impl ParticleFrame {
    pub fn total(&self) -> u32 {
        self.additive + self.covering
    }

    /// Lists the bursts for this frame. `_events` is the frame's recent events; particles can
    /// live longer than those reach, so this reads the playback's whole log instead.
    pub fn fill(&mut self, scene: &Scene, playback: &Playback, _events: &[Event]) {
        self.bursts.clear();
        self.emitters.clear();
        self.additive = 0;
        self.covering = 0;
        // Each emitter keeps its place in the scene, which picks its random pattern, so turning
        // one off leaves the others as they were.
        let mut emitters: Vec<(usize, Emitter)> =
            scene.particles.iter().enumerate().filter(|(_, e)| e.is_active()).map(|(i, e)| (i, e.clone())).collect();
        // A rule's "Burst particles" with no emitter set up for it uses its preset as it comes.
        let longest = emitters.iter().map(|(_, e)| e.max_age()).fold(0.0, f32::max).max(4.0);
        for event in playback.recent_events(longest as f64) {
            if let EventKind::Action { rule, action } = event.kind
                && let Some(name) = burst_name(scene, rule, action)
                && !emitters.iter().any(|(_, e)| {
                    e.on.kind == EventMatch::Action
                        && (e.on.action.is_empty() || e.on.action.eq_ignore_ascii_case(&name))
                })
                && let Some(preset) = ParticlePreset::ALL.into_iter().find(|p| p.name().eq_ignore_ascii_case(&name))
            {
                let mut emitter = preset.emitter(EventMatch::Action);
                emitter.on.action = name;
                emitters.push((scene.particles.len() + emitters.len(), emitter));
            }
        }
        if emitters.is_empty() {
            return;
        }
        let sim = playback.simulation();
        let now = sim.time();
        let full_speed = scene.sound.full_volume_speed.max(1.0);
        let all = playback.events();
        let parts = scene.obstacle_parts();
        let mut lists: [Vec<BurstGpu>; 2] = [Vec::new(), Vec::new()];
        let mut budget = MAX_LIVE_PARTICLES;
        for (index, emitter) in &emitters {
            self.emitters.push(EmitterGpu::new(emitter, *index));
            let emitter_slot = self.emitters.len() as u32 - 1;
            let recent = playback.recent_events(emitter.max_age() as f64);
            let first_id = all.len() - recent.len();
            let list = &mut lists[(emitter.blend == Blend::Alpha) as usize];
            let start = list.len();
            // Newest first, so a full budget drops the oldest bursts.
            for (offset, event) in recent.iter().enumerate().rev() {
                if budget == 0 {
                    break;
                }
                if !matches(scene, emitter, event) {
                    continue;
                }
                let strength = if event.speed > 0.0 { (event.speed / full_speed).min(1.5) } else { 1.0 };
                let count = emitter.burst_count(strength).min(budget);
                budget -= count;
                let normal = event_normal(scene, &parts, event, (first_id + offset) as u32);
                let color = burst_color(scene, playback, emitter, event);
                let place = event_place(scene, event);
                let id = (first_id + offset) as u32;
                list.push(BurstGpu {
                    place: [place.x, place.y, normal.x, normal.y],
                    timing: [(now - event.time).max(0.0) as f32, strength, 0.0, count as f32],
                    color,
                    ids: [emitter_slot, id, 0, 0],
                });
            }
            list[start..].reverse();
        }
        let [add, cover] = lists;
        let mut first = 0u32;
        for (group, list) in [add, cover].into_iter().enumerate() {
            let before = first;
            for mut burst in list {
                burst.timing[2] = first as f32;
                first += burst.count();
                self.bursts.push(burst);
            }
            if group == 0 {
                self.additive = first - before;
            } else {
                self.covering = first - before;
            }
        }
    }
}

/// The particle preset a rule's action bursts, if that action is a "Burst particles" block.
fn burst_name(scene: &Scene, rule: u16, action: u16) -> Option<String> {
    let node = scene.logic.get(rule as usize)?.nodes.iter().find(|n| n.id == action as u32)?;
    let kind = node.kind.to_ascii_lowercase();
    if !(kind.contains("burst") || kind.contains("particle")) {
        return None;
    }
    let name = ["preset", "particles", "name"].iter().find_map(|key| match node.params.get(*key) {
        Some(bpe_scene::logic::Param::Text(text) | bpe_scene::logic::Param::Choice(text)) => Some(text.clone()),
        _ => None,
    });
    Some(name.unwrap_or_else(|| ParticlePreset::Sparks.name().to_string()))
}

fn matches(scene: &Scene, emitter: &Emitter, event: &Event) -> bool {
    if emitter.on.ball.is_some_and(|ball| ball != event.ball) {
        return false;
    }
    match emitter.on.kind {
        EventMatch::AnyHit => event.kind.is_hit(),
        EventMatch::Wall => event.kind == EventKind::Wall,
        EventMatch::Obstacle => matches!(event.kind, EventKind::Obstacle(_)),
        EventMatch::Escape => matches!(event.kind, EventKind::Escape(_)),
        EventMatch::Ball => matches!(event.kind, EventKind::Ball(_)),
        EventMatch::Soft => matches!(event.kind, EventKind::Soft { .. }),
        EventMatch::Spawn => event.kind == EventKind::Spawn,
        EventMatch::Beat => matches!(event.kind, EventKind::Beat { .. }),
        EventMatch::Action => match event.kind {
            EventKind::Action { rule, action } => burst_name(scene, rule, action)
                .is_some_and(|name| emitter.on.action.is_empty() || emitter.on.action.eq_ignore_ascii_case(&name)),
            _ => false,
        },
    }
}

/// Where a burst starts. Events without a place of their own, such as beats, burst from the
/// middle of the arena.
fn event_place(scene: &Scene, event: &Event) -> Vec2 {
    if event.ball_index().is_none() && event.position == Vec2::ZERO { scene.container.center() } else { event.position }
}

/// Which way particles fly from an event: away from the surface that was hit. It depends only
/// on the event and the scene as built, never on where things are now, so a burst keeps its
/// direction for its whole life.
fn event_normal(scene: &Scene, parts: &[ObstaclePart], event: &Event, id: u32) -> Vec2 {
    let center = scene.container.center();
    let away = match event.kind {
        EventKind::Wall => center - event.position,
        EventKind::Escape(_) => event.position - center,
        EventKind::Obstacle(i) => match parts.get(i as usize) {
            Some(ObstaclePart::Disc { center, .. }) => event.position - *center,
            Some(ObstaclePart::Segment { from, to, .. }) => {
                let side = (*to - *from).perp();
                if side.dot(event.position - *from) >= 0.0 { side } else { -side }
            }
            None => Vec2::NEG_Y,
        },
        // Bumps between balls and new balls have no surface: pick a steady direction per event.
        EventKind::Ball(_) | EventKind::Spawn => Vec2::from_angle(particle_hash(id, 7) * std::f32::consts::TAU),
        _ => Vec2::NEG_Y,
    };
    away.try_normalize().unwrap_or(Vec2::NEG_Y)
}

fn burst_color(scene: &Scene, playback: &Playback, emitter: &Emitter, hit: &Event) -> [f32; 4] {
    let ball = || hit.ball_index().and_then(|b| playback.simulation().balls().get(b)).map_or([1.0; 4], |b| b.color);
    match &emitter.color {
        ParticleColor::FromBall => ball(),
        ParticleColor::Fixed(c) => c.to_linear(),
        ParticleColor::Gradient(colors) if colors.is_empty() => ball(),
        ParticleColor::Gradient(_) => [1.0; 4],
        ParticleColor::Wall => match hit.kind {
            EventKind::Obstacle(i) => match playback.simulation().obstacles().get(i as usize) {
                Some(ObstaclePart::Disc { color, .. } | ObstaclePart::Segment { color, .. }) => color.to_linear(),
                None => ball(),
            },
            EventKind::Escape(ring) => match &scene.container {
                Container::Rings { gradient, count, .. } => {
                    let t = if *count > 1 { ring as f32 / (*count - 1) as f32 } else { 0.0 };
                    Color::sample_gradient(gradient, t)
                }
                container => container.color().to_linear(),
            },
            EventKind::Wall => match &scene.container {
                Container::Rings { gradient, .. } => Color::sample_gradient(gradient, 0.0),
                container => container.color().to_linear(),
            },
            _ => ball(),
        },
    }
}

/// Draws the particles of each frame.
pub struct Particles {
    layout: wgpu::BindGroupLayout,
    additive: wgpu::RenderPipeline,
    covering: wgpu::RenderPipeline,
    bursts: StorageBuffer,
    emitters: StorageBuffer,
    group: Option<wgpu::BindGroup>,
    additive_count: u32,
    covering_count: u32,
    /// Which wall field the bind group was made with.
    sdf_generation: u64,
}

impl Particles {
    pub fn new(device: &wgpu::Device, hdr_format: wgpu::TextureFormat) -> Self {
        use wgpu::ShaderStages as S;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("particles layout"),
            entries: &[
                util::storage_entry(0, S::VERTEX, true),
                util::storage_entry(1, S::VERTEX, true),
                util::float_texture_entry(2, S::VERTEX, false),
                util::uniform_entry(3, S::VERTEX, false),
            ],
        });
        let globals = util::globals_layout(device);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("particles"),
            bind_group_layouts: &[Some(&globals), Some(&layout)],
            immediate_size: 0,
        });
        let module =
            util::shader(device, "particles", &[util::COMMON_WGSL, include_str!("shaders/gpu/particles.wgsl")]);
        let make = |label, fs, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_particle"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: hdr_format,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let add = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::OVER,
        };
        Self {
            additive: make("particles glow", "fs_add", add),
            covering: make("particles cover", "fs_alpha", wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            layout,
            bursts: StorageBuffer::new(device, "particle bursts", wgpu::BufferUsages::empty()),
            emitters: StorageBuffer::new(device, "particle emitters", wgpu::BufferUsages::empty()),
            group: None,
            additive_count: 0,
            covering_count: 0,
            sdf_generation: 0,
        }
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _scene: &Scene,
        frame: &FrameData,
        gpu: &GpuLayers,
    ) {
        let particles = &frame.particles;
        self.additive_count = particles.additive;
        self.covering_count = particles.covering;
        if particles.total() == 0 {
            return;
        }
        let mut grew = self.bursts.write(device, queue, 0, bytemuck::cast_slice(&particles.bursts));
        grew |= self.emitters.write(device, queue, 0, bytemuck::cast_slice(&particles.emitters));
        let (sdf_view, sdf_info) = gpu.sdf_binding();
        if grew || self.group.is_none() || gpu.sdf_generation() != self.sdf_generation {
            self.sdf_generation = gpu.sdf_generation();
            self.group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("particles"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.bursts.buffer().as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: self.emitters.buffer().as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(sdf_view) },
                    wgpu::BindGroupEntry { binding: 3, resource: sdf_info.as_entire_binding() },
                ],
            }));
        }
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, globals: &wgpu::BindGroup) {
        let Some(group) = &self.group else { return };
        if self.additive_count + self.covering_count == 0 {
            return;
        }
        pass.set_bind_group(0, globals, &[]);
        pass.set_bind_group(1, group, &[]);
        if self.additive_count > 0 {
            pass.set_pipeline(&self.additive);
            pass.draw(0..4, 0..self.additive_count);
        }
        if self.covering_count > 0 {
            pass.set_pipeline(&self.covering);
            pass.draw(0..4, self.additive_count..self.additive_count + self.covering_count);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_scene::ParticlePreset;

    fn frame_at(scene: &Scene, time: f64) -> ParticleFrame {
        let mut playback = Playback::new(scene);
        playback.seek(time);
        let mut frame = ParticleFrame::default();
        frame.fill(scene, &playback, &[]);
        frame
    }

    #[test]
    fn no_emitters_means_no_work() {
        let frame = frame_at(&Scene::default(), 3.0);
        assert_eq!(frame.total(), 0);
        assert!(frame.bursts.is_empty() && frame.emitters.is_empty());
        let mut scene = Scene::default();
        scene.particles.push(Emitter { enabled: false, ..Emitter::default() });
        assert_eq!(frame_at(&scene, 3.0).total(), 0);
    }

    #[test]
    fn bursts_follow_recent_hits_and_add_up() {
        let mut scene = Scene::default();
        scene.particles.push(ParticlePreset::Sparks.emitter(EventMatch::Wall));
        scene.particles.push(ParticlePreset::Confetti.emitter(EventMatch::Wall));
        let frame = frame_at(&scene, 5.0);
        assert!(frame.additive > 0 && frame.covering > 0);
        let mut first = 0;
        for burst in &frame.bursts {
            assert_eq!(burst.timing[2] as u32, first);
            assert!(burst.timing[0] >= 0.0 && burst.timing[0] <= scene.particles[burst.ids[0] as usize].life);
            first += burst.count();
        }
        assert_eq!(first, frame.total());
        // Glowing bursts come before covering ones.
        let split = frame.bursts.iter().position(|b| b.ids[0] == 1).unwrap();
        assert!(frame.bursts[..split].iter().all(|b| b.ids[0] == 0));
    }

    #[test]
    fn scrubbing_back_gives_the_same_bursts() {
        let mut scene = Scene::escape_rings();
        scene.particles.push(ParticlePreset::Shards.emitter(EventMatch::Escape));
        scene.particles.push(ParticlePreset::Stars.emitter(EventMatch::AnyHit));
        let straight = frame_at(&scene, 8.0);
        let mut playback = Playback::new(&scene);
        playback.seek(12.0);
        playback.seek(8.0);
        let mut scrubbed = ParticleFrame::default();
        scrubbed.fill(&scene, &playback, &[]);
        assert_eq!(straight.bursts, scrubbed.bursts);
    }

    #[test]
    fn the_budget_caps_live_particles() {
        let mut scene = Scene::default();
        scene.particles.push(Emitter { count: 400, life: 3.0, follow_impact: false, ..Emitter::default() });
        let frame = frame_at(&scene, 6.0);
        assert!(frame.total() <= MAX_LIVE_PARTICLES);
        assert_eq!(frame.total(), MAX_LIVE_PARTICLES, "784 balls hitting for 3 s should fill the budget");
    }

    #[test]
    fn filters_pick_the_right_events() {
        let mut scene = Scene::plinko();
        scene.particles.push(Emitter {
            on: bpe_scene::EventFilter { kind: EventMatch::Obstacle, ..Default::default() },
            ..Emitter::default()
        });
        let pegs = frame_at(&scene, 6.0);
        assert!(!pegs.bursts.is_empty());
        scene.particles[0].on.kind = EventMatch::Escape;
        assert!(frame_at(&scene, 6.0).bursts.is_empty());
        scene.particles[0].on =
            bpe_scene::EventFilter { kind: EventMatch::AnyHit, ball: Some(9999), ..Default::default() };
        assert!(frame_at(&scene, 6.0).bursts.is_empty());
    }

    #[test]
    fn normals_point_away_from_what_was_hit() {
        let scene = Scene::default();
        let mut playback = Playback::new(&scene);
        playback.seek(4.0);
        for hit in playback.recent_hits(1.0) {
            let n = event_normal(&scene, &scene.obstacle_parts(), hit, 0);
            assert!((n.length() - 1.0).abs() < 1e-4);
            // The ring is centred on the frame, so inward points back towards the middle.
            assert!(n.dot(-hit.position) > 0.0);
        }
    }
}

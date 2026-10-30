//! Particle bursts on the GPU: sparks, smoke, confetti, stars, shards and rings.
//!
//! The CPU only lists the bursts that are still going (`pick`). The vertex shader works every
//! particle out from its burst and age, so nothing is kept between frames, scrubbing is exact
//! and the cost is one draw call per blend mode.

use bytemuck::{Pod, Zeroable};

use crate::HDR_FORMAT;
use crate::frame::Burst;
use crate::gradients::PaintGpu;

/// Most bursts drawn in one frame. Beyond this the oldest are left out.
pub const MAX_BURSTS: usize = 400;
/// Most particles on screen at once. Beyond this the oldest bursts are left out.
pub const MAX_LIVE_PARTICLES: u32 = 200_000;

/// One burst as the shader reads it. Matches `struct Burst` in particles.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct BurstGpu {
    /// Origin, then the way the particles fly.
    pub place: [f32; 4],
    /// Age in seconds, first particle index, particle count, and in the first burst only the
    /// number of bursts (the buffer bound to the shader may be longer than the list).
    pub timing: [f32; 4],
    pub color: [f32; 4],
    /// Speed, spread in radians, size, life.
    pub motion: [f32; 4],
    /// Gravity, drag, and how much of a colour painted here shows (see `Burst::amount`).
    pub more: [f32; 4],
    /// Shape, seed, follow (0 the colour above, 1 the paint over each particle's life, 2 one of
    /// the paint's stops at random).
    pub ids: [u32; 4],
    /// The paint, as `PaintGpu`'s fields in order.
    pub paint_a: [f32; 4],
    pub paint_b: [f32; 4],
}

/// The bursts of one frame in draw order: glowing ones first, then covering ones, each oldest
/// first, with their particles numbered in that order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Picked {
    pub bursts: Vec<BurstGpu>,
    /// Particles in glowing bursts.
    pub additive: u32,
    /// Particles in covering bursts.
    pub covering: u32,
}

impl Picked {
    pub fn total(&self) -> u32 {
        self.additive + self.covering
    }
}

/// The bursts still going at `time`, newest kept when there are more than fit.
pub fn pick(bursts: &[Burst], time: f32) -> Picked {
    let mut live: Vec<&Burst> =
        bursts.iter().filter(|b| b.count > 0 && b.life > 0.0 && time >= b.time && time - b.time < b.life).collect();
    // Newest first, so a full budget drops the oldest.
    live.sort_by(|a, b| b.time.total_cmp(&a.time));
    let mut budget = MAX_LIVE_PARTICLES;
    let mut lists: [Vec<(&Burst, u32)>; 2] = [Vec::new(), Vec::new()];
    for b in live.into_iter().take(MAX_BURSTS) {
        let count = b.count.min(budget);
        if count == 0 {
            break;
        }
        budget -= count;
        lists[(!b.additive) as usize].push((b, count));
    }
    let mut picked = Picked::default();
    let mut first = 0u32;
    for (group, list) in lists.into_iter().enumerate() {
        let before = first;
        for (b, count) in list.into_iter().rev() {
            // A paint with no row has nothing to paint with: the burst keeps its own colour.
            let (p, follow) =
                if b.follow == 0 || b.paint.row < 0.0 { (PaintGpu::default(), 0) } else { (b.paint, b.follow.min(2)) };
            picked.bursts.push(BurstGpu {
                place: [b.origin[0], b.origin[1], b.normal[0], b.normal[1]],
                timing: [time - b.time, first as f32, count as f32, 0.0],
                color: b.color,
                motion: [b.speed.max(0.0), b.spread.clamp(0.0, 360.0).to_radians(), b.size.max(0.0), b.life],
                more: [b.gravity, b.drag.max(0.0), b.amount.clamp(0.0, 1.0), 0.0],
                ids: [b.shape.min(5), b.seed, follow, 0],
                paint_a: [p.row, p.t0, p.pattern, p.steps],
                paint_b: [p.angle, p.scale, p.scroll, p.repeat],
            });
            first += count;
        }
        if group == 0 {
            picked.additive = first - before;
        } else {
            picked.covering = first - before;
        }
    }
    let total = picked.bursts.len() as f32;
    if let Some(head) = picked.bursts.first_mut() {
        head.timing[3] = total;
    }
    picked
}

/// Draws the particles of each frame.
pub struct Particles {
    layout: wgpu::BindGroupLayout,
    additive: wgpu::RenderPipeline,
    covering: wgpu::RenderPipeline,
    buffer: Option<wgpu::Buffer>,
    group: Option<wgpu::BindGroup>,
    picked: Picked,
}

impl Particles {
    pub fn new(
        device: &wgpu::Device,
        globals_layout: &wgpu::BindGroupLayout,
        paint_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("particles layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("particles"),
            bind_group_layouts: &[Some(globals_layout), Some(&layout), Some(paint_layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("particles"),
            source: wgpu::ShaderSource::Wgsl(
                [crate::gradients::paint_wgsl(2).as_str(), include_str!("shaders/particles.wgsl")].concat().into(),
            ),
        });
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
                        format: HDR_FORMAT,
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
            buffer: None,
            group: None,
            picked: Picked::default(),
        }
    }

    /// Uploads the frame's bursts.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, picked: Picked) {
        self.picked = picked;
        if self.picked.total() == 0 {
            return;
        }
        let bytes: &[u8] = bytemuck::cast_slice(&self.picked.bursts);
        let len = bytes.len() as u64;
        // The buffer only grows, and the bind group is made once per buffer: the shader reads
        // how many bursts are in use from the first one.
        if self.buffer.as_ref().is_none_or(|b| b.size() < len) {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("particle bursts"),
                size: len.next_power_of_two().max(1024),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("particles"),
                layout: &self.layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
            }));
            self.buffer = Some(buffer);
        }
        queue.write_buffer(self.buffer.as_ref().unwrap(), 0, bytes);
    }

    /// Draws the prepared bursts; the globals must be bound at group 0 already. `paint` is the
    /// gradient atlas, bound at group 2.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, paint: &wgpu::BindGroup) {
        let Some(group) = &self.group else { return };
        let (additive, covering) = (self.picked.additive, self.picked.covering);
        if additive + covering == 0 {
            return;
        }
        pass.set_bind_group(1, group, &[]);
        pass.set_bind_group(2, paint, &[]);
        if additive > 0 {
            pass.set_pipeline(&self.additive);
            pass.draw(0..4, 0..additive);
        }
        if covering > 0 {
            pass.set_pipeline(&self.covering);
            pass.draw(0..4, additive..additive + covering);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn burst(time: f32, count: u32, additive: bool) -> Burst {
        Burst {
            origin: [0.0, 0.0],
            normal: [0.0, -1.0],
            time,
            color: [1.0; 4],
            count,
            speed: 300.0,
            spread: 90.0,
            size: 3.0,
            life: 1.0,
            gravity: 0.0,
            drag: 0.0,
            shape: 0,
            additive,
            seed: 7,
            paint: crate::PaintGpu::default(),
            follow: 0,
            amount: 1.0,
        }
    }

    #[test]
    fn pick_lists_live_bursts_glowing_first_and_oldest_first_with_running_indices() {
        let bursts = vec![
            burst(0.5, 10, false),
            burst(0.2, 5, true),
            burst(0.8, 3, true),
            burst(-0.5, 100, true), // over
            burst(2.0, 100, true),  // not yet
        ];
        let p = pick(&bursts, 1.0);
        assert_eq!((p.additive, p.covering, p.total()), (8, 10, 18));
        assert_eq!(p.bursts[0].timing[3], 3.0);
        let firsts: Vec<(f32, f32, f32)> =
            p.bursts.iter().map(|b| ((b.timing[0] * 10.0).round() / 10.0, b.timing[1], b.timing[2])).collect();
        assert_eq!(firsts, [(0.8, 0.0, 5.0), (0.2, 5.0, 3.0), (0.5, 8.0, 10.0)]);
        assert_eq!(p.bursts[0].place, [0.0, 0.0, 0.0, -1.0]);
        assert!((p.bursts[0].motion[1] - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        assert!(pick(&bursts, 3.0).bursts.is_empty());
    }

    #[test]
    fn pick_hands_the_paint_over_only_with_a_row() {
        let paint = PaintGpu { row: 3.0, t0: 0.25, steps: 1.0, scale: 4.0, repeat: 2.0, ..PaintGpu::default() };
        let painted = Burst { paint, follow: 2, ..burst(0.4, 4, true) };
        let rowless = Burst { paint: PaintGpu::default(), follow: 1, ..burst(0.5, 4, true) };
        let p = pick(&[painted, rowless], 1.0);
        assert_eq!(p.bursts[0].ids[2], 2);
        assert_eq!(p.bursts[0].paint_a, [3.0, 0.25, 0.0, 1.0]);
        assert_eq!(p.bursts[0].paint_b, [0.0, 4.0, 0.0, 2.0]);
        // Nothing to paint with: its own colour.
        assert_eq!(p.bursts[1].ids[2], 0);
    }

    #[test]
    fn pick_hands_the_blend_amount_over_for_the_shader_to_fade_painted_particles() {
        let paint = PaintGpu { row: 0.0, ..PaintGpu::default() };
        let faint = Burst { paint, follow: 1, amount: 0.25, ..burst(0.4, 4, false) };
        let p = pick(&[faint, burst(0.5, 4, false)], 1.0);
        assert_eq!(p.bursts[0].more[2], 0.25);
        assert_eq!(p.bursts[1].more[2], 1.0);
    }

    #[test]
    fn pick_keeps_the_newest_bursts_within_the_budgets() {
        let many: Vec<Burst> = (0..500).map(|i| burst(i as f32 * 0.001, 1, i % 2 == 0)).collect();
        let p = pick(&many, 0.6);
        assert_eq!(p.bursts.len(), MAX_BURSTS);
        assert_eq!(p.total(), MAX_BURSTS as u32);
        // The oldest hundred went.
        assert!(p.bursts.iter().all(|b| b.timing[0] <= 0.6 - 0.1 + 1e-4));
        let heavy: Vec<Burst> = (0..300).map(|i| burst(i as f32 * 0.001, 1000, true)).collect();
        let p = pick(&heavy, 0.5);
        assert_eq!(p.total(), MAX_LIVE_PARTICLES);
        assert_eq!(p.bursts.len(), 200);
        assert!((p.bursts.last().unwrap().timing[0] - (0.5 - 0.299)).abs() < 1e-5);
        let last = p.bursts.last().unwrap();
        assert_eq!(last.timing[1] + last.timing[2], MAX_LIVE_PARTICLES as f32);
        // At the budget the newest bursts keep their particles whatever their blend: the newest
        // covering burst is kept whole and the oldest glowing one is cut.
        let mixed = vec![burst(0.3, 50_000, false), burst(0.2, 100_000, true), burst(0.1, 100_000, true)];
        let p = pick(&mixed, 0.5);
        assert_eq!(p.total(), MAX_LIVE_PARTICLES);
        let counts: Vec<(f32, u32)> =
            p.bursts.iter().map(|b| ((b.timing[0] * 10.0).round() / 10.0, b.timing[2] as u32)).collect();
        assert_eq!(counts, [(0.4, 50_000), (0.3, 100_000), (0.2, 50_000)]);
    }
}

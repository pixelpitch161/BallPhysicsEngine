//! The Ripples field (gradients part 2): what a gradient laid out as Ripples measures from. When
//! a frame has Ripples ([`Frame::ripples`]) its balls and shape outlines ([`RippleSource`]s) are
//! seeded into a picture half the video's size and a jump flood leaves in each texel the two
//! nearest sources of different owners among everything, the balls and the shapes (two so a thing
//! painting itself can skip its own). paint.wgsl reads it through the paint bind group (bindings
//! 2 to 4). A frame without Ripples builds nothing and binds a 1×1 empty field.

use crate::frame::RippleSource;
use crate::{Frame, GrowableBuffer, util};

/// The field's two pictures: every list's indices, then the shapes'.
const FIELD_A: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Uint;
const FIELD_B: wgpu::TextureFormat = wgpu::TextureFormat::Rg32Uint;
/// The most jumps: enough for a field 2^15 texels across.
const MOST_JUMPS: usize = 17;
/// Uniforms sit this far apart for dynamic offsets.
const UNIFORM_STRIDE: u64 = 256;

/// A field picture's layout entry in the paint bind group (read with `textureLoad`).
pub(crate) fn field_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Uint,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

/// The sources' layout entry: a read-only storage buffer.
pub(crate) fn sources_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// The field's size for a video `width` × `height`: half, at least one texel.
pub(crate) fn field_size(width: u32, height: u32) -> (u32, u32) {
    (width.div_ceil(2).max(1), height.div_ceil(2).max(1))
}

/// The jumps of a field `w` × `h`: from half its larger side down to one, then one again.
pub(crate) fn jumps(w: u32, h: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let mut step = (w.max(h) / 2).max(1).next_power_of_two();
    while step >= 1 && out.len() < MOST_JUMPS - 1 {
        out.push(step);
        step /= 2;
    }
    out.push(1);
    out
}

/// The head the shaders read before the sources: the camera, then the video's and the field's
/// sizes (a field of 0 says none was built).
fn head(frame: &Frame, field: (u32, u32)) -> [[f32; 4]; 2] {
    let c = &frame.camera;
    let zoom = if c.zoom.is_finite() && c.zoom > 1e-6 { c.zoom } else { 1.0 };
    [
        [c.center.x, c.center.y, zoom, c.turn],
        [frame.width.max(1) as f32, frame.height.max(1) as f32, field.0 as f32, field.1 as f32],
    ]
}

/// A field picture pair: its texture views.
struct Pair {
    a: wgpu::TextureView,
    b: wgpu::TextureView,
}

impl Pair {
    fn new(device: &wgpu::Device, (w, h): (u32, u32), usage: wgpu::TextureUsages) -> Self {
        let make = |format, label| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        Self { a: make(FIELD_A, "ripples field a"), b: make(FIELD_B, "ripples field b") }
    }
}

/// The field's pictures, sized for one video size.
struct Field {
    size: (u32, u32),
    pairs: [Pair; 2],
    /// The pair the last jump wrote.
    last: usize,
}

pub(crate) struct Ripples {
    layout: wgpu::BindGroupLayout,
    seed_balls: wgpu::RenderPipeline,
    seed_shapes: wgpu::RenderPipeline,
    jump: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    sources: GrowableBuffer,
    /// The head alone, for a frame with no field.
    empty_sources: wgpu::Buffer,
    empty: Pair,
    field: Option<Field>,
    /// Whether the last frame built the field.
    built: bool,
    /// Counts the changes of what [`Ripples::entries`] binds, so paint groups know to rebind.
    generation: u64,
}

impl Ripples {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        use wgpu::ShaderStages as S;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ripples layout"),
            entries: &[
                sources_entry(0, S::VERTEX_FRAGMENT),
                util::uniform_entry(1, S::FRAGMENT, true),
                field_entry(2, S::FRAGMENT),
                field_entry(3, S::FRAGMENT),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ripples"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = util::shader(
            device,
            "ripples",
            &[include_str!("shaders/ripple_common.wgsl"), include_str!("shaders/ripples.wgsl")],
        );
        let make = |label: &'static str, vs: &str, fs: &str, masks: [wgpu::ColorWrites; 2], strip: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    targets: &[
                        Some(wgpu::ColorTargetState { format: FIELD_A, blend: None, write_mask: masks[0] }),
                        Some(wgpu::ColorTargetState { format: FIELD_B, blend: None, write_mask: masks[1] }),
                    ],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: if strip {
                        wgpu::PrimitiveTopology::TriangleStrip
                    } else {
                        wgpu::PrimitiveTopology::TriangleList
                    },
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        use wgpu::ColorWrites as W;
        // A ball goes in everything's list and the balls' (A's x and z), a shape in everything's
        // and the shapes' (A's x, B's x).
        let seed_balls = make("ripples seed balls", "vs_seed", "fs_seed", [W::RED | W::BLUE, W::empty()], true);
        let seed_shapes = make("ripples seed shapes", "vs_seed", "fs_seed", [W::RED, W::RED], true);
        let jump = make("ripples jump", "vs_full", "fs_jump", [W::ALL, W::ALL], false);
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ripples jumps"),
            size: UNIFORM_STRIDE * MOST_JUMPS as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let empty_sources = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ripples none"),
            size: 64,
            // wgpu clears new buffers: a field of size 0, none.
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let empty = Pair::new(device, (1, 1), wgpu::TextureUsages::TEXTURE_BINDING);
        Self {
            layout,
            seed_balls,
            seed_shapes,
            jump,
            uniforms,
            sources: GrowableBuffer::storage("ripples sources"),
            empty_sources,
            empty,
            field: None,
            built: false,
            generation: 0,
        }
    }

    /// Whether the last frame built the field.
    pub(crate) fn built(&self) -> bool {
        self.built
    }

    /// Changes whenever what [`Ripples::entries`] binds changes.
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    /// The paint bind group's field entries (bindings 2 to 4): the last frame's field, or none.
    pub(crate) fn entries(&self) -> [wgpu::BindGroupEntry<'_>; 3] {
        let (pair, sources) = match (&self.field, self.built) {
            (Some(f), true) => (&f.pairs[f.last], self.sources.buffer.as_ref().unwrap_or(&self.empty_sources)),
            _ => (&self.empty, &self.empty_sources),
        };
        [
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&pair.a) },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&pair.b) },
            wgpu::BindGroupEntry { binding: 4, resource: sources.as_entire_binding() },
        ]
    }

    /// Builds `frame`'s field into `encoder` when it has Ripples; otherwise leaves the empty
    /// field bound.
    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &Frame,
    ) {
        if !frame.ripples {
            if self.built {
                self.built = false;
                self.generation += 1;
            }
            return;
        }
        let size = field_size(frame.width, frame.height);
        let jumps = jumps(size.0, size.1);
        let mut changed = !self.built;
        if self.field.as_ref().is_none_or(|f| f.size != size) {
            let usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT;
            self.field =
                Some(Field { size, pairs: [Pair::new(device, size, usage), Pair::new(device, size, usage)], last: 0 });
            changed = true;
        }
        // Shapes first, then balls: one draw each.
        let (mut all, balls): (Vec<RippleSource>, Vec<RippleSource>) =
            frame.all_ripple_sources().into_iter().partition(|s| s.b[2] > 0.5);
        let shapes = all.len() as u32;
        all.extend(balls);
        let mut bytes: Vec<u8> = bytemuck::cast_slice(&head(frame, size)).to_vec();
        bytes.extend_from_slice(bytemuck::cast_slice(&all));
        changed |= self.sources.upload(device, queue, &bytes);
        let mut steps = vec![0u8; UNIFORM_STRIDE as usize * jumps.len()];
        for (i, s) in jumps.iter().enumerate() {
            let at = i * UNIFORM_STRIDE as usize;
            steps[at..at + 4].copy_from_slice(&s.to_le_bytes());
        }
        queue.write_buffer(&self.uniforms, 0, &steps);
        let field = self.field.as_mut().expect("made above");
        let sources = self.sources.buffer.as_ref().expect("uploaded above");
        let group = |read: &Pair| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("ripples"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: sources.as_entire_binding() },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &self.uniforms,
                            offset: 0,
                            size: wgpu::BufferSize::new(16),
                        }),
                    },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&read.a) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&read.b) },
                ],
            })
        };
        // `groups[n]` reads pair n, so it binds while writing the other pair.
        let groups = [group(&field.pairs[0]), group(&field.pairs[1])];
        let pass = |encoder: &mut wgpu::CommandEncoder, to: &Pair, label: &str| {
            let clear =
                wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store };
            let attachment = |view| {
                Some(wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: clear })
            };
            encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some(label),
                    color_attachments: &[attachment(&to.a), attachment(&to.b)],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime()
        };
        {
            // The seeds read nothing from the field, but the group has to bind one: pair 1.
            let mut p = pass(encoder, &field.pairs[0], "ripples seeds");
            p.set_bind_group(0, &groups[1], &[0]);
            if shapes > 0 {
                p.set_pipeline(&self.seed_shapes);
                p.draw(0..4, 0..shapes);
            }
            // Balls after shapes, so a ball on a shape's band is seeded where they overlap.
            if all.len() as u32 > shapes {
                p.set_pipeline(&self.seed_balls);
                p.draw(0..4, shapes..all.len() as u32);
            }
        }
        let mut from = 0;
        for (i, _) in jumps.iter().enumerate() {
            let to = 1 - from;
            let mut p = pass(encoder, &field.pairs[to], "ripples jump");
            p.set_pipeline(&self.jump);
            p.set_bind_group(0, &groups[from], &[(i as u64 * UNIFORM_STRIDE) as u32]);
            p.draw(0..3, 0..1);
            from = to;
        }
        changed |= field.last != from;
        field.last = from;
        self.built = true;
        if changed {
            self.generation += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_field_is_half_the_video_and_jumps_halve_down_to_one_twice() {
        assert_eq!(field_size(1080, 1920), (540, 960));
        assert_eq!(field_size(1, 1), (1, 1));
        assert_eq!(jumps(540, 960), [512, 256, 128, 64, 32, 16, 8, 4, 2, 1, 1]);
        assert_eq!(jumps(1, 1), [1, 1]);
    }

    #[test]
    fn sources_match_the_shaders_layout() {
        assert_eq!(std::mem::size_of::<RippleSource>(), 48);
    }
}

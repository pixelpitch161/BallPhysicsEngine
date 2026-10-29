//! The liquid surface: a liquid's particles splatted into a thickness picture, blurred and
//! shaded as one body of liquid that bends what is behind it.

use bytemuck::{Pod, Zeroable};

use super::util;
use crate::frame::LiquidLayer;
use crate::{HDR_FORMAT, Targets, begin_pass};

/// The format of the half-size thickness pictures.
pub const THICKNESS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;

/// Matches `struct Surface` in surface.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
struct SurfaceUniform {
    view: [f32; 4],
    color: [f32; 4],
    look: [f32; 4],
    blur: [f32; 4],
    extra: [f32; 4],
}

/// Where a layer's particle positions come from.
#[derive(Clone, Copy)]
pub enum PointsBinding<'a> {
    /// Positions from the CPU, uploaded for this frame.
    Cpu(&'a [[f32; 2]]),
    /// The state buffer of a GPU liquid: position and velocity, 16 bytes per particle.
    Gpu {
        buffer: &'a wgpu::Buffer,
        count: u32,
        spacing: f32,
        /// Layer steps the pour takes (0 when the liquid starts in place) and the step reached.
        pour_steps: f32,
        step: u64,
    },
}

/// Per-layer buffers, one set per liquid drawn in a frame.
struct Slot {
    uniform: wgpu::Buffer,
    uniform_v: wgpu::Buffer,
    points: Option<wgpu::Buffer>,
}

pub struct Surface {
    layout: wgpu::BindGroupLayout,
    splat: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    slots: Vec<Slot>,
}

impl Surface {
    pub fn new(device: &wgpu::Device) -> Self {
        use wgpu::ShaderStages as S;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("liquid surface layout"),
            entries: &[
                util::uniform_entry(0, S::VERTEX_FRAGMENT, false),
                util::storage_entry(1, S::VERTEX, true),
                util::float_texture_entry(2, S::FRAGMENT),
                util::float_texture_entry(3, S::FRAGMENT),
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: S::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("liquid surface"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = util::shader(device, "liquid surface", &[include_str!("../shaders/flow/surface.wgsl")]);
        let make = |label, vs, fs, format, blend, topology| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState { topology, ..Default::default() },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        use wgpu::PrimitiveTopology::{TriangleList, TriangleStrip};
        Self {
            splat: make("liquid splat", "vs_splat", "fs_splat", THICKNESS_FORMAT, Some(util::ADDITIVE), TriangleStrip),
            blur: make("liquid blur", "vs_full", "fs_blur", THICKNESS_FORMAT, None, TriangleList),
            composite: make(
                "liquid composite",
                "vs_full",
                "fs_composite",
                HDR_FORMAT,
                Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                TriangleList,
            ),
            layout,
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("liquid linear"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            slots: Vec::new(),
        }
    }

    /// Records one liquid's surface into `encoder`: splat, blur across and down, then shade
    /// into the HDR target over what is there. `slot` tells layers of one frame apart;
    /// `world` is the frame's size in world pixels.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        targets: &Targets,
        slot: usize,
        world: (u32, u32),
        layer: &LiquidLayer,
        points: PointsBinding,
    ) {
        while self.slots.len() <= slot {
            let uniform = |label| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(label),
                    size: std::mem::size_of::<SurfaceUniform>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            };
            self.slots.push(Slot {
                uniform: uniform("liquid surface"),
                uniform_v: uniform("liquid blur"),
                points: None,
            });
        }
        let slot = &mut self.slots[slot];
        let (count, spacing, pour_steps, step) = match points {
            PointsBinding::Cpu(cpu) => {
                let bytes: Vec<[f32; 4]> = cpu.iter().map(|p| [p[0], p[1], 0.0, 0.0]).collect();
                let size = (bytes.len() as u64 * 16).max(16);
                if slot.points.as_ref().is_none_or(|b| b.size() < size) {
                    slot.points = Some(device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("liquid points"),
                        size: size.next_power_of_two(),
                        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }));
                }
                if !bytes.is_empty() {
                    queue.write_buffer(slot.points.as_ref().unwrap(), 0, bytemuck::cast_slice(&bytes));
                }
                (cpu.len() as u32, layer.radius * 2.0, 0.0, 0)
            }
            PointsBinding::Gpu { count, spacing, pour_steps, step, .. } => (count, spacing, pour_steps, step),
        };
        if count == 0 {
            return;
        }
        let points = match points {
            PointsBinding::Cpu(_) => slot.points.as_ref().unwrap(),
            PointsBinding::Gpu { buffer, .. } => buffer,
        };
        let (w, h) = (world.0.max(1) as f32, world.1.max(1) as f32);
        // Blobs overlap their neighbours so a resting lattice reads as one body of thickness 1.
        let blob = (layer.radius.max(0.25) * 2.5).max(spacing * 1.25);
        let strength = spacing * spacing * 0.866 / (0.7134 * blob * blob);
        let world_per_texel = w / targets.thickness_size.0.max(1) as f32;
        let blur_step = (layer.smoothing.max(0.0) / world_per_texel * 0.5).min(16.0);
        let uniform = SurfaceUniform {
            view: [2.0 / w, 2.0 / h, blob, strength],
            color: [layer.color[0], layer.color[1], layer.color[2], layer.clarity.clamp(0.0, 1.0)],
            look: [layer.refraction.max(0.0), layer.highlight.max(0.0), blur_step, count as f32],
            blur: [1.0, 0.0, pour_steps, step as f32],
            extra: [layer.glow.max(0.0), 0.0, 0.0, 0.0],
        };
        queue.write_buffer(&slot.uniform, 0, bytemuck::bytes_of(&uniform));
        let vertical = SurfaceUniform { blur: [0.0, 1.0, pour_steps, step as f32], ..uniform };
        queue.write_buffer(&slot.uniform_v, 0, bytemuck::bytes_of(&vertical));

        let group = |uniform: &wgpu::Buffer, source: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("liquid surface"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: points.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(source) },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&targets.behind_view),
                    },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            })
        };
        let from_blur = group(&slot.uniform, &targets.blur);
        let from_thickness = group(&slot.uniform, &targets.thickness);
        let down = group(&slot.uniform_v, &targets.blur);
        let clear = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
        {
            let mut pass = begin_pass(encoder, &targets.thickness, "liquid splat", clear);
            pass.set_pipeline(&self.splat);
            pass.set_bind_group(0, &from_blur, &[]);
            pass.draw(0..4, 0..count);
        }
        {
            let mut pass = begin_pass(encoder, &targets.blur, "liquid blur across", clear);
            pass.set_pipeline(&self.blur);
            pass.set_bind_group(0, &from_thickness, &[]);
            pass.draw(0..3, 0..1);
        }
        {
            let mut pass = begin_pass(encoder, &targets.thickness, "liquid blur down", clear);
            pass.set_pipeline(&self.blur);
            pass.set_bind_group(0, &down, &[]);
            pass.draw(0..3, 0..1);
        }
        {
            let mut pass = begin_pass(encoder, &targets.hdr, "liquid surface", wgpu::LoadOp::Load);
            pass.set_pipeline(&self.composite);
            pass.set_bind_group(0, &from_thickness, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

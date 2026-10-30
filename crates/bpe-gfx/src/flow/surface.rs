//! The liquid surface: a liquid's particles splatted into a thickness picture, blurred and
//! shaded as one body of liquid that bends what is behind it.

use bpe_runtime::CameraView;
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
    /// The camera: centre xy, zoom, turn.
    camera: [f32; 4],
    /// The paint, as `PaintGpu`'s fields in order.
    paint_a: [f32; 4],
    paint_b: [f32; 4],
    /// Direction in degrees, span, centre.
    place: [f32; 4],
}

/// `LiquidLayer::follow` for a liquid painted by the speed of its particles.
const SPEED: u32 = 3;

/// A render pass that clears each of `views` and draws into them in order.
fn begin_cleared<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    views: &[&'a wgpu::TextureView],
    label: &str,
) -> wgpu::RenderPass<'a> {
    let attachments: Vec<_> = views
        .iter()
        .map(|&view| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })
        })
        .collect();
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &attachments,
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

/// Where a layer's particle positions come from.
#[derive(Clone, Copy)]
pub enum PointsBinding<'a> {
    /// Positions and velocities from the CPU, uploaded for this frame.
    Cpu(&'a [[f32; 4]]),
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
    /// The same, also splatting and blurring the particles' speed, for a liquid painted by it.
    splat_speed: wgpu::RenderPipeline,
    blur_speed: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    /// The shading for a liquid in a blend layer: its own light only.
    composite_layer: wgpu::RenderPipeline,
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
                util::float_texture_entry(5, S::FRAGMENT),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("liquid surface"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        // Only the shading reads the gradients.
        let paint = crate::gradients::paint_layout(device);
        let composite_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("liquid surface shading"),
            bind_group_layouts: &[Some(&layout), Some(&paint)],
            immediate_size: 0,
        });
        let module = util::shader(
            device,
            "liquid surface",
            &[&crate::gradients::paint_wgsl(1), include_str!("../shaders/flow/surface.wgsl")],
        );
        let make = |label, layout, vs, fs, formats: &[wgpu::TextureFormat], blend, topology| {
            let targets: Vec<_> = formats
                .iter()
                .map(|&format| Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL }))
                .collect();
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
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
                    targets: &targets,
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        use wgpu::PrimitiveTopology::{TriangleList, TriangleStrip};
        let (one, two) = (&[THICKNESS_FORMAT][..], &[THICKNESS_FORMAT; 2][..]);
        let additive = Some(util::ADDITIVE);
        Self {
            splat: make("liquid splat", &pipeline_layout, "vs_splat", "fs_splat", one, additive, TriangleStrip),
            blur: make("liquid blur", &pipeline_layout, "vs_full", "fs_blur", one, None, TriangleList),
            splat_speed: make(
                "liquid splat with speed",
                &pipeline_layout,
                "vs_splat",
                "fs_splat_speed",
                two,
                additive,
                TriangleStrip,
            ),
            blur_speed: make(
                "liquid blur with speed",
                &pipeline_layout,
                "vs_full",
                "fs_blur_speed",
                two,
                None,
                TriangleList,
            ),
            composite: make(
                "liquid composite",
                &composite_layout,
                "vs_full",
                "fs_composite",
                &[HDR_FORMAT],
                Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                TriangleList,
            ),
            composite_layer: make(
                "liquid composite in a layer",
                &composite_layout,
                "vs_full",
                "fs_composite_layer",
                &[HDR_FORMAT],
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
    /// into `out` (the HDR picture or a blend layer) over what is there, bending
    /// `targets.behind`. `slot` tells the liquids of one frame apart; `world` is the frame's
    /// size in world pixels, seen through `camera`; `paint` holds the frame's gradients. A
    /// liquid painted by speed splats and blurs its speed alongside. `in_layer` shades only the
    /// liquid's own light (colour, shine, rim) at its cover, without the picture under it: a
    /// blend layer's composite mixes that in, and bending it in as well would mix it twice.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        targets: &Targets,
        slot: usize,
        world: (u32, u32),
        camera: &CameraView,
        layer: &LiquidLayer,
        points: PointsBinding,
        paint: &wgpu::BindGroup,
        out: &wgpu::TextureView,
        in_layer: bool,
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
                let size = (cpu.len() as u64 * 16).max(16);
                if slot.points.as_ref().is_none_or(|b| b.size() < size) {
                    slot.points = Some(device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("liquid points"),
                        size: size.next_power_of_two(),
                        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }));
                }
                if !cpu.is_empty() {
                    queue.write_buffer(slot.points.as_ref().unwrap(), 0, bytemuck::cast_slice(cpu));
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
        let world_per_texel = w / targets.thickness_size.0.max(1) as f32 / camera.zoom.max(1e-3);
        let blur_step = (layer.smoothing.max(0.0) / world_per_texel * 0.5).min(16.0);
        // With no row there is nothing to paint with: the liquid keeps its one colour.
        let p = &layer.paint;
        let follow = if p.row < 0.0 { 0 } else { layer.follow.min(SPEED) };
        let uniform = SurfaceUniform {
            view: [2.0 / w, 2.0 / h, blob, strength],
            color: [layer.color[0], layer.color[1], layer.color[2], layer.clarity.clamp(0.0, 1.0)],
            look: [layer.refraction.max(0.0), layer.highlight.max(0.0), blur_step, count as f32],
            blur: [1.0, 0.0, pour_steps, step as f32],
            extra: [layer.glow.max(0.0), follow as f32, 0.0, 0.0],
            camera: [camera.center.x, camera.center.y, camera.zoom, camera.turn],
            paint_a: [p.row, p.t0, p.pattern, p.steps],
            paint_b: [p.angle, p.scale, p.scroll, p.repeat],
            place: layer.paint_place,
        };
        queue.write_buffer(&slot.uniform, 0, bytemuck::bytes_of(&uniform));
        let vertical = SurfaceUniform { blur: [0.0, 1.0, pour_steps, step as f32], ..uniform };
        queue.write_buffer(&slot.uniform_v, 0, bytemuck::bytes_of(&vertical));

        let group = |uniform: &wgpu::Buffer, source: &wgpu::TextureView, speed: &wgpu::TextureView| {
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
                    wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(speed) },
                ],
            })
        };
        let from_blur = group(&slot.uniform, &targets.blur, &targets.speed_blur);
        let from_thickness = group(&slot.uniform, &targets.thickness, &targets.speed);
        let down = group(&slot.uniform_v, &targets.blur, &targets.speed_blur);
        // The speed is only splatted and blurred for a liquid painted by it.
        let speed = follow == SPEED;
        let (splat, blur) = if speed { (&self.splat_speed, &self.blur_speed) } else { (&self.splat, &self.blur) };
        let pictures = |thickness, speed_view| if speed { vec![thickness, speed_view] } else { vec![thickness] };
        {
            let mut pass = begin_cleared(encoder, &pictures(&targets.thickness, &targets.speed), "liquid splat");
            pass.set_pipeline(splat);
            pass.set_bind_group(0, &from_blur, &[]);
            pass.draw(0..4, 0..count);
        }
        {
            let mut pass = begin_cleared(encoder, &pictures(&targets.blur, &targets.speed_blur), "liquid blur across");
            pass.set_pipeline(blur);
            pass.set_bind_group(0, &from_thickness, &[]);
            pass.draw(0..3, 0..1);
        }
        {
            let mut pass = begin_cleared(encoder, &pictures(&targets.thickness, &targets.speed), "liquid blur down");
            pass.set_pipeline(blur);
            pass.set_bind_group(0, &down, &[]);
            pass.draw(0..3, 0..1);
        }
        {
            let mut pass = begin_pass(encoder, out, "liquid surface", wgpu::LoadOp::Load);
            pass.set_pipeline(if in_layer { &self.composite_layer } else { &self.composite });
            pass.set_bind_group(0, &from_thickness, &[]);
            pass.set_bind_group(1, paint, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

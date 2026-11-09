//! Liquid (particles part 6): many particles run together into one smooth surface. Each
//! [`Liquid`] of a frame is drawn in two passes: its blobs are splatted into a field picture the
//! size of the picture (additive: colour times weight, and the weight alone), then a pass over the
//! whole picture cuts the field at its level into a surface, works a normal out from the field's
//! slope near the edge, and shades it as its surface says (Matte, Plastic, Chrome, Metal, Glass
//! bending the picture behind it, Toon), mixing it over what is there. A frame without a liquid
//! makes nothing here. In a lit scene a liquid can instead be shaded by the real lights (W15,
//! [`LiquidStyle::real`]): it draws its plain colour, and its colour × Lit, its height (the same
//! rounded edge) and its Surface's table index into the light passes, for the lighting mix to
//! shade as it does a ball.

use std::collections::BTreeMap;

use bytemuck::{Pod, Zeroable};

use crate::{GrowableBuffer, HDR_FORMAT, util};

/// The field picture's format: blendable floats, colour × weight and the weight.
const FIELD_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Uniforms sit this far apart for dynamic offsets.
const STRIDE: u64 = 256;
/// The most blobs one liquid draws (a guard far above what a layer makes).
pub const MOST_BLOBS: usize = 200_000;

/// One particle of a liquid: where it is and how far it reaches (world units), how much it
/// counts, and its colour (linear light, alpha unused).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct LiquidBlob {
    /// Centre xy, radius, weight.
    pub place: [f32; 4],
    pub colour: [f32; 4],
}

/// How a liquid's surface looks, as the surface pass reads it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct LiquidStyle {
    /// Level (where the field is cut), edge softness (world units), rim roundness (world units
    /// the edge takes to round over), surface code (0 Matte, 1 Plastic, 2 Chrome, 3 Metal,
    /// 4 Glass, 5 Toon).
    pub a: [f32; 4],
    /// Light angle (radians, on the screen), shine (0 to 1), bend (world units, Glass), tint
    /// (0 to 1, Glass).
    pub b: [f32; 4],
    /// Rim light (0 to 1), opacity (0 to 1), how much it bends lamp light (its index of
    /// refraction, glass and light part 3: above 1 it is free glass), w spare.
    pub c: [f32; 4],
    /// Chrome's sky: top and bottom colours (linear rgb, w spare).
    pub sky_top: [f32; 4],
    pub sky_bottom: [f32; 4],
    /// Shaded by the real lights (W15): its Lit (0: its own pretend light, the rest of `d` unread),
    /// its surface stamp (table index plus [`crate::SurfaceGpu::GLASS_FLAG`] when see-through),
    /// how much it blocks light (0 to 1), its thing's link number (pass 6, for lights that light up
    /// only some things).
    pub d: [f32; 4],
}

impl LiquidStyle {
    pub const GLASS: f32 = 4.0;

    /// Whether it reads the picture behind it (its pretend Glass: lit by the real lights, the
    /// lighting mix shows what is behind it).
    pub fn glass(&self) -> bool {
        !self.real() && (self.a[3] - Self::GLASS).abs() < 0.5
    }

    /// Whether the real lights shade it (W15).
    pub fn real(&self) -> bool {
        self.d[0] > 0.0
    }
}

/// One liquid to draw: its blobs and how it looks, behind the balls or in front of them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Liquid {
    pub blobs: Vec<LiquidBlob>,
    pub style: LiquidStyle,
    /// Drawn over the balls.
    pub front: bool,
}

/// Where a liquid's numbers sit in this frame's buffers.
#[derive(Clone, Copy, Debug)]
struct Placed {
    /// Its uniform's offset.
    offset: u32,
    /// Its blobs' first instance and count.
    first: u32,
    count: u32,
}

pub(crate) struct LiquidPass {
    splat: wgpu::RenderPipeline,
    surface: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    /// The splat pass's group: the style alone (the field is its target).
    splat_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: Option<wgpu::Buffer>,
    blobs: GrowableBuffer,
    field: Option<(u32, u32, wgpu::TextureView)>,
    group: Option<wgpu::BindGroup>,
    splat_group: Option<wgpu::BindGroup>,
    /// This frame's liquids, by their address (the frame stays put between upload and draw).
    placed: BTreeMap<usize, Placed>,
}

const ATTRIBUTES: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4];

impl LiquidPass {
    pub(crate) fn new(device: &wgpu::Device, globals_layout: &wgpu::BindGroupLayout) -> Self {
        use wgpu::ShaderStages as S;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("liquid"),
            entries: &[
                util::uniform_entry(0, S::VERTEX_FRAGMENT, true),
                util::float_texture_entry(1, S::FRAGMENT),
                util::float_texture_entry(2, S::FRAGMENT),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: S::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("liquid"),
            bind_group_layouts: &[Some(globals_layout), Some(&layout)],
            immediate_size: 0,
        });
        let splat_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("liquid splat"),
            entries: &[util::uniform_entry(0, S::VERTEX_FRAGMENT, true)],
        });
        let splat_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("liquid splat"),
            bind_group_layouts: &[Some(globals_layout), Some(&splat_layout)],
            immediate_size: 0,
        });
        let module = util::shader(
            device,
            "liquid",
            &[crate::light_pass::WGSL, include_str!("shaders/tint.wgsl"), include_str!("shaders/liquid.wgsl")],
        );
        let add = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let splat = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("liquid splat"),
            layout: Some(&splat_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_blob"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<LiquidBlob>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &ATTRIBUTES,
                })],
            },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_blob"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: FIELD_FORMAT,
                    blend: Some(wgpu::BlendState { color: add, alpha: add }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let surface = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("liquid surface"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_full"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_surface"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("liquid behind"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        Self {
            splat,
            surface,
            layout,
            splat_layout,
            sampler,
            uniforms: None,
            blobs: GrowableBuffer::new("liquid blobs"),
            field: None,
            group: None,
            splat_group: None,
            placed: BTreeMap::new(),
        }
    }

    /// Uploads every liquid of `liquids` (the frame's, its overlay's and its blend layers', in any
    /// order) for this frame's draws, the field sized `width` × `height`, the picture behind read
    /// from `behind`.
    pub(crate) fn prepare<'a>(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        liquids: impl Iterator<Item = &'a Liquid>,
        (width, height): (u32, u32),
        behind: &wgpu::TextureView,
    ) {
        self.placed.clear();
        let mut blobs: Vec<LiquidBlob> = Vec::new();
        let mut styles: Vec<u8> = Vec::new();
        for (i, l) in liquids.enumerate() {
            let count = l.blobs.len().min(MOST_BLOBS);
            if count == 0 {
                continue;
            }
            let offset = i as u64 * STRIDE;
            styles.resize(offset as usize, 0);
            styles.extend_from_slice(bytemuck::bytes_of(&l.style));
            styles.resize((offset + STRIDE) as usize, 0);
            self.placed.insert(
                l as *const Liquid as usize,
                Placed { offset: offset as u32, first: blobs.len() as u32, count: count as u32 },
            );
            blobs.extend_from_slice(&l.blobs[..count]);
        }
        if self.placed.is_empty() {
            return;
        }
        self.blobs.upload(device, queue, bytemuck::cast_slice(&blobs));
        let need = styles.len() as u64;
        if self.uniforms.as_ref().is_none_or(|b| b.size() < need) {
            self.uniforms = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("liquid styles"),
                size: need.next_power_of_two().max(STRIDE * 4),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let uniforms = self.uniforms.as_ref().expect("made above");
        queue.write_buffer(uniforms, 0, &styles);
        if self.field.as_ref().is_none_or(|(w, h, _)| (*w, *h) != (width, height)) {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("liquid field"),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FIELD_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.field = Some((width, height, texture.create_view(&Default::default())));
        }
        let field = &self.field.as_ref().expect("made above").2;
        let style = || {
            wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: uniforms,
                offset: 0,
                size: wgpu::BufferSize::new(std::mem::size_of::<LiquidStyle>() as u64),
            })
        };
        self.splat_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("liquid splat"),
            layout: &self.splat_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: style() }],
        }));
        self.group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("liquid"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: style() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(field) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(behind) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        }));
    }

    /// Whether `liquid` was uploaded this frame (it has blobs).
    pub(crate) fn has(&self, liquid: &Liquid) -> bool {
        self.placed.contains_key(&(liquid as *const Liquid as usize))
    }

    /// Draws `liquid` (uploaded by [`LiquidPass::prepare`]) over `target`, through `globals` (the
    /// camera and the light pass). A Glass liquid's picture behind must be copied first.
    pub(crate) fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        liquid: &Liquid,
        globals: &wgpu::BindGroup,
        target: &wgpu::TextureView,
    ) {
        let (Some(p), Some(group), Some(splat_group), Some((_, _, field))) =
            (self.placed.get(&(liquid as *const Liquid as usize)), &self.group, &self.splat_group, &self.field)
        else {
            return;
        };
        {
            let mut pass =
                crate::begin_pass(encoder, field, "liquid splat", wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
            pass.set_pipeline(&self.splat);
            pass.set_bind_group(0, globals, &[]);
            pass.set_bind_group(1, splat_group, &[p.offset]);
            pass.set_vertex_buffer(0, self.blobs.slice());
            pass.draw(0..4, p.first..p.first + p.count);
        }
        let mut pass = crate::begin_pass(encoder, target, "liquid surface", wgpu::LoadOp::Load);
        pass.set_pipeline(&self.surface);
        pass.set_bind_group(0, globals, &[]);
        pass.set_bind_group(1, group, &[p.offset]);
        pass.draw(0..3, 0..1);
    }
}

/// Every liquid a frame draws: its own, its overlay's and its blend layers'.
pub(crate) fn all_in(frame: &crate::Frame) -> Vec<&Liquid> {
    let mut out: Vec<&Liquid> = frame.liquids.iter().collect();
    if let Some(o) = frame.overlay.as_deref() {
        out.extend(all_in(o));
    }
    for l in &frame.layers {
        out.extend(all_in(&l.frame));
    }
    out
}

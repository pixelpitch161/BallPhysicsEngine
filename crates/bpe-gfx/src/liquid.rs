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
/// The drops' colour weights' format: the colour weight, and (liquid part 3) the drops' foam,
/// speed and kick, each times its weight.
const WEIGHTS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Uniforms sit this far apart for dynamic offsets.
const STRIDE: u64 = 512;
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
    /// Drops on the card ([`LiquidDrops`]): the grid's origin (world xy), its cell (world units)
    /// and each drop's reach (world units).
    pub e: [f32; 4],
    /// Drops on the card: Swirl detail (0 colours blend over a drop's whole reach, 1 sharp), 1
    /// when the colours are read from the second field (the drops' own colour weights), the
    /// liquid's number (only its drops are drawn; below 0 every drop), and 1 to draw every drop
    /// in `g`'s colour instead of its own.
    pub f: [f32; 4],
    /// Drops on the card: the one colour (linear rgb, w spare).
    pub g: [f32; 4],
    /// Liquid part 3, a Liquid thing's full look (read only when `v[3]` is 1; the Particles
    /// Liquid layer leaves all of these 0 and draws as before). Clarity (world units: how far in
    /// you see before its colour takes over, 0 solid), Light soaks in, Reflects (the pretend
    /// sky), Rainbow split.
    pub h: [f32; 4],
    /// Deep colour (linear, multiplied with its colour deep down), outline width (world units).
    pub i: [f32; 4],
    /// Ripples, ripple size (world units), Thin film, film size (world units).
    pub j: [f32; 4],
    /// Gives off light, Mixing shows, shows as (0 shimmer, 1 glow, 2 foam), w spare.
    pub k: [f32; 4],
    /// Outline colour (linear), w spare.
    pub l: [f32; 4],
    /// White water and foam: amount (0 none), from speed (world units a second), splashes foam,
    /// on top (world units).
    pub m: [f32; 4],
    /// Foam colour (linear), froth size (world units).
    pub n: [f32; 4],
    /// Bubbles: amount (0 none), size, size variety, rise speed (world units a second).
    pub o: [f32; 4],
    /// Bubbles: wobble, shine; Caustics: amount (0 none), size (world units).
    pub p: [f32; 4],
    /// Caustics: speed, deep (world units); Things inside: share (0 none), size (world units).
    pub q: [f32; 4],
    /// Things inside: one colour (linear), 1 to use it (else each drop's own).
    pub r: [f32; 4],
    /// Things inside: sparkle; Mist: amount (0 none), size (world units); w spare.
    pub s: [f32; 4],
    /// Caustics colour (linear), w spare.
    pub t: [f32; 4],
    /// Wet walls: amount (0 none), the wet grid's dry floor (`exp(−dries / DRY_SECONDS)`),
    /// streaks, w spare.
    pub u: [f32; 4],
    /// The wet grid's columns, rows and newest half (the pass fills these from the drops), and 1
    /// for a full look.
    pub v: [f32; 4],
    /// Wet walls' colour (linear), w spare.
    pub w: [f32; 4],
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

    /// Whether it is a Liquid thing's full look (liquid part 3).
    pub fn full(&self) -> bool {
        self.v[3] > 0.5
    }

    /// Whether its surface pass reads the picture behind it (which must be copied first): its
    /// pretend Glass, or (a full look) any surface that bends what is behind it and is clear.
    pub fn reads_behind(&self) -> bool {
        self.glass() || (self.full() && !self.real() && self.b[2] > 0.0 && self.h[0] > 0.0)
    }

    /// Whether its drops are drawn one by one after the surface (Things inside, Mist).
    pub fn dots(&self) -> bool {
        self.full() && (self.q[2] > 0.0 || self.s[1] > 0.0)
    }

    /// Whether it draws wet walls before the surface.
    pub fn wet(&self) -> bool {
        self.full() && self.u[0] > 0.0
    }
}

/// One liquid to draw: its blobs (or its drops on the card) and how it looks, behind the balls
/// or in front of them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Liquid {
    pub blobs: Vec<LiquidBlob>,
    pub style: LiquidStyle,
    /// Drawn over the balls.
    pub front: bool,
    /// The drops of a Liquid thing, drawn straight from the card's buffers (the style's `e` and
    /// `f` say where and how): drawn instead of `blobs`.
    pub drops: Option<crate::liquid_sim::DropsSource>,
}

/// Where a liquid's numbers sit in this frame's buffers.
#[derive(Clone, Debug)]
struct Placed {
    /// Its uniform's offset.
    offset: u32,
    /// Its blobs' first instance and count.
    first: u32,
    count: u32,
    /// A liquid of drops on the card: its splat's group (the style and the drops) and the buffer
    /// its draw's numbers are in.
    drops: Option<(wgpu::BindGroup, wgpu::Buffer)>,
    /// Liquid part 3: its dots' group (style, drops, field) and its wet walls' group (style,
    /// field, wet grid), when it draws them.
    dots: Option<wgpu::BindGroup>,
    wet: Option<wgpu::BindGroup>,
}

pub(crate) struct LiquidPass {
    splat: wgpu::RenderPipeline,
    /// Drops on the card, into the field and the colour weights.
    splat_drops: wgpu::RenderPipeline,
    drops_layout: wgpu::BindGroupLayout,
    surface: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    /// Liquid part 3: drops one by one after the surface (Things inside, Mist), and the wet walls
    /// before it.
    dots: wgpu::RenderPipeline,
    dots_layout: wgpu::BindGroupLayout,
    wet: wgpu::RenderPipeline,
    wet_layout: wgpu::BindGroupLayout,
    /// The splat pass's group: the style alone (the field is its target).
    splat_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: Option<wgpu::Buffer>,
    blobs: GrowableBuffer,
    /// The field, and the drops' colour weights.
    field: Option<(u32, u32, wgpu::TextureView, wgpu::TextureView)>,
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
                util::float_texture_entry(4, S::FRAGMENT),
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
        let drops_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("liquid drops"),
            entries: &[
                util::uniform_entry(0, S::VERTEX_FRAGMENT, true),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: S::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let drops_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("liquid drops"),
            bind_group_layouts: &[Some(globals_layout), Some(&drops_layout)],
            immediate_size: 0,
        });
        let drops_module = util::shader(device, "liquid drops", &[include_str!("shaders/liquid_drops.wgsl")]);
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
        let weights = wgpu::BlendState { color: add, alpha: add };
        let splat_drops = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("liquid splat drops"),
            layout: Some(&drops_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &drops_module,
                entry_point: Some("vs_drop"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &drops_module,
                entry_point: Some("fs_drop"),
                compilation_options: Default::default(),
                targets: &[
                    Some(wgpu::ColorTargetState {
                        format: FIELD_FORMAT,
                        blend: Some(weights),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: WEIGHTS_FORMAT,
                        blend: Some(weights),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                ],
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
        let storage = |binding, visibility| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let dots_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("liquid dots"),
            entries: &[
                util::uniform_entry(0, S::VERTEX_FRAGMENT, true),
                storage(1, S::VERTEX),
                util::float_texture_entry(2, S::VERTEX),
            ],
        });
        let wet_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("liquid wet"),
            entries: &[
                util::uniform_entry(0, S::VERTEX_FRAGMENT, true),
                util::float_texture_entry(1, S::FRAGMENT),
                storage(5, S::FRAGMENT),
            ],
        });
        let over = |label, module: &wgpu::ShaderModule, vs, fs, layout: &wgpu::BindGroupLayout| {
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[Some(globals_layout), Some(layout)],
                immediate_size: 0,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState {
                    topology: if vs == "vs_full" {
                        wgpu::PrimitiveTopology::TriangleList
                    } else {
                        wgpu::PrimitiveTopology::TriangleStrip
                    },
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: HDR_FORMAT,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let dots_module = util::shader(device, "liquid dots", &[include_str!("shaders/liquid_dots.wgsl")]);
        let dots = over("liquid dots", &dots_module, "vs_dot", "fs_dot", &dots_layout);
        let wet = over("liquid wet walls", &module, "vs_full", "fs_wet", &wet_layout);
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
            splat_drops,
            drops_layout,
            surface,
            layout,
            dots,
            dots_layout,
            wet,
            wet_layout,
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
        let mut drops: Vec<(usize, &crate::liquid_sim::DropsSource, bool, bool)> = Vec::new();
        for (i, l) in liquids.enumerate() {
            // Drops on another card than this renderer's can't be drawn here.
            let on_card = l.drops.as_ref().filter(|d| d.device == *device);
            let count = if on_card.is_some() { 0 } else { l.blobs.len().min(MOST_BLOBS) };
            if count == 0 && on_card.is_none() {
                continue;
            }
            let offset = i as u64 * STRIDE;
            styles.resize(offset as usize, 0);
            let mut style = l.style;
            if let Some(d) = on_card {
                // Where its wet grid's newest half is.
                style.v = [d.grid.0 as f32, d.grid.1 as f32, d.wet_half as f32, style.v[3]];
            }
            styles.extend_from_slice(bytemuck::bytes_of(&style));
            styles.resize((offset + STRIDE) as usize, 0);
            let key = l as *const Liquid as usize;
            self.placed.insert(
                key,
                Placed {
                    offset: offset as u32,
                    first: blobs.len() as u32,
                    count: count as u32,
                    drops: None,
                    dots: None,
                    wet: None,
                },
            );
            if let Some(d) = on_card {
                drops.push((key, d, l.style.dots(), l.style.wet()));
            }
            blobs.extend_from_slice(&l.blobs[..count]);
        }
        if self.placed.is_empty() {
            return;
        }
        if !blobs.is_empty() {
            self.blobs.upload(device, queue, bytemuck::cast_slice(&blobs));
        }
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
        if self.field.as_ref().is_none_or(|(w, h, _, _)| (*w, *h) != (width, height)) {
            let texture = |label, format| {
                device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some(label),
                        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    })
                    .create_view(&Default::default())
            };
            self.field = Some((
                width,
                height,
                texture("liquid field", FIELD_FORMAT),
                texture("liquid colour weights", WEIGHTS_FORMAT),
            ));
        }
        let (_, _, field, weights) = self.field.as_ref().expect("made above");
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
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(weights) },
            ],
        }));
        for (key, d, dots, wet) in drops {
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("liquid drops"),
                layout: &self.drops_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: style() },
                    wgpu::BindGroupEntry { binding: 1, resource: d.drops.as_entire_binding() },
                ],
            });
            let dots = dots.then(|| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("liquid dots"),
                    layout: &self.dots_layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: style() },
                        wgpu::BindGroupEntry { binding: 1, resource: d.drops.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(field) },
                    ],
                })
            });
            let wet = wet.then(|| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("liquid wet walls"),
                    layout: &self.wet_layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: style() },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(field) },
                        wgpu::BindGroupEntry { binding: 5, resource: d.wet.as_entire_binding() },
                    ],
                })
            });
            if let Some(p) = self.placed.get_mut(&key) {
                p.drops = Some((group, d.counts.clone()));
                p.dots = dots;
                p.wet = wet;
            }
        }
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
        let (Some(p), Some(group), Some(splat_group), Some((_, _, field, weights))) =
            (self.placed.get(&(liquid as *const Liquid as usize)), &self.group, &self.splat_group, &self.field)
        else {
            return;
        };
        if let Some(wet) = &p.wet {
            let mut pass = crate::begin_pass(encoder, target, "liquid wet walls", wgpu::LoadOp::Load);
            pass.set_pipeline(&self.wet);
            pass.set_bind_group(0, globals, &[]);
            pass.set_bind_group(1, wet, &[p.offset]);
            pass.draw(0..3, 0..1);
        }
        if let Some((drops, counts)) = &p.drops {
            let clear =
                wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store };
            let attach = |view| {
                Some(wgpu::RenderPassColorAttachment { view, resolve_target: None, ops: clear, depth_slice: None })
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("liquid splat drops"),
                color_attachments: &[attach(field), attach(weights)],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.splat_drops);
            pass.set_bind_group(0, globals, &[]);
            pass.set_bind_group(1, drops, &[p.offset]);
            pass.draw_indirect(counts, 0);
        } else {
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
        if let (Some(dots), Some((_, counts))) = (&p.dots, &p.drops) {
            pass.set_pipeline(&self.dots);
            pass.set_bind_group(1, dots, &[p.offset]);
            pass.draw_indirect(counts, 0);
        }
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

//! Lighting in the flat picture (parts 1a and 1b of the lighting spec): with a Lighting card on
//! the scene the renderer draws the things again (their lit colour, their surface and, when
//! something gives off light, that light; see [`crate::Renderer::render`]), then this works out
//! a signed distance field of everything that blocks light (a jump flood), the light from every
//! Light thing at each pixel with its shadow marched through the field (and the light in the
//! haze), the light field (radiance cascades: every glowing thing lights the rest, and lit things
//! bounce light), the Light blooms, and mixes it all in. Nothing here is made or run for a scene
//! without the card, and no 1b pass runs when its settings are off.
//!
//! Part 4 adds, each only when used: the balls stirring moving haze, the extras picture (the
//! moving haze's density and the remembered light of glow in the dark, which is kept from frame
//! to frame), and in the mix smoke, underwater light, UV glow, the memory's glow and laser beams.

use bytemuck::{Pod, Zeroable};

use crate::frame::{LightGpu, LightScene};
use crate::{Frame, GrowableBuffer, HDR_FORMAT};

/// The light field's pictures: what it is made from and what it gives.
const GI_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// The most cascades the light field uses (the last reaches 87381 field texels).
const MOST_CASCADES: u32 = 9;
/// The bloom of the lights: its first picture and this many smaller ones.
const GLOW_LEVELS: u32 = 4;

const SEED_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;
const FIELD_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;
/// The most jump flood passes: enough for a field 2^15 texels across.
const MOST_JUMPS: usize = 16;

/// What the lighting passes read (see `Lighting` in lighting.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct Uniform {
    view: [f32; 4],
    camera: [f32; 4],
    ambient: [f32; 4],
    shadows: [f32; 4],
    more: [f32; 4],
    field: [f32; 4],
}

/// What the part 1b passes read (see `Glow` in lighting.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct GlowUniform {
    /// The field's size in texels, then its size rounded up for the cascades.
    size: [f32; 4],
    /// This cascade, how many there are, steps a ray, which run (0 the first, 1 the bounce's).
    cascade: [f32; 4],
    /// Bounce light, glow reach (world units), Light blooms, the bloom level being made.
    glow: [f32; 4],
    /// The haze's colour times its amount, how much it dims.
    air: [f32; 4],
    /// 1 when there is a light field, 1 when there is a bloom, 1 when there is haze, spare.
    on: [f32; 4],
    /// Part 4: the haze moving, the water, the memory (see `Glow` in lighting.wgsl).
    haze: [f32; 4],
    haze_more: [f32; 4],
    water: [f32; 4],
    water_colour: [f32; 4],
    water_depth: [f32; 4],
    memory: [f32; 4],
    memory_colour: [f32; 4],
    memory_step: [f32; 4],
    last_camera: [f32; 4],
    more_on: [f32; 4],
}

/// Which pictures each pass's group 1 binds (besides its uniform), so no pass binds more than
/// the 16 textures a stage may see: the 1b passes, the direct light (which reads the tags for
/// Lights up), the mix and the extras pass.
const GLOW_BINDINGS: &[u32] = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
const DIRECT_BINDINGS: &[u32] = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
const MIX_BINDINGS: &[u32] = &[2, 3, 7, 8, 9, 10, 11, 12, 13];
const EXTRAS_BINDINGS: &[u32] = &[2, 6, 7, 8, 9, 14];

/// The longest gap between two frames over which the remembered light carries on; a longer jump
/// (or one back) starts it again from nothing.
const MOST_MEMORY_STEP: f32 = 0.5;

/// Glow in the dark's memory between frames.
#[derive(Default)]
struct Memory {
    /// When the last frame that remembered was drawn, and through which camera.
    last: Option<(f32, [f32; 4])>,
    /// Which of the two extras pictures holds the memory now.
    current: usize,
    /// The memory started again from nothing at a moment after the start (a seek): the editor
    /// warms it up.
    cold: bool,
}

/// How many cascades a light field needs to reach `reach` field texels, the first reaching one.
pub fn cascade_count(reach: f32) -> u32 {
    let mut n = 1;
    // Cascade i reaches (4^(i+1) - 1) / 3 texels.
    while n < MOST_CASCADES && ((4f32.powi(n as i32) - 1.0) / 3.0) < reach {
        n += 1;
    }
    n
}

/// How many steps a light field ray takes in one cascade, for a Quality.
pub fn gi_steps(quality: f32) -> u32 {
    match quality.round() as i32 {
        i32::MIN..=0 => 8,
        1 => 12,
        2 => 16,
        _ => 24,
    }
}
/// How big the field is against the picture, and how many steps a shadow takes, for a Quality.
pub fn quality_tier(quality: f32) -> (u32, u32) {
    match quality.round() as i32 {
        i32::MIN..=0 => (4, 16),
        1 => (2, 32),
        2 => (2, 64),
        _ => (1, 96),
    }
}

/// The pictures lighting draws into and reads, made the first time a frame is lit and remade
/// when the size or the Quality changes.
struct Targets {
    width: u32,
    height: u32,
    divisor: u32,
    lit: (wgpu::Texture, wgpu::TextureView),
    surface: (wgpu::Texture, wgpu::TextureView),
    /// The light things give off (pass 3).
    emission: (wgpu::Texture, wgpu::TextureView),
    /// The light from Light things on the surfaces (and the background's own light), then the
    /// light in the air.
    direct: (wgpu::Texture, wgpu::TextureView),
    air: (wgpu::Texture, wgpu::TextureView),
    /// The jump flood's pictures: the blockers' pair, then the light field's pair.
    seeds: [(wgpu::Texture, wgpu::TextureView); 4],
    field: (wgpu::Texture, wgpu::TextureView),
    /// What the light field is made from (light given off and bounced, how opaque) and how far
    /// each texel is from something opaque.
    gi_scene: (wgpu::Texture, wgpu::TextureView),
    gi_dist: (wgpu::Texture, wgpu::TextureView),
    /// The light field: the light arriving at each texel from each of four ways (see
    /// `gi_a` in lighting.wgsl).
    gi: [(wgpu::Texture, wgpu::TextureView); 3],
    /// The cascades, made the first time a light field is drawn.
    cascades: Option<[(wgpu::Texture, wgpu::TextureView); 2]>,
    /// The field's size rounded up so every cascade's probes tile it.
    padded: (u32, u32),
    /// The lights' bloom: a picture with [`GLOW_LEVELS`] levels, a view of each and of them all.
    glow: (wgpu::Texture, Vec<wgpu::TextureView>, wgpu::TextureView),
    /// Bound in place of a picture a pass draws into.
    dummy: (wgpu::Texture, wgpu::TextureView),
    dummy_colour: (wgpu::Texture, wgpu::TextureView),
    /// Part 4: what glows under UV (light pass 5) and the tags (pass 6: what catches beams, link
    /// numbers), full size.
    uv: (wgpu::Texture, wgpu::TextureView),
    tags: (wgpu::Texture, wgpu::TextureView),
    /// The extras pictures (the memory and the haze's density, field size, two to keep the last
    /// frame's memory) and the balls' stirring, made the first time they are drawn.
    extras: Option<[(wgpu::Texture, wgpu::TextureView); 2]>,
    stir: Option<(wgpu::Texture, wgpu::TextureView)>,
}

impl Targets {
    fn field_size(&self) -> (u32, u32) {
        ((self.width / self.divisor).max(1), (self.height / self.divisor).max(1))
    }
}

pub(crate) struct Lighting {
    layout: wgpu::BindGroupLayout,
    glow_layout: wgpu::BindGroupLayout,
    direct_layout: wgpu::BindGroupLayout,
    mix_layout: wgpu::BindGroupLayout,
    extras_layout: wgpu::BindGroupLayout,
    stir_layout: wgpu::BindGroupLayout,
    seed: wgpu::RenderPipeline,
    jump: wgpu::RenderPipeline,
    field: wgpu::RenderPipeline,
    direct: wgpu::RenderPipeline,
    gi_scene: wgpu::RenderPipeline,
    seed_gi: wgpu::RenderPipeline,
    gi_dist: wgpu::RenderPipeline,
    cascade: wgpu::RenderPipeline,
    resolve: wgpu::RenderPipeline,
    glow_source: wgpu::RenderPipeline,
    glow_down: wgpu::RenderPipeline,
    mix: wgpu::RenderPipeline,
    extras: wgpu::RenderPipeline,
    stir: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    /// The base uniform, then one per jump flood pass (each with its own step).
    uniforms: Vec<wgpu::Buffer>,
    /// The 1b passes' uniforms: the base one, then one per cascade of each run, then one per
    /// bloom level.
    glow_uniforms: Vec<wgpu::Buffer>,
    lights: GrowableBuffer,
    segments: GrowableBuffer,
    links: GrowableBuffer,
    stirs: GrowableBuffer,
    targets: Option<Targets>,
    memory: Memory,
}

fn texture(
    device: &wgpu::Device,
    label: &'static str,
    (w, h): (u32, u32),
    format: wgpu::TextureFormat,
    copy: bool,
) -> (wgpu::Texture, wgpu::TextureView) {
    let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
    if copy {
        usage |= wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST;
    }
    let t = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    });
    let v = t.create_view(&Default::default());
    (t, v)
}

fn uniform_buffer(device: &wgpu::Device, label: &'static str, size: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// The pictures bound to group 1 (see lighting.wgsl), in binding order.
#[derive(Clone, Copy)]
struct GlowInputs<'a> {
    emission: &'a wgpu::TextureView,
    direct: &'a wgpu::TextureView,
    air: &'a wgpu::TextureView,
    gi_scene: &'a wgpu::TextureView,
    gi_dist: &'a wgpu::TextureView,
    above: &'a wgpu::TextureView,
    gi: [&'a wgpu::TextureView; 3],
    glow: &'a wgpu::TextureView,
    tags: &'a wgpu::TextureView,
    uv: &'a wgpu::TextureView,
    extras: &'a wgpu::TextureView,
    stir: &'a wgpu::TextureView,
}

impl<'a> GlowInputs<'a> {
    /// The picture at group 1 `binding`.
    fn at(&self, binding: u32) -> &'a wgpu::TextureView {
        match binding {
            1 => self.emission,
            2 => self.direct,
            3 => self.air,
            4 => self.gi_scene,
            5 => self.gi_dist,
            6 => self.above,
            7 => self.gi[0],
            8 => self.gi[1],
            9 => self.gi[2],
            10 => self.glow,
            11 => self.tags,
            12 => self.uv,
            13 => self.extras,
            _ => self.stir,
        }
    }
}

impl Lighting {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("lighting shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/lighting.wgsl").into()),
        });
        let tex = |binding, filterable| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let storage = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let uniform = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lighting layout"),
            entries: &[
                uniform(0),
                tex(1, true),
                tex(2, true),
                tex(3, true),
                tex(4, true),
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                storage(6),
                storage(7),
                tex(8, false),
                storage(9),
            ],
        });
        let group_one = |label: &'static str, bindings: &[u32]| {
            let mut entries = vec![uniform(0)];
            entries.extend(bindings.iter().map(|b| tex(*b, true)));
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(label), entries: &entries })
        };
        let glow_layout = group_one("lighting 1b layout", GLOW_BINDINGS);
        let direct_layout = group_one("direct light layout", DIRECT_BINDINGS);
        let mix_layout = group_one("light mix layout", MIX_BINDINGS);
        let extras_layout = group_one("light extras layout", EXTRAS_BINDINGS);
        // The stirring pass reads the balls in its vertex stage.
        let stir_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("haze stir layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 12,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("lighting pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let two_groups = |label: &'static str, second: &wgpu::BindGroupLayout| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[Some(&layout), Some(second)],
                immediate_size: 0,
            })
        };
        let glow_pipeline_layout = two_groups("lighting 1b pipeline layout", &glow_layout);
        let direct_pipeline_layout = two_groups("direct light pipeline layout", &direct_layout);
        let mix_pipeline_layout = two_groups("light mix pipeline layout", &mix_layout);
        let extras_pipeline_layout = two_groups("light extras pipeline layout", &extras_layout);
        let stir_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("haze stir pipeline layout"),
            bind_group_layouts: &[Some(&stir_layout)],
            immediate_size: 0,
        });
        let make = |label: &'static str,
                    entry: &'static str,
                    layout: &wgpu::PipelineLayout,
                    formats: &[wgpu::TextureFormat],
                    blend: Option<wgpu::BlendState>| {
            let targets: Vec<_> = formats
                .iter()
                .map(|format| {
                    Some(wgpu::ColorTargetState { format: *format, blend, write_mask: wgpu::ColorWrites::ALL })
                })
                .collect();
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_full"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    targets: &targets,
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("lighting sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniforms = (0..=MOST_JUMPS)
            .map(|_| uniform_buffer(device, "lighting uniform", std::mem::size_of::<Uniform>()))
            .collect();
        let add = Some(wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::REPLACE,
        });
        let (one, two) = (&pipeline_layout, &glow_pipeline_layout);
        let stir = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("haze stir"),
            layout: Some(&stir_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_stir"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_stir"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: GI_FORMAT,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent::REPLACE,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            seed: make("light seed", "fs_seed", one, &[SEED_FORMAT], None),
            jump: make("light jump", "fs_jump", one, &[SEED_FORMAT], None),
            field: make("light field", "fs_field", one, &[FIELD_FORMAT], None),
            direct: make("direct light", "fs_direct", &direct_pipeline_layout, &[HDR_FORMAT, HDR_FORMAT], add),
            gi_scene: make("light field scene", "fs_gi_scene", two, &[GI_FORMAT], None),
            seed_gi: make("light field seed", "fs_seed_gi", two, &[SEED_FORMAT], None),
            gi_dist: make("light field distance", "fs_gi_dist", two, &[FIELD_FORMAT], None),
            cascade: make("light cascade", "fs_cascade", two, &[GI_FORMAT], None),
            resolve: make("light field resolve", "fs_gi_resolve", two, &[GI_FORMAT, GI_FORMAT, GI_FORMAT], None),
            glow_source: make("light bloom source", "fs_glow_source", two, &[GI_FORMAT], None),
            glow_down: make("light bloom down", "fs_glow_down", two, &[GI_FORMAT], None),
            mix: make("light", "fs_mix", &mix_pipeline_layout, &[HDR_FORMAT], None),
            extras: make("light extras", "fs_extras", &extras_pipeline_layout, &[GI_FORMAT], None),
            stir,
            layout,
            glow_layout,
            direct_layout,
            mix_layout,
            extras_layout,
            stir_layout,
            sampler,
            uniforms,
            glow_uniforms: Vec::new(),
            lights: GrowableBuffer::storage("lights"),
            segments: GrowableBuffer::storage("light segments"),
            links: GrowableBuffer::storage("light links"),
            stirs: GrowableBuffer::storage("haze stirs"),
            targets: None,
            memory: Memory::default(),
        }
    }

    /// Makes the lighting pictures for a `width` × `height` output lit as `scene` asks, unless
    /// they are there already.
    pub fn ensure(&mut self, device: &wgpu::Device, width: u32, height: u32, scene: &LightScene) {
        let (mut divisor, _) = quality_tier(scene.settings.more[2]);
        // The cascades are twice the (rounded up) field across: keep them within the device's limit.
        let most = device.limits().max_texture_dimension_2d;
        let padded = |divisor: u32| {
            let (w, h) = ((width / divisor).max(1), (height / divisor).max(1));
            let n = cascade_count(((w * w + h * h) as f32).sqrt());
            let tile = 1u32 << (n - 1);
            (w.div_ceil(tile) * tile, h.div_ceil(tile) * tile)
        };
        while divisor < 64 && {
            let (pw, ph) = padded(divisor);
            2 * pw.max(ph) > most
        } {
            divisor *= 2;
        }
        if self.targets.as_ref().is_some_and(|t| t.width == width && t.height == height && t.divisor == divisor) {
            return;
        }
        let full = (width, height);
        let small = ((width / divisor).max(1), (height / divisor).max(1));
        let glow = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("light bloom"),
            // At least 8 texels a side, so every level has one (a tiny draft render).
            size: wgpu::Extent3d { width: small.0.max(8), height: small.1.max(8), depth_or_array_layers: 1 },
            mip_level_count: GLOW_LEVELS,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: GI_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let levels = (0..GLOW_LEVELS)
            .map(|level| {
                glow.create_view(&wgpu::TextureViewDescriptor {
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let all = glow.create_view(&Default::default());
        let seeds = || texture(device, "light seeds", small, SEED_FORMAT, false);
        self.targets = Some(Targets {
            width,
            height,
            divisor,
            lit: texture(device, "lit colour", full, HDR_FORMAT, true),
            surface: texture(device, "surface", full, HDR_FORMAT, true),
            emission: texture(device, "light given off", full, HDR_FORMAT, true),
            direct: texture(device, "direct light", full, HDR_FORMAT, false),
            air: texture(device, "light in the air", full, HDR_FORMAT, false),
            seeds: [seeds(), seeds(), seeds(), seeds()],
            field: texture(device, "light field", small, FIELD_FORMAT, false),
            gi_scene: texture(device, "light field scene", small, GI_FORMAT, false),
            gi_dist: texture(device, "light field distance", small, FIELD_FORMAT, false),
            gi: [
                texture(device, "light field ways a", small, GI_FORMAT, false),
                texture(device, "light field ways b", small, GI_FORMAT, false),
                texture(device, "light field ways c", small, GI_FORMAT, false),
            ],
            cascades: None,
            padded: padded(divisor),
            glow: (glow, levels, all),
            dummy: texture(device, "light field stand-in", (1, 1), FIELD_FORMAT, false),
            dummy_colour: texture(device, "lighting stand-in", (1, 1), GI_FORMAT, false),
            uv: texture(device, "glows under UV", full, HDR_FORMAT, true),
            tags: texture(device, "light tags", full, HDR_FORMAT, true),
            extras: None,
            stir: None,
        });
        // New pictures: nothing is remembered.
        self.memory.last = None;
    }

    /// The lit colour, surface and emission pictures (texture, view), made by
    /// [`Lighting::ensure`].
    pub fn pictures(&self) -> Option<[(wgpu::Texture, wgpu::TextureView); 3]> {
        self.targets.as_ref().map(|t| [t.lit.clone(), t.surface.clone(), t.emission.clone()])
    }

    /// The picture light pass `mode` draws into (1 lit colour, 2 surface, 3 emission, 5 what
    /// glows under UV, 6 the tags), made by [`Lighting::ensure`].
    pub fn picture(&self, mode: u32) -> Option<(wgpu::Texture, wgpu::TextureView)> {
        let t = self.targets.as_ref()?;
        Some(match mode {
            1 => t.lit.clone(),
            2 => t.surface.clone(),
            3 => t.emission.clone(),
            5 => t.uv.clone(),
            _ => t.tags.clone(),
        })
    }

    /// Whether the remembered light (glow in the dark) started again from nothing at a moment
    /// after the start, so it is less than it would be in the video until warmed up.
    pub fn memory_cold(&self) -> bool {
        self.memory.cold
    }

    /// Forgets the remembered light (the scene changed): the next lit frame starts it again.
    pub fn forget_memory(&mut self) {
        self.memory.last = None;
    }

    /// The picture the background's own light is drawn into, before [`Lighting::run`] adds the
    /// lamps' light to it.
    pub fn background_glow(&self) -> Option<wgpu::TextureView> {
        self.targets.as_ref().map(|t| t.direct.1.clone())
    }

    /// The 1b uniform at `index`, made the first time.
    fn glow_uniform(&mut self, device: &wgpu::Device, index: usize) -> wgpu::Buffer {
        while self.glow_uniforms.len() <= index {
            self.glow_uniforms.push(uniform_buffer(device, "lighting 1b uniform", std::mem::size_of::<GlowUniform>()));
        }
        self.glow_uniforms[index].clone()
    }

    /// Lights `frame` (whose things are in `picture`, a copy of the HDR picture) into `out`.
    /// `background_glows`: the background's own light was drawn into the direct light picture.
    #[allow(clippy::too_many_arguments)]
    pub fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &Frame,
        scene: &LightScene,
        output_width: u32,
        picture: &wgpu::TextureView,
        out: &wgpu::TextureView,
        background_glows: bool,
    ) {
        if self.targets.is_none() {
            return;
        }
        let s = &scene.settings;
        let (_, steps) = quality_tier(s.more[2]);
        let (fw, fh) = self.targets.as_ref().expect("checked").field_size();
        let divisor = self.targets.as_ref().expect("checked").divisor;
        let camera = frame.camera;
        let pixel = frame.width as f32 / output_width.max(1) as f32;
        let world_pixel = if camera.zoom == 1.0 { pixel } else { pixel / camera.zoom };
        let base = Uniform {
            view: [2.0 / frame.width.max(1) as f32, 2.0 / frame.height.max(1) as f32, world_pixel, frame.time],
            camera: [camera.center.x, camera.center.y, camera.zoom, camera.turn],
            ambient: [s.ambient[0], s.ambient[1], s.ambient[2], scene.show.code()],
            shadows: s.shadows,
            more: [s.more[0], s.more[1], steps as f32, scene.lights.len() as f32],
            field: [fw as f32, fh as f32, world_pixel * divisor as f32, 0.0],
        };
        // The jump flood's steps: from half the field's size down to one, then one again.
        let mut jumps = Vec::new();
        let mut step = (fw.max(fh) / 2).max(1).next_power_of_two();
        while step >= 1 && jumps.len() < MOST_JUMPS - 1 {
            jumps.push(step);
            step /= 2;
        }
        jumps.push(1);
        queue.write_buffer(&self.uniforms[0], 0, bytemuck::bytes_of(&base));
        for (i, step) in jumps.iter().enumerate() {
            let mut u = base;
            u.field[3] = *step as f32;
            queue.write_buffer(&self.uniforms[i + 1], 0, bytemuck::bytes_of(&u));
        }
        // A lit scene with no lights still binds something.
        let lights: &[LightGpu] = if scene.lights.is_empty() { &[LightGpu::ZERO] } else { &scene.lights };
        self.lights.upload(device, queue, bytemuck::cast_slice(lights));
        let segments: &[[f32; 4]] = if scene.segments.is_empty() { &[[0.0; 4]] } else { &scene.segments };
        self.segments.upload(device, queue, bytemuck::cast_slice(segments));
        let links: &[f32] = if scene.links.is_empty() { &[0.0; 4] } else { &scene.links };
        self.links.upload(device, queue, bytemuck::cast_slice(links));

        // Part 4: the moving haze (stirred by the balls), the memory, the UV and tags pictures.
        let haze_moves = s.haze_moves();
        let stirring = haze_moves && s.haze_more[2] != 0.0 && !scene.stirs.is_empty();
        let remembers = s.remembers();
        let extras_on = haze_moves || remembers;
        let uv_on = scene.needs_uv();
        let tags_on = scene.needs_tags();
        let camera_now = [camera.center.x, camera.center.y, camera.zoom, camera.turn];
        let mut memory_step = [0.0f32; 4];
        let mut last_camera = camera_now;
        if remembers {
            let fade = s.memory[1].max(1e-3);
            let charge = s.memory[2];
            match self.memory.last {
                // Carried on from the last frame (the same frame again changes nothing).
                Some((last, cam)) if frame.time >= last && frame.time - last <= MOST_MEMORY_STEP => {
                    let dt = frame.time - last;
                    memory_step = [(-dt / fade).exp(), (1.0 - (-charge.max(0.0) * dt).exp()).clamp(0.0, 1.0), 1.0, 0.0];
                    last_camera = cam;
                    self.memory.cold = false;
                }
                // A seek: nothing remembered, cold unless this is the very start.
                _ => self.memory.cold = frame.time > 1e-4,
            }
            if stirring || remembers {
                self.memory.last = Some((frame.time, camera_now));
            }
        } else {
            self.memory.last = None;
            self.memory.cold = false;
        }
        if stirring {
            let stirs: Vec<[f32; 4]> = scene.stirs.iter().flat_map(|s| *s).collect();
            self.stirs.upload(device, queue, bytemuck::cast_slice(&stirs));
        }
        if let Some(t) = self.targets.as_mut() {
            let small = t.field_size();
            if extras_on && t.extras.is_none() {
                t.extras = Some([
                    texture(device, "light extras", small, GI_FORMAT, false),
                    texture(device, "light extras", small, GI_FORMAT, false),
                ]);
            }
            if stirring && t.stir.is_none() {
                t.stir = Some(texture(device, "haze stir", small, GI_FORMAT, false));
            }
        }

        // What part 1b draws: the light field (glowing things, bounce light), the bloom, the haze.
        let [bounce, reach, blooms, _] = s.glow;
        let field_on = scene.glows || bounce != 0.0;
        let runs = if field_on && scene.glows && bounce != 0.0 { 2 } else { usize::from(field_on) };
        let blooms_on = blooms != 0.0;
        // The light in the air is worked out for haze and for smoke that catches beams.
        let haze_on = s.air[..3].iter().any(|c| *c != 0.0) || s.air[3] != 0.0 || scene.smoke;
        if field_on && self.targets.as_ref().is_some_and(|t| t.cascades.is_none()) {
            let t = self.targets.as_mut().expect("checked");
            let size = (2 * t.padded.0, 2 * t.padded.1);
            t.cascades = Some([
                texture(device, "light cascade", size, GI_FORMAT, false),
                texture(device, "light cascade", size, GI_FORMAT, false),
            ]);
        }
        let texel_world = world_pixel * divisor as f32;
        let diagonal = ((fw * fw + fh * fh) as f32).sqrt();
        let count = cascade_count(if reach > 0.0 { diagonal.min(reach / texel_world.max(1e-6)) } else { 1.0 });
        let (pw, ph) = self.targets.as_ref().expect("checked").padded;
        let glow_base = GlowUniform {
            size: [fw as f32, fh as f32, pw as f32, ph as f32],
            cascade: [0.0, count as f32, gi_steps(s.more[2]) as f32, 0.0],
            glow: [bounce, reach, blooms, 0.0],
            air: s.air,
            on: [f32::from(u8::from(field_on)), f32::from(u8::from(blooms_on)), f32::from(u8::from(haze_on)), 0.0],
            haze: s.haze,
            haze_more: [s.haze_more[0], s.haze_more[1], if stirring { s.haze_more[2] } else { 0.0 }, 0.0],
            water: s.water,
            water_colour: s.water_colour,
            water_depth: s.water_depth,
            memory: s.memory,
            memory_colour: s.memory_colour,
            memory_step,
            last_camera,
            more_on: [
                f32::from(u8::from(uv_on)),
                f32::from(u8::from(tags_on)),
                f32::from(u8::from(extras_on)),
                f32::from(u8::from(haze_moves)),
            ],
        };
        let base_glow = self.glow_uniform(device, 0);
        queue.write_buffer(&base_glow, 0, bytemuck::bytes_of(&glow_base));
        let mut cascade_uniforms = Vec::new();
        for run in 0..runs {
            for i in 0..count {
                let b = self.glow_uniform(device, 1 + run * MOST_CASCADES as usize + i as usize);
                let mut u = glow_base;
                u.cascade[0] = i as f32;
                u.cascade[3] = run as f32;
                queue.write_buffer(&b, 0, bytemuck::bytes_of(&u));
                cascade_uniforms.push(b);
            }
        }
        let run_uniforms: Vec<wgpu::Buffer> = (0..runs)
            .map(|run| {
                let b = self.glow_uniform(device, 1 + 2 * MOST_CASCADES as usize + run);
                let mut u = glow_base;
                u.cascade[3] = run as f32;
                queue.write_buffer(&b, 0, bytemuck::bytes_of(&u));
                b
            })
            .collect();
        let level_uniforms: Vec<wgpu::Buffer> = (1..GLOW_LEVELS as usize)
            .map(|level| {
                let b = self.glow_uniform(device, 3 + 2 * MOST_CASCADES as usize + level);
                let mut u = glow_base;
                u.glow[3] = level as f32;
                queue.write_buffer(&b, 0, bytemuck::bytes_of(&u));
                b
            })
            .collect();

        let t = self.targets.as_ref().expect("checked");
        let group = |uniform: &wgpu::Buffer, field: &wgpu::TextureView, seeds: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("lighting"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(picture) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&t.lit.1) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&t.surface.1) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(field) },
                    wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: self.lights.buffer.as_ref().expect("just uploaded").as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 7,
                        resource: self.segments.buffer.as_ref().expect("just uploaded").as_entire_binding(),
                    },
                    wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::TextureView(seeds) },
                    wgpu::BindGroupEntry {
                        binding: 9,
                        resource: self.links.buffer.as_ref().expect("just uploaded").as_entire_binding(),
                    },
                ],
            })
        };
        let none = &t.dummy_colour.1;
        // The memory: last frame's in one extras picture, this frame's drawn into the other.
        let (memory_from, memory_to) = (self.memory.current, 1 - self.memory.current);
        let all = GlowInputs {
            emission: &t.emission.1,
            direct: &t.direct.1,
            air: &t.air.1,
            gi_scene: &t.gi_scene.1,
            gi_dist: &t.gi_dist.1,
            above: none,
            gi: [&t.gi[0].1, &t.gi[1].1, &t.gi[2].1],
            glow: &t.glow.2,
            tags: if tags_on { &t.tags.1 } else { none },
            uv: if uv_on { &t.uv.1 } else { none },
            extras: t.extras.as_ref().filter(|_| extras_on).map_or(none, |e| &e[memory_to].1),
            stir: t.stir.as_ref().filter(|_| stirring).map_or(none, |e| &e.1),
        };
        let group_in = |layout: &wgpu::BindGroupLayout, bindings: &[u32], uniform: &wgpu::Buffer, i: &GlowInputs| {
            let mut entries = vec![wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }];
            entries.extend(bindings.iter().map(|b| wgpu::BindGroupEntry {
                binding: *b,
                resource: wgpu::BindingResource::TextureView(i.at(*b)),
            }));
            device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("lighting 1b"), layout, entries: &entries })
        };
        let glow_group = |uniform: &wgpu::Buffer, i: &GlowInputs| group_in(&self.glow_layout, GLOW_BINDINGS, uniform, i);
        let pass = |encoder: &mut wgpu::CommandEncoder,
                    views: &[(&wgpu::TextureView, wgpu::LoadOp<wgpu::Color>)],
                    pipeline: &wgpu::RenderPipeline,
                    binds: &[&wgpu::BindGroup],
                    label: &str| {
            let attachments: Vec<_> = views
                .iter()
                .map(|(view, load)| {
                    Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: *load, store: wgpu::StoreOp::Store },
                    })
                })
                .collect();
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &attachments,
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            for (k, b) in binds.iter().enumerate() {
                pass.set_bind_group(k as u32, *b, &[]);
            }
            pass.draw(0..3, 0..1);
        };
        let clear = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
        // Seeds into the first picture, then back and forth.
        let seed = group(&self.uniforms[0], &t.field.1, &t.seeds[1].1);
        pass(encoder, &[(&t.seeds[0].1, clear)], &self.seed, &[&seed], "light seeds");
        let mut from = 0;
        for i in 0..jumps.len() {
            let to = 1 - from;
            let g = group(&self.uniforms[i + 1], &t.field.1, &t.seeds[from].1);
            pass(encoder, &[(&t.seeds[to].1, clear)], &self.jump, &[&g], "light jump");
            from = to;
        }
        let field = group(&self.uniforms[0], &t.dummy.1, &t.seeds[from].1);
        pass(encoder, &[(&t.field.1, clear)], &self.field, &[&field], "light field");
        // Every pass after this reads the blockers' field and their jump flood (nearest blocker
        // and nearest open texel).
        let main = group(&self.uniforms[0], &t.field.1, &t.seeds[from].1);

        // The light from Light things, on surfaces and in the air, added over the background's
        // own light when it gives any.
        let direct_load = if background_glows { wgpu::LoadOp::Load } else { clear };
        let g = group_in(&self.direct_layout, DIRECT_BINDINGS, &base_glow, &GlowInputs { direct: none, air: none, ..all });
        pass(encoder, &[(&t.direct.1, direct_load), (&t.air.1, clear)], &self.direct, &[&main, &g], "direct light");

        // The light field, once or twice (the second time the first's light bounces too).
        if let Some(cascades) = t.cascades.as_ref().filter(|_| field_on) {
            for run in 0..runs {
                let g = glow_group(&run_uniforms[run], &GlowInputs { gi_scene: none, ..all });
                pass(encoder, &[(&t.gi_scene.1, clear)], &self.gi_scene, &[&main, &g], "light field scene");
                if run == 0 {
                    // How far each texel is from something opaque: a jump flood of its own.
                    let g = glow_group(&base_glow, &all);
                    pass(encoder, &[(&t.seeds[2].1, clear)], &self.seed_gi, &[&main, &g], "light field seeds");
                    let mut from = 2;
                    for i in 0..jumps.len() {
                        let to = 5 - from;
                        let jg = group(&self.uniforms[i + 1], &t.field.1, &t.seeds[from].1);
                        pass(encoder, &[(&t.seeds[to].1, clear)], &self.jump, &[&jg], "light field jump");
                        from = to;
                    }
                    let sg = group(&self.uniforms[0], &t.field.1, &t.seeds[from].1);
                    let g = glow_group(&base_glow, &GlowInputs { gi_dist: none, ..all });
                    pass(encoder, &[(&t.gi_dist.1, clear)], &self.gi_dist, &[&sg, &g], "light field distance");
                }
                // The cascades, the farthest first, each merging the one above it.
                let mut into = 0;
                for i in (0..count).rev() {
                    let above = if i + 1 == count { none } else { &cascades[1 - into].1 };
                    let u = &cascade_uniforms[run * count as usize + i as usize];
                    let g = glow_group(u, &GlowInputs { above, ..all });
                    pass(encoder, &[(&cascades[into].1, clear)], &self.cascade, &[&main, &g], "light cascade");
                    into = 1 - into;
                }
                let g = glow_group(
                    &run_uniforms[run],
                    &GlowInputs { above: &cascades[1 - into].1, gi: [none, none, none], ..all },
                );
                let targets = [(&t.gi[0].1, clear), (&t.gi[1].1, clear), (&t.gi[2].1, clear)];
                pass(encoder, &targets, &self.resolve, &[&main, &g], "light field resolve");
            }
        }

        // The lights' bloom: the lamps and what gives off light, then smaller and smaller.
        if blooms_on {
            let levels = &t.glow.1;
            let g = glow_group(&base_glow, &GlowInputs { glow: none, ..all });
            pass(encoder, &[(&levels[0], clear)], &self.glow_source, &[&main, &g], "light bloom");
            for level in 1..GLOW_LEVELS as usize {
                let g = glow_group(&level_uniforms[level - 1], &GlowInputs { glow: &levels[level - 1], ..all });
                pass(encoder, &[(&levels[level], clear)], &self.glow_down, &[&main, &g], "light bloom down");
            }
        }

        // The balls stirring the haze, then the extras: the haze's density and the memory.
        if stirring && let Some(stir) = t.stir.as_ref() {
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("haze stir"),
                layout: &self.stir_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.uniforms[0].as_entire_binding() },
                    wgpu::BindGroupEntry {
                        binding: 12,
                        resource: self.stirs.buffer.as_ref().expect("just uploaded").as_entire_binding(),
                    },
                ],
            });
            let mut p = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("haze stir"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &stir.1,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: clear, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            p.set_pipeline(&self.stir);
            p.set_bind_group(0, &group, &[]);
            p.draw(0..4, 0..scene.stirs.len() as u32);
        }
        if extras_on && let Some(extras) = t.extras.as_ref() {
            let inputs = GlowInputs { above: &extras[memory_from].1, extras: none, ..all };
            let g = group_in(&self.extras_layout, EXTRAS_BINDINGS, &base_glow, &inputs);
            pass(encoder, &[(&extras[memory_to].1, clear)], &self.extras, &[&main, &g], "light extras");
        }

        let g = group_in(&self.mix_layout, MIX_BINDINGS, &base_glow, &all);
        pass(encoder, &[(out, clear)], &self.mix, &[&main, &g], "light");
        if extras_on {
            self.memory.current = memory_to;
        }
    }
}

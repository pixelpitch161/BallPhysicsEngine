//! The lighting's pipelines and layouts, built once per device.

use super::*;

/// The lighting's pipelines, their bind group layouts and sampler, and the Stage camera's
/// ([`crate::stage_camera_pass::StageKit`]): everything a lighting pass draws with that isn't a
/// picture or a buffer of its own. Building one compiles the lighting shaders, which takes a
/// moment, so it is built once for a device and shared (see [`crate::ProgramStore`]); in the
/// editor on a background thread.
#[derive(Clone)]
pub(crate) struct LightKit {
    pub(super) layout: wgpu::BindGroupLayout,
    pub(super) glow_layout: wgpu::BindGroupLayout,
    pub(super) direct_layout: wgpu::BindGroupLayout,
    pub(super) mix_layout: wgpu::BindGroupLayout,
    pub(super) extras_layout: wgpu::BindGroupLayout,
    pub(super) stir_layout: wgpu::BindGroupLayout,
    pub(super) seed: wgpu::RenderPipeline,
    pub(super) jump: wgpu::RenderPipeline,
    pub(super) field: wgpu::RenderPipeline,
    pub(super) direct: wgpu::RenderPipeline,
    pub(super) gi_scene: wgpu::RenderPipeline,
    pub(super) seed_gi: wgpu::RenderPipeline,
    pub(super) gi_dist: wgpu::RenderPipeline,
    pub(super) cascade: wgpu::RenderPipeline,
    pub(super) resolve: wgpu::RenderPipeline,
    pub(super) glow_source: wgpu::RenderPipeline,
    pub(super) glow_down: wgpu::RenderPipeline,
    pub(super) mix: wgpu::RenderPipeline,
    pub(super) extras: wgpu::RenderPipeline,
    pub(super) stir: wgpu::RenderPipeline,
    pub(super) floor: wgpu::RenderPipeline,
    pub(super) dof: wgpu::RenderPipeline,
    pub(super) dof_small: wgpu::RenderPipeline,
    pub(super) mirror: wgpu::RenderPipeline,
    pub(super) trace: wgpu::RenderPipeline,
    pub(super) denoise: wgpu::RenderPipeline,
    pub(super) trace_add: wgpu::RenderPipeline,
    pub(super) caustic: wgpu::RenderPipeline,
    pub(super) caustic_layout: wgpu::BindGroupLayout,
    pub(super) ribbon: wgpu::RenderPipeline,
    pub(super) ribbon_layout: wgpu::BindGroupLayout,
    pub(super) empty_group: wgpu::BindGroup,
    pub(super) bent: wgpu::ComputePipeline,
    pub(super) bent_splat: wgpu::RenderPipeline,
    pub(super) bent_layout: wgpu::BindGroupLayout,
    pub(super) bent_draw_layout: wgpu::BindGroupLayout,
    pub(super) free_glass: wgpu::RenderPipeline,
    pub(super) free_glass_layout: wgpu::BindGroupLayout,
    pub(super) sampler: wgpu::Sampler,
    pub(super) stage_layout: wgpu::BindGroupLayout,
    pub(super) stage: crate::stage_camera_pass::StageKit,
}

impl LightKit {
    /// Whether two kits are the same one, shared (tests).
    #[cfg(test)]
    pub(crate) fn same_as(&self, other: &LightKit) -> bool {
        self.seed == other.seed && self.stage.seed == other.stage.seed
    }

    /// Compiles the lighting shaders and makes every pipeline.
    pub(crate) fn build(device: &wgpu::Device) -> Self {
        // Glass and light part 3's photons share the lighting shader's bindings and helpers.
        let source = [crate::LIGHTING_WGSL, include_str!("../shaders/bent_light.wgsl")].concat();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("lighting shader"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let l = Layouts::build(device);
        let pipeline_layout = |label: &'static str, groups: &[&wgpu::BindGroupLayout]| {
            let groups: Vec<_> = groups.iter().map(|g| Some(*g)).collect();
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &groups,
                immediate_size: 0,
            })
        };
        let one = &pipeline_layout("lighting pipeline layout", &[&l.layout]);
        let two = &pipeline_layout("lighting 1b pipeline layout", &[&l.layout, &l.glow_layout]);
        let direct_pipeline_layout = pipeline_layout("direct light pipeline layout", &[&l.layout, &l.direct_layout]);
        let extras_pipeline_layout = pipeline_layout("light extras pipeline layout", &[&l.layout, &l.extras_layout]);
        let three = &pipeline_layout("lighting pipeline layout (stage)", &[&l.layout, &l.mix_layout, &l.stage_layout]);
        let make = |label: &'static str,
                    entry: &'static str,
                    layout: &wgpu::PipelineLayout,
                    formats: &[wgpu::TextureFormat],
                    blend: Option<wgpu::BlendState>| {
            full_screen(device, &shader, (label, entry), layout, formats, blend)
        };
        let add = Some(wgpu::BlendState { color: ADD, alpha: wgpu::BlendComponent::REPLACE });
        let add_all = wgpu::BlendState { color: ADD, alpha: ADD };
        let empty = &l.empty_layout;
        // Caustics (part 3): their own uniform and glass list in group 3, nothing in group 1.
        let caustic = strip(
            device,
            &shader,
            ("caustics", "vs_caustic", "fs_caustic"),
            &pipeline_layout("caustics pipeline layout", &[&l.layout, empty, &l.stage_layout, &l.caustic_layout]),
            &[target(HDR_FORMAT, Some(add_all))],
        );
        // Light beams' bands (glass and light part 2): their ribbons in group 3, groups 1 and 2 empty;
        // added over the picture, its cover (alpha) kept.
        let keep_alpha = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Zero,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let ribbon = strip(
            device,
            &shader,
            ("beam ribbons", "vs_ribbon", "fs_ribbon"),
            &pipeline_layout("beam ribbons pipeline layout", &[&l.layout, empty, empty, &l.ribbon_layout]),
            &[target(HDR_FORMAT, Some(wgpu::BlendState { color: ADD, alpha: keep_alpha }))],
        );
        // Light bent by glass (part 3): the photons (group 3: uniform, pairs, legs written) and the
        // splat of their legs into the direct light and the light in the air (uniform, legs read).
        let bent = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("bent light photons"),
            layout: Some(&pipeline_layout(
                "bent light photons layout",
                &[&l.layout, empty, &l.stage_layout, &l.bent_layout],
            )),
            module: &shader,
            entry_point: Some("cs_bent"),
            compilation_options: Default::default(),
            cache: None,
        });
        let bent_splat = strip(
            device,
            &shader,
            ("bent light", "vs_bent", "fs_bent"),
            &pipeline_layout("bent light splat layout", &[&l.layout, empty, empty, &l.bent_draw_layout]),
            &[target(HDR_FORMAT, Some(add_all)), target(HDR_FORMAT, Some(add_all))],
        );
        let free_glass = free_glass_pipeline(device, &pipeline_layout("free glass layout", &[&l.free_glass_layout]));
        // The balls stirring the haze, added up.
        let stir = strip(
            device,
            &shader,
            ("haze stir", "vs_stir", "fs_stir"),
            &pipeline_layout("haze stir pipeline layout", &[&l.stir_layout]),
            &[target(GI_FORMAT, add)],
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("lighting sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let stage = crate::stage_camera_pass::StageKit::build(device, &l.layout, &l.stage_layout);
        LightKit {
            seed: make("light seed", "fs_seed", one, &[SEED_FORMAT], None),
            jump: make("light jump", "fs_jump", one, &[SEED_FORMAT], None),
            field: make("light field", "fs_field", one, &[BLOCKER_FIELD_FORMAT], None),
            direct: make("direct light", "fs_direct", &direct_pipeline_layout, &[HDR_FORMAT, HDR_FORMAT], add),
            gi_scene: make("light field scene", "fs_gi_scene", two, &[GI_FORMAT], None),
            seed_gi: make("light field seed", "fs_seed_gi", two, &[SEED_FORMAT], None),
            gi_dist: make("light field distance", "fs_gi_dist", two, &[FIELD_FORMAT], None),
            cascade: make("light cascade", "fs_cascade", two, &[GI_FORMAT], None),
            resolve: make("light field resolve", "fs_gi_resolve", two, &[GI_FORMAT, GI_FORMAT, GI_FORMAT], None),
            glow_source: make("light bloom source", "fs_glow_source", two, &[GI_FORMAT], None),
            glow_down: make("light bloom down", "fs_glow_down", two, &[GI_FORMAT], None),
            mix: make("light", "fs_mix", three, &[HDR_FORMAT], None),
            extras: make("light extras", "fs_extras", &extras_pipeline_layout, &[GI_FORMAT], None),
            stir,
            floor: make("glossy floor", "fs_floor", three, &[HDR_FORMAT], None),
            dof: make("depth of field", "fs_dof", three, &[HDR_FORMAT], None),
            dof_small: make("depth of field copy", "fs_dof_small", three, &[HDR_FORMAT], None),
            mirror: make("mirror walls", "fs_mirror", three, &[HDR_FORMAT], None),
            trace: make("bounce light", "fs_trace", three, &[HDR_FORMAT], None),
            denoise: make("bounce clean-up", "fs_denoise", three, &[HDR_FORMAT], None),
            trace_add: make("bounce light added", "fs_trace_add", three, &[HDR_FORMAT], Some(add_all)),
            caustic,
            caustic_layout: l.caustic_layout,
            ribbon,
            ribbon_layout: l.ribbon_layout,
            empty_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("nothing"),
                layout: &l.empty_layout,
                entries: &[],
            }),
            bent,
            bent_splat,
            bent_layout: l.bent_layout,
            bent_draw_layout: l.bent_draw_layout,
            free_glass,
            free_glass_layout: l.free_glass_layout,
            stage_layout: l.stage_layout,
            layout: l.layout,
            glow_layout: l.glow_layout,
            direct_layout: l.direct_layout,
            mix_layout: l.mix_layout,
            extras_layout: l.extras_layout,
            stir_layout: l.stir_layout,
            sampler,
            stage,
        }
    }
}

/// Adding what is drawn to what is there.
const ADD: wgpu::BlendComponent = wgpu::BlendComponent {
    src_factor: wgpu::BlendFactor::One,
    dst_factor: wgpu::BlendFactor::One,
    operation: wgpu::BlendOperation::Add,
};

/// A picture `binding` the fragment stage reads.
fn tex(binding: u32, filterable: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

/// A buffer `binding` of `ty` that `visibility` sees.
fn buffer_entry(
    binding: u32,
    visibility: wgpu::ShaderStages,
    ty: wgpu::BufferBindingType,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    }
}

/// A read-only storage buffer the fragment stage reads.
fn storage(binding: u32) -> wgpu::BindGroupLayoutEntry {
    buffer_entry(binding, wgpu::ShaderStages::FRAGMENT, wgpu::BufferBindingType::Storage { read_only: true })
}

/// A uniform, read by the caustics' vertex stage too (as are the lights and their segments).
fn uniform(binding: u32) -> wgpu::BindGroupLayoutEntry {
    buffer_entry(binding, wgpu::ShaderStages::VERTEX_FRAGMENT, wgpu::BufferBindingType::Uniform)
}

/// One colour target of `format` blended by `blend`.
fn target(format: wgpu::TextureFormat, blend: Option<wgpu::BlendState>) -> Option<wgpu::ColorTargetState> {
    Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })
}

/// A full-screen pass's pipeline: `vs_full` and the fragment `entry`, drawing into `formats`.
fn full_screen(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    (label, entry): (&'static str, &'static str),
    layout: &wgpu::PipelineLayout,
    formats: &[wgpu::TextureFormat],
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    let targets: Vec<_> = formats.iter().map(|format| target(*format, blend)).collect();
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_full"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
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
}

/// A pipeline drawing instanced triangle strips with its own vertex and fragment entries
/// (caustics, beams' bands, bent light's legs, the haze stir).
fn strip(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    (label, vertex, fragment): (&'static str, &'static str, &'static str),
    layout: &wgpu::PipelineLayout,
    targets: &[Option<wgpu::ColorTargetState>],
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(vertex),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment),
            targets,
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

/// Marks free glass in the surface picture (`shaders/free_glass.wgsl`).
fn free_glass_pipeline(device: &wgpu::Device, layout: &wgpu::PipelineLayout) -> wgpu::RenderPipeline {
    let glass_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("free glass"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/free_glass.wgsl").into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("free glass"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &glass_shader,
            entry_point: Some("vs_glass"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &glass_shader,
            entry_point: Some("fs_glass"),
            targets: &[target(HDR_FORMAT, None)],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

/// Every bind group layout the lighting passes use.
struct Layouts {
    /// Group 0 of every pass: the uniform, the pictures, the field, the lights and links.
    layout: wgpu::BindGroupLayout,
    /// Group 1 of the 1b passes, the direct light, the mix and the extras pass.
    glow_layout: wgpu::BindGroupLayout,
    direct_layout: wgpu::BindGroupLayout,
    mix_layout: wgpu::BindGroupLayout,
    extras_layout: wgpu::BindGroupLayout,
    /// The haze stir's only group.
    stir_layout: wgpu::BindGroupLayout,
    /// Group 2: surfaces and the stage.
    stage_layout: wgpu::BindGroupLayout,
    /// Group 3 of the caustics, the beams' bands and bent light (photons, splat).
    caustic_layout: wgpu::BindGroupLayout,
    ribbon_layout: wgpu::BindGroupLayout,
    bent_layout: wgpu::BindGroupLayout,
    bent_draw_layout: wgpu::BindGroupLayout,
    free_glass_layout: wgpu::BindGroupLayout,
    /// An empty group, for the groups a pass leaves out.
    empty_layout: wgpu::BindGroupLayout,
}

impl Layouts {
    fn build(device: &wgpu::Device) -> Layouts {
        let layout_of = |label: &'static str, entries: &[wgpu::BindGroupLayoutEntry]| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(label), entries })
        };
        // What the photons of light bent by glass read (part 3) is seen by the compute stage too.
        let all_stages = wgpu::ShaderStages::VERTEX_FRAGMENT | wgpu::ShaderStages::COMPUTE;
        let fragment_compute = wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE;
        let layout = layout_of(
            "lighting layout",
            &[
                wgpu::BindGroupLayoutEntry { visibility: all_stages, ..uniform(0) },
                tex(1, true),
                tex(2, true),
                wgpu::BindGroupLayoutEntry { visibility: fragment_compute, ..tex(3, true) },
                wgpu::BindGroupLayoutEntry { visibility: fragment_compute, ..tex(4, true) },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: fragment_compute,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { visibility: all_stages, ..storage(6) },
                wgpu::BindGroupLayoutEntry { visibility: all_stages, ..storage(7) },
                tex(8, false),
                storage(9),
            ],
        );
        let group_one = |label: &'static str, bindings: &[u32]| {
            let mut entries = vec![uniform(0)];
            entries.extend(bindings.iter().map(|b| tex(*b, true)));
            layout_of(label, &entries)
        };
        let read = wgpu::BufferBindingType::Storage { read_only: true };
        let glass_tex = |binding| tex(binding, false);
        Layouts {
            layout,
            glow_layout: group_one("lighting 1b layout", GLOW_BINDINGS),
            direct_layout: group_one("direct light layout", DIRECT_BINDINGS),
            mix_layout: group_one("light mix layout", MIX_BINDINGS),
            extras_layout: group_one("light extras layout", EXTRAS_BINDINGS),
            // The stirring pass reads the balls in its vertex stage.
            stir_layout: layout_of(
                "haze stir layout",
                &[uniform(0), buffer_entry(12, wgpu::ShaderStages::VERTEX, read)],
            ),
            stage_layout: layout_of(
                "lighting stage layout",
                &[
                    wgpu::BindGroupLayoutEntry { visibility: all_stages, ..uniform(0) },
                    wgpu::BindGroupLayoutEntry { visibility: fragment_compute, ..storage(1) },
                    // The spheres and their grid: a mirror ball's photons (glass and light part 4a) find
                    // its sphere. With the lights, segments, pairs and legs that is the 8 storage
                    // buffers a compute stage may see.
                    wgpu::BindGroupLayoutEntry { visibility: fragment_compute, ..storage(2) },
                    wgpu::BindGroupLayoutEntry { visibility: fragment_compute, ..storage(3) },
                    wgpu::BindGroupLayoutEntry { visibility: fragment_compute, ..storage(4) },
                    storage(5),
                    tex(6, true),
                ],
            ),
            caustic_layout: layout_of(
                "caustics layout",
                &[uniform(0), wgpu::BindGroupLayoutEntry { visibility: wgpu::ShaderStages::VERTEX, ..storage(1) }],
            ),
            ribbon_layout: layout_of(
                "beam ribbons layout",
                &[wgpu::BindGroupLayoutEntry { visibility: wgpu::ShaderStages::VERTEX, ..storage(2) }],
            ),
            bent_layout: layout_of(
                "bent light layout",
                &[
                    buffer_entry(4, wgpu::ShaderStages::COMPUTE, wgpu::BufferBindingType::Uniform),
                    buffer_entry(5, wgpu::ShaderStages::COMPUTE, read),
                    buffer_entry(6, wgpu::ShaderStages::COMPUTE, wgpu::BufferBindingType::Storage { read_only: false }),
                ],
            ),
            bent_draw_layout: layout_of(
                "bent light splat layout",
                &[
                    buffer_entry(4, wgpu::ShaderStages::VERTEX, wgpu::BufferBindingType::Uniform),
                    buffer_entry(7, wgpu::ShaderStages::VERTEX, read),
                ],
            ),
            free_glass_layout: layout_of("free glass layout", &[glass_tex(0), glass_tex(1)]),
            empty_layout: layout_of("nothing", &[]),
        }
    }
}

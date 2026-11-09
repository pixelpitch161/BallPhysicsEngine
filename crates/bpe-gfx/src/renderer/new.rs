//! Building the [`Renderer`]: its pipelines, bind group layouts, samplers and starting pictures.

use super::*;

impl Renderer {
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let scene_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene shader"),
            source: wgpu::ShaderSource::Wgsl(scene_source(None).into()),
        });
        let post_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("post shader"),
            source: wgpu::ShaderSource::Wgsl(
                concat!(
                    include_str!("../shaders/post.wgsl"),
                    include_str!("../shaders/grade.wgsl"),
                    include_str!("../shaders/common.wgsl")
                )
                .into(),
            ),
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let screen_globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screen globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let screen_globals_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("screen globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: screen_globals.as_entire_binding() }],
        });
        let globals_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
        });

        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let sampler_entry = wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let post_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post layout"),
            entries: &[texture_entry(0), sampler_entry],
        });
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("composite layout"),
            entries: &[texture_entry(0), sampler_entry, texture_entry(2)],
        });
        let paint_layout = gradients::paint_layout(device);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("linear clamp"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let scene_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene pipeline layout"),
            bind_group_layouts: &[Some(&globals_layout), Some(&paint_layout)],
            immediate_size: 0,
        });
        let layers_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ball layers layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<LayerGpu>() as u64),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<effects::chain::Fx>() as u64),
                    },
                    count: None,
                },
                // Shapes' outline points (see paths.wgsl), and the clips of broken things' pieces,
                // whose quads the vertex stage fits to them.
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let layered_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("layered ball pipeline layout"),
            bind_group_layouts: &[Some(&globals_layout), Some(&paint_layout), Some(&layers_layout)],
            immediate_size: 0,
        });
        let post_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post pipeline layout"),
            bind_group_layouts: &[Some(&globals_layout), Some(&post_layout)],
            immediate_size: 0,
        });
        // Group 2 is the colour grade's program (`grade.wgsl`).
        let grade = GradeGpu::new(device);
        let composite_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("composite pipeline layout"),
            bind_group_layouts: &[Some(&globals_layout), Some(&composite_layout), Some(&grade.layout)],
            immediate_size: 0,
        });
        let additive = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::OVER,
        };
        let premultiplied = wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING;

        let make = |label: &str,
                    layout: &wgpu::PipelineLayout,
                    module: &wgpu::ShaderModule,
                    vs: &str,
                    fs: &str,
                    buffers: &[Option<wgpu::VertexBufferLayout>],
                    topology: wgpu::PrimitiveTopology,
                    format: wgpu::TextureFormat,
                    blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState { topology, ..Default::default() },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };

        let instances = |stride, attributes| wgpu::VertexBufferLayout {
            array_stride: stride,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes,
        };

        use wgpu::PrimitiveTopology::{TriangleList, TriangleStrip};
        let background = make(
            "background",
            &scene_pipeline_layout,
            &scene_shader,
            "vs_background",
            "fs_background",
            &[],
            TriangleList,
            HDR_FORMAT,
            None,
        );
        let path = make(
            "paths",
            &layered_pipeline_layout,
            &scene_shader,
            "vs_path",
            "fs_path",
            &[Some(instances(std::mem::size_of::<PathInstance>() as u64, &PATH_ATTRIBUTES))],
            TriangleStrip,
            HDR_FORMAT,
            Some(premultiplied),
        );
        let path_object = make(
            "path objects",
            &layered_pipeline_layout,
            &scene_shader,
            "vs_path",
            "fs_path_object",
            &[Some(instances(std::mem::size_of::<PathInstance>() as u64, &PATH_ATTRIBUTES))],
            TriangleStrip,
            OBJECT_FORMAT,
            None,
        );
        let trail = make(
            "trails",
            &scene_pipeline_layout,
            &scene_shader,
            "vs_trail",
            "fs_trail",
            &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<TrailVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &TRAIL_ATTRIBUTES,
            })],
            TriangleList,
            HDR_FORMAT,
            Some(premultiplied),
        );
        let layered = make(
            "layered balls",
            &layered_pipeline_layout,
            &scene_shader,
            "vs_layers",
            "fs_layers",
            &[Some(instances(std::mem::size_of::<LayeredInstance>() as u64, &LAYERED_ATTRIBUTES))],
            TriangleStrip,
            HDR_FORMAT,
            Some(premultiplied),
        );
        let layered_object = make(
            "layered ball objects",
            &layered_pipeline_layout,
            &scene_shader,
            "vs_layers",
            "fs_layers_object",
            &[Some(instances(std::mem::size_of::<LayeredInstance>() as u64, &LAYERED_ATTRIBUTES))],
            TriangleStrip,
            OBJECT_FORMAT,
            None,
        );
        let flash = make(
            "flashes",
            &scene_pipeline_layout,
            &scene_shader,
            "vs_flash",
            "fs_flash",
            &[Some(instances(std::mem::size_of::<FlashInstance>() as u64, &FLASH_ATTRIBUTES))],
            TriangleStrip,
            HDR_FORMAT,
            Some(additive),
        );
        let downsample = make(
            "bloom down",
            &post_pipeline_layout,
            &post_shader,
            "vs_fullscreen",
            "fs_downsample",
            &[],
            TriangleList,
            HDR_FORMAT,
            None,
        );
        let upsample = make(
            "bloom up",
            &post_pipeline_layout,
            &post_shader,
            "vs_fullscreen",
            "fs_upsample",
            &[],
            TriangleList,
            HDR_FORMAT,
            Some(additive),
        );
        let composite = make(
            "composite",
            &composite_pipeline_layout,
            &post_shader,
            "vs_fullscreen",
            "fs_composite",
            &[],
            TriangleList,
            OUTPUT_FORMAT,
            None,
        );

        let each = {
            use each::{Kind, Recipe};
            use wgpu::VertexStepMode::{Instance, Vertex};
            let recipe = |vs, fs, stride: usize, step, attributes, topology| Recipe {
                module: scene_shader.clone(),
                layout: scene_pipeline_layout.clone(),
                vs,
                fs,
                stride: stride as u64,
                step,
                attributes,
                topology,
            };
            use std::mem::size_of;
            each::EachPipelines::new([
                (
                    Kind::Trail,
                    recipe(
                        "vs_trail",
                        "fs_trail_each",
                        size_of::<TrailVertex>(),
                        Vertex,
                        &TRAIL_ATTRIBUTES,
                        TriangleList,
                    ),
                ),
                (
                    Kind::Path,
                    Recipe {
                        layout: layered_pipeline_layout.clone(),
                        ..recipe(
                            "vs_path",
                            "fs_path_each",
                            size_of::<PathInstance>(),
                            Instance,
                            &PATH_ATTRIBUTES,
                            TriangleStrip,
                        )
                    },
                ),
                (
                    Kind::Layered,
                    Recipe {
                        layout: layered_pipeline_layout.clone(),
                        ..recipe(
                            "vs_layers",
                            "fs_layers_each",
                            size_of::<LayeredInstance>(),
                            Instance,
                            &LAYERED_ATTRIBUTES,
                            TriangleStrip,
                        )
                    },
                ),
                (
                    Kind::Flash,
                    recipe(
                        "vs_flash",
                        "fs_flash_each",
                        size_of::<FlashInstance>(),
                        Instance,
                        &FLASH_ATTRIBUTES,
                        TriangleStrip,
                    ),
                ),
            ])
        };

        let targets = Targets::new(device, width, height, &post_layout, &composite_layout, &sampler);
        let first_height = gradients::atlas_height(0, device.limits().max_texture_dimension_2d as usize);
        let ripples = ripples::Ripples::new(device);
        let paint = PaintTexture::new(device, &paint_layout, &sampler, first_height, &ripples);
        let overlay_paint = PaintTexture::new(device, &paint_layout, &sampler, first_height, &ripples);
        let sprites = sprites::Sprites::new(device, &globals_layout);
        let fields = fields::Fields::new(device, &globals_layout);
        let swarms = swarm::draw::SwarmDraw::new(device, &globals_layout);
        Self {
            globals,
            globals_group,
            screen_globals,
            screen_globals_group,
            buffers: FrameBuffers::default(),
            overlay: FrameBuffers::default(),
            layer_buffers: Vec::new(),
            blend: blend::Composite::new(device),
            each,
            each_globals: each::EachGlobals::default(),
            globals_layout,
            post_layout,
            composite_layout,
            paint_layout,
            ripples,
            liquid: None,
            layers_layout,
            layered_pipeline_layout: layered_pipeline_layout.clone(),
            programs: ProgramStore::default(),
            waiting: false,
            sampler,
            background,
            trail,
            layered,
            flash,
            path,
            path_object,
            layered_object,
            chain: effects::chain::Chain::new(device),
            sprites,
            fields,
            swarms,
            downsample,
            upsample,
            composite,
            paint,
            overlay_paint,
            targets,
            grade,
            histogram: None,
            lighting: None,
            stage_camera: None,
            light_timer: None,
            lit_lights: None,
            field_like_video: false,
            light_tiles: true,
            bloom_black: false,
            light_mode: light_pass::PICTURE,
            light_stage: false,
            accumulate: None,
            light_globals: Vec::new(),
        }
    }
}

//! The look drawing's pipelines on one device (`shaders/plain.wgsl` followed by `shaders/look.wgsl`,
//! one module, so the look reads the plain drawing's field and ladder exactly as it does).

use std::sync::Arc;

use crate::draw::plain::PlainGpu;

/// The look pictures' and the light's format.
pub(crate) const PICTURE: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

#[derive(Clone, Copy)]
pub(crate) enum B {
    Uniform,
    Storage,
    /// A float texture, read texel by texel.
    Texture,
}

/// A pipeline and the bind group layout it draws with.
pub(crate) struct Pipe {
    pub(crate) pipeline: wgpu::RenderPipeline,
    pub(crate) layout: wgpu::BindGroupLayout,
}

/// The look drawing's pipelines on one device.
pub struct LookGpu {
    pub(crate) plain: Arc<PlainGpu>,
    pub(crate) splat: Pipe,
    pub(crate) weights: Pipe,
    pub(crate) nodes: Pipe,
    pub(crate) half: Pipe,
    pub(crate) light: Pipe,
    pub(crate) spread_in: Pipe,
    pub(crate) spread_down: Pipe,
    pub(crate) spread_gather: Pipe,
    pub(crate) compose: Pipe,
    pub(crate) extras: Pipe,
    /// A 1 x 1 black texture, bound where a picture isn't drawn.
    pub(crate) black: wgpu::TextureView,
}

impl std::fmt::Debug for LookGpu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LookGpu")
    }
}

impl LookGpu {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Arc<LookGpu> {
        let plain = PlainGpu::new(device, queue);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("water look"),
            source: wgpu::ShaderSource::Wgsl(
                concat!(include_str!("../../shaders/plain.wgsl"), "\n", include_str!("../../shaders/look.wgsl")).into(),
            ),
        });
        let stages = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let group = |name: &str, bindings: &[(u32, B)]| {
            let entries: Vec<wgpu::BindGroupLayoutEntry> = bindings
                .iter()
                .map(|&(binding, b)| wgpu::BindGroupLayoutEntry {
                    binding,
                    visibility: stages,
                    ty: match b {
                        B::Uniform => wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        B::Storage => wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        B::Texture => wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                    },
                    count: None,
                })
                .collect();
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(name), entries: &entries })
        };
        let add = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let summed = || {
            Some(wgpu::ColorTargetState {
                format: PICTURE,
                blend: Some(wgpu::BlendState { color: add, alpha: add }),
                write_mask: wgpu::ColorWrites::ALL,
            })
        };
        let plain_target =
            |format| Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL });
        let pipe =
            |name: &str, bindings: &[(u32, B)], vs: &str, fs: &str, targets: &[Option<wgpu::ColorTargetState>]| {
                let layout = group(name, bindings);
                let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some(name),
                    bind_group_layouts: &[Some(&layout)],
                    immediate_size: 0,
                });
                let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(name),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some(vs),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: Some(fs),
                        compilation_options: Default::default(),
                        targets,
                    }),
                    multiview_mask: None,
                    cache: None,
                });
                Pipe { pipeline, layout }
            };
        use B::{Storage as S, Texture as T, Uniform as U};
        let drops = [(0, U), (1, S), (2, S), (10, U)];
        let three = [summed(), summed(), summed()];
        let splat = pipe("water look splats", &drops, "vs_look_splat", "fs_look_splat", &three);
        let weights = pipe("water look weights", &drops, "vs_look_splat", "fs_look_weights", &three);
        let four = [summed(), summed(), summed(), summed()];
        let nodes = pipe("water look nodes", &drops, "vs_look_nodes", "fs_look_nodes", &four);
        let half = pipe(
            "water look half",
            &[(21, T), (22, T), (23, T), (34, T)],
            "vs_full",
            "fs_half",
            &[plain_target(PICTURE), plain_target(PICTURE), plain_target(PICTURE), plain_target(PICTURE)],
        );
        let light = pipe(
            "water look light",
            &[(0, U), (5, S), (10, U), (18, T), (26, U)],
            "vs_full",
            "fs_light",
            &[plain_target(PICTURE), plain_target(PICTURE), plain_target(PICTURE)],
        );
        let one = [plain_target(PICTURE)];
        let spread_in = pipe(
            "water look spread in",
            &[(0, U), (5, S), (10, U), (19, T), (20, T), (32, T)],
            "vs_full",
            "fs_spread_in",
            &one,
        );
        let spread_down = pipe("water look spread down", &[(30, T)], "vs_full", "fs_spread_down", &one);
        let spread_gather =
            pipe("water look spread gather", &[(0, U), (5, S), (10, U), (29, T)], "vs_full", "fs_spread_gather", &one);
        let compose = pipe(
            "water look",
            &[
                (0, U),
                (5, S),
                (6, T),
                (10, U),
                (11, T),
                (13, T),
                (14, T),
                (15, T),
                (16, T),
                (17, S),
                (24, T),
                (25, T),
                (27, T),
                (28, T),
                (29, T),
                (33, T),
                (35, T),
            ],
            "vs_full",
            "fs_look",
            &[plain_target(bpe_gfx::HDR_FORMAT)],
        );
        // Mist over the picture, sparks added to it (premultiplied).
        let over = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        };
        let extras = pipe(
            "water look extras",
            &[(0, U), (1, S), (2, S), (5, S), (10, U), (18, T), (24, T), (27, T)],
            "vs_extra",
            "fs_extra",
            &[Some(wgpu::ColorTargetState {
                format: bpe_gfx::HDR_FORMAT,
                blend: Some(wgpu::BlendState { color: over, alpha: over }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        );
        let black = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("water look black"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: PICTURE,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
            .create_view(&Default::default());
        // (A new texture's contents are zero.)
        Arc::new(LookGpu {
            plain,
            splat,
            weights,
            nodes,
            half,
            light,
            spread_in,
            spread_down,
            spread_gather,
            compose,
            extras,
            black,
        })
    }

    pub fn device(&self) -> &wgpu::Device {
        self.plain.device()
    }

    pub fn queue(&self) -> &wgpu::Queue {
        self.plain.queue()
    }

    /// The plain drawing's pipelines on the same device.
    pub fn plain(&self) -> &Arc<PlainGpu> {
        &self.plain
    }
}

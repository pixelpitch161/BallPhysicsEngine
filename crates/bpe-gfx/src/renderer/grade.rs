//! The colour grade's program on the GPU: the compiled stack (`crate::grade::compile`) in a
//! storage buffer the composite reads at group 2, and, for a grade with area steps, its passes:
//! each segment but the last drawn into an HDR pair (colour and kept colour), with the blur pyramid
//! of a pass's picture built before the segment that reads it.

use super::*;
use crate::grade::{self, PYRAMID_LEVELS, Program};

/// The grade's buffer and bind groups. The stack is compiled and written only when it differs
/// from the last frame's; the buffer grows (and its bind groups are made again) when a program
/// does not fit.
pub(crate) struct GradeGpu {
    pub(crate) layout: wgpu::BindGroupLayout,
    buffer: wgpu::Buffer,
    /// The program with 1x1 stand-ins for the pass pictures: a grade of one pass.
    pub(crate) group: wgpu::BindGroup,
    stand_in: wgpu::TextureView,
    /// The stack the buffer holds the program of, and that program.
    held: Option<(GradeStack, Program)>,
    /// What a grade of several passes draws with, made the first time one runs.
    areas: Option<Areas>,
}

/// The pipelines, pictures and bind groups of a grade of several passes.
struct Areas {
    pass: wgpu::RenderPipeline,
    down0: wgpu::RenderPipeline,
    down: wgpu::RenderPipeline,
    size: (u32, u32),
    /// Two pairs, written in turn: colour then kept colour.
    pairs: [[wgpu::TextureView; 2]; 2],
    /// Each mip of the pyramid as a target, as a source for the next, and all together.
    mips: Vec<wgpu::TextureView>,
    mip_sources: Vec<wgpu::BindGroup>,
    /// The first pyramid level's sources: each pair's colour.
    pair_sources: [wgpu::BindGroup; 2],
    /// Group 2 reading each pair and the pyramid.
    reads: [wgpu::BindGroup; 2],
    pyramid: wgpu::TextureView,
}

/// Bytes in one program word.
const WORD: u64 = 16;

/// What the renderer hands the grade to draw its passes with.
pub(crate) struct GradeTools<'a> {
    pub(crate) globals_layout: &'a wgpu::BindGroupLayout,
    pub(crate) composite_layout: &'a wgpu::BindGroupLayout,
    pub(crate) post_layout: &'a wgpu::BindGroupLayout,
    pub(crate) sampler: &'a wgpu::Sampler,
    pub(crate) globals: &'a wgpu::BindGroup,
    pub(crate) composite: &'a wgpu::BindGroup,
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn hdr_texture(device: &wgpu::Device, label: &str, (w, h): (u32, u32), mips: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: HDR_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

fn read_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    buffer: &wgpu::Buffer,
    [colour, kept, pyramid]: [&wgpu::TextureView; 3],
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("grade program"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(colour) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(kept) },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(pyramid) },
        ],
    })
}

/// How many mips the pyramid of a `w` × `h` picture has (its first is half size).
pub(crate) fn pyramid_levels(w: u32, h: u32) -> u32 {
    let half = (w / 2).max(h / 2).max(1);
    (32 - half.leading_zeros()).clamp(1, PYRAMID_LEVELS)
}

impl GradeGpu {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("grade layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(WORD),
                    },
                    count: None,
                },
                texture_entry(1),
                texture_entry(2),
                texture_entry(3),
            ],
        });
        let stand_in = hdr_texture(device, "grade stand-in", (1, 1), 1).create_view(&Default::default());
        // Room for a Basic panel and a Tone Curve before it first grows.
        let buffer = Self::make_buffer(device, 4 * (grade::CURVE_SAMPLES as u64 + 8));
        let group = read_group(device, &layout, &buffer, [&stand_in; 3]);
        Self { layout, buffer, group, stand_in, held: None, areas: None }
    }

    fn make_buffer(device: &wgpu::Device, words: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grade program"),
            size: words.max(1) * WORD,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Makes the buffer hold `stack`'s program, when the composite runs it (the stack does
    /// something: see `Globals::new`).
    pub(crate) fn sync(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, stack: &GradeStack) {
        if stack.is_identity() || self.held.as_ref().is_some_and(|(s, _)| s == stack) {
            return;
        }
        let program: Program = grade::compile(stack);
        let words = program.words.len() as u64;
        if words * WORD > self.buffer.size() {
            // Half again, so a growing stack doesn't remake it every frame.
            self.buffer = Self::make_buffer(device, words + words / 2);
            self.group = read_group(device, &self.layout, &self.buffer, [&self.stand_in; 3]);
            if let Some(a) = self.areas.as_mut() {
                a.reads = [0, 1].map(|i| {
                    read_group(device, &self.layout, &self.buffer, [&a.pairs[i][0], &a.pairs[i][1], &a.pyramid])
                });
            }
        }
        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&program.words));
        self.held = Some((stack.clone(), program));
    }

    /// The segments `stack`'s grade runs in (1 for no grade or no area steps).
    fn segments(&self, stack: &GradeStack) -> &[grade::Segment] {
        match &self.held {
            Some((s, p)) if !stack.is_identity() && s == stack => &p.segments,
            _ => &[],
        }
    }

    /// Draws every grade pass but the last (which the composite runs) for `stack` on a `size`
    /// picture, and returns the composite's group 2 and the segment it runs.
    pub(crate) fn run_passes(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        tools: &GradeTools,
        stack: &GradeStack,
        size: (u32, u32),
    ) -> (wgpu::BindGroup, u32) {
        let segments: Vec<grade::Segment> = self.segments(stack).to_vec();
        if segments.len() <= 1 {
            return (self.group.clone(), 0);
        }
        self.ensure_areas(device, tools, size);
        let a = self.areas.as_ref().expect("made above");
        let last = segments.len() - 1;
        for (s, segment) in segments.iter().enumerate() {
            // A segment reads the pair the one before wrote, and its pyramid when it needs it.
            let from = (s + 1) % 2;
            if s > 0 && segment.pyramid {
                Self::pyramid(a, encoder, tools.globals, from);
            }
            if s == last {
                return (a.reads[from].clone(), s as u32);
            }
            let to = s % 2;
            let attach = |view| {
                Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("grade pass"),
                color_attachments: &[attach(&a.pairs[to][0]), attach(&a.pairs[to][1])],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&a.pass);
            pass.set_bind_group(0, tools.globals, &[]);
            pass.set_bind_group(1, tools.composite, &[]);
            pass.set_bind_group(2, if s == 0 { &self.group } else { &a.reads[from] }, &[]);
            let s = s as u32;
            pass.draw(0..3, s..s + 1);
        }
        unreachable!("the last segment returns")
    }

    /// The pyramid of pair `from`'s colour.
    fn pyramid(a: &Areas, encoder: &mut wgpu::CommandEncoder, globals: &wgpu::BindGroup, from: usize) {
        for (level, target) in a.mips.iter().enumerate() {
            let mut pass = begin_pass(encoder, target, "grade pyramid", wgpu::LoadOp::Clear(wgpu::Color::BLACK));
            pass.set_bind_group(0, globals, &[]);
            if level == 0 {
                pass.set_pipeline(&a.down0);
                pass.set_bind_group(1, &a.pair_sources[from], &[]);
            } else {
                pass.set_pipeline(&a.down);
                pass.set_bind_group(1, &a.mip_sources[level - 1], &[]);
            }
            pass.draw(0..3, 0..1);
        }
    }

    /// Makes the passes' pipelines the first time, and their pictures for `size`.
    fn ensure_areas(&mut self, device: &wgpu::Device, tools: &GradeTools, size: (u32, u32)) {
        if self.areas.as_ref().is_some_and(|a| a.size == size) {
            return;
        }
        let pipelines = match self.areas.take() {
            Some(a) => (a.pass, a.down0, a.down),
            None => Self::pipelines(device, tools, &self.layout),
        };
        let pairs = [0, 1].map(|_| {
            [
                hdr_texture(device, "grade pass", size, 1).create_view(&Default::default()),
                hdr_texture(device, "grade kept", size, 1).create_view(&Default::default()),
            ]
        });
        let levels = pyramid_levels(size.0, size.1);
        let pyramid_texture = hdr_texture(device, "grade pyramid", ((size.0 / 2).max(1), (size.1 / 2).max(1)), levels);
        let pyramid = pyramid_texture.create_view(&Default::default());
        let mips: Vec<wgpu::TextureView> = (0..levels)
            .map(|l| {
                pyramid_texture.create_view(&wgpu::TextureViewDescriptor {
                    base_mip_level: l,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let source = |view: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("grade pyramid source"),
                layout: tools.post_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(tools.sampler) },
                ],
            })
        };
        let mip_sources = mips[..mips.len() - 1].iter().map(source).collect();
        let pair_sources = [0, 1].map(|i| source(&pairs[i][0]));
        let reads =
            [0, 1].map(|i| read_group(device, &self.layout, &self.buffer, [&pairs[i][0], &pairs[i][1], &pyramid]));
        let (pass, down0, down) = pipelines;
        self.areas = Some(Areas { pass, down0, down, size, pairs, mips, mip_sources, pair_sources, reads, pyramid });
    }

    fn pipelines(
        device: &wgpu::Device,
        tools: &GradeTools,
        layout: &wgpu::BindGroupLayout,
    ) -> (wgpu::RenderPipeline, wgpu::RenderPipeline, wgpu::RenderPipeline) {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("grade passes"),
            source: wgpu::ShaderSource::Wgsl(post_source().into()),
        });
        let pass_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("grade pass layout"),
            bind_group_layouts: &[Some(tools.globals_layout), Some(tools.composite_layout), Some(layout)],
            immediate_size: 0,
        });
        let down_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("grade pyramid layout"),
            bind_group_layouts: &[Some(tools.globals_layout), Some(tools.post_layout)],
            immediate_size: 0,
        });
        let target =
            Some(wgpu::ColorTargetState { format: HDR_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL });
        let make = |label, layout: &wgpu::PipelineLayout, vs, fs, targets: &[Option<wgpu::ColorTargetState>]| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets,
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        (
            make("grade pass", &pass_layout, "vs_grade", "fs_grade_pass", &[target.clone(), target.clone()]),
            make("grade pyramid first", &down_layout, "vs_fullscreen", "fs_grade_down0", std::slice::from_ref(&target)),
            make("grade pyramid", &down_layout, "vs_fullscreen", "fs_grade_down", &[target]),
        )
    }
}

/// The post shader's source: bloom and the composite, the grade, the shared helpers.
pub(crate) fn post_source() -> &'static str {
    concat!(
        include_str!("../shaders/post.wgsl"),
        include_str!("../shaders/grade.wgsl"),
        include_str!("../shaders/common.wgsl")
    )
}

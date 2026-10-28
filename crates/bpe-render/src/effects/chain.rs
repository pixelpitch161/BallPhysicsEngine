//! Runs a scene's effects, in order, over the HDR picture.

use super::{EffectInfo, EffectLibrary, MAX_PARAMS};
use crate::frame::FrameData;
use bpe_scene::Scene;
use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;
use std::sync::Arc;

pub const MAX_HITS: usize = 16;
/// A jump in time bigger than this (a seek, or a new export) starts feedback effects afresh.
const MAX_STEP: f64 = 0.25;

/// Matches `struct Fx` in prelude.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct FxUniform {
    resolution: [f32; 2],
    time: f32,
    amount: f32,
    pulse: f32,
    hit_count: u32,
    frame: f32,
    dt: f32,
    scale: f32,
    level: f32,
    view: u32,
    pad: f32,
    audio: [f32; 4],
    params: [f32; MAX_PARAMS],
    hits: [[f32; 4]; MAX_HITS],
    hit_colors: [[f32; 4]; MAX_HITS],
}

struct Pipeline {
    info: Arc<EffectInfo>,
    /// `None` when the GPU refused the shader even though naga accepted it.
    pipeline: Option<wgpu::RenderPipeline>,
}

/// Per-position state: the uniform buffer and, for feedback effects, last frame's output.
struct Slot {
    id: String,
    uniform: wgpu::Buffer,
    history: Option<(wgpu::Texture, wgpu::TextureView)>,
    frames: u32,
}

pub(crate) struct EffectChain {
    library: EffectLibrary,
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    sampler: wgpu::Sampler,
    black: wgpu::TextureView,
    pipelines: HashMap<String, Pipeline>,
    slots: Vec<Slot>,
    scratch: Option<(wgpu::Texture, wgpu::TextureView)>,
    last_time: Option<f64>,
    gpu_errors: Vec<(String, String)>,
}

impl EffectChain {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
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
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effect layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture_entry(1),
                texture_entry(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("effect pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("effect sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let black = make_texture(device, 1, 1, format, "effect black").1;
        Self {
            library: EffectLibrary::new(),
            layout,
            pipeline_layout,
            sampler,
            black,
            pipelines: HashMap::new(),
            slots: Vec::new(),
            scratch: None,
            last_time: None,
            gpu_errors: Vec::new(),
        }
    }

    pub fn library(&self) -> &EffectLibrary {
        &self.library
    }

    pub fn set_library(&mut self, library: EffectLibrary) {
        self.library = library;
        self.pipelines.clear();
        self.gpu_errors.clear();
    }

    /// Effects that passed the shader check but that the GPU still refused, as (id, message).
    pub fn gpu_errors(&self) -> &[(String, String)] {
        &self.gpu_errors
    }

    /// Forgets feedback history, so the next frame starts the echo effects from black.
    pub fn reset(&mut self) {
        self.last_time = None;
    }

    /// Applies the scene's enabled effects to `hdr` in place, recording the passes into `encoder`.
    #[allow(clippy::too_many_arguments)]
    pub fn apply(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        hdr: &wgpu::Texture,
        hdr_view: &wgpu::TextureView,
        scene: &Scene,
        time: f64,
        frame: &FrameData,
    ) {
        let active: Vec<_> = scene
            .effects
            .iter()
            .filter(|e| e.enabled && e.amount > 0.0)
            .filter_map(|effect| {
                let info = self.library.get(&effect.shader)?;
                info.compiled.is_ok().then_some((effect, info))
            })
            .collect();
        for (_, info) in &active {
            self.prepare(device, info);
        }
        let active: Vec<_> =
            active.into_iter().filter(|(_, info)| self.pipelines[&info.id].pipeline.is_some()).collect();
        if active.is_empty() {
            self.last_time = None;
            return;
        }

        let (width, height) = (hdr.width(), hdr.height());
        let fresh = match self.last_time {
            Some(last) => time < last || time - last > MAX_STEP,
            None => true,
        };
        let dt = match self.last_time {
            Some(last) if !fresh => (time - last) as f32,
            _ => 1.0 / scene.video.fps.max(1) as f32,
        };
        self.last_time = Some(time);
        if self.scratch.as_ref().is_none_or(|(t, _)| t.width() != width || t.height() != height) {
            self.scratch = Some(make_texture(device, width, height, hdr.format(), "effect scratch"));
        }

        let video = &scene.video;
        let mut base = FxUniform::zeroed();
        base.resolution = [width as f32, height as f32];
        base.time = time as f32;
        base.pulse = frame.pulse;
        base.dt = dt;
        base.scale = width as f32 / video.width.max(1) as f32;
        base.hit_count = frame.hits.len().min(MAX_HITS) as u32;
        for (i, hit) in frame.hits.iter().take(MAX_HITS).enumerate() {
            let uv = [hit.position[0] / video.width as f32 + 0.5, hit.position[1] / video.height as f32 + 0.5];
            base.hits[i] = [uv[0], uv[1], hit.age, hit.strength];
            base.hit_colors[i] = hit.color;
        }

        let (scratch_texture, scratch_view) = self.scratch.as_ref().unwrap();
        let mut in_scratch = false;
        for (index, (effect, info)) in active.iter().enumerate() {
            let pipeline = self.pipelines[&info.id].pipeline.as_ref().unwrap();

            if self.slots.len() <= index {
                self.slots.push(Slot {
                    id: String::new(),
                    uniform: device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("effect uniform"),
                        size: std::mem::size_of::<FxUniform>() as u64,
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }),
                    history: None,
                    frames: 0,
                });
            }
            let slot = &mut self.slots[index];
            if slot.id != info.id || fresh {
                slot.id = info.id.clone();
                slot.frames = 0;
            }
            if info.uses_prev {
                let stale = slot.history.as_ref().is_none_or(|(t, _)| t.width() != width || t.height() != height);
                if stale {
                    slot.history = Some(make_texture(device, width, height, hdr.format(), "effect history"));
                    slot.frames = 0;
                }
                if slot.frames == 0 {
                    clear(encoder, &slot.history.as_ref().unwrap().1);
                }
            }

            let mut uniform = base;
            uniform.amount = effect.amount.clamp(0.0, 1.0);
            uniform.frame = slot.frames as f32;
            uniform.params = info.values(&effect.params);
            queue.write_buffer(&slot.uniform, 0, bytemuck::bytes_of(&uniform));
            slot.frames = slot.frames.saturating_add(1);

            let (source, target, target_texture) =
                if in_scratch { (scratch_view, hdr_view, hdr) } else { (hdr_view, scratch_view, scratch_texture) };
            let prev = match (&slot.history, info.uses_prev) {
                (Some((_, view)), true) => view,
                _ => &self.black,
            };
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("effect inputs"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: slot.uniform.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(source) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(prev) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            });
            {
                let mut pass = crate::begin_pass(encoder, target, "effect", wgpu::LoadOp::Clear(wgpu::Color::BLACK));
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            if info.uses_prev
                && let Some((history, _)) = &slot.history
            {
                copy(encoder, target_texture, history);
            }
            in_scratch = !in_scratch;
        }
        if in_scratch {
            copy(encoder, scratch_texture, hdr);
        }
    }

    /// Builds the pipeline for an effect, or rebuilds it when the library has a newer version.
    fn prepare(&mut self, device: &wgpu::Device, info: &Arc<EffectInfo>) {
        let current = self.pipelines.get(&info.id).is_some_and(|p| Arc::ptr_eq(&p.info, info));
        if !current {
            self.gpu_errors.retain(|(id, _)| *id != info.id);
            let Ok(source) = &info.compiled else { return };
            let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(&info.id),
                source: wgpu::ShaderSource::Wgsl(source.as_str().into()),
            });
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(&info.id),
                layout: Some(&self.pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: crate::HDR_FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
            let pipeline = match pollster::block_on(scope.pop()) {
                None => Some(pipeline),
                Some(error) => {
                    self.gpu_errors.push((info.id.clone(), error.to_string()));
                    None
                }
            };
            self.pipelines.insert(info.id.clone(), Pipeline { info: info.clone(), pipeline });
        }
    }
}

fn make_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    label: &str,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    (texture, view)
}

fn clear(encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
    crate::begin_pass(encoder, view, "effect history clear", wgpu::LoadOp::Clear(wgpu::Color::BLACK));
}

fn copy(encoder: &mut wgpu::CommandEncoder, from: &wgpu::Texture, to: &wgpu::Texture) {
    encoder.copy_texture_to_texture(from.as_image_copy(), to.as_image_copy(), from.size());
}

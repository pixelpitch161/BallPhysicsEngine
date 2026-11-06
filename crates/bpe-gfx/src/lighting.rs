//! Lighting in the flat picture (part 1a of the lighting spec): with a Lighting card on the scene
//! the renderer draws the things twice more (their lit colour and their surface, see
//! [`crate::Renderer::render`]), then this works out a signed distance field of everything that
//! blocks light (a jump flood) and adds each light at every pixel, its shadow marched through the
//! field. Nothing here is made or run for a scene without the card.

use bytemuck::{Pod, Zeroable};

use crate::frame::{LightGpu, LightScene};
use crate::{Frame, GrowableBuffer, HDR_FORMAT};

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
    seeds: [(wgpu::Texture, wgpu::TextureView); 2],
    field: (wgpu::Texture, wgpu::TextureView),
    /// Bound as the field while the field itself is drawn.
    dummy: (wgpu::Texture, wgpu::TextureView),
}

impl Targets {
    fn field_size(&self) -> (u32, u32) {
        ((self.width / self.divisor).max(1), (self.height / self.divisor).max(1))
    }
}

pub(crate) struct Lighting {
    layout: wgpu::BindGroupLayout,
    seed: wgpu::RenderPipeline,
    jump: wgpu::RenderPipeline,
    field: wgpu::RenderPipeline,
    light: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    /// The base uniform, then one per jump flood pass (each with its own step).
    uniforms: Vec<wgpu::Buffer>,
    lights: GrowableBuffer,
    segments: GrowableBuffer,
    targets: Option<Targets>,
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
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lighting layout"),
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
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("lighting pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let make = |label: &'static str, entry: &'static str, format: wgpu::TextureFormat| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_full"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
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
            .map(|_| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("lighting uniform"),
                    size: std::mem::size_of::<Uniform>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            })
            .collect();
        Self {
            seed: make("light seed", "fs_seed", SEED_FORMAT),
            jump: make("light jump", "fs_jump", SEED_FORMAT),
            field: make("light field", "fs_field", FIELD_FORMAT),
            light: make("light", "fs_light", HDR_FORMAT),
            layout,
            sampler,
            uniforms,
            lights: GrowableBuffer::storage("lights"),
            segments: GrowableBuffer::storage("light segments"),
            targets: None,
        }
    }

    /// Makes the lighting pictures for a `width` × `height` output at `quality`, unless they are
    /// there already.
    pub fn ensure(&mut self, device: &wgpu::Device, width: u32, height: u32, quality: f32) {
        let (divisor, _) = quality_tier(quality);
        if self.targets.as_ref().is_some_and(|t| t.width == width && t.height == height && t.divisor == divisor) {
            return;
        }
        let full = (width, height);
        let small = ((width / divisor).max(1), (height / divisor).max(1));
        self.targets = Some(Targets {
            width,
            height,
            divisor,
            lit: texture(device, "lit colour", full, HDR_FORMAT, true),
            surface: texture(device, "surface", full, HDR_FORMAT, true),
            seeds: [
                texture(device, "light seeds", small, SEED_FORMAT, false),
                texture(device, "light seeds", small, SEED_FORMAT, false),
            ],
            field: texture(device, "light field", small, FIELD_FORMAT, false),
            dummy: texture(device, "light field stand-in", (1, 1), FIELD_FORMAT, false),
        });
    }

    /// The lit colour and surface pictures (texture, view), made by [`Lighting::ensure`].
    pub fn pictures(&self) -> Option<[(wgpu::Texture, wgpu::TextureView); 2]> {
        self.targets.as_ref().map(|t| [t.lit.clone(), t.surface.clone()])
    }

    /// Lights `frame` (whose things are in `picture`, a copy of the HDR picture) into `out`.
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
    ) {
        let Some(t) = self.targets.as_ref() else { return };
        let s = &scene.settings;
        let (_, steps) = quality_tier(s.more[2]);
        let (fw, fh) = t.field_size();
        let camera = frame.camera;
        let pixel = frame.width as f32 / output_width.max(1) as f32;
        let world_pixel = if camera.zoom == 1.0 { pixel } else { pixel / camera.zoom };
        let base = Uniform {
            view: [2.0 / frame.width.max(1) as f32, 2.0 / frame.height.max(1) as f32, world_pixel, frame.time],
            camera: [camera.center.x, camera.center.y, camera.zoom, camera.turn],
            ambient: [s.ambient[0], s.ambient[1], s.ambient[2], 0.0],
            shadows: s.shadows,
            more: [s.more[0], s.more[1], steps as f32, scene.lights.len() as f32],
            field: [fw as f32, fh as f32, world_pixel * t.divisor as f32, 0.0],
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
                ],
            })
        };
        let pass = |encoder: &mut wgpu::CommandEncoder,
                    view: &wgpu::TextureView,
                    pipeline: &wgpu::RenderPipeline,
                    bind: &wgpu::BindGroup,
                    label: &str| {
            let mut pass = crate::begin_pass(encoder, view, label, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.draw(0..3, 0..1);
        };
        // Seeds into the first picture, then back and forth.
        let seed = group(&self.uniforms[0], &t.field.1, &t.seeds[1].1);
        pass(encoder, &t.seeds[0].1, &self.seed, &seed, "light seeds");
        let mut from = 0;
        for i in 0..jumps.len() {
            let to = 1 - from;
            let g = group(&self.uniforms[i + 1], &t.field.1, &t.seeds[from].1);
            pass(encoder, &t.seeds[to].1, &self.jump, &g, "light jump");
            from = to;
        }
        let field = group(&self.uniforms[0], &t.dummy.1, &t.seeds[from].1);
        pass(encoder, &t.field.1, &self.field, &field, "light field");
        let light = group(&self.uniforms[0], &t.field.1, &t.seeds[from].1);
        pass(encoder, out, &self.light, &light, "light");
    }
}

//! Lighting in the flat picture (part 1a of the lighting spec): with a Lighting card on the scene
//! the renderer draws the things twice more (their lit colour and their surface, see
//! [`crate::Renderer::render`]), then this works out a signed distance field of everything that
//! blocks light (a jump flood) and adds each light at every pixel, its shadow marched through the
//! field. Nothing here is made or run for a scene without the card.

use bytemuck::{Pod, Zeroable};

use crate::frame::{LightGpu, LightScene, SphereGpu, SurfaceGpu};
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

/// What the surfaces and the stage read (see `Stage` in lighting.wgsl, part 2a).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct StageUniform {
    room: [f32; 4],
    floor: [f32; 4],
    more: [f32; 4],
    sky_top: [f32; 4],
    sky_bottom: [f32; 4],
    grid: [f32; 4],
    grid_size: [f32; 4],
}

/// The most cells a sphere grid has along a side.
const MOST_CELLS: f32 = 256.0;

/// A uniform grid over the stage's spheres, built on the CPU each frame: shadow rays and contact
/// shading look up the spheres near a point by its cell. Each sphere is listed in every cell its
/// disc reaches, widened by `margin` (twice the largest radius) so penumbras and contact
/// shading near a ball find it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SphereGrid {
    /// The grid's corner (world), cell size, the largest radius.
    pub grid: [f32; 4],
    /// Cells across, cells down, the margin, 0.
    pub size: [f32; 4],
    /// Each cell's first item and how many, row by row.
    pub cells: Vec<[u32; 2]>,
    /// The spheres' indices, cell by cell.
    pub items: Vec<u32>,
}

impl SphereGrid {
    pub fn build(spheres: &[SphereGpu]) -> SphereGrid {
        let live: Vec<(usize, &SphereGpu)> = spheres
            .iter()
            .enumerate()
            .filter(|(_, s)| s.a.iter().take(3).all(|v| v.is_finite()) && s.a[2] > 0.0)
            .collect();
        if live.is_empty() {
            return SphereGrid {
                grid: [0.0, 0.0, 1.0, 0.0],
                size: [1.0, 1.0, 0.0, 0.0],
                cells: vec![[0, 0]],
                items: Vec::new(),
            };
        }
        let rmax = live.iter().map(|(_, s)| s.a[2]).fold(0.0f32, f32::max);
        let margin = 2.0 * rmax;
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for (_, s) in &live {
            let reach = s.a[2] + margin;
            for k in 0..2 {
                lo[k] = lo[k].min(s.a[k] - reach);
                hi[k] = hi[k].max(s.a[k] + reach);
            }
        }
        let (w, h) = ((hi[0] - lo[0]).max(1e-3), (hi[1] - lo[1]).max(1e-3));
        let mut cell = (2.0 * rmax).max((w * h / live.len() as f32).sqrt());
        cell = cell.max(w / MOST_CELLS).max(h / MOST_CELLS);
        let nx = ((w / cell).ceil() as usize).max(1);
        let ny = ((h / cell).ceil() as usize).max(1);
        let cell_of = |v: f32, k: usize, n: usize| (((v - lo[k]) / cell).floor().max(0.0) as usize).min(n - 1);
        let mut lists: Vec<Vec<u32>> = vec![Vec::new(); nx * ny];
        for (i, s) in &live {
            let reach = s.a[2] + margin;
            let (x0, x1) = (cell_of(s.a[0] - reach, 0, nx), cell_of(s.a[0] + reach, 0, nx));
            let (y0, y1) = (cell_of(s.a[1] - reach, 1, ny), cell_of(s.a[1] + reach, 1, ny));
            for y in y0..=y1 {
                for x in x0..=x1 {
                    lists[y * nx + x].push(*i as u32);
                }
            }
        }
        let mut cells = Vec::with_capacity(lists.len());
        let mut items = Vec::new();
        for list in lists {
            cells.push([items.len() as u32, list.len() as u32]);
            items.extend(list);
        }
        SphereGrid { grid: [lo[0], lo[1], cell, rmax], size: [nx as f32, ny as f32, margin, 0.0], cells, items }
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
    seeds: [(wgpu::Texture, wgpu::TextureView); 2],
    field: (wgpu::Texture, wgpu::TextureView),
    /// The light pass's picture when a glossy floor mirrors it afterwards (made when first needed).
    stage: Option<(wgpu::Texture, wgpu::TextureView)>,
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
    /// The glossy floor's pass (Stage).
    floor: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    /// Surfaces and the stage (group 1 of the light and floor passes).
    stage_layout: wgpu::BindGroupLayout,
    stage_uniform: wgpu::Buffer,
    surfaces: GrowableBuffer,
    spheres: GrowableBuffer,
    cells: GrowableBuffer,
    items: GrowableBuffer,
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
        let stage_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lighting stage layout"),
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
                storage(1),
                storage(2),
                storage(3),
                storage(4),
            ],
        });
        let lit_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("lighting pipeline layout (stage)"),
            bind_group_layouts: &[Some(&layout), Some(&stage_layout)],
            immediate_size: 0,
        });
        let make = |label: &'static str, entry: &'static str, format: wgpu::TextureFormat| {
            let layout = if entry == "fs_light" || entry == "fs_floor" { &lit_layout } else { &pipeline_layout };
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
            floor: make("glossy floor", "fs_floor", HDR_FORMAT),
            stage_uniform: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("lighting stage uniform"),
                size: std::mem::size_of::<StageUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            stage_layout,
            surfaces: GrowableBuffer::storage("light surfaces"),
            spheres: GrowableBuffer::storage("light spheres"),
            cells: GrowableBuffer::storage("light sphere cells"),
            items: GrowableBuffer::storage("light sphere items"),
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
            stage: None,
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
        let s = &scene.settings;
        // A glossy floor mirrors the lit picture: the light pass draws into a picture of its own.
        let reflects = s.stage[0] > 0.5 && s.floor[0] > 0.5 && s.floor[2] > 0.5;
        if reflects && let Some(t) = self.targets.as_mut().filter(|t| t.stage.is_none()) {
            t.stage = Some(texture(device, "lit stage", (t.width, t.height), HDR_FORMAT, false));
        }
        let Some(t) = self.targets.as_ref() else { return };
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
        // Surfaces (0 always the plain sheet) and, in Stage, the spheres and their grid.
        let mut surfaces = scene.surfaces.clone();
        if surfaces.is_empty() {
            surfaces.push(SurfaceGpu::MATTE);
        }
        surfaces[0] = SurfaceGpu::MATTE;
        self.surfaces.upload(device, queue, bytemuck::cast_slice(&surfaces));
        let stage = s.stage[0] > 0.5;
        let grid = if stage { SphereGrid::build(&scene.spheres) } else { SphereGrid::build(&[]) };
        let spheres: &[SphereGpu] =
            if stage && !scene.spheres.is_empty() { &scene.spheres } else { &[SphereGpu { a: [0.0; 4], b: [0.0; 4] }] };
        self.spheres.upload(device, queue, bytemuck::cast_slice(spheres));
        self.cells.upload(device, queue, bytemuck::cast_slice(&grid.cells));
        let items: &[u32] = if grid.items.is_empty() { &[0] } else { &grid.items };
        self.items.upload(device, queue, bytemuck::cast_slice(items));
        let sphere_count = if stage && !grid.items.is_empty() { scene.spheres.len() as f32 } else { 0.0 };
        let uniform = StageUniform {
            room: s.stage,
            floor: s.floor,
            more: [s.floor_more[0], s.floor_more[1], surfaces.len() as f32, sphere_count],
            sky_top: s.sky_top,
            sky_bottom: s.sky_bottom,
            grid: grid.grid,
            grid_size: grid.size,
        };
        queue.write_buffer(&self.stage_uniform, 0, bytemuck::bytes_of(&uniform));
        fn buffer(b: &GrowableBuffer) -> wgpu::BindingResource<'_> {
            b.buffer.as_ref().expect("just uploaded").as_entire_binding()
        }
        let stage_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("lighting stage"),
            layout: &self.stage_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.stage_uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: buffer(&self.surfaces) },
                wgpu::BindGroupEntry { binding: 2, resource: buffer(&self.spheres) },
                wgpu::BindGroupEntry { binding: 3, resource: buffer(&self.cells) },
                wgpu::BindGroupEntry { binding: 4, resource: buffer(&self.items) },
            ],
        });

        let group = |uniform: &wgpu::Buffer,
                     picture: &wgpu::TextureView,
                     field: &wgpu::TextureView,
                     seeds: &wgpu::TextureView| {
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
                    stage: Option<&wgpu::BindGroup>,
                    label: &str| {
            let mut pass = crate::begin_pass(encoder, view, label, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind, &[]);
            if let Some(stage) = stage {
                pass.set_bind_group(1, stage, &[]);
            }
            pass.draw(0..3, 0..1);
        };
        // Seeds into the first picture, then back and forth.
        let seed = group(&self.uniforms[0], picture, &t.field.1, &t.seeds[1].1);
        pass(encoder, &t.seeds[0].1, &self.seed, &seed, None, "light seeds");
        let mut from = 0;
        for i in 0..jumps.len() {
            let to = 1 - from;
            let g = group(&self.uniforms[i + 1], picture, &t.field.1, &t.seeds[from].1);
            pass(encoder, &t.seeds[to].1, &self.jump, &g, None, "light jump");
            from = to;
        }
        let field = group(&self.uniforms[0], picture, &t.dummy.1, &t.seeds[from].1);
        pass(encoder, &t.field.1, &self.field, &field, None, "light field");
        let light = group(&self.uniforms[0], picture, &t.field.1, &t.seeds[from].1);
        match t.stage.as_ref().filter(|_| reflects) {
            Some((_, lit)) => {
                pass(encoder, lit, &self.light, &light, Some(&stage_group), "light");
                let floor = group(&self.uniforms[0], lit, &t.field.1, &t.seeds[from].1);
                pass(encoder, out, &self.floor, &floor, Some(&stage_group), "glossy floor");
            }
            None => pass(encoder, out, &self.light, &light, Some(&stage_group), "light"),
        }
    }
}

#[cfg(test)]
mod grid_tests {
    use super::*;

    fn ball(x: f32, y: f32, r: f32) -> SphereGpu {
        SphereGpu { a: [x, y, r, 0.0], b: [1.0; 4] }
    }

    #[test]
    fn an_empty_grid_has_one_empty_cell() {
        let g = SphereGrid::build(&[]);
        assert_eq!(g.cells, vec![[0, 0]]);
        assert!(g.items.is_empty());
    }

    #[test]
    fn every_sphere_is_found_from_every_cell_its_widened_disc_reaches() {
        let balls: Vec<SphereGpu> = (0..200)
            .map(|i| ball((i * 37 % 900) as f32 - 450.0, (i * 91 % 1600) as f32 - 800.0, 6.0 + (i % 5) as f32))
            .collect();
        let g = SphereGrid::build(&balls);
        let (lo, cell, margin) = ([g.grid[0], g.grid[1]], g.grid[2], g.size[2]);
        let (nx, ny) = (g.size[0] as usize, g.size[1] as usize);
        assert_eq!(g.cells.len(), nx * ny);
        for (i, b) in balls.iter().enumerate() {
            // Points round the ball, out to its radius plus the margin, each in a cell listing it.
            for k in 0..16 {
                let a = k as f32 * std::f32::consts::TAU / 16.0;
                let r = (b.a[2] + margin) * 0.999;
                let (x, y) = (b.a[0] + r * a.cos(), b.a[1] + r * a.sin());
                let cx = (((x - lo[0]) / cell).floor() as usize).min(nx - 1);
                let cy = (((y - lo[1]) / cell).floor() as usize).min(ny - 1);
                let [first, n] = g.cells[cy * nx + cx];
                let list = &g.items[first as usize..(first + n) as usize];
                assert!(list.contains(&(i as u32)), "ball {i} missing from cell ({cx}, {cy})");
            }
        }
        assert!(nx <= 256 && ny <= 256);
    }
}

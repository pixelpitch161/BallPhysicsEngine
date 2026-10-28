//! Liquid on the graphics card, from 8,000 up to 300,000 particles, drawn as a smooth surface.

use super::sdf::{self, Layout, SdfField, Walls};
use super::util::{self, groups};
use bpe_scene::{Scene, Vec2};
use bytemuck::{Pod, Zeroable};

/// Small steps per swarm/liquid step, for a steadier liquid.
const SUBSTEPS: u32 = 2;
/// Most grid cells the prefix sum can handle: 1024 blocks of 1024.
const MAX_CELLS: u32 = 1024 * 1024 - 1;
/// Closest the particles may sit, in world pixels.
const MIN_SPACING: f32 = 0.6;

/// Matches `struct Params` in fluid.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
struct FluidParams {
    counts: [u32; 4],
    shape: [f32; 4],
    motion: [f32; 4],
    more: [f32; 4],
    spout: [f32; 4],
}

/// Matches `struct Surface` in fluid_surface.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
struct SurfaceUniform {
    view: [f32; 4],
    color: [f32; 4],
    look: [f32; 4],
    blur: [f32; 4],
}

/// Where the liquid starts and how closely its particles sit.
#[derive(Clone, Debug, PartialEq)]
pub struct Start {
    pub positions: Vec<Vec2>,
    pub spacing: f32,
    /// Where poured liquid comes from, and how wide the stream is.
    pub spout: Vec2,
    pub spout_width: f32,
}

/// Fast wall distances on the CPU, using the same tiles as the GPU bake.
struct WallLookup<'a> {
    walls: &'a Walls,
    layout: Layout,
    starts: Vec<u32>,
    lists: Vec<u32>,
    tiles_x: u32,
}

impl<'a> WallLookup<'a> {
    fn new(walls: &'a Walls, layout: Layout) -> Self {
        let (starts, lists) = sdf::bin_pieces(&layout, &walls.pieces);
        Self { walls, layout, starts, lists, tiles_x: layout.width.div_ceil(8) }
    }

    fn distance(&self, p: Vec2) -> f32 {
        let tile_world = 8.0 * self.layout.texel;
        let t = ((p - self.layout.origin) / tile_world).floor();
        let mut d = sdf::MAX_DISTANCE;
        let tiles_y = (self.starts.len() as u32 - 1) / self.tiles_x;
        if t.x >= 0.0 && t.y >= 0.0 && (t.x as u32) < self.tiles_x && (t.y as u32) < tiles_y {
            let tile = t.y as usize * self.tiles_x as usize + t.x as usize;
            for &k in &self.lists[self.starts[tile] as usize..self.starts[tile + 1] as usize] {
                d = d.min(self.walls.pieces[k as usize].distance(p));
            }
        }
        if let Some(outline) = &self.walls.closed {
            d = d.min(sdf::closed_distance(outline, p));
        }
        d
    }
}

/// Lays the liquid out from the bottom of the arena up, filling `fill` of its open space.
pub fn start_positions(scene: &Scene, walls: &Walls) -> Start {
    let fluid = &scene.gpu.fluid;
    let count = fluid.particles() as usize;
    let (w, h) = (scene.video.width.max(1) as f32, scene.video.height.max(1) as f32);
    let half = Vec2::new(w, h) * 0.5;
    let lookup = WallLookup::new(walls, Layout::for_video(scene.video.width, scene.video.height));
    // How much open space there is, measured on a coarse grid.
    let probe = 6.0;
    let mut open = 0usize;
    let mut top = f32::INFINITY;
    let center = scene.container.center();
    let mut y = -half.y + probe * 0.5;
    while y < half.y {
        let mut x = -half.x + probe * 0.5;
        while x < half.x {
            if lookup.distance(Vec2::new(x, y)) > 0.0 {
                open += 1;
                if (x - center.x).abs() < probe {
                    top = top.min(y);
                }
            }
            x += probe;
        }
        y += probe;
    }
    let area = (open as f32 * probe * probe).max(1.0) * fluid.fill.clamp(0.0, 1.0);
    let spacing = (area / count.max(1) as f32).sqrt().max(MIN_SPACING);
    let margin = spacing * 0.5;
    let mut positions = Vec::with_capacity(count);
    let mut row = 0u32;
    let mut y = half.y - margin;
    'rows: while y > -half.y && positions.len() < count {
        // Every other row shifts by half a spacing, so particles don't stack in columns.
        let shift = if row % 2 == 1 { spacing * 0.5 } else { 0.0 };
        let mut x = -half.x + margin + shift;
        while x < half.x - margin {
            let p = Vec2::new(x, y);
            if lookup.distance(p) > margin {
                positions.push(p);
                if positions.len() == count {
                    break 'rows;
                }
            }
            x += spacing;
        }
        y -= spacing;
        row += 1;
    }
    let top = if top.is_finite() { top } else { -half.y };
    // Pour from a quarter of the way down, so the stream doesn't hit too hard.
    let floor = positions.first().map_or(half.y, |p| p.y);
    let spout = Vec2::new(center.x, (top + (floor - top) * 0.25).min(half.y - margin));
    Start { positions, spacing, spout, spout_width: (spacing * 10.0).max(16.0) }
}

/// Density of a particle in a resting lattice of this spacing, as the shader sums it.
pub fn rest_density(spacing: f32, reach: f32) -> f32 {
    let n = (reach / spacing).ceil() as i32 + 1;
    let mut rho = 0.0;
    for row in -n..=n {
        let shift = if row.rem_euclid(2) == 1 { 0.5 } else { 0.0 };
        for column in -n - 1..=n + 1 {
            let d = Vec2::new((column as f32 + shift) * spacing, row as f32 * spacing);
            let q = 1.0 - d.length() / reach;
            if q > 0.0 {
                rho += q * q;
            }
        }
    }
    rho
}

/// A copy of the liquid at one step.
pub struct Snapshot {
    state: wgpu::Buffer,
}

struct Targets {
    size: (u32, u32),
    behind: wgpu::Texture,
    groups: [wgpu::BindGroup; 4],
    thick_a: wgpu::TextureView,
    thick_b: wgpu::TextureView,
}

pub struct FluidSim {
    layout: wgpu::BindGroupLayout,
    extra_layout: wgpu::BindGroupLayout,
    passes: [wgpu::ComputePipeline; 10],
    params: wgpu::Buffer,
    state: Option<wgpu::Buffer>,
    buffers: Vec<wgpu::Buffer>,
    group: Option<wgpu::BindGroup>,
    extra: Option<wgpu::BindGroup>,
    count: u32,
    cells: u32,
    blocks: u32,
    params_now: Option<FluidParams>,
    surface_layout: wgpu::BindGroupLayout,
    splat: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    surface: wgpu::Buffer,
    surface_v: wgpu::Buffer,
    sampler: wgpu::Sampler,
    targets: Option<Targets>,
    surface_now: Option<SurfaceUniform>,
    spacing: f32,
    /// The step reached, for hiding liquid still waiting to pour.
    step: u64,
    hdr_format: wgpu::TextureFormat,
}

const PASSES: [&str; 10] = [
    "predict",
    "clear_cells",
    "count_cells",
    "scan_blocks",
    "scan_sums",
    "scan_finish",
    "scatter",
    "density",
    "relax",
    "finish",
];

impl FluidSim {
    pub fn new(device: &wgpu::Device, inputs_layout: &wgpu::BindGroupLayout, hdr_format: wgpu::TextureFormat) -> Self {
        use wgpu::ShaderStages as S;
        let mut entries = vec![util::uniform_entry(0, S::COMPUTE, false)];
        entries.extend((1..=7).map(|b| util::storage_entry(b, S::COMPUTE, false)));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("fluid layout"), entries: &entries });
        let extra_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fluid extra layout"),
            entries: &[
                util::storage_entry(0, S::COMPUTE, false),
                util::float_texture_entry(1, S::COMPUTE, false),
                util::uniform_entry(2, S::COMPUTE, false),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fluid"),
            bind_group_layouts: &[Some(inputs_layout), Some(&layout), Some(&extra_layout)],
            immediate_size: 0,
        });
        let module =
            util::shader(device, "fluid", &[util::COMMON_WGSL, super::STEP_WGSL, include_str!("../shaders/gpu/fluid.wgsl")]);
        let passes = PASSES.map(|entry| util::compute_pipeline(device, entry, &pipeline_layout, &module, entry));

        let surface_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fluid surface layout"),
            entries: &[
                util::uniform_entry(0, S::VERTEX_FRAGMENT, false),
                util::storage_entry(1, S::VERTEX, true),
                util::float_texture_entry(2, S::FRAGMENT, true),
                util::float_texture_entry(3, S::FRAGMENT, true),
                util::sampler_entry(4, S::FRAGMENT),
            ],
        });
        let surface_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fluid surface"),
            bind_group_layouts: &[Some(&surface_layout)],
            immediate_size: 0,
        });
        let surface_module = util::shader(device, "fluid surface", &[include_str!("../shaders/gpu/fluid_surface.wgsl")]);
        let make = |label, vs, fs, format, blend, topology| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&surface_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &surface_module,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState { topology, ..Default::default() },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &surface_module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let add = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::OVER,
        };
        use wgpu::PrimitiveTopology::{TriangleList, TriangleStrip};
        let thick = wgpu::TextureFormat::R16Float;
        let uniform = |label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: std::mem::size_of::<SurfaceUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        Self {
            layout,
            extra_layout,
            passes,
            params: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("fluid params"),
                size: std::mem::size_of::<FluidParams>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            state: None,
            buffers: Vec::new(),
            group: None,
            extra: None,
            count: 0,
            cells: 0,
            blocks: 0,
            params_now: None,
            splat: make("fluid splat", "vs_splat", "fs_splat", thick, Some(add), TriangleStrip),
            blur: make("fluid blur", "vs_full", "fs_blur", thick, None, TriangleList),
            composite: make("fluid composite", "vs_full", "fs_composite", hdr_format, None, TriangleList),
            surface_layout,
            surface: uniform("fluid surface"),
            surface_v: uniform("fluid surface blur"),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("fluid linear"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            targets: None,
            surface_now: None,
            spacing: 1.0,
            step: 0,
            hdr_format,
        }
    }

    pub fn state_bytes(&self) -> u64 {
        self.count as u64 * 16
    }

    pub fn forget_bindings(&mut self) {
        self.extra = None;
    }

    /// Lays the liquid out afresh for the scene.
    #[allow(clippy::too_many_arguments)]
    pub fn reset(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _encoder: &mut wgpu::CommandEncoder,
        _inputs: &wgpu::BindGroup,
        scene: &Scene,
        walls: &Walls,
        sdf: &SdfField,
    ) {
        let fluid = &scene.gpu.fluid;
        let start = start_positions(scene, walls);
        let count = start.positions.len() as u32;
        let spacing = start.spacing;
        let reach = spacing * 2.0;
        let (w, h) = (scene.video.width.max(1) as f32, scene.video.height.max(1) as f32);
        let mut cell = reach;
        while ((w / cell).ceil() * (h / cell).ceil()) as u32 > MAX_CELLS {
            cell *= 1.1;
        }
        let (cx, cy) = ((w / cell).ceil() as u32, (h / cell).ceil() as u32);
        let cells = cx * cy;
        let blocks = (cells + 1).div_ceil(1024);
        let pour_steps = fluid.pour_seconds.max(0.0) * super::STEP_RATE as f32;
        let stiffness = 0.05 + 0.3 * fluid.stiffness.clamp(0.0, 2.0);
        let params = FluidParams {
            counts: [count, cx, cy, blocks],
            shape: [spacing, reach, rest_density(spacing, reach), cell],
            motion: [stiffness, stiffness * 0.1, fluid.viscosity.clamp(0.0, 1.0) * 0.5, fluid.ball_push.max(0.0)],
            more: [fluid.gravity, pour_steps, SUBSTEPS as f32, 1.0 / (super::STEP_RATE * SUBSTEPS) as f32],
            spout: [start.spout.x, start.spout.y, start.spout_width, 0.0],
        };
        queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&params));
        self.params_now = Some(params);
        self.spacing = spacing;
        self.step = 0;

        if count != self.count || cells != self.cells || self.state.is_none() {
            let storage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC;
            let make = |label, size: u64| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(label),
                    size: size.max(16),
                    usage: storage,
                    mapped_at_creation: false,
                })
            };
            let n = count as u64;
            self.state = Some(make("fluid state", n * 16));
            self.buffers = vec![
                make("fluid predicted", n * 16),
                make("fluid relaxed", n * 16),
                make("fluid pressure", n * 8),
                make("fluid cell counts", (cells as u64 + 2) * 4),
                make("fluid cell starts", (cells as u64 + 2) * 4),
                make("fluid sorted", n * 4),
                make("fluid block sums", 1024 * 4),
            ];
            self.count = count;
            self.cells = cells;
            self.group = None;
            self.extra = None;
            self.targets = None;
        }
        self.blocks = blocks;
        // Particles that pour in wait at the spout until their turn.
        let poured = pour_steps > 0.0;
        let initial: Vec<[f32; 4]> = start
            .positions
            .iter()
            .enumerate()
            .map(|(i, p)| {
                if poured {
                    let jitter = bpe_scene::particle_hash(i as u32, 3) - 0.5;
                    let lift = bpe_scene::particle_hash(i as u32, 5) * spacing * 3.0;
                    [start.spout.x + jitter * start.spout_width, start.spout.y - lift, 0.0, 150.0]
                } else {
                    [p.x, p.y, 0.0, 0.0]
                }
            })
            .collect();
        if let Some(state) = &self.state {
            queue.write_buffer(state, 0, bytemuck::cast_slice(&initial));
        }
        self.ensure_groups(device, sdf);
    }

    fn ensure_groups(&mut self, device: &wgpu::Device, sdf: &SdfField) {
        if self.group.is_none() {
            let mut entries = vec![
                wgpu::BindGroupEntry { binding: 0, resource: self.params.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: self.state.as_ref().unwrap().as_entire_binding() },
            ];
            entries.extend(
                self.buffers[..6]
                    .iter()
                    .enumerate()
                    .map(|(k, b)| wgpu::BindGroupEntry { binding: k as u32 + 2, resource: b.as_entire_binding() }),
            );
            self.group =
                Some(device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("fluid"), layout: &self.layout, entries: &entries }));
        }
        if self.extra.is_none() {
            self.extra = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("fluid extra"),
                layout: &self.extra_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.buffers[6].as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&sdf.view) },
                    wgpu::BindGroupEntry { binding: 2, resource: sdf.info.as_entire_binding() },
                ],
            }));
        }
    }

    pub fn step(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        inputs: &wgpu::BindGroup,
        offset: u32,
        sdf: &SdfField,
        step: u64,
    ) {
        self.step = step;
        if self.count == 0 {
            return;
        }
        self.ensure_groups(device, sdf);
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("fluid step"), timestamp_writes: None });
        pass.set_bind_group(0, inputs, &[offset]);
        pass.set_bind_group(1, self.group.as_ref().unwrap(), &[]);
        pass.set_bind_group(2, self.extra.as_ref().unwrap(), &[]);
        let particles = groups(self.count, 256);
        let cells = groups(self.cells + 1, 256);
        for _ in 0..SUBSTEPS {
            for (pipeline, name) in self.passes.iter().zip(PASSES) {
                pass.set_pipeline(pipeline);
                match name {
                    "clear_cells" | "scan_finish" => pass.dispatch_workgroups(cells, 1, 1),
                    "scan_blocks" => pass.dispatch_workgroups(self.blocks, 1, 1),
                    "scan_sums" => pass.dispatch_workgroups(1, 1, 1),
                    _ => pass.dispatch_workgroups(particles, 1, 1),
                }
            }
        }
    }

    /// Writes the surface settings for this frame. Looks can change without restarting.
    pub fn prepare_draw(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, scene: &Scene, size: (u32, u32)) {
        if self.state.is_some() {
            self.ensure_targets(device, size);
        }
        let fluid = &scene.gpu.fluid;
        let (w, h) = (scene.video.width.max(1) as f32, scene.video.height.max(1) as f32);
        let radius = self.spacing * 1.8 + 1.0;
        let strength = self.spacing * self.spacing * 0.866 / (0.7134 * radius * radius);
        let world_per_texel = self.targets.as_ref().map_or(2.0, |t| w / t.size.0.max(1) as f32);
        let c = fluid.color.to_linear();
        let uniform = SurfaceUniform {
            view: [2.0 / w, 2.0 / h, radius, strength],
            color: [c[0], c[1], c[2], fluid.clarity.clamp(0.0, 1.0)],
            look: [
                fluid.refraction.max(0.0),
                fluid.highlight.max(0.0),
                (fluid.smoothing.max(0.0) / world_per_texel * 0.5).min(16.0),
                self.count.max(1) as f32,
            ],
            blur: [1.0, 0.0, self.params_now.map_or(0.0, |p| p.more[1]), self.step as f32],
        };
        if self.surface_now != Some(uniform) {
            queue.write_buffer(&self.surface, 0, bytemuck::bytes_of(&uniform));
            let vertical = SurfaceUniform { blur: [0.0, 1.0, uniform.blur[2], uniform.blur[3]], ..uniform };
            queue.write_buffer(&self.surface_v, 0, bytemuck::bytes_of(&vertical));
            self.surface_now = Some(uniform);
        }
    }

    fn ensure_targets(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let texture = |label, w: u32, h: u32, format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let draw = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let (tw, th) = (size.0.div_ceil(2), size.1.div_ceil(2));
        let thick_a = texture("fluid thickness", tw, th, wgpu::TextureFormat::R16Float, draw).create_view(&Default::default());
        let thick_b = texture("fluid thickness blur", tw, th, wgpu::TextureFormat::R16Float, draw).create_view(&Default::default());
        let behind = texture(
            "behind the liquid",
            size.0,
            size.1,
            self.hdr_format,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        );
        let behind_view = behind.create_view(&Default::default());
        let group = |uniform: &wgpu::Buffer, source: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("fluid surface"),
                layout: &self.surface_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: self.state.as_ref().unwrap().as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(source) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&behind_view) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            })
        };
        let groups = [
            group(&self.surface, &thick_b),
            group(&self.surface, &thick_a),
            group(&self.surface_v, &thick_b),
            group(&self.surface, &thick_a),
        ];
        self.targets = Some(Targets { size, behind, groups, thick_a, thick_b });
        self.surface_now = None;
    }

    pub fn draw_surface(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        hdr_texture: &wgpu::Texture,
        hdr: &wgpu::TextureView,
    ) {
        if self.count == 0 || self.state.is_none() {
            return;
        }
        self.ensure_targets(device, (hdr_texture.width(), hdr_texture.height()));
        let t = self.targets.as_ref().unwrap();
        encoder.copy_texture_to_texture(hdr_texture.as_image_copy(), t.behind.as_image_copy(), hdr_texture.size());
        let clear = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
        let pass = crate::begin_pass;
        {
            let mut p = pass(encoder, &t.thick_a, "fluid splat", clear);
            p.set_pipeline(&self.splat);
            p.set_bind_group(0, &t.groups[0], &[]);
            p.draw(0..4, 0..self.count);
        }
        {
            let mut p = pass(encoder, &t.thick_b, "fluid blur across", clear);
            p.set_pipeline(&self.blur);
            p.set_bind_group(0, &t.groups[1], &[]);
            p.draw(0..3, 0..1);
        }
        {
            let mut p = pass(encoder, &t.thick_a, "fluid blur down", clear);
            p.set_pipeline(&self.blur);
            p.set_bind_group(0, &t.groups[2], &[]);
            p.draw(0..3, 0..1);
        }
        {
            let mut p = pass(encoder, hdr, "fluid surface", wgpu::LoadOp::Load);
            p.set_pipeline(&self.composite);
            p.set_bind_group(0, &t.groups[3], &[]);
            p.draw(0..3, 0..1);
        }
    }

    pub fn snapshot(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder) -> Snapshot {
        let source = self.state.as_ref().unwrap();
        let state = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fluid checkpoint"),
            size: source.size(),
            usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(source, 0, &state, 0, source.size());
        Snapshot { state }
    }

    pub fn restore(&mut self, encoder: &mut wgpu::CommandEncoder, snapshot: &Snapshot, step: u64) {
        let state = self.state.as_ref().unwrap();
        if snapshot.state.size() == state.size() {
            encoder.copy_buffer_to_buffer(&snapshot.state, 0, state, 0, state.size());
        }
        self.step = step;
    }

    /// Reads the particles back to the CPU, for tests.
    #[cfg(test)]
    pub fn read_state(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<[f32; 4]> {
        let source = self.state.as_ref().unwrap();
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fluid readback"),
            size: source.size(),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(source, 0, &buffer, 0, source.size());
        queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let data = buffer.slice(..).get_mapped_range().unwrap();
        bytemuck::cast_slice::<u8, [f32; 4]>(&data)[..self.count as usize].to_vec()
    }

    #[cfg(test)]
    pub fn spacing(&self) -> f32 {
        self.spacing
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_scene::GpuFluid;
    use bpe_sim::Simulation;

    fn scene_with(count: u32, fill: f32) -> Scene {
        let mut scene = Scene::default();
        scene.gpu.fluid = GpuFluid { enabled: true, count, fill, ..GpuFluid::default() };
        scene
    }

    #[test]
    fn liquid_starts_inside_the_arena_from_the_bottom() {
        let scene = scene_with(5000, 0.3);
        let walls = Walls::of(&Simulation::new(&scene));
        let start = start_positions(&scene, &walls);
        assert_eq!(start.positions.len(), 5000);
        for p in &start.positions {
            assert!(p.length() < 508.0, "{p} is outside the ring");
            assert!(walls.distance(*p) > 0.0);
        }
        // Lower half of the ring.
        let mean_y = start.positions.iter().map(|p| p.y).sum::<f32>() / 5000.0;
        assert!(mean_y > 150.0, "{mean_y}");
        assert!(start.spout.y < 0.0, "the spout is near the top");
    }

    #[test]
    fn more_fill_means_wider_spacing() {
        let walls = Walls::of(&Simulation::new(&Scene::default()));
        let a = start_positions(&scene_with(10_000, 0.2), &walls).spacing;
        let b = start_positions(&scene_with(10_000, 0.6), &walls).spacing;
        assert!(b > a * 1.5, "{a} {b}");
    }

    #[test]
    fn rest_density_is_sensible() {
        let rho = rest_density(2.0, 4.0);
        assert!(rho > 2.0 && rho < 6.0, "{rho}");
        // It only depends on how many spacings the reach covers.
        assert!((rest_density(1.0, 2.0) - rho).abs() < 1e-4);
    }

    #[test]
    fn an_arena_too_small_keeps_what_fits() {
        let mut scene = scene_with(300_000, 1.0);
        scene.container = bpe_scene::Container::Circle {
            center: Vec2::ZERO,
            radius: 20.0,
            thickness: 3.0,
            color: bpe_scene::Color::WHITE,
        };
        let walls = Walls::of(&Simulation::new(&scene));
        let start = start_positions(&scene, &walls);
        assert!(start.positions.len() < 300_000);
        assert!(start.spacing >= MIN_SPACING);
        assert!(start.positions.iter().all(|p| p.length() < 20.0));
    }
}

//! The plain drawing on the card (`shaders/plain.wgsl`; the plan
//! `docs/superpowers/plans/water-mode/core-c3-plain-drawing.md`, "Decisions"): a flat back, the rock
//! from the solid field with a smoothed edge and a darker rim, and the liquid as a smooth body with a
//! crisp outline, going from its Colour to its Deep colour with depth over its Clarity, a thin lighter
//! rim along its surface, and round droplets. Drawn straight from a body's drops, never read back,
//! in the drops' order, so the same drops give the same picture to the bit.
//!
//! [`PlainGpu`] holds the pipelines (one a device); a [`Plain`] holds the course's solid field and
//! depth ladder (over the whole course, so depth doesn't change with the view) and one picture
//! size's textures. Part 2b draws the editor's water with it, and part 6's looks build on its amount
//! pictures.

use std::sync::Arc;

use bpe_core::maths::Maths;
use bytemuck::{Pod, Zeroable};
use glam::Vec2;

use crate::drops::DropBuffers;
use crate::drops::numbers::MOST_LIQUIDS;
use crate::solid::{Layout, SolidField};

/// A splat's radius, in cells (drops at rest are half a cell apart).
pub const RADIUS: f32 = 0.9;
/// The level the summed splats are cut at (a body of liquid at rest sums to 1).
pub const LEVEL: f32 = 0.2;
/// The least splat radius, picture px: a lone drop shows at any zoom.
pub const LEAST_R: f32 = 1.25;
/// A splat made wider than its own size peaks at least this many times [`LEVEL`], so a lone drop
/// still crosses the level within 0.75 px of its middle (some pixel's middle is always that near).
pub const LONE_PEAK: f32 = 2.5;
/// The liquid's rim, world px (and never under [`LEAST_RIM`] picture px).
pub const RIM: f32 = 3.0;
pub const LEAST_RIM: f32 = 1.25;
/// The rock's darker rim, world px (and never under [`LEAST_ROCK_RIM`] picture px).
pub const ROCK_RIM: f32 = 3.0;
pub const LEAST_ROCK_RIM: f32 = 1.5;
/// The dither's size in output steps (a fixed 4 x 4 pattern under half a step either way).
const DITHER: f32 = 1.0 / 255.0;

const AMOUNT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const LADDER: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;
/// The splats summed at the layout's nodes.
const NODES: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;
/// The picture's format: 8 bits a channel holding sRGB (the sRGB view may be used to sample it).
pub const OUTPUT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// What part of the world a picture shows: the world point at its top left and picture pixels a
/// world pixel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub lo: Vec2,
    pub scale: f32,
    pub width: u32,
    pub height: u32,
}

impl View {
    /// The 1080 x 1920 screen round the world's middle, at `scale` picture pixels a world pixel.
    pub fn screen(scale: f32) -> View {
        View {
            lo: Vec2::new(-540.0, -960.0),
            scale,
            width: (1080.0 * scale).round().max(1.0) as u32,
            height: (1920.0 * scale).round().max(1.0) as u32,
        }
    }
}

/// One liquid's look: sRGB colours from 0 to 1, Clarity in world pixels (0: no depth).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlainLiquid {
    pub colour: [f32; 3],
    pub deep: [f32; 3],
    pub clarity: f32,
}

impl Default for PlainLiquid {
    /// The Water Look card's defaults (`#2f9bff`, Deep colour `#0b2a4f`, Clarity 200 px).
    fn default() -> Self {
        let c = |r: u8, g: u8, b: u8| [f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0];
        PlainLiquid { colour: c(0x2f, 0x9b, 0xff), deep: c(0x0b, 0x2a, 0x4f), clarity: 200.0 }
    }
}

/// What the plain drawing is told by the cards: the back's colour, the rock's, each liquid's look by
/// its kind (a kind past the list takes the list's last; none: [`PlainLiquid::default`]), and each
/// solid's colour by its owner in the field ([`SolidField::owner`]: owner `k` from 1 is
/// `solids[k - 1]`; owner 0, and one past the list or past [`MOST_COLOURED`], is the rock's).
#[derive(Clone, Debug, PartialEq)]
pub struct PlainLook {
    pub back: [f32; 3],
    pub rock: [f32; 3],
    pub liquids: Vec<PlainLiquid>,
    pub solids: Vec<[f32; 3]>,
}

/// The most pieces and walls drawn in their own colours (the rest in the rock's).
pub const MOST_COLOURED: usize = 64;

impl Default for PlainLook {
    /// The Rock card's colour (`#6b5e52`) and the Back card's default, the rock half as bright
    /// (Shows: Darker rock, Darker 0.5).
    fn default() -> Self {
        let rock = [f32::from(0x6bu8) / 255.0, f32::from(0x5eu8) / 255.0, f32::from(0x52u8) / 255.0];
        PlainLook { back: rock.map(|c| c * 0.5), rock, liquids: Vec::new(), solids: Vec::new() }
    }
}

fn linear(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.040_45 { c / 12.92 } else { ((c + 0.055) / 1.055).det_powf(2.4) }
}

pub(crate) fn linear3(c: [f32; 3]) -> [f32; 3] {
    c.map(linear)
}

/// The drawing's numbers on the card. Matches `U` in plain.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct PlainU {
    view: [f32; 4],
    size: [u32; 4],
    layout: [f32; 4],
    splat: [f32; 4],
    widths: [f32; 4],
    back: [f32; 4],
    rock: [f32; 4],
    depth: [f32; 4],
    liquids: [[f32; 8]; MOST_LIQUIDS],
    solids: [[f32; 4]; MOST_COLOURED],
}

/// A splat's peak weight at its own size: a body at rest (four drops a cell) sums to 1.
fn body_peak() -> f32 {
    3.0 / (4.0 * std::f32::consts::PI * RADIUS * RADIUS)
}

/// The splat's radius (picture px) and peak weight at `scale` picture px a world px, for `cell`-px
/// cells: a body at rest sums to 1; a splat widened to [`LEAST_R`] is made fainter to keep that, but
/// never so faint a lone drop falls under the level.
pub fn splat_for(cell: f32, scale: f32) -> (f32, f32) {
    let own = RADIUS * cell * scale;
    let r = own.max(LEAST_R);
    let peak = body_peak();
    if r > own {
        let share = (own / r) * (own / r);
        (r, (peak * share).max(LONE_PEAK * LEVEL))
    } else {
        (r, peak)
    }
}

/// The depth ladder of a layout `cols` x `rows` nodes: its first level's size (a texel a node,
/// padded past the layout to a whole number of top texels, so every halving is exact) and how many
/// levels it has (until a top texel spans the course's narrower side), within `most` texels a side.
fn ladder_for(cols: u32, rows: u32, most: u32) -> (u32, u32, u32) {
    let narrow = cols.min(rows).max(1);
    let mut levels = 32 - (narrow - 1).leading_zeros() + 1;
    loop {
        let top = 1u32 << (levels - 1);
        let (w, h) = (cols.max(1).div_ceil(top) * top, rows.max(1).div_ceil(top) * top);
        if (w <= most && h <= most) || levels == 1 {
            if w > most || h > most {
                eprintln!("water plain: a course {cols} x {rows} nodes is past the card's {most} texels a side");
            }
            return (w.min(most), h.min(most), levels);
        }
        levels -= 1;
    }
}

/// The plain drawing's pipelines on one device.
pub struct PlainGpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    splat: (wgpu::RenderPipeline, wgpu::BindGroupLayout),
    world: (wgpu::RenderPipeline, wgpu::BindGroupLayout),
    base: (wgpu::RenderPipeline, wgpu::BindGroupLayout),
    down: (wgpu::RenderPipeline, wgpu::BindGroupLayout),
    show: (wgpu::RenderPipeline, wgpu::BindGroupLayout),
}

impl std::fmt::Debug for PlainGpu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PlainGpu")
    }
}

#[derive(Clone, Copy)]
enum B {
    Uniform,
    Storage,
    /// A float texture, read texel by texel.
    Texture,
}

impl PlainGpu {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Arc<PlainGpu> {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("water plain"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/plain.wgsl").into()),
        });
        let stages = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let layout = |name: &str, bindings: &[(u32, B)]| {
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
        let pipeline = |name: &str,
                        group: wgpu::BindGroupLayout,
                        vs: &str,
                        fs: &str,
                        targets: &[Option<wgpu::ColorTargetState>]| {
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(name),
                bind_group_layouts: &[Some(&group)],
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
            (pipeline, group)
        };
        let add = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let summed_as = |format| {
            Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState { color: add, alpha: add }),
                write_mask: wgpu::ColorWrites::ALL,
            })
        };
        let summed = summed_as(AMOUNT);
        let plain = |format| Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL });
        let splat = pipeline(
            "water plain splats",
            layout("water plain splats", &[(0, B::Uniform), (1, B::Storage), (2, B::Storage)]),
            "vs_splat",
            "fs_splat",
            &[summed.clone(), summed],
        );
        let world = pipeline(
            "water plain nodes",
            layout("water plain nodes", &[(0, B::Uniform), (1, B::Storage), (2, B::Storage)]),
            "vs_world",
            "fs_world",
            &[summed_as(NODES)],
        );
        let base = pipeline(
            "water plain ladder base",
            layout("water plain ladder base", &[(0, B::Uniform), (8, B::Texture), (5, B::Storage)]),
            "vs_full",
            "fs_base",
            &[plain(LADDER)],
        );
        let down = pipeline(
            "water plain ladder down",
            layout("water plain ladder down", &[(6, B::Texture)]),
            "vs_full",
            "fs_down",
            &[plain(LADDER)],
        );
        let show = pipeline(
            "water plain show",
            layout(
                "water plain show",
                &[(0, B::Uniform), (3, B::Texture), (4, B::Texture), (5, B::Storage), (6, B::Texture)],
            ),
            "vs_full",
            "fs_show",
            &[plain(OUTPUT)],
        );
        Arc::new(PlainGpu { device: device.clone(), queue: queue.clone(), splat, world, base, down, show })
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
}

/// A picture drawn afresh (cleared first).
pub(crate) fn attach(view: &wgpu::TextureView) -> Option<wgpu::RenderPassColorAttachment<'_>> {
    Some(wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
    })
}

/// One render pass drawing `vertices` x `instances` with `pipeline` (nothing drawn with no instances).
pub(crate) fn run_pass(
    encoder: &mut wgpu::CommandEncoder,
    label: &str,
    targets: &[Option<wgpu::RenderPassColorAttachment>],
    pipeline: &wgpu::RenderPipeline,
    group: &wgpu::BindGroup,
    vertices: u32,
    instances: u32,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: targets,
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    if instances > 0 {
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..vertices, 0..instances);
    }
}

/// One picture size's textures.
struct Pictures {
    width: u32,
    height: u32,
    amount: [wgpu::TextureView; 2],
    output: wgpu::Texture,
    output_view: wgpu::TextureView,
}

/// The course's depth ladder: the splats summed at its nodes, and the ladder of the air's share.
pub(crate) struct Depth {
    width: u32,
    height: u32,
    levels: u32,
    nodes: wgpu::TextureView,
    /// Each level alone (to draw into, and to read the one before), and all of them.
    ladder_levels: Vec<wgpu::TextureView>,
    pub(crate) ladder_all: wgpu::TextureView,
    /// `down[k]` reads level k (and draws level k + 1).
    down: Vec<wgpu::BindGroup>,
}

pub(crate) fn texture(
    device: &wgpu::Device,
    label: &str,
    (width, height): (u32, u32),
    format: wgpu::TextureFormat,
    mips: u32,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

impl Depth {
    fn new(gpu: &PlainGpu, layout: &Layout) -> Depth {
        let device = &gpu.device;
        let most = device.limits().max_texture_dimension_2d;
        let nodes_size = (layout.cols.clamp(1, most), layout.rows.clamp(1, most));
        let nodes = texture(device, "water plain nodes", nodes_size, NODES, 1).create_view(&Default::default());
        let (width, height, levels) = ladder_for(layout.cols, layout.rows, most);
        let ladder = texture(device, "water plain ladder", (width, height), LADDER, levels);
        let ladder_levels: Vec<wgpu::TextureView> = (0..levels)
            .map(|k| {
                ladder.create_view(&wgpu::TextureViewDescriptor {
                    base_mip_level: k,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let ladder_all = ladder.create_view(&Default::default());
        let down = ladder_levels[..levels.saturating_sub(1) as usize]
            .iter()
            .map(|view| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("water plain ladder down"),
                    layout: &gpu.down.1,
                    entries: &[wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(view) }],
                })
            })
            .collect();
        Depth { width, height, levels, nodes, ladder_levels, ladder_all, down }
    }
}

/// A plain drawing of one body's drops on one course.
pub struct Plain {
    gpu: Arc<PlainGpu>,
    pub(crate) layout: Layout,
    rest: f32,
    pub(crate) field: wgpu::Buffer,
    pub(crate) depth: Depth,
    pub(crate) uniform: wgpu::Buffer,
    pictures: Option<Pictures>,
    /// The last picture size told to be past the card's most (told once).
    told: Option<(u32, u32)>,
    /// Seconds every drop is moved on by its speed in the next picture ([`Plain::draw_at`]).
    pub(crate) ahead: f32,
}

impl std::fmt::Debug for Plain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let size = self.pictures.as_ref().map(|p| (p.width, p.height));
        write!(f, "Plain({} x {} nodes, picture {size:?})", self.layout.cols, self.layout.rows)
    }
}

/// The field as the drawing reads it: each node's distance (world px), then each node's owner.
fn field_buffer(device: &wgpu::Device, queue: &wgpu::Queue, field: &SolidField) -> wgpu::Buffer {
    let far = 64.0 * field.layout.cell;
    let mut distances: Vec<f32> = field.distance.iter().map(|d| d.clamp(-far, far)).collect();
    let nodes = distances.len();
    distances.extend((0..nodes).map(|i| f32::from(field.owner.get(i).copied().unwrap_or(0))));
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("water plain field"),
        size: (distances.len() as u64 * 4).max(16),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buffer, 0, bytemuck::cast_slice(&distances));
    buffer
}

impl Plain {
    /// A drawing of drops laid out on `field`'s layout (a body's drops are in its grid units), with
    /// its solids. `rest`: how far off a face the drops rest, in cells (the engine's
    /// `Numbers::wall_off`), so liquid resting on a face is drawn up to it.
    pub fn new(gpu: Arc<PlainGpu>, field: &SolidField, rest: f32) -> Plain {
        let field_buf = field_buffer(&gpu.device, &gpu.queue, field);
        let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("water plain numbers"),
            size: std::mem::size_of::<PlainU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let depth = Depth::new(&gpu, &field.layout);
        Plain {
            gpu,
            layout: field.layout,
            rest,
            field: field_buf,
            depth,
            uniform,
            pictures: None,
            told: None,
            ahead: 0.0,
        }
    }

    /// The course changed (its layout must be the drops' own).
    pub fn set_field(&mut self, field: &SolidField) {
        self.field = field_buffer(&self.gpu.device, &self.gpu.queue, field);
        if field.layout != self.layout {
            self.depth = Depth::new(&self.gpu, &field.layout);
        }
        self.layout = field.layout;
    }

    pub fn gpu(&self) -> &Arc<PlainGpu> {
        &self.gpu
    }

    /// The textures for a `width` x `height` picture (kept while the size stays).
    fn pictures_for(&mut self, width: u32, height: u32) {
        if self.pictures.as_ref().is_some_and(|p| p.width == width && p.height == height) {
            return;
        }
        let device = &self.gpu.device;
        let amount = [
            texture(device, "water plain amount", (width, height), AMOUNT, 1).create_view(&Default::default()),
            texture(device, "water plain deep", (width, height), AMOUNT, 1).create_view(&Default::default()),
        ];
        let output = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("water plain picture"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OUTPUT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[wgpu::TextureFormat::Rgba8UnormSrgb],
        });
        let output_view = output.create_view(&Default::default());
        self.pictures = Some(Pictures { width, height, amount, output, output_view });
    }

    /// The numbers for drawing `view` of this layout's drops with `look`.
    pub(crate) fn numbers(&self, view: &View, width: u32, height: u32, look: &PlainLook) -> PlainU {
        let l = &self.layout;
        let (r, peak) = splat_for(l.cell, view.scale);
        let mut liquids = [[0.0f32; 8]; MOST_LIQUIDS];
        let given: Vec<PlainLiquid> =
            if look.liquids.is_empty() { vec![PlainLiquid::default()] } else { look.liquids.clone() };
        for (k, liquid) in given.iter().take(MOST_LIQUIDS).enumerate() {
            let [r, g, b] = linear3(liquid.colour);
            let [dr, dg, db] = linear3(liquid.deep);
            liquids[k] = [r, g, b, liquid.clarity.max(0.0), dr, dg, db, 0.0];
        }
        let [br, bg, bb] = linear3(look.back);
        let [rr, rg, rb] = linear3(look.rock);
        let mut solids = [[rr, rg, rb, 1.0]; MOST_COLOURED];
        for (k, c) in look.solids.iter().take(MOST_COLOURED).enumerate() {
            let [r, g, b] = linear3(*c);
            solids[k] = [r, g, b, 1.0];
        }
        PlainU {
            view: [view.lo.x, view.lo.y, view.scale, if self.ahead.is_finite() { self.ahead } else { 0.0 }],
            size: [width, height, l.cols, l.rows],
            layout: [l.origin.x, l.origin.y, l.cell, self.rest],
            splat: [r, peak, LEVEL, given.len().min(MOST_LIQUIDS) as f32],
            widths: [
                (RIM * view.scale).max(LEAST_RIM),
                (ROCK_RIM * view.scale).max(LEAST_ROCK_RIM),
                self.depth.levels as f32,
                DITHER,
            ],
            back: [br, bg, bb, 1.0],
            rock: [rr, rg, rb, 1.0],
            depth: [RADIUS, body_peak(), self.depth.width as f32, self.depth.height as f32],
            liquids,
            solids,
        }
    }

    /// Records one drawing of `drops` (on this drawing's layout) into `encoder`.
    fn record(&self, encoder: &mut wgpu::CommandEncoder, drops: &DropBuffers) {
        let gpu = &self.gpu;
        let p = self.pictures.as_ref().expect("the pictures are made before recording");
        let entry = |binding, resource| wgpu::BindGroupEntry { binding, resource };
        let splat_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("water plain splats"),
            layout: &gpu.splat.1,
            entries: &[
                entry(0, self.uniform.as_entire_binding()),
                entry(1, drops.xv.as_entire_binding()),
                entry(2, drops.id.as_entire_binding()),
            ],
        });
        // (Every drop in the buffers' order: blending adds in that order, so the sums repeat.)
        run_pass(
            encoder,
            "water plain splats",
            &[attach(&p.amount[0]), attach(&p.amount[1])],
            &gpu.splat.0,
            &splat_group,
            6,
            drops.count,
        );
        self.record_ladder(encoder, drops);
        let d = &self.depth;
        let show_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("water plain show"),
            layout: &gpu.show.1,
            entries: &[
                entry(0, self.uniform.as_entire_binding()),
                entry(3, wgpu::BindingResource::TextureView(&p.amount[0])),
                entry(4, wgpu::BindingResource::TextureView(&p.amount[1])),
                entry(5, self.field.as_entire_binding()),
                entry(6, wgpu::BindingResource::TextureView(&d.ladder_all)),
            ],
        });
        run_pass(encoder, "water plain show", &[attach(&p.output_view)], &gpu.show.0, &show_group, 3, 1);
    }

    /// Records the depth ladder of `drops` over the whole course (the part 6 look drawing reads it as
    /// the plain drawing does), with the numbers last written.
    pub(crate) fn record_ladder(&self, encoder: &mut wgpu::CommandEncoder, drops: &DropBuffers) {
        let gpu = &self.gpu;
        let entry = |binding, resource| wgpu::BindGroupEntry { binding, resource };
        let d = &self.depth;
        let world_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("water plain nodes"),
            layout: &gpu.world.1,
            entries: &[
                entry(0, self.uniform.as_entire_binding()),
                entry(1, drops.xv.as_entire_binding()),
                entry(2, drops.id.as_entire_binding()),
            ],
        });
        let base_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("water plain ladder base"),
            layout: &gpu.base.1,
            entries: &[
                entry(0, self.uniform.as_entire_binding()),
                entry(8, wgpu::BindingResource::TextureView(&d.nodes)),
                entry(5, self.field.as_entire_binding()),
            ],
        });
        // The depth ladder, over the whole course.
        run_pass(encoder, "water plain nodes", &[attach(&d.nodes)], &gpu.world.0, &world_group, 6, drops.count);
        run_pass(encoder, "water plain ladder base", &[attach(&d.ladder_levels[0])], &gpu.base.0, &base_group, 3, 1);
        for k in 1..d.levels as usize {
            let target = [attach(&d.ladder_levels[k])];
            run_pass(encoder, "water plain ladder down", &target, &gpu.down.0, &d.down[k - 1], 3, 1);
        }
    }

    /// The picture size `view` asks for, within the card's largest texture (said once a size).
    pub(crate) fn size_of(&mut self, view: &View) -> (u32, u32) {
        let most = self.gpu.device.limits().max_texture_dimension_2d;
        let asked = (view.width, view.height);
        if (asked.0 > most || asked.1 > most) && self.told != Some(asked) {
            eprintln!(
                "water plain: a picture {} x {} is drawn within {most} a side (the card's most)",
                asked.0, asked.1
            );
            self.told = Some(asked);
        }
        (asked.0.clamp(1, most), asked.1.clamp(1, most))
    }

    /// Draws `drops` (in this drawing's layout's grid units) as `view` shows them, with `look`, and
    /// sends it to the card. The picture is [`Plain::output`] (and [`Plain::read`]).
    pub fn draw(&mut self, drops: &DropBuffers, view: &View, look: &PlainLook) {
        self.ahead = 0.0;
        self.draw_times(drops, view, look, 1);
    }

    /// The same with every drop moved on by its speed for `ahead` seconds (a final's kept moment
    /// drawn a little after it, part 8).
    pub fn draw_at(&mut self, drops: &DropBuffers, view: &View, look: &PlainLook, ahead: f32) {
        self.ahead = ahead;
        self.draw_times(drops, view, look, 1);
        self.ahead = 0.0;
    }

    fn draw_times(&mut self, drops: &DropBuffers, view: &View, look: &PlainLook, times: u32) {
        let (width, height) = self.size_of(view);
        self.pictures_for(width, height);
        let numbers = self.numbers(view, width, height, look);
        self.gpu.queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&numbers));
        let mut encoder =
            self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("water plain") });
        for _ in 0..times {
            self.record(&mut encoder, drops);
        }
        self.gpu.queue.submit([encoder.finish()]);
    }

    /// Draws `times` pictures in one submission and waits for the card: the time a picture takes,
    /// the card's own time once it is busy (for the measuring runs).
    pub fn draw_many(&mut self, drops: &DropBuffers, view: &View, look: &PlainLook, times: u32) -> std::time::Duration {
        // (Three to warm up.)
        self.draw_times(drops, view, look, 3);
        self.gpu.device.poll(wgpu::PollType::wait_indefinitely()).expect("the card");
        let at = std::time::Instant::now();
        self.draw_times(drops, view, look, times.max(1));
        self.gpu.device.poll(wgpu::PollType::wait_indefinitely()).expect("the card");
        at.elapsed() / times.max(1)
    }

    /// The last picture drawn (rgba8 holding sRGB; the `Rgba8UnormSrgb` view may sample it).
    pub fn output(&self) -> Option<&wgpu::Texture> {
        self.pictures.as_ref().map(|p| &p.output)
    }

    /// The last picture's own view (rgba8 holding sRGB), for a readback.
    pub fn output_view(&self) -> Option<&wgpu::TextureView> {
        self.pictures.as_ref().map(|p| &p.output_view)
    }

    /// The last picture drawn, read back: rgba8, sRGB, rows packed (empty before any draw).
    pub fn read(&self) -> Vec<u8> {
        let Some(p) = self.pictures.as_ref() else { return Vec::new() };
        let device = &self.gpu.device;
        let row = (p.width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("water plain read"),
            size: u64::from(row) * u64::from(p.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("water plain read") });
        encoder.copy_texture_to_buffer(
            p.output.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None },
            },
            wgpu::Extent3d { width: p.width, height: p.height, depth_or_array_layers: 1 },
        );
        self.gpu.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).expect("the card");
        let data = slice.get_mapped_range().expect("the picture is mapped");
        let mut out = Vec::with_capacity((p.width * p.height * 4) as usize);
        for y in 0..p.height as usize {
            let start = y * row as usize;
            out.extend_from_slice(&data[start..start + p.width as usize * 4]);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_splat_is_never_under_its_least_size_nor_too_faint_to_show() {
        // Full size at 2 px cells: its own size.
        let (r, peak) = splat_for(2.0, 1.0);
        assert!((r - 1.8).abs() < 1e-6 && peak > LEVEL);
        // Far out: widened, and still over the level.
        let (r, peak) = splat_for(2.0, 0.05);
        assert_eq!(r, LEAST_R);
        assert!(peak >= LONE_PEAK * LEVEL - 1e-6);
    }

    #[test]
    fn the_ladder_halves_exactly_until_a_texel_spans_the_course() {
        // A screen at 2 px cells: 541 x 961 nodes, padded to 1024 x 1024; its top level (1 << 10
        // nodes a texel) is one texel.
        assert_eq!(ladder_for(541, 961, 8192), (1024, 1024, 11));
        // A course three screens tall: the top texels span its width, three of them down it.
        assert_eq!(ladder_for(541, 2881, 8192), (1024, 3072, 11));
        // At 8 px cells.
        assert_eq!(ladder_for(136, 241, 8192), (256, 256, 9));
        assert_eq!(ladder_for(1, 1, 8192), (1, 1, 1));
        // Every level of it halves exactly.
        for (cols, rows) in [(541u32, 961u32), (541, 2881), (136, 241), (3, 7), (1000, 1)] {
            let (w, h, levels) = ladder_for(cols, rows, 8192);
            assert!(w >= cols && h >= rows);
            let top = 1 << (levels - 1);
            assert_eq!((w % top, h % top), (0, 0), "{cols} x {rows}");
        }
        // Past the card's most it keeps within it.
        let (w, h, _) = ladder_for(300, 9000, 8192);
        assert!(w <= 8192 && h <= 8192);
    }
}

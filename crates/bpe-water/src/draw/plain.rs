//! The plain drawing on the card (`shaders/plain.wgsl`; the plan
//! `docs/superpowers/plans/water-mode/core-c3-plain-drawing.md`, "Decisions"): a flat back, the rock
//! from the solid field with a smoothed edge and a darker rim, and the liquid as a smooth body with a
//! crisp outline, going from its Colour to its Deep colour with depth over its Clarity, a thin lighter
//! rim along its surface, and round droplets. Drawn straight from a body's drops, never read back,
//! in the drops' order, so the same drops give the same picture to the bit.
//!
//! [`PlainGpu`] holds the pipelines (one a device); a [`Plain`] holds one picture's textures and the
//! solid field. Part 2b draws the editor's water with it, and part 6's looks build on its amount
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
const LADDER: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;
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

/// What the plain drawing is told by the cards: the back's colour, the rock's, and each liquid's
/// look by its kind (a kind past the list takes the list's last; none: [`PlainLiquid::default`]).
#[derive(Clone, Debug, PartialEq)]
pub struct PlainLook {
    pub back: [f32; 3],
    pub rock: [f32; 3],
    pub liquids: Vec<PlainLiquid>,
}

impl Default for PlainLook {
    /// The Rock card's colour (`#6b5e52`) and the Back card's default, the rock half as bright
    /// (Shows: Darker rock, Darker 0.5).
    fn default() -> Self {
        let rock = [f32::from(0x6bu8) / 255.0, f32::from(0x5eu8) / 255.0, f32::from(0x52u8) / 255.0];
        PlainLook { back: rock.map(|c| c * 0.5), rock, liquids: Vec::new() }
    }
}

fn linear(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.040_45 { c / 12.92 } else { ((c + 0.055) / 1.055).det_powf(2.4) }
}

fn linear3(c: [f32; 3]) -> [f32; 3] {
    c.map(linear)
}

/// The drawing's numbers on the card. Matches `U` in plain.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct PlainU {
    view: [f32; 4],
    size: [u32; 4],
    layout: [f32; 4],
    splat: [f32; 4],
    widths: [f32; 4],
    back: [f32; 4],
    rock: [f32; 4],
    liquids: [[f32; 8]; MOST_LIQUIDS],
}

/// The splat's radius (picture px) and peak weight at `scale` picture px a world px, for `cell`-px
/// cells: a body at rest sums to 1; a splat widened to [`LEAST_R`] is made fainter to keep that, but
/// never so faint a lone drop falls under the level.
pub fn splat_for(cell: f32, scale: f32) -> (f32, f32) {
    let own = RADIUS * cell * scale;
    let r = own.max(LEAST_R);
    let peak = 3.0 / (4.0 * std::f32::consts::PI * RADIUS * RADIUS);
    if r > own {
        let share = (own / r) * (own / r);
        (r, (peak * share).max(LONE_PEAK * LEVEL))
    } else {
        (r, peak)
    }
}

/// How many levels the ladder of a picture this size has (its first level is half the size; each
/// next one half again, down to a texel).
fn ladder_levels(width: u32, height: u32) -> u32 {
    let side = width.div_ceil(2).max(height.div_ceil(2)).max(1);
    32 - side.leading_zeros()
}

/// The plain drawing's pipelines on one device.
pub struct PlainGpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    splat: (wgpu::RenderPipeline, wgpu::BindGroupLayout),
    base: (wgpu::RenderPipeline, wgpu::BindGroupLayout),
    down: (wgpu::RenderPipeline, wgpu::BindGroupLayout),
    show: (wgpu::RenderPipeline, wgpu::BindGroupLayout),
    sampler: wgpu::Sampler,
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
    Amount,
    Ladder,
    Sampler,
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
                        B::Amount | B::Ladder => wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: matches!(b, B::Ladder) },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        B::Sampler => wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
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
        let summed = Some(wgpu::ColorTargetState {
            format: AMOUNT,
            blend: Some(wgpu::BlendState { color: add, alpha: add }),
            write_mask: wgpu::ColorWrites::ALL,
        });
        let plain = |format| Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL });
        let splat = pipeline(
            "water plain splats",
            layout("water plain splats", &[(0, B::Uniform), (1, B::Storage), (2, B::Storage)]),
            "vs_splat",
            "fs_splat",
            &[summed.clone(), summed],
        );
        let base = pipeline(
            "water plain ladder base",
            layout("water plain ladder base", &[(0, B::Uniform), (3, B::Amount), (5, B::Storage)]),
            "vs_full",
            "fs_base",
            &[plain(LADDER)],
        );
        let down = pipeline(
            "water plain ladder down",
            layout("water plain ladder down", &[(6, B::Ladder), (7, B::Sampler)]),
            "vs_full",
            "fs_down",
            &[plain(LADDER)],
        );
        let show = pipeline(
            "water plain show",
            layout(
                "water plain show",
                &[(0, B::Uniform), (3, B::Amount), (4, B::Amount), (5, B::Storage), (6, B::Ladder), (7, B::Sampler)],
            ),
            "vs_full",
            "fs_show",
            &[plain(OUTPUT)],
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("water plain ladder"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        Arc::new(PlainGpu { device: device.clone(), queue: queue.clone(), splat, base, down, show, sampler })
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
}

/// A picture drawn afresh (cleared first).
fn attach(view: &wgpu::TextureView) -> Option<wgpu::RenderPassColorAttachment<'_>> {
    Some(wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
    })
}

/// One picture size's textures.
struct Pictures {
    width: u32,
    height: u32,
    levels: u32,
    amount: [wgpu::TextureView; 2],
    /// Each level alone (to draw into, and to read the one before), and all of them.
    ladder_levels: Vec<wgpu::TextureView>,
    ladder_all: wgpu::TextureView,
    /// `down[k]` reads level k (and draws level k + 1).
    down: Vec<wgpu::BindGroup>,
    output: wgpu::Texture,
    output_view: wgpu::TextureView,
}

/// A plain drawing of one body's drops on one course.
pub struct Plain {
    gpu: Arc<PlainGpu>,
    layout: Layout,
    rest: f32,
    field: wgpu::Buffer,
    uniform: wgpu::Buffer,
    pictures: Option<Pictures>,
}

impl std::fmt::Debug for Plain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let size = self.pictures.as_ref().map(|p| (p.width, p.height));
        write!(f, "Plain({} x {} nodes, picture {size:?})", self.layout.cols, self.layout.rows)
    }
}

fn field_buffer(device: &wgpu::Device, queue: &wgpu::Queue, field: &SolidField) -> wgpu::Buffer {
    let far = 64.0 * field.layout.cell;
    let distances: Vec<f32> = field.distance.iter().map(|d| d.clamp(-far, far)).collect();
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
        Plain { gpu, layout: field.layout, rest, field: field_buf, uniform, pictures: None }
    }

    /// The course changed (its layout must be the drops' own).
    pub fn set_field(&mut self, field: &SolidField) {
        self.field = field_buffer(&self.gpu.device, &self.gpu.queue, field);
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
        let texture = |label, w: u32, h: u32, format, mips: u32, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: mips,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let draw_read = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let amount = [
            texture("water plain amount", width, height, AMOUNT, 1, draw_read).create_view(&Default::default()),
            texture("water plain deep", width, height, AMOUNT, 1, draw_read).create_view(&Default::default()),
        ];
        let levels = ladder_levels(width, height);
        let ladder = texture("water plain ladder", width.div_ceil(2), height.div_ceil(2), LADDER, levels, draw_read);
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
                    layout: &self.gpu.down.1,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(view) },
                        wgpu::BindGroupEntry {
                            binding: 7,
                            resource: wgpu::BindingResource::Sampler(&self.gpu.sampler),
                        },
                    ],
                })
            })
            .collect();
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
        self.pictures =
            Some(Pictures { width, height, levels, amount, ladder_levels, ladder_all, down, output, output_view });
    }

    /// The numbers for drawing `view` of this layout's drops with `look`.
    fn numbers(&self, view: &View, width: u32, height: u32, levels: u32, look: &PlainLook) -> PlainU {
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
        PlainU {
            view: [view.lo.x, view.lo.y, view.scale, 0.0],
            size: [width, height, l.cols, l.rows],
            layout: [l.origin.x, l.origin.y, l.cell, self.rest],
            splat: [r, peak, LEVEL, given.len().min(MOST_LIQUIDS) as f32],
            widths: [
                (RIM * view.scale).max(LEAST_RIM),
                (ROCK_RIM * view.scale).max(LEAST_ROCK_RIM),
                levels as f32,
                DITHER,
            ],
            back: [br, bg, bb, 1.0],
            rock: [rr, rg, rb, 1.0],
            liquids,
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
        let base_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("water plain ladder base"),
            layout: &gpu.base.1,
            entries: &[
                entry(0, self.uniform.as_entire_binding()),
                entry(3, wgpu::BindingResource::TextureView(&p.amount[0])),
                entry(5, self.field.as_entire_binding()),
            ],
        });
        let show_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("water plain show"),
            layout: &gpu.show.1,
            entries: &[
                entry(0, self.uniform.as_entire_binding()),
                entry(3, wgpu::BindingResource::TextureView(&p.amount[0])),
                entry(4, wgpu::BindingResource::TextureView(&p.amount[1])),
                entry(5, self.field.as_entire_binding()),
                entry(6, wgpu::BindingResource::TextureView(&p.ladder_all)),
                entry(7, wgpu::BindingResource::Sampler(&gpu.sampler)),
            ],
        });
        let pass = |encoder: &mut wgpu::CommandEncoder,
                    label: &str,
                    targets: &[Option<wgpu::RenderPassColorAttachment>],
                    pipeline: &wgpu::RenderPipeline,
                    group: &wgpu::BindGroup,
                    vertices: u32,
                    instances: u32| {
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
        };
        // (Every drop in the buffers' order: blending adds in that order, so the sums repeat.)
        pass(
            encoder,
            "water plain splats",
            &[attach(&p.amount[0]), attach(&p.amount[1])],
            &gpu.splat.0,
            &splat_group,
            6,
            drops.count,
        );
        pass(encoder, "water plain ladder base", &[attach(&p.ladder_levels[0])], &gpu.base.0, &base_group, 3, 1);
        for k in 1..p.levels as usize {
            pass(encoder, "water plain ladder down", &[attach(&p.ladder_levels[k])], &gpu.down.0, &p.down[k - 1], 3, 1);
        }
        pass(encoder, "water plain show", &[attach(&p.output_view)], &gpu.show.0, &show_group, 3, 1);
    }

    /// The picture size `view` asks for, within the card's largest texture.
    fn size_of(&self, view: &View) -> (u32, u32) {
        let most = self.gpu.device.limits().max_texture_dimension_2d;
        let fit = |side: u32| {
            if side > most {
                eprintln!("water plain: a picture {side} pixels across is drawn {most} across (the card's most)");
            }
            side.clamp(1, most)
        };
        (fit(view.width), fit(view.height))
    }

    /// Draws `drops` (in this drawing's layout's grid units) as `view` shows them, with `look`, and
    /// sends it to the card. The picture is [`Plain::output`] (and [`Plain::read`]).
    pub fn draw(&mut self, drops: &DropBuffers, view: &View, look: &PlainLook) {
        self.draw_times(drops, view, look, 1);
    }

    fn draw_times(&mut self, drops: &DropBuffers, view: &View, look: &PlainLook, times: u32) {
        let (width, height) = self.size_of(view);
        self.pictures_for(width, height);
        let levels = self.pictures.as_ref().map_or(1, |p| p.levels);
        let numbers = self.numbers(view, width, height, levels, look);
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
        self.draw_times(drops, view, look, 1);
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
    fn the_ladder_goes_down_to_one_texel() {
        // (wgpu's levels: 960, 480, 240, 120, 60, 30, 15, 7, 3, 1.)
        assert_eq!(ladder_levels(1080, 1920), 10);
        assert_eq!(ladder_levels(1, 1), 1);
        assert_eq!(ladder_levels(3, 7), 3);
    }
}

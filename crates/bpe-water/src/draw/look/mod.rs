//! The look drawing (Water mode part 6; the plan
//! `docs/superpowers/plans/water-mode/part6-light-and-looks.md`, "Decisions"): the water as the cards
//! say, lit by its lamps, its glow and the light it bounces, over its back and under its solids'
//! materials, finished as a Balls picture is (bloom, exposure; effects and the grade in part 6c).
//!
//! [`LookGpu`] holds the pipelines (one a device); a [`LookDraw`] holds a [`Plain`] (its solid field
//! and depth ladder, read by the look as the plain drawing reads them), the course's node fields,
//! the light maps and the spread light (on grids fixed to the course, so the light doesn't depend on
//! the view), one picture size's textures and a `bpe_gfx::Renderer` for the finish. Drawn straight
//! from a body's drops in their order, so the same drops and look give the same picture to the bit.

mod back;
mod gpu;
pub mod inputs;
mod numbers;

use std::sync::Arc;

use bpe_gfx::{GivenPicture, Renderer};
use glam::Vec2;

pub use gpu::LookGpu;
pub use inputs::{
    BackLook, BackPicture, ColourFrom, FinishLook, GRADIENT_SAMPLES, GlowFrom, Lamp, LampKind, LightLook, LiquidLook,
    Look, MOST_LAMPS, MaterialKind, MaterialLook, Quality,
};

use gpu::PICTURE;
use numbers::{BoxU, Grid, Placing};

use crate::draw::plain::{Plain, PlainLook, View, attach, run_pass};
use crate::drops::DropBuffers;
use crate::drops::numbers::MOST_LIQUIDS;
use crate::solid::SolidField;

/// The most texels a side of a light map or the spread light.
const MOST_TEXELS: u32 = 2048;
/// The node fields' mip levels (the light's march reads up to level 5).
const NODE_LEVELS: u32 = 6;

fn texture(
    device: &wgpu::Device,
    label: &str,
    (width, height): (u32, u32),
    mips: u32,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 },
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: PICTURE,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | usage,
        view_formats: &[],
    })
}

fn view_of(device: &wgpu::Device, label: &str, size: (u32, u32)) -> wgpu::TextureView {
    texture(device, label, size, 1, wgpu::TextureUsages::empty()).create_view(&Default::default())
}

fn level_view(t: &wgpu::Texture, k: u32) -> wgpu::TextureView {
    t.create_view(&wgpu::TextureViewDescriptor { base_mip_level: k, mip_level_count: Some(1), ..Default::default() })
}

/// One picture size's look pictures.
struct Pictures {
    width: u32,
    height: u32,
    a0: wgpu::TextureView,
    a1: wgpu::TextureView,
    w0: wgpu::TextureView,
    /// Liquids 4 to 15's shares, made only when a look has more than four.
    w_more: Option<[wgpu::TextureView; 3]>,
}

/// The course's node fields (density, glow, colour, motion), each with its mips.
struct Nodes {
    levels: u32,
    all: [wgpu::TextureView; 4],
    /// `by_level[k]` holds level k of each.
    by_level: Vec<[wgpu::TextureView; 4]>,
}

impl Nodes {
    fn new(device: &wgpu::Device, cols: u32, rows: u32) -> Nodes {
        let most = device.limits().max_texture_dimension_2d;
        let size = (cols.clamp(1, most), rows.clamp(1, most));
        let levels = NODE_LEVELS.min(32 - size.0.max(size.1).leading_zeros());
        let make = |label| texture(device, label, size, levels, wgpu::TextureUsages::empty());
        let t =
            [make("water look density"), make("water look glow"), make("water look colour"), make("water look motion")];
        let all = [0, 1, 2, 3].map(|i| t[i].create_view(&Default::default()));
        let by_level = (0..levels).map(|k| [0, 1, 2, 3].map(|i| level_view(&t[i], k))).collect();
        Nodes { levels, all, by_level }
    }
}

/// A light map: (light, open), (way, weight) and (light with no liquid, open), on its grid.
struct LightMap {
    grid: Grid,
    light: wgpu::Texture,
    light_view: wgpu::TextureView,
    way_view: wgpu::TextureView,
    free_view: wgpu::TextureView,
    numbers: wgpu::Buffer,
}

impl LightMap {
    fn new(device: &wgpu::Device, grid: Grid, label: &str) -> LightMap {
        let size = (grid.width, grid.height);
        let light = texture(device, label, size, 1, wgpu::TextureUsages::COPY_SRC);
        let light_view = light.create_view(&Default::default());
        let way_view = view_of(device, label, size);
        let free_view = view_of(device, label, size);
        let numbers = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: std::mem::size_of::<BoxU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        LightMap { grid, light, light_view, way_view, free_view, numbers }
    }

    fn fits(&self, grid: &Grid) -> bool {
        self.grid.width == grid.width && self.grid.height == grid.height
    }
}

/// The spread light over the whole course: what is given off with its mips, the light gathered from
/// it, and the faces' light map it bounces.
struct Spread {
    grid: Grid,
    /// Each mip level of what is given off alone (to draw into, and to read the one before), and all.
    given_levels: Vec<wgpu::TextureView>,
    given_all: wgpu::TextureView,
    light: wgpu::TextureView,
    bounce: LightMap,
}

impl Spread {
    fn new(device: &wgpu::Device, grid: Grid) -> Spread {
        let size = (grid.width, grid.height);
        let levels = (32 - size.0.max(size.1).leading_zeros()).clamp(1, 8);
        let given = texture(device, "water look given off", size, levels, wgpu::TextureUsages::empty());
        let given_levels = (0..levels).map(|k| level_view(&given, k)).collect();
        let given_all = given.create_view(&Default::default());
        let light = view_of(device, "water look spread light", size);
        let bounce = LightMap::new(device, grid, "water look bounce light");
        Spread { grid, given_levels, given_all, light, bounce }
    }
}

/// A look drawing of one body's drops on one course.
pub struct LookDraw {
    gpu: Arc<LookGpu>,
    plain: Plain,
    uniform: wgpu::Buffer,
    grads: wgpu::Buffer,
    pictures: Option<Pictures>,
    nodes: Nodes,
    shot: Option<LightMap>,
    /// Whether the shot's light map is drawn in the next picture.
    shot_on: bool,
    spread: Option<Spread>,
    spread_on: bool,
    /// Whether the faces' light is drawn (lamps and Bounce).
    bounce_on: bool,
    renderer: Option<Renderer>,
    back: back::BackPicture,
    /// Too many lamps in the last look.
    too_many_lamps: bool,
    /// Whether mist or sparks are drawn.
    extras_on: bool,
    /// The wet marks drawn and the water frame they are read against.
    wet: Option<(wgpu::Buffer, u32)>,
}

impl std::fmt::Debug for LookDraw {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let size = self.pictures.as_ref().map(|p| (p.width, p.height));
        write!(f, "LookDraw({:?}, picture {size:?})", self.plain)
    }
}

impl LookDraw {
    /// A drawing of drops laid out on `field`'s layout, with its solids (`rest` as
    /// [`Plain::new`]'s).
    pub fn new(gpu: Arc<LookGpu>, field: &SolidField, rest: f32) -> LookDraw {
        let device = gpu.device().clone();
        let plain = Plain::new(gpu.plain.clone(), field, rest);
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("water look numbers"),
            size: std::mem::size_of::<numbers::LookU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let grads = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("water look gradients"),
            size: ((MOST_LIQUIDS + 1) * GRADIENT_SAMPLES * 16) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let nodes = Nodes::new(&device, field.layout.cols, field.layout.rows);
        LookDraw {
            gpu,
            plain,
            uniform,
            grads,
            pictures: None,
            nodes,
            shot: None,
            shot_on: false,
            spread: None,
            spread_on: false,
            bounce_on: false,
            renderer: None,
            back: back::BackPicture::default(),
            too_many_lamps: false,
            extras_on: false,
            wet: None,
        }
    }

    /// The course changed (its layout must be the drops' own).
    pub fn set_field(&mut self, field: &SolidField) {
        let layout = self.plain.layout;
        self.plain.set_field(field);
        if field.layout != layout {
            self.nodes = Nodes::new(self.gpu.device(), field.layout.cols, field.layout.rows);
            self.spread = None;
            self.shot = None;
        }
    }

    /// Draws another body's drops on `field` (`rest` as [`LookDraw::new`]'s), keeping the pictures,
    /// the finish and the back's picture (a new body after a physics edit).
    pub fn renew(&mut self, field: &SolidField, rest: f32) {
        let layout = self.plain.layout;
        self.plain = Plain::new(self.gpu.plain.clone(), field, rest);
        if field.layout != layout {
            self.nodes = Nodes::new(self.gpu.device(), field.layout.cols, field.layout.rows);
            self.spread = None;
            self.shot = None;
        }
    }

    pub fn gpu(&self) -> &Arc<LookGpu> {
        &self.gpu
    }

    /// The layout it draws drops on.
    pub fn layout(&self) -> crate::Layout {
        self.plain.layout
    }

    /// What the stage should tell about the last look drawn (a back picture that can't be read, too
    /// many lamps), in plain words.
    pub fn notes(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.back.failed() {
            out.push("The back's picture can't be read".to_string());
        }
        if self.too_many_lamps {
            out.push(format!("Only {MOST_LAMPS} lamps light a Water scene"));
        }
        out
    }

    fn pictures_for(&mut self, width: u32, height: u32, many: bool) {
        let device = self.gpu.device();
        let fits = self.pictures.as_ref().is_some_and(|p| p.width == width && p.height == height);
        if !fits {
            self.pictures = Some(Pictures {
                width,
                height,
                a0: view_of(device, "water look amount", (width, height)),
                a1: view_of(device, "water look way", (width, height)),
                w0: view_of(device, "water look shares", (width, height)),
                w_more: None,
            });
        }
        let p = self.pictures.as_mut().expect("just made");
        if many && p.w_more.is_none() {
            p.w_more = Some([0, 1, 2].map(|_| view_of(device, "water look more shares", (width, height))));
        }
        match self.renderer.as_mut() {
            Some(r) if r.size() == (width, height) => {}
            Some(r) => r.resize(device, width, height),
            None => self.renderer = Some(Renderer::new(device, width, height)),
        }
    }

    /// The wet marks the next pictures draw, read against water frame `frame` (`None`, or marks
    /// on another layout: none).
    pub fn set_wet(&mut self, wet: Option<&crate::draw::WetField>, frame: u32) {
        self.wet = wet.filter(|w| w.layout() == self.plain.layout).map(|w| (w.buffer().clone(), frame));
    }

    /// Draws `drops` (in this drawing's layout's grid units) as `view` shows them, with `look`, and
    /// sends it to the card. The picture is [`LookDraw::output`] (and [`LookDraw::read`]).
    pub fn draw(&mut self, drops: &DropBuffers, view: &View, look: &Look) {
        self.plain.ahead = 0.0;
        self.draw_times(drops, view, look, 1);
    }

    /// The same with every drop moved on by its speed for `ahead` seconds (a final's kept moment
    /// drawn a little after it, part 8).
    pub fn draw_at(&mut self, drops: &DropBuffers, view: &View, look: &Look, ahead: f32) {
        self.plain.ahead = ahead;
        self.draw_times(drops, view, look, 1);
        self.plain.ahead = 0.0;
    }

    /// Draws `times` pictures and waits for the card: the time a picture takes (for the measuring
    /// runs).
    pub fn draw_many(&mut self, drops: &DropBuffers, view: &View, look: &Look, times: u32) -> std::time::Duration {
        self.draw_times(drops, view, look, 3);
        self.gpu.device().poll(wgpu::PollType::wait_indefinitely()).expect("the card");
        let at = std::time::Instant::now();
        self.draw_times(drops, view, look, times.max(1));
        self.gpu.device().poll(wgpu::PollType::wait_indefinitely()).expect("the card");
        at.elapsed() / times.max(1)
    }

    fn draw_times(&mut self, drops: &DropBuffers, view: &View, look: &Look, times: u32) {
        let gpu = self.gpu.clone();
        let (device, queue) = (gpu.device(), gpu.queue());
        let (width, height) = self.plain.size_of(view);
        let liquids = numbers::liquids_of(look).len().min(MOST_LIQUIDS);
        self.pictures_for(width, height, liquids > 4);
        let plain_numbers = self.plain.numbers(view, width, height, &PlainLook::default());
        queue.write_buffer(&self.plain.uniform, 0, bytemuck::bytes_of(&plain_numbers));

        // The grids, fixed to the course.
        let layout = self.plain.layout;
        let most = MOST_TEXELS.min(device.limits().max_texture_dimension_2d);
        let quality = look.light.quality;
        let lit = numbers::lit(look);
        let seen_hi = view.lo + Vec2::new(width as f32, height as f32) / view.scale.max(1e-6);
        let reach = Vec2::splat(numbers::light_reach(look, quality.light_texel(), layout.cell));
        let shot = if lit {
            numbers::grid_over(&layout, view.lo - reach, seen_hi + reach, quality.light_texel(), most)
        } else {
            None
        };
        self.shot_on = shot.is_some();
        if let Some(grid) = shot {
            if !self.shot.as_ref().is_some_and(|m| m.fits(&grid)) {
                self.shot = Some(LightMap::new(device, grid, "water look light"));
            }
            let m = self.shot.as_mut().expect("just made");
            m.grid = grid;
            let b = BoxU { lo: [grid.lo.x, grid.lo.y, grid.texel, 0.0], size: [grid.width, grid.height, 0, 0] };
            queue.write_buffer(&m.numbers, 0, bytemuck::bytes_of(&b));
        }
        self.spread_on = numbers::spreads(look);
        self.bounce_on = self.spread_on && lit && look.light.bounce > 0.0;
        if self.spread_on {
            let grid = numbers::whole_course(&layout, quality.spread_texel(), most);
            if self.spread.as_ref().is_none_or(|s| s.grid != grid) {
                self.spread = Some(Spread::new(device, grid));
            }
            let s = self.spread.as_ref().expect("just made");
            let b = BoxU { lo: [grid.lo.x, grid.lo.y, grid.texel, 1.0], size: [grid.width, grid.height, 0, 0] };
            queue.write_buffer(&s.bounce.numbers, 0, bytemuck::bytes_of(&b));
        }
        self.extras_on = numbers::extras(look);
        self.too_many_lamps = look.lamps.iter().filter(|l| l.brightness > 0.0).count() > MOST_LAMPS;
        let picture = self.back.picture_for(device, queue, &look.back, &layout);
        let placing = Placing {
            scale: view.scale,
            shot,
            spread: self.spread.as_ref().filter(|_| self.spread_on).map(|s| (s.grid, s.given_levels.len() as u32)),
            picture,
        };
        let mut u = numbers::look_u(look, &layout, &placing);
        u.time[3] = self.wet.as_ref().map_or(-1.0, |w| w.1 as f32);
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&u));
        queue.write_buffer(&self.grads, 0, bytemuck::cast_slice(&numbers::gradients(look)));

        // (The finish measures in the video's pixels when the picture is its shot: across, as the
        // masks' handles do, so a picture of another shape than the video, the editor's 9:16 shot of
        // a video of another size, is measured the same way both ways.)
        let video = match look.finish.video {
            (w, h) if w > 0 && h > 0 => {
                let tall = (f64::from(w) * f64::from(height) / f64::from(width.max(1))).round().max(1.0);
                (w, tall as u32)
            }
            _ => (width, height),
        };
        let frame = bpe_gfx::Frame {
            width: video.0,
            height: video.1,
            video,
            time: look.time,
            bloom: if look.finish.bloom.is_finite() { look.finish.bloom.max(0.0) } else { 0.0 },
            exposure: if look.finish.exposure.is_finite() { look.finish.exposure.max(0.0) } else { 1.0 },
            effects: look.finish.effects.clone(),
            grade: look.finish.grade.clone(),
            ..bpe_gfx::Frame::default()
        };
        for _ in 0..times.max(1) {
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("water look") });
            self.record(&mut encoder, drops, liquids > 4);
            let renderer = self.renderer.as_mut().expect("made with the pictures");
            renderer.finish_picture(device, queue, encoder, &frame, GivenPicture::default());
        }
    }

    /// Records one look drawing of `drops` into `encoder`, into the renderer's HDR picture.
    fn record(&self, encoder: &mut wgpu::CommandEncoder, drops: &DropBuffers, many: bool) {
        let gpu = &self.gpu;
        let device = gpu.device();
        let p = self.pictures.as_ref().expect("the pictures are made before recording");
        let tex = wgpu::BindingResource::TextureView;
        let group = |pipe: &gpu::Pipe, label: &str, entries: &[wgpu::BindGroupEntry]| {
            device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some(label), layout: &pipe.layout, entries })
        };
        let drop_entries = || {
            [
                entry(0, self.plain.uniform.as_entire_binding()),
                entry(1, drops.xv.as_entire_binding()),
                entry(2, drops.id.as_entire_binding()),
                entry(10, self.uniform.as_entire_binding()),
            ]
        };
        // 1. The look splats, every drop in the buffers' order (so the sums repeat).
        let splats = group(&gpu.splat, "water look splats", &drop_entries());
        let targets = [attach(&p.a0), attach(&p.a1), attach(&p.w0)];
        run_pass(encoder, "water look splats", &targets, &gpu.splat.pipeline, &splats, 6, drops.count);
        if many && let Some(w) = &p.w_more {
            let weights = group(&gpu.weights, "water look weights", &drop_entries());
            let targets = [attach(&w[0]), attach(&w[1]), attach(&w[2])];
            run_pass(encoder, "water look weights", &targets, &gpu.weights.pipeline, &weights, 6, drops.count);
        }
        // 2. The depth ladder (plain's).
        self.plain.record_ladder(encoder, drops);
        // 3. The node fields and their mips.
        let n = &self.nodes;
        let nodes = group(&gpu.nodes, "water look nodes", &drop_entries());
        let targets = n.by_level[0].each_ref().map(attach);
        run_pass(encoder, "water look nodes", &targets, &gpu.nodes.pipeline, &nodes, 6, drops.count);
        for k in 1..n.levels as usize {
            let [d, g, c, m] = &n.by_level[k - 1];
            let half = group(
                &gpu.half,
                "water look half",
                &[entry(21, tex(d)), entry(22, tex(g)), entry(23, tex(c)), entry(34, tex(m))],
            );
            let targets = n.by_level[k].each_ref().map(attach);
            run_pass(encoder, "water look half", &targets, &gpu.half.pipeline, &half, 3, 1);
        }
        // 4. The light maps.
        let light_pass = |encoder: &mut wgpu::CommandEncoder, map: &LightMap, on: bool| {
            let light = group(
                &gpu.light,
                "water look light",
                &[
                    entry(0, self.plain.uniform.as_entire_binding()),
                    entry(5, self.plain.field.as_entire_binding()),
                    entry(10, self.uniform.as_entire_binding()),
                    entry(18, tex(&n.all[0])),
                    entry(26, map.numbers.as_entire_binding()),
                ],
            );
            let targets = [attach(&map.light_view), attach(&map.way_view), attach(&map.free_view)];
            run_pass(encoder, "water look light", &targets, &gpu.light.pipeline, &light, 3, u32::from(on));
        };
        if self.shot_on
            && let Some(map) = &self.shot
        {
            light_pass(encoder, map, true);
        }
        // 5. The spread light.
        let spread = self.spread.as_ref().filter(|_| self.spread_on);
        if let Some(s) = spread {
            light_pass(encoder, &s.bounce, self.bounce_on);
            let spread_in = group(
                &gpu.spread_in,
                "water look spread in",
                &[
                    entry(0, self.plain.uniform.as_entire_binding()),
                    entry(5, self.plain.field.as_entire_binding()),
                    entry(10, self.uniform.as_entire_binding()),
                    entry(19, tex(&n.all[1])),
                    entry(20, tex(&n.all[2])),
                    entry(32, tex(&s.bounce.light_view)),
                ],
            );
            let first = [attach(&s.given_levels[0])];
            run_pass(encoder, "water look spread in", &first, &gpu.spread_in.pipeline, &spread_in, 3, 1);
            for k in 1..s.given_levels.len() {
                let down = group(&gpu.spread_down, "water look spread down", &[entry(30, tex(&s.given_levels[k - 1]))]);
                let target = [attach(&s.given_levels[k])];
                run_pass(encoder, "water look spread down", &target, &gpu.spread_down.pipeline, &down, 3, 1);
            }
            let gather = group(
                &gpu.spread_gather,
                "water look spread gather",
                &[
                    entry(0, self.plain.uniform.as_entire_binding()),
                    entry(5, self.plain.field.as_entire_binding()),
                    entry(10, self.uniform.as_entire_binding()),
                    entry(29, tex(&s.given_all)),
                ],
            );
            let target = [attach(&s.light)];
            run_pass(encoder, "water look spread gather", &target, &gpu.spread_gather.pipeline, &gather, 3, 1);
        }
        // 6. The picture, in linear light, into the renderer's HDR picture.
        let black = &gpu.black;
        let (lmap, ldir, lfree) = match (&self.shot, self.shot_on) {
            (Some(m), true) => (&m.light_view, &m.way_view, &m.free_view),
            _ => (black, black, black),
        };
        let (spread_s, spread_eo) = spread.map_or((black, black), |s| (&s.light, &s.given_all));
        let [w1, w2, w3] = match (&p.w_more, many) {
            (Some([a, b, c]), true) => [a, b, c],
            _ => [&p.w0, &p.w0, &p.w0],
        };
        let compose = group(
            &gpu.compose,
            "water look",
            &[
                entry(0, self.plain.uniform.as_entire_binding()),
                entry(5, self.plain.field.as_entire_binding()),
                entry(6, tex(&self.plain.depth.ladder_all)),
                entry(10, self.uniform.as_entire_binding()),
                entry(11, tex(&p.a0)),
                entry(13, tex(&p.w0)),
                entry(14, tex(w1)),
                entry(15, tex(w2)),
                entry(16, tex(w3)),
                entry(17, self.grads.as_entire_binding()),
                entry(24, tex(lmap)),
                entry(25, tex(ldir)),
                entry(27, tex(spread_s)),
                entry(28, tex(self.back.view().unwrap_or(black))),
                entry(29, tex(spread_eo)),
                entry(33, tex(lfree)),
                entry(35, tex(&n.all[3])),
                entry(36, self.wet.as_ref().map_or(&gpu.no_wet, |w| &w.0).as_entire_binding()),
            ],
        );
        let renderer = self.renderer.as_ref().expect("made with the pictures");
        run_pass(encoder, "water look", &[attach(renderer.hdr_target())], &gpu.compose.pipeline, &compose, 3, 1);
        // 7. Mist and sparks over it.
        if self.extras_on {
            let extras = group(
                &gpu.extras,
                "water look extras",
                &[
                    entry(0, self.plain.uniform.as_entire_binding()),
                    entry(1, drops.xv.as_entire_binding()),
                    entry(2, drops.id.as_entire_binding()),
                    entry(5, self.plain.field.as_entire_binding()),
                    entry(10, self.uniform.as_entire_binding()),
                    entry(18, tex(&n.all[0])),
                    entry(24, tex(lmap)),
                    entry(27, tex(spread_s)),
                ],
            );
            let target = [Some(wgpu::RenderPassColorAttachment {
                view: renderer.hdr_target(),
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })];
            run_pass(encoder, "water look extras", &target, &gpu.extras.pipeline, &extras, 12, drops.count);
        }
    }

    /// The last picture drawn (rgba8 holding sRGB), the renderer's output.
    pub fn output(&self) -> Option<&wgpu::Texture> {
        self.renderer.as_ref().filter(|_| self.pictures.is_some()).map(|r| r.output_texture())
    }

    /// The last picture's own view (rgba8 holding sRGB), for a readback.
    pub fn output_view(&self) -> Option<&wgpu::TextureView> {
        self.renderer.as_ref().filter(|_| self.pictures.is_some()).map(|r| r.output_view())
    }

    /// The last picture drawn, read back: rgba8, sRGB, rows packed (empty before any draw).
    pub fn read(&self) -> Vec<u8> {
        match (&self.renderer, &self.pictures) {
            (Some(r), Some(_)) => r.read_output(self.gpu.device(), self.gpu.queue()).unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// The last picture's shot light map at world point `p`, read back (slow: for tests). `None`
    /// where it wasn't drawn.
    pub fn light_at(&self, p: Vec2) -> Option<[f32; 3]> {
        let map = self.shot.as_ref().filter(|_| self.shot_on)?;
        let t = ((p - map.grid.lo) / map.grid.texel).floor();
        if t.x < 0.0 || t.y < 0.0 || t.x >= map.grid.width as f32 || t.y >= map.grid.height as f32 {
            return None;
        }
        let device = self.gpu.device();
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("water look light read"),
            size: 8,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &map.light,
                mip_level: 0,
                origin: wgpu::Origin3d { x: t.x as u32, y: t.y as u32, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: None, rows_per_image: None },
            },
            wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        );
        self.gpu.queue().submit([encoder.finish()]);
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).expect("the card");
        let data = slice.get_mapped_range().expect("mapped");
        let half = |i: usize| f16_to_f32(u16::from_le_bytes([data[2 * i], data[2 * i + 1]]));
        Some([half(0), half(1), half(2)])
    }
}

fn entry(binding: u32, resource: wgpu::BindingResource<'_>) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry { binding, resource }
}

/// A half float's value.
fn f16_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = i32::from((h >> 10) & 0x1f);
    let frac = f32::from(h & 0x3ff);
    sign * match exp {
        0 => frac / 1024.0 * (2.0f32).powi(-14),
        31 => f32::INFINITY,
        e => (1.0 + frac / 1024.0) * (2.0f32).powi(e - 15),
    }
}

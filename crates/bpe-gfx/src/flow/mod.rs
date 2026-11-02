//! GPU-stepped layers: liquids with thousands of particles and swarms, stepped at [`SIM_HZ`]
//! from a private copy of the world so the playback the sound and rules read never depends
//! on the graphics card.
//!
//! [`FlowLayers`] owns that private [`World`], steps it along with the layers in batches, and
//! keeps checkpoints of both so seeking back is cheap. Each GPU step is told the first few
//! balls and where the wall is ([`inputs`]) and reads the wall distance field ([`sdf`]).
//! Canvases are layers too: each step stamps the marks the private world made that step into
//! their textures, and checkpoints copy the textures.

mod inputs;
mod liquid;
mod sdf;
pub(crate) mod surface;
mod swarm;
pub(crate) mod util;

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use bpe_core::{Resolved, Scene, ThingId};
use bpe_runtime::{Log, Mark, MarkTarget, Outline, Sims, World};

use crate::canvas::CanvasTextures;
use crate::canvas::pictures::PictureAtlas;
use crate::canvas::stamp::{CanvasGpu, StampPipeline};
use crate::canvas::texture_bytes;
use crate::frame::{Frame, LiquidLayer, Points, SwarmLayer};
use inputs::{Inputs, step_inputs};
use liquid::{LiquidGpu, LiquidPipelines};
use sdf::Sdf;
use swarm::{SwarmGpu, SwarmPipelines, SwarmSim};

/// Layer steps per second of video.
pub const SIM_HZ: u32 = 120;
/// Steps sent to the GPU in one submission when catching up.
pub const BATCH_STEPS: u32 = 64;
/// Most balls each step tells the layers about.
pub const MAX_BALLS: usize = 64;
/// Layer steps between checkpoints, at first.
pub const CHECKPOINT_STEPS: u64 = 240;
/// Most bytes the checkpoints may hold together. Past this, every other one is dropped.
pub const CHECKPOINT_BYTES: u64 = 256 << 20;
/// Events kept in the private log before older ones are dropped.
const LOG_KEEP: usize = 4096;

/// What the layers step along with: the scene the private world is built from, the
/// fingerprint that says when to start over, and how long one frame may spend catching up.
pub struct FlowSource<'a> {
    pub scene: &'a Scene,
    pub resolved: &'a Resolved,
    pub sims: &'a Sims,
    /// The scene's physics fingerprint; a new one rebuilds the private world and the layers.
    pub key: u64,
    /// Stop catching up after about this long, for a preview that must stay responsive.
    pub budget: Option<Duration>,
    /// The linear-light colour of each ball thing, for layers that colour by the nearest ball.
    pub ball_colors: BTreeMap<ThingId, [f32; 4]>,
    /// The folder relative picture paths are read from: the scene file's folder.
    pub picture_folder: Option<std::path::PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlowStatus {
    /// False when the budget ran out before the layers reached the frame's time.
    pub caught_up: bool,
}

/// The pipelines the layers share.
struct Pipelines {
    swarm: SwarmPipelines,
    liquid: LiquidPipelines,
    stamp: StampPipeline,
}

/// What a layer belongs to: a swarm or liquid thing, or the marks of a canvas or a skin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum LayerKey {
    Thing(ThingId),
    Marks(MarkTarget),
}

/// One GPU-stepped layer.
enum Layer {
    Swarm(Box<SwarmGpu>),
    Liquid(LiquidGpu),
    Canvas(Box<CanvasGpu>),
}

/// What one layer step works with.
struct StepCx<'a> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    inputs: &'a Inputs,
    /// Binds the step's inputs in `inputs.group`.
    offset: u32,
    sdf: &'a Sdf,
    /// The marks the private world made during this step, in order.
    marks: &'a [Mark],
    steps_per_second: u32,
    /// Whether the step is the first of its batch.
    first: bool,
}

impl Layer {
    /// Records one step into `encoder`.
    fn step(&mut self, pipelines: &Pipelines, encoder: &mut wgpu::CommandEncoder, cx: &StepCx) {
        let (inputs, offset, sdf) = (cx.inputs, cx.offset, cx.sdf);
        match self {
            Layer::Swarm(s) => s.step(&pipelines.swarm, encoder, inputs, offset, sdf),
            Layer::Liquid(l) => l.step(&pipelines.liquid, encoder, inputs, offset, sdf),
            Layer::Canvas(c) => {
                c.step(cx.device, cx.queue, &pipelines.stamp, encoder, cx.marks, cx.steps_per_second, cx.first)
            }
        }
    }

    /// The buffers that carry the layer's state from one step to the next; checkpoints copy them.
    fn state(&self) -> Vec<&wgpu::Buffer> {
        match self {
            Layer::Swarm(s) => s.state(),
            Layer::Liquid(l) => l.state(),
            Layer::Canvas(_) => Vec::new(),
        }
    }

    /// The textures that carry the layer's state; checkpoints copy them too.
    fn textures(&self) -> Vec<&wgpu::Texture> {
        match self {
            Layer::Canvas(c) => c.state().to_vec(),
            Layer::Swarm(_) | Layer::Liquid(_) => Vec::new(),
        }
    }
}

/// Copies of a layer's state buffers and textures at one step.
struct Snapshot {
    buffers: Vec<wgpu::Buffer>,
    textures: Vec<wgpu::Texture>,
}

impl Snapshot {
    fn take(device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, layer: &Layer) -> Self {
        let buffers = layer
            .state()
            .into_iter()
            .map(|source| {
                let copy = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("flow checkpoint"),
                    size: source.size(),
                    usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                encoder.copy_buffer_to_buffer(source, 0, &copy, 0, source.size());
                copy
            })
            .collect();
        let textures = layer
            .textures()
            .into_iter()
            .map(|source| {
                let copy = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("flow checkpoint"),
                    size: source.size(),
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: source.format(),
                    usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                encoder.copy_texture_to_texture(source.as_image_copy(), copy.as_image_copy(), source.size());
                copy
            })
            .collect();
        Self { buffers, textures }
    }

    fn restore(&self, encoder: &mut wgpu::CommandEncoder, layer: &Layer) {
        for (copy, target) in self.buffers.iter().zip(layer.state()) {
            encoder.copy_buffer_to_buffer(copy, 0, target, 0, copy.size().min(target.size()));
        }
        for (copy, target) in self.textures.iter().zip(layer.textures()) {
            encoder.copy_texture_to_texture(copy.as_image_copy(), target.as_image_copy(), copy.size());
        }
    }

    fn bytes(&self) -> u64 {
        self.buffers.iter().map(|b| b.size()).sum::<u64>() + self.textures.iter().map(texture_bytes).sum::<u64>()
    }
}

struct Checkpoint {
    step: u64,
    world: World,
    log: Log,
    layers: BTreeMap<LayerKey, Snapshot>,
}

impl Checkpoint {
    fn bytes(&self) -> u64 {
        (self.world.memory() + self.log.memory()) as u64 + self.layers.values().map(Snapshot::bytes).sum::<u64>()
    }
}

/// The private world and where the layers have got to.
struct Driver {
    key: u64,
    video: (u32, u32),
    world: World,
    log: Log,
    /// The last layer step run. Step 0 is the starting state.
    step: u64,
    checkpoints: Vec<Checkpoint>,
    /// Layer steps between checkpoints.
    interval: u64,
}

/// Steps every GPU layer of a frame to the frame's time.
pub struct FlowLayers {
    inputs: Inputs,
    sdf: Sdf,
    pipelines: Pipelines,
    driver: Option<Driver>,
    layers: BTreeMap<LayerKey, Layer>,
    steps_run: u64,
    /// Most bytes the checkpoints may hold together; [`CHECKPOINT_BYTES`] unless changed.
    checkpoint_bytes: u64,
    /// The scene's picture files that could not be read, as the scene gave them.
    missing_pictures: Vec<String>,
}

/// The frame's swarms, then those of its blend layers: all of them are stepped here.
fn all_swarms(frame: &Frame) -> impl Iterator<Item = &SwarmLayer> + Clone {
    frame.swarms.iter().chain(frame.layers.iter().flat_map(|l| &l.frame.swarms))
}

/// The frame's liquids, then those of its blend layers.
fn all_liquids(frame: &Frame) -> impl Iterator<Item = &LiquidLayer> + Clone {
    frame.liquids.iter().chain(frame.layers.iter().flat_map(|l| &l.frame.liquids))
}

/// The wall the layers keep out of: the first closed wall in id order.
fn first_wall(world: &World) -> Option<Outline> {
    world.walls.values().next().map(|w| w.outline)
}

/// Half the size of the area a swarm's Fill start covers: the wall's, or the frame's.
fn arena_size(wall: Option<&Outline>, video: (u32, u32)) -> f32 {
    match wall {
        Some(Outline::Ring { radius, .. }) => *radius,
        Some(Outline::Box { half, .. }) | Some(Outline::Rounded { half, .. }) => half.max_element(),
        None => video.0.max(video.1) as f32 * 0.5,
    }
}

/// Keeps the log small: emitters only read back one step, so older events can go, and the
/// marks up to `world_step` are drawn already.
fn trim_log(log: &mut Log, world_step: u64) {
    log.drop_marks_upto(world_step);
    if log.len() > LOG_KEEP {
        let recent = log.since(world_step.saturating_sub(1)).to_vec();
        *log = Log::default();
        for e in recent {
            log.push(e);
        }
    }
}

impl FlowLayers {
    pub fn new(device: &wgpu::Device) -> Self {
        let inputs = Inputs::new(device);
        let sdf = Sdf::new(device);
        let pipelines = Pipelines {
            swarm: SwarmPipelines::new(device, &inputs.layout, &sdf.layout),
            liquid: LiquidPipelines::new(device, &inputs.layout, &sdf.layout),
            stamp: StampPipeline::new(device),
        };
        Self {
            inputs,
            sdf,
            pipelines,
            driver: None,
            missing_pictures: Vec::new(),
            layers: BTreeMap::new(),
            steps_run: 0,
            checkpoint_bytes: CHECKPOINT_BYTES,
        }
    }

    /// The picture files the running scene names that could not be read.
    pub fn missing_pictures(&self) -> &[String] {
        &self.missing_pictures
    }

    /// Changes how many bytes the checkpoints may hold together.
    pub fn set_checkpoint_budget(&mut self, bytes: u64) {
        self.checkpoint_bytes = bytes;
    }

    /// Checkpoints kept, the steps between them, and the bytes they hold, for tests.
    #[cfg(test)]
    pub(crate) fn checkpoint_stats(&self) -> (usize, u64, u64) {
        self.driver
            .as_ref()
            .map_or((0, 0, 0), |d| (d.checkpoints.len(), d.interval, d.checkpoints.iter().map(Checkpoint::bytes).sum()))
    }

    /// One swarm's GPU state, for tests.
    #[cfg(test)]
    pub(crate) fn swarm(&self, thing: ThingId) -> Option<&SwarmGpu> {
        match self.layers.get(&LayerKey::Thing(thing))? {
            Layer::Swarm(s) => Some(s.as_ref()),
            _ => None,
        }
    }

    /// One liquid's GPU state, for tests.
    #[cfg(test)]
    pub(crate) fn liquid(&self, thing: ThingId) -> Option<&LiquidGpu> {
        match self.layers.get(&LayerKey::Thing(thing))? {
            Layer::Liquid(l) => Some(l),
            _ => None,
        }
    }

    /// The textures a canvas's or skin's marks are painted into; `None` until its layer runs.
    pub fn canvas_textures(&self, key: MarkTarget) -> Option<&CanvasTextures> {
        match self.layers.get(&LayerKey::Marks(key))? {
            Layer::Canvas(c) => Some(&c.textures),
            _ => None,
        }
    }

    /// Where a GPU liquid's particles are, for the surface pass; `None` when the frame's
    /// layer is not (yet) running.
    pub(crate) fn gpu_points(&self, thing: ThingId) -> Option<surface::PointsBinding<'_>> {
        match self.layers.get(&LayerKey::Thing(thing))? {
            Layer::Liquid(l) => Some(surface::PointsBinding::Gpu {
                buffer: l.positions_buffer(),
                count: l.count(),
                spacing: l.spacing(),
                pour_steps: l.pour_steps(),
                step: self.step(),
            }),
            _ => None,
        }
    }

    /// The last layer step run; 0 before any, or without GPU layers.
    pub fn step(&self) -> u64 {
        self.driver.as_ref().map_or(0, |d| d.step)
    }

    /// Steps run since this was made, for tests and benchmarks.
    pub fn steps_run(&self) -> u64 {
        self.steps_run
    }

    /// Makes the layers match the frame: starts over when `source.key`, the set of GPU layers
    /// or how one of them moves changed, takes a changed look in place, and lets everything go
    /// when the frame has no GPU layers. The frame's canvases are layers too (only the top
    /// frame's: the renderer draws no others); a new size or texture size starts over.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame, source: &FlowSource) {
        let gpu_liquids = all_liquids(frame).filter_map(|l| match &l.points {
            Points::Gpu(desc) => Some((l.thing, desc)),
            Points::Cpu(_) => None,
        });
        let wanted: BTreeSet<LayerKey> = all_swarms(frame)
            .map(|s| LayerKey::Thing(s.thing))
            .chain(gpu_liquids.clone().map(|(id, _)| LayerKey::Thing(id)))
            .chain(frame.canvases.iter().map(|c| LayerKey::Marks(c.key)))
            .collect();
        if wanted.is_empty() {
            self.driver = None;
            self.layers.clear();
            return;
        }
        let same = self.driver.as_ref().is_some_and(|d| d.key == source.key && d.video == frame.video)
            && self.layers.keys().eq(wanted.iter())
            && all_swarms(frame).all(|l| match self.layers.get(&LayerKey::Thing(l.thing)) {
                Some(Layer::Swarm(s)) => *s.sim() == SwarmSim::of(l),
                _ => false,
            })
            && gpu_liquids.clone().all(|(id, desc)| match self.layers.get(&LayerKey::Thing(id)) {
                Some(Layer::Liquid(l)) => l.desc() == desc,
                _ => false,
            })
            && frame.canvases.iter().all(|c| match self.layers.get(&LayerKey::Marks(c.key)) {
                Some(Layer::Canvas(g)) => g.size == c.size && g.texels == c.texels,
                _ => false,
            });
        if !same {
            self.restart(device, queue, frame, source);
            return;
        }
        for l in all_swarms(frame) {
            if let Some(Layer::Swarm(s)) = self.layers.get_mut(&LayerKey::Thing(l.thing)) {
                s.set_look(queue, l);
            }
        }
    }

    /// Builds the private world afresh and every layer at step 0.
    fn restart(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame, source: &FlowSource) {
        let world = World::build(source.scene, source.resolved, source.sims);
        // The pictures are read here, once, never in the frame loop. The canvases' bind groups
        // keep the atlas alive.
        let atlas = PictureAtlas::load(device, queue, &world.pictures, source.picture_folder.as_deref());
        self.missing_pictures = atlas.missing.clone();
        let video = frame.video;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("flow start") });
        let wall = first_wall(&world);
        match &wall {
            Some(outline) => self.sdf.bake(device, queue, &mut encoder, outline),
            None => self.sdf.clear(queue),
        }
        let bytes = step_inputs(&world, 0.0, 0, video, &source.ball_colors, &self.sdf.frame(wall.as_ref()));
        self.inputs.write(queue, 0, &bytes);
        let arena = arena_size(wall.as_ref(), video);
        let (pipelines, inputs, sdf) = (&self.pipelines, &self.inputs, &self.sdf);
        let swarms = all_swarms(frame).map(|l| {
            let swarm = SwarmGpu::new(device, queue, &mut encoder, &pipelines.swarm, inputs, sdf, l, video, arena);
            (LayerKey::Thing(l.thing), Layer::Swarm(Box::new(swarm)))
        });
        let liquids = all_liquids(frame).filter_map(|l| match &l.points {
            Points::Gpu(desc) => {
                let liquid = LiquidGpu::new(device, queue, &pipelines.liquid, desc, wall.as_ref(), video);
                Some((LayerKey::Thing(l.thing), Layer::Liquid(liquid)))
            }
            Points::Cpu(_) => None,
        });
        let canvases = frame.canvases.iter().map(|c| {
            let canvas = CanvasGpu::new(device, queue, &pipelines.stamp, &atlas, c.key, c.size, c.texels);
            (LayerKey::Marks(c.key), Layer::Canvas(Box::new(canvas)))
        });
        self.layers = liquids.collect();
        self.layers.extend(swarms);
        self.layers.extend(canvases);
        let snapshots = self.layers.iter().map(|(id, l)| (*id, Snapshot::take(device, &mut encoder, l))).collect();
        queue.submit([encoder.finish()]);
        let checkpoint = Checkpoint { step: 0, world: world.clone(), log: Log::default(), layers: snapshots };
        self.driver = Some(Driver {
            key: source.key,
            video,
            world,
            log: Log::default(),
            step: 0,
            checkpoints: vec![checkpoint],
            interval: CHECKPOINT_STEPS,
        });
    }

    /// Draws the frame's swarms into the scene pass, after the walls and before everything
    /// in front of them; in a layer whose things blend with each other (`each`), with the
    /// pipeline [`FlowLayers::ensure_each`] made.
    pub(crate) fn draw_swarms(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        globals: &wgpu::BindGroup,
        paint: &wgpu::BindGroup,
        frame: &Frame,
        each: Option<bpe_core::Blend>,
    ) {
        // Without swarms no each-pipeline was made for this layer's mode.
        if frame.swarms.is_empty() {
            return;
        }
        let pipeline = match each {
            Some(blend) => self.pipelines.swarm.each(blend),
            None => self.pipelines.swarm.draw(),
        };
        for l in &frame.swarms {
            if let Some(Layer::Swarm(s)) = self.layers.get(&LayerKey::Thing(l.thing)) {
                s.draw(pipeline, pass, globals, paint);
            }
        }
    }

    /// Makes the pipeline swarms blending with each other by `blend` are drawn with.
    pub(crate) fn ensure_each(&mut self, device: &wgpu::Device, blend: bpe_core::Blend) {
        self.pipelines.swarm.ensure_each(device, blend);
    }

    /// Steps the layers to `time`, in batches of [`BATCH_STEPS`]. Going back restores the
    /// nearest checkpoint first. With a budget, stops between batches once it is spent.
    pub fn step_to(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        time: f64,
        source: &FlowSource,
    ) -> FlowStatus {
        let Some(driver) = &self.driver else { return FlowStatus { caught_up: true } };
        let started = Instant::now();
        let target = (time.max(0.0) * SIM_HZ as f64).floor() as u64;
        if target < driver.step {
            self.rewind(device, queue, target);
        }
        while self.driver.as_ref().unwrap().step < target {
            self.run_batch(device, queue, target, source);
            if let Some(budget) = source.budget
                && self.driver.as_ref().unwrap().step < target
            {
                // Steps run on the GPU after they are sent, so wait for them to know the time
                // they really took.
                let _ = device.poll(wgpu::PollType::wait_indefinitely());
                if started.elapsed() >= budget {
                    return FlowStatus { caught_up: false };
                }
            }
        }
        FlowStatus { caught_up: true }
    }

    /// Back to the latest checkpoint at or before `target`.
    fn rewind(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, target: u64) {
        let driver = self.driver.as_mut().unwrap();
        let index = driver.checkpoints.iter().rposition(|c| c.step <= target).unwrap_or(0);
        driver.checkpoints.truncate(index + 1);
        let checkpoint = &driver.checkpoints[index];
        driver.world = checkpoint.world.clone();
        driver.log = checkpoint.log.clone();
        driver.step = checkpoint.step;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("flow rewind") });
        for (id, layer) in &self.layers {
            if let Some(snapshot) = checkpoint.layers.get(id) {
                snapshot.restore(&mut encoder, layer);
            }
        }
        queue.submit([encoder.finish()]);
    }

    /// Runs up to [`BATCH_STEPS`] steps towards `target` and sends them to the GPU.
    fn run_batch(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, target: u64, source: &FlowSource) {
        let Self { inputs, sdf, pipelines, driver, layers, steps_run, checkpoint_bytes, .. } = self;
        let driver = driver.as_mut().unwrap();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("flow steps") });
        let mut slot = 0;
        while slot < BATCH_STEPS && driver.step < target {
            driver.step += 1;
            let step = driver.step;
            let time = step as f64 / SIM_HZ as f64;
            // The private world runs at the scene's own rate: whole steps up to this one.
            let world_steps = step * driver.world.steps_per_second as u64 / SIM_HZ as u64;
            let before = driver.world.step;
            while driver.world.step < world_steps {
                driver.world.step(source.sims, &mut driver.log);
            }
            let wall = first_wall(&driver.world);
            let bytes =
                step_inputs(&driver.world, time, step, driver.video, &source.ball_colors, &sdf.frame(wall.as_ref()));
            let offset = inputs.write(queue, slot, &bytes);
            let cx = StepCx {
                device,
                queue,
                inputs,
                offset,
                sdf,
                marks: driver.log.marks_between(before, driver.world.step),
                steps_per_second: driver.world.steps_per_second,
                first: slot == 0,
            };
            for layer in layers.values_mut() {
                layer.step(pipelines, &mut encoder, &cx);
            }
            trim_log(&mut driver.log, driver.world.step);
            *steps_run += 1;
            if step.is_multiple_of(driver.interval) && driver.checkpoints.last().is_none_or(|c| c.step < step) {
                let snapshots = layers.iter().map(|(id, l)| (*id, Snapshot::take(device, &mut encoder, l))).collect();
                driver.checkpoints.push(Checkpoint {
                    step,
                    world: driver.world.clone(),
                    log: driver.log.clone(),
                    layers: snapshots,
                });
                let bytes: u64 = driver.checkpoints.iter().map(Checkpoint::bytes).sum();
                if bytes > *checkpoint_bytes && driver.checkpoints.len() > 2 {
                    driver.interval *= 2;
                    let interval = driver.interval;
                    driver.checkpoints.retain(|c| c.step.is_multiple_of(interval));
                }
            }
            slot += 1;
        }
        queue.submit([encoder.finish()]);
    }
}

#[cfg(test)]
mod canvas_tests;

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::Duration;

    use super::inputs::{BallIn, STEP_BYTES, STEP_STRIDE, StepHeader, step_inputs};
    use super::sdf::{MAX_DISTANCE, Sdf, WallFrame};
    use super::*;
    use crate::{
        BallInstance, Flock, Frame, GpuLiquid, LiquidLayer, Points, Renderer, SwarmLayer, SwarmLook, SwarmStart,
        headless_device,
    };
    use bpe_core::{Resolved, Scene, ThingId, resolve, testkit};
    use bpe_runtime::{Bodies, Outline, Sims, World};
    use glam::Vec2;

    pub(super) fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
        match headless_device() {
            Ok(pair) => Some(pair),
            Err(_) => {
                eprintln!("no GPU adapter available, skipping");
                None
            }
        }
    }

    /// Puts a wall around the origin and sets the gravity, standing in for the arena.
    struct TestArena(Outline, Vec2);
    impl bpe_runtime::ThingSim for TestArena {
        fn kind(&self) -> &'static str {
            "balls"
        }
        fn build(&self, thing: &bpe_core::Thing, _: &bpe_core::ResolvedThing, world: &mut World) {
            world.gravity = self.1;
            world.walls.insert(
                thing.id,
                bpe_runtime::Wall { outline: self.0, spin: 0.0, growth: 0.0, velocity: glam::Vec2::ZERO },
            );
        }
    }

    fn ring_sims(radius: f32) -> Sims {
        let ring = Outline::Ring { center: Vec2::ZERO, radius, sides: 0, angle: 0.0, thickness: 4.0 };
        let mut sims = Sims::default();
        sims.add_thing(TestArena(ring, Vec2::ZERO));
        sims
    }

    /// A box wall 800 wide and 1400 tall with gravity pulling down at 900 px/s².
    fn box_sims() -> Sims {
        let b = Outline::Box { center: Vec2::ZERO, half: Vec2::new(400.0, 700.0), angle: 0.0, thickness: 4.0 };
        let mut sims = Sims::default();
        sims.add_thing(TestArena(b, Vec2::new(0.0, 900.0)));
        sims
    }

    /// A pool filling the bottom 35% of the box.
    fn gpu_pool(thing: ThingId, count: u32) -> LiquidLayer {
        LiquidLayer {
            thing,
            points: Points::Gpu(GpuLiquid {
                count,
                region: [-400.0, 210.0, 400.0, 700.0],
                pour_seconds: 0.0,
                pour_from: [0.0, -600.0],
                seed: 1,
                viscosity: 0.2,
                stiffness: 0.5,
                ball_push: 1.0,
                gravity_scale: 1.0,
                bounciness: 0.0,
            }),
            radius: 3.0,
            color: [0.1, 0.5, 1.0, 1.0],
            follow: 0,
            paint: crate::PaintGpu::default(),
            paint_place: [0.0; 4],
            clarity: 0.45,
            refraction: 0.5,
            highlight: 0.8,
            smoothing: 6.0,
            glow: 0.1,
        }
    }

    pub(super) fn tiny_scene() -> (Scene, Resolved) {
        let reg = testkit::registry();
        let mut scene = Scene::empty(&reg);
        testkit::push(&mut scene, "balls", ThingId::ROOT);
        let resolved = resolve(&scene, &reg);
        (scene, resolved)
    }

    fn swarm_layer(thing: ThingId) -> SwarmLayer {
        SwarmLayer {
            thing,
            count: 100,
            start: SwarmStart::Fill,
            center: [0.0; 2],
            seed: 1,
            release_seconds: 0.0,
            look: SwarmLook {
                size: 1.5,
                brightness: 0.25,
                follow: 0,
                paint: crate::PaintGpu::default(),
                // The top speed below reaches the end of the colours.
                place: [0.0, 260.0, 0.0, 0.0],
                color: [1.0; 4],
                vary: [0.0; 4],
            },
            motion: Flock {
                top_speed: 260.0,
                drag: 0.6,
                flow: 1.0,
                swirl_size: 180.0,
                swirl_speed: 0.05,
                flock: 0.4,
                separation: 0.3,
                attraction: 0.6,
                reach: 220.0,
                avoid_walls: 1.0,
                gravity_scale: 0.0,
            },
        }
    }

    #[test]
    fn the_sdf_measures_a_ring() {
        let Some((device, queue)) = gpu() else { return };
        // The field is baked in the wall's own frame, so where the ring sits does not matter.
        let ring =
            Outline::Ring { center: Vec2::new(100.0, 50.0), radius: 300.0, sides: 0, angle: 0.3, thickness: 4.0 };
        let mut sdf = Sdf::new(&device);
        let mut encoder = device.create_command_encoder(&Default::default());
        sdf.bake(&device, &queue, &mut encoder, &ring);
        queue.submit([encoder.finish()]);
        let field = sdf.read_back(&device, &queue).unwrap();
        // Open space is positive, walls negative, both clamped at MAX_DISTANCE.
        assert!((field.sample(Vec2::ZERO) - MAX_DISTANCE).abs() < 1e-3, "{}", field.sample(Vec2::ZERO));
        let outside = field.sample(Vec2::new(400.0, 0.0));
        assert!((outside + MAX_DISTANCE).abs() < 1e-3, "{outside}");
        let face = field.sample(Vec2::new(299.0, 0.0));
        assert!(face.abs() < 2.0, "{face}");
        let face = field.sample(Vec2::new(0.0, -299.0));
        assert!(face.abs() < 2.0, "{face}");
        assert!(field.sample(Vec2::new(250.0, 0.0)) > 40.0);
        // The live wall's frame: its centre and angle, and how much it has grown since the bake.
        let grown = Outline::Ring { center: Vec2::new(1.0, 2.0), radius: 150.0, sides: 0, angle: 0.3, thickness: 4.0 };
        let WallFrame { wall, more } = sdf.frame(Some(&grown));
        assert_eq!(&wall[..2], &[1.0, 2.0]);
        assert!((wall[2] - 0.3f32.cos()).abs() < 1e-6 && (wall[3] - 0.3f32.sin()).abs() < 1e-6);
        assert_eq!(more, [0.5, 1.0, 0.0, 0.0]);
        assert_eq!(sdf.frame(None).more[1], 0.0, "no wall: the field is not read");
    }

    #[test]
    fn the_sdf_measures_a_capsule_like_the_processor() {
        // Stage 3's rounded arenas: the baked field and the processor's distance must agree,
        // or a GPU liquid would settle against a wall the balls do not see.
        let Some((device, queue)) = gpu() else { return };
        let capsule = Outline::Rounded {
            center: Vec2::new(30.0, -20.0),
            half: Vec2::new(200.0, 400.0),
            corner: 200.0,
            angle: 0.4,
            thickness: 4.0,
        };
        let mut sdf = Sdf::new(&device);
        let mut encoder = device.create_command_encoder(&Default::default());
        sdf.bake(&device, &queue, &mut encoder, &capsule);
        queue.submit([encoder.finish()]);
        let field = sdf.read_back(&device, &queue).unwrap();
        // The field is baked unturned at the origin; compare in that frame.
        let own = sdf::normalized(&capsule);
        for local in [
            Vec2::new(150.0, 0.0),
            Vec2::new(0.0, 350.0),
            Vec2::new(120.0, 300.0),
            Vec2::new(-170.0, -250.0),
            Vec2::new(190.0, 390.0),
        ] {
            let cpu = liquid::closed_distance(&own, local).clamp(-MAX_DISTANCE, MAX_DISTANCE);
            let baked = field.sample(local);
            assert!((cpu - baked).abs() < 3.0, "{local}: processor {cpu}, field {baked}");
        }
        // Inside the end cap the distance is to the arc, not to the box: (0, 350) is 48 in.
        assert!((field.sample(Vec2::new(0.0, 350.0)) - 48.0).abs() < 3.0, "{}", field.sample(Vec2::new(0.0, 350.0)));
        // The corner of the enclosing box is outside the capsule.
        assert!(field.sample(Vec2::new(190.0, 390.0)) < 0.0);
    }

    #[test]
    fn the_driver_follows_time_and_rebuilds_on_a_new_key() {
        let Some((device, queue)) = gpu() else { return };
        let (scene, resolved) = tiny_scene();
        let sims = Sims::default();
        let mut frame = Frame::default();
        frame.swarms.push(swarm_layer(ThingId(1)));
        let mut source = FlowSource {
            picture_folder: None,
            scene: &scene,
            resolved: &resolved,
            sims: &sims,
            key: 1,
            budget: None,
            ball_colors: BTreeMap::new(),
        };
        let mut flow = FlowLayers::new(&device);
        flow.prepare(&device, &queue, &frame, &source);
        assert_eq!(flow.step(), 0);
        assert!(flow.step_to(&device, &queue, 3.0, &source).caught_up);
        assert_eq!((flow.step(), flow.steps_run()), (360, 360));
        // Back to 2.5 s: the checkpoint at step 240 is restored and 60 steps run again.
        assert!(flow.step_to(&device, &queue, 2.5, &source).caught_up);
        assert_eq!((flow.step(), flow.steps_run()), (300, 420));
        // The same key keeps the state; a new key starts over.
        flow.prepare(&device, &queue, &frame, &source);
        assert_eq!(flow.step(), 300);
        source.key = 2;
        flow.prepare(&device, &queue, &frame, &source);
        assert_eq!(flow.step(), 0);
        // A spent budget stops between batches.
        source.budget = Some(Duration::ZERO);
        let status = flow.step_to(&device, &queue, 10.0, &source);
        assert!(!status.caught_up);
        assert!(flow.step() >= BATCH_STEPS as u64 && flow.step() < 1200, "{}", flow.step());
        // Without GPU layers in the frame there is nothing to step.
        frame.swarms.clear();
        flow.prepare(&device, &queue, &frame, &source);
        assert_eq!(flow.step(), 0);
        assert!(flow.step_to(&device, &queue, 10.0, &source).caught_up);
        assert_eq!(flow.step(), 0);
    }

    #[test]
    fn inputs_pack_the_first_balls_of_each_thing() {
        let (scene, resolved) = tiny_scene();
        let sims = Sims::default();
        let frame = WallFrame::default();
        let mut world = World::build(&scene, &resolved, &sims);
        let mut many = Bodies::default();
        for i in 0..70 {
            many.push(Vec2::new(i as f32, 0.0), Vec2::X, 5.0);
        }
        world.bodies.insert(ThingId(1), many);
        let bytes = step_inputs(&world, 1.5, 180, (1080, 1920), &BTreeMap::new(), &frame);
        assert_eq!(bytes.len() as u64, STEP_BYTES);
        const { assert!(STEP_STRIDE.is_multiple_of(256) && STEP_STRIDE >= STEP_BYTES) };
        let header: StepHeader = *bytemuck::from_bytes(&bytes[..std::mem::size_of::<StepHeader>()]);
        assert_eq!(header.a, [1.5, 1.0 / SIM_HZ as f32, 180.0, MAX_BALLS as f32]);
        assert_eq!(header.b, [0.0, 0.0, 540.0, 960.0]);
        let balls: &[BallIn] = bytemuck::cast_slice(&bytes[std::mem::size_of::<StepHeader>()..]);
        assert_eq!(balls.len(), MAX_BALLS);
        assert_eq!(balls[0].place, [0.0, 0.0, 5.0, 0.0]);
        assert_eq!(balls[63].place, [63.0, 0.0, 5.0, 0.0]);
        assert_eq!(balls[0].velocity, [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(balls[0].color, [1.0; 4], "white without a colour");

        // Things pack in id order; silent and dead bodies are left out.
        let mut world = World::build(&scene, &resolved, &sims);
        let mut few = Bodies::default();
        for i in 0..10 {
            few.push(Vec2::new(i as f32, 0.0), Vec2::ZERO, 5.0);
        }
        few.alive[3] = false;
        world.bodies.insert(ThingId(1), few);
        let mut one = Bodies::default();
        one.push(Vec2::new(-1.0, 7.0), Vec2::ZERO, 9.0);
        world.bodies.insert(ThingId(2), one);
        let mut liquid = Bodies { silent: true, ..Default::default() };
        liquid.push(Vec2::ZERO, Vec2::ZERO, 2.0);
        world.bodies.insert(ThingId(3), liquid);
        let colors = BTreeMap::from([(ThingId(2), [1.0, 0.0, 0.0, 1.0])]);
        let bytes = step_inputs(&world, 0.0, 0, (1080, 1920), &colors, &frame);
        let header: StepHeader = *bytemuck::from_bytes(&bytes[..std::mem::size_of::<StepHeader>()]);
        assert_eq!(header.a[3], 10.0);
        let balls: &[BallIn] = bytemuck::cast_slice(&bytes[std::mem::size_of::<StepHeader>()..]);
        assert_eq!(balls[8].place, [9.0, 0.0, 5.0, 0.0]);
        assert_eq!(balls[9].place, [-1.0, 7.0, 9.0, 0.0]);
        assert_eq!(balls[9].color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(balls[10], BallIn::default());
    }

    #[test]
    fn render_with_flow_on_a_frame_without_layers_matches_render() {
        let Some((device, queue)) = gpu() else { return };
        let mut renderer = Renderer::new(&device, 90, 160);
        let mut frame = Frame::default();
        frame.balls.push(BallInstance {
            center: [0.0; 2],
            radius: 100.0,
            glow: 0.5,
            color: [1.0, 0.5, 0.2, 1.0],
            code: [0.0; 4],
            paint: crate::PaintGpu::default(),
        });
        frame.bloom = 0.3;
        frame.time = 1.0;
        renderer.render(&device, &queue, &frame);
        let plain = renderer.read_output(&device, &queue).unwrap();
        let (scene, resolved) = tiny_scene();
        let sims = Sims::default();
        let source = FlowSource {
            picture_folder: None,
            scene: &scene,
            resolved: &resolved,
            sims: &sims,
            key: 1,
            budget: None,
            ball_colors: BTreeMap::new(),
        };
        let status = renderer.render_with_flow(&device, &queue, &frame, &source);
        assert!(status.caught_up);
        assert_eq!(renderer.read_output(&device, &queue).unwrap(), plain);
        assert_eq!(renderer.flow().unwrap().steps_run(), 0);
    }

    #[test]
    fn balls_blending_with_each_other_draw_with_flow_and_no_swarm() {
        // The exporter always draws with flow; a layer of balls blending with each other has no
        // swarm, so no swarm pipeline was made for its mode.
        let Some((device, queue)) = gpu() else { return };
        let mut renderer = Renderer::new(&device, 90, 160);
        let mut layer = Frame::default();
        layer.balls.push(BallInstance { radius: 30.0, color: [0.3, 0.3, 0.3, 1.0], ..Default::default() });
        let mut frame = Frame::default();
        frame.layers.push(crate::BlendLayer { blend: bpe_core::Blend::Screen, amount: 1.0, each: true, frame: layer });
        let (scene, resolved) = tiny_scene();
        let sims = Sims::default();
        let source = FlowSource {
            picture_folder: None,
            scene: &scene,
            resolved: &resolved,
            sims: &sims,
            key: 1,
            budget: None,
            ball_colors: BTreeMap::new(),
        };
        assert!(renderer.render_with_flow(&device, &queue, &frame, &source).caught_up);
        let pixels = renderer.read_output(&device, &queue).unwrap();
        let i = ((80 * 90 + 45) * 4) as usize;
        assert!(pixels[i] > 20, "the ball shows: {:?}", &pixels[i..i + 4]);
    }

    #[test]
    fn a_swarm_stays_inside_the_wall_and_is_deterministic() {
        let Some((device, queue)) = gpu() else { return };
        let (scene, resolved) = tiny_scene();
        let sims = ring_sims(400.0);
        let mut frame = Frame::default();
        let mut layer = swarm_layer(ThingId(1));
        layer.count = 20_000;
        frame.swarms.push(layer);
        let source = FlowSource {
            picture_folder: None,
            scene: &scene,
            resolved: &resolved,
            sims: &sims,
            key: 7,
            budget: None,
            ball_colors: BTreeMap::new(),
        };
        let run = || {
            let mut flow = FlowLayers::new(&device);
            flow.prepare(&device, &queue, &frame, &source);
            assert!(flow.step_to(&device, &queue, 1.0, &source).caught_up);
            flow.swarm(ThingId(1)).unwrap().read_points(&device, &queue)
        };
        let points = run();
        assert_eq!(points.len(), 20_000);
        let inside = points.iter().filter(|p| Vec2::new(p[0], p[1]).length() <= 402.0).count();
        assert!(points.iter().all(|p| p.iter().all(|v| v.is_finite())));
        assert!(inside >= 19_800, "{inside} of 20000 inside the ring");
        let moving = points.iter().filter(|p| p[2] != 0.0 || p[3] != 0.0).count();
        assert!(moving > 19_000, "{moving} moving");
        assert_eq!(run(), points, "a second run is the same");
    }

    #[test]
    fn swarm_physics_ignores_camera() {
        let Some((device, queue)) = gpu() else { return };
        let (scene, resolved) = tiny_scene();
        let sims = ring_sims(400.0);
        let source = FlowSource {
            picture_folder: None,
            scene: &scene,
            resolved: &resolved,
            sims: &sims,
            key: 7,
            budget: None,
            ball_colors: BTreeMap::new(),
        };
        let run = |camera: bpe_runtime::CameraView| {
            let mut frame = Frame { camera, ..Frame::default() };
            let mut layer = swarm_layer(ThingId(1));
            layer.count = 2_000;
            frame.swarms.push(layer);
            let mut flow = FlowLayers::new(&device);
            flow.prepare(&device, &queue, &frame, &source);
            assert!(flow.step_to(&device, &queue, 0.5, &source).caught_up);
            flow.swarm(ThingId(1)).unwrap().read_points(&device, &queue)
        };
        let moved = bpe_runtime::CameraView { center: Vec2::new(400.0, -300.0), zoom: 1.7, turn: 0.3 };
        assert_eq!(run(bpe_runtime::CameraView::default()), run(moved));
    }

    #[test]
    fn a_wider_picture_keeps_the_layers() {
        let Some((device, queue)) = gpu() else { return };
        let (scene, resolved) = tiny_scene();
        let sims = ring_sims(400.0);
        let source = FlowSource {
            picture_folder: None,
            scene: &scene,
            resolved: &resolved,
            sims: &sims,
            key: 7,
            budget: None,
            ball_colors: BTreeMap::new(),
        };
        let mut frame = Frame::default();
        frame.swarms.push(swarm_layer(ThingId(1)));
        let mut flow = FlowLayers::new(&device);
        flow.prepare(&device, &queue, &frame, &source);
        assert!(flow.step_to(&device, &queue, 0.5, &source).caught_up);
        let stepped = flow.step();
        // The World view draws a picture the panel's shape; the video, and so the layers, stay.
        frame.width = 2400;
        flow.prepare(&device, &queue, &frame, &source);
        assert_eq!(flow.step(), stepped, "the layers started over");
    }

    #[test]
    fn a_swarm_draws_light() {
        let Some((device, queue)) = gpu() else { return };
        let (scene, resolved) = tiny_scene();
        let sims = ring_sims(400.0);
        let mut renderer = Renderer::new(&device, 270, 480);
        let mut frame = Frame { time: 0.5, ..Frame::default() };
        let mut layer = swarm_layer(ThingId(1));
        layer.count = 20_000;
        layer.look.brightness = 0.8;
        layer.look.size = 2.0;
        frame.swarms.push(layer);
        let source = FlowSource {
            picture_folder: None,
            scene: &scene,
            resolved: &resolved,
            sims: &sims,
            key: 7,
            budget: None,
            ball_colors: BTreeMap::new(),
        };
        let bright = |pixels: &[u8]| pixels.chunks(4).filter(|p| p[0] > 60 || p[1] > 60 || p[2] > 60).count();
        renderer.render_with_flow(&device, &queue, &frame, &source);
        let lit = renderer.read_output(&device, &queue).unwrap();
        assert!(bright(&lit) > 500, "{}", bright(&lit));
        // Nothing lights up outside the ring.
        let outside = lit.chunks(4).enumerate().filter(|(i, p)| {
            let (x, y) = ((i % 270) as f32 * 4.0 - 540.0, (i / 270) as f32 * 4.0 - 960.0);
            (x * x + y * y).sqrt() > 420.0 && (p[0] > 60 || p[1] > 60 || p[2] > 60)
        });
        assert_eq!(outside.count(), 0);
        // Brightness 0 draws nothing, and only the look changed: the swarm kept going.
        frame.swarms[0].look.brightness = 0.0;
        renderer.render_with_flow(&device, &queue, &frame, &source);
        assert_eq!(bright(&renderer.read_output(&device, &queue).unwrap()), 0);
        assert_eq!(renderer.flow().unwrap().steps_run(), 60);
    }

    #[test]
    fn a_swarm_in_a_blend_layer_is_stepped_and_drawn() {
        swarm_in_a_layer(false);
    }

    #[test]
    fn a_swarm_blending_with_each_other_is_drawn() {
        swarm_in_a_layer(true);
    }

    fn swarm_in_a_layer(each: bool) {
        let Some((device, queue)) = gpu() else { return };
        let (scene, resolved) = tiny_scene();
        let sims = ring_sims(400.0);
        let mut renderer = Renderer::new(&device, 270, 480);
        let mut frame = Frame { time: 0.5, ..Frame::default() };
        let mut layer = swarm_layer(ThingId(1));
        layer.count = 20_000;
        layer.look.brightness = 0.8;
        layer.look.size = 2.0;
        frame.layer_mut(bpe_core::Blend::Screen, 1.0, each).swarms.push(layer);
        assert_eq!(frame.layers[0].each, each);
        assert!(frame.swarms.is_empty());
        let source = FlowSource {
            picture_folder: None,
            scene: &scene,
            resolved: &resolved,
            sims: &sims,
            key: 7,
            budget: None,
            ball_colors: BTreeMap::new(),
        };
        renderer.render_with_flow(&device, &queue, &frame, &source);
        let flow = renderer.flow().unwrap();
        assert!(flow.swarm(ThingId(1)).is_some(), "the layer's swarm has a GPU layer");
        assert_eq!(flow.steps_run(), 60);
        let lit = renderer.read_output(&device, &queue).unwrap();
        let bright = lit.chunks(4).filter(|p| p[0] > 60 || p[1] > 60 || p[2] > 60).count();
        assert!(bright > 500, "{bright}: screened over black, the swarm's light shows");
    }

    #[test]
    fn changing_only_the_look_keeps_the_layer() {
        let Some((device, queue)) = gpu() else { return };
        let (scene, resolved) = tiny_scene();
        let sims = Sims::default();
        let mut frame = Frame::default();
        frame.swarms.push(swarm_layer(ThingId(1)));
        let source = FlowSource {
            picture_folder: None,
            scene: &scene,
            resolved: &resolved,
            sims: &sims,
            key: 1,
            budget: None,
            ball_colors: BTreeMap::new(),
        };
        let mut flow = FlowLayers::new(&device);
        flow.prepare(&device, &queue, &frame, &source);
        flow.step_to(&device, &queue, 1.0, &source);
        assert_eq!(flow.step(), 120);
        frame.swarms[0].look.brightness = 0.9;
        frame.swarms[0].look.follow = 2;
        frame.swarms[0].look.color = [1.0, 0.0, 0.0, 1.0];
        flow.prepare(&device, &queue, &frame, &source);
        assert_eq!((flow.step(), flow.steps_run()), (120, 120));
        frame.swarms[0].motion.flow = 2.0;
        flow.prepare(&device, &queue, &frame, &source);
        assert_eq!(flow.step(), 0, "a motion change starts the swarm over");
    }

    #[test]
    fn a_gpu_liquid_settles_inside_its_wall() {
        let Some((device, queue)) = gpu() else { return };
        let (scene, resolved) = tiny_scene();
        let sims = box_sims();
        let mut frame = Frame::default();
        frame.liquids.push(gpu_pool(ThingId(1), 12_000));
        let source = FlowSource {
            picture_folder: None,
            scene: &scene,
            resolved: &resolved,
            sims: &sims,
            key: 3,
            budget: None,
            ball_colors: BTreeMap::new(),
        };
        let mut flow = FlowLayers::new(&device);
        flow.prepare(&device, &queue, &frame, &source);
        let liquid = flow.liquid(ThingId(1)).unwrap();
        assert_eq!(liquid.count(), 12_000);
        let start = liquid.read_state(&device, &queue);
        assert!(start.iter().all(|p| p[1] >= 200.0 && p[0].abs() <= 400.0), "the pool starts in its region");
        assert!(flow.step_to(&device, &queue, 1.0, &source).caught_up);
        let state = flow.liquid(ThingId(1)).unwrap().read_state(&device, &queue);
        assert_eq!(state.len(), 12_000);
        assert!(state.iter().all(|p| p.iter().all(|v| v.is_finite())));
        let inside = state.iter().filter(|p| p[0].abs() <= 399.0 && p[1].abs() <= 699.0).count();
        assert!(inside >= 11_940, "{inside} of 12000 inside the box");
        let fast = state.iter().filter(|p| (p[2] * p[2] + p[3] * p[3]).sqrt() > 3000.0).count();
        assert!(fast <= 120, "{fast} particles racing");
        // Gravity pulled the pool down: its top is lower than where the lattice began.
        let top = state.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
        assert!(top > 180.0, "{top}");
    }

    #[test]
    fn seeking_back_restores_a_checkpoint_bit_for_bit() {
        let Some((device, queue)) = gpu() else { return };
        let (scene, resolved) = tiny_scene();
        let sims = box_sims();
        let mut frame = Frame::default();
        frame.liquids.push(gpu_pool(ThingId(1), 4_000));
        let source = FlowSource {
            picture_folder: None,
            scene: &scene,
            resolved: &resolved,
            sims: &sims,
            key: 3,
            budget: None,
            ball_colors: BTreeMap::new(),
        };
        let mut flow = FlowLayers::new(&device);
        flow.prepare(&device, &queue, &frame, &source);
        flow.step_to(&device, &queue, 3.0, &source);
        let straight = flow.liquid(ThingId(1)).unwrap().read_state(&device, &queue);
        flow.step_to(&device, &queue, 2.5, &source);
        assert_eq!(flow.step(), 300);
        flow.step_to(&device, &queue, 3.0, &source);
        let again = flow.liquid(ThingId(1)).unwrap().read_state(&device, &queue);
        assert_eq!(again, straight);
        assert_eq!(flow.steps_run(), 360 + 120, "back to the checkpoint at 2 s, then 60 steps twice");
    }

    #[test]
    fn checkpoints_stay_under_the_budget() {
        let Some((device, queue)) = gpu() else { return };
        let (scene, resolved) = tiny_scene();
        let sims = box_sims();
        let mut frame = Frame::default();
        frame.liquids.push(gpu_pool(ThingId(1), 10_000));
        let source = FlowSource {
            picture_folder: None,
            scene: &scene,
            resolved: &resolved,
            sims: &sims,
            key: 3,
            budget: None,
            ball_colors: BTreeMap::new(),
        };
        let mut flow = FlowLayers::new(&device);
        // 160 KB per checkpoint; the budget allows about six.
        flow.set_checkpoint_budget(1 << 20);
        flow.prepare(&device, &queue, &frame, &source);
        flow.step_to(&device, &queue, 20.0, &source);
        let (kept, interval, bytes) = flow.checkpoint_stats();
        assert!(kept > 1, "{kept}");
        assert!(bytes <= 1 << 20, "{bytes}");
        assert!(interval > CHECKPOINT_STEPS, "{interval}");
        // Every kept checkpoint sits on the interval, so seeking back still finds one.
        flow.step_to(&device, &queue, 10.0, &source);
        assert_eq!(flow.step(), 1200);
    }

    /// A gradient of `colours`, evenly spaced and smooth, baked into `frame`'s atlas.
    fn paint_of(frame: &mut Frame, colours: &[bpe_core::Color]) -> crate::PaintGpu {
        let row = frame.gradients.row(&bpe_core::Gradient::even(colours));
        crate::PaintGpu { row: row as f32, ..crate::PaintGpu::default() }
    }

    #[test]
    fn liquid_place_paints_top_and_bottom() {
        let Some((device, queue)) = gpu() else { return };
        let (scene, resolved) = tiny_scene();
        let sims = box_sims();
        let mut renderer = Renderer::new(&device, 270, 480);
        let mut frame = Frame { bloom: 0.0, ..Frame::default() };
        let blue_red = [bpe_core::Color::rgb(0, 0, 255), bpe_core::Color::rgb(255, 0, 0)];
        let mut pool = gpu_pool(ThingId(1), 12_000);
        // Green, so a surface that ignores its paint fails.
        pool.color = [0.0, 1.0, 0.0, 1.0];
        pool.clarity = 0.0;
        pool.follow = 1;
        pool.paint = paint_of(&mut frame, &blue_red);
        // Straight down, from the top of the pool (y 210) to its floor (y 700).
        pool.paint_place = [90.0, 490.0, 0.0, 455.0];
        frame.liquids.push(pool);
        let source = FlowSource {
            picture_folder: None,
            scene: &scene,
            resolved: &resolved,
            sims: &sims,
            key: 3,
            budget: None,
            ball_colors: BTreeMap::new(),
        };
        renderer.render_with_flow(&device, &queue, &frame, &source);
        let pixels = renderer.read_output(&device, &queue).unwrap();
        // Four world pixels per output pixel, the world's centre in the picture's centre.
        let at = |x: f32, y: f32| {
            let i = ((((y / 4.0) as u32 + 240) * 270 + (x / 4.0) as u32 + 135) * 4) as usize;
            [pixels[i], pixels[i + 1], pixels[i + 2]]
        };
        let (top, bottom) = (at(0.0, 300.0), at(0.0, 650.0));
        assert!(top[1] < 60 && bottom[1] < 60, "{top:?} {bottom:?}: green is the liquid's own colour");
        assert!(top[2] > top[0], "{top:?}: the top is bluer");
        assert!(bottom[0] > bottom[2], "{bottom:?}: the floor is redder");
        assert!(top[2] > bottom[2] && bottom[0] > top[0], "{top:?} {bottom:?}");
    }

    #[test]
    fn swarm_points_vary() {
        let Some((device, queue)) = gpu() else { return };
        let (scene, resolved) = tiny_scene();
        let sims = ring_sims(400.0);
        let mut renderer = Renderer::new(&device, 270, 480);
        let mut frame = Frame { time: 0.5, bloom: 0.0, ..Frame::default() };
        let mut layer = swarm_layer(ThingId(1));
        layer.count = 400;
        layer.look.brightness = 0.3;
        layer.look.size = 12.0;
        // Every point one dull red.
        layer.look.follow = 5;
        layer.look.color = [0.5, 0.0, 0.0, 1.0];
        frame.swarms.push(layer);
        let source = FlowSource {
            picture_folder: None,
            scene: &scene,
            resolved: &resolved,
            sims: &sims,
            key: 7,
            budget: None,
            ball_colors: BTreeMap::new(),
        };
        let mut draw = |vary: [f32; 4], time: f32| {
            frame.time = time;
            frame.swarms[0].look.vary = vary;
            renderer.render_with_flow(&device, &queue, &frame, &source);
            renderer.read_output(&device, &queue).unwrap()
        };
        let plain = draw([0.0; 4], 0.5);
        let lit = |p: &[u8]| p[0] > 30;
        assert!(plain.chunks(4).filter(|p| lit(p)).count() > 200);
        assert!(plain.chunks(4).all(|p| p[1] < 8 && p[2] < 8), "no variation: every point red");
        // The look alone changed: the points are where they were, each brighter or darker.
        let bright = draw([0.0, 0.0, 1.0, 0.0], 0.5);
        let ratios: Vec<f32> = plain
            .chunks(4)
            .zip(bright.chunks(4))
            .filter(|(a, _)| lit(a))
            .map(|(a, b)| b[0] as f32 / a[0] as f32)
            .collect();
        let (lo, hi) = ratios.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &r| (lo.min(r), hi.max(r)));
        assert!(lo < 0.7 && hi > 1.3, "brightness spread across points: {lo} to {hi}");
        // A hue turn of up to 120 degrees shows green and blue points.
        let turned = draw([120.0, 0.0, 0.0, 0.0], 0.5);
        assert!(turned.chunks(4).any(|p| p[1] > p[0] + 20), "some points turn green");
        assert!(turned.chunks(4).any(|p| p[2] > p[0] + 20), "some points turn blue");
        // Wobbling, the colours move with the frame's time.
        let still = draw([120.0, 0.0, 0.0, 3.0], 0.5);
        let later = draw([120.0, 0.0, 0.0, 3.0], 0.6);
        assert_ne!(still, later);
    }

    #[test]
    fn swarm_speed_uses_the_gradient() {
        let Some((device, queue)) = gpu() else { return };
        let (scene, resolved) = tiny_scene();
        let sims = ring_sims(400.0);
        let mut renderer = Renderer::new(&device, 270, 480);
        let mut frame = Frame { time: 0.5, bloom: 0.0, ..Frame::default() };
        let mut layer = swarm_layer(ThingId(1));
        layer.count = 20_000;
        // Dim enough that piled-up points do not burn to white.
        layer.look.brightness = 0.15;
        layer.look.size = 2.0;
        // Blue, so a swarm that ignores its paint fails.
        layer.look.color = [0.0, 0.0, 1.0, 1.0];
        layer.look.follow = 0;
        layer.look.paint = paint_of(&mut frame, &[bpe_core::Color::rgb(255, 0, 0), bpe_core::Color::rgb(0, 255, 0)]);
        frame.swarms.push(layer);
        let source = FlowSource {
            picture_folder: None,
            scene: &scene,
            resolved: &resolved,
            sims: &sims,
            key: 7,
            budget: None,
            ball_colors: BTreeMap::new(),
        };
        renderer.render_with_flow(&device, &queue, &frame, &source);
        let pixels = renderer.read_output(&device, &queue).unwrap();
        let lit: Vec<&[u8]> = pixels.chunks(4).filter(|p| p[0] > 60 || p[1] > 60 || p[2] > 60).collect();
        assert!(lit.len() > 500, "{}", lit.len());
        let blue: Vec<_> = lit.iter().filter(|p| p[2] >= 40).take(10).collect();
        assert!(blue.is_empty(), "no blue: the gradient runs red to green: {blue:?} of {}", lit.len());
        // Slow points are redder, fast ones greener: both show.
        assert!(lit.iter().any(|p| p[0] > p[1] + 40), "some slow red points");
        assert!(lit.iter().any(|p| p[1] > p[0] + 40), "some fast green points");
    }

    #[test]
    fn swarm_speed_runs_over_the_span() {
        let Some((device, queue)) = gpu() else { return };
        let (scene, resolved) = tiny_scene();
        let sims = ring_sims(400.0);
        let source = FlowSource {
            picture_folder: None,
            scene: &scene,
            resolved: &resolved,
            sims: &sims,
            key: 7,
            budget: None,
            ball_colors: BTreeMap::new(),
        };
        let mut renderer = Renderer::new(&device, 270, 480);
        // How many lit pixels are mostly red (slow) and mostly green (fast) with this span.
        let mut tally = |span: f32| {
            let mut frame = Frame { time: 0.5, bloom: 0.0, ..Frame::default() };
            let mut layer = swarm_layer(ThingId(1));
            layer.count = 20_000;
            layer.look.brightness = 0.15;
            layer.look.size = 2.0;
            layer.look.paint =
                paint_of(&mut frame, &[bpe_core::Color::rgb(255, 0, 0), bpe_core::Color::rgb(0, 255, 0)]);
            layer.look.place = [0.0, span, 0.0, 0.0];
            frame.swarms.push(layer);
            renderer.render_with_flow(&device, &queue, &frame, &source);
            let pixels = renderer.read_output(&device, &queue).unwrap();
            let lit: Vec<&[u8]> = pixels.chunks(4).filter(|p| p[0] > 60 || p[1] > 60).collect();
            let red = lit.iter().filter(|p| p[0] as i32 > p[1] as i32 + 40).count();
            let green = lit.iter().filter(|p| p[1] as i32 > p[0] as i32 + 40).count();
            (red, green, lit.len())
        };
        let (red, green, lit) = tally(130.0);
        assert!(red > 50 && green > 50, "{red} red, {green} green of {lit}");
        // Twice the span halves every point's place: more red, less green.
        let (red2, green2, _) = tally(260.0);
        assert!(red2 > red && green2 < green, "{red} -> {red2} red, {green} -> {green2} green");
        // Far past the fastest point, everything is the first colour; far below it, the last.
        let (_, green_far, lit_far) = tally(1e6);
        assert_eq!(green_far, 0, "{lit_far} lit");
        let (red_near, _, lit_near) = tally(1.0);
        assert_eq!(red_near, 0, "{lit_near} lit");
    }

    /// The canvas the marking sims below mark.
    pub(super) const CANVAS: ThingId = ThingId(9);
    /// Its side in world pixels and texels: rows of 384 Rgba16Float texels read back unpadded.
    pub(super) const SIDE: u32 = 384;

    /// Leaves `marks(step)` on [`CANVAS`] after every step, standing in for the canvas sim.
    pub(super) struct Marking(pub(super) fn(u64) -> Vec<bpe_runtime::Mark>);
    impl bpe_runtime::ThingSim for Marking {
        fn kind(&self) -> &'static str {
            "balls"
        }
        fn build(&self, _: &bpe_core::Thing, _: &bpe_core::ResolvedThing, _: &mut World) {}
        fn after_step(&self, _: ThingId, world: &mut World, log: &mut bpe_runtime::Log) {
            for m in (self.0)(world.step) {
                log.push_mark(m);
            }
        }
    }

    pub(super) fn mark(
        step: u64,
        shape: bpe_runtime::MarkShape,
        from: Vec2,
        to: Vec2,
        radius: f32,
    ) -> bpe_runtime::Mark {
        bpe_runtime::Mark {
            step,
            target: bpe_runtime::MarkTarget::Canvas(CANVAS),
            shape,
            from,
            to,
            radius,
            radius_to: radius,
            angle: 0.0,
            within: [0.0, 1.0],
            variant: 0,
            colour: [1.0, 0.0, 0.0, 1.0],
            colour_to: [1.0, 0.0, 0.0, 1.0],
        }
    }

    pub(super) fn canvas_layer() -> crate::CanvasLayer {
        crate::CanvasLayer {
            key: MarkTarget::Canvas(CANVAS),
            center: [0.0; 2],
            angle: 0.0,
            size: [SIDE as f32; 2],
            texels: [SIDE; 2],
            place: crate::CanvasPlace::BehindBalls,
            start: None,
            fade: 0.0,
            blend: bpe_core::Blend::Normal,
            opacity: 1.0,
            time: 0.0,
            mask: Vec::new(),
        }
    }

    pub(super) fn f16_to_f32(bits: u16) -> f32 {
        let exponent = ((bits >> 10) & 0x1f) as i32;
        let mantissa = (bits & 0x3ff) as f32;
        let value = if exponent == 0 {
            mantissa * 2f32.powi(-24)
        } else {
            (1.0 + mantissa / 1024.0) * 2f32.powi(exponent - 15)
        };
        if bits & 0x8000 != 0 { -value } else { value }
    }

    /// The raw bytes of a [`SIDE`]-square Rgba16Float texture.
    pub(super) fn read_texture(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) -> Vec<u8> {
        let row = SIDE * 8;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (row * SIDE) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None },
            },
            texture.size(),
        );
        queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        buffer.slice(..).get_mapped_range().unwrap().to_vec()
    }

    /// The texel under canvas point `p` (from the canvas's middle) of a read-back texture.
    pub(super) fn texel(bytes: &[u8], p: Vec2) -> [f32; 4] {
        let (x, y) = ((p.x + SIDE as f32 / 2.0) as usize, (p.y + SIDE as f32 / 2.0) as usize);
        let i = (y * SIDE as usize + x) * 8;
        std::array::from_fn(|c| f16_to_f32(u16::from_le_bytes([bytes[i + c * 2], bytes[i + c * 2 + 1]])))
    }

    pub(super) fn canvas_source<'a>(scene: &'a Scene, resolved: &'a Resolved, sims: &'a Sims) -> FlowSource<'a> {
        FlowSource { scene, resolved, sims, key: 11, budget: None, ball_colors: BTreeMap::new(), picture_folder: None }
    }

    #[test]
    fn canvas_marks_stamp_their_shapes() {
        use bpe_runtime::MarkShape as S;
        let Some((device, queue)) = gpu() else { return };
        let (scene, resolved) = tiny_scene();
        let mut sims = Sims::default();
        sims.add_thing(Marking(|step| {
            if step != 1 {
                return Vec::new();
            }
            let v = Vec2::new;
            let mut square = mark(1, S::Stroke, v(-150.0, -100.0), v(-100.0, -100.0), 10.0);
            square.variant = 1;
            vec![
                mark(1, S::Stroke, v(-150.0, -150.0), v(-100.0, -150.0), 10.0),
                square,
                mark(1, S::Splat, v(0.0, -150.0), v(0.0, -150.0), 30.0),
                mark(1, S::Ring, v(100.0, -150.0), v(100.0, -150.0), 30.0),
                mark(1, S::Star, v(-150.0, 50.0), v(-150.0, 50.0), 30.0),
                mark(1, S::Crack, v(-50.0, 50.0), v(-50.0, 50.0), 30.0),
                mark(1, S::Burst, v(50.0, 50.0), v(50.0, 50.0), 30.0),
                mark(1, S::Dots, v(150.0, 50.0), v(150.0, 50.0), 30.0),
            ]
        }));
        let mut frame = Frame::default();
        frame.canvases.push(canvas_layer());
        let source = canvas_source(&scene, &resolved, &sims);
        let mut flow = FlowLayers::new(&device);
        flow.prepare(&device, &queue, &frame, &source);
        flow.step_to(&device, &queue, 0.1, &source);
        let textures = flow.canvas_textures(MarkTarget::Canvas(CANVAS)).expect("the canvas has textures");
        let paint = read_texture(&device, &queue, &textures.paint);
        let aux = read_texture(&device, &queue, &textures.aux);
        let painted = |p: Vec2| texel(&paint, p)[3] > 0.99;
        let clear = |p: Vec2| texel(&paint, p)[3] == 0.0;
        let v = Vec2::new;
        // A full mark is its colour, premultiplied, and the time it was made.
        assert_eq!(texel(&paint, v(-125.0, -150.0)), [1.0, 0.0, 0.0, 1.0]);
        let time = 1.0 / World::build(&scene, &resolved, &sims).steps_per_second as f32;
        assert!((texel(&aux, v(-125.0, -150.0))[0] - time).abs() < 1e-5, "{:?}", texel(&aux, v(-125.0, -150.0)));
        assert_eq!(texel(&aux, v(0.0, 0.0))[0], 0.0, "unpainted");
        // Round and square tips.
        assert!(clear(v(-92.0, -142.0)) && painted(v(-92.0, -92.0)));
        // A splat covers its middle and stays inside its radius.
        assert!(painted(v(0.0, -150.0)));
        for d in [v(33.0, 0.0), v(-33.0, 0.0), v(0.0, 33.0), v(0.0, -33.0)] {
            assert!(clear(v(0.0, -150.0) + d), "{d}");
        }
        // A ring has a hole.
        assert!(clear(v(100.0, -150.0)) && painted(v(125.5, -150.0)));
        // A star's point is up; between the two lower points there is none.
        assert!(painted(v(-150.0, 50.0)) && painted(v(-150.0, 25.0)) && clear(v(-150.0, 72.0)));
        // A crack starts from a dot; a burst's rays start away from the middle.
        assert!(painted(v(-50.0, 50.0)));
        assert!(clear(v(50.0, 50.0)) && painted(v(68.0, 50.0)));
        // Dots stay inside their radius.
        let inside = (-30..=30)
            .flat_map(|x| (-30..=30).map(move |y| v(x as f32, y as f32)))
            .filter(|d| d.length() < 30.0 && painted(v(150.0, 50.0) + *d))
            .count();
        assert!(inside > 100, "{inside}");
        assert!((0..64).all(|i| clear(v(150.0, 50.0) + Vec2::from_angle(i as f32 * 0.1) * 32.0)));
    }

    #[test]
    fn stroke_colour_runs_from_start_to_end() {
        let Some((device, queue)) = gpu() else { return };
        let (scene, resolved) = tiny_scene();
        let mut sims = Sims::default();
        sims.add_thing(Marking(|step| {
            if step != 1 {
                return Vec::new();
            }
            let mut m = mark(1, bpe_runtime::MarkShape::Stroke, Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0), 10.0);
            m.colour = [1.0, 0.0, 0.0, 1.0];
            m.colour_to = [0.0, 0.0, 1.0, 1.0];
            vec![m]
        }));
        let paint = stamped(&device, &queue, &scene, &resolved, &sims);
        let near = |c: [f32; 4], want: [f32; 4]| c.iter().zip(want).all(|(a, b)| (a - b).abs() < 0.1);
        let v = Vec2::new;
        assert!(near(texel(&paint, v(0.0, 0.0)), [0.5, 0.0, 0.5, 1.0]), "{:?}", texel(&paint, v(0.0, 0.0)));
        assert!(near(texel(&paint, v(-90.0, 0.0)), [0.95, 0.0, 0.05, 1.0]), "{:?}", texel(&paint, v(-90.0, 0.0)));
        assert!(near(texel(&paint, v(90.0, 0.0)), [0.05, 0.0, 0.95, 1.0]), "{:?}", texel(&paint, v(90.0, 0.0)));
    }

    #[test]
    fn smear_tapers() {
        let Some((device, queue)) = gpu() else { return };
        let (scene, resolved) = tiny_scene();
        let mut sims = Sims::default();
        sims.add_thing(Marking(|step| {
            if step != 1 {
                return Vec::new();
            }
            let mut m = mark(1, bpe_runtime::MarkShape::Smear, Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0), 10.0);
            m.radius_to = 0.0;
            vec![m]
        }));
        let paint = stamped(&device, &queue, &scene, &resolved, &sims);
        let v = Vec2::new;
        let covered = |p: Vec2| texel(&paint, p)[3] > 0.99;
        let clear = |p: Vec2| texel(&paint, p)[3] == 0.0;
        assert!(covered(v(-100.0, 0.0)) && covered(v(-100.0, 8.0)), "full width at the start");
        assert!(covered(v(0.0, 3.0)) && clear(v(0.0, 8.0)), "half width in the middle");
        assert!(covered(v(60.0, 0.0)) && clear(v(80.0, 4.0)), "narrow near the end");
        assert!(clear(v(98.0, 3.0)) && clear(v(110.0, 0.0)), "nothing at the tip");
    }

    /// Where the splat tests put their splats: three columns by three rows, 128 apart.
    fn slot(k: usize) -> Vec2 {
        Vec2::new((k % 3) as f32 - 1.0, (k / 3) as f32 - 1.0) * 128.0
    }

    /// A splat of radius 30 in slot `k`.
    fn splat_in(k: usize, angle: f32, variant: u8) -> bpe_runtime::Mark {
        let mut m = mark(1, bpe_runtime::MarkShape::Splat, slot(k), slot(k), 30.0);
        m.angle = angle;
        m.variant = variant;
        m
    }

    /// The coverage (paint alpha) of the 128 by 128 texels around slot `k`, row by row.
    fn slot_cover(paint: &[u8], k: usize) -> Vec<f32> {
        let from = slot(k) - Vec2::splat(64.0);
        (0..128)
            .flat_map(|y| (0..128).map(move |x| (x, y)))
            .map(|(x, y)| texel(paint, from + Vec2::new(x as f32 + 0.5, y as f32 + 0.5))[3])
            .collect()
    }

    /// The blobs of 128 by 128 `cover` that are mostly covered, as lists of texels (x, y).
    fn blobs(cover: &[f32]) -> Vec<Vec<(i32, i32)>> {
        let mut seen = vec![false; cover.len()];
        let mut found = Vec::new();
        for start in 0..cover.len() {
            if seen[start] || cover[start] <= 0.5 {
                continue;
            }
            let mut stack = vec![start];
            let mut blob = Vec::new();
            seen[start] = true;
            while let Some(i) = stack.pop() {
                let (x, y) = ((i % 128) as i32, (i / 128) as i32);
                blob.push((x, y));
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if !(0..128).contains(&nx) || !(0..128).contains(&ny) {
                        continue;
                    }
                    let j = (ny * 128 + nx) as usize;
                    if !seen[j] && cover[j] > 0.5 {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
            found.push(blob);
        }
        found
    }

    #[test]
    fn splat_has_droplets() {
        let Some((device, queue)) = gpu() else { return };
        let (scene, resolved) = tiny_scene();
        let mut sims = Sims::default();
        sims.add_thing(Marking(|step| {
            if step != 1 {
                return Vec::new();
            }
            (0..8).map(|v| splat_in(v, 0.0, v as u8)).collect()
        }));
        let paint = stamped(&device, &queue, &scene, &resolved, &sims);
        for v in 0..8 {
            let found = blobs(&slot_cover(&paint, v));
            assert!(found.len() > 1, "variant {v}: {} blob", found.len());
        }
    }

    #[test]
    fn splat_droplets_lean_forward() {
        let Some((device, queue)) = gpu() else { return };
        let (scene, resolved) = tiny_scene();
        let mut sims = Sims::default();
        sims.add_thing(Marking(|step| {
            if step != 1 {
                return Vec::new();
            }
            vec![splat_in(0, 0.0, 0), splat_in(1, std::f32::consts::FRAC_PI_2, 0), splat_in(2, std::f32::consts::PI, 3)]
        }));
        let paint = stamped(&device, &queue, &scene, &resolved, &sims);
        let centroid = |k: usize| {
            let cover = slot_cover(&paint, k);
            let total: f32 = cover.iter().sum();
            let (mut x, mut y) = (0.0, 0.0);
            for (i, c) in cover.iter().enumerate() {
                x += c * ((i % 128) as f32 + 0.5 - 64.0);
                y += c * ((i / 128) as f32 + 0.5 - 64.0);
            }
            Vec2::new(x, y) / total
        };
        assert!(centroid(0).x > 3.0, "along +x: {}", centroid(0));
        assert!(centroid(1).y > 3.0, "along +y: {}", centroid(1));
        assert!(centroid(2).x < -3.0, "along -x: {}", centroid(2));
    }

    #[test]
    fn squashed_splat_is_flatter() {
        let Some((device, queue)) = gpu() else { return };
        let (scene, resolved) = tiny_scene();
        let mut sims = Sims::default();
        sims.add_thing(Marking(|step| {
            if step != 1 {
                return Vec::new();
            }
            vec![splat_in(0, 0.0, 1), splat_in(1, 0.0, 1 | 3 << 3)]
        }));
        let paint = stamped(&device, &queue, &scene, &resolved, &sims);
        // The main blob is the one under the middle; droplets stand apart from it.
        let extents = |k: usize| {
            let blob = blobs(&slot_cover(&paint, k))
                .into_iter()
                .find(|b| b.contains(&(64, 64)))
                .expect("a blob at the middle");
            let span = |f: fn(&(i32, i32)) -> i32| {
                let (lo, hi) = (blob.iter().map(f).min().unwrap(), blob.iter().map(f).max().unwrap());
                (hi - lo + 1) as f32
            };
            (span(|p| p.0), span(|p| p.1))
        };
        let (along, across) = extents(0);
        assert!((across / along) < 1.25, "a round splat: {along} by {across}");
        let (along, across) = extents(1);
        assert!(across / along >= 1.4, "a squashed splat is {along} along and {across} across");
    }

    #[test]
    fn splat_edges_are_antialiased() {
        let Some((device, queue)) = gpu() else { return };
        let (scene, resolved) = tiny_scene();
        let mut sims = Sims::default();
        sims.add_thing(Marking(|step| if step == 1 { vec![splat_in(4, 0.3, 2)] } else { Vec::new() }));
        let paint = stamped(&device, &queue, &scene, &resolved, &sims);
        let cover = slot_cover(&paint, 4);
        let soft = cover.iter().filter(|c| **c > 0.1 && **c < 0.9).count();
        let solid = cover.iter().filter(|c| **c >= 0.9).count();
        assert!(soft > 20 && solid > 200, "{soft} soft texels, {solid} solid");
    }

    #[test]
    fn joined_pieces_leave_no_seam() {
        let Some((device, queue)) = gpu() else { return };
        let (scene, resolved) = tiny_scene();
        let mut sims = Sims::default();
        // Pieces of one slanted, slightly bending ribbon: each leaves out the caps its neighbours cover.
        sims.add_thing(Marking(|step| {
            if step != 1 {
                return Vec::new();
            }
            let v = Vec2::new;
            let points = [v(-100.0, -40.0), v(-94.2, -39.1), v(-88.4, -38.1), v(-82.5, -37.0), v(-76.6, -35.8)];
            (0..4)
                .map(|k| {
                    let mut m = mark(1, bpe_runtime::MarkShape::Stroke, points[k], points[k + 1], 30.0);
                    m.variant = if k > 0 { 2 } else { 0 } | if k < 3 { 4 } else { 0 };
                    if k > 0 {
                        m.angle = (points[k] - points[k - 1]).to_angle();
                    }
                    m
                })
                .collect()
        }));
        let paint = stamped(&device, &queue, &scene, &resolved, &sims);
        // Every texel across the ribbon's width, between its first and last joint, is solid.
        for x in -92..=-84 {
            for y in -60..=-20 {
                let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let inside = (p - Vec2::new(p.x, -39.0 + (p.x + 94.0) * 0.16)).length() < 28.0;
                if inside {
                    assert!(texel(&paint, p)[3] > 0.99, "a seam at {p}: {:?}", texel(&paint, p));
                }
            }
        }
    }

    /// Runs `sims` for a tenth of a second and reads back the canvas's paint.
    fn stamped(device: &wgpu::Device, queue: &wgpu::Queue, scene: &Scene, resolved: &Resolved, sims: &Sims) -> Vec<u8> {
        let mut frame = Frame::default();
        frame.canvases.push(canvas_layer());
        let source = canvas_source(scene, resolved, sims);
        let mut flow = FlowLayers::new(device);
        flow.prepare(device, queue, &frame, &source);
        flow.step_to(device, queue, 0.1, &source);
        read_texture(
            device,
            queue,
            &flow.canvas_textures(MarkTarget::Canvas(CANVAS)).expect("the canvas has textures").paint,
        )
    }

    #[test]
    fn canvas_soft_edges_keep_the_time_under_them() {
        let Some((device, queue)) = gpu() else { return };
        let (scene, resolved) = tiny_scene();
        let mut sims = Sims::default();
        // A wide stroke, then a thin one whose soft edge just reaches texel (0, 0).
        sims.add_thing(Marking(|step| match step {
            1 => vec![mark(1, bpe_runtime::MarkShape::Stroke, Vec2::new(-30.0, 0.0), Vec2::new(30.0, 0.0), 20.0)],
            2 => vec![mark(2, bpe_runtime::MarkShape::Stroke, Vec2::new(-10.0, -4.8), Vec2::new(10.0, -4.8), 5.0)],
            _ => Vec::new(),
        }));
        let mut frame = Frame::default();
        frame.canvases.push(canvas_layer());
        let source = canvas_source(&scene, &resolved, &sims);
        let mut flow = FlowLayers::new(&device);
        flow.prepare(&device, &queue, &frame, &source);
        flow.step_to(&device, &queue, 0.1, &source);
        let aux = read_texture(&device, &queue, &flow.canvas_textures(MarkTarget::Canvas(CANVAS)).unwrap().aux);
        let second = 2.0 / World::build(&scene, &resolved, &sims).steps_per_second as f32;
        let first = second / 2.0;
        // Texel (0, 0)'s centre is 5.3 from the thin stroke's middle line: a fifth covered.
        let edge = texel(&aux, Vec2::new(0.5, 0.5))[0];
        assert!((edge - first).abs() < 1e-5, "{edge}: the soft edge keeps the older time");
        let inside = texel(&aux, Vec2::new(0.5, -4.5))[0];
        assert!((inside - second).abs() < 1e-5, "{inside}: the thin stroke re-dates what it covers");
    }

    #[test]
    fn a_skin_and_a_canvas_keep_their_own_marks() {
        let Some((device, queue)) = gpu() else { return };
        let (scene, resolved) = tiny_scene();
        let mut sims = Sims::default();
        // Thing 9's canvas gets a stroke at the top, its skin one at the bottom.
        sims.add_thing(Marking(|step| {
            if step != 1 {
                return Vec::new();
            }
            let stroke = |y| mark(1, bpe_runtime::MarkShape::Stroke, Vec2::new(-50.0, y), Vec2::new(50.0, y), 10.0);
            let mut skin = stroke(100.0);
            skin.target = MarkTarget::Skin(CANVAS);
            vec![stroke(-100.0), skin]
        }));
        let mut frame = Frame::default();
        frame.canvases.push(canvas_layer());
        let mut skin = canvas_layer();
        skin.key = MarkTarget::Skin(CANVAS);
        skin.place = crate::CanvasPlace::OnThing;
        frame.canvases.push(skin);
        let source = canvas_source(&scene, &resolved, &sims);
        let mut flow = FlowLayers::new(&device);
        flow.prepare(&device, &queue, &frame, &source);
        flow.step_to(&device, &queue, 0.1, &source);
        let paint = |key| read_texture(&device, &queue, &flow.canvas_textures(key).unwrap().paint);
        let (canvas, skin) = (paint(MarkTarget::Canvas(CANVAS)), paint(MarkTarget::Skin(CANVAS)));
        let (top, bottom) = (Vec2::new(0.0, -100.0), Vec2::new(0.0, 100.0));
        assert!(texel(&canvas, top)[3] > 0.99 && texel(&canvas, bottom)[3] == 0.0, "the canvas has its stroke");
        assert!(texel(&skin, bottom)[3] > 0.99 && texel(&skin, top)[3] == 0.0, "the skin has its stroke");
        // Without canvases or skins in the frame, nothing is kept.
        frame.canvases.clear();
        flow.prepare(&device, &queue, &frame, &source);
        assert!(flow.canvas_textures(MarkTarget::Skin(CANVAS)).is_none() && flow.layers.is_empty());
    }

    #[test]
    fn canvas_checkpoints_restore_its_textures() {
        let Some((device, queue)) = gpu() else { return };
        let (scene, resolved) = tiny_scene();
        let mut sims = Sims::default();
        // A stroke every step, round and round, its colour by the step.
        sims.add_thing(Marking(|step| {
            let at = |s: u64| Vec2::from_angle(s as f32 * 0.03) * (60.0 + (s % 97) as f32);
            let mut m = mark(step, bpe_runtime::MarkShape::Stroke, at(step - 1), at(step), 5.0);
            m.colour = [(step % 7) as f32 / 7.0, (step % 13) as f32 / 13.0, 0.5, 0.8];
            vec![m]
        }));
        let mut frame = Frame::default();
        frame.canvases.push(canvas_layer());
        let source = canvas_source(&scene, &resolved, &sims);
        let read = |flow: &FlowLayers| {
            let t = flow.canvas_textures(MarkTarget::Canvas(CANVAS)).unwrap();
            (read_texture(&device, &queue, &t.paint), read_texture(&device, &queue, &t.aux))
        };
        let mut seeked = FlowLayers::new(&device);
        seeked.prepare(&device, &queue, &frame, &source);
        seeked.step_to(&device, &queue, 3.0, &source);
        let (count, _, bytes) = seeked.checkpoint_stats();
        assert_eq!(count, 2, "at steps 0 and 240");
        assert!(bytes >= 2 * 2 * (SIDE * SIDE * 8) as u64, "{bytes}: the checkpoints hold the textures");
        let at_three = read(&seeked);
        seeked.step_to(&device, &queue, 2.5, &source);
        let mut straight = FlowLayers::new(&device);
        straight.prepare(&device, &queue, &frame, &source);
        straight.step_to(&device, &queue, 2.5, &source);
        let (back, ahead) = (read(&seeked), read(&straight));
        assert!(back == ahead, "seeking back to 2.5 s matches running straight there");
        assert!(back != at_three, "the canvas changed between 2.5 s and 3 s");
        // A new size starts over.
        frame.canvases[0].texels = [SIDE / 2; 2];
        seeked.prepare(&device, &queue, &frame, &source);
        assert_eq!(seeked.step(), 0);
    }
}

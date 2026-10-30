//! GPU-stepped layers: liquids with thousands of particles and swarms, stepped at [`SIM_HZ`]
//! from a private copy of the world so the playback the sound and rules read never depends
//! on the graphics card.
//!
//! [`FlowLayers`] owns that private [`World`], steps it along with the layers in batches, and
//! keeps checkpoints of both so seeking back is cheap. Each GPU step is told the first few
//! balls and where the wall is ([`inputs`]) and reads the wall distance field ([`sdf`]).

mod inputs;
mod liquid;
mod sdf;
pub(crate) mod surface;
mod swarm;
mod util;

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use bpe_core::{Resolved, Scene, ThingId};
use bpe_runtime::{Log, Outline, Sims, World};

use crate::frame::{Frame, Points};
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
}

/// One GPU-stepped layer.
enum Layer {
    Swarm(Box<SwarmGpu>),
    Liquid(LiquidGpu),
}

impl Layer {
    /// Records one step into `encoder`. `offset` binds the step's inputs in `inputs.group`.
    fn step(&self, pipelines: &Pipelines, encoder: &mut wgpu::CommandEncoder, inputs: &Inputs, offset: u32, sdf: &Sdf) {
        match self {
            Layer::Swarm(s) => s.step(&pipelines.swarm, encoder, inputs, offset, sdf),
            Layer::Liquid(l) => l.step(&pipelines.liquid, encoder, inputs, offset, sdf),
        }
    }

    /// The buffers that carry the layer's state from one step to the next; checkpoints copy them.
    fn state(&self) -> Vec<&wgpu::Buffer> {
        match self {
            Layer::Swarm(s) => s.state(),
            Layer::Liquid(l) => l.state(),
        }
    }
}

/// Copies of a layer's state buffers at one step.
struct Snapshot {
    buffers: Vec<wgpu::Buffer>,
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
        Self { buffers }
    }

    fn restore(&self, encoder: &mut wgpu::CommandEncoder, layer: &Layer) {
        for (copy, target) in self.buffers.iter().zip(layer.state()) {
            encoder.copy_buffer_to_buffer(copy, 0, target, 0, copy.size().min(target.size()));
        }
    }

    fn bytes(&self) -> u64 {
        self.buffers.iter().map(|b| b.size()).sum()
    }
}

struct Checkpoint {
    step: u64,
    world: World,
    log: Log,
    layers: BTreeMap<ThingId, Snapshot>,
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
    layers: BTreeMap<ThingId, Layer>,
    steps_run: u64,
    /// Most bytes the checkpoints may hold together; [`CHECKPOINT_BYTES`] unless changed.
    checkpoint_bytes: u64,
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

/// Keeps the log small: emitters only read back one step, so older events can go.
fn trim_log(log: &mut Log, world_step: u64) {
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
        };
        Self {
            inputs,
            sdf,
            pipelines,
            driver: None,
            layers: BTreeMap::new(),
            steps_run: 0,
            checkpoint_bytes: CHECKPOINT_BYTES,
        }
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
        match self.layers.get(&thing)? {
            Layer::Swarm(s) => Some(s.as_ref()),
            Layer::Liquid(_) => None,
        }
    }

    /// One liquid's GPU state, for tests.
    #[cfg(test)]
    pub(crate) fn liquid(&self, thing: ThingId) -> Option<&LiquidGpu> {
        match self.layers.get(&thing)? {
            Layer::Liquid(l) => Some(l),
            Layer::Swarm(_) => None,
        }
    }

    /// Where a GPU liquid's particles are, for the surface pass; `None` when the frame's
    /// layer is not (yet) running.
    pub(crate) fn gpu_points(&self, thing: ThingId) -> Option<surface::PointsBinding<'_>> {
        match self.layers.get(&thing)? {
            Layer::Liquid(l) => Some(surface::PointsBinding::Gpu {
                buffer: l.positions_buffer(),
                count: l.count(),
                spacing: l.spacing(),
                pour_steps: l.pour_steps(),
                step: self.step(),
            }),
            Layer::Swarm(_) => None,
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
    /// when the frame has no GPU layers.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame, source: &FlowSource) {
        let gpu_liquids = frame.liquids.iter().filter_map(|l| match &l.points {
            Points::Gpu(desc) => Some((l.thing, desc)),
            Points::Cpu(_) => None,
        });
        let wanted: BTreeSet<ThingId> =
            frame.swarms.iter().map(|s| s.thing).chain(gpu_liquids.clone().map(|(id, _)| id)).collect();
        if wanted.is_empty() {
            self.driver = None;
            self.layers.clear();
            return;
        }
        let same = self.driver.as_ref().is_some_and(|d| d.key == source.key && d.video == frame.video)
            && self.layers.keys().eq(wanted.iter())
            && frame.swarms.iter().all(|l| match self.layers.get(&l.thing) {
                Some(Layer::Swarm(s)) => *s.sim() == SwarmSim::of(l),
                _ => false,
            })
            && gpu_liquids.clone().all(|(id, desc)| match self.layers.get(&id) {
                Some(Layer::Liquid(l)) => l.desc() == desc,
                _ => false,
            });
        if !same {
            self.restart(device, queue, frame, source);
            return;
        }
        for l in &frame.swarms {
            if let Some(Layer::Swarm(s)) = self.layers.get_mut(&l.thing) {
                s.set_look(queue, l);
            }
        }
    }

    /// Builds the private world afresh and every layer at step 0.
    fn restart(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame, source: &FlowSource) {
        let world = World::build(source.scene, source.resolved, source.sims);
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
        let swarms = frame.swarms.iter().map(|l| {
            let swarm = SwarmGpu::new(device, queue, &mut encoder, &pipelines.swarm, inputs, sdf, l, video, arena);
            (l.thing, Layer::Swarm(Box::new(swarm)))
        });
        let liquids = frame.liquids.iter().filter_map(|l| match &l.points {
            Points::Gpu(desc) => {
                let liquid = LiquidGpu::new(device, queue, &pipelines.liquid, desc, wall.as_ref(), video);
                Some((l.thing, Layer::Liquid(liquid)))
            }
            Points::Cpu(_) => None,
        });
        self.layers = liquids.collect();
        self.layers.extend(swarms);
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
    /// in front of them.
    pub(crate) fn draw_swarms(&self, pass: &mut wgpu::RenderPass<'_>, globals: &wgpu::BindGroup, frame: &Frame) {
        for l in &frame.swarms {
            if let Some(Layer::Swarm(s)) = self.layers.get(&l.thing) {
                s.draw(&self.pipelines.swarm, pass, globals);
            }
        }
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
        let Self { inputs, sdf, pipelines, driver, layers, steps_run, checkpoint_bytes } = self;
        let driver = driver.as_mut().unwrap();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("flow steps") });
        let mut slot = 0;
        while slot < BATCH_STEPS && driver.step < target {
            driver.step += 1;
            let step = driver.step;
            let time = step as f64 / SIM_HZ as f64;
            // The private world runs at the scene's own rate: whole steps up to this one.
            let world_steps = step * driver.world.steps_per_second as u64 / SIM_HZ as u64;
            while driver.world.step < world_steps {
                driver.world.step(source.sims, &mut driver.log);
            }
            trim_log(&mut driver.log, driver.world.step);
            let wall = first_wall(&driver.world);
            let bytes =
                step_inputs(&driver.world, time, step, driver.video, &source.ball_colors, &sdf.frame(wall.as_ref()));
            let offset = inputs.write(queue, slot, &bytes);
            for layer in layers.values() {
                layer.step(pipelines, &mut encoder, inputs, offset, sdf);
            }
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

    fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
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
            clarity: 0.45,
            refraction: 0.5,
            highlight: 0.8,
            smoothing: 6.0,
            glow: 0.1,
        }
    }

    fn tiny_scene() -> (Scene, Resolved) {
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
            look: SwarmLook { size: 1.5, brightness: 0.25, colors_by: 0, colors: vec![[1.0; 4]] },
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
        });
        frame.bloom = 0.3;
        frame.time = 1.0;
        renderer.render(&device, &queue, &frame);
        let plain = renderer.read_output(&device, &queue).unwrap();
        let (scene, resolved) = tiny_scene();
        let sims = Sims::default();
        let source = FlowSource {
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
    fn a_swarm_stays_inside_the_wall_and_is_deterministic() {
        let Some((device, queue)) = gpu() else { return };
        let (scene, resolved) = tiny_scene();
        let sims = ring_sims(400.0);
        let mut frame = Frame::default();
        let mut layer = swarm_layer(ThingId(1));
        layer.count = 20_000;
        frame.swarms.push(layer);
        let source = FlowSource {
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
    fn changing_only_the_look_keeps_the_layer() {
        let Some((device, queue)) = gpu() else { return };
        let (scene, resolved) = tiny_scene();
        let sims = Sims::default();
        let mut frame = Frame::default();
        frame.swarms.push(swarm_layer(ThingId(1)));
        let source = FlowSource {
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
        frame.swarms[0].look.colors_by = 2;
        frame.swarms[0].look.colors = vec![[1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]];
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
}

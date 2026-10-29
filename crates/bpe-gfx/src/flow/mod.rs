//! GPU-stepped layers: liquids with thousands of particles and swarms, stepped at [`SIM_HZ`]
//! from a private copy of the world so the playback the sound and rules read never depends
//! on the graphics card.
//!
//! [`FlowLayers`] owns that private [`World`], steps it along with the layers in batches, and
//! keeps checkpoints of both so seeking back is cheap. Each GPU step is told the first few
//! balls and where the wall is ([`inputs`]) and reads the wall distance field ([`sdf`]).

mod inputs;
mod sdf;
mod util;

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use bpe_core::{Resolved, Scene, ThingId};
use bpe_runtime::{Log, Outline, Sims, World};

use crate::frame::{Frame, Points};
use inputs::{Inputs, step_inputs};
use sdf::Sdf;

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

/// One GPU-stepped layer.
pub(crate) trait Layer {
    /// Records one step into `encoder`. `offset` binds the step's inputs in `inputs.group`.
    fn step(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        inputs: &Inputs,
        offset: u32,
        sdf: &Sdf,
        step: u64,
    );
    /// The buffers that carry the layer's state from one step to the next; checkpoints copy them.
    fn state(&self) -> Vec<&wgpu::Buffer>;
}

/// A layer that does nothing, standing in until a kind of layer has its GPU code.
struct Noop;

impl Layer for Noop {
    fn step(&mut self, _: &wgpu::Device, _: &mut wgpu::CommandEncoder, _: &Inputs, _: u32, _: &Sdf, _: u64) {}
    fn state(&self) -> Vec<&wgpu::Buffer> {
        Vec::new()
    }
}

/// Copies of a layer's state buffers at one step.
struct Snapshot {
    buffers: Vec<wgpu::Buffer>,
}

impl Snapshot {
    fn take(device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, layer: &dyn Layer) -> Self {
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

    fn restore(&self, encoder: &mut wgpu::CommandEncoder, layer: &dyn Layer) {
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
    driver: Option<Driver>,
    layers: BTreeMap<ThingId, Box<dyn Layer>>,
    steps_run: u64,
}

/// The wall the layers keep out of: the first closed wall in id order.
fn first_wall(world: &World) -> Option<Outline> {
    world.walls.values().next().map(|w| w.outline)
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
        Self { inputs: Inputs::new(device), sdf: Sdf::new(device), driver: None, layers: BTreeMap::new(), steps_run: 0 }
    }

    /// The last layer step run; 0 before any, or without GPU layers.
    pub fn step(&self) -> u64 {
        self.driver.as_ref().map_or(0, |d| d.step)
    }

    /// Steps run since this was made, for tests and benchmarks.
    pub fn steps_run(&self) -> u64 {
        self.steps_run
    }

    /// Makes the layers match the frame: starts over when `source.key` or the set of GPU
    /// layers changed, and lets everything go when the frame has none.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame, source: &FlowSource) {
        let wanted: BTreeSet<ThingId> = frame
            .swarms
            .iter()
            .map(|s| s.thing)
            .chain(frame.liquids.iter().filter(|l| matches!(l.points, Points::Gpu(_))).map(|l| l.thing))
            .collect();
        if wanted.is_empty() {
            self.driver = None;
            self.layers.clear();
            return;
        }
        let same = self.driver.as_ref().is_some_and(|d| d.key == source.key && d.video == (frame.width, frame.height))
            && self.layers.keys().eq(wanted.iter());
        if !same {
            self.restart(device, queue, frame, source, &wanted);
        }
    }

    /// Builds the private world afresh and every layer at step 0.
    fn restart(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame,
        source: &FlowSource,
        wanted: &BTreeSet<ThingId>,
    ) {
        let world = World::build(source.scene, source.resolved, source.sims);
        let video = (frame.width, frame.height);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("flow start") });
        let wall = first_wall(&world);
        match &wall {
            Some(outline) => self.sdf.bake(device, queue, &mut encoder, outline),
            None => self.sdf.clear(queue),
        }
        self.layers = wanted.iter().map(|id| (*id, Box::new(Noop) as Box<dyn Layer>)).collect();
        let bytes = step_inputs(&world, 0.0, 0, video, &source.ball_colors, &self.sdf.frame(wall.as_ref()));
        self.inputs.write(queue, 0, &bytes);
        let snapshots =
            self.layers.iter().map(|(id, l)| (*id, Snapshot::take(device, &mut encoder, l.as_ref()))).collect();
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
                snapshot.restore(&mut encoder, layer.as_ref());
            }
        }
        queue.submit([encoder.finish()]);
    }

    /// Runs up to [`BATCH_STEPS`] steps towards `target` and sends them to the GPU.
    fn run_batch(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, target: u64, source: &FlowSource) {
        let Self { inputs, sdf, driver, layers, steps_run } = self;
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
            for layer in layers.values_mut() {
                layer.step(device, &mut encoder, inputs, offset, sdf, step);
            }
            *steps_run += 1;
            if step.is_multiple_of(driver.interval) && driver.checkpoints.last().is_none_or(|c| c.step < step) {
                let snapshots =
                    layers.iter().map(|(id, l)| (*id, Snapshot::take(device, &mut encoder, l.as_ref()))).collect();
                driver.checkpoints.push(Checkpoint {
                    step,
                    world: driver.world.clone(),
                    log: driver.log.clone(),
                    layers: snapshots,
                });
                let bytes: u64 = driver.checkpoints.iter().map(Checkpoint::bytes).sum();
                if bytes > CHECKPOINT_BYTES && driver.checkpoints.len() > 2 {
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
    use crate::{BallInstance, Flock, Frame, Renderer, SwarmLayer, SwarmLook, SwarmStart, headless_device};
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
        frame.balls.push(BallInstance { center: [0.0; 2], radius: 100.0, glow: 0.5, color: [1.0, 0.5, 0.2, 1.0] });
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
}

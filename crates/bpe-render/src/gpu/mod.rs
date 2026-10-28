//! Graphics-card layers: the wall distance field, million-point swarms and big liquids.
//!
//! The swarm and the liquid move in fixed steps of their own ([`STEP_RATE`] a second), each
//! step seeing the balls and walls of a private copy of the CPU simulation at that moment. So
//! the picture at any time is the same however the preview or export got there, at whatever
//! frame rate. Seeking back restores the nearest saved checkpoint and steps forward from it.
//!
//! None of this feeds back into the simulation: balls are never pushed by the swarm or the
//! liquid, nothing here makes sound, and nothing is read back from the GPU.

pub mod sdf;
pub mod util;

mod fluid;
mod swarm;

use crate::frame::FrameData;
use bpe_scene::{Scene, Vec2};
use bpe_sim::{Event, Playback, SimInputs, Simulation};
use bytemuck::{Pod, Zeroable};
pub use sdf::Walls;

/// Swarm and liquid steps per second of video.
pub const STEP_RATE: u32 = 120;
/// Most balls each step tells the swarm and liquid about.
pub const MAX_INPUT_BALLS: usize = 64;
/// Steps recorded before they are sent to the GPU, when catching up after a seek.
const BATCH_STEPS: u64 = 64;
/// Bytes of GPU memory saved checkpoints may use together.
const CHECKPOINT_BUDGET: u64 = 256 << 20;

/// One ball as the swarm and liquid see it. Matches `struct BallIn` in the shaders.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
struct BallGpu {
    /// x, y, radius.
    place: [f32; 4],
    velocity: [f32; 4],
    color: [f32; 4],
}

/// Header of one step's inputs. Matches the start of `struct Step` in the shaders.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct StepHeader {
    /// Time in seconds, step length, step number, ball count.
    a: [f32; 4],
    /// Scene gravity, half the frame's width and height.
    b: [f32; 4],
}

const STEP_BYTES: u64 = (std::mem::size_of::<StepHeader>() + MAX_INPUT_BALLS * std::mem::size_of::<BallGpu>()) as u64;
/// Distance between steps in the inputs buffer, as dynamic offsets need.
const STEP_STRIDE: u64 = STEP_BYTES.next_multiple_of(256);

/// WGSL declarations of a step's inputs, shared by the swarm and liquid shaders.
pub(crate) const STEP_WGSL: &str = "
struct BallIn {
    place: vec4<f32>,
    velocity: vec4<f32>,
    color: vec4<f32>,
};
struct Step {
    // x: time, y: step length, z: step number, w: ball count
    a: vec4<f32>,
    // xy: gravity, zw: half the frame size
    b: vec4<f32>,
    balls: array<BallIn, 64>,
};
";

/// What the GPU layers need from one frame, built on the CPU.
#[derive(Clone, Debug, Default)]
pub struct GpuFrame {
    /// The walls right now, when something needs the distance field this frame.
    pub walls: Option<Walls>,
    pub video: (u32, u32),
    /// The scene and inputs the playback simulates, before keyframes are applied, when the
    /// swarm or liquid is on. Their own copy of the simulation starts from these.
    pub source: Option<(Scene, SimInputs)>,
    /// How long one frame may spend catching the swarm and liquid up to the frame's time.
    /// `None` (the default, for export) always catches up fully.
    pub budget: Option<std::time::Duration>,
    /// Set by the renderer when the budget ran out, so the layers show an earlier moment
    /// and the caller should draw again to let them catch up.
    behind: std::cell::Cell<bool>,
}

impl GpuFrame {
    /// Whether the last render ran out of time before the swarm and liquid caught up.
    pub fn behind(&self) -> bool {
        self.behind.get()
    }

    pub fn fill(&mut self, scene: &Scene, playback: &Playback) {
        self.video = (scene.video.width, scene.video.height);
        let bouncing = scene.particles.iter().any(|e| e.is_active() && e.bounce);
        let sim = playback.simulation();
        self.walls = (bouncing || scene.gpu.is_active()).then(|| Walls::of(sim));
        if scene.gpu.is_active() {
            let fresh = self.source.as_ref().is_none_or(|(s, i)| {
                s != sim.scene() || i.beats != sim.inputs().beats || !same_music(i, sim.inputs())
            });
            if fresh {
                self.source = Some((sim.scene().clone(), sim.inputs().clone()));
            }
        } else {
            self.source = None;
        }
    }
}

fn same_music(a: &SimInputs, b: &SimInputs) -> bool {
    match (&a.music, &b.music) {
        (Some(x), Some(y)) => std::sync::Arc::ptr_eq(x, y),
        (None, None) => true,
        _ => false,
    }
}

/// The parts of a scene that change how the swarm and liquid move. Changing anything else
/// (colours, effects, sound) keeps their current state.
fn motion_key(scene: &Scene) -> Scene {
    let mut key = scene.clone();
    key.name.clear();
    key.look = Default::default();
    key.effects.clear();
    key.sound = Default::default();
    key.particles.clear();
    key.video.fps = 0;
    key.video.duration = 0.0;
    let (swarm, fluid) = (&mut key.gpu.swarm, &mut key.gpu.fluid);
    (swarm.size, swarm.brightness, swarm.color) = (0.0, 0.0, bpe_scene::SwarmColor::Speed);
    swarm.colors.clear();
    if !swarm.enabled {
        *swarm = Default::default();
    }
    (fluid.color, fluid.clarity, fluid.refraction, fluid.highlight, fluid.smoothing) =
        (bpe_scene::GpuFluid::default().color, 0.0, 0.0, 0.0, 0.0);
    if !fluid.enabled {
        *fluid = Default::default();
    }
    key
}

/// A saved moment: the CPU simulation and copies of the GPU state.
struct Checkpoint {
    step: u64,
    sim: Simulation,
    swarm: Option<swarm::Snapshot>,
    fluid: Option<fluid::Snapshot>,
}

/// A private copy of the simulation that the swarm and liquid step along with.
struct Driver {
    key: Scene,
    inputs: SimInputs,
    sim: Simulation,
    /// The last step run. Step 0 is the starting state.
    step: u64,
    events: Vec<Event>,
    checkpoints: Vec<Checkpoint>,
    /// Steps between checkpoints.
    interval: u64,
}

pub struct GpuLayers {
    hdr_format: wgpu::TextureFormat,
    sdf: sdf::SdfBaker,
    /// Stand-ins bound when there is no wall field yet.
    empty_view: wgpu::TextureView,
    empty_info: wgpu::Buffer,
    /// Changes whenever the wall field's texture or info buffer is replaced.
    sdf_generation: u64,
    sdf_key: Option<sdf::Layout>,
    inputs: wgpu::Buffer,
    inputs_layout: wgpu::BindGroupLayout,
    inputs_group: wgpu::BindGroup,
    swarm: Option<swarm::SwarmSim>,
    fluid: Option<fluid::FluidSim>,
    driver: Option<Driver>,
    /// Steps run since the renderer was made, for tests and benchmarks.
    steps_run: u64,
}

impl GpuLayers {
    pub fn new(device: &wgpu::Device, hdr_format: wgpu::TextureFormat) -> Self {
        let empty = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("no wall field"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let empty_info = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("no wall field info"),
            size: std::mem::size_of::<sdf::SdfInfo>() as u64,
            usage: wgpu::BufferUsages::UNIFORM,
            mapped_at_creation: false,
        });
        let inputs = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu step inputs"),
            size: STEP_STRIDE * BATCH_STEPS,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let inputs_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gpu step inputs layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(STEP_BYTES),
                },
                count: None,
            }],
        });
        let inputs_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gpu step inputs"),
            layout: &inputs_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &inputs,
                    offset: 0,
                    size: wgpu::BufferSize::new(STEP_BYTES),
                }),
            }],
        });
        Self {
            hdr_format,
            inputs_group,
            sdf: sdf::SdfBaker::new(device),
            empty_view: empty.create_view(&Default::default()),
            empty_info,
            sdf_generation: 1,
            sdf_key: None,
            inputs,
            inputs_layout,
            swarm: None,
            fluid: None,
            driver: None,
            steps_run: 0,
        }
    }

    /// The wall field and its info, or empty stand-ins when there is none.
    pub fn sdf_binding(&self) -> (&wgpu::TextureView, &wgpu::Buffer) {
        match self.sdf.field() {
            Some(field) => (&field.view, &field.info),
            None => (&self.empty_view, &self.empty_info),
        }
    }

    /// A number that changes whenever [`GpuLayers::sdf_binding`] returns different resources.
    pub fn sdf_generation(&self) -> u64 {
        self.sdf_generation
    }

    /// Swarm and liquid steps run so far.
    pub fn steps_run(&self) -> u64 {
        self.steps_run
    }

    /// Whether the scene pass must stop after the walls so the liquid can see what is behind it.
    pub fn wants_split_pass(&self, scene: &Scene) -> bool {
        scene.gpu.fluid.is_active()
    }

    /// Bakes the wall field and steps the swarm and liquid up to `time`. Catching up after a
    /// seek is sent to the GPU in batches of its own, before `encoder`.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        scene: &Scene,
        frame: &FrameData,
        time: f64,
        size: (u32, u32),
    ) {
        let video = frame.gpu.video;
        if let (true, Some((source, inputs))) = (scene.gpu.is_active(), &frame.gpu.source) {
            let caught_up = self.step_to(device, queue, source, inputs, time, video, frame.gpu.budget);
            frame.gpu.behind.set(!caught_up);
        } else if self.driver.is_some() {
            self.driver = None;
            self.swarm = None;
            self.fluid = None;
        }
        if let Some(walls) = &frame.gpu.walls {
            self.sdf.begin_batch();
            self.sdf.bake(device, queue, encoder, walls, video);
            self.note_sdf();
        }
        let globals = (scene.video.width, scene.video.height);
        if let Some(swarm) = &mut self.swarm {
            swarm.prepare_draw(device, queue, scene, globals);
        }
        if let Some(fluid) = &mut self.fluid {
            fluid.prepare_draw(device, queue, scene, size);
        }
    }

    /// Notes a new wall field texture, so bind groups that use it are made again.
    fn note_sdf(&mut self) {
        let layout = self.sdf.field().map(|f| f.layout);
        if layout != self.sdf_key {
            self.sdf_key = layout;
            self.sdf_generation += 1;
            if let Some(swarm) = &mut self.swarm {
                swarm.forget_bindings();
            }
            if let Some(fluid) = &mut self.fluid {
                fluid.forget_bindings();
            }
        }
    }

    fn step_to(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &Scene,
        inputs: &SimInputs,
        time: f64,
        video: (u32, u32),
        budget: Option<std::time::Duration>,
    ) -> bool {
        let started = std::time::Instant::now();
        let key = motion_key(scene);
        let target = (time.max(0.0) * STEP_RATE as f64).floor() as u64;
        if self.driver.as_ref().is_none_or(|d| d.key != key || !same_music(&d.inputs, inputs) || d.inputs.beats != inputs.beats) {
            self.restart(device, queue, scene, inputs, key, video);
        }
        let driver = self.driver.as_mut().unwrap();
        if target < driver.step {
            // Back to the latest checkpoint at or before the target.
            let index = driver.checkpoints.iter().rposition(|c| c.step <= target).unwrap_or(0);
            driver.checkpoints.truncate(index + 1);
            let checkpoint = &driver.checkpoints[index];
            driver.sim = checkpoint.sim.clone();
            driver.step = checkpoint.step;
            let mut encoder = device.create_command_encoder(&Default::default());
            if let (Some(swarm), Some(snapshot)) = (&mut self.swarm, &checkpoint.swarm) {
                swarm.restore(&mut encoder, snapshot);
            }
            if let (Some(fluid), Some(snapshot)) = (&mut self.fluid, &checkpoint.fluid) {
                fluid.restore(&mut encoder, snapshot, checkpoint.step);
            }
            queue.submit([encoder.finish()]);
        }
        while self.driver.as_ref().unwrap().step < target {
            self.run_batch(device, queue, target);
            if let Some(budget) = budget
                && self.driver.as_ref().unwrap().step < target
            {
                // Steps run on the GPU after they are sent, so wait for them to know the time
                // they really took.
                let _ = device.poll(wgpu::PollType::wait_indefinitely());
                if started.elapsed() >= budget {
                    return false;
                }
            }
        }
        true
    }

    /// Starts the swarm and liquid afresh at time 0.
    fn restart(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &Scene,
        inputs: &SimInputs,
        key: Scene,
        video: (u32, u32),
    ) {
        let sim = Simulation::with_inputs(scene, inputs);
        let walls = Walls::of(&sim);
        queue.write_buffer(&self.inputs, 0, &step_inputs(&sim, 0.0, 0));
        let mut encoder = device.create_command_encoder(&Default::default());
        self.sdf.begin_batch();
        self.sdf.bake(device, queue, &mut encoder, &walls, video);
        self.note_sdf();
        let field = self.sdf.field().unwrap();
        self.swarm = scene.gpu.swarm.is_active().then(|| {
            let mut s = self.swarm.take().unwrap_or_else(|| swarm::SwarmSim::new(device, &self.inputs_layout, self.hdr_format));
            s.reset(device, queue, &mut encoder, &self.inputs_group, scene, field);
            s
        });
        self.fluid = scene.gpu.fluid.is_active().then(|| {
            let mut f = self.fluid.take().unwrap_or_else(|| fluid::FluidSim::new(device, &self.inputs_layout, self.hdr_format));
            f.reset(device, queue, &mut encoder, &self.inputs_group, scene, &walls, field);
            f
        });
        queue.submit([encoder.finish()]);
        let bytes = self.swarm.as_ref().map_or(0, |s| s.state_bytes()) + self.fluid.as_ref().map_or(0, |f| f.state_bytes());
        let duration_steps = (scene.video.duration.max(1.0) * STEP_RATE as f32) as u64;
        let most = (CHECKPOINT_BUDGET / bytes.max(1)).clamp(1, 64);
        let interval = duration_steps.div_ceil(most).max(STEP_RATE as u64);
        let mut driver = Driver {
            key,
            inputs: inputs.clone(),
            sim: sim.clone(),
            step: 0,
            events: Vec::new(),
            checkpoints: Vec::new(),
            interval,
        };
        let mut encoder = device.create_command_encoder(&Default::default());
        driver.checkpoints.push(Checkpoint {
            step: 0,
            sim,
            swarm: self.swarm.as_ref().map(|s| s.snapshot(device, &mut encoder)),
            fluid: self.fluid.as_ref().map(|f| f.snapshot(device, &mut encoder)),
        });
        queue.submit([encoder.finish()]);
        self.driver = Some(driver);
    }

    /// Runs up to [`BATCH_STEPS`] steps towards `target` and sends them to the GPU.
    fn run_batch(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, target: u64) {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("gpu steps") });
        self.sdf.begin_batch();
        let mut slot = 0;
        while slot < BATCH_STEPS && self.driver.as_ref().unwrap().step < target {
            let driver = self.driver.as_mut().unwrap();
            driver.step += 1;
            let step = driver.step;
            let time = step as f64 / STEP_RATE as f64;
            driver.events.clear();
            driver.sim.advance_to(time, &mut driver.events);
            let scene = driver.sim.scene();
            let video = (scene.video.width, scene.video.height);
            let bytes = step_inputs(&driver.sim, time, step);
            let walls = Walls::of(&driver.sim);
            let offset = slot * STEP_STRIDE;
            queue.write_buffer(&self.inputs, offset, &bytes);
            self.sdf.bake(device, queue, &mut encoder, &walls, video);
            self.note_sdf();
            let field = self.sdf.field().unwrap();
            let offset = offset as u32;
            if let Some(swarm) = &mut self.swarm {
                swarm.step(device, &mut encoder, &self.inputs_group, offset, field);
            }
            if let Some(fluid) = &mut self.fluid {
                fluid.step(device, &mut encoder, &self.inputs_group, offset, field, step);
            }
            self.steps_run += 1;
            let driver = self.driver.as_mut().unwrap();
            if step % driver.interval == 0 && driver.checkpoints.last().is_none_or(|c| c.step < step) {
                driver.checkpoints.push(Checkpoint {
                    step,
                    sim: driver.sim.clone(),
                    swarm: self.swarm.as_ref().map(|s| s.snapshot(device, &mut encoder)),
                    fluid: self.fluid.as_ref().map(|f| f.snapshot(device, &mut encoder)),
                });
            }
            slot += 1;
            // Very busy walls can fill the bake buffers; send what there is and carry on.
            if self.sdf.batch_bytes() > 32 << 20 {
                break;
            }
        }
        queue.submit([encoder.finish()]);
    }

    /// Draws the liquid between the walls and everything in front of them. The HDR picture so
    /// far is copied so the liquid can bend what is behind it.
    pub fn draw_between(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        hdr_texture: &wgpu::Texture,
        hdr: &wgpu::TextureView,
    ) {
        if let Some(fluid) = &mut self.fluid {
            fluid.draw_surface(device, encoder, hdr_texture, hdr);
        }
    }

    /// Draws the swarm into the open scene pass, under the trails.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, globals: &wgpu::BindGroup) {
        if let Some(swarm) = &self.swarm {
            swarm.draw(pass, globals);
        }
    }
}

/// One step's inputs as bytes: the header, then up to [`MAX_INPUT_BALLS`] balls spread over
/// all of them.
fn step_inputs(sim: &Simulation, time: f64, step: u64) -> Vec<u8> {
    let scene = sim.scene();
    let balls = sim.balls();
    let picked = balls.len().min(MAX_INPUT_BALLS);
    let mut list = [BallGpu::default(); MAX_INPUT_BALLS];
    for (k, slot) in list.iter_mut().take(picked).enumerate() {
        let ball = &balls[k * balls.len() / picked.max(1)];
        *slot = BallGpu {
            place: [ball.position.x, ball.position.y, ball.radius, 0.0],
            velocity: [ball.velocity.x, ball.velocity.y, 0.0, 0.0],
            color: ball.color,
        };
    }
    let gravity: Vec2 = scene.physics.gravity;
    let header = StepHeader {
        a: [time as f32, 1.0 / STEP_RATE as f32, step as f32, picked as f32],
        b: [gravity.x, gravity.y, scene.video.width as f32 * 0.5, scene.video.height as f32 * 0.5],
    };
    let mut bytes = bytemuck::bytes_of(&header).to_vec();
    bytes.extend_from_slice(bytemuck::cast_slice(&list));
    bytes
}

#[cfg(test)]
mod tests;

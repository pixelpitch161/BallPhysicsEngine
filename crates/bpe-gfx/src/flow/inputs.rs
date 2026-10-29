//! What every GPU step is told about the rest of the world: the time, gravity, where the wall
//! is now, and the first few balls.

use std::collections::BTreeMap;

use bpe_core::ThingId;
use bpe_runtime::World;
use bytemuck::{Pod, Zeroable};

use super::sdf::WallFrame;
use super::util;
use super::{BATCH_STEPS, MAX_BALLS, SIM_HZ};

/// One ball as the layers see it. Matches `struct BallIn` in [`STEP_WGSL`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct BallIn {
    /// x, y, radius.
    pub place: [f32; 4],
    pub velocity: [f32; 4],
    /// Linear light.
    pub color: [f32; 4],
}

/// Header of one step's inputs. Matches the start of `struct Step` in [`STEP_WGSL`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct StepHeader {
    /// Time in seconds, step length, step number, ball count.
    pub a: [f32; 4],
    /// Scene gravity, half the frame's width and height.
    pub b: [f32; 4],
    /// The live wall: centre, cos and sin of its angle.
    pub wall: [f32; 4],
    /// x: the live wall's size over the baked one, y: 1 when there is a wall.
    pub more: [f32; 4],
}

pub const STEP_BYTES: u64 = (std::mem::size_of::<StepHeader>() + MAX_BALLS * std::mem::size_of::<BallIn>()) as u64;
/// Distance between steps in the inputs buffer, as dynamic offsets need.
pub const STEP_STRIDE: u64 = STEP_BYTES.next_multiple_of(256);

/// WGSL declarations of a step's inputs, shared by the layer shaders.
pub const STEP_WGSL: &str = "
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
    // xy: the live wall's centre, zw: cos and sin of its angle
    wall: vec4<f32>,
    // x: the live wall's size over the baked one, y: 1 when there is a wall
    more: vec4<f32>,
    balls: array<BallIn, 64>,
};
";

/// The inputs of a batch of steps, one [`STEP_STRIDE`] apart, bound with a dynamic offset.
pub struct Inputs {
    buffer: wgpu::Buffer,
    pub layout: wgpu::BindGroupLayout,
    pub group: wgpu::BindGroup,
}

impl Inputs {
    pub fn new(device: &wgpu::Device) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("flow step inputs"),
            size: STEP_STRIDE * BATCH_STEPS as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("flow step inputs layout"),
            entries: &[util::uniform_entry(0, wgpu::ShaderStages::COMPUTE, true)],
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("flow step inputs"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(STEP_BYTES),
                }),
            }],
        });
        Self { buffer, layout, group }
    }

    /// Writes one step's inputs into `slot` and returns the dynamic offset that binds them.
    pub fn write(&self, queue: &wgpu::Queue, slot: u32, bytes: &[u8]) -> u32 {
        debug_assert!(slot < BATCH_STEPS && bytes.len() as u64 == STEP_BYTES);
        let offset = slot as u64 * STEP_STRIDE;
        queue.write_buffer(&self.buffer, offset, bytes);
        offset as u32
    }
}

/// One step's inputs as bytes: the header, then the first [`MAX_BALLS`] alive bodies of every
/// thing in id order, leaving out silent things (liquids). A ball's colour is its thing's
/// entry in `colors`, white without one.
pub fn step_inputs(
    world: &World,
    time: f64,
    step: u64,
    video: (u32, u32),
    colors: &BTreeMap<ThingId, [f32; 4]>,
    wall: &WallFrame,
) -> Vec<u8> {
    let mut balls = [BallIn::default(); MAX_BALLS];
    let mut count = 0;
    'things: for (id, bodies) in &world.bodies {
        if bodies.silent {
            continue;
        }
        let color = colors.get(id).copied().unwrap_or([1.0; 4]);
        for i in 0..bodies.len() {
            if !bodies.alive[i] {
                continue;
            }
            if count == MAX_BALLS {
                break 'things;
            }
            let p = bodies.position[i];
            let v = bodies.velocity[i];
            balls[count] = BallIn { place: [p.x, p.y, bodies.radius[i], 0.0], velocity: [v.x, v.y, 0.0, 0.0], color };
            count += 1;
        }
    }
    let header = StepHeader {
        a: [time as f32, 1.0 / SIM_HZ as f32, step as f32, count as f32],
        b: [world.gravity.x, world.gravity.y, video.0 as f32 * 0.5, video.1 as f32 * 0.5],
        wall: wall.wall,
        more: wall.more,
    };
    let mut bytes = Vec::with_capacity(STEP_BYTES as usize);
    bytes.extend_from_slice(bytemuck::bytes_of(&header));
    bytes.extend_from_slice(bytemuck::cast_slice(&balls));
    bytes
}

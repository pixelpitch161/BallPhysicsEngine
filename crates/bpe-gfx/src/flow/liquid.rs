//! The GPU liquid layer: thousands of particles kept apart by position-based density
//! relaxation, laid out on the CPU and stepped by compute shaders.

use bpe_runtime::Outline;
use bytemuck::{Pod, Zeroable};
use glam::Vec2;

use super::SIM_HZ;
use super::inputs::{Inputs, STEP_WGSL};
use super::sdf::Sdf;
use super::util::{self, groups};
use crate::frame::GpuLiquid;

/// Small steps per layer step, for a steadier liquid.
const SUBSTEPS: u32 = 2;
/// Most grid cells the prefix sum can handle: 1024 blocks of 1024.
const MAX_CELLS: u32 = 1024 * 1024 - 1;
/// Closest the particles may sit, in world pixels.
const MIN_SPACING: f32 = 0.6;
/// Row height of a hexagonal lattice, in spacings.
const ROW: f32 = 0.866_025_4;

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

/// Matches `struct Params` in liquid.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
struct LiquidParams {
    counts: [u32; 4],
    shape: [f32; 4],
    motion: [f32; 4],
    more: [f32; 4],
    spout: [f32; 4],
}

/// Distance from `p` to a closed wall's inner face, positive inside. Matches `closed_distance`
/// in sdf.wgsl.
fn closed_distance(outline: &Outline, p: Vec2) -> f32 {
    use std::f32::consts::{PI, TAU};
    match *outline {
        Outline::Ring { center, radius, sides, angle, thickness } => {
            let o = p - center;
            if sides < 3 {
                return radius - o.length() - thickness * 0.5;
            }
            let step = TAU / sides as f32;
            let apothem = radius * (PI / sides as f32).cos();
            let k = ((o.y.atan2(o.x) - angle) / step).round();
            let best =
                (-1..=1).map(|j| o.dot(Vec2::from_angle(angle + (k + j as f32) * step))).fold(f32::MIN, f32::max);
            apothem - best - thickness * 0.5
        }
        Outline::Box { center, half, angle, thickness } => {
            let x = Vec2::from_angle(angle);
            let o = p - center;
            let q = Vec2::new(o.dot(x), o.dot(x.perp())).abs() - half;
            let outside = q.max(Vec2::ZERO).length() + q.x.max(q.y).min(0.0);
            -outside - thickness * 0.5
        }
    }
}

/// A hash of two numbers into 0..1, for the jitter at the spout.
fn hash(a: u32, b: u32) -> f32 {
    let mut h = a.wrapping_mul(0x9e37_79b9) ^ b.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    (h >> 8) as f32 / 16_777_216.0
}

/// Hexagonal rows from the bottom of the region up, `spacing` apart, every other row shifted
/// by half a spacing, keeping the points `keep` accepts, until `count` are placed or the
/// region runs out.
fn lattice(min: Vec2, max: Vec2, spacing: f32, keep: impl Fn(Vec2) -> bool, count: usize) -> Vec<Vec2> {
    let mut out = Vec::with_capacity(count);
    let mut row = 0u32;
    loop {
        let y = max.y - spacing * 0.5 - row as f32 * spacing * ROW;
        if y < min.y {
            break;
        }
        let shift = if row % 2 == 1 { spacing * 0.5 } else { 0.0 };
        let mut x = min.x + spacing * 0.5 + shift;
        while x <= max.x - spacing * 0.5 + 1e-3 {
            let p = Vec2::new(x, y);
            if keep(p) {
                out.push(p);
                if out.len() >= count {
                    return out;
                }
            }
            x += spacing;
        }
        row += 1;
    }
    out
}

/// Density of a particle in a resting hexagonal lattice of this spacing, as the shader sums it.
fn rest_density(spacing: f32, reach: f32) -> f32 {
    let n = (reach / (spacing * ROW)).ceil() as i32 + 1;
    let mut rho = 0.0;
    for row in -n..=n {
        let shift = if row.rem_euclid(2) == 1 { 0.5 } else { 0.0 };
        for column in -n - 1..=n + 1 {
            let d = Vec2::new((column as f32 + shift) * spacing, row as f32 * spacing * ROW);
            let q = 1.0 - d.length() / reach;
            if q > 0.0 {
                rho += q * q;
            }
        }
    }
    rho
}

/// Where the particles begin and how closely they sit.
struct Start {
    /// Position and velocity of each particle.
    state: Vec<[f32; 4]>,
    spacing: f32,
    spout_width: f32,
}

/// Lays the liquid out in its region, inside the wall, as the CPU liquid does: a hexagonal
/// lattice from the bottom up, tightened until the count fits (down to [`MIN_SPACING`]). A pour
/// instead parks every particle at the spout, to be released in turn.
fn start(desc: &GpuLiquid, wall: Option<&Outline>) -> Start {
    let min = Vec2::new(desc.region[0].min(desc.region[2]), desc.region[1].min(desc.region[3]));
    let max = Vec2::new(desc.region[0].max(desc.region[2]), desc.region[1].max(desc.region[3]));
    let size = (max - min).max(Vec2::ONE);
    let inside = |p: Vec2, margin: f32| wall.is_none_or(|w| closed_distance(w, p) > margin);
    const SAMPLES: u32 = 40;
    let mut kept = 0;
    for y in 0..SAMPLES {
        for x in 0..SAMPLES {
            let p = min + size * Vec2::new((x as f32 + 0.5) / SAMPLES as f32, (y as f32 + 0.5) / SAMPLES as f32);
            if inside(p, 0.0) {
                kept += 1;
            }
        }
    }
    let fraction = (kept as f32 / (SAMPLES * SAMPLES) as f32).max(0.01);
    let count = desc.count.max(1) as usize;
    // One particle per spacing × (spacing × ROW) of the area inside the wall.
    let mut spacing = ((size.x * size.y * fraction).max(1.0) / (count as f32 * ROW)).sqrt() * 0.98;
    let mut positions = Vec::new();
    for _ in 0..6 {
        spacing = spacing.max(MIN_SPACING);
        positions = lattice(min, max, spacing, |p| inside(p, spacing * 0.5), count);
        if positions.len() >= count || spacing <= MIN_SPACING {
            break;
        }
        spacing *= 0.9;
    }
    let spout_width = (spacing * 10.0).max(16.0);
    let state = if desc.pour_seconds > 0.0 {
        let [sx, sy] = desc.pour_from;
        (0..count as u32)
            .map(|i| {
                let jitter = hash(i ^ desc.seed, 3) - 0.5;
                let lift = hash(i ^ desc.seed, 5) * spacing * 3.0;
                [sx + jitter * spout_width, sy - lift, 0.0, 150.0]
            })
            .collect()
    } else {
        positions.iter().map(|p| [p.x, p.y, 0.0, 0.0]).collect()
    };
    Start { state, spacing, spout_width }
}

/// The pipelines every GPU liquid shares.
pub struct LiquidPipelines {
    layout: wgpu::BindGroupLayout,
    passes: [wgpu::ComputePipeline; 10],
}

impl LiquidPipelines {
    pub fn new(
        device: &wgpu::Device,
        inputs_layout: &wgpu::BindGroupLayout,
        sdf_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        use wgpu::ShaderStages as S;
        let mut entries = vec![util::uniform_entry(0, S::COMPUTE, false)];
        entries.extend((1..=8).map(|b| util::storage_entry(b, S::COMPUTE, false)));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("liquid layout"),
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("liquid"),
            bind_group_layouts: &[Some(inputs_layout), Some(&layout), Some(sdf_layout)],
            immediate_size: 0,
        });
        let module = util::shader(
            device,
            "liquid",
            &[include_str!("../shaders/flow/common.wgsl"), STEP_WGSL, include_str!("../shaders/flow/liquid.wgsl")],
        );
        let passes = PASSES.map(|entry| util::compute_pipeline(device, entry, &pipeline_layout, &module, entry));
        Self { layout, passes }
    }
}

/// One liquid's particles on the GPU.
pub struct LiquidGpu {
    desc: GpuLiquid,
    count: u32,
    cells: u32,
    blocks: u32,
    spacing: f32,
    pour_steps: f32,
    /// Position and velocity of each particle; the only state a checkpoint needs.
    state: wgpu::Buffer,
    /// Holds the params, the state and the working buffers (which live only in the group).
    group: wgpu::BindGroup,
}

impl LiquidGpu {
    /// Lays the liquid out for `desc` inside `wall` and makes its buffers.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipelines: &LiquidPipelines,
        desc: &GpuLiquid,
        wall: Option<&Outline>,
        video: (u32, u32),
    ) -> Self {
        let start = start(desc, wall);
        let count = start.state.len() as u32;
        let spacing = start.spacing;
        let reach = spacing * 2.0;
        let (w, h) = (video.0.max(1) as f32, video.1.max(1) as f32);
        let mut cell = reach;
        while ((w / cell).ceil() * (h / cell).ceil()) as u32 > MAX_CELLS {
            cell *= 1.1;
        }
        let (cx, cy) = ((w / cell).ceil() as u32, (h / cell).ceil() as u32);
        let cells = cx * cy;
        let blocks = (cells + 1).div_ceil(1024);
        let pour_steps = desc.pour_seconds.max(0.0) * SIM_HZ as f32;
        let stiffness = 0.05 + 0.3 * desc.stiffness.clamp(0.0, 2.0);
        let params = LiquidParams {
            counts: [count, cx, cy, blocks],
            shape: [spacing, reach, rest_density(spacing, reach), cell],
            motion: [stiffness, stiffness * 0.1, desc.viscosity.clamp(0.0, 1.0) * 0.5, desc.ball_push.max(0.0)],
            more: [desc.gravity_scale, pour_steps, SUBSTEPS as f32, 1.0 / (SIM_HZ * SUBSTEPS) as f32],
            spout: [desc.pour_from[0], desc.pour_from[1], start.spout_width, desc.bounciness.max(0.0)],
        };
        let storage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC;
        let make = |label, size: u64, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size.max(16),
                usage,
                mapped_at_creation: false,
            })
        };
        let n = count as u64;
        let uniform = make(
            "liquid params",
            std::mem::size_of::<LiquidParams>() as u64,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        queue.write_buffer(&uniform, 0, bytemuck::bytes_of(&params));
        let state = make("liquid state", n * 16, storage);
        queue.write_buffer(&state, 0, bytemuck::cast_slice(&start.state));
        let working = [
            make("liquid predicted", n * 16, storage),
            make("liquid relaxed", n * 16, storage),
            make("liquid pressure", n * 8, storage),
            make("liquid cell counts", (cells as u64 + 2) * 4, storage),
            make("liquid cell starts", (cells as u64 + 2) * 4, storage),
            make("liquid sorted", n * 4, storage),
            make("liquid block sums", 1024 * 4, storage),
        ];
        let mut entries = vec![
            wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: state.as_entire_binding() },
        ];
        entries.extend(
            working
                .iter()
                .enumerate()
                .map(|(k, b)| wgpu::BindGroupEntry { binding: k as u32 + 2, resource: b.as_entire_binding() }),
        );
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("liquid"),
            layout: &pipelines.layout,
            entries: &entries,
        });
        Self { desc: desc.clone(), count, cells, blocks, spacing, pour_steps, state, group }
    }

    /// What the liquid was made from; a frame with a different one remakes it.
    pub fn desc(&self) -> &GpuLiquid {
        &self.desc
    }

    /// Particles that fit; less than asked for when the region could not hold them.
    pub fn count(&self) -> u32 {
        self.count
    }

    /// Distance between resting particles, in world pixels.
    pub fn spacing(&self) -> f32 {
        self.spacing
    }

    /// Layer steps the pour takes; 0 when the liquid starts in place.
    pub fn pour_steps(&self) -> f32 {
        self.pour_steps
    }

    /// Position and velocity of each particle, 16 bytes each, as the surface reads them.
    pub fn positions_buffer(&self) -> &wgpu::Buffer {
        &self.state
    }

    /// Records one step ([`SUBSTEPS`] substeps), reading the step inputs at `offset`.
    pub fn step(
        &self,
        pipelines: &LiquidPipelines,
        encoder: &mut wgpu::CommandEncoder,
        inputs: &Inputs,
        offset: u32,
        sdf: &Sdf,
    ) {
        if self.count == 0 {
            return;
        }
        let mut pass = encoder
            .begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("liquid step"), timestamp_writes: None });
        pass.set_bind_group(0, &inputs.group, &[offset]);
        pass.set_bind_group(1, &self.group, &[]);
        pass.set_bind_group(2, sdf.bind_group(), &[]);
        let particles = groups(self.count, 256);
        let cells = groups(self.cells + 1, 256);
        for _ in 0..SUBSTEPS {
            for (pipeline, name) in pipelines.passes.iter().zip(PASSES) {
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

    /// The buffers a checkpoint saves.
    pub fn state(&self) -> Vec<&wgpu::Buffer> {
        vec![&self.state]
    }

    /// Reads the particles back (position, velocity), for tests.
    #[cfg(test)]
    pub fn read_state(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<[f32; 4]> {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("liquid readback"),
            size: self.state.size(),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&self.state, 0, &buffer, 0, self.state.size());
        queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let data = buffer.slice(..).get_mapped_range().unwrap();
        bytemuck::cast_slice::<u8, [f32; 4]>(&data)[..self.count as usize].to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool(count: u32) -> GpuLiquid {
        GpuLiquid {
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
        }
    }

    #[test]
    fn the_layout_fills_the_region_inside_the_wall_from_the_bottom() {
        let wall = Outline::Box { center: Vec2::ZERO, half: Vec2::new(400.0, 700.0), angle: 0.0, thickness: 4.0 };
        let s = start(&pool(12_000), Some(&wall));
        assert_eq!(s.state.len(), 12_000);
        assert!(s.state.iter().all(|p| p[0].abs() < 398.0 && p[1] > 210.0 && p[1] < 698.0));
        assert!(s.state[0][1] > s.state[11_999][1], "the first particle sits at the bottom");
        let expected = (800.0 * 490.0 / (12_000.0_f32 * ROW)).sqrt();
        assert!((s.spacing - expected * 0.98).abs() < expected * 0.05, "{} vs {expected}", s.spacing);
        let top = s.state.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
        assert!(top < 210.0 + 490.0 * 0.06, "the lattice stops {} px short of the top", top - 210.0);
        // A round wall leaves the corners out and the lattice tightens to still hold the count.
        let ring = Outline::Ring { center: Vec2::ZERO, radius: 400.0, sides: 0, angle: 0.0, thickness: 4.0 };
        let r = start(&GpuLiquid { region: [-400.0, 120.0, 400.0, 400.0], ..pool(5_000) }, Some(&ring));
        assert_eq!(r.state.len(), 5_000);
        assert!(r.state.iter().all(|p| closed_distance(&ring, Vec2::new(p[0], p[1])) > 0.0));
    }

    #[test]
    fn a_pour_parks_every_particle_at_the_spout() {
        let s = start(&GpuLiquid { pour_seconds: 2.0, ..pool(1_000) }, None);
        assert_eq!(s.state.len(), 1_000);
        assert!(s.state.iter().all(|p| (p[0]).abs() <= s.spout_width * 0.5 + 1e-3 && p[1] <= -600.0 && p[3] == 150.0));
    }

    #[test]
    fn rest_density_only_depends_on_the_reach_in_spacings() {
        let rho = rest_density(2.0, 4.0);
        assert!(rho > 2.0 && rho < 8.0, "{rho}");
        assert!((rest_density(1.0, 2.0) - rho).abs() < 1e-4);
    }
}

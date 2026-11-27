//! The all-drops engine (`docs/superpowers/specs/2026-11-25-all-drops-water-core-design.md`):
//! [`WaterGpu`] (one per device: the pipelines) and [`Water`] (one body of liquid). Every drop
//! feels its neighbours directly; deep water is also held up by the grid's pressure underneath
//! ([`deep`]), and only deep water.
//!
//! A body moves on a frame (1/60 s) at a time ([`Water::frame`]), cut into steps, and a frame's
//! steps go to the card as one submission. A frame never waits for the card unless the card is
//! more than a few frames behind.

use std::sync::{Arc, Mutex};

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

mod air;
mod buffers;
pub mod capture;
mod clock;
pub mod deep;
pub mod drains;
pub mod forces;
pub mod numbers;
mod passes;
pub mod state;

use crate::drop::{AIR, Drop, Pour, Poured};
use crate::lumps::Timed;
use crate::solid::{Layout, SolidField};
use air::Air;
pub use buffers::DropBuffers;
use buffers::{ByDrop, room_for, storage};
use clock::Clock;
pub use clock::{MOST_POURS, Report};
use deep::{Deep, MOST_LEVELS, TICKS};
pub use drains::{Drain, MOST_DRAINS};
use forces::ForceGpu;
pub use forces::{Force, MOST_FORCES, Push};
use numbers::{FRAME, Feel, FeelGpu, MOST_LIQUIDS, Numbers};
use passes::{ENTRIES, P, Pipelines, STEP_ROOM};
pub use state::WaterState;

/// The engine's shader, as it is built (what a take's engine print hashes).
pub fn engine_source() -> &'static str {
    passes::SOURCE
}

/// The most drops a body holds: each of its drops' buffers (16 bytes a drop) stays within the
/// 128 MiB a storage buffer may be bound with on every card (wgpu's default limits).
pub const MOST_DROPS: u32 = 8_388_608;
/// The most drops born in one frame (the births buffer's binding stays within 128 MiB); any more
/// are let go.
pub(crate) const MOST_BIRTHS: u32 = 1 << 22;
/// How thick a thin solid is taken to be, in cells, for this engine ([`SolidField::build_least`]).
///
/// A wall exactly a cell thick whose middle line falls half way between two rows of nodes has
/// both rows on its faces: the field is nothing all through it and has no slope, so a drop there
/// is never pushed out and goes straight through. At a cell and a half every wall has nodes
/// inside it wherever it lies, the field's slope is whole up to half a cell past the face, and
/// sub-moves and sweeps (under half a cell, each kept off the solids) never get further.
pub const LEAST_WALL: f32 = 1.5;
/// The most steps a frame may take, whatever the numbers ask.
const MOST_STEPS: u32 = 1024;
/// Cells from the grid's edge where drops stop (`EDGE` in common.wgsl, which stops them at this
/// plus 0.05).
const EDGE: f32 = 2.0;
const WORKGROUP: u32 = 256;
const BLOCK: u32 = 512;
/// The most drops exported at once (each chunk's binding stays under 128 MiB).
const EXPORT_CHUNK: u32 = 1 << 21;

/// A frame's numbers on the card. Matches `Fr` in common.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct FrameGpu {
    grid: [u32; 4],
    cells: [u32; 4],
    time: [f32; 4],
    k: [f32; 4],
    w: [f32; 4],
    s: [f32; 4],
    feels: [FeelGpu; MOST_LIQUIDS],
    wall: [[f32; 4]; 17],
    wet: [[f32; 4]; 17],
    spouts: [[f32; 4]; MOST_POURS / 2],
    deep: [f32; 4],
    deep2: [f32; 4],
    mg: [f32; 4],
    mg2: [f32; 4],
    dgrid: [u32; 4],
    air: [f32; 4],
    lv: [[u32; 4]; MOST_LEVELS],
    drains: [[f32; 4]; MOST_DRAINS],
    dedges: [[u32; 4]; MOST_DRAINS / 4],
    dmeta: [u32; 4],
    forces_n: [u32; 4],
    forces: [ForceGpu; MOST_FORCES],
}

/// Where the deep water's pressure passes' numbers start among the step numbers' slots (after
/// every step's and the export's).
const TICK_SLOT: u32 = MOST_STEPS + 1;
/// The first of the pack passes' numbers' slots ([`capture::PACK_SLOTS`]).
const PACK_SLOT: u32 = TICK_SLOT + TICKS;

/// A drop born in a step. Matches `Born` in common.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub(crate) struct Born {
    xv: [f32; 4],
    id: [u32; 4],
}

/// The all-drops engine's pipelines on one device.
pub struct WaterGpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipes: Pipelines,
}

impl std::fmt::Debug for WaterGpu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WaterGpu")
    }
}

impl WaterGpu {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Arc<WaterGpu> {
        Arc::new(WaterGpu { device: device.clone(), queue: queue.clone(), pipes: Pipelines::new(device) })
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
}

/// One body of liquid on the card, all drops.
pub struct Water {
    gpu: Arc<WaterGpu>,
    layout: Layout,
    numbers: Numbers,
    feels: Vec<Feel>,
    gravity: glam::Vec2,
    edges_shut: bool,
    /// The pours, and how far each is through its rows.
    pours: Vec<(Pour, Poured)>,
    /// The drops the next frame is to give birth to at their own moments ([`Water::set_timed`]).
    timed: Vec<Timed>,
    /// The drains, and the liquid drops they have taken that fed pours have not poured yet.
    drains: Vec<Drain>,
    tank: u64,
    /// The forces (at most [`MOST_FORCES`], each [`Force::is_felt`]).
    forces: Vec<Force>,
    count: u32,
    room: u32,
    /// The serial the next drop added is given.
    serial: u32,
    /// How many liquid drops have been added since the start (wrapping): with the reports' count
    /// of the liquid in the course, how much liquid the body holds ([`Water::liquid`]).
    poured: u32,
    steps: u64,
    frames: u32,
    /// The bins across and down (each a reach wide).
    cells: (u32, u32),
    /// The density at rest and a drop's stiffness there, the solids' share by distance, and what a
    /// wetted face adds to a drop's skin by distance.
    rest: (f32, f32),
    wall: [[f32; 4]; 17],
    wet: [[f32; 4]; 17],
    frame_uniform: wgpu::Buffer,
    step_uniform: wgpu::Buffer,
    solid: wgpu::Buffer,
    counts: wgpu::Buffer,
    starts: wgpu::Buffer,
    blocks: wgpu::Buffer,
    births: wgpu::Buffer,
    by: ByDrop,
    /// Which of the two kinds' buffers holds the drops now.
    now: usize,
    /// Every step pass's bind group, for each of the two kinds' buffers and each way round of the
    /// sweeps' places ([`Water::group`]), and the report pass's for each report slot.
    groups: Vec<wgpu::BindGroup>,
    report_groups: Vec<wgpu::BindGroup>,
    /// The drains' counting pass's, for each report slot.
    drained_groups: Vec<wgpu::BindGroup>,
    /// What the card reports and the host keeps of it (behind a lock so a copy, made from a shared
    /// body, can read the reports still on their way).
    clock: Mutex<Clock>,
    last: Option<wgpu::SubmissionIndex>,
    /// The deep water's cells and pressure.
    deep: Deep,
    /// The cells of trapped air found at a frame's last step.
    air: Air,
    /// Whether the last frame ran the deep part, its step then (the kept pressure is that step's
    /// pressure times it), whether drops were added since (the next deep frame primes), the frames
    /// still to run it whatever the reports say (an add's frame and the next: their reports come
    /// from before it), and the frames that ran it.
    deep_last: bool,
    deep_dt: f32,
    prime_next: bool,
    deep_hold: u32,
    deep_frames: u32,
}

impl std::fmt::Debug for Water {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Water({} drops, {} x {}, {} frames)", self.count, self.layout.cols, self.layout.rows, self.frames)
    }
}

impl Water {
    /// A body with no drops on `field`'s layout, in a course whose gravity is `gravity` (pixels a
    /// second squared). `feels[k]` is liquid `k`.
    pub fn new(gpu: Arc<WaterGpu>, field: &SolidField, gravity: glam::Vec2, numbers: Numbers, feels: &[Feel]) -> Water {
        let device = &gpu.device;
        let layout = field.layout;
        // (The bins and the card's kernel take the same reach.)
        let numbers = Numbers { reach: numbers.reach.max(0.5), ..numbers };
        let reach = numbers.reach;
        let cells =
            (((layout.cols as f32 / reach).ceil() as u32).max(1), ((layout.rows as f32 / reach).ceil() as u32).max(1));
        // (The bins, and one more for drops that left the course.)
        let bins = u64::from(cells.0 * cells.1) + 1;
        let frame_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("water frame"),
            size: std::mem::size_of::<FrameGpu>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let step_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("water steps"),
            size: u64::from(PACK_SLOT + capture::PACK_SLOTS) * STEP_ROOM,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let solid = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("water solid field"),
            contents: bytemuck::cast_slice(&field.for_card()),
            usage: buffers::STORE,
        });
        let room = room_for(0);
        let mut water = Water {
            layout,
            numbers,
            feels: Vec::new(),
            gravity,
            edges_shut: true,
            pours: Vec::new(),
            timed: Vec::new(),
            drains: Vec::new(),
            tank: 0,
            forces: Vec::new(),
            count: 0,
            room,
            serial: 0,
            poured: 0,
            steps: 0,
            frames: 0,
            cells,
            rest: numbers.at_rest(),
            wall: Numbers::wall_table(),
            wet: numbers.wet_table(),
            frame_uniform,
            step_uniform,
            solid,
            counts: storage(device, "water counts", bins * 4),
            starts: storage(device, "water starts", (bins + 1) * 4),
            blocks: storage(device, "water blocks", bins.div_ceil(u64::from(BLOCK)) * 4),
            births: storage(device, "water births", 256),
            by: ByDrop::new(device, room),
            now: 0,
            groups: Vec::new(),
            report_groups: Vec::new(),
            drained_groups: Vec::new(),
            clock: Mutex::new(Clock::new(device)),
            last: None,
            deep: Deep::new(device, &layout),
            air: Air::new(device, &layout),
            deep_last: false,
            deep_dt: 0.0,
            prime_next: false,
            deep_hold: 0,
            deep_frames: 0,
            gpu,
        };
        water.set_feels(feels);
        water.bind();
        water.put_walls();
        water
    }

    /// The pressure passes' numbers (twice the level, plus the colour), each at its own slot, and
    /// the solids by cell for the deep water: once, as the body is made.
    fn put_walls(&mut self) {
        let gpu = self.gpu.clone();
        let mut ticks = vec![0u8; (TICKS * STEP_ROOM as u32) as usize];
        for k in 0..TICKS {
            let at = (k * STEP_ROOM as u32) as usize;
            ticks[at..at + 4].copy_from_slice(&k.to_le_bytes());
        }
        gpu.queue.write_buffer(&self.step_uniform, u64::from(TICK_SLOT) * STEP_ROOM, &ticks);
        let n = self.numbers.floor.max(1);
        gpu.queue.write_buffer(
            &self.frame_uniform,
            0,
            bytemuck::bytes_of(&self.frame_gpu(FRAME / n as f32, false, 0.0)),
        );
        let mut encoder =
            gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("water walls") });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("water walls"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&gpu.pipes.list[P::DeepWalls as usize].0);
            pass.set_bind_group(0, &self.groups[(P::DeepWalls as usize * 2) * 2], &[0]);
            pass.dispatch_workgroups((self.layout.nodes() as u32).div_ceil(WORKGROUP).max(1), 1, 1);
        }
        gpu.queue.submit([encoder.finish()]);
    }

    /// Makes the bind groups (again, after a buffer was replaced): every step pass's and the
    /// report's. (The export's [`Water::read`] makes for its own buffer.)
    fn bind(&mut self) {
        let mut groups = Vec::with_capacity(P::Report as usize * 4);
        for pass in 0..P::Report as usize {
            for now in 0..2 {
                for y_in in 0..2 {
                    groups.push(self.make_group(pass, now, y_in));
                }
            }
        }
        self.groups = groups;
        let (name, bindings) = ENTRIES[P::Report as usize];
        let clock = self.clock.lock().expect("the clock");
        // (One for each report slot and each of the kinds buffers the last step can sort into:
        // `slot * 2 + kinds`.)
        self.report_groups = (0..6)
            .map(|at| {
                let (slot, kinds) = (at / 2, at % 2);
                let entries: Vec<wgpu::BindGroupEntry> = bindings
                    .iter()
                    .map(|&b| {
                        let resource = match b {
                            0 => self.frame_uniform.as_entire_binding(),
                            1 => self.step_binding(),
                            2 => self.by.xv.as_entire_binding(),
                            5 => self.by.id[kinds].as_entire_binding(),
                            8 => self.starts.as_entire_binding(),
                            15 => self.solid.as_entire_binding(),
                            25 => self.deep.count.as_entire_binding(),
                            28 => self.air.blocks.as_entire_binding(),
                            _ => clock.on_card(slot).as_entire_binding(),
                        };
                        wgpu::BindGroupEntry { binding: b, resource }
                    })
                    .collect();
                self.gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(name),
                    layout: &self.gpu.pipes.list[P::Report as usize].1,
                    entries: &entries,
                })
            })
            .collect();
        let (name, bindings) = ENTRIES[P::Drained as usize];
        self.drained_groups = (0..clock::SLOTS)
            .map(|slot| {
                let entries: Vec<wgpu::BindGroupEntry> = bindings
                    .iter()
                    .map(|&b| {
                        let resource = match b {
                            0 => self.frame_uniform.as_entire_binding(),
                            1 => self.step_binding(),
                            2 => self.by.xv.as_entire_binding(),
                            _ => clock.on_card(slot).as_entire_binding(),
                        };
                        wgpu::BindGroupEntry { binding: b, resource }
                    })
                    .collect();
                self.gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(name),
                    layout: &self.gpu.pipes.list[P::Drained as usize].1,
                    entries: &entries,
                })
            })
            .collect();
    }

    /// The step's numbers, each step at its own offset.
    fn step_binding(&self) -> wgpu::BindingResource<'_> {
        wgpu::BindingResource::Buffer(wgpu::BufferBinding {
            buffer: &self.step_uniform,
            offset: 0,
            size: wgpu::BufferSize::new(16),
        })
    }

    fn make_group(&self, pass: usize, now: usize, y_in: usize) -> wgpu::BindGroup {
        let (name, bindings) = ENTRIES[pass];
        let entries: Vec<wgpu::BindGroupEntry> = bindings
            .iter()
            .map(|&b| {
                let resource = match b {
                    0 => self.frame_uniform.as_entire_binding(),
                    1 => self.step_binding(),
                    2 => self.by.xv.as_entire_binding(),
                    3 => self.by.nf.as_entire_binding(),
                    4 => self.by.id[now].as_entire_binding(),
                    5 => self.by.id[1 - now].as_entire_binding(),
                    6 => self.by.xp.as_entire_binding(),
                    7 => self.counts.as_entire_binding(),
                    8 => self.starts.as_entire_binding(),
                    9 => self.blocks.as_entire_binding(),
                    10 => self.by.idx.as_entire_binding(),
                    11 => self.by.y[y_in].as_entire_binding(),
                    12 => self.by.y[1 - y_in].as_entire_binding(),
                    13 => self.by.lamb.as_entire_binding(),
                    14 => self.by.velb.as_entire_binding(),
                    15 => self.solid.as_entire_binding(),
                    16 => self.births.as_entire_binding(),
                    19 => self.deep.sums.as_entire_binding(),
                    20 => self.deep.wallc.as_entire_binding(),
                    21 => self.deep.cellp.as_entire_binding(),
                    22 => self.deep.coef.as_entire_binding(),
                    23 => self.deep.dfa.as_entire_binding(),
                    24 => self.deep.dfb.as_entire_binding(),
                    26 => self.deep.rest.as_entire_binding(),
                    27 => self.air.cells.as_entire_binding(),
                    28 => self.air.blocks.as_entire_binding(),
                    _ => self.deep.count.as_entire_binding(),
                };
                wgpu::BindGroupEntry { binding: b, resource }
            })
            .collect();
        self.gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(name),
            layout: &self.gpu.pipes.list[pass].1,
            entries: &entries,
        })
    }

    pub fn layout(&self) -> Layout {
        self.layout
    }

    pub fn numbers(&self) -> &Numbers {
        &self.numbers
    }

    /// From the next frame. (The reach sizes the bins: it stays as the body was made with. The
    /// deep water's solids by cell are worked out once, from `wall_off` as the body was made.)
    pub fn set_numbers(&mut self, numbers: Numbers) {
        self.numbers = Numbers { reach: self.numbers.reach, ..numbers };
    }

    /// The liquids, from the next frame (`feels[k]` is liquid `k`; at most [`MOST_LIQUIDS`]).
    pub fn set_feels(&mut self, feels: &[Feel]) {
        self.feels = feels.iter().copied().take(MOST_LIQUIDS).collect();
        if self.feels.is_empty() {
            self.feels.push(Feel::default());
        }
    }

    pub fn feels(&self) -> &[Feel] {
        &self.feels
    }

    pub fn gpu(&self) -> &Arc<WaterGpu> {
        &self.gpu
    }

    /// How many drops the body holds (the last few may have left the course: they take no part;
    /// trapped air's drops and the room kept for them count too).
    pub fn count(&self) -> u32 {
        self.count
    }

    /// How many of its drops are liquid, at most: the liquid in the course as the card counted it two
    /// frames back, and every liquid drop added since (some may have left). Trapped air is left out.
    pub fn liquid(&self) -> u32 {
        let (counted, by) = self.clock.lock().expect("the clock").liquid;
        counted.wrapping_add(self.poured.wrapping_sub(by)).min(self.count)
    }

    /// How many drops its buffers have room for (a copy takes as much).
    pub fn room(&self) -> u32 {
        self.room
    }

    /// The steps taken so far, all frames together.
    pub fn steps(&self) -> u64 {
        self.steps
    }

    /// The frames played so far: the body is at `frames() / 60` seconds.
    pub fn frames(&self) -> u32 {
        self.frames
    }

    /// From the next frame.
    pub fn set_edges_shut(&mut self, shut: bool) {
        self.edges_shut = shut;
    }

    /// From the next frame (pixels a second squared).
    pub fn set_gravity(&mut self, gravity: glam::Vec2) {
        self.gravity = gravity;
    }

    /// How many times the course's gravity air drops fall upward at, from the next frame (the
    /// Water card's Bubbles rise; [`Numbers::air_lift`]).
    pub fn set_air_lift(&mut self, lift: f32) {
        self.numbers.air_lift = lift;
    }

    /// The pours (at most [`MOST_POURS`]), from the next frame. A pour that was there before (the
    /// same place in the list) keeps how far it is through its rows; a new one starts afresh.
    pub fn set_pours(&mut self, pours: &[Pour]) {
        let was = std::mem::take(&mut self.pours);
        self.pours = pours
            .iter()
            .take(MOST_POURS)
            .enumerate()
            .map(|(k, p)| (*p, was.get(k).map_or_else(Poured::default, |w| w.1)))
            .collect();
    }

    pub fn pours(&self) -> impl Iterator<Item = &Pour> {
        self.pours.iter().map(|(p, _)| p)
    }

    /// The drops the next frame gives birth to, each at its own moment (rain, drips and bursts:
    /// [`crate::Lumps`]): each comes in the step its moment falls in, moved on to that step's end by
    /// its speed and its liquid's fall, as a pour's rows are. Used once: a frame with no call
    /// before it has none. (Not [`Water::add`], which primes the deep water.) At most a frame's
    /// births ([`MOST_DROPS`] in all); while the body is full none are born.
    pub fn set_timed(&mut self, timed: &[Timed]) {
        self.timed = timed.iter().take(MOST_BIRTHS as usize).copied().collect();
    }

    /// The drains (at most [`MOST_DRAINS`]), from the next frame.
    pub fn set_drains(&mut self, drains: &[Drain]) {
        self.drains = drains.iter().take(MOST_DRAINS).copied().collect();
    }

    pub fn drains(&self) -> &[Drain] {
        &self.drains
    }

    /// The liquid drops the drains that feed pours have taken and fed pours have not poured yet
    /// (as the reports read back say: two frames behind).
    pub fn tank(&self) -> u64 {
        self.tank
    }

    /// The liquid drops each drain took in the frame whose report the last boundary read (two
    /// frames before it).
    pub fn drained(&self) -> [u32; MOST_DRAINS] {
        self.clock.lock().expect("the clock").drained
    }

    /// The forces, from the next frame: the first [`MOST_FORCES`] that can be felt
    /// ([`Force::is_felt`]), the rest left out.
    pub fn set_forces(&mut self, forces: &[Force]) {
        self.forces = forces.iter().filter(|f| f.is_felt()).take(MOST_FORCES).copied().collect();
    }

    /// The forces it feels each frame until they are set again.
    pub fn forces(&self) -> &[Force] {
        &self.forces
    }

    /// Where the drops are on the card, for the drawing.
    pub fn drop_buffers(&self) -> DropBuffers<'_> {
        DropBuffers { xv: &self.by.xv, nf: &self.by.nf, id: &self.by.id[self.now], count: self.count }
    }

    /// Makes room for `n` drops, keeping the drops there are.
    fn grow_to(&mut self, n: u32) {
        if n <= self.room {
            return;
        }
        let room = room_for(n);
        let device = &self.gpu.device;
        let bigger = ByDrop::new(device, room);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("water grow") });
        self.by.copy_kept(&mut encoder, &bigger, self.count, self.now);
        self.gpu.queue.submit([encoder.finish()]);
        self.by = bigger;
        self.room = room;
        self.bind();
    }

    /// Adds `born` between frames, each given the body's next serial (theirs is not used; those
    /// past [`MOST_DROPS`] are let go). Returns how many were added. The next frame runs the deep
    /// part and primes its pressure (a pool added holds its weight before it moves).
    pub fn add(&mut self, born: &[Drop]) -> u32 {
        self.settle();
        let n = (born.len() as u32).min(MOST_DROPS - self.count);
        if n == 0 {
            return 0;
        }
        self.prime_next = true;
        self.deep_hold = 2;
        self.grow_to(self.count + n);
        let mut xv = Vec::with_capacity(n as usize);
        let mut nf = Vec::with_capacity(n as usize);
        let mut id = Vec::with_capacity(n as usize);
        let mut fastest = 0.0f32;
        for d in &born[..n as usize] {
            fastest = fastest.max(glam::Vec2::from(d.v).length() * self.layout.cell);
            xv.push([d.x[0], d.x[1], d.v[0], d.v[1]]);
            nf.push([0.0f32, 0.0, 1.0, 0.0]);
            id.push([d.kind, self.serial, d.colour, 0]);
            self.serial = self.serial.wrapping_add(1);
        }
        let at = u64::from(self.count) * 16;
        let queue = &self.gpu.queue;
        queue.write_buffer(&self.by.xv, at, bytemuck::cast_slice(&xv));
        queue.write_buffer(&self.by.nf, at, bytemuck::cast_slice(&nf));
        queue.write_buffer(&self.by.id[self.now], at, bytemuck::cast_slice(&id));
        self.count += n;
        let liquid = born[..n as usize].iter().filter(|d| !d.is_air()).count() as u32;
        self.poured = self.poured.wrapping_add(liquid);
        self.clock.get_mut().expect("the clock").added(self.frames, fastest);
        n
    }

    fn bins(&self) -> u32 {
        self.cells.0 * self.cells.1 + 1
    }

    /// The frame's numbers for steps of `dt` seconds (`capped`: the skin is held to what a step of
    /// `dt` keeps stable; `kept`: what the deep water's kept pressure is scaled by on its first step).
    fn frame_gpu(&self, dt: f32, capped: bool, kept: f32) -> FrameGpu {
        let n = &self.numbers;
        let cell = self.layout.cell;
        let g = self.gravity / cell;
        let (rho0, stiff) = self.rest;
        let mut feels = [FeelGpu::default(); MOST_LIQUIDS];
        for (k, f) in self.feels.iter().enumerate() {
            feels[k] = n.feel_gpu(f, cell, g.length(), dt, capped);
        }
        let mut spouts = [[0.0f32; 4]; MOST_POURS / 2];
        for (k, (p, _)) in self.pours.iter().enumerate() {
            // (Where the room for a new drop is judged: a reach behind the spout, where the stream's
            // own drops do not count.)
            let g = self.layout.to_grid(p.place) - p.way() * n.reach;
            spouts[k / 2][2 * (k % 2)] = g.x;
            spouts[k / 2][2 * (k % 2) + 1] = g.y;
        }
        let (deep_d, deep_b, reach) = n.deep_window(cell);
        let (drains, dedges, dmeta) = drains::on_card(&self.drains, &self.layout, dt, self.frames);
        let mut forces = [ForceGpu::default(); MOST_FORCES];
        for (k, f) in self.forces.iter().take(MOST_FORCES).enumerate() {
            forces[k] = f.gpu(&self.layout);
        }
        FrameGpu {
            grid: [self.layout.cols, self.layout.rows, self.pours.len() as u32, u32::from(self.edges_shut)],
            cells: [self.cells.0, self.cells.1, self.cells.0 * self.cells.1, self.bins().div_ceil(BLOCK)],
            time: [dt, g.x, g.y, n.most_move / dt],
            k: [n.reach, rho0, n.eps.max(0.0) * stiff, n.relax],
            w: [n.wet_drag.clamp(0.0, 1.0), n.wall_off, n.sub_move, n.wall_share.max(0.0)],
            s: [cell, n.drag_reach.max(n.wall_off + 0.01), n.most_fix, n.drag.max(0.0)],
            feels,
            wall: self.wall,
            wet: self.wet,
            spouts,
            deep: [deep_d, deep_b, n.deep_full, n.deep_most.max(0.0)],
            deep2: [
                if kept.is_finite() { kept.max(0.0) } else { 0.0 },
                n.meet_share(dt, cell),
                n.rest_most.max(0.0),
                if n.wet_bend.is_finite() { n.wet_bend.max(0.0) } else { 0.0 },
            ],
            mg: [n.omega, n.scale, n.push_out.max(0.0), n.most_out.max(0.0)],
            // (Drops rest `wall_off` off a face and fill the half spacing round them: the liquid's
            // edge is a quarter cell nearer the face. Drops stop at `EDGE + 0.05` from the grid's
            // edge, so the course's own liquid starts a quarter cell before that.)
            mg2: [n.fill_rate.max(0.0), n.face_floor.max(0.0), n.wall_off - 0.25, EDGE + 0.05 - 0.25],
            dgrid: [self.deep.levels.len() as u32, reach, 0, 0],
            air: [
                if n.air_lift.is_finite() { n.air_lift.clamp(-1e3, 1e3) } else { 0.0 },
                n.air_pop,
                n.air_window(cell) as f32,
                if n.air_drag.is_finite() { n.air_drag.clamp(0.0, 1.0) } else { 1.0 },
            ],
            lv: self.deep.ladder(),
            drains,
            dedges,
            dmeta,
            forces_n: [self.forces.len().min(MOST_FORCES) as u32, 0, 0, 0],
            forces,
        }
    }

    /// Settles the boundary the body is at (the start of frame `frames()`), once: drops that left
    /// the course two frames ago are let go from the end, where the sort put them.
    fn settle(&mut self) {
        let (go, fed) = self.clock.get_mut().expect("the clock").settle(&self.gpu.device, self.frames);
        self.count -= go.min(self.count);
        self.tank = self.tank.saturating_add(fed);
    }

    /// The steps frame `frames()` takes: enough for the fastest water two frames ago and what was
    /// added since, with room for two frames' fall, and for the liquids' skins.
    fn steps_now(&mut self) -> u32 {
        let mut fastest = self.clock.get_mut().expect("the clock").fastest(self.frames);
        let from = f64::from(self.frames) * f64::from(FRAME);
        for (p, _) in &self.pours {
            if p.runs(from, f64::from(FRAME)) && p.speed.is_finite() {
                fastest = fastest.max(p.speed);
            }
        }
        for t in &self.timed {
            let v = glam::Vec2::from(t.drop.v).length() * self.layout.cell;
            if v.is_finite() {
                fastest = fastest.max(v);
            }
        }
        // (The strongest of the liquids' Gravity, and never less than the course's.)
        let pull = self.feels.iter().map(|f| f.gravity.abs()).filter(|g| g.is_finite()).fold(1.0f32, f32::max);
        // (And the forces' pushes, as for gravity.)
        let pushed: f32 = self.forces.iter().map(Force::headroom).sum();
        let fastest = fastest + (self.gravity.length() * pull + pushed) * 2.0 * FRAME;
        self.numbers.steps_for(fastest, &self.feels, self.layout.cell).min(MOST_STEPS)
    }

    /// One frame (1/60 s), cut into as many steps as the water needs, all sent to the card as one
    /// submission. It never waits for the card but for the report of two frames before.
    pub fn frame(&mut self) {
        self.settle();
        let n = self.steps_now();
        self.play_frame(n, None);
    }

    /// The frames that ran the deep part (deep water held up from below).
    pub fn deep_frames(&self) -> u32 {
        self.deep_frames
    }

    /// The spots of trapped air the card found at the end of the frame two before the last one
    /// (each is born an air drop when there is room).
    pub fn air_found(&self) -> u32 {
        self.clock.lock().expect("the clock").air
    }

    /// The steps the last frame took.
    pub fn steps_last_frame(&self) -> u32 {
        self.clock.lock().expect("the clock").last_steps
    }

    /// The pours' births for a frame of `n` steps, each step's own at its end (a pour whose spout
    /// was full two frames ago owes its rows instead), then the timed drops of each step, and each
    /// step's numbers: its drops, the drops before its births, where its births start, and its
    /// flags (bit 0 the last step, bit 1 the first, bits 8 and up its number).
    fn births(&mut self, n: u32) -> (Vec<Born>, Vec<[u32; 4]>) {
        let held = self.clock.get_mut().expect("the clock").spouts;
        let dt = f64::from(FRAME) / f64::from(n);
        let start = f64::from(self.frames) * f64::from(FRAME);
        let (fed, budget) = self.fed_shares(start);
        let mut left = budget.clone();
        // (The timed drops by the step their moment falls in: one before the frame into its first,
        // one at or past its end into its last; in the order given within a step.)
        let timed = std::mem::take(&mut self.timed);
        let step_of = |at: f64| {
            let j = ((at - start) / dt).floor();
            if j.is_nan() { 0 } else { j.clamp(0.0, f64::from(n - 1)) as u32 }
        };
        let mut order: Vec<(u32, usize)> = timed.iter().enumerate().map(|(i, t)| (step_of(t.at), i)).collect();
        order.sort_unstable();
        let mut next = 0;
        let mut born = Vec::new();
        let mut steps = Vec::with_capacity(n as usize);
        let mut count = self.count;
        for j in 0..n {
            let from = start + f64::from(j) * dt;
            let first = born.len() as u32;
            let room = |born: &Vec<Born>| {
                count + (born.len() as u32 - first) < MOST_DROPS && (born.len() as u32) < MOST_BIRTHS
            };
            for (k, (pour, state)) in self.pours.iter_mut().enumerate() {
                if held[k] >= self.numbers.full_spout {
                    pour.hold(state, &self.layout, from, dt, self.numbers.held_rows);
                    continue;
                }
                // (A fed pour's stream narrowed to its share of the tank this frame.)
                let narrowed = Pour { width: pour.width * fed[k] as f32, ..*pour };
                for d in narrowed.rows(state, &self.layout, from, dt) {
                    if !room(&born) || (pour.fed && left[k] == 0) {
                        break;
                    }
                    if pour.fed {
                        left[k] -= 1;
                    }
                    born.push(Born { xv: [d.x[0], d.x[1], d.v[0], d.v[1]], id: [d.kind, self.serial, d.colour, 0] });
                    self.serial = self.serial.wrapping_add(1);
                }
            }
            // (Moved on from its moment to the step's end: `x + v t + g t^2 / 2`, `v + g t`.)
            let end = from + dt;
            while let Some(&(_, i)) = order.get(next).filter(|o| o.0 == j) {
                next += 1;
                if !room(&born) {
                    continue;
                }
                let d = timed[i].drop;
                let t = (end - timed[i].at).clamp(0.0, dt) as f32;
                let g = if d.is_air() {
                    glam::Vec2::ZERO
                } else {
                    let pull = self.feels.get(d.liquid()).map_or(1.0, |f| f.gravity);
                    self.gravity / self.layout.cell * if pull.is_finite() { pull } else { 0.0 }
                };
                let (x, v) = (glam::Vec2::from(d.x), glam::Vec2::from(d.v));
                let (x, v) = (x + v * t + 0.5 * g * t * t, v + g * t);
                if !x.is_finite() || !v.is_finite() {
                    continue;
                }
                born.push(Born { xv: [x.x, x.y, v.x, v.y], id: [d.kind, self.serial, d.colour, 0] });
                self.serial = self.serial.wrapping_add(1);
            }
            let here = born.len() as u32 - first;
            let ends = u32::from(j + 1 == n) | (u32::from(j == 0) << 1) | (j << 8);
            steps.push([count + here, count, first, ends]);
            count += here;
        }
        // (What the fed pours poured comes out of the tank.)
        let poured_fed: u64 = budget.iter().zip(&left).map(|(b, l)| b - l).sum();
        self.tank = self.tank.saturating_sub(poured_fed);
        // (The reports of the next two frames do not see these drops yet.)
        let fastest = born.iter().map(|b| glam::Vec2::new(b.xv[2], b.xv[3]).length()).fold(0.0f32, f32::max);
        if !born.is_empty() {
            self.clock.get_mut().expect("the clock").added(self.frames, fastest * self.layout.cell);
        }
        let liquid = born.iter().filter(|b| b.id[0] & AIR == 0).count() as u32;
        self.poured = self.poured.wrapping_add(liquid);
        (born, steps)
    }

    /// Each pour's share of its stream this frame (from `start` seconds) and the most drops it may
    /// pour: 1 and no limit for a pour that is not fed. A fed pour, in the pours' order, may pour a
    /// sixth of what the tank still holds (at least a row while it holds one), never more than it
    /// would unfed; its stream is narrowed to that share, so it stays one stream.
    fn fed_shares(&self, start: f64) -> (Vec<f64>, Vec<u64>) {
        let mut left = self.tank;
        let mut shares = vec![1.0; self.pours.len()];
        let mut most = vec![u64::MAX; self.pours.len()];
        for (k, (p, _)) in self.pours.iter().enumerate() {
            if !p.fed {
                continue;
            }
            let full = p.amount(&self.layout, start, f64::from(FRAME));
            let tank = left as f64;
            let may = full.min((tank / 6.0).max(p.row(&self.layout).min(tank)));
            shares[k] = if full > 0.0 && may > 0.0 { (may / full).clamp(0.0, 1.0) } else { 0.0 };
            most[k] = (may.ceil() as u64).min(left);
            left -= most[k];
        }
        (shares, most)
    }

    /// Makes room in the births buffer for `n`.
    fn births_room(&mut self, n: u32) {
        let bytes = u64::from(n) * std::mem::size_of::<Born>() as u64;
        if bytes > self.births.size() {
            self.births = storage(&self.gpu.device, "water births", bytes.next_power_of_two());
            self.bind();
        }
    }

    /// One frame of `n` steps (keeping its moments through `keep`'s capture, as many as it asks:
    /// see [`capture`]).
    fn play_frame(&mut self, n: u32, mut keep: Option<(&mut capture::Capture, u32)>) {
        let dt = FRAME / n as f32;
        let capped = n >= self.numbers.most_steps.min(MOST_STEPS);
        let k = self.frames;
        let (born, records) = self.births(n);
        let end = records.last().map_or(self.count, |r| r[0]);
        // The deep part runs while frame k - 2 found deep water (and on the two frames after drops
        // are added: their reports have not seen them); the other frames only measure it, on their
        // last step. The pressure kept from the last deep frame's last step is scaled to
        // this frame's step (it is the push of one step); a frame after one that did not run starts
        // the pressure afresh and primes it with more cycles on its first step, as one that follows
        // an add does.
        let deep_on = self.numbers.deep != 0 && end > 0;
        let reported_deep = self.clock.get_mut().expect("the clock").deep;
        let deep_frame = deep_on && (self.deep_hold > 0 || reported_deep > 0);
        let measure_only = deep_on && !deep_frame;
        let prime = deep_frame && (!self.deep_last || self.prime_next);
        let kept = if self.deep_last && self.deep_dt > 0.0 { dt / self.deep_dt } else { 0.0 };
        // Trapped air is found on the last step whenever it measures the deep cells, and born at the
        // head of the drops that left, into room kept after the frame's drops (as many slots as the
        // report two frames back found, and some): the body still ends in drops that left.
        let air_on = self.numbers.air != 0 && deep_on;
        let found = self.clock.get_mut().expect("the clock").air;
        let slots =
            if air_on { air::slots_for(found, reported_deep, MOST_DROPS - end, self.numbers.air_most) } else { 0 };
        let mut frame = self.frame_gpu(dt, capped, kept);
        frame.dgrid[2] = self.serial;
        frame.dgrid[3] = slots;
        let mut done = None;
        if end > 0 {
            self.grow_to(end + slots);
            self.births_room(born.len() as u32);
            let counts: Vec<u32> = records.iter().map(|r| r[0]).collect();
            let packing = keep.as_mut().map(|(c, f)| capture::Packing::prepare(self, c, n, *f, &counts, end + slots));
            let cut_after = |j: u32| packing.as_ref().is_some_and(|p| p.cuts.contains(&j));
            let gpu = self.gpu.clone();
            let queue = &gpu.queue;
            queue.write_buffer(&self.frame_uniform, 0, bytemuck::bytes_of(&frame));
            if !born.is_empty() {
                queue.write_buffer(&self.births, 0, bytemuck::cast_slice(&born));
            }
            let mut steps = vec![0u8; n as usize * STEP_ROOM as usize];
            for (j, r) in records.iter().enumerate() {
                let at = j * STEP_ROOM as usize;
                steps[at..at + 16].copy_from_slice(bytemuck::cast_slice(r));
            }
            queue.write_buffer(&self.step_uniform, 0, &steps);
            let by_bin = self.bins().div_ceil(WORKGROUP);
            let by_block = self.bins().div_ceil(BLOCK);
            let by_cell = (self.layout.nodes() as u32).div_ceil(WORKGROUP);
            let iters = self.numbers.iters.clamp(1, 16) as usize;
            let joined = self.numbers.join != 0;
            let top = self.deep.levels.len() - 1;
            let tiles: Vec<(u32, u32)> = (0..=top).map(|l| self.deep.tiles(l)).collect();
            let (pre, post, coarse) = (self.numbers.pre, self.numbers.post, self.numbers.coarse);
            let drains_on = !self.drains.is_empty();
            let clock = self.clock.get_mut().expect("the clock");
            let slot = clock.slot_of(k);
            let mut encoder =
                gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("water frame") });
            encoder.clear_buffer(clock.on_card(slot), 0, None);
            // (Every frame, so a frame without the deep part reports no deep cells, and one that does
            // not look for trapped air reports none.)
            encoder.clear_buffer(&self.deep.count, 0, None);
            encoder.clear_buffer(&self.air.blocks, 0, None);
            let by_air_block = (self.layout.nodes() as u32).div_ceil(BLOCK);
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("water frame"),
                    timestamp_writes: None,
                });
                // (A pack pass's `now` is which of the frame's moments it keeps.)
                let pack_groups: &[wgpu::BindGroup] = packing.as_ref().map_or(&[], |p| &p.groups);
                let mut go2 = |p: P, now: usize, y_in: usize, step: u32, (gx, gy): (u32, u32)| {
                    pass.set_pipeline(&gpu.pipes.list[p as usize].0);
                    let offset = [step * STEP_ROOM as u32];
                    let group = match p {
                        P::Pack => &pack_groups[now],
                        P::Drained => &self.drained_groups[slot],
                        _ => &self.groups[(p as usize * 2 + now) * 2 + y_in],
                    };
                    pass.set_bind_group(0, group, &offset);
                    pass.dispatch_workgroups(gx.max(1), gy.max(1), 1);
                };
                macro_rules! go {
                    ($p:expr, $now:expr, $y:expr, $step:expr, $groups:expr) => {
                        go2($p, $now, $y, $step, ($groups, 1))
                    };
                }
                // (A pressure sweep of `level`, `times` over: both colours, each its own number.)
                macro_rules! sweep {
                    ($now:expr, $y:expr, $level:expr, $times:expr) => {
                        for _ in 0..$times {
                            for colour in 0..2u32 {
                                go2(P::DeepSor, $now, $y, TICK_SLOT + 2 * $level as u32 + colour, tiles[$level]);
                            }
                        }
                    };
                }
                for j in 0..n {
                    let now = self.now ^ (j as usize & 1);
                    let [count, before, _, _] = records[j as usize];
                    let by_drop = count.div_ceil(WORKGROUP);
                    // (The first step falls from the kept places; the others' fall was joined with
                    // the last step's end, and only their births come in here.)
                    if j == 0 || !joined || cut_after(j - 1) {
                        go!(P::Predict, now, 0, j, by_drop);
                    } else if count > before {
                        go!(P::Born, now, 0, j, (count - before).div_ceil(WORKGROUP));
                    }
                    go!(P::SumA, now, 0, j, by_block);
                    go!(P::SumB, now, 0, j, 1);
                    go!(P::SumC, now, 0, j, by_bin);
                    go!(P::Scatter, now, 0, j, by_drop);
                    go!(P::Order, now, 0, j, by_drop);
                    // (Before the drops feel each other: the sorted drops' places are where they
                    // would fall to, in `y[0]`, and `velb` holds where they were. The sweeps after
                    // take the push into their speeds.)
                    let y = 0;
                    if deep_frame || (measure_only && j + 1 == n) {
                        go!(P::DeepGather, now, y, j, by_drop);
                        go!(P::DeepCells, now, y, j, by_cell);
                        go!(P::DeepColumn, now, y, j, by_cell);
                        // (From the cells' sums, before the across pass empties them.)
                        if air_on && j + 1 == n {
                            go!(P::AirFind, now, y, j, by_cell);
                            go!(P::AirSumA, now, y, j, by_air_block);
                            go!(P::AirSumB, now, y, j, 1);
                        }
                        go!(P::DeepAcross, now, y, j, by_cell);
                    }
                    if deep_frame {
                        go!(P::DeepWant, now, y, j, by_cell);
                        let cycles = if prime && j == 0 { self.numbers.prime } else { self.numbers.cycles };
                        if top > 0 {
                            for _ in 0..cycles.max(1) {
                                sweep!(now, y, 0usize, pre);
                                for (level, &tile) in tiles.iter().enumerate().skip(1) {
                                    go2(P::DeepDown, now, y, TICK_SLOT + 2 * level as u32, tile);
                                    sweep!(now, y, level, if level == top { coarse } else { pre });
                                }
                                for (level, &tile) in tiles.iter().enumerate().take(top).rev() {
                                    go2(P::DeepUp, now, y, TICK_SLOT + 2 * level as u32, tile);
                                    sweep!(now, y, level, post);
                                }
                            }
                        } else {
                            sweep!(now, y, 0usize, coarse * cycles.max(1));
                        }
                        go!(P::DeepPush, now, y, j, by_drop);
                    }
                    for k in 0..iters {
                        go!(if k == 0 { P::LambdaFirst } else { P::Lambda }, now, k % 2, j, by_drop);
                        go!(if k + 1 == iters { P::ApplyLast } else { P::Apply }, now, k % 2, j, by_drop);
                    }
                    if joined && j + 1 < n && !cut_after(j) {
                        go!(P::FinishPredict, now, iters % 2, j + 1, by_drop);
                    } else {
                        go!(P::Finish, now, iters % 2, j, by_drop);
                    }
                    // (A moment kept inside the frame: the drops as this step left them.)
                    if let Some(k) = packing.as_ref().and_then(|p| p.cuts.iter().position(|&c| c == j)) {
                        go!(P::Pack, k, 0, PACK_SLOT + k as u32, by_drop);
                    }
                    // (What each drain took this frame, before trapped air is born over the drops
                    // that left.)
                    if drains_on && j + 1 == n {
                        go!(P::Drained, now, 0, j, by_drop);
                    }
                    // (After the drops in the course, over those that left and into the room kept:
                    // the kinds into the ones the step sorted into.)
                    if slots > 0 && j + 1 == n {
                        go!(P::AirDead, now, 0, j, slots.div_ceil(WORKGROUP));
                        go!(P::AirBorn, now, 0, j, by_cell);
                    }
                    // (The frame's end, with the air born at it.)
                    if let Some(p) = packing.as_ref().filter(|_| j + 1 == n) {
                        let last = p.cuts.len();
                        go!(P::Pack, last, 0, PACK_SLOT + last as u32, (end + slots).div_ceil(WORKGROUP));
                    }
                }
                pass.set_pipeline(&gpu.pipes.list[P::Report as usize].0);
                // (The kinds the last step sorted into, as its passes' binding 5.)
                let kinds = 1 - (self.now ^ ((n - 1) as usize & 1));
                pass.set_bind_group(0, &self.report_groups[slot * 2 + kinds], &[(n - 1) * STEP_ROOM as u32]);
                pass.dispatch_workgroups(end.div_ceil(WORKGROUP).max(1), 1, 1);
            }
            clock.copy_back(&mut encoder, k);
            let moments: Vec<(capture::Moment, u32)> = packing
                .as_ref()
                .map(|p| {
                    let inside =
                        p.cuts.iter().map(|&j| (capture::Moment { frame: k, steps: j + 1, of: n }, counts[j as usize]));
                    inside
                        .chain(std::iter::once((capture::Moment { frame: k + 1, steps: 0, of: 1 }, end + slots)))
                        .collect()
                })
                .unwrap_or_default();
            if let (Some(p), Some((c, _))) = (&packing, keep.as_ref()) {
                let sizes: Vec<u32> = moments.iter().map(|m| m.1).collect();
                p.copy_back(c, &mut encoder, &sizes);
            }
            let index = queue.submit([encoder.finish()]);
            if let (Some(p), Some((c, _))) = (packing, keep.as_mut()) {
                p.sent(c, moments, index.clone());
            }
            let _ = gpu.device.poll(wgpu::PollType::Poll);
            self.last = Some(index.clone());
            done = Some(index);
            self.now ^= n as usize & 1;
        }
        if done.is_none()
            && let Some((c, f)) = keep.as_mut()
        {
            let inside = capture::cuts(n, *f).into_iter().map(|j| capture::Moment { frame: k, steps: j + 1, of: n });
            let all: Vec<capture::Moment> =
                inside.chain(std::iter::once(capture::Moment { frame: k + 1, steps: 0, of: 1 })).collect();
            c.none(&all);
        }
        let end = if done.is_some() { end + slots } else { end };
        if done.is_some() {
            self.serial = self.serial.wrapping_add(slots);
        }
        self.count = end;
        let feeds = self.drains.iter().enumerate().filter(|(_, d)| d.feeds).fold(0u32, |m, (i, _)| m | (1 << i));
        self.clock.get_mut().expect("the clock").sent(k, end, self.poured, feeds, done, n);
        self.steps += u64::from(n);
        self.frames += 1;
        self.deep_last = deep_frame;
        self.deep_hold = self.deep_hold.saturating_sub(1);
        if deep_frame {
            self.deep_dt = dt;
            self.deep_frames += 1;
            self.prime_next = false;
        }
    }

    /// Waits until the card has done every frame sent so far.
    pub fn wait(&mut self) {
        if let Some(last) = self.last.take() {
            let _ = self.gpu.device.poll(wgpu::PollType::Wait { submission_index: Some(last), timeout: None });
        }
    }

    /// A copy that plays on exactly as this body does, from the frame it is at. (Waits for the
    /// card.)
    pub fn copy(&self) -> Water {
        let nodes = self.layout.nodes();
        let field = SolidField {
            layout: self.layout,
            distance: vec![0.0; nodes],
            wetting: vec![0.0; nodes],
            owner: vec![0; nodes],
        };
        let mut new = Water::new(self.gpu.clone(), &field, self.gravity, self.numbers, &self.feels);
        let device = &self.gpu.device;
        if self.room > new.room {
            new.by = ByDrop::new(device, self.room);
            new.room = self.room;
        }
        new.now = self.now;
        self.clock.lock().expect("the clock").copy_into(device, new.clock.get_mut().expect("the clock"));
        new.bind();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("water copy") });
        self.by.copy_kept(&mut encoder, &new.by, self.count, self.now);
        encoder.copy_buffer_to_buffer(&self.solid, 0, &new.solid, 0, self.solid.size());
        self.deep.copy_kept(&mut encoder, &new.deep);
        let done = self.gpu.queue.submit([encoder.finish()]);
        let _ = device.poll(wgpu::PollType::Wait { submission_index: Some(done), timeout: None });
        new.count = self.count;
        new.serial = self.serial;
        new.poured = self.poured;
        new.steps = self.steps;
        new.frames = self.frames;
        new.edges_shut = self.edges_shut;
        new.pours = self.pours.clone();
        new.timed = self.timed.clone();
        new.drains = self.drains.clone();
        new.tank = self.tank;
        new.forces = self.forces.clone();
        new.deep_last = self.deep_last;
        new.deep_dt = self.deep_dt;
        new.prime_next = self.prime_next;
        new.deep_hold = self.deep_hold;
        new.deep_frames = self.deep_frames;
        new
    }

    /// Waits for the card and reads `bytes` of `buffer` (tests and measuring only).
    fn read_buffer(&self, buffer: &wgpu::Buffer, bytes: u64) -> Vec<u8> {
        let device = &self.gpu.device;
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("water read"),
            size: bytes.max(4),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("water read") });
        encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, bytes.max(4));
        self.gpu.queue.submit([encoder.finish()]);
        staging.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        staging.slice(..).get_mapped_range().expect("mapped").to_vec()
    }

    /// Every drop in the course, once the frames sent so far are done, in `Drop`'s layout:
    /// its `c` is (how full its neighbourhood is, 0, which way the liquid lies from it), its `e` 0.
    /// (Drops that left are not read: their order among themselves does not repeat.) Waits for the
    /// card: for tests and measuring, never for a frame.
    pub fn read(&self) -> Vec<Drop> {
        let mut all = Vec::with_capacity(self.count as usize);
        let size = std::mem::size_of::<Drop>() as u64;
        let mut from = 0;
        while from < self.count {
            let n = (self.count - from).min(EXPORT_CHUNK);
            let out = storage(&self.gpu.device, "water read out", u64::from(n) * size);
            self.export_range(from, n, &out);
            all.extend(bytemuck::pod_collect_to_vec::<u8, Drop>(&self.read_buffer(&out, u64::from(n) * size)));
            from += n;
        }
        // (As the card's `alive`.)
        let (cols, rows) = ((self.layout.cols - 1) as f32, (self.layout.rows - 1) as f32);
        all.retain(|d| {
            let [x, y] = d.x;
            x.is_finite() && y.is_finite() && (0.0..=cols).contains(&x) && (0.0..=rows).contains(&y)
        });
        all
    }

    /// Writes drops `from` to `from + n` in `Drop`'s layout into `out`.
    fn export_range(&self, from: u32, n: u32, out: &wgpu::Buffer) {
        let gpu = &self.gpu;
        let slot = u64::from(MOST_STEPS) * STEP_ROOM;
        gpu.queue.write_buffer(&self.step_uniform, slot, bytemuck::cast_slice(&[from, n, 0u32, 0]));
        let (name, bindings) = ENTRIES[P::Export as usize];
        let entries: Vec<wgpu::BindGroupEntry> = bindings
            .iter()
            .map(|&b| {
                let resource = match b {
                    0 => self.frame_uniform.as_entire_binding(),
                    1 => self.step_binding(),
                    2 => self.by.xv.as_entire_binding(),
                    3 => self.by.nf.as_entire_binding(),
                    4 => self.by.id[self.now].as_entire_binding(),
                    _ => out.as_entire_binding(),
                };
                wgpu::BindGroupEntry { binding: b, resource }
            })
            .collect();
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(name),
            layout: &gpu.pipes.list[P::Export as usize].1,
            entries: &entries,
        });
        let mut encoder =
            gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("water export") });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("water export"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&gpu.pipes.list[P::Export as usize].0);
            pass.set_bind_group(0, &group, &[slot as u32]);
            pass.dispatch_workgroups(n.max(1).div_ceil(WORKGROUP), 1, 1);
        }
        gpu.queue.submit([encoder.finish()]);
    }
}

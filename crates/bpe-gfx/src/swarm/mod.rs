//! Huge swarm (particles part 8): a Particles thing's particles moved on the graphics card. The
//! world drives it: each particle step its thing works out the step's inputs on the CPU (births,
//! balls, walls, air) and [`SwarmBuffers::step`] submits one command buffer that moves the
//! particles in their GPU buffers (`shaders/swarm_step.wgsl`). Nothing comes back to the CPU while
//! stepping. A copy of the buffers ([`Clone`]) is a copy on the card, queued after every earlier
//! step, so checkpoints and snapshots are still copies the later steps never touch. The renderer
//! draws straight from the buffers ([`draw`]).

pub mod draw;

use std::cell::RefCell;
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, Weak};

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

/// The most particles a swarm holds.
pub const MOST_SWARM: u32 = 4_000_000;
/// The most forces a swarm applies (its Motion card's first ones).
pub const MOST_FORCES: usize = 16;
/// Threads in a workgroup (`WG` in swarm_step.wgsl).
const WORKGROUP: u32 = 256;
/// The fewest particles a swarm's buffers hold.
const LEAST_ROOM: u32 = 1024;

/// Force codes (`FORCE_*` in swarm_step.wgsl).
pub const FORCE_GRAVITY: u32 = 1;
pub const FORCE_WIND: u32 = 2;
pub const FORCE_DRAG: u32 = 3;
pub const FORCE_SWIRL: u32 = 4;
pub const FORCE_WANDER: u32 = 5;
pub const FORCE_RISE: u32 = 6;
pub const FORCE_SWAY: u32 = 7;
pub const FORCE_AIR: u32 = 8;
/// Particles part 3's forces (Flock isn't on the card).
pub const FORCE_PULL: u32 = 9;
pub const FORCE_SPRING: u32 = 10;
pub const FORCE_MAGNETIC: u32 = 11;
pub const FORCE_FLOW: u32 = 12;
pub const FORCE_SPIROGRAPH: u32 = 13;
pub const FORCE_PULSE: u32 = 14;
pub const FORCE_RIPPLE: u32 = 15;

/// Step flags (`STEP_*` in swarm_step.wgsl).
pub const STEP_CROWD: u32 = 1;
pub const STEP_BALLS: u32 = 2;
pub const STEP_WALLS: u32 = 4;
pub const STEP_EDGES: u32 = 8;
pub const STEP_AIR: u32 = 16;
/// Births only, nothing moved (the world's build).
pub const STEP_STILL: u32 = 32;

/// The half of a particle every pass reads. Matches `Main` in swarm_step.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct SpeckMain {
    pub place: [f32; 2],
    pub velocity: [f32; 2],
    /// The particle step it was born on.
    pub born: u32,
    /// Seconds.
    pub life: f32,
    pub seed: u32,
    /// Radians.
    pub turn: f32,
}

/// The rest of a particle. Matches `More` in swarm_step.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct SpeckMore {
    pub home: [f32; 2],
    /// Half size, pixels.
    pub size: f32,
    /// Radians a second.
    pub spin: f32,
    /// Flutter's phase, radians.
    pub flip: f32,
    /// The parent ball it came from (body row + 1), 0 when none.
    pub ball: u32,
    /// The word's, picture's or grid's place it belongs to (index + 1), 0 when none.
    pub spot: u32,
    pub spare: u32,
}

/// One force. Matches `Force` in swarm_step.wgsl: `kind` = [code, Only for (0 all, 1 younger
/// than, 2 older than, 3 a share, 4 the rest), the force's own word, another], `a` = [amount,
/// age, share, the force's own number], `b` and `c` the force's own numbers.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct ForceGpu {
    pub kind: [u32; 4],
    pub a: [f32; 4],
    pub b: [f32; 4],
    pub c: [f32; 4],
}

/// One step's settings. Matches `Step` in swarm_step.wgsl (see there for each field).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct StepGpu {
    pub counts: [u32; 4],
    pub more: [u32; 4],
    pub time: [f32; 4],
    pub meets: [f32; 4],
    pub edges: [f32; 4],
    pub balls: [f32; 4],
    pub ball_at: [u32; 4],
    pub ball_more: [u32; 4],
    pub walls: [f32; 4],
    pub wall_at: [u32; 4],
    pub wall_more: [u32; 4],
    pub air: [f32; 4],
    pub air_at: [u32; 4],
    pub wind: [f32; 4],
    pub gust: [f32; 4],
    /// What particles part 3's forces read (see `Step` in swarm_step.wgsl).
    pub middle: [f32; 4],
    pub parent: [f32; 4],
    pub homes: [f32; 4],
    pub lists: [u32; 4],
    pub fballs: [f32; 4],
    pub fball_at: [u32; 4],
    pub fball_more: [u32; 4],
    pub forces: [ForceGpu; MOST_FORCES],
}

/// What one particle step reads: its settings, the shared lists as 32-bit words (balls, the
/// ball grid, wall segments, the wall grid, the air; where each starts is in `step`) and the
/// births, oldest first. `step.counts[1]` and `step.more[1]` are filled in by
/// [`SwarmBuffers::step`].
pub struct StepIn<'a> {
    pub step: StepGpu,
    pub words: &'a [u32],
    pub births: &'a [(SpeckMain, SpeckMore)],
}

fn entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry { binding, resource: buffer.as_entire_binding() }
}

/// The compute pipelines of one device.
struct Pipelines {
    births: (wgpu::ComputePipeline, wgpu::BindGroupLayout),
    moves: (wgpu::ComputePipeline, wgpu::BindGroupLayout),
    scan_local: (wgpu::ComputePipeline, wgpu::BindGroupLayout),
    scan_sums: (wgpu::ComputePipeline, wgpu::BindGroupLayout),
    scatter: (wgpu::ComputePipeline, wgpu::BindGroupLayout),
}

/// A graphics card swarms step on: its device, its queue and the compute pipelines.
pub struct SwarmGpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipelines: Pipelines,
}

impl std::fmt::Debug for SwarmGpu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SwarmGpu")
    }
}

/// Every device's pipelines made so far.
static MADE: Mutex<Vec<Weak<SwarmGpu>>> = Mutex::new(Vec::new());

thread_local! {
    /// The card this thread's worlds step their swarms on.
    static ON_THREAD: RefCell<Option<Arc<SwarmGpu>>> = const { RefCell::new(None) };
}

/// Names the card the worlds built on this thread step their swarms on (`None`: none).
pub fn use_on_this_thread(gpu: Option<Arc<SwarmGpu>>) {
    ON_THREAD.with(|g| *g.borrow_mut() = gpu);
}

/// Names `gpu` the card this thread's worlds step their swarms on until the guard it gives is
/// dropped, when the card named before is named again.
pub fn use_for_now(gpu: Option<Arc<SwarmGpu>>) -> NamedCard {
    let before = on_this_thread();
    use_on_this_thread(gpu);
    NamedCard { before }
}

/// What [`use_for_now`] gives: names the card named before when dropped.
pub struct NamedCard {
    before: Option<Arc<SwarmGpu>>,
}

impl Drop for NamedCard {
    fn drop(&mut self) {
        use_on_this_thread(self.before.take());
    }
}

/// The card [`use_on_this_thread`] named, if any.
pub fn on_this_thread() -> Option<Arc<SwarmGpu>> {
    ON_THREAD.with(|g| g.borrow().clone())
}

/// The [`BufferBindingType`](wgpu::BufferBindingType) of a storage buffer.
fn storage(read_only: bool) -> wgpu::BindingType {
    wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Storage { read_only },
        has_dynamic_offset: false,
        min_binding_size: None,
    }
}

fn pipeline(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    entry: &str,
    bindings: &[(u32, wgpu::BindingType)],
) -> (wgpu::ComputePipeline, wgpu::BindGroupLayout) {
    let entries: Vec<wgpu::BindGroupLayoutEntry> = bindings
        .iter()
        .map(|&(binding, ty)| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty,
            count: None,
        })
        .collect();
    let group =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(entry), entries: &entries });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(entry),
        bind_group_layouts: &[Some(&group)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(entry),
        layout: Some(&layout),
        module,
        entry_point: Some(entry),
        compilation_options: Default::default(),
        cache: None,
    });
    (pipeline, group)
}

impl SwarmGpu {
    /// The swarm pipelines of `device`, made the first time they are asked for.
    pub fn for_device(device: &wgpu::Device, queue: &wgpu::Queue) -> Arc<SwarmGpu> {
        let mut made = MADE.lock().unwrap_or_else(|e| e.into_inner());
        made.retain(|w| w.strong_count() > 0);
        if let Some(gpu) = made.iter().filter_map(Weak::upgrade).find(|g| g.device == *device) {
            return gpu;
        }
        let gpu = Arc::new(SwarmGpu::new(device.clone(), queue.clone()));
        made.push(Arc::downgrade(&gpu));
        gpu
    }

    fn new(device: wgpu::Device, queue: wgpu::Queue) -> SwarmGpu {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("swarm step"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/swarm_step.wgsl").into()),
        });
        let uniform = wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let (r, w) = (storage(true), storage(false));
        let pipelines = Pipelines {
            births: pipeline(&device, &module, "cs_births", &[(0, uniform), (2, r), (3, r), (4, w), (5, w), (10, w)]),
            moves: pipeline(
                &device,
                &module,
                "cs_move",
                &[(0, uniform), (1, r), (4, w), (5, w), (6, w), (7, w), (8, w), (10, w)],
            ),
            scan_local: pipeline(&device, &module, "cs_scan_local", &[(0, uniform), (8, w), (9, w), (10, w)]),
            scan_sums: pipeline(&device, &module, "cs_scan_sums", &[(9, w), (10, w)]),
            scatter: pipeline(
                &device,
                &module,
                "cs_scatter",
                &[(0, uniform), (4, w), (5, w), (6, w), (7, w), (8, w), (9, w), (10, w)],
            ),
        };
        SwarmGpu { device, queue, pipelines }
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
}

/// A buffer that grows by doubling, made again (with what reads it) when it does.
struct Grows {
    label: &'static str,
    usage: wgpu::BufferUsages,
    buffer: Option<wgpu::Buffer>,
}

impl Grows {
    fn new(label: &'static str, usage: wgpu::BufferUsages) -> Grows {
        Grows { label, usage, buffer: None }
    }

    /// The buffer, at least `bytes` long.
    fn at_least(&mut self, device: &wgpu::Device, bytes: u64) -> &wgpu::Buffer {
        let bytes = bytes.max(256);
        if self.buffer.as_ref().is_none_or(|b| b.size() < bytes) {
            self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(self.label),
                size: bytes.next_power_of_two(),
                usage: self.usage,
                mapped_at_creation: false,
            }));
        }
        self.buffer.as_ref().expect("just made")
    }
}

/// What only stepping uses: never copied with the particles.
struct Scratch {
    uniform: wgpu::Buffer,
    words: Grows,
    births_main: Grows,
    births_more: Grows,
    main_b: Grows,
    more_b: Grows,
    flags: Grows,
    sums: Grows,
}

impl Scratch {
    fn new(device: &wgpu::Device) -> Scratch {
        let store = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
        Scratch {
            uniform: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("swarm step"),
                size: std::mem::size_of::<StepGpu>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            words: Grows::new("swarm inputs", store),
            births_main: Grows::new("swarm births", store),
            births_more: Grows::new("swarm births (more)", store),
            main_b: Grows::new("swarm moved", store),
            more_b: Grows::new("swarm moved (more)", store),
            flags: Grows::new("swarm flags", store),
            sums: Grows::new("swarm sums", store),
        }
    }
}

/// The small map-read that brings a swarm's count back late, for the status line.
struct CountReader {
    staging: wgpu::Buffer,
    /// A read is on its way.
    busy: bool,
    /// [`WAITING`], [`MAPPED`] or [`FAILED`]: what the card answered.
    answer: Arc<AtomicU8>,
    /// Births since the step whose count is on its way (the particles there can be now are at
    /// most that count and these).
    after: u32,
}

const WAITING: u8 = 0;
const MAPPED: u8 = 1;
const FAILED: u8 = 2;

/// The particles of one swarm on the card, oldest first, and their count.
pub struct SwarmBuffers {
    gpu: Arc<SwarmGpu>,
    main: wgpu::Buffer,
    more: wgpu::Buffer,
    /// [4, count, 0, 0, the step's working count, 3 spare]: the first four are the draw's
    /// arguments.
    counts: wgpu::Buffer,
    /// How many particles the buffers hold.
    room: u32,
    /// The most particles there can be now (none of them read back): what copies copy.
    bound: u32,
    scratch: Option<Scratch>,
    reader: Option<CountReader>,
    /// The count as the card last told it.
    hint: Arc<AtomicU32>,
}

impl std::fmt::Debug for SwarmBuffers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SwarmBuffers").field("room", &self.room).field("bound", &self.bound).finish()
    }
}

const PARTICLE_USAGE: wgpu::BufferUsages =
    wgpu::BufferUsages::STORAGE.union(wgpu::BufferUsages::COPY_SRC).union(wgpu::BufferUsages::COPY_DST);

fn particle_buffers(device: &wgpu::Device, room: u32) -> (wgpu::Buffer, wgpu::Buffer) {
    let make = |label, size: usize| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: (room as u64 * size as u64).max(256),
            usage: PARTICLE_USAGE,
            mapped_at_creation: false,
        })
    };
    (make("swarm particles", std::mem::size_of::<SpeckMain>()), make("swarm particles (more)", 32))
}

fn counts_buffer(device: &wgpu::Device, label: &str) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::cast_slice(&[4u32, 0, 0, 0, 0, 0, 0, 0]),
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::INDIRECT
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST,
    })
}

/// How much room `n` particles take: a power of two, at least [`LEAST_ROOM`], at most what the
/// biggest swarm needs (4 194 304, whose 32-byte halves are exactly 128 MiB).
fn room_for(n: u32) -> u32 {
    n.max(LEAST_ROOM).next_power_of_two().min(MOST_SWARM.next_power_of_two())
}

impl Clone for SwarmBuffers {
    /// A copy on the card of the particles as they will be once every step queued so far is
    /// done.
    fn clone(&self) -> Self {
        let device = &self.gpu.device;
        let room = room_for(self.bound);
        let (main, more) = particle_buffers(device, room);
        let counts = counts_buffer(device, "swarm counts");
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("swarm copy") });
        let n = self.bound.min(room) as u64;
        if n > 0 {
            encoder.copy_buffer_to_buffer(&self.main, 0, &main, 0, n * std::mem::size_of::<SpeckMain>() as u64);
            encoder.copy_buffer_to_buffer(&self.more, 0, &more, 0, n * std::mem::size_of::<SpeckMore>() as u64);
        }
        encoder.copy_buffer_to_buffer(&self.counts, 0, &counts, 0, 32);
        self.gpu.queue.submit([encoder.finish()]);
        SwarmBuffers {
            gpu: self.gpu.clone(),
            main,
            more,
            counts,
            room,
            bound: self.bound,
            scratch: None,
            reader: None,
            hint: Arc::new(AtomicU32::new(self.hint.load(Ordering::Relaxed))),
        }
    }
}

/// What the renderer draws a swarm from.
#[derive(Clone, Debug)]
pub struct SwarmSource {
    pub device: wgpu::Device,
    pub main: wgpu::Buffer,
    pub more: wgpu::Buffer,
    /// Its first four words are the draw's arguments.
    pub counts: wgpu::Buffer,
}

impl PartialEq for SwarmSource {
    fn eq(&self, other: &Self) -> bool {
        self.main == other.main && self.more == other.more && self.counts == other.counts
    }
}

impl SwarmBuffers {
    /// No particles yet, on `gpu`.
    pub fn new(gpu: Arc<SwarmGpu>) -> SwarmBuffers {
        let room = LEAST_ROOM;
        let (main, more) = particle_buffers(&gpu.device, room);
        let counts = counts_buffer(&gpu.device, "swarm counts");
        SwarmBuffers {
            gpu,
            main,
            more,
            counts,
            room,
            bound: 0,
            scratch: None,
            reader: None,
            hint: Arc::new(AtomicU32::new(0)),
        }
    }

    pub fn gpu(&self) -> &Arc<SwarmGpu> {
        &self.gpu
    }

    /// The most particles there can be now.
    pub fn bound(&self) -> u32 {
        self.bound
    }

    /// Bytes its particles take on the card (and what a copy of them takes).
    pub fn bytes(&self) -> usize {
        self.room as usize * (std::mem::size_of::<SpeckMain>() + std::mem::size_of::<SpeckMore>())
    }

    /// How many particles the card last said there were (a step or two late).
    pub fn count_hint(&self) -> u32 {
        self.hint.load(Ordering::Relaxed)
    }

    /// What the renderer draws it from.
    pub fn source(&self) -> SwarmSource {
        SwarmSource {
            device: self.gpu.device.clone(),
            main: self.main.clone(),
            more: self.more.clone(),
            counts: self.counts.clone(),
        }
    }

    /// Makes room for `n` particles, keeping the first `self.bound`.
    fn ensure_room(&mut self, n: u32) {
        if n <= self.room {
            return;
        }
        let device = &self.gpu.device;
        let room = room_for(n);
        let (main, more) = particle_buffers(device, room);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("swarm grow") });
        let kept = self.bound.min(self.room) as u64;
        if kept > 0 {
            encoder.copy_buffer_to_buffer(&self.main, 0, &main, 0, kept * std::mem::size_of::<SpeckMain>() as u64);
            encoder.copy_buffer_to_buffer(&self.more, 0, &more, 0, kept * std::mem::size_of::<SpeckMore>() as u64);
        }
        self.gpu.queue.submit([encoder.finish()]);
        self.main = main;
        self.more = more;
        self.room = room;
    }

    /// One particle step: `input.births` appended (as many as fit under `input.step.counts[2]`,
    /// Most particles), every particle moved, the dead taken out.
    pub fn step(&mut self, input: &StepIn) {
        let cap = input.step.counts[2].clamp(1, MOST_SWARM);
        let births = (input.births.len() as u32).min(cap);
        self.take_count();
        // More than Most can be alive when Most was just lowered: they live on, unborn to.
        let bound = (self.bound.saturating_add(births)).min(cap.max(self.bound));
        self.ensure_room(bound.max(1));
        let gpu = self.gpu.clone();
        let device = &gpu.device;
        let queue = &gpu.queue;
        let scratch = self.scratch.get_or_insert_with(|| Scratch::new(device));
        let mut step = input.step;
        step.counts[1] = births;
        step.counts[2] = cap;
        step.more[1] = self.room;
        queue.write_buffer(&scratch.uniform, 0, bytemuck::bytes_of(&step));
        let words: &[u32] = if input.words.is_empty() { &[0] } else { input.words };
        let words_buffer = scratch.words.at_least(device, words.len() as u64 * 4).clone();
        queue.write_buffer(&words_buffer, 0, bytemuck::cast_slice(words));
        let (born_main, born_more): (Vec<SpeckMain>, Vec<SpeckMore>) =
            input.births[..births as usize].iter().copied().unzip();
        let main_size = std::mem::size_of::<SpeckMain>() as u64;
        let more_size = std::mem::size_of::<SpeckMore>() as u64;
        let births_main = scratch.births_main.at_least(device, births.max(1) as u64 * main_size).clone();
        let births_more = scratch.births_more.at_least(device, births.max(1) as u64 * more_size).clone();
        if births > 0 {
            queue.write_buffer(&births_main, 0, bytemuck::cast_slice(&born_main));
            queue.write_buffer(&births_more, 0, bytemuck::cast_slice(&born_more));
        }
        let room = self.room as u64;
        let main_b = scratch.main_b.at_least(device, room * main_size).clone();
        let more_b = scratch.more_b.at_least(device, room * more_size).clone();
        let flags = scratch.flags.at_least(device, room * 2 * 4).clone();
        let sums = scratch.sums.at_least(device, room.div_ceil(WORKGROUP as u64) * 4).clone();
        let uniform = scratch.uniform.clone();
        let p = &gpu.pipelines;
        let group = |layout: &wgpu::BindGroupLayout, entries: &[wgpu::BindGroupEntry]| {
            device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("swarm step"), layout, entries })
        };
        let g_births = group(
            &p.births.1,
            &[
                entry(0, &uniform),
                entry(2, &births_main),
                entry(3, &births_more),
                entry(4, &self.main),
                entry(5, &self.more),
                entry(10, &self.counts),
            ],
        );
        let g_move = group(
            &p.moves.1,
            &[
                entry(0, &uniform),
                entry(1, &words_buffer),
                entry(4, &self.main),
                entry(5, &self.more),
                entry(6, &main_b),
                entry(7, &more_b),
                entry(8, &flags),
                entry(10, &self.counts),
            ],
        );
        let g_local =
            group(&p.scan_local.1, &[entry(0, &uniform), entry(8, &flags), entry(9, &sums), entry(10, &self.counts)]);
        let g_sums = group(&p.scan_sums.1, &[entry(9, &sums), entry(10, &self.counts)]);
        let g_scatter = group(
            &p.scatter.1,
            &[
                entry(0, &uniform),
                entry(4, &self.main),
                entry(5, &self.more),
                entry(6, &main_b),
                entry(7, &more_b),
                entry(8, &flags),
                entry(9, &sums),
                entry(10, &self.counts),
            ],
        );
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("swarm step") });
        {
            let mut pass = encoder
                .begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("swarm step"), timestamp_writes: None });
            let groups = bound.max(1).div_ceil(WORKGROUP);
            pass.set_pipeline(&p.births.0);
            pass.set_bind_group(0, &g_births, &[]);
            pass.dispatch_workgroups(births.max(1).div_ceil(WORKGROUP), 1, 1);
            pass.set_pipeline(&p.moves.0);
            pass.set_bind_group(0, &g_move, &[]);
            pass.dispatch_workgroups(groups, 1, 1);
            pass.set_pipeline(&p.scan_local.0);
            pass.set_bind_group(0, &g_local, &[]);
            pass.dispatch_workgroups(groups, 1, 1);
            pass.set_pipeline(&p.scan_sums.0);
            pass.set_bind_group(0, &g_sums, &[]);
            pass.dispatch_workgroups(1, 1, 1);
            pass.set_pipeline(&p.scatter.0);
            pass.set_bind_group(0, &g_scatter, &[]);
            pass.dispatch_workgroups(groups, 1, 1);
        }
        let ask = self.reader.as_ref().is_none_or(|r| !r.busy);
        if ask {
            let reader = self.reader.get_or_insert_with(|| CountReader {
                staging: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("swarm count"),
                    size: 32,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                busy: false,
                answer: Arc::new(AtomicU8::new(WAITING)),
                after: 0,
            });
            encoder.copy_buffer_to_buffer(&self.counts, 0, &reader.staging, 0, 32);
        }
        queue.submit([encoder.finish()]);
        if let Some(reader) = self.reader.as_mut() {
            if ask {
                reader.busy = true;
                reader.after = 0;
                let answer = reader.answer.clone();
                reader.staging.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                    answer.store(if r.is_ok() { MAPPED } else { FAILED }, Ordering::Release);
                });
            } else {
                reader.after = reader.after.saturating_add(births);
            }
        }
        let _ = device.poll(wgpu::PollType::Poll);
        self.bound = bound;
    }

    /// Takes the count the card sent back, when it has.
    /// Takes the count the card sent back, when it has, and lowers the bound to it (and the
    /// births since), so dispatches and copies shrink with the swarm.
    fn take_count(&mut self) {
        let Some(reader) = self.reader.as_mut().filter(|r| r.busy) else { return };
        match reader.answer.swap(WAITING, Ordering::Acquire) {
            MAPPED => {
                if let Ok(view) = reader.staging.slice(..).get_mapped_range() {
                    let words: &[u32] = bytemuck::cast_slice(&view);
                    let count = words[1];
                    self.hint.store(count, Ordering::Relaxed);
                    self.bound = self.bound.min(count.saturating_add(reader.after));
                }
                reader.staging.unmap();
                reader.busy = false;
            }
            FAILED => reader.busy = false,
            _ => {}
        }
    }

    /// Every particle as the card has it once its queued steps are done (waits for it: tests and
    /// tools only).
    pub fn read(&self) -> Vec<(SpeckMain, SpeckMore)> {
        let device = &self.gpu.device;
        let n = self.bound.min(self.room) as u64;
        let main_bytes = (n * std::mem::size_of::<SpeckMain>() as u64).max(4);
        let more_bytes = (n * std::mem::size_of::<SpeckMore>() as u64).max(4);
        let staging = |size| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("swarm read"),
                size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let (a, b, c) = (staging(main_bytes), staging(more_bytes), staging(32));
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("swarm read") });
        if n > 0 {
            encoder.copy_buffer_to_buffer(&self.main, 0, &a, 0, main_bytes);
            encoder.copy_buffer_to_buffer(&self.more, 0, &b, 0, more_bytes);
        }
        encoder.copy_buffer_to_buffer(&self.counts, 0, &c, 0, 32);
        self.gpu.queue.submit([encoder.finish()]);
        for s in [&a, &b, &c] {
            s.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        }
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        let count = {
            let view = c.slice(..).get_mapped_range().expect("mapped");
            bytemuck::cast_slice::<u8, u32>(&view)[1] as usize
        };
        let count = count.min(n as usize);
        let mains: Vec<SpeckMain> = if n > 0 {
            bytemuck::cast_slice(&a.slice(..).get_mapped_range().expect("mapped"))[..count].to_vec()
        } else {
            Vec::new()
        };
        let mores: Vec<SpeckMore> = if n > 0 {
            bytemuck::cast_slice(&b.slice(..).get_mapped_range().expect("mapped"))[..count].to_vec()
        } else {
            Vec::new()
        };
        mains.into_iter().zip(mores).collect()
    }
}

#[cfg(test)]
mod tests;

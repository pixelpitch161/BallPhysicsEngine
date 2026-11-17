//! The Liquid thing's drops on the graphics card (liquid part 1): a weakly compressible MLS-MPM
//! liquid (`shaders/liquid_sim.wgsl`). The world drives it: each liquid step the Liquid thing works
//! out on the processor what the step reads (births, the walls band when the walls moved, the
//! balls, every liquid's numbers and the meeting rules) and [`Body::step`] submits one command
//! buffer that moves the drops in their buffers. Nothing comes back but a late count and how long a
//! step took, for the status line. A copy ([`Clone`]) is a copy on the card, queued after every
//! earlier step, so checkpoints and snapshots are still copies the later steps never touch.
//!
//! Everything the drops add up on the grid is added in fixed point (integer atomics), and every
//! drop is moved by one thread, so the same card always makes the same drops.

#[cfg(test)]
mod tests;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Instant;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

/// The most drops a body holds: 2^20, so its biggest buffer (80 bytes a drop, 80 MiB) stays under
/// the 128 MiB a storage buffer may be bound with on every card (wgpu's default limits).
pub const MOST_DROPS: u32 = 1 << 20;
/// The most Liquid things one body holds.
pub const MOST_LIQUIDS: usize = 16;
/// The most grid nodes a body has (the Liquid thing grows its cell to stay under).
pub const MOST_NODES: u32 = 2_000_000;
/// Drops mix with their neighbours every this many liquid steps.
pub const MIX_EVERY: u32 = 4;
/// The most substeps a liquid step takes.
pub const MOST_SUBSTEPS: u32 = 64;
/// A drop's `kind` bit: it stops at the area's edges (a liquid that fills the scene).
pub const STOPS: u32 = 256;
/// A drop's `kind` bit: it is frozen (a stiff solid until it melts).
pub const FROZEN: u32 = 512;
/// A stain's liquid number is this plus its liquid's: the liquid's own draw never shows it, and
/// nothing moves it.
pub const STAIN_BASE: u32 = 16;
/// The most forces (heat, drains, currents, magnets) a liquid step reads.
pub const MOST_FORCES: usize = 48;
/// Force kinds ([`ForceGpu::c`]`[0]`).
pub const FORCE_HEAT: u32 = 0;
pub const FORCE_DRAIN: u32 = 1;
pub const FORCE_PUMP: u32 = 2;
pub const FORCE_WHIRLPOOL: u32 = 3;
pub const FORCE_WIND: u32 = 4;
pub const FORCE_MAGNET: u32 = 5;
/// The most walls that can paint the liquid (a band node's wall number is below it, or 255).
pub const MOST_PAINTING_WALLS: usize = 64;
/// A sample of the drops for Un-mix: 5 words a drop, after a 4-word header (the count).
pub const SAMPLE_WORDS: usize = 5;
/// The most bytes Un-mix's history keeps.
/// About the most Un-mix keeps on the card: callers turn it into a number of samples from a
/// drop count that never depends on timing ([`Body::record`] counts samples only).
pub const MOST_HISTORY_BYTES: usize = 256 << 20;
/// Bytes one drop takes in an Un-mix sample.
pub const SAMPLE_DROP_BYTES: usize = SAMPLE_WORDS * 4;
/// A drop's share of a grid cell (two drops a cell each way).
pub const DROP_VOLUME: f32 = 0.25;
/// Liquid steps submitted and not yet done before the stepping thread waits.
const IN_FLIGHT: usize = 3;
const WORKGROUP: u32 = 256;
/// The fewest drops a body's buffers hold.
const LEAST_ROOM: u32 = 1024;

/// Mix styles (`STYLE_*` in liquid_sim.wgsl).
pub const STYLE_PAINT: u32 = 0;
pub const STYLE_LIGHT: u32 = 1;
pub const STYLE_BLEND: u32 = 2;
pub const STYLE_MARBLE: u32 = 3;
pub const STYLE_KEEP_APART: u32 = 4;
/// When drops mix (`WHEN_*`).
pub const WHEN_ALWAYS: u32 = 0;
pub const WHEN_STIRRED: u32 = 1;
pub const WHEN_HITS: u32 = 2;
/// Meeting modes in a pair rule.
pub const MEET_MIX: u32 = 0;
pub const MEET_LAYER: u32 = 1;
/// Meeting drops of the two turn into a third liquid (the pair's target).
pub const MEET_REACT: u32 = 2;

/// One drop. Matches `Drop` in liquid_sim.wgsl (96 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct Drop {
    /// Grid units.
    pub x: [f32; 2],
    /// Grid units a second.
    pub v: [f32; 2],
    /// The APIC matrix: xx, xy, yx, yy.
    pub c: [f32; 4],
    /// Linear rgb and its strength.
    pub colour: [f32; 4],
    /// Its volume ratio (1 at rest).
    pub j: f32,
    /// Its liquid's index, and [`STOPS`].
    pub kind: u32,
    /// Its liquid's birth number: drops born before a liquid's floor are removed (Most drops).
    pub serial: u32,
    /// 1 when a ball touched it, fading (Only on hits mixing).
    pub kick: f32,
    /// Weight and viscosity moved away from its liquid's by mixing.
    pub mat: [f32; 2],
    /// Its temperature (0 to 1) and how dry it is (it goes past its own threshold).
    pub spare: [f32; 2],
    /// Its elastic stretch F (xx, xy, yx, yy): the identity at rest.
    pub f: [f32; 4],
}

impl Drop {
    /// A new drop at rest at `x` (grid units), moving at `v`.
    pub fn new(x: [f32; 2], v: [f32; 2], colour: [f32; 4], kind: u32, serial: u32) -> Drop {
        Drop { x, v, colour, kind, serial, j: 1.0, f: [1.0, 0.0, 0.0, 1.0], ..Default::default() }
    }

    /// The same drop at temperature `t`.
    pub fn at_heat(self, t: f32) -> Drop {
        Drop { spare: [if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.5 }, 0.0], ..self }
    }

    /// Whether it is a stain (a dried drop's mark).
    pub fn is_stain(&self) -> bool {
        (self.kind & 255) >= STAIN_BASE
    }

    /// Its liquid's index (a stain's too).
    pub fn liquid(&self) -> usize {
        (self.kind & 15) as usize
    }
}

/// One liquid's numbers. Matches `Liquid` in liquid_sim.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct LiquidGpu {
    /// Stiffness (E at weight 1), surface pull, tear, damping (per second).
    pub a: [f32; 4],
    /// Viscosity (cells² a second), weight, gravity ×, balls push it (0 or 1).
    pub b: [f32; 4],
    /// Mix style, 0, 0, the cut line (grid y: drops above it go; -1e30 none).
    pub c: [f32; 4],
    /// The serial floor, the newest serial (drops a reaction makes take it), 0, 0.
    pub d: [u32; 4],
    /// With the balls: push scale (1 / the ball's density to it, 0: it pushes no balls), drag
    /// (per second, fully under), carry (how much of its own speed it gives a ball), sticks.
    pub e: [f32; 4],
    /// The ink balls drop in it (linear rgb) and how much.
    pub f: [f32; 4],
    /// Splash size, splashes from (grid units a second), wakes, balls take its colour.
    pub g: [f32; 4],
    /// Stretch (its shear stiffness), tears at (the stretch's log), freezes below (-1: never),
    /// melts above.
    pub h: [f32; 4],
    /// Cools, warms, cold thickens it, dries (a second).
    pub i: [f32; 4],
    /// Stains (0 or 1), fades (a second), balls heat it, shapes paint it.
    pub j: [f32; 4],
    /// The colour it fades to (linear rgb).
    pub k: [f32; 4],
    /// The colour shapes paint it (linear rgb).
    pub l: [f32; 4],
    /// The colour a reaction that makes it gives its drops (linear rgb, strength).
    pub m: [f32; 4],
    /// What it does to the balls in it: shrink rate (a second, below 0 grows), dissolves (0 or 1).
    pub n: [f32; 4],
}

/// One force on the liquid, in grid units. Matches `Force` in liquid_sim.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct ForceGpu {
    /// Place x, y, reach, strength.
    pub a: [f32; 4],
    /// A way (x, y, unit) and two more numbers (see each kind).
    pub b: [f32; 4],
    /// Kind ([`FORCE_HEAT`]...), the liquids it moves (bit k for liquid k), spikes, 0.
    pub c: [u32; 4],
}

impl LiquidGpu {
    /// No cut line.
    pub const NO_CUT: f32 = -1e30;
}

/// One liquid step's settings. Matches `Step` in liquid_sim.wgsl (see there for each field).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct StepGpu {
    pub grid: [u32; 4],
    pub time: [f32; 4],
    pub counts: [u32; 4],
    pub flags: [u32; 4],
    pub more: [f32; 4],
    pub words_at: [u32; 4],
    pub ball_grid: [u32; 4],
    /// What the balls feel: recorded this step (0 or 1), 0, 0, 0.
    pub feel: [u32; 4],
    /// The liquid step's number (the dice), how many forces, 0, 0.
    pub extra: [u32; 4],
    pub paint: [[f32; 4]; 8],
    pub back: [[f32; 4]; 8],
    pub liquids: [LiquidGpu; MOST_LIQUIDS],
    pub pairs: [[u32; 4]; 64],
    /// A reacting pair's heat and fizz (0 to 255 each, bits 0..7 and 8..15).
    pub pairs2: [[u32; 4]; 64],
    pub forces: [ForceGpu; MOST_FORCES],
    /// Each wall number's liquids it paints (bit k for liquid k), 4 to a vec4.
    pub paints: [[u32; 4]; 16],
}

impl Default for StepGpu {
    fn default() -> Self {
        let (paint, back) = paint_tables();
        StepGpu {
            grid: [0; 4],
            time: [0.0; 4],
            counts: [0; 4],
            flags: [0; 4],
            more: [0.0; 4],
            words_at: [0; 4],
            ball_grid: [0; 4],
            feel: [0; 4],
            extra: [0; 4],
            paint,
            back,
            liquids: [LiquidGpu::default(); MOST_LIQUIDS],
            pairs: [[0; 4]; 64],
            pairs2: [[0; 4]; 64],
            forces: [ForceGpu::default(); MOST_FORCES],
            paints: [[0; 4]; 16],
        }
    }
}

impl StepGpu {
    /// Sets the meeting rule of liquid `a` with liquid `b`: `mode` ([`MEET_MIX`] or
    /// [`MEET_LAYER`]), `style`, `when`, and the share of the way a mixing moves (0 to 1).
    pub fn set_pair(&mut self, a: usize, b: usize, mode: u32, style: u32, when: u32, rate: f32) {
        if a >= MOST_LIQUIDS || b >= MOST_LIQUIDS {
            return;
        }
        let rate = if rate.is_finite() { (rate.clamp(0.0, 1.0) * 65535.0).round() as u32 } else { 0 };
        self.pairs[a * 4 + b / 4][b % 4] = (mode & 3) | ((style & 7) << 2) | ((when & 3) << 5) | (rate << 16);
    }

    /// Makes liquid `a` meeting liquid `b` react into liquid `makes` at `rate` (0 to 1), each
    /// turned drop warmed by `heat` and thrown up by `fizz` (0 to 1 each).
    pub fn set_react(&mut self, a: usize, b: usize, makes: usize, rate: f32, heat: f32, fizz: f32) {
        if a >= MOST_LIQUIDS || b >= MOST_LIQUIDS || makes >= MOST_LIQUIDS {
            return;
        }
        let unit = |x: f32, top: f32| if x.is_finite() { (x.clamp(0.0, 1.0) * top).round() as u32 } else { 0 };
        self.pairs[a * 4 + b / 4][b % 4] = MEET_REACT | ((makes as u32 & 15) << 7) | (unit(rate, 65535.0) << 16);
        self.pairs2[a * 4 + b / 4][b % 4] = unit(heat, 255.0) | (unit(fizz, 255.0) << 8);
    }

    /// Adds a force; false when the table is full.
    pub fn add_force(&mut self, f: ForceGpu) -> bool {
        let n = self.extra[1] as usize;
        if n >= MOST_FORCES {
            return false;
        }
        self.forces[n] = f;
        self.extra[1] += 1;
        true
    }

    /// Sets the liquids wall number `wall` paints.
    pub fn set_paints(&mut self, wall: usize, mask: u32) {
        if wall < MOST_PAINTING_WALLS {
            self.paints[wall / 4][wall % 4] = mask;
        }
    }
}

/// Paint's 8 bands (410 to 690 nm): blue, green and red's reflectance in each.
const BASIS: [[f32; 3]; 8] = [
    [0.9, 0.08, 0.02],
    [1.0, 0.0, 0.0],
    [0.5, 0.5, 0.0],
    [0.1, 0.9, 0.0],
    [0.03, 0.85, 0.12],
    [0.02, 0.25, 0.73],
    [0.02, 0.05, 0.93],
    [0.03, 0.03, 0.94],
];

/// Paint's tables: each band's basis (blue, green, red, 0) and the way back from 8 reflectances to
/// rgb (each band's r, g, b weights, 0): the basis's exact pseudo-inverse, so a colour mixed with
/// itself stays itself.
pub fn paint_tables() -> ([[f32; 4]; 8], [[f32; 4]; 8]) {
    // B is 8 × 3 with columns blue, green, red; back = (BᵀB)⁻¹ Bᵀ, 3 × 8, rows blue, green, red.
    let b: Vec<[f64; 3]> = BASIS.iter().map(|r| [r[0] as f64, r[1] as f64, r[2] as f64]).collect();
    let mut m = [[0.0f64; 3]; 3];
    for row in &b {
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] += row[i] * row[j];
            }
        }
    }
    let inv = invert3(m);
    let mut paint = [[0.0f32; 4]; 8];
    let mut back = [[0.0f32; 4]; 8];
    for (band, row) in b.iter().enumerate() {
        paint[band] = [row[0] as f32, row[1] as f32, row[2] as f32, 0.0];
        // The weight band `band` gives blue, green and red, then put in r, g, b order.
        let w: Vec<f64> = (0..3).map(|i| (0..3).map(|j| inv[i][j] * row[j]).sum()).collect();
        back[band] = [w[2] as f32, w[1] as f32, w[0] as f32, 0.0];
    }
    (paint, back)
}

fn invert3(m: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let d = 1.0 / det;
    [
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * d,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * d,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * d,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * d,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * d,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * d,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * d,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * d,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * d,
        ],
    ]
}

/// One node of the walls band: the node's index, the wall's normal there (pointing off a filled
/// wall, or out of a hollow one's outline), how far the node is from the wall's line along it
/// (grid units, below 0 on the other side), half the wall's thickness (grid units; below 0 for a
/// filled wall, which keeps the liquid on the normal's side) and the wall's speed there (grid
/// units a second). A drop is kept on its own side of a hollow wall's line, whichever side that
/// is, by a band half thick either side of it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WallNode {
    pub index: u32,
    pub n: [f32; 2],
    pub dist: f32,
    pub half: f32,
    pub v: [f32; 2],
    /// Its wall's number (for Shapes paint it), 255 for none.
    pub wall: u32,
}

/// One ball the drops meet, in grid units, and the liquids that meet it (bit `k` for liquid `k`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BallGpu {
    pub x: [f32; 2],
    pub v: [f32; 2],
    pub r: f32,
    pub mask: u32,
}

/// Words a ball's feel takes ([`Feel`]).
pub const FEEL_WORDS: usize = 32;
/// The points round a ball where the feel looks for liquid.
pub const RING: u32 = 32;
/// Fixed-point scales of the feel's sums: the push (momentum, grid units), and the touching mass
/// and what it is weighted by.
pub const S_PUSH: f32 = 64.0;
pub const S_TOUCH: f32 = 256.0;

/// Where each number is in a ball's [`FEEL_WORDS`] words.
pub mod feel_at {
    /// The momentum the drops gave it, x and y (fixed point, [`super::S_PUSH`]), scaled by 1 /
    /// its density to each liquid.
    pub const PUSH: usize = 0;
    /// The mass of drops that touched it ([`super::S_TOUCH`]), summed over every contact.
    pub const TOUCH: usize = 2;
    /// Their colour (rgb) times their mass.
    pub const COLOUR: usize = 3;
    /// Each touching liquid's drag, carry and take, times the touching mass.
    pub const DRAG: usize = 6;
    pub const CARRY: usize = 7;
    pub const TAKE: usize = 11;
    /// Each touching liquid's shrink rate (below 0 grows), and whether it dissolves, times the
    /// touching mass.
    pub const CHANGE: usize = 12;
    /// How many of the [`super::RING`] points round it are in the liquid (last step of the
    /// batch), and the liquid's mean speed there (f32 bits, grid units a second).
    pub const WET: usize = 8;
    pub const FLOW: usize = 9;
    /// Contacts with each liquid (16 words).
    pub const LIQUIDS: usize = 16;
}

/// What the balls felt over some liquid steps, on its way back from the card. Every copy of a
/// world shares one, so whichever asks first waits and all read the same words.
pub struct Feel {
    device: wgpu::Device,
    staging: wgpu::Buffer,
    index: wgpu::SubmissionIndex,
    balls: usize,
    answer: Arc<AtomicU8>,
    got: std::sync::OnceLock<Vec<i32>>,
}

impl std::fmt::Debug for Feel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Feel").field("balls", &self.balls).finish()
    }
}

impl Feel {
    /// How many balls it is for.
    pub fn balls(&self) -> usize {
        self.balls
    }

    /// Every ball's [`FEEL_WORDS`] words, waiting for the card when they aren't back yet (all
    /// zero if the read failed).
    pub fn wait(&self) -> &[i32] {
        self.got.get_or_init(|| {
            let words = self.balls * FEEL_WORDS;
            let _ =
                self.device.poll(wgpu::PollType::Wait { submission_index: Some(self.index.clone()), timeout: None });
            while self.answer.load(Ordering::Acquire) == WAITING {
                if self.device.poll(wgpu::PollType::wait_indefinitely()).is_err() {
                    break;
                }
            }
            if self.answer.load(Ordering::Acquire) != MAPPED {
                return vec![0; words];
            }
            let out = match self.staging.slice(..).get_mapped_range() {
                Ok(view) => bytemuck::cast_slice::<u8, i32>(&view)[..words].to_vec(),
                Err(_) => vec![0; words],
            };
            self.staging.unmap();
            out
        })
    }
}

/// What one liquid step reads. `step.grid`, `counts`, `words_at`, `ball_grid` and `flags[1..3]`
/// are filled in by [`Body::step`]; the rest is the caller's.
pub struct StepIn<'a> {
    pub step: StepGpu,
    pub births: &'a [Drop],
    /// The walls band, when it changed (or the body asks for it: [`Body::walls_ready`]).
    pub walls: Option<&'a [WallNode]>,
    pub balls: &'a [BallGpu],
    /// Record what the balls feel ([`Body::send_feel`] sends it back).
    pub feel: bool,
    /// Start the feel afresh (the first liquid step since the last [`Body::send_feel`]).
    pub fresh: bool,
}

/// The ball grid's cell, in grid units.
const BALL_CELL: f32 = 8.0;

fn storage(read_only: bool) -> wgpu::BindingType {
    wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Storage { read_only },
        has_dynamic_offset: false,
        min_binding_size: None,
    }
}

const UNIFORM: wgpu::BindingType = wgpu::BindingType::Buffer {
    ty: wgpu::BufferBindingType::Uniform,
    has_dynamic_offset: false,
    min_binding_size: None,
};

/// The bindings: 0 the step, 1 drops, 2 births, 3 counts, 4 grid, 5 solid, 6 words, 7 flags, 8
/// sums, 9 items, 10 spare, 11 mixout, 12 feel.
fn binding_type(b: u32) -> wgpu::BindingType {
    match b {
        0 => UNIFORM,
        2 | 6 | 14 | 15 => storage(true),
        _ => storage(false),
    }
}

/// Every entry point and the bindings it uses.
const ENTRIES: &[(&str, &[u32])] = &[
    ("cs_births", &[0, 1, 2, 3]),
    ("cs_births_done", &[0, 3]),
    ("cs_clear_solid", &[0, 5]),
    ("cs_put_solid", &[0, 5, 6]),
    ("cs_clear_grid", &[0, 4]),
    ("cs_p2g", &[0, 1, 3, 4]),
    ("cs_grid", &[0, 4, 5]),
    ("cs_g2p", &[0, 1, 3, 4, 5, 6, 12]),
    ("cs_scan_cells", &[0, 3]),
    ("cs_scan_drops", &[0, 3]),
    ("cs_bin_clear", &[0, 7]),
    ("cs_bin_count", &[0, 1, 3, 7]),
    ("cs_scan_local", &[0, 3, 7, 8]),
    ("cs_scan_sums", &[3, 8]),
    ("cs_bin_zero", &[0, 7]),
    ("cs_bin_place", &[0, 1, 3, 7, 8, 9]),
    ("cs_mix", &[0, 1, 3, 7, 8, 9, 11]),
    ("cs_mix_apply", &[0, 1, 3, 11]),
    ("cs_flag", &[0, 1, 3, 7, 10]),
    ("cs_scatter", &[0, 1, 3, 7, 8, 10]),
    ("cs_packed", &[3]),
    ("cs_feel", &[0, 3, 4, 6, 12]),
    ("cs_tick", &[3]),
    ("cs_record", &[0, 1, 3, 13]),
    ("cs_rewind", &[0, 1, 3, 6, 14, 15]),
    ("cs_rewind_done", &[0, 3, 14, 15]),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum E {
    Births,
    BirthsDone,
    ClearSolid,
    PutSolid,
    ClearGrid,
    P2g,
    Grid,
    G2p,
    ScanCells,
    ScanDrops,
    BinClear,
    BinCount,
    ScanLocal,
    ScanSums,
    BinZero,
    BinPlace,
    Mix,
    MixApply,
    Flag,
    Scatter,
    Packed,
    Feel,
    Tick,
    Record,
    Rewind,
    RewindDone,
}

/// The card the Liquid things step on: its device, its queue and the compute pipelines.
pub struct LiquidCard {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipelines: Vec<(wgpu::ComputePipeline, wgpu::BindGroupLayout)>,
    /// Dropped bodies' scratch, by grid nodes, for the next copy to step with: the editor copies
    /// its body about once a picture (a snapshot shares it), and a new grid each time was most of
    /// a copy's cost.
    spares: Mutex<Vec<(u32, Scratch)>>,
}

/// The most dropped bodies' scratch a card keeps.
const MOST_SPARES: usize = 4;

impl LiquidCard {
    fn take_spare(&self, cells: u32) -> Option<Scratch> {
        let mut spares = self.spares.lock().ok()?;
        let k = spares.iter().position(|(c, _)| *c == cells)?;
        Some(spares.swap_remove(k).1)
    }

    fn give_spare(&self, cells: u32, scratch: Scratch) {
        if let Ok(mut spares) = self.spares.lock() {
            if spares.len() >= MOST_SPARES {
                spares.remove(0);
            }
            spares.push((cells, scratch));
        }
    }
}

impl std::fmt::Debug for LiquidCard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LiquidCard")
    }
}

static MADE: Mutex<Vec<Weak<LiquidCard>>> = Mutex::new(Vec::new());

impl LiquidCard {
    /// The liquid pipelines of `device`, made the first time they are asked for.
    pub fn for_device(device: &wgpu::Device, queue: &wgpu::Queue) -> Arc<LiquidCard> {
        let mut made = MADE.lock().unwrap_or_else(|e| e.into_inner());
        made.retain(|w| w.strong_count() > 0);
        if let Some(card) = made.iter().filter_map(Weak::upgrade).find(|c| c.device == *device) {
            return card;
        }
        let card = Arc::new(LiquidCard::new(device.clone(), queue.clone()));
        made.push(Arc::downgrade(&card));
        card
    }

    /// The card this thread's worlds step on: the one named for Huge swarms
    /// ([`crate::swarm::on_this_thread`]), so one naming covers every thing on the card.
    pub fn on_this_thread() -> Option<Arc<LiquidCard>> {
        crate::swarm::on_this_thread().map(|s| LiquidCard::for_device(s.device(), s.queue()))
    }

    fn new(device: wgpu::Device, queue: wgpu::Queue) -> LiquidCard {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("liquid step"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/liquid_sim.wgsl").into()),
        });
        let pipelines = ENTRIES
            .iter()
            .map(|(entry, bindings)| {
                let entries: Vec<wgpu::BindGroupLayoutEntry> = bindings
                    .iter()
                    .map(|&b| wgpu::BindGroupLayoutEntry {
                        binding: b,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: binding_type(b),
                        count: None,
                    })
                    .collect();
                let group = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some(entry),
                    entries: &entries,
                });
                let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some(entry),
                    bind_group_layouts: &[Some(&group)],
                    immediate_size: 0,
                });
                let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(entry),
                    layout: Some(&layout),
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    cache: None,
                });
                (pipeline, group)
            })
            .collect();
        LiquidCard { device, queue, pipelines, spares: Mutex::new(Vec::new()) }
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    fn pipeline(&self, e: E) -> &(wgpu::ComputePipeline, wgpu::BindGroupLayout) {
        &self.pipelines[e as usize]
    }
}

/// A buffer that grows by doubling.
struct Grows {
    label: &'static str,
    usage: wgpu::BufferUsages,
    buffer: Option<wgpu::Buffer>,
}

impl Grows {
    fn new(label: &'static str, usage: wgpu::BufferUsages) -> Grows {
        Grows { label, usage, buffer: None }
    }

    fn at_least(&mut self, device: &wgpu::Device, bytes: u64) -> wgpu::Buffer {
        let bytes = bytes.max(256);
        if self.buffer.as_ref().is_none_or(|b| b.size() < bytes) {
            self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(self.label),
                size: bytes.next_power_of_two(),
                usage: self.usage,
                mapped_at_creation: false,
            }));
        }
        self.buffer.clone().expect("just made")
    }
}

/// What only stepping uses (the walls band is here too: a copy uploads it again).
struct Scratch {
    uniform: wgpu::Buffer,
    births: Grows,
    words: Grows,
    grid: wgpu::Buffer,
    solid: wgpu::Buffer,
    flags: Grows,
    sums: Grows,
    items: Grows,
    spare: Grows,
    mixout: Grows,
    feel: Grows,
}

const STORE: wgpu::BufferUsages = wgpu::BufferUsages::STORAGE.union(wgpu::BufferUsages::COPY_DST);

impl Scratch {
    fn new(device: &wgpu::Device, cells: u32) -> Scratch {
        let fixed = |label, bytes: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes.max(256),
                usage: STORE,
                mapped_at_creation: false,
            })
        };
        Scratch {
            uniform: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("liquid step"),
                size: std::mem::size_of::<StepGpu>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            births: Grows::new("liquid births", STORE),
            words: Grows::new("liquid words", STORE),
            grid: fixed("liquid grid", cells as u64 * 16),
            solid: fixed("liquid walls", cells as u64 * 32),
            flags: Grows::new("liquid flags", STORE),
            sums: Grows::new("liquid sums", STORE),
            items: Grows::new("liquid bins", STORE),
            spare: Grows::new("liquid spare", STORE),
            mixout: Grows::new("liquid mixing", STORE),
            feel: Grows::new("liquid feel", STORE.union(wgpu::BufferUsages::COPY_SRC)),
        }
    }
}

const WAITING: u8 = 0;
const MAPPED: u8 = 1;
const FAILED: u8 = 2;

/// The small map-read that brings the count back late, and when the step it belongs to was sent.
struct CountReader {
    staging: wgpu::Buffer,
    busy: bool,
    answer: Arc<AtomicU8>,
    after: u32,
    sent: Instant,
    /// When the card answered (micros since `sent`).
    took: Arc<AtomicU32>,
}

/// The drops of every Liquid thing of a world, on the card, and the grid they move on.
pub struct Body {
    gpu: Arc<LiquidCard>,
    cols: u32,
    rows: u32,
    drops: wgpu::Buffer,
    /// [4, count, 0, 0, working count, scan length, scan total, 0]: the first four are the draw's.
    counts: wgpu::Buffer,
    room: u32,
    bound: u32,
    scratch: Option<Scratch>,
    walls_ready: bool,
    reader: Option<CountReader>,
    hint: Arc<AtomicU32>,
    /// Micros the last answered step took, as f32 bits.
    ms: Arc<AtomicU32>,
    in_flight: VecDeque<wgpu::SubmissionIndex>,
    steps: u32,
    /// Un-mix's samples of the drops, oldest first. Never changed once written, so copies share
    /// them.
    history: VecDeque<Arc<Sample>>,
    /// Liquid steps played back into the gap before the newest sample.
    into: u32,
}

/// One sample of every drop, for Un-mix ([`SAMPLE_WORDS`] words a drop after a 4-word header
/// holding the count).
pub struct Sample {
    buffer: wgpu::Buffer,
    /// The most drops it can hold.
    bound: u32,
}

impl Sample {
    pub fn bytes(&self) -> usize {
        self.buffer.size() as usize
    }
}

impl std::fmt::Debug for Body {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Body").field("cols", &self.cols).field("rows", &self.rows).field("bound", &self.bound).finish()
    }
}

const DROP_USAGE: wgpu::BufferUsages =
    wgpu::BufferUsages::STORAGE.union(wgpu::BufferUsages::COPY_SRC).union(wgpu::BufferUsages::COPY_DST);

fn drops_buffer(device: &wgpu::Device, room: u32) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("liquid drops"),
        size: (room as u64 * std::mem::size_of::<Drop>() as u64).max(256),
        usage: DROP_USAGE,
        mapped_at_creation: false,
    })
}

fn counts_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("liquid counts"),
        contents: bytemuck::cast_slice(&[4u32, 0, 0, 0, 0, 0, 0, 0]),
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::INDIRECT
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST,
    })
}

fn room_for(n: u32) -> u32 {
    n.max(LEAST_ROOM).next_power_of_two().min(MOST_DROPS.next_power_of_two())
}

/// What the renderer draws the drops from.
#[derive(Clone, Debug)]
pub struct DropsSource {
    pub device: wgpu::Device,
    pub drops: wgpu::Buffer,
    /// Its first four words are the draw's arguments.
    pub counts: wgpu::Buffer,
}

impl PartialEq for DropsSource {
    fn eq(&self, other: &Self) -> bool {
        self.drops == other.drops && self.counts == other.counts
    }
}

impl Clone for Body {
    /// A copy on the card of the drops as they will be once every step queued so far is done. Its
    /// walls band is put on the grid again before its first step.
    fn clone(&self) -> Self {
        let device = &self.gpu.device;
        let room = room_for(self.bound);
        let drops = drops_buffer(device, room);
        let counts = counts_buffer(device);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("liquid copy") });
        let n = self.bound.min(room) as u64;
        if n > 0 {
            encoder.copy_buffer_to_buffer(&self.drops, 0, &drops, 0, n * std::mem::size_of::<Drop>() as u64);
        }
        encoder.copy_buffer_to_buffer(&self.counts, 0, &counts, 0, 32);
        self.gpu.queue.submit([encoder.finish()]);
        Body {
            gpu: self.gpu.clone(),
            cols: self.cols,
            rows: self.rows,
            drops,
            counts,
            room,
            bound: self.bound,
            // Its walls band is put on again either way (a spare's is another body's).
            scratch: self.gpu.take_spare(self.cols * self.rows),
            walls_ready: false,
            reader: None,
            hint: Arc::new(AtomicU32::new(self.hint.load(Ordering::Relaxed))),
            ms: Arc::new(AtomicU32::new(self.ms.load(Ordering::Relaxed))),
            in_flight: VecDeque::new(),
            steps: self.steps,
            history: self.history.clone(),
            into: self.into,
        }
    }
}

impl std::ops::Drop for Body {
    fn drop(&mut self) {
        if let Some(scratch) = self.scratch.take() {
            self.gpu.give_spare(self.cols * self.rows, scratch);
        }
    }
}

fn entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry { binding, resource: buffer.as_entire_binding() }
}

impl Body {
    /// No drops yet, on a `cols` × `rows` grid (at most [`MOST_NODES`] nodes, at least 8 each way).
    pub fn new(gpu: Arc<LiquidCard>, cols: u32, rows: u32) -> Body {
        let (cols, rows) = (cols.max(8), rows.max(8));
        let rows = rows.min((MOST_NODES / cols).max(8));
        let room = LEAST_ROOM;
        let drops = drops_buffer(&gpu.device, room);
        let counts = counts_buffer(&gpu.device);
        Body {
            gpu,
            cols,
            rows,
            drops,
            counts,
            room,
            bound: 0,
            scratch: None,
            walls_ready: false,
            reader: None,
            hint: Arc::new(AtomicU32::new(0)),
            ms: Arc::new(AtomicU32::new(0)),
            in_flight: VecDeque::new(),
            steps: 0,
            history: VecDeque::new(),
            into: 0,
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.cols, self.rows)
    }

    /// The most drops there can be now.
    pub fn bound(&self) -> u32 {
        self.bound
    }

    /// Whether the grid has the walls band (a new body or a copy needs it put on again).
    pub fn walls_ready(&self) -> bool {
        self.walls_ready
    }

    /// How many drops the card last said there were.
    pub fn count_hint(&self) -> u32 {
        self.hint.load(Ordering::Relaxed)
    }

    /// How long the last answered liquid step took on the card, in milliseconds (with the wait in
    /// its queue).
    pub fn step_ms(&self) -> f32 {
        f32::from_bits(self.ms.load(Ordering::Relaxed))
    }

    /// Bytes the drops take on the card (and what a copy of them takes).
    pub fn bytes(&self) -> usize {
        self.room as usize * std::mem::size_of::<Drop>()
    }

    /// What the renderer draws the drops from.
    pub fn source(&self) -> DropsSource {
        DropsSource { device: self.gpu.device.clone(), drops: self.drops.clone(), counts: self.counts.clone() }
    }

    fn ensure_room(&mut self, n: u32) {
        if n <= self.room {
            return;
        }
        let device = &self.gpu.device;
        let room = room_for(n);
        let drops = drops_buffer(device, room);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("liquid grow") });
        let kept = self.bound.min(self.room) as u64;
        if kept > 0 {
            encoder.copy_buffer_to_buffer(&self.drops, 0, &drops, 0, kept * std::mem::size_of::<Drop>() as u64);
        }
        self.gpu.queue.submit([encoder.finish()]);
        self.drops = drops;
        self.room = room;
    }

    /// One liquid step: births appended (as many as fit under [`MOST_DROPS`], counted on the card,
    /// so how many fit never hangs on when a count came back), the walls band put
    /// on when given, `step.grid[2]` substeps, mixing when `step.flags[0]` is 1, the dead removed.
    pub fn step(&mut self, input: &StepIn) {
        self.take_count();
        self.wait_for_room();
        let births = (input.births.len() as u32).min(MOST_DROPS);
        let bound = self.bound.saturating_add(births).min(MOST_DROPS);
        self.ensure_room(bound.max(1));
        let gpu = self.gpu.clone();
        let (device, queue) = (&gpu.device, &gpu.queue);
        let cells = self.cols * self.rows;
        let scratch = self.scratch.get_or_insert_with(|| Scratch::new(device, cells));
        let half = self.room.max(cells);

        // The words: the walls band, then the balls and their grid.
        let mut words: Vec<u32> = Vec::new();
        let walls = input.walls.filter(|w| !w.is_empty());
        let put_walls = input.walls.is_some() || !self.walls_ready;
        let walls_at = words.len() as u32;
        if let Some(w) = walls {
            for n in w {
                words.extend([n.index, n.n[0].to_bits(), n.n[1].to_bits(), n.dist.to_bits()]);
                words.extend([n.v[0].to_bits(), n.v[1].to_bits(), n.half.to_bits(), n.wall]);
            }
        }
        let wall_count = walls.map_or(0, |w| w.len() as u32);
        // The balls move on along their speed through the step's substeps.
        let travel = input.step.time[0].abs() * input.step.grid[2].clamp(1, MOST_SUBSTEPS) as f32;
        let travel = if travel.is_finite() { travel } else { 0.0 };
        let (balls_at, starts_at, items_at, ball_cols, ball_rows, ball_count) =
            put_balls(&mut words, input.balls, self.cols, self.rows, travel);
        if words.is_empty() {
            words.push(0);
        }

        let mut step = input.step;
        step.grid[0] = self.cols;
        step.grid[1] = self.rows;
        step.grid[2] = step.grid[2].clamp(1, MOST_SUBSTEPS);
        step.counts = [births, self.room, half, cells];
        step.flags[1] = u32::from(put_walls);
        step.flags[2] = ball_count;
        step.words_at = [walls_at, wall_count, balls_at, starts_at];
        step.ball_grid = [ball_cols, ball_rows, BALL_CELL.to_bits(), items_at];
        step.feel = [u32::from(input.feel && ball_count > 0), RING, 0, 0];
        queue.write_buffer(&scratch.uniform, 0, bytemuck::bytes_of(&step));

        let drop_size = std::mem::size_of::<Drop>() as u64;
        let births_buffer = scratch.births.at_least(device, births.max(1) as u64 * drop_size);
        if births > 0 {
            queue.write_buffer(&births_buffer, 0, bytemuck::cast_slice(&input.births[..births as usize]));
        }
        let words_buffer = scratch.words.at_least(device, words.len() as u64 * 4);
        queue.write_buffer(&words_buffer, 0, bytemuck::cast_slice(&words));
        let room = self.room as u64;
        let flags = scratch.flags.at_least(device, half as u64 * 2 * 4);
        let sums = scratch.sums.at_least(device, (half as u64).div_ceil(WORKGROUP as u64) * 4 + 4);
        let items = scratch.items.at_least(device, room * 4);
        let spare = scratch.spare.at_least(device, room * drop_size);
        let mixout = scratch.mixout.at_least(device, room * 48);
        let felt = if input.feel { ball_count } else { 0 };
        let feel = scratch.feel.at_least(device, (felt.max(1) as usize * FEEL_WORDS * 4) as u64);
        let buffers: [&wgpu::Buffer; 13] = [
            &scratch.uniform,
            &self.drops,
            &births_buffer,
            &self.counts,
            &scratch.grid,
            &scratch.solid,
            &words_buffer,
            &flags,
            &sums,
            &items,
            &spare,
            &mixout,
            &feel,
        ];
        let group = |e: E| {
            let (_, layout) = gpu.pipeline(e);
            let bindings = ENTRIES[e as usize].1;
            let entries: Vec<wgpu::BindGroupEntry> = bindings.iter().map(|&b| entry(b, buffers[b as usize])).collect();
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("liquid step"),
                layout,
                entries: &entries,
            })
        };
        let all = [
            E::Births,
            E::BirthsDone,
            E::ClearSolid,
            E::PutSolid,
            E::ClearGrid,
            E::P2g,
            E::Grid,
            E::G2p,
            E::ScanCells,
            E::ScanDrops,
            E::BinClear,
            E::BinCount,
            E::ScanLocal,
            E::ScanSums,
            E::BinZero,
            E::BinPlace,
            E::Mix,
            E::MixApply,
            E::Flag,
            E::Scatter,
            E::Packed,
            E::Feel,
            E::Tick,
        ];
        let groups: Vec<wgpu::BindGroup> = all.iter().map(|e| group(*e)).collect();
        let drop_groups = bound.max(1).div_ceil(WORKGROUP);
        let cell_groups = cells.div_ceil(WORKGROUP);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("liquid step") });
        if input.feel && input.fresh {
            encoder.clear_buffer(&feel, 0, None);
        }
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("liquid step"),
                timestamp_writes: None,
            });
            let mut run = |e: E, n: u32| {
                pass.set_pipeline(&gpu.pipeline(e).0);
                pass.set_bind_group(0, &groups[e as usize], &[]);
                pass.dispatch_workgroups(n.max(1), 1, 1);
            };
            run(E::Births, births.max(1).div_ceil(WORKGROUP));
            run(E::BirthsDone, 1);
            if put_walls {
                run(E::ClearSolid, cell_groups);
                if wall_count > 0 {
                    run(E::PutSolid, wall_count.div_ceil(WORKGROUP));
                }
            }
            for _ in 0..step.grid[2] {
                run(E::ClearGrid, cell_groups);
                run(E::P2g, drop_groups);
                run(E::Grid, cell_groups);
                run(E::G2p, drop_groups);
                run(E::Tick, 1);
            }
            if felt > 0 {
                run(E::Feel, felt.div_ceil(WORKGROUP));
            }
            if step.flags[0] == 1 {
                run(E::BinClear, cell_groups);
                run(E::BinCount, drop_groups);
                run(E::ScanCells, 1);
                run(E::ScanLocal, cell_groups);
                run(E::ScanSums, 1);
                run(E::BinZero, cell_groups);
                run(E::BinPlace, drop_groups);
                run(E::Mix, drop_groups);
                run(E::MixApply, drop_groups);
            }
            run(E::Flag, drop_groups);
            run(E::ScanDrops, 1);
            run(E::ScanLocal, drop_groups);
            run(E::ScanSums, 1);
            run(E::Scatter, drop_groups);
            run(E::Packed, 1);
        }
        let ask = self.reader.as_ref().is_none_or(|r| !r.busy);
        if ask {
            let reader = self.reader.get_or_insert_with(|| CountReader {
                staging: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("liquid count"),
                    size: 32,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                busy: false,
                answer: Arc::new(AtomicU8::new(WAITING)),
                after: 0,
                sent: Instant::now(),
                took: Arc::new(AtomicU32::new(0)),
            });
            encoder.copy_buffer_to_buffer(&self.counts, 0, &reader.staging, 0, 32);
        }
        let index = queue.submit([encoder.finish()]);
        self.in_flight.push_back(index);
        if let Some(reader) = self.reader.as_mut() {
            if ask {
                reader.busy = true;
                reader.after = 0;
                reader.sent = Instant::now();
                let (answer, took, sent) = (reader.answer.clone(), reader.took.clone(), reader.sent);
                reader.staging.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                    took.store(sent.elapsed().as_micros().min(u32::MAX as u128) as u32, Ordering::Relaxed);
                    answer.store(if r.is_ok() { MAPPED } else { FAILED }, Ordering::Release);
                });
            } else {
                reader.after = reader.after.saturating_add(births);
            }
        }
        let _ = device.poll(wgpu::PollType::Poll);
        self.walls_ready = true;
        self.bound = bound;
        self.steps += 1;
    }

    /// Waits for the oldest step sent while too many are on their way.
    fn wait_for_room(&mut self) {
        while self.in_flight.len() >= IN_FLIGHT {
            let Some(oldest) = self.in_flight.pop_front() else { break };
            let _ = self.gpu.device.poll(wgpu::PollType::Wait { submission_index: Some(oldest), timeout: None });
        }
        self.take_count();
    }

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
                let ms = reader.took.load(Ordering::Relaxed) as f32 / 1000.0;
                self.ms.store(ms.to_bits(), Ordering::Relaxed);
            }
            FAILED => reader.busy = false,
            _ => {}
        }
    }

    /// Sends back what `balls` balls felt over the liquid steps since the last one sent (those
    /// stepped with [`StepIn::feel`], the first of them [`StepIn::fresh`]); `None` with no balls.
    /// The next step that records starts afresh.
    pub fn send_feel(&mut self, balls: u32) -> Option<Arc<Feel>> {
        if balls == 0 {
            return None;
        }
        let device = self.gpu.device.clone();
        let scratch = self.scratch.as_mut()?;
        let bytes = balls as u64 * FEEL_WORDS as u64 * 4;
        let feel = scratch.feel.at_least(&device, bytes);
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("liquid feel read"),
            size: bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("liquid feel") });
        encoder.copy_buffer_to_buffer(&feel, 0, &staging, 0, bytes);
        let index = self.gpu.queue.submit([encoder.finish()]);
        let answer = Arc::new(AtomicU8::new(WAITING));
        let told = answer.clone();
        staging.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            told.store(if r.is_ok() { MAPPED } else { FAILED }, Ordering::Release);
        });
        let _ = device.poll(wgpu::PollType::Poll);
        Some(Arc::new(Feel { device, staging, index, balls: balls as usize, answer, got: std::sync::OnceLock::new() }))
    }

    /// Keeps a sample of every drop as they are once the queued steps are done, for Un-mix; then
    /// forgets the oldest past `most` samples (by count only, so every run keeps the same).
    pub fn record(&mut self, most: usize) {
        self.into = 0;
        let device = self.gpu.device.clone();
        let bound = self.bound.min(self.room);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("liquid sample"),
            size: 16 + bound.max(1) as u64 * SAMPLE_WORDS as u64 * 4,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let Some(uniform) = self.scratch.as_ref().map(|s| s.uniform.clone()) else { return };
        let group = self.bind(E::Record, &[(0, &uniform), (1, &self.drops), (3, &self.counts), (13, &buffer)]);
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("liquid record") });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("liquid record"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.gpu.pipeline(E::Record).0);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups((bound + 1).div_ceil(WORKGROUP), 1, 1);
        }
        self.in_flight.push_back(self.gpu.queue.submit([encoder.finish()]));
        self.history.push_back(Arc::new(Sample { buffer, bound }));
        while self.history.len() > most.max(1) {
            self.history.pop_front();
        }
    }

    /// Forgets every sample.
    pub fn forget(&mut self) {
        self.history.clear();
        self.into = 0;
    }

    /// How many samples Un-mix has.
    pub fn samples(&self) -> usize {
        self.history.len()
    }

    /// Bytes Un-mix's samples take on the card (shared with copies of the body).
    pub fn history_bytes(&self) -> usize {
        self.history.iter().map(|s| s.bytes()).sum()
    }

    fn bind(&self, e: E, entries: &[(u32, &wgpu::Buffer)]) -> wgpu::BindGroup {
        let entries: Vec<wgpu::BindGroupEntry> = entries.iter().map(|(b, buf)| entry(*b, buf)).collect();
        self.gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("liquid history"),
            layout: &self.gpu.pipeline(e).1,
            entries: &entries,
        })
    }

    /// One liquid step played backwards: the drops slid from the newest sample towards the one
    /// before, `every` steps a sample, the newest forgotten once passed, then kept out of `balls`
    /// (place only). `step` gives the liquids (their Balls push it). False when there is nothing
    /// before the newest sample left: the drops are then the oldest sample's.
    pub fn rewind_step(&mut self, step: &StepGpu, balls: &[BallGpu], every: u32) -> bool {
        let Some(_) = self.history.back() else { return false };
        // A count asked for before the rewind no longer bounds the drops.
        if let Some(r) = self.reader.as_mut() {
            r.after = u32::MAX;
        }
        self.take_count();
        self.wait_for_room();
        let every = every.max(1);
        self.into += 1;
        if self.into >= every && self.history.len() >= 2 {
            self.history.pop_back();
            self.into = 0;
        }
        let n = self.history.len();
        let newer = self.history[n - 1].clone();
        let older = if n >= 2 { self.history[n - 2].clone() } else { newer.clone() };
        let t = if n >= 2 { self.into as f32 / every as f32 } else { 0.0 };
        let chosen = if t >= 0.5 { older.bound } else { newer.bound };
        self.ensure_room(newer.bound.max(older.bound).max(1));
        let gpu = self.gpu.clone();
        let (device, queue) = (&gpu.device, &gpu.queue);
        let cells = self.cols * self.rows;
        let scratch = self.scratch.get_or_insert_with(|| Scratch::new(device, cells));
        let mut words = Vec::new();
        let (balls_at, starts_at, items_at, ball_cols, ball_rows, ball_count) =
            put_balls(&mut words, balls, self.cols, self.rows, 0.0);
        if words.is_empty() {
            words.push(0);
        }
        let mut st = *step;
        st.grid[0] = self.cols;
        st.grid[1] = self.rows;
        st.counts = [0, self.room, self.room.max(cells), cells];
        st.flags = [0, 0, ball_count, st.flags[3]];
        st.words_at = [0, 0, balls_at, starts_at];
        st.ball_grid = [ball_cols, ball_rows, BALL_CELL.to_bits(), items_at];
        st.feel = [0, RING, 0, 0];
        st.extra[2] = t.to_bits();
        st.extra[3] = u32::from(t >= 0.5);
        queue.write_buffer(&scratch.uniform, 0, bytemuck::bytes_of(&st));
        let words_buffer = scratch.words.at_least(device, words.len() as u64 * 4);
        queue.write_buffer(&words_buffer, 0, bytemuck::cast_slice(&words));
        let uniform = scratch.uniform.clone();
        let rewind = self.bind(
            E::Rewind,
            &[
                (0, &uniform),
                (1, &self.drops),
                (3, &self.counts),
                (6, &words_buffer),
                (14, &newer.buffer),
                (15, &older.buffer),
            ],
        );
        let done =
            self.bind(E::RewindDone, &[(0, &uniform), (3, &self.counts), (14, &newer.buffer), (15, &older.buffer)]);
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("liquid rewind") });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("liquid rewind"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&gpu.pipeline(E::Rewind).0);
            pass.set_bind_group(0, &rewind, &[]);
            pass.dispatch_workgroups(chosen.max(1).div_ceil(WORKGROUP), 1, 1);
            pass.set_pipeline(&gpu.pipeline(E::RewindDone).0);
            pass.set_bind_group(0, &done, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        self.in_flight.push_back(queue.submit([encoder.finish()]));
        let _ = device.poll(wgpu::PollType::Poll);
        self.bound = chosen;
        self.steps += 1;
        n >= 2
    }

    /// Every drop as the card has it once its queued steps are done (waits: tests and tools only).
    pub fn read(&self) -> Vec<Drop> {
        let device = &self.gpu.device;
        let n = self.bound.min(self.room) as u64;
        let bytes = (n * std::mem::size_of::<Drop>() as u64).max(4);
        let staging = |size| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("liquid read"),
                size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let (a, c) = (staging(bytes), staging(32));
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("liquid read") });
        if n > 0 {
            encoder.copy_buffer_to_buffer(&self.drops, 0, &a, 0, bytes);
        }
        encoder.copy_buffer_to_buffer(&self.counts, 0, &c, 0, 32);
        self.gpu.queue.submit([encoder.finish()]);
        a.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        c.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        let count = {
            let view = c.slice(..).get_mapped_range().expect("mapped");
            bytemuck::cast_slice::<u8, u32>(&view)[1] as usize
        };
        let count = count.min(n as usize);
        if n == 0 {
            return Vec::new();
        }
        bytemuck::cast_slice::<u8, Drop>(&a.slice(..).get_mapped_range().expect("mapped"))[..count].to_vec()
    }
}

/// Puts the balls (6 words each: place, speed, radius, mask) and a grid over them (cell [`BALL_CELL`] grid units: each cell's
/// start, then one past the last, then the items) into `words`. Gives where the balls, the starts
/// and the items begin, the grid's columns and rows, and how many balls were put (the finite ones).
fn put_balls(
    words: &mut Vec<u32>,
    balls: &[BallGpu],
    cols: u32,
    rows: u32,
    travel: f32,
) -> (u32, u32, u32, u32, u32, u32) {
    let bc = (cols as f32 / BALL_CELL).ceil().max(1.0) as u32;
    let br = (rows as f32 / BALL_CELL).ceil().max(1.0) as u32;
    let balls_at = words.len() as u32;
    let fine = |b: &BallGpu| b.x.iter().chain(&b.v).all(|v| v.is_finite()) && b.r.is_finite() && b.r >= 0.0;
    let kept: Vec<&BallGpu> = balls.iter().filter(|b| fine(b)).collect();
    for b in &kept {
        words.extend([b.x[0], b.x[1], b.v[0], b.v[1], b.r].map(f32::to_bits));
        words.push(b.mask);
    }
    let mut lists: Vec<Vec<u32>> = vec![Vec::new(); (bc * br) as usize];
    let cell_of = |x: f32, n: u32| ((x / BALL_CELL).floor().clamp(0.0, (n - 1) as f32)) as u32;
    for (i, b) in kept.iter().enumerate() {
        // Contact, sticking (1.5 cells) and wakes (3 cells) reach this far.
        // From where it starts to where the step's substeps move it.
        let reach = b.r + 3.5;
        let to = [b.x[0] + b.v[0] * travel, b.x[1] + b.v[1] * travel];
        let (c0, c1) = (cell_of(b.x[0].min(to[0]) - reach, bc), cell_of(b.x[0].max(to[0]) + reach, bc));
        let (r0, r1) = (cell_of(b.x[1].min(to[1]) - reach, br), cell_of(b.x[1].max(to[1]) + reach, br));
        for r in r0..=r1 {
            for c in c0..=c1 {
                lists[(r * bc + c) as usize].push(i as u32);
            }
        }
    }
    let starts_at = words.len() as u32;
    let mut at = 0u32;
    for l in &lists {
        words.push(at);
        at += l.len() as u32;
    }
    words.push(at);
    let items_at = words.len() as u32;
    for l in &lists {
        words.extend_from_slice(l);
    }
    (balls_at, starts_at, items_at, bc, br, kept.len() as u32)
}

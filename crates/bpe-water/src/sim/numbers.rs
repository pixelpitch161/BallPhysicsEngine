//! The solver's own numbers ([`Numbers`]) and what a liquid is like ([`Feel`]).

use bytemuck::{Pod, Zeroable};

/// The most liquids a body holds.
pub const MOST_LIQUIDS: usize = 16;
/// The most levels the pressure's ladder has.
pub const MOST_LEVELS: usize = 12;

/// The solver's numbers. The defaults are the ones the liquid ships with; the measuring tests may
/// replace any by a `BPE_WATER_*` environment variable ([`Numbers::from_env`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Numbers {
    /// Liquid steps a second.
    pub rate: u32,
    /// Multigrid cycles a liquid step: each sweeps the cells, hands what they still lack to a grid
    /// half as fine (and that to the next, down to a handful of cells), then adds the coarser
    /// answers back on the way up. A pool's weight then reaches its floor in a cycle or two, where
    /// plain sweeps need about as many as the pool is cells deep.
    pub cycles: u32,
    /// Sweeps on a level on the way down, on the way up, and on the coarsest level.
    pub pre: u32,
    pub post: u32,
    pub coarse: u32,
    /// Plain sweeps after the cycles.
    pub iters: u32,
    /// How much of a coarser level's answer is added.
    pub scale: f32,
    /// The sweeps' over-relaxation.
    pub omega: f32,
    /// The share of a cell's excess density pushed out a step, and the most a step.
    pub push_out: f32,
    pub most_out: f32,
    /// The share of a cell's room that may fill a step before it pushes back.
    pub fill: f32,
    /// The most of a cell's volume an over-full cell pushes out a step where the liquid is thin (a
    /// thread, a film, a surface): gentler than `most_out`, which bodies of liquid keep.
    pub most_out_thin: f32,
    /// The least mass on a face for it to have a speed (a drop's is 0.25).
    pub face_floor: f32,
    /// A sub-move's length, in cells.
    pub sub_move: f32,
    /// The most cells a drop moves in a liquid step.
    pub most_move: f32,
    /// The hold-together (thin streams and films): on or off.
    pub hold: bool,
    /// The pull of a liquid at Surface pull 1, in pixels squared a second squared for each unit of
    /// stretch.
    pub pull: f32,
    /// How far the pull's wave may travel in a liquid step, in cells: the pull is capped so the
    /// spring stays stable in one step.
    pub pull_cfl: f32,
    /// The damping of the pull's spring (of the spreading at the drop, sized by the pull's wave).
    pub hold_damp: f32,
    /// How fast a drop's stretch settles to none inside full liquid, a second.
    pub hold_settle: f32,
    /// How fast a drop's stretch eases off everywhere, a second.
    pub hold_relax: f32,
    /// The liquid round a drop (1 is full) from which it counts as inside full liquid.
    pub hold_full: f32,
    /// How fast thin liquid on its way evens out its own spacing, in cells squared a second (0:
    /// never), and the furthest that moves a drop in a liquid step, in cells.
    pub even: f32,
    pub even_most: f32,
    /// The density at which a torn drop joins the liquid again.
    pub hold_join: f32,
    /// The stretch a liquid tears at with no Spray, and with all of it.
    pub tear_calm: f32,
    pub tear_spray: f32,
}

impl Default for Numbers {
    fn default() -> Self {
        Numbers {
            rate: 240,
            cycles: 2,
            pre: 2,
            post: 2,
            coarse: 16,
            iters: 0,
            scale: 1.0,
            omega: 1.0,
            push_out: 0.25,
            most_out: 0.1,
            fill: 1.0,
            most_out_thin: 0.005,
            face_floor: 0.005,
            sub_move: 0.4,
            most_move: 8.0,
            hold: true,
            pull: 4.0e5,
            pull_cfl: 0.3,
            hold_damp: 0.7,
            hold_settle: 200.0,
            hold_relax: 4.0,
            hold_full: 0.9,
            even: 12.0,
            even_most: 0.15,
            hold_join: 0.6,
            tear_calm: 2.0,
            tear_spray: 0.2,
        }
    }
}

impl Numbers {
    /// The defaults, each replaced by its `BPE_WATER_*` environment variable when that is set
    /// (`BPE_WATER_CYCLES`, `BPE_WATER_PULL`, `BPE_WATER_HOLD=0`...): tuning without building again.
    /// For the measuring tests only.
    pub fn from_env() -> Numbers {
        fn num<T: std::str::FromStr>(name: &str, or: T) -> T {
            std::env::var(format!("BPE_WATER_{name}")).ok().and_then(|s| s.parse().ok()).unwrap_or(or)
        }
        let d = Numbers::default();
        Numbers {
            rate: num("RATE", d.rate),
            cycles: num("CYCLES", d.cycles),
            pre: num("PRE", d.pre),
            post: num("POST", d.post),
            coarse: num("COARSE", d.coarse),
            iters: num("ITERS", d.iters),
            scale: num("SCALE", d.scale),
            omega: num("OMEGA", d.omega),
            push_out: num("PUSH_OUT", d.push_out),
            most_out: num("MOST_OUT", d.most_out),
            fill: num("FILL", d.fill),
            most_out_thin: num("MOST_OUT_THIN", d.most_out_thin),
            face_floor: num("FACE_FLOOR", d.face_floor),
            sub_move: num("SUB_MOVE", d.sub_move),
            most_move: num("MOST_MOVE", d.most_move),
            hold: num("HOLD", u32::from(d.hold)) != 0,
            pull: num("PULL", d.pull),
            pull_cfl: num("PULL_CFL", d.pull_cfl),
            hold_damp: num("HOLD_DAMP", d.hold_damp),
            hold_settle: num("HOLD_SETTLE", d.hold_settle),
            hold_relax: num("HOLD_RELAX", d.hold_relax),
            hold_full: num("HOLD_FULL", d.hold_full),
            even: num("EVEN", d.even),
            even_most: num("EVEN_MOST", d.even_most),
            hold_join: num("HOLD_JOIN", d.hold_join),
            tear_calm: num("TEAR_CALM", d.tear_calm),
            tear_spray: num("TEAR_SPRAY", d.tear_spray),
        }
    }

    /// Seconds a liquid step.
    pub fn dt(&self) -> f32 {
        1.0 / self.rate.max(1) as f32
    }
}

/// What a liquid is like, in the owner's settings' own terms (the Feel card's, part 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Feel {
    /// How many times the course's gravity it falls at.
    pub gravity: f32,
    /// How strongly it pulls itself together: 0 is spray, 1 ropes.
    pub surface_pull: f32,
    /// How easily it tears into droplets (0 to 1).
    pub spray: f32,
    /// How much it clings to the course.
    pub stickiness: f32,
    /// How heavy it is (water is 1).
    pub weight: f32,
    /// How fast its motion dies down (0: never).
    pub settles: f32,
}

impl Default for Feel {
    fn default() -> Self {
        Feel { gravity: 1.0, surface_pull: 0.2, spray: 0.5, stickiness: 0.0, weight: 1.0, settles: 0.0 }
    }
}

/// A liquid's numbers on the card. Matches `Feel` in common.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct FeelGpu {
    /// gravity x, pull (cells squared a second squared a unit of stretch), tear, stickiness
    pub a: [f32; 4],
    /// weight, damping (a second), 0, 0
    pub b: [f32; 4],
}

impl Feel {
    /// This liquid on a grid of `cell`-pixel cells.
    pub fn gpu(&self, numbers: &Numbers, cell: f32) -> FeelGpu {
        let pull = numbers.pull * self.surface_pull * self.surface_pull / (cell * cell);
        let cap = (numbers.pull_cfl / numbers.dt()).powi(2);
        let spray = self.spray.clamp(0.0, 1.0);
        let tear = numbers.tear_calm + (numbers.tear_spray - numbers.tear_calm) * spray;
        FeelGpu {
            a: [self.gravity, pull.min(cap).max(0.0), tear, self.stickiness.max(0.0)],
            b: [self.weight.clamp(0.05, 20.0), 8.0 * self.settles * self.settles, 0.0, 0.0],
        }
    }
}

/// One liquid step's numbers. Matches `Step` in common.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct StepGpu {
    pub grid: [u32; 4],
    pub time: [f32; 4],
    pub flags: [u32; 4],
    pub a: [f32; 4],
    pub b: [f32; 4],
    pub h: [f32; 4],
    pub h2: [f32; 4],
    pub lv: [[u32; 4]; MOST_LEVELS],
    pub feels: [FeelGpu; MOST_LIQUIDS],
}

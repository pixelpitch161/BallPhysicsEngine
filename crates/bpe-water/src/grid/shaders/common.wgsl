// Water mode's physics (docs/notes/water.md, plan docs/superpowers/plans/water-mode/part2a.md).
//
// One liquid step: the stretched drops pull each other together through the hold nodes
// (`cs_hold_p2g`, `cs_hold_grid`); the drops take that pull and spread their mass and momentum to
// the faces of a staggered grid (speed across on each cell's left face, speed down on its top
// face) and their volume to the cell middles (`cs_p2g`, whole-number sums); each cell works out its
// faces' speeds, how full it is and how much must flow out of it (`cs_grid`); the pressure is found
// by red-black sweeps on a ladder of coarser grids, never below nothing (`cs_sor`, `cs_down`,
// `cs_up`); the faces' speeds are corrected by it (`cs_proj`, `cs_slip`); the drops gather their
// speed and their APIC matrix back and move, kept off the solids (`cs_g2p`).
//
// Grid units: a cell is 1. Cell (x, y) is the square from node (x, y) to node (x + 1, y + 1), and
// shares the node's index.

struct Drop {
    x: vec2<f32>,
    v: vec2<f32>,
    // The APIC matrix: xx, xy, yx, yy.
    c: vec4<f32>,
    // How stretched it is (J - 1): above 0 it pulls; at its liquid's tear it is loose.
    e: f32,
    // Bits 0 to 3 its liquid, bit 8 it stops at the course's edge.
    kind: u32,
    serial: u32,
    // rgba8, linear.
    colour: u32,
};

// One liquid's feel.
struct Feel {
    // gravity x, pull (cells squared a second squared for each unit of stretch), tear, stickiness
    a: vec4<f32>,
    // weight, damping (a second), 0, 0
    b: vec4<f32>,
};

struct Step {
    // columns and rows of nodes, drops, the pressure's levels
    grid: vec4<u32>,
    // dt, gravity x and y (cells a second squared), the speed cap (cells a second)
    time: vec4<f32>,
    // the step's number, whether the course's edges are shut (0 or 1), 0, 0
    flags: vec4<u32>,
    // over-relaxation, the share of a cell's excess pushed out a step, the most a step, the least
    // mass on a face for it to have a speed
    a: vec4<f32>,
    // the share of a cell's room that may fill a step, a sub-move's length (cells), how much of a
    // coarser level's answer is added, the most an over-full cell in thin liquid pushes out a step
    b: vec4<f32>,
    // the hold-together: its damping, how fast stretch settles in full liquid (a second), the
    // density a loose drop joins at, on (0 or 1)
    h: vec4<f32>,
    // the hold-together: how fast stretch eases off everywhere (a second), the liquid round a
    // drop from which it is inside full liquid, how fast thin liquid evens out its spacing (cells
    // squared a second), the furthest that moves a drop a step (cells)
    h2: vec4<f32>,
    // the drops' elbow room: the share of an overlap undone a step (0: there is none, and `spots`
    // is not touched), how near two drops may be before they part (cells), the furthest that moves
    // a drop a step (cells), the speed from which a drop has it in full (cells a second)
    r: vec4<f32>,
    // the pressure's levels (0 the cells themselves, each next one half as many each way): where
    // the level starts in `cellp` and `coef`, its columns, its rows, 0
    lv: array<vec4<u32>, 12>,
    feels: array<Feel, 16>,
};

struct Tick {
    n: vec4<u32>,
};

const S_M: f32 = 1048576.0;
const S_P: f32 = 4096.0;
const LIMIT: f32 = 33554432.0;
const STOPS: u32 = 256u;
const EDGE: f32 = 2.0;
// Half the room a drop at rest takes: the liquid's own edge is this far from where drops stop.
const GAP: f32 = 0.25;
// How far off a solid's face a drop is kept.
const OFF: f32 = 0.5;
// Words a cell in `sums`: mass on its left face, momentum across, mass on its top face, momentum
// down, volume at its middle, spare.
const SUMS: u32 = 6u;
// Words a node in `hold`: mass, the pull's impulse x and y, spare.
const HOLD: u32 = 4u;
// A drop's volume, in cells (four drops fill a cell).
const VOL: f32 = 0.25;
// Words a spot in `spots`.
const SPOT: u32 = 3u;
// A spot's width, as a drop's place within it is summed.
const SPOT_ONE: f32 = 4096.0;

// The spot a drop at `x` is in (held inside the grid), and its place within it in whole numbers
// (0 to SPOT_ONE).
struct Spot {
    at: vec2<i32>,
    within: vec2<i32>,
};

fn spot_of(x: vec2<f32>) -> Spot {
    let q = x * 2.0;
    let at = clamp(vec2<i32>(floor(q)), vec2<i32>(0), vec2<i32>(2 * i32(st.grid.x) - 1, 2 * i32(st.grid.y) - 1));
    let within = clamp(vec2<i32>((q - vec2<f32>(at)) * SPOT_ONE), vec2<i32>(0), vec2<i32>(i32(SPOT_ONE) - 1));
    return Spot(at, within);
}

fn spot_index(at: vec2<i32>) -> u32 {
    return SPOT * (u32(at.y) * 2u * st.grid.x + u32(at.x));
}

@group(0) @binding(0) var<uniform> st: Step;
@group(0) @binding(1) var<storage, read_write> drops: array<Drop>;
// per node: how far it is from the nearest solid's face, in cells (below 0 inside the solid)
@group(0) @binding(2) var<storage, read_write> solid: array<f32>;
@group(0) @binding(3) var<storage, read_write> sums: array<atomic<i32>>;
// per cell, from the solids: the open share of its left face, of its top face, the solids' share
// of its middle's kernel, the solids' share of its node's hold kernel
@group(0) @binding(4) var<storage, read_write> wallc: array<vec4<f32>>;
// per cell: speed across on its left face, speed down on its top face, the drops' volume at its
// middle, bits (1 the left face has a speed, 2 the top face has, 4 the cell is solved)
@group(0) @binding(5) var<storage, read_write> vel: array<vec4<f32>>;
// per cell: pressure x dt, what must flow out less what does, the open shares' sum, solved (0/1)
@group(0) @binding(6) var<storage, read_write> cellp: array<vec4<f32>>;
// per cell: its faces' open shares: left, right, top, bottom
@group(0) @binding(7) var<storage, read_write> coef: array<vec4<f32>>;
// `vel` as the drops gather it: with the shut faces at solids given the liquid's speed along them
@group(0) @binding(8) var<storage, read_write> vel_out: array<vec4<f32>>;
@group(0) @binding(9) var<uniform> tick: Tick;
@group(0) @binding(10) var<storage, read_write> hold: array<atomic<i32>>;
// per node: the pull's change of speed x and y, the liquid at the node (its volume, a full one 1,
// with the solids' share), the drops' own volume there
@group(0) @binding(11) var<storage, read_write> hold_out: array<vec4<f32>>;

// per spot (a square half a cell wide, four a cell: spot (i, j) of the 2 x cols by 2 x rows is the
// square from (i, j) / 2 to (i + 1, j + 1) / 2): how many drops are in it, the sum of their places
// across and down within it (0 to SPOT_ONE each). Whole numbers, added atomically: the sums are
// the same whatever order the drops are handled in. (A stand-in a few words long when the course
// is too big for it: then `st.r.x` is 0.)
@group(0) @binding(12) var<storage, read_write> spots: array<atomic<i32>>;

fn finite2(v: vec2<f32>) -> bool {
    return abs(v.x) < 1e30 && abs(v.y) < 1e30;
}

fn node(x: i32, y: i32) -> u32 {
    return u32(y) * st.grid.x + u32(x);
}

fn in_grid(x: i32, y: i32) -> bool {
    return x >= 0 && y >= 0 && x < i32(st.grid.x) && y < i32(st.grid.y);
}

// To the nearest whole number (toward zero, every add lost up to a unit the same way and still
// liquid drifted).
fn fixed(x: f32, scale: f32) -> i32 {
    return i32(round(clamp(x * scale, -LIMIT, LIMIT)));
}

fn liquid_of(d: Drop) -> u32 {
    return d.kind & 15u;
}

// Whether a drop takes part in the step.
fn steps(p: Drop) -> bool {
    return finite2(p.x) && finite2(p.v) && p.x.x >= 0.0 && p.x.y >= 0.0 && p.x.x <= f32(st.grid.x) - 1.0
        && p.x.y <= f32(st.grid.y) - 1.0;
}

// How far point `q` is from the nearest solid's face (cells, below 0 inside), and which way is out
// (the distance's slope): between the four nodes round it. Off the grid it is far from any.
fn solid_at(q: vec2<f32>) -> vec3<f32> {
    let lo = clamp(vec2<i32>(floor(q)), vec2<i32>(0), vec2<i32>(i32(st.grid.x) - 2, i32(st.grid.y) - 2));
    let f = clamp(q - vec2<f32>(lo), vec2<f32>(0.0), vec2<f32>(1.0));
    let d00 = solid[node(lo.x, lo.y)];
    let d10 = solid[node(lo.x + 1, lo.y)];
    let d01 = solid[node(lo.x, lo.y + 1)];
    let d11 = solid[node(lo.x + 1, lo.y + 1)];
    let d = mix(mix(d00, d10, f.x), mix(d01, d11, f.x), f.y);
    var g = vec2<f32>(mix(d10 - d00, d11 - d01, f.y), mix(d01 - d00, d11 - d10, f.x));
    let len = length(g);
    if len > 1e-4 {
        g = g / len;
    } else {
        g = vec2<f32>(0.0);
    }
    return vec3<f32>(g, d);
}

// Where the 2 x 2 samples round `x` start, for samples at whole numbers plus `off`.
fn base2(x: vec2<f32>, off: vec2<f32>) -> vec2<i32> {
    let b = vec2<i32>(floor(x - off));
    return clamp(b, vec2<i32>(0), vec2<i32>(i32(st.grid.x) - 2, i32(st.grid.y) - 2));
}

// The hold nodes' kernel (a quadratic B-spline over 3 nodes): where a drop's 3 x 3 nodes start,
// the weights and their slopes.
fn base3(x: vec2<f32>) -> vec2<i32> {
    let b = vec2<i32>(floor(x - 0.5));
    return clamp(b, vec2<i32>(0), vec2<i32>(i32(st.grid.x) - 3, i32(st.grid.y) - 3));
}

// Where `x` is among its 3 x 3 nodes. Held inside the kernel's own span: at the grid's very edge
// the clamped start would put it outside, where a weight goes below nothing.
fn frac3(x: vec2<f32>, base: vec2<i32>) -> vec2<f32> {
    return clamp(x - vec2<f32>(base), vec2<f32>(0.5), vec2<f32>(1.5));
}

fn weights3(fx: vec2<f32>) -> array<vec2<f32>, 3> {
    return array<vec2<f32>, 3>(
        0.5 * (1.5 - fx) * (1.5 - fx),
        0.75 - (fx - 1.0) * (fx - 1.0),
        0.5 * (fx - 0.5) * (fx - 0.5),
    );
}

fn slopes3(fx: vec2<f32>) -> array<vec2<f32>, 3> {
    return array<vec2<f32>, 3>(fx - 1.5, -2.0 * (fx - 1.0), fx - 0.5);
}

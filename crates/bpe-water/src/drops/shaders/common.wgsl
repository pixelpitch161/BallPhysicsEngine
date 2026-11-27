// The all-drops engine (docs/superpowers/specs/2026-11-25-all-drops-water-core-design.md): every
// drop feels its neighbours directly, position based (Macklin and Mueller 2013), with a skin
// (Akinci and others 2013).
//
// One step: each drop falls and is moved to where it would be, kept off the solids, and its bin
// counted (`cs_predict`, which also takes the step's births); the bins' running sums give every
// drop a sorted place, by bin and then by serial (`cs_sum_a/b/c`, `cs_scatter`, `cs_order`); each
// drop finds how over-full its neighbourhood is and changes its pressure multiplier, never above
// nothing (`cs_lambda`); it is moved by its own and its neighbours' changes (`cs_apply`); and its
// speed, the evening of speeds, the skin's pull, grip and drag are worked out (`cs_finish`).
//
// The drops are stored by field, 16 bytes a drop in each buffer. Grid units: a layout cell is 1,
// four drops fill one, drops at rest are half a cell apart.

struct Feel {
    // gravity x, the skin's pull between drops (cells a second squared), its pull along the
    // surface's bend, the pull toward a near face (cells a second squared)
    a: vec4<f32>,
    // damping (a second), drag along a face (a second), the share of the neighbours' mean speed
    // taken a step, weight
    b: vec4<f32>,
};

// A force on the water (drops/forces.rs `ForceGpu`): its middle (grid units), its reach (cells), the
// most it pushes a drop (cells a second squared); by kind: a magnet's pull (cells a second squared), its
// spikes' pull, their aim and spread (radians), a whirlpool's speed (cells a second) and rate, a wind's
// velocity (cells a second) and rate; its kind (1 magnet, 2 whirlpool, 3 wind), the liquids it moves
// (bit k: liquid k), a magnet's spikes, 0.
struct Force {
    a: vec4<f32>,
    b: vec4<f32>,
    c: vec4<u32>,
};

// A frame's numbers: the same for each of its steps.
struct Fr {
    // the layout's columns and rows of nodes, how many pours, bit 0 the course's edges are shut
    grid: vec4<u32>,
    // the bins across and down, how many (the next is the bin of drops that left), how many
    // blocks of running sums
    cells: vec4<u32>,
    // dt, gravity x and y (cells a second squared), the speed cap (cells a second)
    time: vec4<f32>,
    // the reach (cells), the density at rest, the softening of a correction, its share taken
    k: vec4<f32>,
    // the share of the drag every liquid has that a waxy face keeps, how far off a face a drop
    // stays, a sub-move's length, the solids' share of the density
    w: vec4<f32>,
    // a cell's pixels, how far from a face drag reaches, the most a sweep moves a drop, the drag
    // along a face every liquid has (a second)
    s: vec4<f32>,
    feels: array<Feel, 16>,
    // the solids' share of a drop's neighbourhood by its distance from the face (`wall_table`)
    wall: array<vec4<f32>, 17>,
    // what a wetted face adds to a drop's skin by its distance from the face (`wet_table`): the
    // pull toward the face, and how far the liquid lies toward it
    wet: array<vec4<f32>, 17>,
    // where each pour judges the room for its drops (grid units: a reach behind its spout), two
    // to an entry
    spouts: array<vec4<f32>, 32>,
    // deep water (deep.wgsl): no push within this many cells of a cell that is not full, all of it
    // this many cells further in, the fill from which a cell is full, the most cells the push moves a
    // drop a step
    deep: vec4<f32>,
    // what the kept pressure is scaled by on a frame's first step (its dt over the last's; 0 when the
    // deep part starts afresh); the share of the speed at which drops close on each other taken
    // away a step (`meet_even`); the most fuller than at rest the deep water rests (`rest_most`); how
    // much a wetted face counts in the skin's bend (`wet_bend`)
    deep2: vec4<f32>,
    // the pressure's sweeps: over-relaxation, the share of a coarser level's answer added, the share
    // of a cell's excess pushed out a step, the most a step (cells of volume)
    mg: vec4<f32>,
    // the share of a cell's room that may fill a step, the least mass on a face for it to have a
    // speed, the liquid's edge off a solid's face (cells), the course's own liquid's edge from the
    // grid's edge (cells)
    mg2: vec4<f32>,
    // the pressure's levels, how far the depth is looked for (cells), the serial the frame's first
    // air drop takes, the slots kept for air drops after the frame's drops (air.wgsl)
    dgrid: vec4<u32>,
    // trapped air (air.wgsl): how many gravities an air drop falls upward at, the fullness under
    // which it bursts, how far round a cell that is not full the water over it is looked for
    // (cells), the share of the evening and the closing loss air and liquid have between them
    air: vec4<f32>,
    // each level of the pressure: where it starts in `cellp` and `coef`, its columns, its rows, 0
    lv: array<vec4<u32>, 12>,
    // the drains (drains.rs): each one's middle (grid units), reach (cells) and chance a step
    drains: array<vec4<f32>, 32>,
    // each drain's edges, four to an entry (0 a disc; 1 left, 2 right, 4 top, 8 bottom)
    dedges: array<vec4<u32>, 8>,
    // how many drains, the frame's code for the drops they take (`drained_y`), 0, 0
    dmeta: vec4<u32>,
    // the forces (forces.wgsl): how many, 0, 0, 0; then each force
    forces_n: vec4<u32>,
    forces: array<Force, 16>,
};

// A step's numbers (one for each step of the frame, at its own offset): the drops this step, the
// drops before this step's births, where its births start in the births buffer, and bit 0 the
// frame's last step, bit 1 its first, bits 8 and up its number in the frame. The export takes (the first drop, how many, 0, 0); a pass of
// the deep water's pressure (its number, 0, 0, 0).
struct St {
    n: vec4<u32>,
};

// A drop born this step: its place and speed at the step's end (grid units), and its kind,
// serial, colour and 0.
struct Born {
    xv: vec4<f32>,
    id: vec4<u32>,
};

// A drop as `Drop` (drop.rs) lays it out: `c` is (how full its neighbourhood
// is, 0, which way the liquid lies from it).
struct DropOut {
    x: vec2<f32>,
    v: vec2<f32>,
    c: vec4<f32>,
    e: f32,
    kind: u32,
    serial: u32,
    colour: u32,
};

const STOPS: u32 = 256u;
// An air drop (drop.rs `AIR`): it takes up room as the liquid does, falls upward and bursts at the
// top; it has no skin of its own, and the liquid's skin leaves it out.
const AIR: u32 = 128u;
const EDGE: f32 = 2.0;
const BLOCK: u32 = 512u;
const PI: f32 = 3.14159265;

@group(0) @binding(0) var<uniform> fr: Fr;
@group(0) @binding(1) var<uniform> st: St;
// Per drop, in the order the last step sorted them: place and speed, and which way the liquid
// lay from it, how full its neighbourhood was, 0.
@group(0) @binding(2) var<storage, read_write> xv: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> nf: array<vec4<f32>>;
// Per drop: kind, serial, colour, 0; as the last step sorted them (`id_cur`) and as this one does.
@group(0) @binding(4) var<storage, read_write> id_cur: array<vec4<u32>>;
@group(0) @binding(5) var<storage, read_write> id_nxt: array<vec4<u32>>;
// Per drop, unsorted: where it would be this step, and where it was.
@group(0) @binding(6) var<storage, read_write> xp: array<vec4<f32>>;
// Per bin (and one more, for drops that left the course): how many drops.
@group(0) @binding(7) var<storage, read_write> counts: array<atomic<u32>>;
// Per bin: where its drops start in the sorted order (one more entry: the end).
@group(0) @binding(8) var<storage, read_write> starts: array<u32>;
@group(0) @binding(9) var<storage, read_write> blocks: array<u32>;
@group(0) @binding(10) var<storage, read_write> idx: array<u32>;
// Per drop, sorted: its place this step (x, y) and which way the liquid lay from it last step,
// in and out of a sweep.
@group(0) @binding(11) var<storage, read_write> y_in: array<vec4<f32>>;
@group(0) @binding(12) var<storage, read_write> y_out: array<vec4<f32>>;
// Per drop, sorted: this sweep's change of its multiplier, the multiplier, how full, 1 when it is air.
@group(0) @binding(13) var<storage, read_write> lamb: array<vec4<f32>>;
// Per drop, sorted: where it was (until the last sweep, then its new speed), its kind, 0.
@group(0) @binding(14) var<storage, read_write> velb: array<vec4<f32>>;
// Per layout node: how far it is from the nearest solid's face, in cells (below 0 inside); then per
// node, how wet that face is (0 to 1).
@group(0) @binding(15) var<storage, read_write> solid: array<f32>;
@group(0) @binding(16) var<storage, read_write> births: array<Born>;
@group(0) @binding(17) var<storage, read_write> outd: array<DropOut>;

fn finite2(v: vec2<f32>) -> bool {
    return abs(v.x) < 1e30 && abs(v.y) < 1e30;
}

// Whether a drop at `x` is in the course (one that left takes no part).
fn alive(x: vec2<f32>) -> bool {
    return finite2(x) && x.x >= 0.0 && x.y >= 0.0 && x.x <= f32(fr.grid.x) - 1.0 && x.y <= f32(fr.grid.y) - 1.0;
}

fn cell_of(x: vec2<f32>) -> vec2<i32> {
    let c = vec2<i32>(floor(x / fr.k.x));
    return clamp(c, vec2<i32>(0), vec2<i32>(i32(fr.cells.x) - 1, i32(fr.cells.y) - 1));
}

// The bin a drop at `x` is sorted into: its bin, or the last bin when it left the course.
fn bin_of(x: vec2<f32>) -> u32 {
    if !alive(x) {
        return fr.cells.z;
    }
    let c = cell_of(x);
    return u32(c.y) * fr.cells.x + u32(c.x);
}

fn node(x: i32, y: i32) -> u32 {
    return u32(y) * fr.grid.x + u32(x);
}

// How far `q` is from the nearest solid's face (cells, below 0 inside) and which way is out.
fn solid_at(q: vec2<f32>) -> vec3<f32> {
    let lo = clamp(vec2<i32>(floor(q)), vec2<i32>(0), vec2<i32>(i32(fr.grid.x) - 2, i32(fr.grid.y) - 2));
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

struct Kept {
    x: vec2<f32>,
    v: vec2<f32>,
};

// Keeps a drop at `x` moving at `v` off the solids and (when it stops there) the course's edge.
// It keeps none of its speed into a face.
fn keep_off(xin: vec2<f32>, vin: vec2<f32>, stops: bool) -> Kept {
    var x = xin;
    var v = vin;
    let s = solid_at(x);
    let off = fr.w.y;
    if s.z < off {
        x = x + s.xy * min(off - s.z, 4.0);
        let vn = dot(v, s.xy);
        if vn < 0.0 {
            v = v - vn * s.xy;
        }
    }
    if stops {
        let lo = vec2<f32>(EDGE);
        let hi = vec2<f32>(f32(fr.grid.x) - 1.0 - EDGE, f32(fr.grid.y) - 1.0 - EDGE);
        if x.x < lo.x + 0.05 { x.x = lo.x + 0.05; v.x = max(v.x, 0.0); }
        if x.x > hi.x - 0.05 { x.x = hi.x - 0.05; v.x = min(v.x, 0.0); }
        if x.y < lo.y + 0.05 { x.y = lo.y + 0.05; v.y = max(v.y, 0.0); }
        if x.y > hi.y - 0.05 { x.y = hi.y - 0.05; v.y = min(v.y, 0.0); }
    }
    return Kept(x, v);
}

fn is_air(kind: u32) -> bool {
    return (kind & AIR) != 0u;
}

fn stops_at_edge(kind: u32) -> bool {
    return (kind & STOPS) != 0u && (fr.grid.w & 1u) != 0u;
}

// The density a drop `far` away (cells) adds (the spiky kernel in the plane: steep close in, so
// two drops on top of each other count as too many and are parted; a flat-topped kernel lets
// drops pair up into strings).
fn weight(far: f32) -> f32 {
    let h = fr.k.x;
    let t = max(1.0 - far / h, 0.0);
    return 10.0 / (PI * h * h) * t * t * t;
}

// The slope of the density a drop at `r` from this one adds, as this one moves. `far` is `r`'s
// length, above 0.
fn slope(r: vec2<f32>, far: f32) -> vec2<f32> {
    let h = fr.k.x;
    let t = max(1.0 - far / h, 0.0);
    return r * (-30.0 / (PI * h * h * h) * t * t / far);
}

// A share in [0, 1) from three whole numbers, the same on every card (a hash, no generator).
fn share_of(a: u32, b: u32, c: u32) -> f32 {
    var h = (a * 0x9E3779B1u) ^ ((b + 0x7F4A7C15u) * 0x85EBCA77u) ^ ((c + 0x165667B1u) * 0xC2B2AE3Du);
    h = (h ^ (h >> 16u)) * 0x7FEB352Du;
    h = (h ^ (h >> 15u)) * 0x846CA68Bu;
    h = h ^ (h >> 16u);
    return f32(h >> 8u) / 16777216.0;
}

// The drain that takes drop `i` at `x` (grid units) this step, or -1. A drain whose reach holds it
// (a disc's within its reach of its middle, an edge's within `EDGE` + 0.8 cells of one of its edges:
// the drops stop at `EDGE` + 0.05) takes it when one share, a hash of the drop's place in the order,
// the frame and the step (the same every run), is under its chance; the first in the list that does
// is the one. So where drains overlap the strongest decides, and one that takes nothing (a Rate of
// 0) changes nothing.
fn drain_taking(i: u32, x: vec2<f32>, step: u32) -> i32 {
    if fr.dmeta.x == 0u {
        return -1;
    }
    let share = share_of(i, fr.dmeta.y, step);
    let strip = EDGE + 0.8;
    let far = vec2<f32>(fr.grid.xy) - vec2<f32>(1.0 + strip);
    for (var k = 0u; k < min(fr.dmeta.x, 32u); k = k + 1u) {
        let e = fr.dedges[k / 4u][k % 4u];
        var inside = false;
        if e == 0u {
            let r = x - fr.drains[k].xy;
            inside = dot(r, r) <= fr.drains[k].z * fr.drains[k].z;
        } else {
            inside = ((e & 1u) != 0u && x.x < strip) || ((e & 2u) != 0u && x.x > far.x)
                || ((e & 4u) != 0u && x.y < strip) || ((e & 8u) != 0u && x.y > far.y);
        }
        if inside && share < fr.drains[k].w {
            return i32(k);
        }
    }
    return -1;
}

// Where a drain sends the drops it takes in this frame: x says which drain (`-2e6 - k`), y the
// frame (`drained_y`), both whole numbers a float holds exactly.
fn drained_y() -> f32 {
    return -1e6 - f32(fr.dmeta.y);
}

// The way from one drop to another that is on top of it: each its own, by their places in the
// order.
fn any_way(a: u32, b: u32) -> vec2<f32> {
    let turn = f32(((a ^ (b * 40503u)) * 2654435761u) >> 22u) * 0.006135923;
    let w = vec2<f32>(cos(turn), sin(turn));
    return select(-w, w, a < b);
}

// The share of a drop's neighbourhood behind a flat face `q` reaches away (x), and how fast that
// share falls as the drop moves off the face, a reach (y): from the host's table.
fn wall_table(qin: f32) -> vec2<f32> {
    let q = clamp(qin, 0.0, 1.0) * 32.0;
    let k = min(u32(floor(q)), 31u);
    let f = q - f32(k);
    let a = select(fr.wall[k / 2u].xy, fr.wall[k / 2u].zw, (k & 1u) == 1u);
    let k1 = k + 1u;
    let b = select(fr.wall[k1 / 2u].xy, fr.wall[k1 / 2u].zw, (k1 & 1u) == 1u);
    return mix(a, b, f);
}

// What a wetted face `q` reaches away adds to a drop's skin, times the face's outward way: (the
// pull toward it, how far the liquid lies toward it). From the host's table.
fn wet_table(qin: f32) -> vec2<f32> {
    let q = clamp(qin, 0.0, 1.0) * 32.0;
    let k = min(u32(floor(q)), 31u);
    let f = q - f32(k);
    let a = select(fr.wet[k / 2u].xy, fr.wet[k / 2u].zw, (k & 1u) == 1u);
    let k1 = k + 1u;
    let b = select(fr.wet[k1 / 2u].xy, fr.wet[k1 / 2u].zw, (k1 & 1u) == 1u);
    return mix(a, b, f);
}

// How wet the face nearest `q` is (0 to 1), between the nodes round it.
fn wet_at(q: vec2<f32>) -> f32 {
    let lo = clamp(vec2<i32>(floor(q)), vec2<i32>(0), vec2<i32>(i32(fr.grid.x) - 2, i32(fr.grid.y) - 2));
    let f = clamp(q - vec2<f32>(lo), vec2<f32>(0.0), vec2<f32>(1.0));
    let at = fr.grid.x * fr.grid.y;
    let w00 = solid[at + node(lo.x, lo.y)];
    let w10 = solid[at + node(lo.x + 1, lo.y)];
    let w01 = solid[at + node(lo.x, lo.y + 1)];
    let w11 = solid[at + node(lo.x + 1, lo.y + 1)];
    return clamp(mix(mix(w00, w10, f.x), mix(w01, w11, f.x), f.y), 0.0, 1.0);
}

// What the solids add to a drop at `x`: (the slope of the density x and y, the density).
fn wall_density(x: vec2<f32>) -> vec3<f32> {
    let s = solid_at(x);
    let h = fr.k.x;
    if s.z >= h || fr.w.w <= 0.0 {
        return vec3<f32>(0.0);
    }
    let t = wall_table(s.z / h);
    let k = fr.k.y * fr.w.w;
    return vec3<f32>(-k * t.y / h * s.xy, k * t.x);
}

// The skin's pull between two drops `q` reaches apart: a pull that is strongest half a reach
// away, and a push when they touch (Akinci and others 2013; 1 at its strongest).
fn skin(q: f32) -> f32 {
    let t = (1.0 - q) * q;
    let c = 64.0 * t * t * t;
    if q > 0.5 {
        return c;
    }
    return 2.0 * c - 1.0;
}

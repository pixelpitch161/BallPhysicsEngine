// The Water mode's physics trial (throwaway, never merged; plan
// docs/superpowers/plans/water-mode/physics-trial.md): a second core for the Liquid thing's drops
// that solves the pressure, so the liquid cannot squash, instead of today's spring pressure and
// its many small steps. The drops keep today's layout, so today's drawing draws them.
//
// One liquid step: the drops spread their mass and momentum to the faces of a staggered grid
// (speed across on each cell's left face, speed down on its top face) and their volume to the cell
// middles (`cs2_p2g`, fixed-point sums as in today's core); each cell works out its faces' speeds,
// how full it is and how much must flow out of it (`cs2_grid`); the pressure is found by red-black
// sweeps, never below nothing (`cs2_sor`, twice an iteration); the faces' speeds are corrected by
// it (`cs2_proj`); the drops gather their speed and their APIC matrix back and move, kept off the
// walls (`cs2_g2p`). Grid units: a cell is 1. Cell (x, y) is the square from node (x, y) to node
// (x + 1, y + 1), and shares the node's index.

struct Drop {
    x: vec2<f32>,
    v: vec2<f32>,
    // The APIC matrix: xx, xy, yx, yy.
    c: vec4<f32>,
    colour: vec4<f32>,
    j: f32,
    kind: u32,
    serial: u32,
    kick: f32,
    mat: vec2<f32>,
    spare: vec2<f32>,
    f: vec4<f32>,
    more: vec4<f32>,
};

struct Liquid {
    a: vec4<f32>,
    // viscosity, weight, gravity ×, balls push it
    b: vec4<f32>,
    c: vec4<f32>,
    d: vec4<u32>,
    e: vec4<f32>,
    f: vec4<f32>,
    g: vec4<f32>,
    h: vec4<f32>,
    i: vec4<f32>,
    j: vec4<f32>,
    k: vec4<f32>,
    l: vec4<f32>,
    m: vec4<f32>,
    n: vec4<f32>,
};

struct Force {
    a: vec4<f32>,
    b: vec4<f32>,
    c: vec4<u32>,
};

// Today's step settings, as liquid_sim.wgsl has them.
struct Step {
    // cols, rows, substeps, liquids
    grid: vec4<u32>,
    // dt, gravity x and y (cells/s²), speed cap (cells/s)
    time: vec4<f32>,
    counts: vec4<u32>,
    // mixing this step, put the walls, balls, edges block (0/1)
    flags: vec4<u32>,
    more: vec4<f32>,
    words_at: vec4<u32>,
    ball_grid: vec4<u32>,
    feel: vec4<u32>,
    extra: vec4<u32>,
    paint: array<vec4<f32>, 8>,
    back: array<vec4<f32>, 8>,
    liquids: array<Liquid, 16>,
    pairs: array<vec4<u32>, 64>,
    pairs2: array<vec4<u32>, 64>,
    forces: array<Force, 48>,
    paints: array<vec4<u32>, 16>,
};

// The trial core's own numbers.
struct Trial {
    // iterations, 0, 0, 0
    n: vec4<u32>,
    // over-relaxation, the share of a cell's excess pushed out a step, the most a step, the least
    // mass on a face for it to have a speed
    a: vec4<f32>,
    // the share of a cell's room that may fill a step, a sub-move's length (cells), how much of a
    // coarser level's answer is added, 0
    b: vec4<f32>,
    // the pressure's levels (0 the cells themselves, each next one half as many each way): where
    // the level starts in `cellp` and `coef`, its columns, its rows, 0
    lv: array<vec4<u32>, 12>,
};

struct Tick {
    n: vec4<u32>,
};

const S_M: f32 = 1048576.0;
const S_P: f32 = 4096.0;
const LIMIT: f32 = 33554432.0;
const STOPS: u32 = 256u;
const STAIN_BASE: u32 = 16u;
const EDGE: f32 = 2.0;
// Half the room a drop at rest takes: the liquid's own edge is this far from where drops stop.
const GAP: f32 = 0.25;
const BIG: f32 = 1e9;
// Words a cell in `sums`: mass on its left face, momentum across, mass on its top face, momentum
// down, volume at its middle, spare.
const SUMS: u32 = 6u;

@group(0) @binding(0) var<uniform> st: Step;
@group(0) @binding(1) var<storage, read_write> drops: array<Drop>;
@group(0) @binding(3) var<storage, read_write> counts: array<u32>;
// per node: (normal xy, distance, in the band), (wall speed xy, half thickness, wall number)
@group(0) @binding(5) var<storage, read_write> solid: array<vec4<f32>>;
@group(0) @binding(19) var<uniform> tick: Tick;
@group(0) @binding(20) var<uniform> tr: Trial;
@group(0) @binding(21) var<storage, read_write> sums: array<atomic<i32>>;
// per cell, from the walls band: (open share of its left face, of its top face, the walls' share
// of its middle's kernel, 0), (line distance of a thin wall at its middle, at its left face's
// middle, at its top face's middle, the three walls' numbers packed)
@group(0) @binding(22) var<storage, read_write> wallc: array<vec4<f32>>;
// per cell: speed across on its left face, speed down on its top face, the drops' volume at its
// middle, bits (1 the left face has a speed, 2 the top face has, 4 the cell is solved)
@group(0) @binding(23) var<storage, read_write> vel: array<vec4<f32>>;
// per cell: pressure × dt, what must flow out less what does, the open shares' sum, solved (0/1)
@group(0) @binding(24) var<storage, read_write> cellp: array<vec4<f32>>;
// per cell: its faces' open shares: left, right, top, bottom
@group(0) @binding(25) var<storage, read_write> coef: array<vec4<f32>>;
// per node, for the wet grid's pass: mass, 0, 0, volume (fixed point, as today's sums)
@group(0) @binding(26) var<storage, read_write> nodes_out: array<i32>;
// `vel` as the drops gather it: with the shut faces at walls given the liquid's speed along the wall
@group(0) @binding(27) var<storage, read_write> vel_out: array<vec4<f32>>;

fn finite2(v: vec2<f32>) -> bool {
    return abs(v.x) < 1e30 && abs(v.y) < 1e30;
}

fn node(x: i32, y: i32) -> u32 {
    return u32(y) * st.grid.x + u32(x);
}

fn fixed(x: f32, scale: f32) -> i32 {
    return i32(round(clamp(x * scale, -LIMIT, LIMIT)));
}

fn liquid_of(d: Drop) -> u32 {
    return d.kind & 15u;
}

fn steps(p: Drop) -> bool {
    return finite2(p.x) && finite2(p.v) && (p.kind & 255u) < STAIN_BASE;
}

fn wall_face(s: vec4<f32>, t: vec4<f32>, line_d: f32) -> vec3<f32> {
    if t.z < 0.0 {
        return vec3<f32>(s.xy, line_d);
    }
    let side = select(-1.0, 1.0, line_d >= 0.0);
    return vec3<f32>(s.xy * side, abs(line_d) - t.z);
}

// The share of a cell middle's kernel (a hat a cell wide each way) that lies more than `d` away
// along one way.
fn hat_beyond(d: f32) -> f32 {
    if d >= 1.0 {
        return 0.0;
    }
    if d <= -1.0 {
        return 1.0;
    }
    if d >= 0.0 {
        return 0.5 * (1.0 - d) * (1.0 - d);
    }
    return 1.0 - 0.5 * (1.0 + d) * (1.0 + d);
}

// ---- the walls, once they are put ------------------------------------------------------------

// How far node (x, y) is from the liquid's edge at a wall (a quarter cell off its face): below 0
// where no liquid can be. 4 off the band.
fn node_phi(x: i32, y: i32) -> f32 {
    if x < 0 || y < 0 || x >= i32(st.grid.x) || y >= i32(st.grid.y) {
        return 4.0;
    }
    let n = node(x, y);
    let s = solid[2u * n];
    if s.w < 0.5 {
        return 4.0;
    }
    return clamp(wall_face(s, solid[2u * n + 1u], s.z).z - GAP, -4.0, 4.0);
}

// What band node (x, y) says about point `q`: its wall's line distance there, its face distance
// there, the wall's half thickness (below 0 filled) and its number. The face is BIG off the band.
fn node_says(x: i32, y: i32, q: vec2<f32>) -> vec4<f32> {
    if x < 0 || y < 0 || x >= i32(st.grid.x) || y >= i32(st.grid.y) {
        return vec4<f32>(BIG, BIG, -1.0, 255.0);
    }
    let n = node(x, y);
    let s = solid[2u * n];
    if s.w < 0.5 {
        return vec4<f32>(BIG, BIG, -1.0, 255.0);
    }
    let t = solid[2u * n + 1u];
    let d = s.z + dot(s.xy, q - vec2<f32>(f32(x), f32(y)));
    return vec4<f32>(d, wall_face(s, t, d).z, t.z, t.w);
}

// How much to trust what band node (x, y) says about another point (less is better, added to the
// face distance when the nearest wall is picked): a node on its wall's line has that line's
// normal "either way round" (`Outline::nearest`), so which side it puts a point on is a coin
// toss. (That let a whole pool out through one wall of a box whose wall fell on a column of
// nodes.) Such a node is only used when no other node has the wall.
fn doubt(x: i32, y: i32) -> f32 {
    if x < 0 || y < 0 || x >= i32(st.grid.x) || y >= i32(st.grid.y) {
        return 0.0;
    }
    return select(0.0, 100.0, abs(solid[2u * node(x, y)].z) < 0.05);
}

// What the walls say about point `q`, of the four nodes round it: the nearest face's line
// distance, face distance, half thickness and number, from a node that can be trusted when one
// has it. With `thin`, only thin (hollow) walls are looked at.
fn walls_say(q: vec2<f32>, thin: bool) -> vec4<f32> {
    let lo = clamp(vec2<i32>(floor(q)), vec2<i32>(0), vec2<i32>(i32(st.grid.x) - 2, i32(st.grid.y) - 2));
    var best = vec4<f32>(BIG, BIG, -1.0, 255.0);
    var least = BIG;
    for (var a = 0; a < 2; a = a + 1) {
        for (var b = 0; b < 2; b = b + 1) {
            let w = node_says(lo.x + a, lo.y + b, q);
            if w.y > BIG * 0.5 || (thin && w.z < 0.0) {
                continue;
            }
            let score = w.y + doubt(lo.x + a, lo.y + b);
            if score < least {
                least = score;
                best = w;
            }
        }
    }
    return best;
}

// The open share of a face whose two ends are `a` and `b` from the liquid's edge.
fn open_share(a: f32, b: f32) -> f32 {
    if a >= 0.0 && b >= 0.0 {
        return 1.0;
    }
    if a < 0.0 && b < 0.0 {
        return 0.0;
    }
    let hi = max(a, b);
    let f = hi / (hi - min(a, b));
    if f < 0.1 {
        return 0.0;
    }
    if f > 0.9 {
        return 1.0;
    }
    return f;
}

// The thin (hollow) wall nearest the middle of cell (x, y): its line distance there and its
// number. (BIG, 255) with none.
fn thin_at_middle(x: i32, y: i32) -> vec2<f32> {
    let w = walls_say(vec2<f32>(f32(x) + 0.5, f32(y) + 0.5), true);
    return vec2<f32>(w.x, w.w);
}

// The same for any point.
fn thin_at(q: vec2<f32>) -> vec2<f32> {
    let w = walls_say(q, true);
    return vec2<f32>(w.x, w.w);
}

// Whether a thin wall's line passes between two cell middles (`a`, `b`: their `thin_at_middle`).
fn line_between(a: vec2<f32>, b: vec2<f32>) -> bool {
    return a.x < BIG * 0.5 && b.x < BIG * 0.5 && a.y == b.y && (a.x < 0.0) != (b.x < 0.0) && abs(a.x) + abs(b.x) < 1.5;
}

@compute @workgroup_size(256)
fn cs2_walls(@builtin(global_invocation_id) id: vec3<u32>) {
    let k = id.x;
    if k >= st.counts.w {
        return;
    }
    let x = i32(k % st.grid.x);
    let y = i32(k / st.grid.x);
    // The faces' open shares, by their end nodes.
    let p00 = node_phi(x, y);
    var open_u = open_share(p00, node_phi(x, y + 1));
    var open_v = open_share(p00, node_phi(x + 1, y));
    let mine = thin_at_middle(x, y);
    if line_between(mine, thin_at_middle(x - 1, y)) {
        open_u = 0.0;
    }
    if line_between(mine, thin_at_middle(x, y - 1)) {
        open_v = 0.0;
    }
    // The walls' share of the middle's kernel: the nearest face of its corners' walls.
    let near = walls_say(vec2<f32>(f32(x) + 0.5, f32(y) + 0.5), false);
    let face = near.y;
    let half = near.z;
    var share = 0.0;
    if face < BIG * 0.5 {
        let phi = face - GAP;
        share = hat_beyond(phi);
        if half >= 0.0 {
            // A band: only as far as the liquid's edge on its other side.
            share = share - hat_beyond(phi + 2.0 * (half + GAP));
        }
    }
    let at_u = thin_at(vec2<f32>(f32(x), f32(y) + 0.5));
    let at_v = thin_at(vec2<f32>(f32(x) + 0.5, f32(y)));
    wallc[2u * k] = vec4<f32>(open_u, open_v, share, 0.0);
    wallc[2u * k + 1u] = vec4<f32>(mine.x, at_u.x, at_v.x, mine.y + 256.0 * at_u.y + 65536.0 * at_v.y);
}

// ---- one step ------------------------------------------------------------------------------

@compute @workgroup_size(256)
fn cs2_clear(@builtin(global_invocation_id) id: vec3<u32>) {
    let k = id.x;
    if k >= st.counts.w {
        return;
    }
    for (var c = 0u; c < SUMS; c = c + 1u) {
        atomicStore(&sums[SUMS * k + c], 0);
    }
}

// The thin wall a drop at `x` is beside: its line distance and its number.
fn drop_side(x: vec2<f32>) -> vec2<f32> {
    return thin_at(x);
}

// Whether a point with thin-wall line distance `d` of wall `wall` is across that wall from a
// drop (`side`: its `drop_side`).
fn across(side: vec2<f32>, d: f32, wall: f32) -> bool {
    return side.x < BIG * 0.5 && d < BIG * 0.5 && side.y == wall && (side.x < 0.0) != (d < 0.0);
}

// The three wall numbers packed in a cell's `wallc`: of its middle, its left face, its top face.
fn wall_numbers(packed: f32) -> vec3<f32> {
    let p = u32(packed + 0.5);
    return vec3<f32>(f32(p & 255u), f32((p >> 8u) & 255u), f32((p >> 16u) & 255u));
}

// Where the 2 × 2 samples round `x` start, for samples at whole numbers plus `off`.
fn base2(x: vec2<f32>, off: vec2<f32>) -> vec2<i32> {
    let b = vec2<i32>(floor(x - off));
    return clamp(b, vec2<i32>(0), vec2<i32>(i32(st.grid.x) - 2, i32(st.grid.y) - 2));
}

@compute @workgroup_size(256)
fn cs2_p2g(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= counts[4] {
        return;
    }
    let p = drops[i];
    if !steps(p) {
        return;
    }
    let lq = st.liquids[liquid_of(p)];
    let weight = max(lq.b.y + p.mat.x, 0.05);
    let vol = 0.25;
    let mass = weight * vol;
    let dt = st.time.x;
    let v = p.v + vec2<f32>(st.time.y, st.time.z) * lq.b.z * dt;
    let side = drop_side(p.x);
    // Speed across, on left faces (at whole x, y + a half).
    {
        let off = vec2<f32>(0.0, 0.5);
        let base = base2(p.x, off);
        let f = clamp(p.x - off - vec2<f32>(base), vec2<f32>(0.0), vec2<f32>(1.0));
        for (var a = 0; a < 2; a = a + 1) {
            for (var b = 0; b < 2; b = b + 1) {
                let k = node(base.x + a, base.y + b);
                let wc = wallc[2u * k + 1u];
                if across(side, wc.y, wall_numbers(wc.w).y) {
                    continue;
                }
                let w = select(1.0 - f.x, f.x, a == 1) * select(1.0 - f.y, f.y, b == 1);
                let dpos = vec2<f32>(f32(base.x + a), f32(base.y + b) + 0.5) - p.x;
                let val = v.x + p.c.x * dpos.x + p.c.y * dpos.y;
                atomicAdd(&sums[SUMS * k], fixed(w * mass, S_M));
                atomicAdd(&sums[SUMS * k + 1u], fixed(w * mass * val, S_P));
            }
        }
    }
    // Speed down, on top faces (at x + a half, whole y).
    {
        let off = vec2<f32>(0.5, 0.0);
        let base = base2(p.x, off);
        let f = clamp(p.x - off - vec2<f32>(base), vec2<f32>(0.0), vec2<f32>(1.0));
        for (var a = 0; a < 2; a = a + 1) {
            for (var b = 0; b < 2; b = b + 1) {
                let k = node(base.x + a, base.y + b);
                let wc = wallc[2u * k + 1u];
                if across(side, wc.z, wall_numbers(wc.w).z) {
                    continue;
                }
                let w = select(1.0 - f.x, f.x, a == 1) * select(1.0 - f.y, f.y, b == 1);
                let dpos = vec2<f32>(f32(base.x + a) + 0.5, f32(base.y + b)) - p.x;
                let val = v.y + p.c.z * dpos.x + p.c.w * dpos.y;
                atomicAdd(&sums[SUMS * k + 2u], fixed(w * mass, S_M));
                atomicAdd(&sums[SUMS * k + 3u], fixed(w * mass * val, S_P));
            }
        }
    }
    // Volume, at cell middles.
    {
        let off = vec2<f32>(0.5, 0.5);
        let base = base2(p.x, off);
        let f = clamp(p.x - off - vec2<f32>(base), vec2<f32>(0.0), vec2<f32>(1.0));
        for (var a = 0; a < 2; a = a + 1) {
            for (var b = 0; b < 2; b = b + 1) {
                let k = node(base.x + a, base.y + b);
                let wc = wallc[2u * k + 1u];
                if across(side, wc.x, wall_numbers(wc.w).x) {
                    continue;
                }
                let w = select(1.0 - f.x, f.x, a == 1) * select(1.0 - f.y, f.y, b == 1);
                atomicAdd(&sums[SUMS * k + 4u], fixed(w * vol, S_M));
            }
        }
    }
}

// The speed on the left (`c` 0) or top (`c` 2) face of cell `k` from the sums, and whether it has
// one (enough liquid on it).
fn face_speed(k: u32, c: u32) -> vec2<f32> {
    let m = f32(atomicLoad(&sums[SUMS * k + c])) / S_M;
    if m < tr.a.w {
        return vec2<f32>(0.0, 0.0);
    }
    return vec2<f32>(f32(atomicLoad(&sums[SUMS * k + c + 1u])) / (S_P * m), 1.0);
}

// The open share of the left face of cell (x, y): its walls', and shut at the picture's edges
// when they stop the liquid.
fn open_left(x: i32, y: i32) -> f32 {
    let cols = i32(st.grid.x);
    let rows = i32(st.grid.y);
    if x < 0 || y < 0 || x >= cols || y >= rows - 1 {
        return 0.0;
    }
    if st.flags.w == 1u && (x <= i32(EDGE) || x >= cols - i32(EDGE) || y < i32(EDGE) || y >= rows - i32(EDGE)) {
        return 0.0;
    }
    return wallc[2u * node(x, y)].x;
}

fn open_top(x: i32, y: i32) -> f32 {
    let cols = i32(st.grid.x);
    let rows = i32(st.grid.y);
    if x < 0 || y < 0 || x >= cols - 1 || y >= rows {
        return 0.0;
    }
    if st.flags.w == 1u && (y <= i32(EDGE) || y >= rows - i32(EDGE) || x < i32(EDGE) || x >= cols - i32(EDGE)) {
        return 0.0;
    }
    return wallc[2u * node(x, y)].y;
}

// The picture's edges' share of a cell middle's kernel, when they stop the liquid.
fn edge_share(q: vec2<f32>) -> f32 {
    if st.flags.w != 1u {
        return 0.0;
    }
    let lo = EDGE + 0.05 - GAP;
    let hi = vec2<f32>(f32(st.grid.x), f32(st.grid.y)) - lo;
    let sx = hat_beyond(q.x - lo) + hat_beyond(hi.x - q.x);
    let sy = hat_beyond(q.y - lo) + hat_beyond(hi.y - q.y);
    return 1.0 - (1.0 - min(sx, 1.0)) * (1.0 - min(sy, 1.0));
}

@compute @workgroup_size(16, 16)
fn cs2_grid(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= st.grid.x || id.y >= st.grid.y {
        return;
    }
    let x = i32(id.x);
    let y = i32(id.y);
    let k = node(x, y);
    let cols = i32(st.grid.x);
    let rows = i32(st.grid.y);
    let left = face_speed(k, 0u);
    let top = face_speed(k, 2u);
    var bits = left.y + 2.0 * top.y;
    let liquid = f32(atomicLoad(&sums[SUMS * k + 4u])) / S_M;
    if x >= cols - 1 || y >= rows - 1 {
        // (The last column and row of nodes are no cells.)
        vel[k] = vec4<f32>(left.x, top.x, 0.0, bits);
        coef[k] = vec4<f32>(0.0);
        cellp[k] = vec4<f32>(0.0);
        return;
    }
    let right = face_speed(k + 1u, 0u);
    let bottom = face_speed(k + st.grid.x, 2u);
    let a_l = open_left(x, y);
    let a_r = open_left(x + 1, y);
    let a_t = open_top(x, y);
    let a_b = open_top(x, y + 1);
    let diag = a_l + a_r + a_t + a_b;
    let q = vec2<f32>(f32(x) + 0.5, f32(y) + 0.5);
    let walls = wallc[2u * k].z;
    let rho = liquid + 1.0 - (1.0 - walls) * (1.0 - edge_share(q));
    let solved = liquid > 1e-4 && diag > 1e-3;
    var want = 0.0;
    var p = 0.0;
    if solved {
        let dt = st.time.x;
        let out = a_r * right.x - a_l * left.x + a_b * bottom.x - a_t * top.x;
        // What must flow out: an over-full cell empties (a share of its excess a step, capped); one
        // with room may fill (a share of its room a step) before it pushes back.
        let e = rho - 1.0;
        var s = 0.0;
        if e > 0.0 {
            s = min(tr.a.y * e, tr.a.z) / dt;
        } else {
            s = tr.b.x * e / dt;
        }
        want = s - out;
        // The pressure it had: where the sweeps start from.
        p = cellp[k].x;
        if !(p >= 0.0 && p < 1e9) {
            p = 0.0;
        }
        bits = bits + 4.0;
    }
    vel[k] = vec4<f32>(left.x, top.x, liquid, bits);
    coef[k] = vec4<f32>(a_l, a_r, a_t, a_b);
    cellp[k] = vec4<f32>(p, want, diag, select(0.0, 1.0, solved));
}

// Whether a cell of `level` is solved, from its `cellp` entry: level 0 keeps a flag in w; a
// coarser level keeps its floor there (below) and is solved when its middle is above 0.
fn is_solved(c: vec4<f32>, level: u32) -> bool {
    if level == 0u {
        return c.w > 0.5;
    }
    return c.z > 0.0;
}

// The least a cell's answer may be: no pressure on level 0; on a coarser level, as far down as
// the fullest-drained of its four can still go (so a correction never asks any cell under it for
// a pressure below 0). Without this a pool in free fall, whose cells all lack "less than
// nothing", handed the coarse levels a big negative sum, and their answer wiped out the pressure
// of everything.
fn floor_of(c: vec4<f32>, level: u32) -> f32 {
    if level == 0u {
        return 0.0;
    }
    return c.w;
}

// One colour of a red-black sweep of one level (`tick.n.x`: twice the level, plus the colour).
// Level 0 is the pressure itself, which is never below 0; a coarser level holds the correction to
// the level under it, which may be.
@compute @workgroup_size(16, 16)
fn cs2_sor(@builtin(global_invocation_id) id: vec3<u32>) {
    let level = tick.n.x >> 1u;
    let lv = tr.lv[level];
    if id.x >= lv.y || id.y >= lv.z {
        return;
    }
    if ((id.x + id.y) & 1u) != (tick.n.x & 1u) {
        return;
    }
    let k = lv.x + id.y * lv.y + id.x;
    let c = cellp[k];
    if !is_solved(c, level) {
        return;
    }
    let a = coef[k];
    var sum = c.y;
    if id.x > 0u {
        sum = sum + a.x * cellp[k - 1u].x;
    }
    if id.x + 1u < lv.y {
        sum = sum + a.y * cellp[k + 1u].x;
    }
    if id.y > 0u {
        sum = sum + a.z * cellp[k - lv.y].x;
    }
    if id.y + 1u < lv.z {
        sum = sum + a.w * cellp[k + lv.y].x;
    }
    var p = c.x + tr.a.x * (sum / c.z - c.x);
    let least = floor_of(c, level);
    if !(p > least) {
        p = least;
    }
    cellp[k].x = p;
}

// Makes level `tick.n.x >> 1` (1 or more) from the level under it: each of its cells is four of
// that level's (2 x 2), its faces the sums of theirs that cross to a neighbouring four, what it
// must make up the sum of what theirs still lack. Its own answer starts at 0.
@compute @workgroup_size(16, 16)
fn cs2_down(@builtin(global_invocation_id) id: vec3<u32>) {
    let level = tick.n.x >> 1u;
    let lv = tr.lv[level];
    if id.x >= lv.y || id.y >= lv.z {
        return;
    }
    let f = tr.lv[level - 1u];
    var faces = vec4<f32>(0.0);
    var diag = 0.0;
    var lacks = 0.0;
    var any = false;
    var least = -1e30;
    for (var b = 0u; b < 2u; b = b + 1u) {
        for (var a = 0u; a < 2u; a = a + 1u) {
            let x = 2u * id.x + a;
            let y = 2u * id.y + b;
            if x >= f.y || y >= f.z {
                continue;
            }
            let k = f.x + y * f.y + x;
            let c = cellp[k];
            if !is_solved(c, level - 1u) {
                continue;
            }
            any = true;
            let under = floor_of(c, level - 1u);
            least = max(least, under - c.x);
            let o = coef[k];
            // Its four neighbours' answers, and whether each is solved.
            var n = vec4<f32>(0.0);
            var on = vec4<bool>(false);
            if x > 0u {
                let m = cellp[k - 1u];
                n.x = m.x;
                on.x = is_solved(m, level - 1u);
            }
            if x + 1u < f.y {
                let m = cellp[k + 1u];
                n.y = m.x;
                on.y = is_solved(m, level - 1u);
            }
            if y > 0u {
                let m = cellp[k - f.y];
                n.z = m.x;
                on.z = is_solved(m, level - 1u);
            }
            if y + 1u < f.z {
                let m = cellp[k + f.y];
                n.w = m.x;
                on.w = is_solved(m, level - 1u);
            }
            var r = c.y + dot(o, n) - c.z * c.x;
            // A cell held at its floor that would rather have less lacks nothing.
            if c.x <= under && r < 0.0 {
                r = 0.0;
            }
            lacks = lacks + r;
            diag = diag + c.z;
            // Left and right: the face inside the four (taken off the middle once from each
            // side), or the one that crosses to the next four.
            if a == 1u {
                if on.x {
                    diag = diag - o.x;
                }
                if on.y {
                    faces.y = faces.y + o.y;
                }
            } else {
                if on.y {
                    diag = diag - o.y;
                }
                if on.x {
                    faces.x = faces.x + o.x;
                }
            }
            if b == 1u {
                if on.z {
                    diag = diag - o.z;
                }
                if on.w {
                    faces.w = faces.w + o.w;
                }
            } else {
                if on.w {
                    diag = diag - o.w;
                }
                if on.z {
                    faces.z = faces.z + o.z;
                }
            }
        }
    }
    let k = lv.x + id.y * lv.y + id.x;
    // What is left of the middle once the faces to the neighbouring fours are taken off is the
    // four's way out to the air. The faces are halved (the fours' middles are twice as far apart
    // as the cells' were); the way out is not, but its middle is a quarter of itself further
    // from the air than the cells' middles were. (Halved like the faces, every level put the
    // surface further off than it is, and a deep pool's pressure swung instead of settling.)
    let air = max(diag - (faces.x + faces.y + faces.z + faces.w), 0.0);
    faces = 0.5 * faces;
    diag = faces.x + faces.y + faces.z + faces.w + air / (1.0 + 0.25 * air);
    let solved = any && diag > 1e-4;
    coef[k] = select(vec4<f32>(0.0), faces, solved);
    cellp[k] = vec4<f32>(0.0, select(0.0, lacks, solved), select(0.0, diag, solved), min(least, 0.0));
}

// Adds level `(tick.n.x >> 1) + 1`'s answer to level `tick.n.x >> 1`: each cell takes its four's.
@compute @workgroup_size(16, 16)
fn cs2_up(@builtin(global_invocation_id) id: vec3<u32>) {
    let level = tick.n.x >> 1u;
    let lv = tr.lv[level];
    if id.x >= lv.y || id.y >= lv.z {
        return;
    }
    let k = lv.x + id.y * lv.y + id.x;
    let c = cellp[k];
    if !is_solved(c, level) {
        return;
    }
    let up = tr.lv[level + 1u];
    let e = cellp[up.x + (id.y >> 1u) * up.y + (id.x >> 1u)].x;
    var p = c.x + tr.b.z * e;
    let least = floor_of(c, level);
    if !(p > least) {
        p = least;
    }
    cellp[k].x = p;
}

// The faces' speeds with the pressure's push, for the drops to gather; and the liquid at each
// node as today's sums have it, for the wet grid.
@compute @workgroup_size(16, 16)
fn cs2_proj(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= st.grid.x || id.y >= st.grid.y {
        return;
    }
    let k = id.y * st.grid.x + id.x;
    let x = i32(id.x);
    let y = i32(id.y);
    var v = vel[k];
    let bits = u32(v.w + 0.5);
    var has_u = (bits & 1u) != 0u;
    var has_v = (bits & 2u) != 0u;
    let p = cellp[k].x;
    if has_u {
        if open_left(x, y) > 0.0 {
            var other = 0.0;
            if x > 0 {
                other = cellp[k - 1u].x;
            }
            v.x = v.x - (p - other);
        } else {
            // A shut face has no speed of its own: the drops leave it out.
            has_u = false;
        }
    }
    if has_v {
        if open_top(x, y) > 0.0 {
            var other = 0.0;
            if y > 0 {
                other = cellp[k - st.grid.x].x;
            }
            v.y = v.y - (p - other);
        } else {
            has_v = false;
        }
    }
    vel[k] = vec4<f32>(v.x, v.y, v.z, f32(u32(has_u) + 2u * u32(has_v) + (bits & 4u)));
    // The liquid at node (x, y): the mean of the four cells that meet there.
    var liquid = 0.0;
    for (var a = -1; a <= 0; a = a + 1) {
        for (var b = -1; b <= 0; b = b + 1) {
            if x + a >= 0 && y + b >= 0 {
                liquid = liquid + f32(atomicLoad(&sums[SUMS * node(x + a, y + b) + 4u])) / S_M;
            }
        }
    }
    liquid = 0.25 * liquid;
    nodes_out[4u * k] = i32(liquid * S_M);
    nodes_out[4u * k + 1u] = 0;
    nodes_out[4u * k + 2u] = 0;
    nodes_out[4u * k + 3u] = i32(liquid * S_M);
}

// The mean of the speeds (`which` 0 across, 1 down) of the faces of cells `a` to `d` that have one,
// and how many had.
fn mean_of(which: u32, a: u32, b: u32, c: u32, d: u32) -> vec2<f32> {
    var sum = 0.0;
    var n = 0.0;
    let ks = array<u32, 4>(a, b, c, d);
    for (var e = 0; e < 4; e = e + 1) {
        let g = vel[ks[e]];
        if (u32(g.w + 0.5) & (1u << which)) != 0u {
            sum = sum + select(g.y, g.x, which == 0u);
            n = n + 1.0;
        }
    }
    if n < 0.5 {
        return vec2<f32>(0.0, 0.0);
    }
    return vec2<f32>(sum / n, n);
}

// The wall's normal at `q`, of the nearer of nodes `a` and `b`'s walls. Nothing with no wall.
fn normal_at(q: vec2<f32>, a: vec2<i32>, b: vec2<i32>) -> vec2<f32> {
    var best = vec2<f32>(0.0);
    var least = BIG;
    let both = array<vec2<i32>, 2>(a, b);
    for (var e = 0; e < 2; e = e + 1) {
        let at = both[e];
        if at.x < 0 || at.y < 0 || at.x >= i32(st.grid.x) || at.y >= i32(st.grid.y) {
            continue;
        }
        let n = node(at.x, at.y);
        let s = solid[2u * n];
        if s.w < 0.5 {
            continue;
        }
        let f = wall_face(s, solid[2u * n + 1u], s.z + dot(s.xy, q - vec2<f32>(at)));
        if f.z < least {
            least = f.z;
            best = f.xy;
        }
    }
    return best;
}

// Free slip: a face a wall shuts takes the liquid's speed beside it, less its part into or out
// of the wall (so a face along a wall's normal reads nothing, and one inside a slope reads the
// slide along it). Without it the drops by a wall gathered a share of "nothing" from every shut
// face: a slope braked the liquid like a staircase.
@compute @workgroup_size(16, 16)
fn cs2_slip(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= st.grid.x || id.y >= st.grid.y {
        return;
    }
    let k = id.y * st.grid.x + id.x;
    let x = i32(id.x);
    let y = i32(id.y);
    let cols = i32(st.grid.x);
    let rows = i32(st.grid.y);
    var v = vel[k];
    var bits = u32(v.w + 0.5);
    let inner = x > 0 && y > 0 && x < cols - 1 && y < rows - 1;
    if inner && (bits & 1u) == 0u && open_left(x, y) <= 0.0 {
        let own = mean_of(0u, k - 1u, k + 1u, k - st.grid.x, k + st.grid.x);
        if own.y > 0.5 {
            let other = mean_of(1u, k - 1u, k, k - 1u + st.grid.x, k + st.grid.x);
            var n = normal_at(vec2<f32>(f32(x), f32(y) + 0.5), vec2<i32>(x, y), vec2<i32>(x, y + 1));
            if st.flags.w == 1u && (x <= i32(EDGE) || x >= cols - i32(EDGE)) {
                n = vec2<f32>(1.0, 0.0);
            } else if st.flags.w == 1u && (y < i32(EDGE) || y >= rows - i32(EDGE)) {
                n = vec2<f32>(0.0, 1.0);
            }
            let flow = vec2<f32>(own.x, other.x);
            v.x = (flow - dot(flow, n) * n).x;
            bits = bits | 1u;
        }
    }
    if inner && (bits & 2u) == 0u && open_top(x, y) <= 0.0 {
        let own = mean_of(1u, k - 1u, k + 1u, k - st.grid.x, k + st.grid.x);
        if own.y > 0.5 {
            let other = mean_of(0u, k, k + 1u, k - st.grid.x, k - st.grid.x + 1u);
            var n = normal_at(vec2<f32>(f32(x) + 0.5, f32(y)), vec2<i32>(x, y), vec2<i32>(x + 1, y));
            if st.flags.w == 1u && (y <= i32(EDGE) || y >= rows - i32(EDGE)) {
                n = vec2<f32>(0.0, 1.0);
            } else if st.flags.w == 1u && (x < i32(EDGE) || x >= cols - i32(EDGE)) {
                n = vec2<f32>(1.0, 0.0);
            }
            let flow = vec2<f32>(other.x, own.x);
            v.y = (flow - dot(flow, n) * n).y;
            bits = bits | 2u;
        }
    }
    vel_out[k] = vec4<f32>(v.x, v.y, v.z, f32(bits));
}

// One of a drop's two speeds gathered from the 2 × 2 faces round it: the speed and its slope
// (along x, along y). Faces with no speed, and those across a thin wall, are left out and the
// rest re-weighted, as if the missing ones had the mean. `had` is 0 when no face had a speed.
struct Got {
    v: f32,
    slope: vec2<f32>,
    had: f32,
};

fn gather_one(x: vec2<f32>, off: vec2<f32>, which: u32, side: vec2<f32>) -> Got {
    let base = base2(x, off);
    let f = clamp(x - off - vec2<f32>(base), vec2<f32>(0.0), vec2<f32>(1.0));
    var ws = array<f32, 4>(0.0, 0.0, 0.0, 0.0);
    var vs = array<f32, 4>(0.0, 0.0, 0.0, 0.0);
    var sum_w = 0.0;
    var sum_v = 0.0;
    for (var a = 0; a < 2; a = a + 1) {
        for (var b = 0; b < 2; b = b + 1) {
            let k = node(base.x + a, base.y + b);
            let g = vel_out[k];
            let bits = u32(g.w + 0.5);
            let wc = wallc[2u * k + 1u];
            let numbers = wall_numbers(wc.w);
            var ok = (bits & (1u << which)) != 0u;
            if which == 0u {
                ok = ok && !across(side, wc.y, numbers.y);
            } else {
                ok = ok && !across(side, wc.z, numbers.z);
            }
            if ok {
                let w = select(1.0 - f.x, f.x, a == 1) * select(1.0 - f.y, f.y, b == 1);
                let val = select(g.y, g.x, which == 0u);
                ws[a * 2 + b] = 1.0;
                vs[a * 2 + b] = val;
                sum_w = sum_w + w;
                sum_v = sum_v + w * val;
            }
        }
    }
    if sum_w < 1e-4 {
        return Got(0.0, vec2<f32>(0.0), 0.0);
    }
    let mean = sum_v / sum_w;
    var slope = vec2<f32>(0.0);
    for (var a = 0; a < 2; a = a + 1) {
        for (var b = 0; b < 2; b = b + 1) {
            if ws[a * 2 + b] > 0.5 {
                let wx = select(1.0 - f.x, f.x, a == 1);
                let wy = select(1.0 - f.y, f.y, b == 1);
                let dx = select(-1.0, 1.0, a == 1) * wy;
                let dy = select(-1.0, 1.0, b == 1) * wx;
                slope = slope + vec2<f32>(dx, dy) * (vs[a * 2 + b] - mean);
            }
        }
    }
    return Got(mean, slope, 1.0);
}

// Keeps a drop at `x` moving at `v` off the walls and the picture's edges, as today's core does.
struct Kept {
    x: vec2<f32>,
    v: vec2<f32>,
};

fn keep_off(xin: vec2<f32>, vin: vec2<f32>, stops: bool) -> Kept {
    var x = xin;
    var v = vin;
    // The nearest node's wall, as today; or, when that node is on its wall's line (its normal may
    // point either way), the wall as the best of the four nodes round the drop has it.
    var near = clamp(vec2<i32>(round(x)), vec2<i32>(0), vec2<i32>(i32(st.grid.x) - 1, i32(st.grid.y) - 1));
    if solid[2u * node(near.x, near.y)].w > 0.5 && doubt(near.x, near.y) > 0.0 {
        let lo = clamp(vec2<i32>(floor(x)), vec2<i32>(0), vec2<i32>(i32(st.grid.x) - 2, i32(st.grid.y) - 2));
        var least = BIG;
        for (var a = 0; a < 2; a = a + 1) {
            for (var b = 0; b < 2; b = b + 1) {
                let w = node_says(lo.x + a, lo.y + b, x);
                let score = w.y + doubt(lo.x + a, lo.y + b);
                if w.y < BIG * 0.5 && score < least {
                    least = score;
                    near = vec2<i32>(lo.x + a, lo.y + b);
                }
            }
        }
    }
    let n = node(near.x, near.y);
    let s = solid[2u * n];
    if s.w > 0.5 {
        let t = solid[2u * n + 1u];
        let f = wall_face(s, t, s.z + dot(s.xy, x - vec2<f32>(near)));
        if f.z < 0.5 {
            x = x + f.xy * min(0.5 - f.z, 4.0);
            var vr = v - t.xy;
            let vn = dot(vr, f.xy);
            if vn < 0.0 {
                vr = vr - vn * f.xy;
            }
            v = vr + t.xy;
        }
    }
    if stops {
        let lo = vec2<f32>(EDGE);
        let hi = vec2<f32>(f32(st.grid.x) - EDGE, f32(st.grid.y) - EDGE);
        if x.x < lo.x + 0.05 { x.x = lo.x + 0.05; v.x = max(v.x, 0.0); }
        if x.x > hi.x - 0.05 { x.x = hi.x - 0.05; v.x = min(v.x, 0.0); }
        if x.y < lo.y + 0.05 { x.y = lo.y + 0.05; v.y = max(v.y, 0.0); }
        if x.y > hi.y - 0.05 { x.y = hi.y - 0.05; v.y = min(v.y, 0.0); }
    }
    return Kept(x, v);
}

@compute @workgroup_size(256)
fn cs2_g2p(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= counts[4] {
        return;
    }
    var p = drops[i];
    if !steps(p) {
        return;
    }
    let lq = st.liquids[liquid_of(p)];
    let dt = st.time.x;
    let side = drop_side(p.x);
    let gu = gather_one(p.x, vec2<f32>(0.0, 0.5), 0u, side);
    let gv = gather_one(p.x, vec2<f32>(0.5, 0.0), 1u, side);
    // With no face to go by (it is alone in a wall's margin), it falls on its own.
    let own = p.v + vec2<f32>(st.time.y, st.time.z) * lq.b.z * dt;
    var v = vec2<f32>(select(own.x, gu.v, gu.had > 0.5), select(own.y, gv.v, gv.had > 0.5));
    var c = vec4<f32>(gu.slope, gv.slope);
    // The liquid round it (the picture draws a drop with little round it as a droplet).
    {
        let off = vec2<f32>(0.5, 0.5);
        let base = base2(p.x, off);
        let f = clamp(p.x - off - vec2<f32>(base), vec2<f32>(0.0), vec2<f32>(1.0));
        var liquid = 0.0;
        for (var a = 0; a < 2; a = a + 1) {
            for (var b = 0; b < 2; b = b + 1) {
                let w = select(1.0 - f.x, f.x, a == 1) * select(1.0 - f.y, f.y, b == 1);
                liquid = liquid + w * vel_out[node(base.x + a, base.y + b)].z;
            }
        }
        p.j = 1.0 / max(liquid, 0.05);
    }
    v = v * exp(-lq.a.w * dt);
    let speed = length(v);
    if speed > st.time.w {
        v = v * (st.time.w / speed);
        c = vec4<f32>(0.0);
    }
    if !finite2(v) {
        v = vec2<f32>(0.0);
        c = vec4<f32>(0.0);
    }
    // Moved in short sub-moves, kept off the walls at each: a fast drop cannot jump a thin wall.
    let stops = (p.kind & STOPS) != 0u;
    let moves = clamp(u32(ceil(length(v) * dt / max(tr.b.y, 0.05))), 1u, 24u);
    let h = dt / f32(moves);
    var x = p.x;
    for (var m = 0u; m < moves; m = m + 1u) {
        let kept = keep_off(x + v * h, v, stops);
        x = kept.x;
        v = kept.v;
    }
    drops[i].x = x;
    drops[i].v = v;
    drops[i].c = c;
    drops[i].j = p.j;
    drops[i].kick = max(p.kick - 2.0 * dt, 0.0);
    drops[i].more = vec4<f32>(0.0);
}

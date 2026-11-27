// ---- deep water: the grid's pressure underneath (part C2) ----------------------------------------
//
// After the sort and before the sweeps, on the sorted drops: each drop's mass, the speed it would
// have (the place it would reach less where it was, over the step) and its volume are gathered onto
// the layout's cells as whole numbers (`cs_deep_gather`); each cell works out its faces' speeds, how
// full it is and what must flow out of it (`cs_deep_cells`, the grid core's `cs_grid`); how deep each
// cell lies under the nearest cell that is not full is found down its column and then across its row
// (`cs_deep_column`, `cs_deep_across`), and from that its depth share `w`; the one-sided pressure is
// solved by red-black sweeps on a ladder of coarser grids from the pressure the step before kept
// (`cs_deep_sor`, `cs_deep_down`, `cs_deep_up`, the grid core's own); and every drop with a depth share
// takes the pressure's change of speed at its place, times its share, on the place it would reach
// (`cs_deep_push`). A drop whose share is 0 is not written: thin water is the drops' own.
//
// Cells: cell (x, y) is the square from node (x, y) to node (x + 1, y + 1) and shares the node's
// index; its left face is at x, its top face at y.

const S_M: f32 = 1048576.0;
const S_P: f32 = 4096.0;
const LIMIT: f32 = 33554432.0;
// Words a cell in `sums`: mass on its left face, momentum across, mass on its top face, momentum
// down, volume at its middle, and that volume times how full each drop in it was.
const SUMS: u32 = 6u;
// A drop's volume, in cells (four drops fill a cell).
const VOL: f32 = 0.25;

// Per cell: the drops' sums (whole numbers, added atomically: the same in any order).
@group(0) @binding(19) var<storage, read_write> sums: array<atomic<i32>>;
// Per cell, from the solids: the open share of its left face, of its top face, the solids' share of
// its middle's kernel, 0.
@group(0) @binding(20) var<storage, read_write> wallc: array<vec4<f32>>;
// Per cell of every level: pressure x dt, what must flow out less what does, the open shares' sum,
// solved (level 0) or the least the level's answer may be (coarser levels).
@group(0) @binding(21) var<storage, read_write> cellp: array<vec4<f32>>;
// Per cell of every level: its faces' open shares, left, right, top, bottom.
@group(0) @binding(22) var<storage, read_write> coef: array<vec4<f32>>;
// Per cell: how full it is (the cells pass), then its depth share (the across pass).
@group(0) @binding(23) var<storage, read_write> dfa: array<f32>;
// Per cell: how far down or up its column the nearest cell that is not full is, in cells.
@group(0) @binding(24) var<storage, read_write> dfb: array<f32>;
// The cells deep at the frame's last step.
@group(0) @binding(25) var<storage, read_write> deep_count: array<atomic<u32>>;
// The liquid, and the liquid times its drops' fullness, where the drops-only water meets the deep
// water (`REST` fixed point; emptied by the cells pass, summed by the across pass). One pair for the
// whole body, so pools in one course rest alike; the sums stay under 2^32 while that band has
// under a million cells (it is a strip a few cells deep under each surface).
@group(0) @binding(26) var<storage, read_write> rest: array<atomic<u32>>;
const REST: f32 = 4096.0;

fn fixed(x: f32, scale: f32) -> i32 {
    return i32(round(clamp(x * scale, -LIMIT, LIMIT)));
}

fn in_grid(x: i32, y: i32) -> bool {
    return x >= 0 && y >= 0 && x < i32(fr.grid.x) && y < i32(fr.grid.y);
}

fn edges_shut() -> bool {
    return (fr.grid.w & 1u) != 0u;
}

// Where the 2 x 2 samples round `x` start, for samples at whole numbers plus `off`.
fn base2(x: vec2<f32>, off: vec2<f32>) -> vec2<i32> {
    let b = vec2<i32>(floor(x - off));
    return clamp(b, vec2<i32>(0), vec2<i32>(i32(fr.grid.x) - 2, i32(fr.grid.y) - 2));
}

fn bilinear(f: vec2<f32>, a: i32, b: i32) -> f32 {
    return select(1.0 - f.x, f.x, a == 1) * select(1.0 - f.y, f.y, b == 1);
}

// ---- the solids, once ----------------------------------------------------------------------------

// How far node (x, y) is from the liquid's edge at a solid (below 0 where no liquid can be). Far, off
// the grid.
fn node_phi(x: i32, y: i32) -> f32 {
    if !in_grid(x, y) {
        return 4.0;
    }
    return clamp(solid[node(x, y)] - fr.mg2.z, -4.0, 4.0);
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

// The solids' share of a kernel a cell wide each way round `q` (a hat, as the drops' volume is
// spread): how much of it lies past the liquid's edge, from 5 x 5 samples of the solid field.
fn solid_share(q: vec2<f32>) -> f32 {
    var sum = 0.0;
    var all = 0.0;
    for (var a = -2; a <= 2; a = a + 1) {
        for (var b = -2; b <= 2; b = b + 1) {
            let o = vec2<f32>(f32(a), f32(b)) * 0.4;
            let w = (1.0 - abs(o.x)) * (1.0 - abs(o.y));
            let phi = solid_at(q + o).z - fr.mg2.z;
            sum = sum + w * clamp(0.5 - phi / 0.4, 0.0, 1.0);
            all = all + w;
        }
    }
    return sum / all;
}

@compute @workgroup_size(256)
fn cs_deep_walls(@builtin(global_invocation_id) gid: vec3<u32>) {
    let k = gid.x;
    if k >= fr.grid.x * fr.grid.y {
        return;
    }
    let x = i32(k % fr.grid.x);
    let y = i32(k / fr.grid.x);
    let p00 = node_phi(x, y);
    let open_u = open_share(p00, node_phi(x, y + 1));
    let open_v = open_share(p00, node_phi(x + 1, y));
    wallc[k] = vec4<f32>(open_u, open_v, solid_share(vec2<f32>(f32(x) + 0.5, f32(y) + 0.5)), 0.0);
}

// ---- the drops to the cells ----------------------------------------------------------------------

@compute @workgroup_size(256)
fn cs_deep_gather(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= st.n.x {
        return;
    }
    let x = y_in[i].xy;
    if !alive(x) {
        return;
    }
    let was = velb[i];
    let v = (x - was.xy) / fr.time.x;
    if !finite2(v) {
        return;
    }
    // (Air is gathered as water: a pocket of it is full to the grid, and moves.)
    let kind = u32(was.z);
    let mass = select(fr.feels[kind & 15u].b.w, 1.0, is_air(kind)) * VOL;
    // How full it was (its own density by its neighbours and the solids, as its sweeps see it).
    let full = clamp(was.w, 0.0, 4.0);
    // Speed across, on left faces (at whole x, y + a half).
    {
        let off = vec2<f32>(0.0, 0.5);
        let b = base2(x, off);
        let f = clamp(x - off - vec2<f32>(b), vec2<f32>(0.0), vec2<f32>(1.0));
        for (var a = 0; a < 2; a = a + 1) {
            for (var c = 0; c < 2; c = c + 1) {
                let k = node(b.x + a, b.y + c);
                let w = bilinear(f, a, c);
                atomicAdd(&sums[SUMS * k], fixed(w * mass, S_M));
                atomicAdd(&sums[SUMS * k + 1u], fixed(w * mass * v.x, S_P));
            }
        }
    }
    // Speed down, on top faces (at x + a half, whole y).
    {
        let off = vec2<f32>(0.5, 0.0);
        let b = base2(x, off);
        let f = clamp(x - off - vec2<f32>(b), vec2<f32>(0.0), vec2<f32>(1.0));
        for (var a = 0; a < 2; a = a + 1) {
            for (var c = 0; c < 2; c = c + 1) {
                let k = node(b.x + a, b.y + c);
                let w = bilinear(f, a, c);
                atomicAdd(&sums[SUMS * k + 2u], fixed(w * mass, S_M));
                atomicAdd(&sums[SUMS * k + 3u], fixed(w * mass * v.y, S_P));
            }
        }
    }
    // Volume, at cell middles.
    {
        let off = vec2<f32>(0.5, 0.5);
        let b = base2(x, off);
        let f = clamp(x - off - vec2<f32>(b), vec2<f32>(0.0), vec2<f32>(1.0));
        for (var a = 0; a < 2; a = a + 1) {
            for (var c = 0; c < 2; c = c + 1) {
                let at = SUMS * node(b.x + a, b.y + c);
                atomicAdd(&sums[at + 4u], fixed(bilinear(f, a, c) * VOL, S_M));
                atomicAdd(&sums[at + 5u], fixed(bilinear(f, a, c) * VOL * full, S_M));
            }
        }
    }
}

// ---- the cells: how full, and what must flow out ------------------------------------------------

// The speed on the left (`c` 0) or top (`c` 2) face of cell `k` from the sums, and whether it has
// one (enough liquid on it).
fn face_speed(k: u32, c: u32) -> vec2<f32> {
    let m = f32(atomicLoad(&sums[SUMS * k + c])) / S_M;
    if m < fr.mg2.y {
        return vec2<f32>(0.0, 0.0);
    }
    return vec2<f32>(f32(atomicLoad(&sums[SUMS * k + c + 1u])) / (S_P * m), 1.0);
}

// The open share of the left face of cell (x, y): its solids', and shut at the course's edges when
// they stop the liquid.
fn open_left(x: i32, y: i32) -> f32 {
    let cols = i32(fr.grid.x);
    let rows = i32(fr.grid.y);
    if x < 0 || y < 0 || x >= cols || y >= rows - 1 {
        return 0.0;
    }
    if edges_shut() && (x <= i32(EDGE) || x >= cols - 1 - i32(EDGE) || y < i32(EDGE) || y >= rows - 1 - i32(EDGE)) {
        return 0.0;
    }
    return wallc[node(x, y)].x;
}

fn open_top(x: i32, y: i32) -> f32 {
    let cols = i32(fr.grid.x);
    let rows = i32(fr.grid.y);
    if x < 0 || y < 0 || x >= cols - 1 || y >= rows {
        return 0.0;
    }
    if edges_shut() && (y <= i32(EDGE) || y >= rows - 1 - i32(EDGE) || x < i32(EDGE) || x >= cols - 1 - i32(EDGE)) {
        return 0.0;
    }
    return wallc[node(x, y)].y;
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

// The course's edges' share of a cell middle's kernel, when they stop the liquid.
fn edge_share(q: vec2<f32>) -> f32 {
    if !edges_shut() {
        return 0.0;
    }
    let lo = fr.mg2.w;
    let hi = vec2<f32>(f32(fr.grid.x) - 1.0, f32(fr.grid.y) - 1.0) - lo;
    let sx = hat_beyond(q.x - lo) + hat_beyond(hi.x - q.x);
    let sy = hat_beyond(q.y - lo) + hat_beyond(hi.y - q.y);
    return 1.0 - (1.0 - min(sx, 1.0)) * (1.0 - min(sy, 1.0));
}

// Whether the drops reach the cell at (x, y) and it is less than half solid.
fn wet_open(x: i32, y: i32) -> bool {
    if !in_grid(x, y) {
        return false;
    }
    let k = node(x, y);
    return f32(atomicLoad(&sums[SUMS * k + 4u])) / S_M > 1e-4 && wallc[k].z < 0.5;
}

@compute @workgroup_size(256)
fn cs_deep_cells(@builtin(global_invocation_id) gid: vec3<u32>) {
    let k = gid.x;
    if k >= fr.grid.x * fr.grid.y {
        return;
    }
    let cols = i32(fr.grid.x);
    let rows = i32(fr.grid.y);
    let x = i32(k % fr.grid.x);
    let y = i32(k / fr.grid.x);
    if x >= cols - 1 || y >= rows - 1 {
        // (The last column and row of nodes are no cells.)
        coef[k] = vec4<f32>(0.0);
        cellp[k] = vec4<f32>(0.0);
        dfa[k] = select(0.0, 1.0, edges_shut());
        return;
    }
    let left = face_speed(k, 0u);
    let top = face_speed(k, 2u);
    let right = face_speed(k + 1u, 0u);
    let bottom = face_speed(k + fr.grid.x, 2u);
    let a_l = open_left(x, y);
    let a_r = open_left(x + 1, y);
    let a_t = open_top(x, y);
    let a_b = open_top(x, y + 1);
    let diag = a_l + a_r + a_t + a_b;
    let liquid = f32(atomicLoad(&sums[SUMS * k + 4u])) / S_M;
    let q = vec2<f32>(f32(x) + 0.5, f32(y) + 0.5);
    let rho = liquid + 1.0 - (1.0 - wallc[k].z) * (1.0 - edge_share(q));
    // How full the drops in it are, as their own sweeps see it (their neighbours and the solids):
    // the drops and the grid then rest at the same spacing. (The cell's own share of liquid reads
    // short beside a face, where the drops rest off it: as the fill, it pulled liquid into the
    // wall's cells for ever.)
    var dense = 0.0;
    if liquid > 1e-4 {
        dense = f32(atomicLoad(&sums[SUMS * k + 5u])) / S_M / liquid;
    }
    let solved = liquid > 1e-4 && diag > 1e-3;
    if k == 0u {
        atomicStore(&rest[0], 0u);
        atomicStore(&rest[1], 0u);
    }
    var want = 0.0;
    var p = 0.0;
    if solved {
        // A face no drop reaches is air: it takes the speed of the face across the cell.
        let u_l = select(select(0.0, right.x, right.y > 0.5), left.x, left.y > 0.5);
        let u_r = select(select(0.0, left.x, left.y > 0.5), right.x, right.y > 0.5);
        let v_t = select(select(0.0, bottom.x, bottom.y > 0.5), top.x, top.y > 0.5);
        let v_b = select(select(0.0, top.x, top.y > 0.5), bottom.x, bottom.y > 0.5);
        // (What must flow out of it is worked out once its depth is known: `cs_deep_want`.)
        want = -(a_r * u_r - a_l * u_l + a_b * v_b - a_t * v_t);
        // The pressure it kept (pressure x the step before's dt: on a frame's first step, scaled to
        // this step's; 0 when the deep part starts afresh).
        p = cellp[k].x * select(1.0, fr.deep2.x, (st.n.w & 2u) != 0u);
        if !(p >= 0.0 && p < 1e9) {
            p = 0.0;
        }
    }
    coef[k] = vec4<f32>(a_l, a_r, a_t, a_b);
    // (A solved cell's flag is one plus how full its drops are, for `cs_deep_want`.)
    cellp[k] = vec4<f32>(p, want, diag, select(0.0, 1.0 + dense, solved));
    // Depth is measured from the liquid's own edge: a cell the drops reach that is not full (full:
    // the drops in it are full and they and the solids cover at least half of it, or it is at least
    // half solid), or an open cell they do not reach beside an open one they do. Not from a cell no
    // drop reaches past a solid: the air beyond a thin wall is not near the water inside it, and
    // water against a wall is as deep as the wall is high.
    let wet = liquid > 1e-4;
    let open = wallc[k].z < 0.5;
    // (Inside a magnet's reach no cell the drops reach is full: that water is the drops' own.)
    let full = (dense >= fr.deep.z && rho >= 0.5 && !in_magnet(q)) || !open;
    var edge = wet && !full;
    if !wet && open {
        edge = wet_open(x - 1, y) || wet_open(x + 1, y) || wet_open(x, y - 1) || wet_open(x, y + 1);
    }
    dfa[k] = select(1.0, 0.0, edge);
}

// ---- how deep each cell is -------------------------------------------------------------------------

// Whether the cell at (x, y) is full (outside the course it is when the course's edges are shut).
fn is_full(x: i32, y: i32) -> bool {
    if !in_grid(x, y) {
        return edges_shut();
    }
    return dfa[node(x, y)] > 0.5;
}

@compute @workgroup_size(256)
fn cs_deep_column(@builtin(global_invocation_id) gid: vec3<u32>) {
    let k = gid.x;
    if k >= fr.grid.x * fr.grid.y {
        return;
    }
    let x = i32(k % fr.grid.x);
    let y = i32(k / fr.grid.x);
    let reach = i32(fr.dgrid.y);
    var g = f32(reach + 1);
    if !is_full(x, y) {
        g = 0.0;
    } else {
        for (var d = 1; d <= reach; d = d + 1) {
            if !is_full(x, y - d) || !is_full(x, y + d) {
                g = f32(d);
                break;
            }
        }
    }
    dfb[k] = g;
}

@compute @workgroup_size(256)
fn cs_deep_across(@builtin(global_invocation_id) gid: vec3<u32>) {
    let k = gid.x;
    if k >= fr.grid.x * fr.grid.y {
        return;
    }
    // (The cell's sums are done with: emptied for the next step's gather.)
    let liquid = f32(atomicLoad(&sums[SUMS * k + 4u])) / S_M;
    let wet = liquid > 1e-4;
    let filled = f32(atomicLoad(&sums[SUMS * k + 5u])) / S_M;
    for (var s = 0u; s < SUMS; s = s + 1u) {
        atomicStore(&sums[SUMS * k + s], 0);
    }
    let x = i32(k % fr.grid.x);
    let y = i32(k / fr.grid.x);
    let reach = i32(fr.dgrid.y);
    let g0 = dfb[k];
    var best = g0 * g0;
    for (var d = 1; d <= reach; d = d + 1) {
        let dd = f32(d * d);
        if dd >= best {
            break;
        }
        for (var side = -1; side <= 1; side = side + 2) {
            let xx = x + side * d;
            var g = 0.0;
            if xx < 0 || xx >= i32(fr.grid.x) {
                if edges_shut() {
                    continue;
                }
            } else {
                g = dfb[node(xx, y)];
            }
            best = min(best, dd + g * g);
        }
    }
    // (Only a cell the drops reach has a depth.)
    let depth = sqrt(best);
    let w = select(0.0, smoothstep(fr.deep.x, fr.deep.x + fr.deep.y, depth), wet);
    dfa[k] = w;
    // Where the drops-only water meets the deep water (its last cell and the blend): how full it is.
    if wet && depth >= fr.deep.x - 1.0 && depth <= fr.deep.x + fr.deep.y {
        atomicAdd(&rest[0], u32(round(liquid * REST)));
        atomicAdd(&rest[1], u32(round(clamp(filled, 0.0, 8.0) * REST)));
    }
    // The pressure is only worked out where it pushes: above that the drops hold themselves up,
    // and their weight is the deep water's to carry (it presses on it, and the deep water pushes
    // back where it is over-full).
    if !(w > 0.0) {
        cellp[k] = vec4<f32>(0.0, 0.0, cellp[k].z, 0.0);
    }
    if (st.n.w & 1u) != 0u && w > 0.0 {
        atomicAdd(&deep_count[0], 1u);
    }
}

// What must flow out of each deep cell: an over-full cell empties (a share of its excess a step,
// capped); one with room may fill (a share of its room a step) before it pushes back. Full is as full
// as the drops-only water just above the deep water (at most `rest_most` over at rest): resting any
// less full, the deep water would be lighter than the water on it, and the pool's top would turn
// over and over.
@compute @workgroup_size(256)
fn cs_deep_want(@builtin(global_invocation_id) gid: vec3<u32>) {
    let k = gid.x;
    if k >= fr.grid.x * fr.grid.y {
        return;
    }
    let c = cellp[k];
    if !(c.w > 0.5) {
        return;
    }
    let liquid = f32(atomicLoad(&rest[0]));
    var full = 1.0;
    if liquid > 0.0 {
        full = clamp(f32(atomicLoad(&rest[1])) / liquid, 1.0, 1.0 + fr.deep2.z);
    }
    let e = (c.w - 1.0) - full;
    var s = 0.0;
    if e > 0.0 {
        s = min(fr.mg.z * e, fr.mg.w) / fr.time.x;
    } else {
        s = fr.mg2.x * e / fr.time.x;
    }
    cellp[k] = vec4<f32>(c.x, c.y + s, c.z, c.w);
}

// ---- the pressure: red-black sweeps on a ladder of coarser grids ---------------------------------
//
// The grid core's (part 2a), with its numbers from the frame's: `st.n.x` is the pass's number (twice
// the level, plus the colour), from the step numbers' extra slots.

// Whether a cell of `level` is solved, from its `cellp` entry: level 0 keeps a flag in w; a coarser
// level keeps its floor there and is solved when its middle is above 0.
fn is_solved(c: vec4<f32>, level: u32) -> bool {
    if level == 0u {
        return c.w > 0.5;
    }
    return c.z > 0.0;
}

// The least a cell's answer may be: no pressure on level 0; on a coarser level, as far down as the
// fullest-drained of its four can still go (so a correction never asks any cell under it for a
// pressure below 0).
fn floor_of(c: vec4<f32>, level: u32) -> f32 {
    if level == 0u {
        return 0.0;
    }
    return c.w;
}

@compute @workgroup_size(16, 16)
fn cs_deep_sor(@builtin(global_invocation_id) id: vec3<u32>) {
    let level = st.n.x >> 1u;
    let lv = fr.lv[level];
    if id.x >= lv.y || id.y >= lv.z {
        return;
    }
    if ((id.x + id.y) & 1u) != (st.n.x & 1u) {
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
    var p = c.x + fr.mg.x * (sum / c.z - c.x);
    let least = floor_of(c, level);
    if !(p > least) {
        p = least;
    }
    cellp[k].x = p;
}

// Makes level `st.n.x >> 1` (1 or more) from the level under it: each of its cells is four of that
// level's (2 x 2), its faces the sums of theirs that cross to a neighbouring four, what it must make up
// the sum of what theirs still lack. Its own answer starts at 0.
@compute @workgroup_size(16, 16)
fn cs_deep_down(@builtin(global_invocation_id) id: vec3<u32>) {
    let level = st.n.x >> 1u;
    let lv = fr.lv[level];
    if id.x >= lv.y || id.y >= lv.z {
        return;
    }
    let f = fr.lv[level - 1u];
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
    // The faces are halved (the fours' middles are twice as far apart); the way out to the air is
    // not, but its middle is a quarter of itself further from the air.
    let air = max(diag - (faces.x + faces.y + faces.z + faces.w), 0.0);
    faces = 0.5 * faces;
    diag = faces.x + faces.y + faces.z + faces.w + air / (1.0 + 0.25 * air);
    let solved = any && diag > 1e-4;
    coef[k] = select(vec4<f32>(0.0), faces, solved);
    cellp[k] = vec4<f32>(0.0, select(0.0, lacks, solved), select(0.0, diag, solved), min(least, 0.0));
}

// Adds level `(st.n.x >> 1) + 1`'s answer to level `st.n.x >> 1`: each cell takes its four's.
@compute @workgroup_size(16, 16)
fn cs_deep_up(@builtin(global_invocation_id) id: vec3<u32>) {
    let level = st.n.x >> 1u;
    let lv = fr.lv[level];
    if id.x >= lv.y || id.y >= lv.z {
        return;
    }
    let k = lv.x + id.y * lv.y + id.x;
    let c = cellp[k];
    if !is_solved(c, level) {
        return;
    }
    let up = fr.lv[level + 1u];
    let e = cellp[up.x + (id.y >> 1u) * up.y + (id.x >> 1u)].x;
    var p = c.x + fr.mg.y * e;
    let least = floor_of(c, level);
    if !(p > least) {
        p = least;
    }
    cellp[k].x = p;
}

// ---- the push --------------------------------------------------------------------------------------

// The pressure's change of the speed across (`which` 0, on left faces) or down (1, on top faces) at
// `x`: the mean over the open faces of the 2 x 2 round it, weighted as they lie.
fn pressure_change(x: vec2<f32>, which: u32) -> f32 {
    let off = select(vec2<f32>(0.5, 0.0), vec2<f32>(0.0, 0.5), which == 0u);
    let b = base2(x, off);
    let f = clamp(x - off - vec2<f32>(b), vec2<f32>(0.0), vec2<f32>(1.0));
    var sum = 0.0;
    var all = 0.0;
    for (var a = 0; a < 2; a = a + 1) {
        for (var c = 0; c < 2; c = c + 1) {
            let cx = b.x + a;
            let cy = b.y + c;
            let k = node(cx, cy);
            let open = select(coef[k].z, coef[k].x, which == 0u);
            if open > 0.0 {
                var other = 0.0;
                if which == 0u && cx > 0 {
                    other = cellp[k - 1u].x;
                } else if which == 1u && cy > 0 {
                    other = cellp[k - fr.grid.x].x;
                }
                let w = bilinear(f, a, c);
                sum = sum - w * (cellp[k].x - other);
                all = all + w;
            }
        }
    }
    if all < 1e-4 {
        return 0.0;
    }
    return sum / all;
}

@compute @workgroup_size(256)
fn cs_deep_push(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= st.n.x {
        return;
    }
    let me = y_in[i];
    let x = me.xy;
    if !alive(x) {
        return;
    }
    // Its depth share, between the cell middles round it.
    let off = vec2<f32>(0.5, 0.5);
    let b = base2(x, off);
    let f = clamp(x - off - vec2<f32>(b), vec2<f32>(0.0), vec2<f32>(1.0));
    var w = 0.0;
    for (var a = 0; a < 2; a = a + 1) {
        for (var c = 0; c < 2; c = c + 1) {
            w = w + bilinear(f, a, c) * dfa[node(b.x + a, b.y + c)];
        }
    }
    // (Thin water, and the top of a pool, are the drops' own: nothing is written.)
    if !(w > 0.0) {
        return;
    }
    var shift = min(w, 1.0) * vec2<f32>(pressure_change(x, 0u), pressure_change(x, 1u)) * fr.time.x;
    let far = length(shift);
    if far > fr.deep.w {
        shift = shift * (fr.deep.w / far);
    }
    if !finite2(shift) {
        return;
    }
    let kind = u32(velb[i].z);
    y_in[i] = vec4<f32>(keep_off(x + shift, vec2<f32>(0.0), stops_at_edge(kind)).x, me.zw);
}

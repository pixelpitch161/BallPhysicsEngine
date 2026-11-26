// ---- trapped air: found under the water each frame and filled with air drops (part 2c) ----------
//
// On a frame's last step, from the deep part's sums (before `cs_deep_across` empties them): a cell
// that is not full (no drop reaches it, or they do not fill it), out of the solids, with full liquid
// over it and full liquid or a solid under it and to each side, within `fr.air.z` cells, is trapped
// air (`cs_air_find`): its lattice spots clear of the solids and of every drop are marked, so the
// air fills the pocket out to its water (air that did not reach the water would have open room
// beside it, and burst), and a pocket part filled is filled the rest of the way on the next frame. The spots' running sums over the cells, in blocks and then over the
// blocks (`cs_air_sum_a`, `cs_air_sum_b`; whole numbers, so every spot takes the same slot on every
// run), give each spot its slot among the `fr.dgrid.w` the host kept after the frame's drops. Once
// the step's drops are finished, each spot is born an air drop in its slot (`cs_air_born`), and the
// slots no spot took hold drops that left (`cs_air_dead`), let go two frames on.

// Per cell: its spots (bits 0 to 3), how many spots the cells before it in its block have, and the
// speed of the liquid round it (x and y, as bits).
@group(0) @binding(27) var<storage, read_write> airc: array<vec4<u32>>;
// Per block of cells: its spots, then how many spots the blocks before it have; after the blocks,
// every spot.
@group(0) @binding(28) var<storage, read_write> airb: array<u32>;

fn air_blocks() -> u32 {
    return (fr.grid.x * fr.grid.y + BLOCK - 1u) / BLOCK;
}

// Whether (x, y) is a cell (the last column and row of nodes are none).
fn is_cell(x: i32, y: i32) -> bool {
    return x >= 0 && y >= 0 && x < i32(fr.grid.x) - 1 && y < i32(fr.grid.y) - 1;
}

// What the cell at (x, y) is to air beside it: 2 full of liquid (the deep part's test: the drops in
// it full, and they and the solids covering at least half of it), 1 a solid (at least half solid, or
// past the course's edge when that is shut), 0 neither (empty or part full).
fn air_side(x: i32, y: i32) -> u32 {
    if !is_cell(x, y) {
        return select(0u, 1u, edges_shut());
    }
    let k = node(x, y);
    let q = vec2<f32>(f32(x) + 0.5, f32(y) + 0.5);
    let solid = wallc[k].z;
    let edge = edge_share(q);
    if solid >= 0.5 || edge >= 0.5 {
        return 1u;
    }
    let liquid = f32(atomicLoad(&sums[SUMS * k + 4u])) / S_M;
    if liquid <= 1e-4 {
        return 0u;
    }
    let dense = f32(atomicLoad(&sums[SUMS * k + 5u])) / S_M / liquid;
    let rho = liquid + 1.0 - (1.0 - solid) * (1.0 - edge);
    return select(0u, 2u, dense >= fr.deep.z && rho >= 0.5);
}

// Whether, looking from (x, y) along (dx, dy) past cells that are neither, full liquid comes first
// within the reach (or, unless `liquid_only`, a solid).
fn air_shut(x: i32, y: i32, dx: i32, dy: i32, liquid_only: bool) -> bool {
    let reach = i32(fr.air.z);
    for (var d = 1; d <= reach; d = d + 1) {
        let s = air_side(x + dx * d, y + dy * d);
        if s == 2u {
            return true;
        }
        if s == 1u {
            return !liquid_only;
        }
    }
    return false;
}

// How near a drop (any, air too) may be to a spot for air to be born there, in cells: a little
// under a spacing, so the air reaches the water round it as the water's own lattice would.
const AIR_CLEAR: f32 = 0.4;

// Whether (x, y) is a cell neither full nor solid with full liquid over it and full liquid or a
// solid under it and to each side within the reach. (Water over it, not a solid: air under a ledge
// or a lid is the course's, not trapped.)
fn air_trapped(x: i32, y: i32) -> bool {
    if !is_cell(x, y) || air_side(x, y) != 0u {
        return false;
    }
    return air_shut(x, y, 0, -1, true) && air_shut(x, y, 0, 1, false) && air_shut(x, y, -1, 0, false)
        && air_shut(x, y, 1, 0, false);
}

// Whether no drop is within `AIR_CLEAR` of `q` (where the step's drops would fall to).
fn air_clear(q: vec2<f32>) -> bool {
    let c = cell_of(q);
    for (var dy = -1; dy <= 1; dy = dy + 1) {
        let y = c.y + dy;
        if y < 0 || y >= i32(fr.cells.y) {
            continue;
        }
        let x0 = u32(max(c.x - 1, 0));
        let x1 = u32(min(c.x + 1, i32(fr.cells.x) - 1));
        let first_j = starts[u32(y) * fr.cells.x + x0];
        let last_j = starts[u32(y) * fr.cells.x + x1 + 1u];
        for (var j = first_j; j < last_j; j = j + 1u) {
            let r = q - y_in[j].xy;
            if dot(r, r) < AIR_CLEAR * AIR_CLEAR {
                return false;
            }
        }
    }
    return true;
}

// Whether a drop at `q` is clear of the solids and (when they stop it) the course's edges.
fn air_room(q: vec2<f32>) -> bool {
    if solid_at(q).z < fr.w.y {
        return false;
    }
    if edges_shut() {
        let lo = vec2<f32>(EDGE + 0.05);
        let hi = vec2<f32>(f32(fr.grid.x) - 1.0 - EDGE - 0.05, f32(fr.grid.y) - 1.0 - EDGE - 0.05);
        return all(q >= lo) && all(q <= hi);
    }
    return true;
}

@compute @workgroup_size(256)
fn cs_air_find(@builtin(global_invocation_id) gid: vec3<u32>) {
    let k = gid.x;
    if k >= fr.grid.x * fr.grid.y {
        return;
    }
    let x = i32(k % fr.grid.x);
    let y = i32(k / fr.grid.x);
    var spots = 0u;
    var v = vec2<f32>(0.0);
    if air_trapped(x, y) {
        for (var b = 0u; b < 4u; b = b + 1u) {
            let q = vec2<f32>(f32(x) + 0.25 + 0.5 * f32(b & 1u), f32(y) + 0.25 + 0.5 * f32(b >> 1u));
            if air_room(q) && air_clear(q) {
                spots = spots | (1u << b);
            }
        }
        // The liquid's speed round it: its faces' momentum over their mass, over the 3 x 3 cells
        // round it.
        var m = vec2<f32>(0.0);
        var p = vec2<f32>(0.0);
        for (var dy = -1; dy <= 1; dy = dy + 1) {
            for (var dx = -1; dx <= 1; dx = dx + 1) {
                if !is_cell(x + dx, y + dy) {
                    continue;
                }
                let at = SUMS * node(x + dx, y + dy);
                m = m + vec2<f32>(f32(atomicLoad(&sums[at])), f32(atomicLoad(&sums[at + 2u]))) / S_M;
                p = p + vec2<f32>(f32(atomicLoad(&sums[at + 1u])), f32(atomicLoad(&sums[at + 3u]))) / S_P;
            }
        }
        v = select(vec2<f32>(0.0), p / max(m, vec2<f32>(1e-6)), m > vec2<f32>(fr.mg2.y));
        if !finite2(v) {
            v = vec2<f32>(0.0);
        }
    }
    airc[k] = vec4<u32>(spots, 0u, bitcast<u32>(v.x), bitcast<u32>(v.y));
}

fn air_count(k: u32) -> u32 {
    if k >= fr.grid.x * fr.grid.y {
        return 0u;
    }
    return countOneBits(airc[k].x);
}

// The running sums within each block of 512 cells (a workgroup a block, each thread two cells)...
@compute @workgroup_size(256)
fn cs_air_sum_a(@builtin(workgroup_id) wid: vec3<u32>, @builtin(local_invocation_id) lid: vec3<u32>) {
    let b = wid.x;
    let t = lid.x;
    let c = b * BLOCK + 2u * t;
    let a = air_count(c);
    let n = air_count(c + 1u);
    part[t] = a + n;
    workgroupBarrier();
    for (var off = 1u; off < 256u; off = off * 2u) {
        var add = 0u;
        if t >= off {
            add = part[t - off];
        }
        workgroupBarrier();
        part[t] = part[t] + add;
        workgroupBarrier();
    }
    var before = 0u;
    if t > 0u {
        before = part[t - 1u];
    }
    let cells = fr.grid.x * fr.grid.y;
    if c < cells {
        airc[c].y = before;
    }
    if c + 1u < cells {
        airc[c + 1u].y = before + a;
    }
    if t == 255u {
        airb[b] = part[255];
    }
}

// ...and over the blocks (one workgroup), with every spot after the blocks.
@compute @workgroup_size(256)
fn cs_air_sum_b(@builtin(local_invocation_id) lid: vec3<u32>) {
    let t = lid.x;
    let all = air_blocks();
    let run = (all + 255u) / 256u;
    let a = min(t * run, all);
    let b = min(a + run, all);
    var sum = 0u;
    for (var k = a; k < b; k = k + 1u) {
        sum = sum + airb[k];
    }
    part[t] = sum;
    workgroupBarrier();
    for (var off = 1u; off < 256u; off = off * 2u) {
        var add = 0u;
        if t >= off {
            add = part[t - off];
        }
        workgroupBarrier();
        part[t] = part[t] + add;
        workgroupBarrier();
    }
    var base = 0u;
    if t > 0u {
        base = part[t - 1u];
    }
    for (var k = a; k < b; k = k + 1u) {
        let here = airb[k];
        airb[k] = base;
        base = base + here;
    }
    if t == 255u {
        airb[all] = part[255];
    }
}

// The kept slots no spot took (after the step's drops, `st.n.x` of them): drops that left.
@compute @workgroup_size(256)
fn cs_air_dead(@builtin(global_invocation_id) gid: vec3<u32>) {
    let t = gid.x;
    let slots = fr.dgrid.w;
    if t >= slots || t < min(airb[air_blocks()], slots) {
        return;
    }
    let i = st.n.x + t;
    xv[i] = vec4<f32>(-1e6, -1e6, 0.0, 0.0);
    nf[i] = vec4<f32>(0.0);
    id_nxt[i] = vec4<u32>(AIR | STOPS, fr.dgrid.z + t, 0u, 0u);
}

// Each trapped cell's spots born as air drops in their slots, at the liquid's speed round it.
@compute @workgroup_size(256)
fn cs_air_born(@builtin(global_invocation_id) gid: vec3<u32>) {
    let k = gid.x;
    if k >= fr.grid.x * fr.grid.y {
        return;
    }
    let c = airc[k];
    if c.x == 0u {
        return;
    }
    let x = f32(k % fr.grid.x);
    let y = f32(k / fr.grid.x);
    let v = vec2<f32>(bitcast<f32>(c.z), bitcast<f32>(c.w));
    var slot = airb[k / BLOCK] + c.y;
    for (var b = 0u; b < 4u; b = b + 1u) {
        if (c.x & (1u << b)) == 0u {
            continue;
        }
        if slot >= fr.dgrid.w {
            return;
        }
        let i = st.n.x + slot;
        xv[i] = vec4<f32>(x + 0.25 + 0.5 * f32(b & 1u), y + 0.25 + 0.5 * f32(b >> 1u), v);
        nf[i] = vec4<f32>(0.0, 0.0, 1.0, 0.0);
        id_nxt[i] = vec4<u32>(AIR | STOPS, fr.dgrid.z + slot, 0u, 0u);
        slot = slot + 1u;
    }
}

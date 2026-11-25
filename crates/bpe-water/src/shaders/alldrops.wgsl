// The all-drops engine trial (docs/superpowers/plans/water-mode/all-drops-trial.md): no grid
// pressure, every drop feels its neighbours directly.
//
// One liquid step: each drop falls and is moved to where it would be, kept off the solids, and
// its cell counted (`cs_predict`); the cells' running sums give every drop a sorted place, by cell
// and then by serial (`cs_sum_a/b/c`, `cs_scatter`, `cs_order`); each drop finds how over-full its
// neighbourhood is and changes its pressure multiplier, never above nothing (`cs_lambda`); it is
// moved by its own and its neighbours' changes (`cs_apply`); and its speed, the skin's pull, grip
// and drag and the pressure it keeps for the next step are worked out (`cs_finish`).
//
// Grid units: a layout cell is 1, four drops fill one, drops at rest are half a cell apart.

struct P {
    x: vec2<f32>,
    v: vec2<f32>,
    // Where it would be this step.
    xp: vec2<f32>,
    // The kept pressure's shift for the next step (cells).
    warm: vec2<f32>,
    // Which way the liquid lies from it (long at a surface, nothing inside).
    n: vec2<f32>,
    // Its kept pressure multiplier (at or below nothing).
    lam: f32,
    // How full its neighbourhood is (1 is full).
    rho: f32,
    kind: u32,
    serial: u32,
    colour: u32,
    spare: u32,
};

// A drop as the grid core lays it out (`Drop` in common.wgsl).
struct DropOut {
    x: vec2<f32>,
    v: vec2<f32>,
    c: vec4<f32>,
    e: f32,
    kind: u32,
    serial: u32,
    colour: u32,
};

struct Feel {
    // gravity x, the skin's pull between drops (cells a second squared), its pull along the
    // surface's bend, the pull toward a near face (cells a second squared)
    a: vec4<f32>,
    // damping (a second), drag along a face (a second), 0, 0
    b: vec4<f32>,
};

struct St {
    // the layout's columns and rows of nodes, drops, bit 0 the course's edges are shut
    grid: vec4<u32>,
    // the neighbour cells across and down, how many, how many blocks of running sums
    cells: vec4<u32>,
    // dt, gravity x and y (cells a second squared), the speed cap (cells a second)
    time: vec4<f32>,
    // the reach (cells), the density at rest, the softening of a correction, its share taken
    k: vec4<f32>,
    // how much of the pressure is kept, how far off a face a drop stays, a sub-shift's length,
    // the solids' share of the density
    w: vec4<f32>,
    // the share of the neighbours' mean speed taken a step, how far from a face drag reaches,
    // the most a sweep moves a drop, the most the kept pressure moves it
    s: vec4<f32>,
    feels: array<Feel, 16>,
    // the solids' share of a drop's neighbourhood by its distance from the face (`wall_table`)
    wall: array<vec4<f32>, 17>,
};

const STOPS: u32 = 256u;
const EDGE: f32 = 2.0;
const BLOCK: u32 = 512u;
// The most drops of one cell put in the order of their serials (more keep the order they came).
const MOST_SORTED: u32 = 192u;
const PI: f32 = 3.14159265;

@group(0) @binding(0) var<uniform> st: St;
// The drops as the last step left them (sorted then), and as this step sorts them.
@group(0) @binding(1) var<storage, read_write> cur: array<P>;
@group(0) @binding(2) var<storage, read_write> nxt: array<P>;
// Per cell (and one more, for drops that left the course): how many drops.
@group(0) @binding(3) var<storage, read_write> counts: array<atomic<u32>>;
// Per cell: where its drops start in the sorted order (one more entry: the end).
@group(0) @binding(4) var<storage, read_write> starts: array<u32>;
@group(0) @binding(5) var<storage, read_write> blocks: array<u32>;
@group(0) @binding(6) var<storage, read_write> idx: array<u32>;
// Per drop, sorted: its place this step (x, y) and which way the liquid lay from it last step.
@group(0) @binding(7) var<storage, read_write> ya: array<vec4<f32>>;
@group(0) @binding(8) var<storage, read_write> yb: array<vec4<f32>>;
// Per drop, sorted: this sweep's change of its multiplier, the multiplier, how full, spare.
@group(0) @binding(9) var<storage, read_write> lamb: array<vec4<f32>>;
// Per drop, sorted: its new speed.
@group(0) @binding(10) var<storage, read_write> velb: array<vec4<f32>>;
// Per layout node: how far it is from the nearest solid's face, in cells (below 0 inside).
@group(0) @binding(11) var<storage, read_write> solid: array<f32>;
@group(0) @binding(12) var<storage, read_write> outd: array<DropOut>;

fn finite2(v: vec2<f32>) -> bool {
    return abs(v.x) < 1e30 && abs(v.y) < 1e30;
}

// Whether a drop at `x` is in the course (one that left takes no part).
fn alive(x: vec2<f32>) -> bool {
    return finite2(x) && x.x >= 0.0 && x.y >= 0.0 && x.x <= f32(st.grid.x) - 1.0 && x.y <= f32(st.grid.y) - 1.0;
}

fn cell_of(x: vec2<f32>) -> vec2<i32> {
    let c = vec2<i32>(floor(x / st.k.x));
    return clamp(c, vec2<i32>(0), vec2<i32>(i32(st.cells.x) - 1, i32(st.cells.y) - 1));
}

// The bin a drop at `x` is sorted into: its cell, or the last bin when it left the course.
fn bin_of(x: vec2<f32>) -> u32 {
    if !alive(x) {
        return st.cells.z;
    }
    let c = cell_of(x);
    return u32(c.y) * st.cells.x + u32(c.x);
}

fn node(x: i32, y: i32) -> u32 {
    return u32(y) * st.grid.x + u32(x);
}

// How far `q` is from the nearest solid's face (cells, below 0 inside) and which way is out.
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
    let off = st.w.y;
    if s.z < off {
        x = x + s.xy * min(off - s.z, 4.0);
        let vn = dot(v, s.xy);
        if vn < 0.0 {
            v = v - vn * s.xy;
        }
    }
    if stops {
        let lo = vec2<f32>(EDGE);
        let hi = vec2<f32>(f32(st.grid.x) - 1.0 - EDGE, f32(st.grid.y) - 1.0 - EDGE);
        if x.x < lo.x + 0.05 { x.x = lo.x + 0.05; v.x = max(v.x, 0.0); }
        if x.x > hi.x - 0.05 { x.x = hi.x - 0.05; v.x = min(v.x, 0.0); }
        if x.y < lo.y + 0.05 { x.y = lo.y + 0.05; v.y = max(v.y, 0.0); }
        if x.y > hi.y - 0.05 { x.y = hi.y - 0.05; v.y = min(v.y, 0.0); }
    }
    return Kept(x, v);
}

fn stops_at_edge(kind: u32) -> bool {
    return (kind & STOPS) != 0u && (st.grid.w & 1u) != 0u;
}

// The density a drop `far` away (cells) adds (the spiky kernel in the plane: steep close in, so
// two drops on top of each other count as too many and are parted; a flat-topped kernel lets
// drops pair up into strings).
fn weight(far: f32) -> f32 {
    let h = st.k.x;
    let t = max(1.0 - far / h, 0.0);
    return 10.0 / (PI * h * h) * t * t * t;
}

// The slope of the density a drop at `r` from this one adds, as this one moves. `far` is `r`'s length,
// above 0.
fn slope(r: vec2<f32>, far: f32) -> vec2<f32> {
    let h = st.k.x;
    let t = max(1.0 - far / h, 0.0);
    return r * (-30.0 / (PI * h * h * h) * t * t / far);
}

// The way from one drop to another that is on top of it: each its own, by their serials.
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
    let a = select(st.wall[k / 2u].xy, st.wall[k / 2u].zw, (k & 1u) == 1u);
    let k1 = k + 1u;
    let b = select(st.wall[k1 / 2u].xy, st.wall[k1 / 2u].zw, (k1 & 1u) == 1u);
    return mix(a, b, f);
}

// What the solids add to a drop at `x`: (the slope of the density x and y, the density).
fn wall_density(x: vec2<f32>) -> vec3<f32> {
    let s = solid_at(x);
    let h = st.k.x;
    if s.z >= h || st.w.w <= 0.0 {
        return vec3<f32>(0.0);
    }
    let t = wall_table(s.z / h);
    let k = st.k.y * st.w.w;
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

// ---- sorting --------------------------------------------------------------------------------

@compute @workgroup_size(256)
fn cs_zero(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x <= st.cells.z {
        atomicStore(&counts[id.x], 0u);
    }
}

@compute @workgroup_size(256)
fn cs_predict(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= st.grid.z {
        return;
    }
    let p = cur[i];
    if !alive(p.x) {
        cur[i].xp = p.x;
        atomicAdd(&counts[st.cells.z], 1u);
        return;
    }
    let feel = st.feels[p.kind & 15u];
    let dt = st.time.x;
    var v = p.v + vec2<f32>(st.time.y, st.time.z) * feel.a.x * dt;
    v = v * exp(-feel.b.x * dt);
    let speed = length(v);
    if speed > st.time.w {
        v = v * (st.time.w / speed);
    }
    if !finite2(v) {
        v = vec2<f32>(0.0);
    }
    // Moved in short sub-moves, kept off the solids at each: a fast drop cannot jump a thin wall.
    let stops = stops_at_edge(p.kind);
    let moves = clamp(u32(ceil(length(v) * dt / max(st.w.z, 0.05))), 1u, 24u);
    let h = dt / f32(moves);
    var x = p.x;
    for (var m = 0u; m < moves; m = m + 1u) {
        let kept = keep_off(x + v * h, v, stops);
        x = kept.x;
        v = kept.v;
    }
    // The pressure kept from the last step.
    var warm = p.warm;
    if !finite2(warm) {
        warm = vec2<f32>(0.0);
    }
    x = keep_off(x + warm, v, stops).x;
    cur[i].xp = x;
    atomicAdd(&counts[bin_of(x)], 1u);
}

// The running sums of the cells' counts, in blocks: within each block...
@compute @workgroup_size(256)
fn cs_sum_a(@builtin(global_invocation_id) id: vec3<u32>) {
    let b = id.x;
    if b >= st.cells.w {
        return;
    }
    var run = 0u;
    for (var k = 0u; k < BLOCK; k = k + 1u) {
        let c = b * BLOCK + k;
        if c <= st.cells.z {
            starts[c] = run;
            run = run + atomicLoad(&counts[c]);
        }
    }
    blocks[b] = run;
}

// ...over the blocks...
@compute @workgroup_size(1)
fn cs_sum_b(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x != 0u {
        return;
    }
    var run = 0u;
    for (var b = 0u; b < st.cells.w; b = b + 1u) {
        let t = blocks[b];
        blocks[b] = run;
        run = run + t;
    }
}

// ...and each cell's start. The counts are emptied for the scatter.
@compute @workgroup_size(256)
fn cs_sum_c(@builtin(global_invocation_id) id: vec3<u32>) {
    let c = id.x;
    if c > st.cells.z {
        return;
    }
    starts[c] = starts[c] + blocks[c / BLOCK];
    atomicStore(&counts[c], 0u);
    if c == st.cells.z {
        starts[c + 1u] = st.grid.z;
    }
}

@compute @workgroup_size(256)
fn cs_scatter(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= st.grid.z {
        return;
    }
    let c = bin_of(cur[i].xp);
    let s = starts[c] + atomicAdd(&counts[c], 1u);
    idx[s] = i;
}

// Each drop to its sorted place: by cell, and within a cell by serial, so every sum over
// neighbours adds in the same order on every run.
@compute @workgroup_size(256)
fn cs_order(@builtin(global_invocation_id) id: vec3<u32>) {
    let s = id.x;
    if s >= st.grid.z {
        return;
    }
    let i = idx[s];
    let p = cur[i];
    let c = bin_of(p.xp);
    let a = starts[c];
    let b = starts[c + 1u];
    var dest = s;
    if c < st.cells.z && b - a <= MOST_SORTED {
        var rank = 0u;
        for (var t = a; t < b; t = t + 1u) {
            let j = idx[t];
            if j != i {
                let other = cur[j].serial;
                if other < p.serial || (other == p.serial && j < i) {
                    rank = rank + 1u;
                }
            }
        }
        dest = a + rank;
    }
    nxt[dest] = p;
    ya[dest] = vec4<f32>(p.xp, p.n);
    lamb[dest] = vec4<f32>(0.0, min(p.lam, 0.0), p.rho, 0.0);
}

// ---- the pressure ---------------------------------------------------------------------------

@compute @workgroup_size(256)
fn cs_lambda(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= st.grid.z {
        return;
    }
    let xi = ya[i].xy;
    if !alive(xi) {
        lamb[i] = vec4<f32>(0.0);
        return;
    }
    let h = st.k.x;
    let c = cell_of(xi);
    var rho = 0.0;
    var sum = vec2<f32>(0.0);
    var sq = 0.0;
    for (var dy = -1; dy <= 1; dy = dy + 1) {
        let y = c.y + dy;
        if y < 0 || y >= i32(st.cells.y) {
            continue;
        }
        let x0 = u32(max(c.x - 1, 0));
        let x1 = u32(min(c.x + 1, i32(st.cells.x) - 1));
        let first = starts[u32(y) * st.cells.x + x0];
        let last = starts[u32(y) * st.cells.x + x1 + 1u];
        for (var j = first; j < last; j = j + 1u) {
            let r = xi - ya[j].xy;
            let r2 = dot(r, r);
            if r2 >= h * h {
                continue;
            }
            let far = sqrt(r2);
            rho = rho + weight(far);
            if j == i {
                continue;
            }
            var g = vec2<f32>(0.0);
            if r2 > 1e-10 {
                g = slope(r, far);
            } else {
                g = any_way(i, j) * (-30.0 / (PI * h * h * h));
            }
            sum = sum + g;
            sq = sq + dot(g, g);
        }
    }
    let wall = wall_density(xi);
    rho = rho + wall.z;
    sum = sum + wall.xy;
    let rho0 = st.k.y;
    var over = rho / rho0 - 1.0;
    // Liquid that is being pulled thin from inside (stretched as it falls) is drawn back together
    // where it is still nearly full: the air's own pressure does this to real water, and without
    // it a falling column opens holes. A drop at the surface or in spray (less than `low` full)
    // is never pulled, so nothing clumps there.
    let tens = st.wall[16].z;
    var top = 0.0;
    if tens > 0.0 {
        top = 1e30;
        if over < 0.0 {
            let low = st.wall[16].w;
            over = over * tens * smoothstep(low, 0.5 * (low + 1.0), rho / rho0);
        }
    }
    let stiff = (dot(sum, sum) + sq) / (rho0 * rho0);
    let was = lamb[i].y;
    var now = min(was - st.k.w * over / (stiff + st.k.z), top);
    if !(abs(now) < 1e30) {
        now = 0.0;
    }
    lamb[i] = vec4<f32>(now - was, now, rho / rho0, 0.0);
}

@compute @workgroup_size(256)
fn cs_apply(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= st.grid.z {
        return;
    }
    let me = ya[i];
    let xi = me.xy;
    let p = nxt[i];
    if !alive(xi) {
        yb[i] = me;
        velb[i] = vec4<f32>(0.0);
        return;
    }
    let h = st.k.x;
    let c = cell_of(xi);
    let mine = lamb[i].x;
    var shift = vec2<f32>(0.0);
    for (var dy = -1; dy <= 1; dy = dy + 1) {
        let y = c.y + dy;
        if y < 0 || y >= i32(st.cells.y) {
            continue;
        }
        let x0 = u32(max(c.x - 1, 0));
        let x1 = u32(min(c.x + 1, i32(st.cells.x) - 1));
        let first = starts[u32(y) * st.cells.x + x0];
        let last = starts[u32(y) * st.cells.x + x1 + 1u];
        for (var j = first; j < last; j = j + 1u) {
            if j == i {
                continue;
            }
            let r = xi - ya[j].xy;
            let r2 = dot(r, r);
            if r2 >= h * h {
                continue;
            }
            var g = vec2<f32>(0.0);
            if r2 > 1e-10 {
                g = slope(r, sqrt(r2));
            } else {
                g = any_way(i, j) * (-30.0 / (PI * h * h * h));
            }
            shift = shift + (mine + lamb[j].x) * g;
        }
    }
    shift = (shift + mine * wall_density(xi).xy) / st.k.y;
    let far = length(shift);
    if far > st.s.z {
        shift = shift * (st.s.z / far);
    }
    if !finite2(shift) {
        shift = vec2<f32>(0.0);
    }
    let x = keep_off(xi + shift, vec2<f32>(0.0), stops_at_edge(p.kind)).x;
    yb[i] = vec4<f32>(x, me.zw);
    velb[i] = vec4<f32>((x - p.x) / st.time.x, 0.0, 0.0);
}

// ---- speeds, the skin, grip and drag ----------------------------------------------------------

@compute @workgroup_size(256)
fn cs_finish(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= st.grid.z {
        return;
    }
    let me = ya[i];
    let xi = me.xy;
    var p = nxt[i];
    if !alive(xi) {
        p.x = xi;
        p.v = vec2<f32>(0.0);
        p.warm = vec2<f32>(0.0);
        p.lam = 0.0;
        nxt[i] = p;
        return;
    }
    let feel = st.feels[p.kind & 15u];
    let dt = st.time.x;
    let h = st.k.x;
    let rho0 = st.k.y;
    let c = cell_of(xi);
    let vi = velb[i].xy;
    let li = lamb[i];
    var even = vec2<f32>(0.0);
    var lies = vec2<f32>(0.0);
    var pull = vec2<f32>(0.0);
    var bend = vec2<f32>(0.0);
    var warm = vec2<f32>(0.0);
    for (var dy = -1; dy <= 1; dy = dy + 1) {
        let y = c.y + dy;
        if y < 0 || y >= i32(st.cells.y) {
            continue;
        }
        let x0 = u32(max(c.x - 1, 0));
        let x1 = u32(min(c.x + 1, i32(st.cells.x) - 1));
        let first = starts[u32(y) * st.cells.x + x0];
        let last = starts[u32(y) * st.cells.x + x1 + 1u];
        for (var j = first; j < last; j = j + 1u) {
            if j == i {
                continue;
            }
            let other = ya[j];
            let r = xi - other.xy;
            let r2 = dot(r, r);
            if r2 >= h * h {
                continue;
            }
            let lj = lamb[j];
            var far = sqrt(r2);
            var way = vec2<f32>(0.0);
            if r2 > 1e-10 {
                way = r / far;
            } else {
                way = -any_way(i, j);
                far = 1e-5;
            }
            let g = slope(way * far, far);
            even = even + (velb[j].xy - vi) * weight(far);
            // (Which way the liquid lies, by a smooth kernel: by the steep one it is mostly the
            // nearest drop's way, and the bend's pull turns the surface ragged.)
            let t = 1.0 - r2 / (h * h);
            lies = lies - way * (far * 24.0 / (PI * h * h * h * h) * t * t);
            // (Drops with little liquid round them pull harder: a surface's drops have fewer
            // neighbours to be pulled by.)
            let thin = 2.0 / max(li.z + lj.z, 0.5);
            // (Inside the liquid the pull between drops is only `feel.b.z` of itself: all of it
            // there squeezes the liquid tighter than it is born, and a thick pour rings.)
            let inside = mix(1.0, feel.b.z, smoothstep(0.8, 1.0, 0.5 * (li.z + lj.z)));
            pull = pull + inside * thin * skin(far / h) * way;
            bend = bend + thin * (me.zw - other.zw);
            warm = warm + (li.y + lj.y) * g;
        }
    }
    var v = vi + st.s.x * even / rho0;
    var a = -feel.a.y * pull - feel.a.z * bend;
    let s = solid_at(xi);
    if s.z < h {
        // Grip: a pull toward a near face, none at the face itself.
        let off = st.w.y;
        let t = clamp((s.z - off) / max(h - off, 1e-3), 0.0, 1.0);
        a = a - feel.a.w * 4.0 * t * (1.0 - t) * s.xy;
        warm = warm + li.y * wall_density(xi).xy;
    }
    // The skin's and the grip's change of speed in a step is held to a share of the cap.
    var dv = a * dt;
    let most = 0.25 * st.time.w;
    let big = length(dv);
    if big > most {
        dv = dv * (most / big);
    }
    v = v + dv;
    if s.z < st.s.y {
        // Drag: speed along a near face is lost.
        let near = 1.0 - smoothstep(st.w.y, st.s.y, s.z);
        let along = v - dot(v, s.xy) * s.xy;
        v = v - along * (1.0 - exp(-feel.b.y * near * dt));
    }
    if !finite2(v) {
        v = vec2<f32>(0.0);
    }
    warm = warm * (st.w.x / rho0);
    let reach = length(warm);
    if reach > st.s.w {
        warm = warm * (st.s.w / reach);
    }
    if !finite2(warm) {
        warm = vec2<f32>(0.0);
    }
    p.x = xi;
    p.v = v;
    p.xp = xi;
    p.warm = warm;
    p.n = lies * (h / 4.0);
    p.lam = st.w.x * li.y;
    p.rho = li.z;
    nxt[i] = p;
}

// The drops in the grid core's layout (its `c.x` holds how full the drop's neighbourhood is).
@compute @workgroup_size(256)
fn cs_export(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= st.grid.z {
        return;
    }
    let p = cur[i];
    outd[i] = DropOut(p.x, p.v, vec4<f32>(p.rho / st.k.y, p.lam, p.n), 0.0, p.kind, p.serial, p.colour);
}

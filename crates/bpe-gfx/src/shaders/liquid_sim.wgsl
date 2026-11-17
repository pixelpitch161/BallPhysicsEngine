// The Liquid thing's drops (liquid part 1): a weakly compressible MLS-MPM liquid in grid units
// (one grid cell is 1). Each liquid step: births appended; the walls band put on the grid when the
// walls moved; `substeps` times: the grid cleared, the drops' mass, momentum and volume scattered to
// it in fixed point (integer atomics, so the order drops add in can't change the sum), the grid's
// speeds worked out and held off the walls, the speeds gathered back to the drops (APIC), the drops
// kept off the walls, the balls and the edges and moved. Every MIX_EVERY-th step the drops are put
// into bins by cell and each one mixes its colour with its neighbours' (fixed point again). Last,
// the living drops are packed back in order by an integer prefix sum. See liquid_sim/mod.rs.

struct Drop {
    x: vec2<f32>,
    v: vec2<f32>,
    // The APIC matrix: xx, xy, yx, yy.
    c: vec4<f32>,
    // rgb (linear light), strength
    colour: vec4<f32>,
    j: f32,
    // the liquid's index (bits 0..7), STOPS at the edges (bit 8)
    kind: u32,
    serial: u32,
    kick: f32,
    // weight and viscosity offsets from its liquid's (mixing moves them)
    mat: vec2<f32>,
    spare: vec2<f32>,
};

struct Liquid {
    // stiffness (E for weight 1), surface pull, tear, damping (per second)
    a: vec4<f32>,
    // viscosity (cells²/s), weight, gravity ×, balls push it (0 or 1)
    b: vec4<f32>,
    // mix style, 0, 0, cut line (cells: drops above it go; -1e30 none)
    c: vec4<f32>,
    // serial floor (drops born before it go), 0, 0, 0
    d: vec4<u32>,
    // with the balls: push scale (1 / the ball's density to it, 0 pushes none), drag (per
    // second), carry, sticks
    e: vec4<f32>,
    // ink rgb, how much ink
    f: vec4<f32>,
    // splash size, splashes from (cells/s), wakes, balls take its colour
    g: vec4<f32>,
};

struct Step {
    // cols, rows, substeps, liquids
    grid: vec4<u32>,
    // dt a substep, gravity x and y (cells/s²), speed cap (cells/s)
    time: vec4<f32>,
    // births, room (drops the buffers hold), flags' half (where the local prefix sums go), cells
    counts: vec4<u32>,
    // mixing this step (0/1), put the walls (0/1), balls (count), edges block (0/1)
    flags: vec4<u32>,
    // viscosity cap, dt a mixing, separating push (cells a mixing), lift (cells a mixing)
    more: vec4<f32>,
    // the walls band: where its entries start in `words`, how many; balls: where they start, where
    // their cells' starts are
    words_at: vec4<u32>,
    // the ball grid: cols, rows, cell (grid cells, as bits), where the cells' items are
    ball_grid: vec4<u32>,
    // what the balls feel: recorded (0/1), ring points, 0, 0
    feel: vec4<u32>,
    // Paint: each band's basis (blue, green, red) and its way back (r, g, b rows over 8 bands)
    paint: array<vec4<f32>, 8>,
    back: array<vec4<f32>, 8>,
    liquids: array<Liquid, 16>,
    // 16 × 16 meeting rules, 4 to a vec4: mode (bits 0..1: 0 mix, 1 layer) | style
    // << 2 (3 bits) | when << 5 (2 bits) | rate × 65535 << 16
    pairs: array<vec4<u32>, 64>,
};

const WG: u32 = 256u;
const S_M: f32 = 1048576.0;
const S_P: f32 = 4096.0;
const LIMIT: f32 = 33554432.0;
const STOPS: u32 = 256u;
const FEEL: u32 = 32u;
const S_PUSH: f32 = 64.0;
const S_TOUCH: f32 = 256.0;
const BALL_WORDS: u32 = 6u;

const STYLE_PAINT: u32 = 0u;
const STYLE_LIGHT: u32 = 1u;
const STYLE_BLEND: u32 = 2u;
const STYLE_MARBLE: u32 = 3u;
const STYLE_KEEP_APART: u32 = 4u;

const WHEN_ALWAYS: u32 = 0u;
const WHEN_STIRRED: u32 = 1u;
const WHEN_HITS: u32 = 2u;

@group(0) @binding(0) var<uniform> st: Step;
@group(0) @binding(1) var<storage, read_write> drops: array<Drop>;
@group(0) @binding(2) var<storage, read> births: array<Drop>;
// [0..3] the draw's arguments (4, count, 0, 0), [4] the drops being worked on, [5] what a scan
// covers
@group(0) @binding(3) var<storage, read_write> counts: array<u32>;
// per node: mass, momentum x, momentum y, volume (fixed point); after the grid pass, speed x and y
// as float bits in the middle two
@group(0) @binding(4) var<storage, read_write> grid: array<atomic<i32>>;
// per node: (normal xy, distance, in the band), (wall speed xy, 0, 0)
@group(0) @binding(5) var<storage, read_write> solid: array<vec4<f32>>;
@group(0) @binding(6) var<storage, read> words: array<u32>;
@group(0) @binding(7) var<storage, read_write> flags: array<atomic<u32>>;
@group(0) @binding(8) var<storage, read_write> sums: array<u32>;
@group(0) @binding(9) var<storage, read_write> items: array<u32>;
@group(0) @binding(10) var<storage, read_write> spare: array<Drop>;
@group(0) @binding(11) var<storage, read_write> mixout: array<vec4<f32>>;
// per ball FEEL words: push x y, touching mass, colour × mass (rgb), drag × mass, carry × mass,
// wet ring points, flow x y (f32 bits), take × mass, 4 spare, contacts with each liquid (16)
@group(0) @binding(12) var<storage, read_write> feel: array<atomic<i32>>;

var<workgroup> shared_sums: array<u32, 256>;

fn wf(i: u32) -> f32 {
    return bitcast<f32>(words[i]);
}

fn finite2(v: vec2<f32>) -> bool {
    // NaN fails every comparison; infinities fail the bound.
    return abs(v.x) < 1e30 && abs(v.y) < 1e30;
}

fn node(x: i32, y: i32) -> u32 {
    return u32(y) * st.grid.x + u32(x);
}

fn fixed(x: f32, scale: f32) -> i32 {
    return i32(clamp(x * scale, -LIMIT, LIMIT));
}

fn liquid_of(d: Drop) -> u32 {
    return min(d.kind & 255u, 15u);
}

// ---- births and walls ----------------------------------------------------------------------

@compute @workgroup_size(256)
fn cs_births(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    // Only as many as there is room for (cs_births_done counts the same).
    if i >= st.counts.x || counts[1] + i >= st.counts.y {
        return;
    }
    drops[counts[1] + i] = births[i];
}

@compute @workgroup_size(1)
fn cs_births_done() {
    counts[0] = 4u;
    counts[1] = min(counts[1] + st.counts.x, st.counts.y);
    counts[2] = 0u;
    counts[3] = 0u;
    counts[4] = counts[1];
    counts[7] = 0u;
}

// Counts the substeps taken: the balls move on along their speed in each.
@compute @workgroup_size(1)
fn cs_tick() {
    counts[7] = counts[7] + 1u;
}

// Where ball `at` is in this substep: where it was at the liquid step's start, moved on along
// its speed for the substeps taken so far.
fn ball_now(at: u32) -> vec2<f32> {
    let x = vec2<f32>(wf(at), wf(at + 1u));
    let v = vec2<f32>(wf(at + 2u), wf(at + 3u));
    return x + v * (f32(counts[7]) * st.time.x);
}

@compute @workgroup_size(256)
fn cs_clear_solid(@builtin(global_invocation_id) id: vec3<u32>) {
    let k = id.x;
    if k >= st.counts.w {
        return;
    }
    solid[2u * k] = vec4<f32>(0.0);
    solid[2u * k + 1u] = vec4<f32>(0.0);
}

// Each band entry: node, normal xy, distance, wall speed xy (6 words).
@compute @workgroup_size(256)
fn cs_put_solid(@builtin(global_invocation_id) id: vec3<u32>) {
    let e = id.x;
    if e >= st.words_at.y {
        return;
    }
    let at = st.words_at.x + e * 7u;
    let k = words[at];
    if k >= st.counts.w {
        return;
    }
    solid[2u * k] = vec4<f32>(wf(at + 1u), wf(at + 2u), wf(at + 3u), 1.0);
    solid[2u * k + 1u] = vec4<f32>(wf(at + 4u), wf(at + 5u), wf(at + 6u), 0.0);
}

// The face of the wall at a node (`s`, `t`: its two solid entries) for a point `line_d` from the
// wall's line along its normal: the normal pointing off the face towards the point's side, and
// how far the point is from the face (below 0 inside the wall). A filled wall (half below 0) has
// one side; a hollow one keeps a point on whichever side of its line it is.
fn wall_face(s: vec4<f32>, t: vec4<f32>, line_d: f32) -> vec3<f32> {
    if t.z < 0.0 {
        return vec3<f32>(s.xy, line_d);
    }
    let side = select(-1.0, 1.0, line_d >= 0.0);
    return vec3<f32>(s.xy * side, abs(line_d) - t.z);
}

// ---- one substep ---------------------------------------------------------------------------

@compute @workgroup_size(256)
fn cs_clear_grid(@builtin(global_invocation_id) id: vec3<u32>) {
    let k = id.x;
    if k >= st.counts.w {
        return;
    }
    for (var c = 0u; c < 4u; c = c + 1u) {
        atomicStore(&grid[4u * k + c], 0);
    }
}

fn weights(fx: vec2<f32>) -> array<vec2<f32>, 3> {
    return array<vec2<f32>, 3>(
        0.5 * (1.5 - fx) * (1.5 - fx),
        0.75 - (fx - 1.0) * (fx - 1.0),
        0.5 * (fx - 0.5) * (fx - 0.5),
    );
}

// Where the drop's 3 × 3 nodes start (kept inside the grid).
fn base_of(x: vec2<f32>) -> vec2<i32> {
    let b = vec2<i32>(floor(x - 0.5));
    return clamp(b, vec2<i32>(0), vec2<i32>(i32(st.grid.x) - 3, i32(st.grid.y) - 3));
}

@compute @workgroup_size(256)
fn cs_p2g(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= counts[4] {
        return;
    }
    let p = drops[i];
    if !finite2(p.x) || !finite2(p.v) {
        return;
    }
    let lq = st.liquids[liquid_of(p)];
    let weight = max(lq.b.y + p.mat.x, 0.05);
    let nu = clamp(lq.b.x + p.mat.y, 0.0, st.more.x);
    let vol = 0.25;
    let mass = weight * vol;
    let j = p.j;
    let stiff = lq.a.x * weight;
    var pressure = 0.0;
    if j < 1.0 {
        pressure = stiff * (1.0 - j);
    } else if j < 1.0 + lq.a.z {
        pressure = -stiff * lq.a.y * (j - 1.0);
    }
    let dt = st.time.x;
    // σ = −p I + μ (C + Cᵀ), μ = ν ρ; affine = −dt · vol · 4 · σ + m C.
    let mu = nu * weight;
    let sxx = -pressure + mu * 2.0 * p.c.x;
    let sxy = mu * (p.c.y + p.c.z);
    let syy = -pressure + mu * 2.0 * p.c.w;
    let k = -dt * vol * 4.0;
    let axx = k * sxx + mass * p.c.x;
    let axy = k * sxy + mass * p.c.y;
    let ayx = k * sxy + mass * p.c.z;
    let ayy = k * syy + mass * p.c.w;
    let g = vec2<f32>(st.time.y, st.time.z) * lq.b.z;
    let mv = mass * (p.v + g * dt);
    let base = base_of(p.x);
    let fx = p.x - vec2<f32>(base);
    var w = weights(fx);
    for (var a = 0; a < 3; a = a + 1) {
        for (var b = 0; b < 3; b = b + 1) {
            let wt = w[a].x * w[b].y;
            let dpos = vec2<f32>(f32(a), f32(b)) - fx;
            let mom = wt * (mv + vec2<f32>(axx * dpos.x + axy * dpos.y, ayx * dpos.x + ayy * dpos.y));
            let n = node(base.x + a, base.y + b);
            atomicAdd(&grid[4u * n], fixed(wt * mass, S_M));
            atomicAdd(&grid[4u * n + 1u], fixed(mom.x, S_P));
            atomicAdd(&grid[4u * n + 2u], fixed(mom.y, S_P));
            atomicAdd(&grid[4u * n + 3u], fixed(wt * vol, S_M));
        }
    }
}

@compute @workgroup_size(256)
fn cs_grid(@builtin(global_invocation_id) id: vec3<u32>) {
    let k = id.x;
    if k >= st.counts.w {
        return;
    }
    let m = f32(atomicLoad(&grid[4u * k])) / S_M;
    var v = vec2<f32>(0.0);
    if m > 1e-6 {
        v = vec2<f32>(f32(atomicLoad(&grid[4u * k + 1u])), f32(atomicLoad(&grid[4u * k + 2u]))) / (S_P * m);
        let s = solid[2u * k];
        let t = solid[2u * k + 1u];
        let f = wall_face(s, t, s.z);
        if s.w > 0.5 && f.z < 0.75 {
            let vw = t.xy;
            var vr = v - vw;
            let vn = dot(vr, f.xy);
            if vn < 0.0 {
                vr = vr - vn * f.xy;
            }
            v = vr + vw;
        }
        if st.flags.w == 1u {
            let cols = i32(st.grid.x);
            let rows = i32(st.grid.y);
            let x = i32(k % st.grid.x);
            let y = i32(k / st.grid.x);
            if x <= 2 { v.x = max(v.x, 0.0); }
            if x >= cols - 2 { v.x = min(v.x, 0.0); }
            if y <= 2 { v.y = max(v.y, 0.0); }
            if y >= rows - 2 { v.y = min(v.y, 0.0); }
        }
    }
    atomicStore(&grid[4u * k + 1u], bitcast<i32>(v.x));
    atomicStore(&grid[4u * k + 2u], bitcast<i32>(v.y));
}

// The balls near `x`: their index range in the ball grid's cell.
fn ball_cell(x: vec2<f32>) -> vec2<u32> {
    let cols = st.ball_grid.x;
    let rows = st.ball_grid.y;
    let cell = bitcast<f32>(st.ball_grid.z);
    let c = u32(clamp(floor(x.x / cell), 0.0, f32(cols - 1u)));
    let r = u32(clamp(floor(x.y / cell), 0.0, f32(rows - 1u)));
    let k = r * cols + c;
    let starts = st.words_at.w;
    return vec2<u32>(words[starts + k], words[starts + k + 1u]);
}

@compute @workgroup_size(256)
fn cs_g2p(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= counts[4] {
        return;
    }
    var p = drops[i];
    if !finite2(p.x) || !finite2(p.v) {
        return;
    }
    let lq = st.liquids[liquid_of(p)];
    let dt = st.time.x;
    let base = base_of(p.x);
    let fx = p.x - vec2<f32>(base);
    var w = weights(fx);
    var v = vec2<f32>(0.0);
    var c = vec4<f32>(0.0);
    var vd = 0.0;
    for (var a = 0; a < 3; a = a + 1) {
        for (var b = 0; b < 3; b = b + 1) {
            let wt = w[a].x * w[b].y;
            let dpos = vec2<f32>(f32(a), f32(b)) - fx;
            let n = node(base.x + a, base.y + b);
            let gv = vec2<f32>(bitcast<f32>(atomicLoad(&grid[4u * n + 1u])), bitcast<f32>(atomicLoad(&grid[4u * n + 2u])));
            v = v + wt * gv;
            c = c + 4.0 * wt * vec4<f32>(gv.x * dpos.x, gv.x * dpos.y, gv.y * dpos.x, gv.y * dpos.y);
            vd = vd + wt * f32(atomicLoad(&grid[4u * n + 3u])) / S_M;
        }
    }
    p.j = 1.0 / max(vd, 0.05);
    v = v * exp(-lq.a.w * dt);
    let speed = length(v);
    if speed > st.time.w {
        v = v * (st.time.w / speed);
    }
    // Which side of the nearest wall's line the drop is on before the balls push it: the walls
    // keep it there, however far a ball pushed it.
    let near0 = clamp(vec2<i32>(round(p.x)), vec2<i32>(0), vec2<i32>(i32(st.grid.x) - 1, i32(st.grid.y) - 1));
    let s0 = solid[2u * node(near0.x, near0.y)];
    let side0 = select(-1.0, 1.0, s0.z + dot(s0.xy, p.x - vec2<f32>(near0)) >= 0.0);
    let x0 = p.x;
    // The balls push it aside (before the walls, so a ball never pushes a drop through one).
    if st.flags.z > 0u && lq.b.w > 0.5 {
        let me = liquid_of(p);
        let mass = max(lq.b.y + p.mat.x, 0.05) * 0.25;
        let record = st.feel.x == 1u;
        let range = ball_cell(p.x);
        for (var e = range.x; e < range.y; e = e + 1u) {
            let bi = words[st.ball_grid.w + e];
            let at = st.words_at.z + bi * BALL_WORDS;
            if ((words[at + 5u] >> me) & 1u) == 0u {
                continue;
            }
            let bx = ball_now(at);
            let bv = vec2<f32>(wf(at + 2u), wf(at + 3u));
            let r = wf(at + 4u) + 0.5;
            let off = p.x - bx;
            let dist = length(off);
            var n = vec2<f32>(0.0, -1.0);
            if dist > 1e-5 {
                n = off / dist;
            }
            let fb = bi * FEEL;
            if dist < r {
                p.x = bx + n * r;
                var vr = v - bv;
                let vn = dot(vr, n);
                if vn < 0.0 {
                    vr = vr - vn * n;
                    // The ball takes what the drop lost: its push, by 1 / its density here.
                    if record {
                        let push = mass * vn * n * lq.e.x;
                        atomicAdd(&feel[fb], fixed(push.x, S_PUSH));
                        atomicAdd(&feel[fb + 1u], fixed(push.y, S_PUSH));
                    }
                    // A hard hit splashes: thrown out harder (Splash size over 1), or slid
                    // along the ball less (under 1).
                    let hit = -vn - lq.g.y;
                    if hit > 0.0 {
                        if lq.g.x > 1.0 {
                            vr = vr + n * ((lq.g.x - 1.0) * 0.5 * hit);
                        } else {
                            let vt = vr - dot(vr, n) * n;
                            vr = vr - vt * (1.0 - max(lq.g.x, 0.0));
                        }
                    }
                }
                v = vr + bv;
                p.kick = 1.0;
                // Ink: the drop's colour runs to the ball's ink.
                if lq.f.w > 0.0 {
                    let t = 1.0 - exp(-12.0 * lq.f.w * dt);
                    p.colour = vec4<f32>(mix(p.colour.rgb, lq.f.rgb, t), p.colour.a);
                }
                if record {
                    atomicAdd(&feel[fb + 2u], fixed(mass, S_TOUCH));
                    atomicAdd(&feel[fb + 3u], fixed(mass * p.colour.r, S_TOUCH));
                    atomicAdd(&feel[fb + 4u], fixed(mass * p.colour.g, S_TOUCH));
                    atomicAdd(&feel[fb + 5u], fixed(mass * p.colour.b, S_TOUCH));
                    atomicAdd(&feel[fb + 6u], fixed(mass * lq.e.y, S_TOUCH));
                    atomicAdd(&feel[fb + 7u], fixed(mass * lq.e.z, S_TOUCH));
                    atomicAdd(&feel[fb + 11u], fixed(mass * lq.g.w, S_TOUCH));
                    atomicAdd(&feel[fb + 16u + me], 1);
                }
            } else if lq.e.w > 0.0 && dist < r + 1.5 {
                // It clings: pulled in, and taking the ball's speed; the ball carries it.
                let before = v;
                v = mix(v, bv, 1.0 - exp(-20.0 * lq.e.w * dt)) - n * (40.0 * lq.e.w * dt);
                if record {
                    let push = -mass * (v - before) * lq.e.x;
                    atomicAdd(&feel[fb], fixed(push.x, S_PUSH));
                    atomicAdd(&feel[fb + 1u], fixed(push.y, S_PUSH));
                }
            }
            // Wakes: the liquid just behind a moving ball is pulled along after it.
            if lq.g.z > 0.0 && dist >= r && dist < r + 3.0 && dot(off, bv) < 0.0 {
                let fade = 1.0 - (dist - r) / 3.0;
                v = v + (bv - v) * ((1.0 - exp(-4.0 * lq.g.z * dt)) * fade);
            }
        }
    }
    // The walls: off the band's nearest node, the distance carried along its normal.
    let near = clamp(vec2<i32>(round(p.x)), vec2<i32>(0), vec2<i32>(i32(st.grid.x) - 1, i32(st.grid.y) - 1));
    let s = solid[2u * node(near.x, near.y)];
    if s.w > 0.5 {
        let t = solid[2u * node(near.x, near.y) + 1u];
        // The drop's own side of the line, from wherever the node is: where it was before a ball
        // pushed it, when one did.
        let line_d = s.z + dot(s.xy, p.x - vec2<f32>(near));
        var f = wall_face(s, t, line_d);
        if s0.w > 0.5 && t.z >= 0.0 && any(p.x != x0) {
            f = vec3<f32>(s.xy * side0, side0 * line_d - t.z);
        }
        if f.z < 0.5 {
            p.x = p.x + f.xy * min(0.5 - f.z, 4.0);
            let vw = t.xy;
            var vr = v - vw;
            let vn = dot(vr, f.xy);
            if vn < 0.0 {
                vr = vr - vn * f.xy;
            }
            v = vr + vw;
        }
    }
    // The edges stop a liquid that fills the scene.
    let lo = vec2<f32>(2.0);
    let hi = vec2<f32>(f32(st.grid.x) - 2.0, f32(st.grid.y) - 2.0);
    if (p.kind & STOPS) != 0u {
        if p.x.x < lo.x + 0.05 { p.x.x = lo.x + 0.05; v.x = max(v.x, 0.0); }
        if p.x.x > hi.x - 0.05 { p.x.x = hi.x - 0.05; v.x = min(v.x, 0.0); }
        if p.x.y < lo.y + 0.05 { p.x.y = lo.y + 0.05; v.y = max(v.y, 0.0); }
        if p.x.y > hi.y - 0.05 { p.x.y = hi.y - 0.05; v.y = min(v.y, 0.0); }
    }
    p.v = v;
    p.c = c;
    p.x = p.x + dt * v;
    p.kick = max(p.kick - 2.0 * dt, 0.0);
    drops[i] = p;
}

// ---- mixing --------------------------------------------------------------------------------

@compute @workgroup_size(1)
fn cs_scan_cells() {
    counts[5] = st.counts.w;
}

@compute @workgroup_size(1)
fn cs_scan_drops() {
    counts[5] = counts[4];
}

@compute @workgroup_size(256)
fn cs_bin_clear(@builtin(global_invocation_id) id: vec3<u32>) {
    let k = id.x;
    if k >= st.counts.w {
        return;
    }
    atomicStore(&flags[k], 0u);
}

fn cell_of(x: vec2<f32>) -> u32 {
    let c = clamp(vec2<i32>(floor(x)), vec2<i32>(0), vec2<i32>(i32(st.grid.x) - 1, i32(st.grid.y) - 1));
    return node(c.x, c.y);
}

@compute @workgroup_size(256)
fn cs_bin_count(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= counts[4] {
        return;
    }
    atomicAdd(&flags[cell_of(drops[i].x)], 1u);
}

@compute @workgroup_size(256)
fn cs_bin_place(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= counts[4] {
        return;
    }
    let k = cell_of(drops[i].x);
    let start = sums[k / WG] + atomicLoad(&flags[st.counts.z + k]);
    let slot = atomicAdd(&flags[k], 1u);
    items[start + slot] = i;
}

fn max3(c: vec3<f32>) -> f32 {
    return max(c.x, max(c.y, c.z));
}

fn to_oklab(c: vec3<f32>) -> vec3<f32> {
    let l = 0.4122214708 * c.r + 0.5363325363 * c.g + 0.0514459929 * c.b;
    let m = 0.2119034982 * c.r + 0.6806995451 * c.g + 0.1073969566 * c.b;
    let s = 0.0883024619 * c.r + 0.2817188376 * c.g + 0.6299787005 * c.b;
    let l_ = pow(max(l, 0.0), 1.0 / 3.0);
    let m_ = pow(max(m, 0.0), 1.0 / 3.0);
    let s_ = pow(max(s, 0.0), 1.0 / 3.0);
    return vec3<f32>(
        0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_,
        1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_,
        0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_,
    );
}

fn from_oklab(c: vec3<f32>) -> vec3<f32> {
    let l_ = c.x + 0.3963377774 * c.y + 0.2158037573 * c.z;
    let m_ = c.x - 0.1055613458 * c.y - 0.0638541728 * c.z;
    let s_ = c.x - 0.0894841775 * c.y - 1.2914855480 * c.z;
    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;
    return vec3<f32>(
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    );
}

fn ks(r: f32) -> f32 {
    let x = clamp(r, 0.002, 1.0);
    return (1.0 - x) * (1.0 - x) / (2.0 * x);
}

fn from_ks(k: f32) -> f32 {
    return 1.0 + k - sqrt(k * k + 2.0 * k);
}

// Pigments: each colour as a reflectance over 8 bands, mixed by Kubelka–Munk, and back.
fn paint_mix(a: vec3<f32>, b: vec3<f32>, t: f32) -> vec3<f32> {
    var out = vec3<f32>(0.0);
    for (var band = 0u; band < 8u; band = band + 1u) {
        let basis = st.paint[band].xyz;
        let ra = dot(basis, vec3<f32>(a.b, a.g, a.r));
        let rb = dot(basis, vec3<f32>(b.b, b.g, b.r));
        let r = from_ks(mix(ks(ra), ks(rb), t));
        out = out + st.back[band].xyz * r;
    }
    return max(out, vec3<f32>(0.0));
}

fn mix_colours(style: u32, a: vec3<f32>, b: vec3<f32>, t: f32) -> vec3<f32> {
    switch style {
        case STYLE_PAINT: {
            // Scaled to the pigments' range and back, so bright paint stays bright.
            let top = max(max3(a), max3(b));
            if top <= 1e-5 {
                return mix(a, b, t);
            }
            let s = max(top, 1.0);
            return paint_mix(a / s, b / s, t) * s;
        }
        case STYLE_LIGHT: {
            let m = mix(a, b, t);
            let peak = max(max3(a), max3(b));
            return m * (peak / max(max3(m), 1e-5));
        }
        case STYLE_BLEND: {
            return max(from_oklab(mix(to_oklab(a), to_oklab(b), t)), vec3<f32>(0.0));
        }
        default: {
            return a;
        }
    }
}

const Q: f32 = 65536.0;

fn q(x: f32) -> i32 {
    return i32(clamp(x * Q, -1.0e9, 1.0e9));
}

@compute @workgroup_size(256)
fn cs_mix(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= counts[4] {
        return;
    }
    let p = drops[i];
    let li = liquid_of(p);
    let wi = max(st.liquids[li].b.y + p.mat.x, 0.05);
    let ni = st.liquids[li].b.x + p.mat.y;
    var acc_c = vec3<i32>(0);
    var acc_m = vec2<i32>(0);
    var acc_x = vec2<i32>(0);
    var acc_lift = 0;
    var acc_w = 0;
    let at = vec2<i32>(floor(p.x));
    for (var dy = -1; dy <= 1; dy = dy + 1) {
        for (var dx = -1; dx <= 1; dx = dx + 1) {
            let cx = at.x + dx;
            let cy = at.y + dy;
            if cx < 0 || cy < 0 || cx >= i32(st.grid.x) || cy >= i32(st.grid.y) {
                continue;
            }
            let k = node(cx, cy);
            let start = sums[k / WG] + atomicLoad(&flags[st.counts.z + k]);
            let n = atomicLoad(&flags[k]);
            for (var e = 0u; e < n; e = e + 1u) {
                let j = items[start + e];
                if j == i {
                    continue;
                }
                let o = drops[j];
                let off = p.x - o.x;
                let d = length(off);
                if d >= 1.0 {
                    continue;
                }
                let w = (1.0 - d) * (1.0 - d);
                acc_w = acc_w + q(w);
                let lj = liquid_of(o);
                let rule = st.pairs[li * 4u + lj / 4u][lj % 4u];
                let mode = rule & 3u;
                let style = (rule >> 2u) & 7u;
                let when = (rule >> 5u) & 3u;
                let rate = f32(rule >> 16u) / 65535.0;
                var gate = 1.0;
                if when == WHEN_STIRRED {
                    gate = min(length(p.v - o.v) / 30.0, 1.0);
                } else if when == WHEN_HITS {
                    gate = p.kick;
                }
                // Layer keeps two liquids apart; so does Keep apart between two liquids.
                let apart = mode == 1u || (style == STYLE_KEEP_APART && li != lj);
                if mode == 0u && style != STYLE_MARBLE && style != STYLE_KEEP_APART {
                    let si = max(p.colour.w, 1e-3);
                    let sj = max(o.colour.w, 1e-3);
                    let m = mix_colours(style, p.colour.rgb, o.colour.rgb, sj / (si + sj));
                    let g = w * gate * rate;
                    acc_c = acc_c + vec3<i32>(q(g * (m.r - p.colour.r)), q(g * (m.g - p.colour.g)), q(g * (m.b - p.colour.b)));
                    let wj = max(st.liquids[lj].b.y + o.mat.x, 0.05);
                    let nj = st.liquids[lj].b.x + o.mat.y;
                    acc_m = acc_m + vec2<i32>(q(g * 0.5 * (wj - wi)), q(g * 0.5 * (nj - ni)));
                }
                if apart && d > 1e-5 {
                    let wj = max(st.liquids[lj].b.y + o.mat.x, 0.05);
                    acc_x = acc_x + vec2<i32>(q(w * off.x / d), q(w * off.y / d));
                    acc_lift = acc_lift + q(w * (wj - wi) / (wi + wj));
                }
            }
        }
    }
    var dc = vec3<f32>(0.0);
    var dm = vec2<f32>(0.0);
    var dpos = vec2<f32>(0.0);
    if acc_w > 0 {
        let total = f32(acc_w);
        dc = vec3<f32>(acc_c) / total;
        dm = vec2<f32>(acc_m) / total;
        let g = vec2<f32>(st.time.y, st.time.z);
        let gl = length(g);
        var up = vec2<f32>(0.0, -1.0);
        if gl > 1e-5 {
            up = -g / gl;
        }
        dpos = vec2<f32>(acc_x) / total * st.more.z + up * (f32(acc_lift) / total) * st.more.w;
    }
    mixout[2u * i] = vec4<f32>(dc, 0.0);
    mixout[2u * i + 1u] = vec4<f32>(dpos, dm);
}

@compute @workgroup_size(256)
fn cs_mix_apply(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= counts[4] {
        return;
    }
    var p = drops[i];
    let a = mixout[2u * i];
    let b = mixout[2u * i + 1u];
    p.colour = vec4<f32>(max(p.colour.rgb + a.xyz, vec3<f32>(0.0)), p.colour.w);
    p.x = p.x + b.xy;
    p.mat = p.mat + b.zw;
    drops[i] = p;
}

// ---- packing the living drops back in order ---------------------------------------------------

fn alive(p: Drop) -> bool {
    if !finite2(p.x) || !finite2(p.v) || !(abs(p.j) < 1e30) {
        return false;
    }
    let lq = st.liquids[liquid_of(p)];
    if p.serial < lq.d.x {
        return false;
    }
    if p.x.y < lq.c.w {
        return false;
    }
    let lo = vec2<f32>(2.0);
    let hi = vec2<f32>(f32(st.grid.x) - 2.0, f32(st.grid.y) - 2.0);
    return all(p.x >= lo) && all(p.x <= hi);
}

@compute @workgroup_size(256)
fn cs_flag(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= counts[4] {
        return;
    }
    let p = drops[i];
    spare[i] = p;
    atomicStore(&flags[i], select(0u, 1u, alive(p)));
}

@compute @workgroup_size(256)
fn cs_scan_local(
    @builtin(global_invocation_id) id: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let n = counts[5];
    let i = id.x;
    var x = 0u;
    if i < n {
        x = atomicLoad(&flags[i]);
    }
    shared_sums[lid.x] = x;
    workgroupBarrier();
    for (var off = 1u; off < WG; off = off * 2u) {
        var add = 0u;
        if lid.x >= off {
            add = shared_sums[lid.x - off];
        }
        workgroupBarrier();
        shared_sums[lid.x] = shared_sums[lid.x] + add;
        workgroupBarrier();
    }
    if i < n {
        atomicStore(&flags[st.counts.z + i], shared_sums[lid.x] - x);
    }
    if lid.x == WG - 1u {
        sums[wid.x] = shared_sums[lid.x];
    }
}

// One workgroup turns the totals into where each workgroup's start; their sum in counts[6].
@compute @workgroup_size(256)
fn cs_scan_sums(@builtin(local_invocation_id) lid: vec3<u32>) {
    let groups = (counts[5] + WG - 1u) / WG;
    let per = (groups + WG - 1u) / WG;
    let first = min(lid.x * per, groups);
    let last = min(first + per, groups);
    var run = 0u;
    for (var k = first; k < last; k = k + 1u) {
        run = run + sums[k];
    }
    shared_sums[lid.x] = run;
    workgroupBarrier();
    for (var off = 1u; off < WG; off = off * 2u) {
        var add = 0u;
        if lid.x >= off {
            add = shared_sums[lid.x - off];
        }
        workgroupBarrier();
        shared_sums[lid.x] = shared_sums[lid.x] + add;
        workgroupBarrier();
    }
    var start = shared_sums[lid.x] - run;
    for (var k = first; k < last; k = k + 1u) {
        let s = sums[k];
        sums[k] = start;
        start = start + s;
    }
    if lid.x == WG - 1u {
        counts[6] = shared_sums[lid.x];
    }
}

// Before the drops are placed in their bins, each bin's count goes back to 0 (it counts again).
@compute @workgroup_size(256)
fn cs_bin_zero(@builtin(global_invocation_id) id: vec3<u32>) {
    let k = id.x;
    if k >= st.counts.w {
        return;
    }
    atomicStore(&flags[k], 0u);
}

@compute @workgroup_size(256)
fn cs_scatter(@builtin(global_invocation_id) id: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>) {
    let i = id.x;
    if i >= counts[4] || atomicLoad(&flags[i]) == 0u {
        return;
    }
    let dst = sums[wid.x] + atomicLoad(&flags[st.counts.z + i]);
    drops[dst] = spare[i];
}

@compute @workgroup_size(1)
fn cs_packed() {
    counts[1] = counts[6];
    counts[4] = counts[6];
}

// ---- what the balls feel -------------------------------------------------------------------

// Each ball's ring: how many of RING points two cells outside it are in the liquid, and the liquid's
// mean speed there (after the last substep, the grid holds the speeds and the volume).
@compute @workgroup_size(256)
fn cs_feel(@builtin(global_invocation_id) id: vec3<u32>) {
    // (bindings: the step, counts, grid, words, feel)
    let bi = id.x;
    if bi >= st.flags.z || st.feel.x == 0u {
        return;
    }
    let at = st.words_at.z + bi * BALL_WORDS;
    let bx = ball_now(at);
    let r = wf(at + 4u) + 2.0;
    let fb = bi * FEEL;
    // A ball no drop of a liquid that meets it touched is in none.
    if atomicLoad(&feel[fb + 2u]) == 0 {
        atomicStore(&feel[fb + 8u], 0);
        atomicStore(&feel[fb + 9u], 0);
        atomicStore(&feel[fb + 10u], 0);
        return;
    }
    let points = max(st.feel.y, 1u);
    var wet = 0u;
    var flow = vec2<f32>(0.0);
    for (var k = 0u; k < points; k = k + 1u) {
        let a = 6.2831853 * (f32(k) + 0.5) / f32(points);
        let q = vec2<i32>(round(bx + r * vec2<f32>(cos(a), sin(a))));
        if q.x < 0 || q.y < 0 || q.x >= i32(st.grid.x) || q.y >= i32(st.grid.y) {
            continue;
        }
        let n = node(q.x, q.y);
        let vol = f32(atomicLoad(&grid[4u * n + 3u])) / S_M;
        if vol > 0.3 {
            wet = wet + 1u;
            flow = flow + vec2<f32>(bitcast<f32>(atomicLoad(&grid[4u * n + 1u])), bitcast<f32>(atomicLoad(&grid[4u * n + 2u])));
        }
    }
    if wet > 0u {
        flow = flow / f32(wet);
    }
    atomicStore(&feel[fb + 8u], i32(wet));
    atomicStore(&feel[fb + 9u], bitcast<i32>(flow.x));
    atomicStore(&feel[fb + 10u], bitcast<i32>(flow.y));
}

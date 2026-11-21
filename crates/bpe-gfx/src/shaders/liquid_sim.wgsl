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
    // temperature (0..1), dryness
    spare: vec2<f32>,
    // its elastic stretch F: xx, xy, yx, yy (the identity at rest)
    f: vec4<f32>,
    // x: the squeeze it carries, as J - 1 (0 at rest, below 0 squeezed, above 0 stretched; at its
    // liquid's tear or more it is torn off). yzw spare.
    more: vec4<f32>,
};

struct Liquid {
    // stiffness (E for weight 1), surface pull, tear, damping (per second)
    a: vec4<f32>,
    // viscosity (cells²/s), weight, gravity ×, balls push it (0 or 1)
    b: vec4<f32>,
    // mix style, how far a drop inside moves into an emptier place a substep (cells, at a
    // slope of 1), the openness it stops moving by, cut line (cells: drops above it go; -1e30 none)
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
    // stretch (a share of its stiffness), tears at (log), freezes below (-1 never), melts above
    h: vec4<f32>,
    // cools, warms, cold thickens it, dries (a second)
    i: vec4<f32>,
    // stains (0/1), fades (a second), balls heat it, shapes paint it
    j: vec4<f32>,
    // the colour it fades to
    k: vec4<f32>,
    // the colour shapes paint it
    l: vec4<f32>,
    // the colour (and strength) a reaction that makes it gives
    m: vec4<f32>,
    // to the balls in it: shrink rate (below 0 grows), dissolves (0/1)
    n: vec4<f32>,
};

struct Force {
    // place xy, reach, strength
    a: vec4<f32>,
    // a way xy, two more numbers
    b: vec4<f32>,
    // kind, the liquids it moves (mask), spikes, 0
    c: vec4<u32>,
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
    // the liquid step's number, forces, un-mix's t (f32 bits), un-mix takes the older sample (0/1)
    extra: vec4<u32>,
    // Paint: each band's basis (blue, green, red) and its way back (r, g, b rows over 8 bands)
    paint: array<vec4<f32>, 8>,
    back: array<vec4<f32>, 8>,
    liquids: array<Liquid, 16>,
    // 16 × 16 meeting rules, 4 to a vec4: mode (bits 0..1: 0 mix, 1 layer) | style
    // << 2 (3 bits) | when << 5 (2 bits) | rate × 65535 << 16
    pairs: array<vec4<u32>, 64>,
    // a reacting pair's heat (bits 0..7) and fizz (8..15)
    pairs2: array<vec4<u32>, 64>,
    forces: array<Force, 48>,
    // the liquids each wall number paints, 4 to a vec4
    paints: array<vec4<u32>, 16>,
};

const WG: u32 = 256u;
const S_M: f32 = 1048576.0;
const S_P: f32 = 4096.0;
const LIMIT: f32 = 33554432.0;
const STOPS: u32 = 256u;
// How far in from each edge of the grid a liquid that fills the scene is stopped, in cells.
const EDGE: f32 = 2.0;
// Half the room a drop at rest takes, in cells: where the walls' share of the liquid begins.
const WALL_GAP: f32 = 0.25;
// How strongly squeezing is damped: times the speed of sound over one cell.
const BULK: f32 = 0.7;
// How far across a gap "holes close" looks for liquid, in cells.
const CLOSE_ACROSS: f32 = 2.5;
// The squeeze a drop carries is held within this share of what the grid says is round it, moved
// to that edge at CARRY_DOWN a second when the liquid is more packed than the drop thinks and at
// CARRY_UP when less. A narrow band, or a quick way up, brings the simmer at walls back.
const CARRY_SLACK: f32 = 0.15;
const CARRY_DOWN: f32 = 30.0;
const CARRY_UP: f32 = 4.0;
// "Holes close" moves a drop only where the liquid round it (walls counted) reads less than this
// share of full, and fully from 0.15 under it.
const HOLE_BELOW: f32 = 0.95;
// How far each way along a thin wall's normal `cs_grid` looks for which side the liquid is on, in cells.
const GHOST_LOOK: f32 = 2.0;
const FROZEN: u32 = 512u;
const STAIN_BASE: u32 = 16u;
const MEET_REACT: u32 = 2u;
const FORCE_HEAT: u32 = 0u;
const FORCE_DRAIN: u32 = 1u;
const FORCE_PUMP: u32 = 2u;
const FORCE_WHIRLPOOL: u32 = 3u;
const FORCE_WIND: u32 = 4u;
const FORCE_MAGNET: u32 = 5u;
const SAMPLE_WORDS: u32 = 5u;
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
// wet ring points, flow x y (f32 bits), take × mass, depth (cells × 16), 3 spare, contacts with each liquid (16)
@group(0) @binding(12) var<storage, read_write> feel: array<atomic<i32>>;
// Un-mix: the sample being written, and the newer and older samples played back between
@group(0) @binding(13) var<storage, read_write> sample: array<u32>;
@group(0) @binding(14) var<storage, read> newer: array<u32>;
@group(0) @binding(15) var<storage, read> older: array<u32>;

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

// To the nearest whole number. Toward zero (a bare `i32()`) every add lost up to one unit in the
// direction that slows the liquid: a friction that stopped slow liquid dead, the more so the more
// substeps, so each Detail level had its own.
fn fixed(x: f32, scale: f32) -> i32 {
    return i32(round(clamp(x * scale, -LIMIT, LIMIT)));
}

fn liquid_of(d: Drop) -> u32 {
    return d.kind & 15u;
}

fn is_stain(d: Drop) -> bool {
    return (d.kind & 255u) >= STAIN_BASE;
}

fn hash(x: u32) -> u32 {
    var h = x * 747796405u + 2891336453u;
    h = ((h >> ((h >> 28u) + 4u)) ^ h) * 277803737u;
    return (h >> 22u) ^ h;
}

// 0 to 1 from a number.
fn unit(x: u32) -> f32 {
    return f32(hash(x) >> 8u) / 16777216.0;
}

// How dry drop `d` gets before it goes: its own, from its serial.
fn dries_at(d: Drop) -> f32 {
    return 0.5 + unit(d.serial * 31u + (d.kind & 15u));
}

// How much of the drop is out at the edge of the liquid (0 deep inside, 1 alone), from the
// volume its nodes hold.
fn exposure(vd: f32) -> f32 {
    return clamp((0.9 - vd) / 0.5, 0.0, 1.0);
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

// Each band entry: node, normal xy, distance, wall speed xy, half, wall number (8 words).
@compute @workgroup_size(256)
fn cs_put_solid(@builtin(global_invocation_id) id: vec3<u32>) {
    let e = id.x;
    if e >= st.words_at.y {
        return;
    }
    let at = st.words_at.x + e * 8u;
    let k = words[at];
    if k >= st.counts.w {
        return;
    }
    solid[2u * k] = vec4<f32>(wf(at + 1u), wf(at + 2u), wf(at + 3u), 1.0);
    // .w: the wall's number (Shapes paint it)
    solid[2u * k + 1u] = vec4<f32>(wf(at + 4u), wf(at + 5u), wf(at + 6u), f32(words[at + 7u]));
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

// How much of a node's weight lies below `x` cells from it, along one way (its weight there is
// the quadratic spline the drops are spread with).
fn weight_below(x: f32) -> f32 {
    if x <= -1.5 {
        return 0.0;
    }
    if x >= 1.5 {
        return 1.0;
    }
    if x < -0.5 {
        let a = x + 1.5;
        return a * a * a / 6.0;
    }
    if x > 0.5 {
        let a = 1.5 - x;
        return 1.0 - a * a * a / 6.0;
    }
    return 0.5 + 0.75 * x - x * x * x / 3.0;
}

// The liquid a node would hold where no drop can be: inside a wall and the margin drops are kept
// out of on each side of it, counted as full (0 none, 1 the whole node). Without it a drop beside
// a wall finds half the liquid round it missing, takes itself for the open surface and pushes back
// at nothing, so the liquid packed against every wall and floor until three times too many drops
// were there, and burst when that layer was disturbed. `WALL_GAP` is how far in from where drops
// stop the wall's share begins: half the room a drop at rest takes.
fn wall_share(n: u32) -> f32 {
    let s = solid[2u * n];
    if s.w < 0.5 {
        return 0.0;
    }
    let t = solid[2u * n + 1u];
    let f = wall_face(s, t, s.z);
    var share = weight_below(0.5 - WALL_GAP - f.z);
    if t.z >= 0.0 {
        // A band: only as far as the margin on its other side (liquid there is counted itself).
        share = share - weight_below(WALL_GAP - 0.5 - 2.0 * t.z - f.z);
    }
    return share;
}

// The same for the picture's edges, which stop a liquid that fills the scene: node (`x`, `y`).
fn edge_share(x: i32, y: i32) -> f32 {
    let lo = EDGE + 0.05 - WALL_GAP;
    let hi = vec2<f32>(f32(st.grid.x), f32(st.grid.y)) - lo;
    let at = vec2<f32>(f32(x), f32(y));
    let sx = weight_below(lo - at.x) + weight_below(at.x - hi.x);
    let sy = weight_below(lo - at.y) + weight_below(at.y - hi.y);
    return 1.0 - (1.0 - min(sx, 1.0)) * (1.0 - min(sy, 1.0));
}

// Both together for node (`x`, `y`): a wall right at the picture's edge is not counted twice.
fn solid_share(x: i32, y: i32, stops: bool) -> f32 {
    let wall = wall_share(node(x, y));
    if !stops {
        return wall;
    }
    return 1.0 - (1.0 - wall) * (1.0 - edge_share(x, y));
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

// The weights' slopes along each way.
fn slopes(fx: vec2<f32>) -> array<vec2<f32>, 3> {
    return array<vec2<f32>, 3>(fx - 1.5, -2.0 * (fx - 1.0), fx - 0.5);
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
    if !finite2(p.x) || !finite2(p.v) || is_stain(p) {
        return;
    }
    let lq = st.liquids[liquid_of(p)];
    let weight = max(lq.b.y + p.mat.x, 0.05);
    let frozen = (p.kind & FROZEN) != 0u;
    // Cold thickens it; frozen, it is as thick as it can be.
    let cold = 1.0 - clamp(p.spare.x, 0.0, 1.0);
    var nu = clamp((lq.b.x + p.mat.y) * (1.0 + 30.0 * lq.i.z * cold * cold), 0.0, st.more.x);
    if frozen {
        nu = st.more.x;
    }
    let vol = 0.25;
    let mass = weight * vol;
    let base = base_of(p.x);
    let fx = p.x - vec2<f32>(base);
    var w = weights(fx);
    // How squeezed it is: the squeeze it carries (`cs_g2p` keeps it), as J - 1. Squeezed, it pushes
    // back; stretched less than its liquid tears at, it pulls together; torn off, neither.
    var e = 0.0;
    if abs(p.more.x) < 1e30 {
        e = p.more.x;
    }
    let stiff = lq.a.x * weight;
    var pressure = 0.0;
    if e < 0.0 {
        pressure = -stiff * e;
    } else if e < lq.a.z {
        pressure = -stiff * lq.a.y * e;
    }
    let dt = st.time.x;
    // σ = −p I + μ (C + Cᵀ), μ = ν ρ; affine = −dt · vol · 4 · σ + m C.
    let mu = nu * weight;
    var sxx = -pressure + mu * 2.0 * p.c.x;
    var sxy = mu * (p.c.y + p.c.z);
    var syy = -pressure + mu * 2.0 * p.c.w;
    // Squeezing and springing back are damped (a bulk viscosity, sized by the speed of sound
    // over one cell): the pressure alone is a spring with nothing to calm it, and any small
    // pile-up was thrown back out as fast as the liquid is stiff. Flowing, which keeps its
    // volume, feels none of it; nor does liquid torn open into spray, which it would smear.
    if e < lq.a.z {
        let squeeze = BULK * sqrt(lq.a.x) * weight * (p.c.x + p.c.w);
        sxx = sxx + squeeze;
        syy = syy + squeeze;
    }
    // Stretch: the elastic part, its shape only (b / J less its mean: pressure keeps the volume).
    var share = lq.h.x;
    if frozen {
        share = max(share, 0.5);
    }
    if share > 0.0 {
        let f = p.f;
        let jf = f.x * f.w - f.y * f.z;
        if jf > 0.05 && abs(jf) < 1e6 {
            let bxx = (f.x * f.x + f.y * f.y) / jf;
            let bxy = (f.x * f.z + f.y * f.w) / jf;
            let byy = (f.z * f.z + f.w * f.w) / jf;
            let me = share * stiff;
            sxx = sxx + me * 0.5 * (bxx - byy);
            syy = syy - me * 0.5 * (bxx - byy);
            sxy = sxy + me * bxy;
        }
    }
    let k = -dt * vol * 4.0;
    let axx = k * sxx + mass * p.c.x;
    let axy = k * sxy + mass * p.c.y;
    let ayx = k * sxy + mass * p.c.z;
    let ayy = k * syy + mass * p.c.w;
    let g = vec2<f32>(st.time.y, st.time.z) * lq.b.z;
    let mv = mass * (p.v + g * dt);
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
        // A node across a thin wall from the liquid whose weight it holds (drops beside a hollow
        // shape's wall reach over it): it moves along the wall with that liquid, and neither into
        // the wall nor away from it. Left free to move away, such nodes drifted off on their own
        // side, where there is nothing, and fed that back to the drops: still water crept or
        // simmered depending on where the wall's faces fell between the grid's nodes.
        var ghost = false;
        if s.w > 0.5 && t.z >= 0.0 {
            let at = vec2<f32>(f32(k % st.grid.x), f32(k / st.grid.x));
            let hi = vec2<f32>(f32(st.grid.x) - 1.0, f32(st.grid.y) - 1.0);
            let pa = vec2<i32>(clamp(round(at + GHOST_LOOK * s.xy), vec2<f32>(0.0), hi));
            let pb = vec2<i32>(clamp(round(at - GHOST_LOOK * s.xy), vec2<f32>(0.0), hi));
            let ma = atomicLoad(&grid[4u * node(pa.x, pa.y)]);
            let mb = atomicLoad(&grid[4u * node(pb.x, pb.y)]);
            // `s.z` is the node's distance out of the outline: its own side is the normal's when
            // that is 0 or more.
            if ma != mb && (ma > mb) != (s.z >= 0.0) {
                ghost = true;
                let vw = t.xy;
                let vr = v - vw;
                v = vr - dot(vr, s.xy) * s.xy + vw;
            }
        }
        if !ghost && s.w > 0.5 && f.z < 0.75 {
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
            let edge = i32(EDGE);
            if x <= edge { v.x = max(v.x, 0.0); }
            if x >= cols - edge { v.x = min(v.x, 0.0); }
            if y <= edge { v.y = max(v.y, 0.0); }
            if y >= rows - edge { v.y = min(v.y, 0.0); }
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
    if !finite2(p.x) || !finite2(p.v) || is_stain(p) {
        return;
    }
    let lq = st.liquids[liquid_of(p)];
    let me = liquid_of(p);
    let dt = st.time.x;
    let base = base_of(p.x);
    let fx = p.x - vec2<f32>(base);
    var w = weights(fx);
    let dw = slopes(fx);
    var v = vec2<f32>(0.0);
    var c = vec4<f32>(0.0);
    var vd = 0.0;
    var liquid = 0.0;
    var slope = vec2<f32>(0.0);
    let stops = (p.kind & STOPS) != 0u;
    for (var a = 0; a < 3; a = a + 1) {
        for (var b = 0; b < 3; b = b + 1) {
            let wt = w[a].x * w[b].y;
            let dpos = vec2<f32>(f32(a), f32(b)) - fx;
            let n = node(base.x + a, base.y + b);
            let gv = vec2<f32>(bitcast<f32>(atomicLoad(&grid[4u * n + 1u])), bitcast<f32>(atomicLoad(&grid[4u * n + 2u])));
            v = v + wt * gv;
            c = c + 4.0 * wt * vec4<f32>(gv.x * dpos.x, gv.x * dpos.y, gv.y * dpos.x, gv.y * dpos.y);
            // The liquid round it, and the walls' share of it (a wall counts as full).
            let real = f32(atomicLoad(&grid[4u * n + 3u])) / S_M;
            let nv = real + solid_share(base.x + a, base.y + b, stops);
            liquid = liquid + wt * real;
            vd = vd + wt * nv;
            slope = slope + vec2<f32>(dw[a].x * w[b].y, w[a].x * dw[b].y) * nv;
        }
    }
    // Kept without the walls' share: the picture draws a drop with little liquid round it as a
    // droplet. The pressure does not go by it (see `more.x` below).
    p.j = 1.0 / max(liquid, 0.05);
    // The squeeze it carries (what `cs_p2g` turns into pressure). It is moved by the flow's own
    // squeezing and only held to what the grid says is round it, loosely: read straight off the
    // grid (as it once was) the pressure heard every bit of the grid's sampling noise, which
    // changes with where a wall's face falls between nodes, and the top two cells of any liquid,
    // or a whole thin sheet, read "less than full" and carried no pressure at all.
    {
        let tear = max(lq.a.z, 0.0);
        // What the liquid round it says, were the drop at a flat open edge: `full` is the share of
        // its reach that lies inside the liquid there (1 deep inside, a half on the edge line),
        // from how steeply the liquid falls away.
        let r = length(slope) / max(vd, 0.05);
        let full = 1.0 / (1.0 + 0.2023 * r + 0.6425 * r * r);
        let jref = 1.0 / max(vd / max(full, 0.2), 0.05);
        var e = p.more.x;
        if !(abs(e) < 1e30) {
            e = 0.0;
        }
        if e >= tear {
            // Torn off: it joins again as soon as the liquid round it is dense enough.
            e = min(jref - 1.0, tear);
        } else {
            e = e + (1.0 + e) * dt * (c.x + c.w);
            // The net: quickly when the liquid is more packed than the drop thinks (that is what
            // bursts), slowly when less (the usual state near an edge the estimate gets wrong;
            // answered quickly, corners simmered).
            let lo = jref * (1.0 + CARRY_SLACK) - 1.0;
            let hi = jref * (1.0 - CARRY_SLACK) - 1.0;
            if e > lo {
                e = e + (lo - e) * (1.0 - exp(-CARRY_DOWN * dt));
            } else if e < hi {
                e = e + (hi - e) * (1.0 - exp(-CARRY_UP * dt));
            }
            e = clamp(e, -0.9, tear);
        }
        p.more = vec4<f32>(e, 0.0, 0.0, 0.0);
    }
    // How open to the air it is (cooling, wind and drying go by it): by the liquid alone, as a
    // drop on a wall is as open as one in the air.
    let open = exposure(liquid);
    // Inside the liquid, a drop moves a little towards where there is less liquid round it, so
    // holes and cracks left by a pour or a ball close up (a hole thinner than a cell has no
    // pressure of its own to close it). Not at the open surface, where that would spread it, nor
    // in ice.
    if lq.c.y > 0.0 && (p.kind & FROZEN) == 0u {
        let inner = 1.0 - smoothstep(0.0, max(lq.c.z, 1e-3), exposure(vd));
        // And only where liquid is really missing (the rim of a hole or a crack reads well under
        // full). Liquid with no holes reads within a tenth of full everywhere, by how its drops
        // happen to sit; moved down those slopes too, still water was kept stirring for good.
        let hole = 1.0 - smoothstep(HOLE_BELOW - 0.15, HOLE_BELOW, vd);
        var shift = -lq.c.y * inner * hole * slope;
        // Only towards liquid on the far side of a gap: at the open surface (nothing across)
        // it would pump the top of the liquid upwards for ever.
        let steep = length(slope);
        var across = 0.0;
        if steep > 1e-6 {
            let q = p.x - slope / steep * CLOSE_ACROSS;
            if q.x >= 0.0 && q.y >= 0.0 && q.x < f32(st.grid.x) - 1.0 && q.y < f32(st.grid.y) - 1.0 {
                let qb = vec2<i32>(floor(q));
                let qf = q - vec2<f32>(qb);
                let v00 = f32(atomicLoad(&grid[4u * node(qb.x, qb.y) + 3u])) / S_M;
                let v10 = f32(atomicLoad(&grid[4u * node(qb.x + 1, qb.y) + 3u])) / S_M;
                let v01 = f32(atomicLoad(&grid[4u * node(qb.x, qb.y + 1) + 3u])) / S_M;
                let v11 = f32(atomicLoad(&grid[4u * node(qb.x + 1, qb.y + 1) + 3u])) / S_M;
                across = smoothstep(0.3, 0.7, mix(mix(v00, v10, qf.x), mix(v01, v11, qf.x), qf.y));
            }
        }
        shift = shift * across;
        // Never into a wall, and not at all right against one: there every drop is put back on
        // the same line, so drops moved along it ended on exactly the same spot and stayed one.
        var off_wall = 1e9;
        let fb = clamp(vec2<i32>(floor(p.x)), vec2<i32>(0), vec2<i32>(i32(st.grid.x) - 2, i32(st.grid.y) - 2));
        for (var a = 0; a < 2; a = a + 1) {
            for (var b = 0; b < 2; b = b + 1) {
                let nk = node(fb.x + a, fb.y + b);
                let ws = solid[2u * nk];
                if ws.w > 0.5 {
                    let at = vec2<f32>(vec2<i32>(fb.x + a, fb.y + b));
                    let wf = wall_face(ws, solid[2u * nk + 1u], ws.z + dot(ws.xy, p.x - at));
                    off_wall = min(off_wall, wf.z);
                    let into = dot(shift, wf.xy);
                    if into < 0.0 {
                        shift = shift - into * wf.xy;
                    }
                }
            }
        }
        shift = shift * smoothstep(0.6, 1.6, off_wall);
        let most = 0.5 * lq.c.y;
        let len = length(shift);
        if len > most {
            shift = shift * (most / len);
        }
        if finite2(shift) {
            p.x = p.x + shift;
        }
    }
    // Heat: lost at the surface, gained near heat.
    var heat = p.spare.x;
    heat = heat - lq.i.x * open * heat * dt;
    // The forces on it: heat, drains, currents and magnets.
    for (var e = 0u; e < st.extra.y; e = e + 1u) {
        let fo = st.forces[e];
        if ((fo.c.y >> me) & 1u) == 0u {
            continue;
        }
        let rel = p.x - fo.a.xy;
        let d = length(rel);
        var dir = vec2<f32>(0.0);
        if d > 1e-4 {
            dir = -rel / d;
        }
        let reach = max(fo.a.z, 1e-3);
        let fall = max(1.0 - d / reach, 0.0);
        switch fo.c.x {
            case FORCE_HEAT: {
                if fall > 0.0 {
                    let to = select(0.0, 1.0, fo.a.w > 0.0);
                    heat = heat + (to - heat) * (1.0 - exp(-lq.i.y * abs(fo.a.w) * fall * dt));
                }
            }
            case FORCE_DRAIN, FORCE_WHIRLPOOL: {
                // A pull in (a.w) and a spin round (b.z), fading out to its reach.
                let spin = vec2<f32>(-dir.y, dir.x);
                v = v + dt * fall * (fo.a.w * dir + fo.b.z * spin) * min(d, 1.0);
            }
            case FORCE_PUMP: {
                let way = fo.b.xy;
                let along = dot(rel, way);
                let across = abs(dot(rel, vec2<f32>(-way.y, way.x)));
                let half = max(fo.b.w, 1e-3);
                if along >= 0.0 && along <= fo.b.z && across <= half {
                    v = v + dt * fo.a.w * way * (1.0 - across / half);
                }
            }
            case FORCE_WIND: {
                v = v + dt * fo.a.w * fo.b.xy * open;
            }
            case FORCE_MAGNET: {
                if fall > 0.0 && d > 1e-3 {
                    let a = atan2(rel.y, rel.x);
                    let lobes = 1.0 + fo.b.z * cos(f32(fo.c.z) * a);
                    v = v + dt * fo.a.w * fall * fall * lobes * dir * min(d * 0.5, 1.0);
                }
            }
            default: {}
        }
    }
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
                    atomicAdd(&feel[fb + 14u], fixed(mass * lq.n.x, S_TOUCH));
                    atomicAdd(&feel[fb + 15u], fixed(mass * lq.n.y, S_TOUCH));
                    atomicAdd(&feel[fb + 16u + me], 1);
                }
                // Balls heat it.
                if lq.j.z > 0.0 {
                    heat = heat + (1.0 - heat) * (1.0 - exp(-4.0 * lq.j.z * dt));
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
        // Shapes paint it: the drop takes the paint of a wall it touches.
        let wall = u32(t.w + 0.5);
        if lq.j.w > 0.0 && f.z < 1.0 && wall < 64u && ((st.paints[wall / 4u][wall % 4u] >> me) & 1u) == 1u {
            p.colour = vec4<f32>(mix(p.colour.rgb, lq.l.rgb, 1.0 - exp(-12.0 * lq.j.w * dt)), p.colour.a);
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
    let lo = vec2<f32>(EDGE);
    let hi = vec2<f32>(f32(st.grid.x) - EDGE, f32(st.grid.y) - EDGE);
    if (p.kind & STOPS) != 0u {
        if p.x.x < lo.x + 0.05 { p.x.x = lo.x + 0.05; v.x = max(v.x, 0.0); }
        if p.x.x > hi.x - 0.05 { p.x.x = hi.x - 0.05; v.x = min(v.x, 0.0); }
        if p.x.y < lo.y + 0.05 { p.x.y = lo.y + 0.05; v.y = max(v.y, 0.0); }
        if p.x.y > hi.y - 0.05 { p.x.y = hi.y - 0.05; v.y = min(v.y, 0.0); }
    }
    // Freezing and melting.
    heat = clamp(heat, 0.0, 1.0);
    var frozen = (p.kind & FROZEN) != 0u;
    if lq.h.z >= 0.0 {
        if !frozen && heat < lq.h.z {
            frozen = true;
            p.kind = p.kind | FROZEN;
            p.f = vec4<f32>(1.0, 0.0, 0.0, 1.0);
        } else if frozen && heat > lq.h.w {
            frozen = false;
            p.kind = p.kind & ~FROZEN;
        }
    } else if frozen {
        frozen = false;
        p.kind = p.kind & ~FROZEN;
    }
    p.spare.x = heat;
    // Stretch: F moves with the liquid; stretched past Tears at (3 times for ice) it snaps.
    if lq.h.x > 0.0 || frozen {
        let f = p.f;
        var g = vec4<f32>(
            (1.0 + dt * c.x) * f.x + dt * c.y * f.z,
            (1.0 + dt * c.x) * f.y + dt * c.y * f.w,
            dt * c.z * f.x + (1.0 + dt * c.w) * f.z,
            dt * c.z * f.y + (1.0 + dt * c.w) * f.w,
        );
        let jf = g.x * g.w - g.y * g.z;
        var snap = !(jf > 0.05) || !(abs(jf) < 1e6);
        if !snap {
            let bxx = (g.x * g.x + g.y * g.y) / jf;
            let bxy = (g.x * g.z + g.y * g.w) / jf;
            let byy = (g.z * g.z + g.w * g.w) / jf;
            let half = 0.5 * (bxx + byy);
            let disc = sqrt(max(half * half - (bxx * byy - bxy * bxy), 0.0));
            let strain = 0.5 * max(abs(log(max(half + disc, 1e-6))), abs(log(max(half - disc, 1e-6))));
            var limit = lq.h.y;
            if frozen {
                limit = 1.0986;
            }
            snap = strain > limit;
        }
        if snap {
            g = vec4<f32>(1.0, 0.0, 0.0, 1.0);
        }
        p.f = g;
    }
    // Drying, at its surface: past its own point it goes (or stays as a stain).
    if lq.i.w > 0.0 {
        p.spare.y = p.spare.y + lq.i.w * open * dt;
        if lq.j.x > 0.5 && p.spare.y > dries_at(p) {
            p.kind = (STAIN_BASE + me) | (p.kind & STOPS);
            v = vec2<f32>(0.0);
            c = vec4<f32>(0.0);
        }
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
    if is_stain(p) {
        mixout[3u * i] = vec4<f32>(0.0);
        mixout[3u * i + 1u] = vec4<f32>(0.0);
        mixout[3u * i + 2u] = vec4<f32>(0.0);
        return;
    }
    let li = liquid_of(p);
    let wi = max(st.liquids[li].b.y + p.mat.x, 0.05);
    let ni = st.liquids[li].b.x + p.mat.y;
    var acc_t = 0;
    var react_w = 0;
    var react_best = 0;
    var react_rule = 0u;
    var react_more = 0u;
    var react_lj = 255u;
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
                if is_stain(o) {
                    continue;
                }
                let off = p.x - o.x;
                let d = length(off);
                if d >= 1.0 {
                    continue;
                }
                let w = (1.0 - d) * (1.0 - d);
                acc_w = acc_w + q(w);
                // Heat passes between neighbours, whatever liquid.
                acc_t = acc_t + q(w * (o.spare.x - p.spare.x));
                let lj = liquid_of(o);
                let rule = st.pairs[li * 4u + lj / 4u][lj % 4u];
                let mode = rule & 3u;
                if mode == MEET_REACT {
                    react_w = react_w + q(w);
                    // Ties go to the lower liquid number: the order of drops in a cell is not set.
                    if q(w) > react_best || (q(w) == react_best && lj < react_lj) {
                        react_best = q(w);
                        react_lj = lj;
                        react_rule = rule;
                        react_more = st.pairs2[li * 4u + lj / 4u][lj % 4u];
                    }
                }
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
    var dt_heat = 0.0;
    var turn = vec4<f32>(0.0);
    if acc_w > 0 {
        let total = f32(acc_w);
        dc = vec3<f32>(acc_c) / total;
        dm = vec2<f32>(acc_m) / total;
        dt_heat = 0.25 * f32(acc_t) / total;
        // A reaction: the chance it turns this mixing, by how much of what it reacts with touches it.
        if react_w > 0 {
            let rate = f32(react_rule >> 16u) / 65535.0;
            let chance = 1.0 - exp(-8.0 * rate * f32(react_w) / Q);
            if unit(p.serial * 7919u + li * 104729u + st.extra.x * 15485863u) < chance {
                let makes = (react_rule >> 7u) & 15u;
                turn = vec4<f32>(f32(makes) + 1.0, f32(react_more & 255u) / 255.0, f32((react_more >> 8u) & 255u) / 255.0, 0.0);
            }
        }
        let g = vec2<f32>(st.time.y, st.time.z);
        let gl = length(g);
        var up = vec2<f32>(0.0, -1.0);
        if gl > 1e-5 {
            up = -g / gl;
        }
        dpos = vec2<f32>(acc_x) / total * st.more.z + up * (f32(acc_lift) / total) * st.more.w;
    }
    mixout[3u * i] = vec4<f32>(dc, dt_heat);
    mixout[3u * i + 1u] = vec4<f32>(dpos, dm);
    mixout[3u * i + 2u] = turn;
}

@compute @workgroup_size(256)
fn cs_mix_apply(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= counts[4] {
        return;
    }
    var p = drops[i];
    if is_stain(p) {
        return;
    }
    let a = mixout[3u * i];
    let b = mixout[3u * i + 1u];
    let r = mixout[3u * i + 2u];
    p.colour = vec4<f32>(max(p.colour.rgb + a.xyz, vec3<f32>(0.0)), p.colour.w);
    p.spare.x = clamp(p.spare.x + a.w, 0.0, 1.0);
    p.x = p.x + b.xy;
    p.mat = p.mat + b.zw;
    // Fades: the colour drifts to the liquid's Fades to.
    let lq = st.liquids[liquid_of(p)];
    if lq.j.y > 0.0 {
        p.colour = vec4<f32>(mix(p.colour.rgb, lq.k.rgb, 1.0 - exp(-lq.j.y * st.more.y)), p.colour.w);
    }
    // It reacted: it is now the liquid the reaction makes, warmed and thrown up by the fizz.
    if r.x > 0.5 {
        let makes = u32(r.x - 0.5) & 15u;
        let to = st.liquids[makes];
        p.kind = makes | (p.kind & STOPS);
        // Its serial is the made liquid's newest, so that liquid's Most drops keeps it as new.
        p.serial = to.d.y;
        p.colour = to.m;
        p.mat = vec2<f32>(0.0);
        p.f = vec4<f32>(1.0, 0.0, 0.0, 1.0);
        p.spare = vec2<f32>(clamp(p.spare.x + r.y, 0.0, 1.0), 0.0);
        if r.z > 0.0 {
            let g = vec2<f32>(st.time.y, st.time.z);
            var up = vec2<f32>(0.0, -1.0);
            if length(g) > 1e-5 {
                up = -normalize(g);
            }
            let side = unit(p.serial * 2654435761u + st.extra.x) - 0.5;
            p.v = p.v + r.z * 40.0 * (up + vec2<f32>(-up.y, up.x) * side);
        }
    }
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
    if is_stain(p) {
        let lo = vec2<f32>(EDGE);
        let hi = vec2<f32>(f32(st.grid.x) - EDGE, f32(st.grid.y) - EDGE);
        return all(p.x >= lo) && all(p.x <= hi);
    }
    if p.x.y < lq.c.w {
        return false;
    }
    // Dried up (with no stains to leave).
    if lq.i.w > 0.0 && p.spare.y > dries_at(p) {
        return false;
    }
    // Down a drain.
    let me = liquid_of(p);
    for (var e = 0u; e < st.extra.y; e = e + 1u) {
        let fo = st.forces[e];
        if fo.c.x == FORCE_DRAIN && ((fo.c.y >> me) & 1u) == 1u && length(p.x - fo.a.xy) < fo.b.w {
            return false;
        }
    }
    let lo = vec2<f32>(EDGE);
    let hi = vec2<f32>(f32(st.grid.x) - EDGE, f32(st.grid.y) - EDGE);
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
        atomicStore(&feel[fb + 12u], 0);
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
    // How deep it is (liquid part 5): cells of liquid straight above its top, against gravity.
    var up = vec2<f32>(0.0, -1.0);
    let g = st.time.yz;
    if dot(g, g) > 1e-12 {
        up = -normalize(g);
    }
    let top = bx + up * (wf(at + 4u) + 1.0);
    var depth = 0u;
    for (var k = 0u; k < 255u; k = k + 1u) {
        let q = vec2<i32>(round(top + up * f32(k)));
        if q.x < 0 || q.y < 0 || q.x >= i32(st.grid.x) || q.y >= i32(st.grid.y) {
            break;
        }
        if f32(atomicLoad(&grid[4u * node(q.x, q.y) + 3u])) / S_M <= 0.3 {
            break;
        }
        depth = depth + 1u;
    }
    atomicStore(&feel[fb + 12u], i32(depth * 16u));
}

// ---- Un-mix: samples of the drops, and playing them back -----------------------------------

fn colour_code(c: vec3<f32>) -> u32 {
    let s = sqrt(clamp(c / 4.0, vec3<f32>(0.0), vec3<f32>(1.0))) * 1023.0;
    let u = vec3<u32>(round(s));
    return u.x | (u.y << 10u) | (u.z << 20u);
}

fn colour_back(w: u32) -> vec3<f32> {
    let s = vec3<f32>(f32(w & 1023u), f32((w >> 10u) & 1023u), f32((w >> 20u) & 1023u)) / 1023.0;
    return s * s * 4.0;
}

// Each drop as 5 words after a 4-word header holding the count: its place (fractions of the grid),
// colour, kind and temperature, serial, strength and dryness.
@compute @workgroup_size(256)
fn cs_record(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    let n = counts[4];
    if i == 0u {
        sample[0] = n;
        sample[1] = 0u;
        sample[2] = 0u;
        sample[3] = 0u;
    }
    if i >= n || 4u + (i + 1u) * SAMPLE_WORDS > arrayLength(&sample) {
        return;
    }
    let p = drops[i];
    let size = vec2<f32>(f32(st.grid.x), f32(st.grid.y));
    let at = 4u + i * SAMPLE_WORDS;
    sample[at] = pack2x16unorm(clamp(p.x / size, vec2<f32>(0.0), vec2<f32>(1.0)));
    sample[at + 1u] = colour_code(p.colour.rgb);
    sample[at + 2u] = pack2x16float(vec2<f32>(f32(p.kind & 1023u), p.spare.x));
    sample[at + 3u] = p.serial;
    sample[at + 4u] = pack2x16float(vec2<f32>(min(p.colour.w, 60000.0), min(p.spare.y, 60000.0)));
}

fn sample_word(old: bool, k: u32) -> u32 {
    if old {
        return older[k];
    }
    return newer[k];
}

fn sample_count(old: bool) -> u32 {
    if old {
        return min(older[0], (arrayLength(&older) - 4u) / SAMPLE_WORDS);
    }
    return min(newer[0], (arrayLength(&newer) - 4u) / SAMPLE_WORDS);
}

@compute @workgroup_size(256)
fn cs_rewind(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    let old = st.extra.w == 1u;
    let n = sample_count(old);
    if i >= n || i >= st.counts.y {
        return;
    }
    let t = bitcast<f32>(st.extra.z);
    let size = vec2<f32>(f32(st.grid.x), f32(st.grid.y));
    let at = 4u + i * SAMPLE_WORDS;
    var x = unpack2x16unorm(sample_word(old, at)) * size;
    var colour = colour_back(sample_word(old, at + 1u));
    let kt = unpack2x16float(sample_word(old, at + 2u));
    let kind = u32(kt.x + 0.5);
    let serial = sample_word(old, at + 3u);
    let more = unpack2x16float(sample_word(old, at + 4u));
    // The same drop in the other sample: slide between the two.
    if i < sample_count(!old) && sample_word(!old, at + 3u) == serial
        && u32(unpack2x16float(sample_word(!old, at + 2u)).x + 0.5) == kind {
        let x2 = unpack2x16unorm(sample_word(!old, at)) * size;
        let c2 = colour_back(sample_word(!old, at + 1u));
        // t runs from the newer sample (0) to the older (1).
        var from_x = x2;
        var from_c = c2;
        var to_x = x;
        var to_c = colour;
        if !old {
            from_x = x;
            from_c = colour;
            to_x = x2;
            to_c = c2;
        }
        x = mix(from_x, to_x, t);
        colour = mix(from_c, to_c, t);
    }
    var p: Drop;
    p.x = x;
    p.v = vec2<f32>(0.0);
    p.c = vec4<f32>(0.0);
    p.colour = vec4<f32>(colour, more.x);
    p.j = 0.0;
    p.kind = kind;
    p.serial = serial;
    p.kick = 0.0;
    p.mat = vec2<f32>(0.0);
    p.spare = vec2<f32>(kt.y, more.y);
    p.more = vec4<f32>(0.0);
    p.f = vec4<f32>(1.0, 0.0, 0.0, 1.0);
    // Balls still push it aside (its place only).
    let lq = st.liquids[liquid_of(p)];
    if st.flags.z > 0u && lq.b.w > 0.5 && !is_stain(p) {
        let range = ball_cell(p.x);
        for (var e = range.x; e < range.y; e = e + 1u) {
            let bi = words[st.ball_grid.w + e];
            let ba = st.words_at.z + bi * BALL_WORDS;
            if ((words[ba + 5u] >> liquid_of(p)) & 1u) == 0u {
                continue;
            }
            let bx = vec2<f32>(wf(ba), wf(ba + 1u));
            let r = wf(ba + 4u) + 0.5;
            let off = p.x - bx;
            let dist = length(off);
            if dist < r {
                var nn = vec2<f32>(0.0, -1.0);
                if dist > 1e-5 {
                    nn = off / dist;
                }
                p.x = bx + nn * r;
            }
        }
    }
    drops[i] = p;
}

@compute @workgroup_size(1)
fn cs_rewind_done() {
    let n = min(sample_count(st.extra.w == 1u), st.counts.y);
    counts[0] = 4u;
    counts[1] = n;
    counts[2] = 0u;
    counts[3] = 0u;
    counts[4] = n;
    counts[7] = 0u;
}

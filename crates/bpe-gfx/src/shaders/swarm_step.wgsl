// One particle step of a Huge swarm (particles part 8), dt 1/120 s: births appended, every
// particle moved (forces, the speed ceiling, balls, shapes, edges), then the living ones packed
// back in order by an integer prefix sum. No atomics and every particle written by one thread,
// so the same card always gives the same particles. The formulas are the exact engine's
// (bpe-kit `things/particles/forces.rs`, `meets.rs`, `step.rs`), written again here.

struct Force {
    // x: the kind (FORCE_*), y: Only for (0 all, 1 younger than, 2 older than, 3 a share, 4 the
    // rest), z and w: the force's own words
    kind: vec4<u32>,
    // x: amount, y: age Only for goes by, z: the share it goes by, w: the force's own number
    a: vec4<f32>,
    b: vec4<f32>,
    c: vec4<f32>,
};

struct Step {
    // x: the particle step being taken, y: births uploaded, z: most particles, w: a crowd's count
    // (0: not a crowd)
    counts: vec4<u32>,
    // x: forces, y: the most particles there can be (what was dispatched), z: flags (STEP_*),
    // w: at balls | at shapes << 4 | at edges << 8
    more: vec4<u32>,
    // x: seconds since the start, y: dt, z: the speed ceiling, w: balls' reach (radii)
    time: vec4<f32>,
    // x: balls push, y: ball bounciness, z: shape bounciness, w: friction
    meets: vec4<f32>,
    // the picture's edges: lo xy, hi xy
    edges: vec4<f32>,
    // the ball grid: origin xy, cell, 0
    balls: vec4<f32>,
    // cols, rows, where the balls start in `words`, where the cells' starts are
    ball_at: vec4<u32>,
    // where the cells' items are, how many balls
    ball_more: vec4<u32>,
    // the wall grid: origin xy, cell, 0
    walls: vec4<f32>,
    // cols, rows, where the segments start, where the cells' starts are
    wall_at: vec4<u32>,
    // where the cells' items are, how many segments
    wall_more: vec4<u32>,
    // the air: cell size, its lattice origin (cells) x and y, 0
    air: vec4<f32>,
    // cols, rows, where its velocities start, awake
    air_at: vec4<u32>,
    // the ambient wind: way xy, speed, gusts
    wind: vec4<f32>,
    // gust every, gust waves, the air's time, 0
    gust: vec4<f32>,
    // the thing's middle xy, the particle clock's time at the step's end, 0
    middle: vec4<f32>,
    // where Its ball goes when the particle's ball is gone: place xy, velocity xy
    parent: vec4<f32>,
    // the homes: centre xy, the way their x axis points xy
    homes: vec4<f32>,
    // where the parent's balls start in `words` (5 words each: place, velocity, alive), how many,
    // where the homes' places start (2 words each), how many
    lists: vec4<u32>,
    // the forces' ball grid (Balls nearby, magnets): origin xy, cell, reach (radii)
    fballs: vec4<f32>,
    // cols, rows, where its balls start, where its cells' starts are
    fball_at: vec4<u32>,
    // where its cells' items are, how many balls
    fball_more: vec4<u32>,
    forces: array<Force, 16>,
};

struct Main {
    place: vec2<f32>,
    velocity: vec2<f32>,
    born: u32,
    life: f32,
    seed: u32,
    turn: f32,
};

struct More {
    home: vec2<f32>,
    size: f32,
    spin: f32,
    flip: f32,
    ball: u32,
    spot: u32,
    spare: u32,
};

const FORCE_GRAVITY: u32 = 1u;
const FORCE_WIND: u32 = 2u;
const FORCE_DRAG: u32 = 3u;
const FORCE_SWIRL: u32 = 4u;
const FORCE_WANDER: u32 = 5u;
const FORCE_RISE: u32 = 6u;
const FORCE_SWAY: u32 = 7u;
const FORCE_AIR: u32 = 8u;
const FORCE_PULL: u32 = 9u;
const FORCE_SPRING: u32 = 10u;
const FORCE_MAGNETIC: u32 = 11u;
const FORCE_FLOW: u32 = 12u;
const FORCE_SPIROGRAPH: u32 = 13u;
const FORCE_PULSE: u32 = 14u;
const FORCE_RIPPLE: u32 = 15u;
const PI: f32 = 3.14159265;

const STEP_CROWD: u32 = 1u;
const STEP_BALLS: u32 = 2u;
const STEP_WALLS: u32 = 4u;
const STEP_EDGES: u32 = 8u;
const STEP_AIR: u32 = 16u;
// Births only, nothing moved (the world's build).
const STEP_STILL: u32 = 32u;

const TAU: f32 = 6.2831853;
const WG: u32 = 256u;
const MOST_NEAR: u32 = 32u;
const SPECK_RADIUS: f32 = 0.5;

@group(0) @binding(0) var<uniform> st: Step;
@group(0) @binding(1) var<storage, read> words: array<u32>;
@group(0) @binding(2) var<storage, read> births_main: array<Main>;
@group(0) @binding(3) var<storage, read> births_more: array<More>;
@group(0) @binding(4) var<storage, read_write> main_a: array<Main>;
@group(0) @binding(5) var<storage, read_write> more_a: array<More>;
@group(0) @binding(6) var<storage, read_write> main_b: array<Main>;
@group(0) @binding(7) var<storage, read_write> more_b: array<More>;
// The keep flags, then each one's place among the kept of its workgroup (from `bound`).
@group(0) @binding(8) var<storage, read_write> flags: array<u32>;
@group(0) @binding(9) var<storage, read_write> sums: array<u32>;
// [4, count, 0, 0] (the draw's arguments), the step's working count, three spare.
@group(0) @binding(10) var<storage, read_write> counts: array<u32>;

var<workgroup> shared_sums: array<u32, 256>;

// ---------- the exact engine's dice and noise ----------

fn mix_in(x0: u32, p: u32) -> u32 {
    var x = x0 ^ (p * 0x85ebca6bu);
    x = ((x << 13u) | (x >> 19u)) * 0xc2b2ae35u;
    return x ^ (x >> 16u);
}

fn finish(x0: u32) -> u32 {
    var x = x0 ^ (x0 >> 15u);
    x = x * 0x2c1b3c6du;
    return x ^ (x >> 12u);
}

fn hash2(a: u32, b: u32) -> u32 {
    return finish(mix_in(mix_in(0x9e3779b9u, a), b));
}

fn hash3(a: u32, b: u32, c: u32) -> u32 {
    return finish(mix_in(mix_in(mix_in(0x9e3779b9u, a), b), c));
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = p - i;
    let ix = i32(i.x);
    let iy = i32(i.y);
    let c00 = f32(hash3(bitcast<u32>(ix), bitcast<u32>(iy), 0x51u) >> 8u) / 8388608.0 - 1.0;
    let c10 = f32(hash3(bitcast<u32>(ix + 1), bitcast<u32>(iy), 0x51u) >> 8u) / 8388608.0 - 1.0;
    let c01 = f32(hash3(bitcast<u32>(ix), bitcast<u32>(iy + 1), 0x51u) >> 8u) / 8388608.0 - 1.0;
    let c11 = f32(hash3(bitcast<u32>(ix + 1), bitcast<u32>(iy + 1), 0x51u) >> 8u) / 8388608.0 - 1.0;
    let u = f.x * f.x * f.x * (f.x * (f.x * 6.0 - 15.0) + 10.0);
    let v = f.y * f.y * f.y * (f.y * (f.y * 6.0 - 15.0) + 10.0);
    let top = c00 + (c10 - c00) * u;
    let bottom = c01 + (c11 - c01) * u;
    return top + (bottom - top) * v;
}

fn curl(at: vec2<f32>) -> vec2<f32> {
    let h = 0.25;
    let dx = (noise(at + vec2<f32>(h, 0.0)) - noise(at - vec2<f32>(h, 0.0))) / (2.0 * h);
    let dy = (noise(at + vec2<f32>(0.0, h)) - noise(at - vec2<f32>(0.0, h))) / (2.0 * h);
    return vec2<f32>(dy, -dx) * 0.5;
}

fn gust_factor(gusts: f32, every: f32, waves: f32, time: f32, place: vec2<f32>, way: vec2<f32>) -> f32 {
    if gusts == 0.0 {
        return 1.0;
    }
    var phase = time / max(every, 0.01);
    if waves > 0.0 {
        phase = phase - dot(place, way) / waves;
    }
    return 1.0 + gusts * noise(vec2<f32>(phase, 3.7));
}

fn from_angle(a: f32) -> vec2<f32> {
    return vec2<f32>(cos(a), sin(a));
}

fn word_f(i: u32) -> f32 {
    return bitcast<f32>(words[i]);
}

fn word_v(i: u32) -> vec2<f32> {
    return vec2<f32>(bitcast<f32>(words[i]), bitcast<f32>(words[i + 1u]));
}

fn finite2(v: vec2<f32>) -> bool {
    // NaN fails both, an infinity the second.
    return all(v == v) && all(abs(v) < vec2<f32>(3.0e38));
}

// ---------- the air (bpe-kit `air.rs`, `Air::at`) ----------

fn air_cell(c: u32, r: u32) -> vec2<f32> {
    return word_v(st.air_at.z + 2u * (r * st.air_at.x + c));
}

fn air_at(p: vec2<f32>) -> vec2<f32> {
    let cols = st.air_at.x;
    let rows = st.air_at.y;
    let g = p / st.air.x - st.air.yz;
    if st.air_at.w != 0u && cols > 0u && g.x >= 0.0 && g.y >= 0.0 && g.x <= f32(cols) && g.y <= f32(rows) {
        let x = clamp(g.x - 0.5, 0.0, f32(cols - 1u));
        let y = clamp(g.y - 0.5, 0.0, f32(rows - 1u));
        let c0 = u32(floor(x));
        let r0 = u32(floor(y));
        let c1 = min(c0 + 1u, cols - 1u);
        let r1 = min(r0 + 1u, rows - 1u);
        let fx = x - f32(c0);
        let fy = y - f32(r0);
        let top = air_cell(c0, r0) * (1.0 - fx) + air_cell(c1, r0) * fx;
        let bottom = air_cell(c0, r1) * (1.0 - fx) + air_cell(c1, r1) * fx;
        return top * (1.0 - fy) + bottom * fy;
    }
    let way = st.wind.xy;
    return way * st.wind.z * gust_factor(st.wind.w, st.gust.x, st.gust.y, st.gust.z, p, way);
}

// ---------- particles part 3's forces (`more_forces.rs`) ----------

fn unit(seed: u32, k: u32) -> f32 {
    return f32(hash2(seed, k) >> 8u) / 16777216.0;
}

fn signed(seed: u32, k: u32) -> f32 {
    return unit(seed, k) * 2.0 - 1.0;
}

fn rem_tau(x: f32) -> f32 {
    return x - TAU * floor(x / TAU);
}

fn perp(v: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(-v.y, v.x);
}

// `v` made a unit long, or zero when it can't be.
fn unit_or_zero(v: vec2<f32>) -> vec2<f32> {
    let d = length(v);
    if d > 0.0 && finite2(v / d) {
        return v / d;
    }
    return vec2<f32>(0.0);
}

// The parent's ball of a particle (body row + 1): place xy, velocity xy, else the parent's middle.
fn its_ball(o: ptr<function, More>) -> vec4<f32> {
    if (*o).ball >= 1u && (*o).ball <= st.lists.y {
        let base = st.lists.x + 5u * ((*o).ball - 1u);
        if words[base + 4u] != 0u {
            return vec4<f32>(word_v(base), word_v(base + 2u));
        }
    }
    return st.parent;
}

// A fixed-centre force's centre for this particle (`centre_for`): mode 0 the step's (at word
// `at`: place xy, velocity zw), 2 the particle's ball.
fn centre_of(o: ptr<function, More>, mode: u32, at: u32) -> vec4<f32> {
    if mode == 2u {
        return its_ball(o);
    }
    return vec4<f32>(word_v(at), word_v(at + 2u));
}

fn home_of(o: ptr<function, More>) -> vec2<f32> {
    let spot = (*o).spot;
    if spot >= 1u && spot <= st.lists.w {
        let p = word_v(st.lists.z + 2u * (spot - 1u));
        let way = st.homes.zw;
        return st.homes.xy + vec2<f32>(way.x * p.x - way.y * p.y, way.y * p.x + way.x * p.y);
    }
    return (*o).home;
}

// The forces' ball grid's balls near `p`: the first and end of their items.
fn fballs_near(p: vec2<f32>) -> vec2<u32> {
    let cols = st.fball_at.x;
    let rows = st.fball_at.y;
    if cols == 0u || !finite2(p) {
        return vec2<u32>(0u);
    }
    let rel = (p - st.fballs.xy) / st.fballs.z;
    if !(rel.x >= 0.0 && rel.y >= 0.0 && rel.x < f32(cols) && rel.y < f32(rows)) {
        return vec2<u32>(0u);
    }
    let cell = u32(rel.y) * cols + u32(rel.x);
    let a = words[st.fball_at.w + cell];
    return vec2<u32>(a, min(words[st.fball_at.w + cell + 1u], a + MOST_NEAR));
}

// Ball `item` of the forces' grid: place, velocity, radius.
fn fball(item: u32) -> array<f32, 5> {
    let base = st.fball_at.z + 5u * words[st.fball_more.x + item];
    return array<f32, 5>(word_f(base), word_f(base + 1u), word_f(base + 2u), word_f(base + 3u), word_f(base + 4u));
}

// `pull_from`: b = [strength, at, reach, orbit], c = [swallow, ball reach, falloff, 0], a.w the
// sign. True when swallowed.
fn pull_from(m: ptr<function, Main>, f: Force, centre: vec2<f32>, at0: f32, reach: f32, amount: f32) -> bool {
    let sgn = f.a.w;
    let swallow = f.c.x;
    let off = centre - (*m).place;
    let d = length(off);
    if sgn > 0.0 && swallow > 0.0 && d < swallow {
        return true;
    }
    if (reach > 0.0 && d > reach) || d < 1e-6 || !(d == d && d < 3.0e38) {
        return false;
    }
    let inward = off / d;
    let soft = max(max(d, 4.0), swallow);
    let at = max(at0, 1e-3);
    var factor = 1.0;
    if f.c.z == 1.0 {
        factor = at / soft;
    } else if f.c.z == 2.0 {
        factor = (at / soft) * (at / soft);
    }
    factor = min(factor, 100.0);
    let a = f.b.x * factor * amount;
    (*m).velocity = (*m).velocity + (inward * sgn + perp(inward) * f.b.w) * a * st.time.y;
    return false;
}

// Pull to and Push away from: kind.z the mode (0 the step's centre at word kind.w, 1 none, 2 its
// ball, 3 balls nearby). True when swallowed.
fn pull(m: ptr<function, Main>, o: ptr<function, More>, f: Force, amount: f32) -> bool {
    let mode = f.kind.z;
    if mode == 1u {
        return false;
    }
    if mode == 3u {
        if f.c.y <= 0.0 {
            return false;
        }
        let near = fballs_near((*m).place);
        for (var k = near.x; k < near.y; k = k + 1u) {
            let b = fball(k);
            let r = b[4];
            if pull_from(m, f, vec2<f32>(b[0], b[1]), max(r, 1.0), max(f.c.y, 0.0) * r, amount) {
                return true;
            }
        }
        return false;
    }
    let c = centre_of(o, mode, f.kind.w);
    return pull_from(m, f, c.xy, f.b.y, max(f.b.z, 0.0), amount);
}

// A magnet at `centre` pointing `mm`, full strength within `size`, at `p` (`dipole`): the
// field's way × strength in xy and the pull × strength in zw; w of `ok` false when none.
fn dipole(p: vec2<f32>, centre: vec2<f32>, mm: vec2<f32>, size: f32) -> vec4<f32> {
    let r = p - centre;
    let d = length(r);
    if !(d > 1e-6 && d < 4.0 * size && size > 0.0) {
        return vec4<f32>(0.0);
    }
    let n = r / d;
    let strength = min(pow(size / d, 3.0), 1.0);
    let b = unit_or_zero(n * 3.0 * dot(mm, n) - mm);
    if all(b == vec2<f32>(0.0)) {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(b * strength, -n * strength);
}

// Magnetic field: kind.z from (0 each ball, 1 its place), kind.w points (0 along its motion,
// 1 a way); b = [the way xy, size (radii), size (px)], c = [line up, pull].
fn magnetic(m: ptr<function, Main>, f: Force, amount: f32) {
    let fixed = f.b.xy;
    var field = vec2<f32>(0.0);
    var toward = vec2<f32>(0.0);
    if f.kind.z == 0u {
        let near = fballs_near((*m).place);
        for (var k = near.x; k < near.y; k = k + 1u) {
            let b = fball(k);
            var mm = fixed;
            if f.kind.w == 0u {
                mm = unit_or_zero(vec2<f32>(b[2], b[3]));
                if all(mm == vec2<f32>(0.0)) {
                    mm = vec2<f32>(0.0, -1.0);
                }
            }
            let got = dipole((*m).place, vec2<f32>(b[0], b[1]), mm, max(f.b.z, 0.0) * b[4]);
            field = field + got.xy;
            toward = toward + got.zw;
        }
    } else {
        let got = dipole((*m).place, st.middle.xy, fixed, max(f.b.w, 0.0));
        field = got.xy;
        toward = got.zw;
    }
    let strength = min(length(field), 1.0);
    if strength > 1e-4 {
        let want = atan2(field.y, field.x);
        var gap = rem_tau(want - (*m).turn + PI) - PI;
        if abs(gap) > PI * 0.5 {
            gap = gap - PI * sign(gap);
        }
        let rate = min(max(f.c.x, 0.0) * (strength + 0.06) * amount * st.time.y, 1.0);
        (*m).turn = rem_tau((*m).turn + gap * rate + PI) - PI;
    }
    (*m).velocity = (*m).velocity + toward * f.c.y * amount * st.time.y;
}

// Flow field: kind.z the pattern (0 noise, 1 spiral, 2 waves, 3 vortex); b = [speed, size, grip,
// changes], c = [angle, pull in, sway] in radians.
fn flow(m: ptr<function, Main>, f: Force, amount: f32) {
    let t = st.middle.z;
    let size = max(f.b.y, 1.0);
    var way = vec2<f32>(0.0);
    switch f.kind.z {
        case 0u: {
            way = from_angle(noise((*m).place / size + vec2<f32>(0.0, t * f.b.w)) * TAU * 2.0);
        }
        case 2u: {
            let base = f.c.x;
            let across = dot((*m).place, perp(from_angle(base)));
            way = from_angle(base + f.c.z * sin(TAU * (across / size - t * f.b.w)));
        }
        default: {
            let off = (*m).place - st.middle.xy;
            let d = length(off);
            if d < 1e-6 {
                return;
            }
            let around = perp(off) / d;
            if f.kind.z == 1u {
                way = around * cos(f.c.y) - off / d * sin(f.c.y);
            } else {
                way = around * select(size / d, d / size, d < size);
            }
        }
    }
    let take = 1.0 - exp(-max(f.b.z, 0.0) * amount * st.time.y);
    (*m).velocity = (*m).velocity + (way * f.b.x - (*m).velocity) * take;
}

// Spirograph: kind.z the centre's mode (as `pull`), kind.w its word; b = [ring, wheel, pen,
// speed], c = [pens differ].
fn spirograph(m: ptr<function, Main>, o: ptr<function, More>, f: Force, amount: f32, age: f32) {
    if f.kind.z == 1u {
        return;
    }
    let c = centre_of(o, f.kind.z, f.kind.w);
    let dt = st.time.y;
    let seed = (*m).seed;
    let differ = f.c.x;
    let big = f.b.x * (1.0 + differ * signed(seed, 70u));
    let small = max(abs(f.b.y * (1.0 + differ * signed(seed, 71u))), 1.0);
    let pen = f.b.z * (1.0 + differ * signed(seed, 72u));
    let theta = TAU * (f.b.w * (age + dt) + unit(seed, 73u));
    let k = (big - small) / small;
    let at = vec2<f32>(
        (big - small) * cos(theta) + pen * cos(k * theta),
        (big - small) * sin(theta) - pen * sin(k * theta),
    );
    let aim = c.xy + c.zw * dt + at;
    let follow = (aim - (*m).place) / dt;
    if finite2(follow) {
        (*m).velocity = mix((*m).velocity, follow, clamp(amount, 0.0, 1.0));
    }
}

// Pulse on the beat: kind.z the way (0 outwards, 1 up, 2 a way, 3 all round); b = [strength,
// this step's share of the kicks, the way xy].
fn pulse(m: ptr<function, Main>, f: Force, amount: f32) {
    if f.b.y == 0.0 {
        return;
    }
    var way = f.b.zw;
    switch f.kind.z {
        case 0u: {
            way = unit_or_zero((*m).place - st.middle.xy);
            if all(way == vec2<f32>(0.0)) {
                way = vec2<f32>(0.0, -1.0);
            }
        }
        case 1u: {
            way = vec2<f32>(0.0, -1.0);
        }
        case 3u: {
            way = from_angle(unit((*m).seed, 74u) * TAU);
        }
        default: {}
    }
    (*m).velocity = (*m).velocity + way * f.b.x * amount * f.b.y;
}

// Ripple from hits: kind.z rings at word kind.w (4 words each: centre, age, impact); b = [speed,
// width, strength, lasts].
fn ripple(m: ptr<function, Main>, f: Force, amount: f32) {
    let speed = max(f.b.x, 1.0);
    let width = max(f.b.y, 1.0);
    let lasts = max(f.b.w, 0.01);
    for (var k = 0u; k < f.kind.z; k = k + 1u) {
        let base = f.kind.w + 4u * k;
        let off = (*m).place - word_v(base);
        let d = length(off);
        let age = word_f(base + 2u);
        let x = abs(d - speed * age) / width;
        if x >= 1.0 || d < 1e-6 {
            continue;
        }
        let fade = max(1.0 - age / lasts, 0.0);
        let bump = (1.0 - x * x) * (1.0 - x * x);
        (*m).velocity = (*m).velocity + off / d * f.b.z * word_f(base + 3u) * fade * bump * amount * st.time.y;
    }
}

// ---------- forces (`Forces::apply`) ----------

// True when the particle is swallowed.
fn apply_forces(m: ptr<function, Main>, o: ptr<function, More>, age: f32) -> bool {
    let dt = st.time.y;
    for (var k = 0u; k < min(st.more.x, 16u); k = k + 1u) {
        let f = st.forces[k];
        let amount = f.a.x;
        if amount == 0.0 {
            continue;
        }
        if (f.kind.y == 1u && !(age < f.a.y)) || (f.kind.y == 2u && !(age >= f.a.y)) {
            continue;
        }
        let dice = unit((*m).seed, 61u);
        if (f.kind.y == 3u && !(dice < f.a.z)) || (f.kind.y == 4u && !(dice >= f.a.z)) {
            continue;
        }
        switch f.kind.x {
            case 1u: {
                (*m).velocity = (*m).velocity + f.b.xy * amount * dt;
            }
            case 2u: {
                let way = f.b.xy;
                let gust = gust_factor(f.c.x, f.c.y, f.c.z, st.time.x, (*m).place, way);
                let wind = way * f.b.z * gust;
                let take = 1.0 - exp(-max(f.b.w, 0.0) * amount * dt);
                (*m).velocity = (*m).velocity + (wind - (*m).velocity) * take;
            }
            case 3u: {
                (*m).velocity = (*m).velocity * exp(-max(f.b.x, 0.0) * amount * dt);
            }
            case 4u: {
                let size = max(f.b.y, 1.0);
                let at = ((*m).place + vec2<f32>(0.0, f.b.z * st.time.x)) / size;
                (*m).velocity = (*m).velocity + curl(at) * f.b.x * amount * dt;
            }
            case 5u: {
                let phase = age * max(f.b.y, 0.0);
                let kk = f32((*m).seed & 0xffffu) * 0.013;
                let a = noise(vec2<f32>(phase, kk)) * TAU * 1.5;
                (*m).velocity = (*m).velocity + from_angle(a) * f.b.x * amount * dt;
            }
            case 6u: {
                var cool = 1.0;
                if f.b.y > 0.0 {
                    cool = exp(-age / f.b.y);
                }
                (*m).velocity.y = (*m).velocity.y - f.b.x * cool * amount * dt;
            }
            case 7u: {
                let omega = TAU * max(f.b.y, 0.0);
                let phase0 = f32(hash2((*m).seed, 0x5a7u) >> 8u) / 16777216.0 * TAU;
                let v1 = f.b.x * omega * cos(omega * age + phase0);
                let v0 = f.b.x * omega * cos(omega * max(age - dt, 0.0) + phase0);
                (*m).velocity.x = (*m).velocity.x + (v1 - v0) * amount;
                if f.b.z > 0.0 {
                    (*o).flip = (*o).flip + TAU * f.b.z * amount * dt;
                    (*m).velocity.x = (*m).velocity.x + f.b.w * sin((*o).flip) * amount * dt * 4.0;
                }
            }
            case 8u: {
                if (st.more.z & STEP_AIR) != 0u {
                    let take = 1.0 - exp(-max(f.b.x, 0.0) * amount * dt);
                    (*m).velocity = (*m).velocity + (air_at((*m).place) - (*m).velocity) * take;
                }
            }
            case 9u: {
                if pull(m, o, f, amount) {
                    return true;
                }
            }
            case 10u: {
                let home = home_of(o);
                (*m).velocity = (*m).velocity + (home - (*m).place) * max(f.b.x, 0.0) * amount * dt;
                (*m).velocity = (*m).velocity * exp(-max(f.b.y, 0.0) * amount * dt);
            }
            case 11u: {
                magnetic(m, f, amount);
            }
            case 12u: {
                flow(m, f, amount);
            }
            case 13u: {
                spirograph(m, o, f, amount, age);
            }
            case 14u: {
                pulse(m, f, amount);
            }
            case 15u: {
                ripple(m, f, amount);
            }
            default: {}
        }
    }
    return false;
}

// ---------- meeting balls (`meet_balls`) ----------

// True when the particle vanishes.
fn meet_balls(m: ptr<function, Main>, size: f32) -> bool {
    let cols = st.ball_at.x;
    let rows = st.ball_at.y;
    if cols == 0u {
        return false;
    }
    let rel = ((*m).place - st.balls.xy) / st.balls.z;
    if !(rel.x >= 0.0 && rel.y >= 0.0 && rel.x < f32(cols) && rel.y < f32(rows)) {
        return false;
    }
    let cell = u32(rel.y) * cols + u32(rel.x);
    let a = words[st.ball_at.w + cell];
    let b = min(words[st.ball_at.w + cell + 1u], a + MOST_NEAR);
    let at_balls = st.more.w & 15u;
    let push = st.meets.x;
    let dt = st.time.y;
    for (var k = a; k < b; k = k + 1u) {
        let i = words[st.ball_more.x + k];
        let base = st.ball_at.z + 5u * i;
        let place = word_v(base);
        let velocity = word_v(base + 2u);
        let radius = word_f(base + 4u);
        let off = (*m).place - place;
        let d = length(off);
        let reach = radius * st.time.w;
        if push != 0.0 && d < reach {
            let near = 1.0 - d / max(reach, 1e-3);
            let w = push * near * near;
            let take = 1.0 - exp(-12.0 * w * dt);
            (*m).velocity = (*m).velocity + (velocity - (*m).velocity) * take;
            if d < radius {
                var out = vec2<f32>(0.0, -1.0);
                if d > 1e-6 {
                    out = off / d;
                }
                (*m).velocity = (*m).velocity + out * length(velocity) * push * take;
            }
        }
        let touch = radius + max(size, 0.0);
        if d < touch {
            if at_balls == 2u {
                return true;
            }
            if at_balls == 1u {
                var n = vec2<f32>(0.0, -1.0);
                if d > 1e-6 {
                    n = off / d;
                }
                (*m).place = place + n * touch;
                let rv = (*m).velocity - velocity;
                let into = dot(rv, n);
                if into < 0.0 {
                    (*m).velocity = velocity + rv - n * into * (1.0 + clamp(st.meets.y, 0.0, 1.0));
                }
            }
        }
    }
    return false;
}

// ---------- meeting shapes (segments of each wall's outline, now and at the last step) ----------

// True when the particle vanishes.
fn meet_segment(m: ptr<function, Main>, q: vec2<f32>, s: u32) -> bool {
    let base = st.wall_at.z + 9u * s;
    let a0 = word_v(base);
    let b0 = word_v(base + 2u);
    let a1 = word_v(base + 4u);
    let b1 = word_v(base + 6u);
    let half = word_f(base + 8u);
    let d0 = b0 - a0;
    let d1 = b1 - a1;
    let l0 = dot(d0, d0);
    let l1 = dot(d1, d1);
    if l1 < 1e-12 || l0 < 1e-12 {
        return false;
    }
    let n0 = vec2<f32>(-d0.y, d0.x) / sqrt(l0);
    let n1 = vec2<f32>(-d1.y, d1.x) / sqrt(l1);
    // Where it was, as it sat against the segment then, put against the segment now.
    let u0 = dot(q - a0, d0) / l0;
    let hq = dot(q - a0, n0);
    let qn = a1 + d1 * u0 + n1 * hq;
    let p = (*m).place;
    let hp = dot(p - a1, n1);
    let up = dot(p - a1, d1) / l1;
    let band = half + SPECK_RADIUS;
    var hit = false;
    if (hq < 0.0) != (hp < 0.0) && hq != hp {
        let k = hq / (hq - hp);
        let x = qn + (p - qn) * k;
        let ux = dot(x - a1, d1) / l1;
        hit = ux >= 0.0 && ux <= 1.0;
    }
    if !hit && abs(hp) < band && up >= 0.0 && up <= 1.0 {
        hit = true;
    }
    if !hit {
        return false;
    }
    var side = 1.0;
    if hq < 0.0 || (hq == 0.0 && hp < 0.0) {
        side = -1.0;
    }
    let n = n1 * side;
    let at = clamp(up, 0.0, 1.0);
    let wall_v = ((a1 + d1 * at) - (a0 + d0 * at)) / st.time.y;
    (*m).place = p - n1 * hp + n * band;
    let shapes = (st.more.w >> 4u) & 15u;
    if shapes == 3u {
        return true;
    }
    if shapes == 2u {
        (*m).velocity = wall_v;
    }
    if shapes == 1u {
        let rv = (*m).velocity - wall_v;
        let into = dot(rv, n);
        let normal = n * into;
        let sideways = (rv - normal) * (1.0 - clamp(st.meets.w, 0.0, 1.0));
        var out = normal;
        if into < 0.0 {
            out = -normal * clamp(st.meets.z, 0.0, 1.0);
        }
        (*m).velocity = wall_v + sideways + out;
    }
    return false;
}

fn wall_cell(p: vec2<f32>) -> i32 {
    let rel = (p - st.walls.xy) / st.walls.z;
    if !(rel.x >= 0.0 && rel.y >= 0.0 && rel.x < f32(st.wall_at.x) && rel.y < f32(st.wall_at.y)) {
        return -1;
    }
    return i32(u32(rel.y) * st.wall_at.x + u32(rel.x));
}

fn meet_cell(m: ptr<function, Main>, q: vec2<f32>, cell: i32) -> bool {
    if cell < 0 {
        return false;
    }
    let c = u32(cell);
    let a = words[st.wall_at.w + c];
    let b = words[st.wall_at.w + c + 1u];
    for (var k = a; k < b; k = k + 1u) {
        if meet_segment(m, q, words[st.wall_more.x + k]) {
            return true;
        }
    }
    return false;
}

// ---------- the picture's edges (`meet_edges`) ----------

fn meet_edges(m: ptr<function, Main>) -> bool {
    let lo = st.edges.xy;
    let hi = st.edges.zw;
    let mode = (st.more.w >> 8u) & 15u;
    let p = (*m).place;
    if mode == 3u {
        return p.x < lo.x || p.y < lo.y || p.x > hi.x || p.y > hi.y;
    }
    if mode == 1u {
        let size = max(hi - lo, vec2<f32>(1.0));
        let r = p - lo;
        (*m).place = lo + r - floor(r / size) * size;
    }
    if mode == 2u {
        var q = p;
        var v = (*m).velocity;
        for (var k = 0; k < 2; k = k + 1) {
            if q[k] < lo[k] {
                q[k] = lo[k] + min(lo[k] - q[k], hi[k] - lo[k]);
                v[k] = abs(v[k]);
            } else if q[k] > hi[k] {
                q[k] = hi[k] - min(q[k] - hi[k], hi[k] - lo[k]);
                v[k] = -abs(v[k]);
            }
        }
        (*m).place = q;
        (*m).velocity = v;
    }
    return false;
}

// ---------- the passes ----------

// Births go after the living particles, as many as fit under Most particles (and, for a crowd,
// as many as it is short of).
@compute @workgroup_size(256)
fn cs_births(@builtin(global_invocation_id) id: vec3<u32>) {
    let count = counts[1];
    let cap = st.counts.z;
    var used = min(st.counts.y, cap - min(count, cap));
    if (st.more.z & STEP_CROWD) != 0u {
        used = min(used, st.counts.w - min(count, st.counts.w));
    }
    let j = id.x;
    if j < used {
        main_a[count + j] = births_main[j];
        more_a[count + j] = births_more[j];
    }
    if j == 0u {
        counts[4] = count + used;
    }
}

@compute @workgroup_size(256)
fn cs_move(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= counts[4] {
        return;
    }
    var m = main_a[i];
    var o = more_a[i];
    if (st.more.z & STEP_STILL) != 0u {
        main_b[i] = m;
        more_b[i] = o;
        flags[i] = 1u;
        return;
    }
    let now = st.counts.x;
    let age = f32(now - min(m.born, now)) * st.time.y;
    var keep = age < m.life;
    if keep {
        let q = m.place;
        if apply_forces(&m, &o, age) {
            keep = false;
        }
        let speed = length(m.velocity);
        if speed > st.time.z {
            m.velocity = m.velocity * (st.time.z / speed);
        }
        m.place = m.place + m.velocity * st.time.y;
        m.turn = m.turn + o.spin * st.time.y;
        if (st.more.z & STEP_BALLS) != 0u && meet_balls(&m, o.size) {
            keep = false;
        }
        if keep && (st.more.z & STEP_WALLS) != 0u {
            let c0 = wall_cell(m.place);
            let c1 = wall_cell(q);
            if meet_cell(&m, q, c0) || (c1 != c0 && meet_cell(&m, q, c1)) {
                keep = false;
            }
        }
        if keep && (st.more.z & STEP_EDGES) != 0u && meet_edges(&m) {
            keep = false;
        }
        let ok = finite2(m.place) && finite2(m.velocity) && finite2(vec2<f32>(m.turn, o.flip));
        keep = keep && ok;
    }
    main_b[i] = m;
    more_b[i] = o;
    flags[i] = select(0u, 1u, keep);
}

// Each workgroup's keep flags turned into each one's place among its kept, and its total.
@compute @workgroup_size(256)
fn cs_scan_local(
    @builtin(global_invocation_id) id: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let n = counts[4];
    let i = id.x;
    var x = 0u;
    if i < n {
        x = flags[i];
    }
    shared_sums[lid.x] = x;
    workgroupBarrier();
    // Hillis and Steele: an inclusive prefix in eight rounds.
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
        flags[st.more.y + i] = shared_sums[lid.x] - x;
    }
    if lid.x == WG - 1u {
        sums[wid.x] = shared_sums[lid.x];
    }
}

// One workgroup turns the totals into where each workgroup's kept start; the new count.
@compute @workgroup_size(256)
fn cs_scan_sums(@builtin(local_invocation_id) lid: vec3<u32>) {
    let groups = (counts[4] + WG - 1u) / WG;
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
        counts[0] = 4u;
        counts[1] = shared_sums[lid.x];
        counts[2] = 0u;
        counts[3] = 0u;
    }
}

// The kept particles back in order.
@compute @workgroup_size(256)
fn cs_scatter(@builtin(global_invocation_id) id: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>) {
    let i = id.x;
    if i >= counts[4] || flags[i] == 0u {
        return;
    }
    let dst = sums[wid.x] + flags[st.more.y + i];
    main_a[dst] = main_b[i];
    more_a[dst] = more_b[i];
}

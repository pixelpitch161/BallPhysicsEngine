// Liquid on the graphics card: position-based "double density relaxation" with a grid built by
// counting sort. Every sum over neighbours is done in whole numbers, so it comes out the same
// whatever order the GPU visits them in, which keeps every run identical.

// Matches `LiquidParams` in flow/liquid.rs.
struct Params {
    // x: particles, y: grid cells across, z: grid cells down, w: scan blocks
    counts: vec4<u32>,
    // x: spacing, y: reach of each particle (h), z: rest density, w: grid cell size
    shape: vec4<f32>,
    // x: stiffness, y: near stiffness, z: viscosity, w: ball push
    motion: vec4<f32>,
    // x: gravity scale, y: steps over which the liquid pours in, z: substeps, w: substep length
    more: vec4<f32>,
    // xy: spout, z: spout width, w: bounciness off the wall
    spout: vec4<f32>,
};

@group(0) @binding(0) var<uniform> step: Step;
@group(1) @binding(0) var<uniform> params: Params;
@group(1) @binding(1) var<storage, read_write> state: array<vec4<f32>>;
@group(1) @binding(2) var<storage, read_write> predicted: array<vec4<f32>>;
@group(1) @binding(3) var<storage, read_write> relaxed: array<vec4<f32>>;
@group(1) @binding(4) var<storage, read_write> pressure: array<vec2<f32>>;
@group(1) @binding(5) var<storage, read_write> cell_count: array<atomic<u32>>;
@group(1) @binding(6) var<storage, read_write> cell_start: array<u32>;
@group(1) @binding(7) var<storage, read_write> sorted: array<u32>;
@group(1) @binding(8) var<storage, read_write> block_sums: array<u32>;
@group(2) @binding(0) var<uniform> sdf_info: SdfInfo;
@group(2) @binding(1) var sdf_texture: texture_2d<f32>;

// Whole-number steps for the sums: 1/65536 of a density unit, 1/4096 of a pixel.
const DENSITY_SCALE: f32 = 65536.0;
const MOVE_SCALE: f32 = 4096.0;

fn cells() -> u32 {
    return params.counts.y * params.counts.z;
}

// Whether particle i has poured in yet.
fn poured(i: u32) -> bool {
    return step.a.z >= floor(f32(i) / f32(params.counts.x) * params.more.y);
}

fn cell_xy(p: vec2<f32>) -> vec2<i32> {
    let c = vec2<i32>(floor((p + step.b.zw) / params.shape.w));
    return clamp(c, vec2<i32>(0), vec2<i32>(params.counts.yz) - vec2<i32>(1));
}

fn cell_index(c: vec2<i32>) -> u32 {
    return u32(c.y) * params.counts.y + u32(c.x);
}

@compute @workgroup_size(256)
fn predict(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= params.counts.x {
        return;
    }
    let s = state[i];
    // Liquid still to pour waits at the spout, where it was placed at the start.
    if !poured(i) {
        predicted[i] = vec4<f32>(s.xy, s.xy);
        return;
    }
    let dt = params.more.w;
    let v = s.zw + step.b.xy * params.more.x * dt;
    predicted[i] = vec4<f32>(s.xy + v * dt, s.xy);
}

@compute @workgroup_size(256)
fn clear_cells(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x <= cells() {
        atomicStore(&cell_count[id.x], 0u);
    }
}

@compute @workgroup_size(256)
fn count_cells(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= params.counts.x || !poured(i) {
        return;
    }
    atomicAdd(&cell_count[cell_index(cell_xy(predicted[i].xy))], 1u);
}

// Prefix sums of the cell counts, 1024 cells per workgroup (four per thread).
var<workgroup> partial: array<u32, 256>;

@compute @workgroup_size(256)
fn scan_blocks(@builtin(local_invocation_id) lid: vec3<u32>, @builtin(workgroup_id) wg: vec3<u32>) {
    let base = wg.x * 1024u + lid.x * 4u;
    var values: array<u32, 4>;
    var total = 0u;
    for (var k = 0u; k < 4u; k++) {
        let c = base + k;
        var n = 0u;
        if c <= cells() {
            n = atomicLoad(&cell_count[c]);
        }
        values[k] = total;
        total += n;
    }
    partial[lid.x] = total;
    workgroupBarrier();
    // Hillis-Steele scan of the per-thread totals.
    for (var offset = 1u; offset < 256u; offset *= 2u) {
        var add = 0u;
        if lid.x >= offset {
            add = partial[lid.x - offset];
        }
        workgroupBarrier();
        partial[lid.x] += add;
        workgroupBarrier();
    }
    let before = partial[lid.x] - total;
    for (var k = 0u; k < 4u; k++) {
        let c = base + k;
        if c <= cells() {
            cell_start[c] = before + values[k];
        }
    }
    if lid.x == 255u {
        block_sums[wg.x] = partial[255];
    }
}

@compute @workgroup_size(256)
fn scan_sums(@builtin(local_invocation_id) lid: vec3<u32>) {
    let blocks = params.counts.w;
    var values: array<u32, 4>;
    var total = 0u;
    for (var k = 0u; k < 4u; k++) {
        let b = lid.x * 4u + k;
        var n = 0u;
        if b < blocks {
            n = block_sums[b];
        }
        values[k] = total;
        total += n;
    }
    partial[lid.x] = total;
    workgroupBarrier();
    for (var offset = 1u; offset < 256u; offset *= 2u) {
        var add = 0u;
        if lid.x >= offset {
            add = partial[lid.x - offset];
        }
        workgroupBarrier();
        partial[lid.x] += add;
        workgroupBarrier();
    }
    let before = partial[lid.x] - total;
    for (var k = 0u; k < 4u; k++) {
        let b = lid.x * 4u + k;
        if b < blocks {
            block_sums[b] = before + values[k];
        }
    }
}

@compute @workgroup_size(256)
fn scan_finish(@builtin(global_invocation_id) id: vec3<u32>) {
    let c = id.x;
    if c > cells() {
        return;
    }
    let start = cell_start[c] + block_sums[c / 1024u];
    cell_start[c] = start;
    // The counts become each cell's next free slot while scattering.
    atomicStore(&cell_count[c], start);
}

@compute @workgroup_size(256)
fn scatter(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= params.counts.x || !poured(i) {
        return;
    }
    let slot = atomicAdd(&cell_count[cell_index(cell_xy(predicted[i].xy))], 1u);
    sorted[slot] = i;
}

@compute @workgroup_size(256)
fn density(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= params.counts.x {
        return;
    }
    if !poured(i) {
        pressure[i] = vec2<f32>(0.0);
        return;
    }
    let p = predicted[i].xy;
    let h = params.shape.y;
    let c = cell_xy(p);
    var rho = 0;
    var near = 0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let n = c + vec2<i32>(x, y);
            if n.x < 0 || n.y < 0 || n.x >= i32(params.counts.y) || n.y >= i32(params.counts.z) {
                continue;
            }
            let cell = cell_index(n);
            for (var k = cell_start[cell]; k < cell_start[cell + 1u]; k++) {
                let r = length(predicted[sorted[k]].xy - p);
                let q = 1.0 - r / h;
                if q > 0.0 {
                    rho += i32(round(q * q * DENSITY_SCALE));
                    near += i32(round(q * q * q * DENSITY_SCALE));
                }
            }
        }
    }
    let rho_f = f32(rho) / DENSITY_SCALE;
    let near_f = f32(near) / DENSITY_SCALE;
    // Thinner than at rest pulls together only gently, so the surface stays whole without clumping.
    var crowd = rho_f - params.shape.z;
    if crowd < 0.0 {
        crowd *= 0.3;
    }
    pressure[i] = vec2<f32>(params.motion.x * crowd, params.motion.y * near_f);
}

@compute @workgroup_size(256)
fn relax(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= params.counts.x {
        return;
    }
    let me = predicted[i];
    if !poured(i) {
        relaxed[i] = vec4<f32>(me.xy, 0.0, 0.0);
        return;
    }
    let p = me.xy;
    let v = state[i].zw;
    let h = params.shape.y;
    let pi = pressure[i];
    let c = cell_xy(p);
    var dx = vec2<i32>(0);
    var dv = vec2<i32>(0);
    var weight = 0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let n = c + vec2<i32>(x, y);
            if n.x < 0 || n.y < 0 || n.x >= i32(params.counts.y) || n.y >= i32(params.counts.z) {
                continue;
            }
            let cell = cell_index(n);
            for (var k = cell_start[cell]; k < cell_start[cell + 1u]; k++) {
                let j = sorted[k];
                if j == i {
                    continue;
                }
                let d = predicted[j].xy - p;
                let r = length(d);
                let q = 1.0 - r / h;
                if q <= 0.0 {
                    continue;
                }
                var dir = vec2<f32>(0.0, 1.0);
                if r > 1e-5 {
                    dir = d / r;
                } else {
                    // Two particles in the same spot: split them apart by their numbers.
                    let a = hash2(min(i, j), max(i, j)) * TAU;
                    dir = vec2<f32>(cos(a), sin(a)) * select(-1.0, 1.0, i < j);
                }
                let pj = pressure[j];
                let push = 0.25 * ((pi.x + pj.x) * q + (pi.y + pj.y) * q * q) * h;
                dx -= vec2<i32>(round(dir * push * MOVE_SCALE));
                dv += vec2<i32>(round((state[j].zw - v) * q * 16.0));
                weight += i32(round(q * 4096.0));
            }
        }
    }
    var shift = vec2<f32>(dx) / MOVE_SCALE;
    // Never move more than a fraction of the reach in one go.
    let len = length(shift);
    if len > h * 0.1 {
        shift *= h * 0.1 / len;
    }
    var visc = vec2<f32>(0.0);
    if weight > 0 {
        visc = vec2<f32>(dv) / 16.0 / (f32(weight) / 4096.0) * params.motion.z;
    }
    relaxed[i] = vec4<f32>(p + shift, visc);
}

@compute @workgroup_size(256)
fn finish(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= params.counts.x {
        return;
    }
    if !poured(i) {
        return;
    }
    let dt = params.more.w;
    let old = predicted[i].zw;
    var p = relaxed[i].xy;
    let r = params.shape.x * 0.5;
    // Balls push the liquid aside, but feel nothing back.
    let count = u32(step.a.w);
    var shove = vec2<f32>(0.0);
    var shoved = false;
    for (var b = 0u; b < count; b++) {
        let ball = step.balls[b];
        let d = p - ball.place.xy;
        let reach = ball.place.z + r;
        let dist = length(d);
        if dist < reach && params.motion.w > 0.0 {
            var n = vec2<f32>(0.0, -1.0);
            if dist > 1e-4 {
                n = d / dist;
            }
            p = ball.place.xy + n * mix(dist, reach, min(params.motion.w, 1.0));
            shove = ball.velocity.xy * min(params.motion.w, 1.0);
            shoved = true;
        }
    }
    // Walls.
    let wall = sdf_at(p);
    var hit_wall = false;
    var n = vec2<f32>(0.0);
    if wall < r {
        n = sdf_normal(p);
        p += n * (r - wall);
        hit_wall = true;
    }
    let half = step.b.zw - vec2<f32>(r);
    p = clamp(p, -half, half);
    var v = (p - old) / dt + relaxed[i].zw;
    if shoved {
        v = mix(v, shove, 0.5);
    }
    if hit_wall {
        let into = dot(v, n);
        if into < 0.0 {
            v -= n * into * (1.0 + params.spout.w);
        }
    }
    let speed = length(v);
    if speed > 4000.0 {
        v *= 4000.0 / speed;
    }
    if !(p.x == p.x && p.y == p.y && v.x == v.x && v.y == v.y) {
        p = old;
        v = vec2<f32>(0.0);
    }
    state[i] = vec4<f32>(p, v);
}

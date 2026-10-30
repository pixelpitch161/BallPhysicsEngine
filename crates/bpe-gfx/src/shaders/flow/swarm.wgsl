// The swarm: up to a million points that swirl, flock, follow balls and stay out of walls.
//
// Flocking uses a coarse grid: every point adds itself to its cell's count and velocity, then
// steers by its neighbourhood's average. The sums are whole numbers, so they come out the same
// whatever order the GPU adds them in, which keeps every run identical.

// Matches `SwarmParams` in flow/swarm.rs.
struct Params {
    // x: points, y: grid cells across, z: grid cells down, w: seed
    counts: vec4<u32>,
    // x: top speed, y: drag, z: flow strength, w: swirl size
    motion: vec4<f32>,
    // x: swirl change speed, y: flock, z: separation, w: attraction to balls
    steer: vec4<f32>,
    // x: ball reach, y: wall avoidance, z: gravity scale, w: grid cell size
    more: vec4<f32>,
    // x: colour follow (0 speed, 1 nearest ball, 2 random, 3 place along a direction, 4 distance
    // from a centre, 5 one colour), z: start (0 fill, 1 centre, 2 ring)
    look: vec4<u32>,
    // xy: swarm centre, z: half size of the area a Fill start covers, w: point size (drawing)
    arena: vec4<f32>,
    // x: brightness (drawing), y: seconds the release from the centre takes (0: all at once)
    draw: vec4<f32>,
    // The one colour, and a point's colour before it has a nearest ball.
    color: vec4<f32>,
    // The paint, as `PaintGpu`'s fields in order.
    paint_a: vec4<f32>,
    paint_b: vec4<f32>,
    // x: direction in degrees, y: span (also of a speed), zw: centre
    place: vec4<f32>,
};

@group(0) @binding(0) var<uniform> step: Step;
@group(1) @binding(0) var<uniform> params: Params;
@group(1) @binding(1) var<storage, read_write> points: array<vec4<f32>>;
@group(1) @binding(2) var<storage, read_write> tints: array<vec2<u32>>;
@group(1) @binding(3) var<storage, read_write> field: array<atomic<i32>>;
@group(2) @binding(0) var<uniform> sdf_info: SdfInfo;
@group(2) @binding(1) var sdf_texture: texture_2d<f32>;

// Velocity sums in the grid are stored in steps of 1/8 pixel per second.
const VELOCITY_SCALE: f32 = 8.0;
// Radius of the disc a Centre start fills, and of the Ring start.
const CENTRE_RADIUS: f32 = 40.0;
const RING_RADIUS: f32 = 200.0;

fn rand(i: u32, k: u32) -> f32 {
    return hash2(params.counts.w ^ (i * 0x632be5abu), k);
}

fn cell_hash(c: vec2<i32>, layer: u32) -> f32 {
    return hash2(bitcast<u32>(c.x) * 0x27d4eb2du ^ layer, bitcast<u32>(c.y) + params.counts.w);
}

// Smooth noise and its slope in x and y.
fn noise(p: vec2<f32>, layer: u32) -> vec3<f32> {
    let i = vec2<i32>(floor(p));
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let du = 6.0 * f * (1.0 - f);
    let a = cell_hash(i, layer);
    let b = cell_hash(i + vec2<i32>(1, 0), layer);
    let c = cell_hash(i + vec2<i32>(0, 1), layer);
    let d = cell_hash(i + vec2<i32>(1, 1), layer);
    let k = a - b - c + d;
    let value = a + (b - a) * u.x + (c - a) * u.y + k * u.x * u.y;
    return vec3<f32>(value, du.x * ((b - a) + k * u.y), du.y * ((c - a) + k * u.x));
}

// A swirling flow with no sources or sinks, so points never bunch up because of it.
fn curl(p: vec2<f32>, t: f32) -> vec2<f32> {
    let q = p / max(params.motion.w, 1.0);
    let phase = t * params.steer.x;
    let layer = u32(floor(phase));
    let blend = smoothstep(0.0, 1.0, fract(phase));
    let n = mix(noise(q, layer), noise(q + vec2<f32>(17.3, 9.1), layer + 1u), blend)
        + 0.5 * mix(noise(q * 2.3, layer + 101u), noise(q * 2.3 + vec2<f32>(5.7, 3.3), layer + 102u), blend);
    return vec2<f32>(n.z, -n.y) * 0.8;
}

fn pack_color(c: vec4<f32>) -> vec2<u32> {
    return vec2<u32>(pack2x16float(c.rg), pack2x16float(c.ba));
}

// When point i leaves the centre; 0 when every point starts in place.
fn release_time(i: u32) -> f32 {
    return params.draw.y * f32(i) / f32(max(params.counts.x, 1u));
}

// A point's velocity as it leaves the centre.
fn release_velocity(i: u32) -> vec2<f32> {
    let angle = rand(i, 40u) * TAU;
    return vec2<f32>(cos(angle), sin(angle)) * max(params.motion.x, 1.0) * (0.3 + 0.7 * rand(i, 41u));
}

fn start_point(i: u32, k: u32) -> vec2<f32> {
    let half = step.b.zw;
    let center = params.arena.xy;
    let a = rand(i, k * 2u);
    let b = rand(i, k * 2u + 1u);
    switch params.look.z {
        case 1u: {
            let r = CENTRE_RADIUS * sqrt(a);
            return center + vec2<f32>(cos(b * TAU), sin(b * TAU)) * r;
        }
        case 2u: {
            let r = RING_RADIUS + 10.0 * a;
            return center + vec2<f32>(cos(b * TAU), sin(b * TAU)) * r;
        }
        default: {
            // Anywhere in the square around the centre, within the frame.
            let size = params.arena.z;
            let corner = min(center + vec2<f32>(size), half);
            let start = max(center - vec2<f32>(size), -half);
            return mix(start, corner, vec2<f32>(a, b));
        }
    }
}

@compute @workgroup_size(256)
fn init_points(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= params.counts.x {
        return;
    }
    if params.draw.y > 0.0 {
        points[i] = vec4<f32>(params.arena.xy, release_velocity(i));
        tints[i] = pack_color(params.color);
        return;
    }
    var p = start_point(i, 0u);
    // Try a few more spots if the first is inside a wall.
    for (var k = 1u; k < 32u && sdf_at(p) < 3.0; k++) {
        p = start_point(i, k);
    }
    if sdf_at(p) < 3.0 {
        p = params.arena.xy;
    }
    let angle = rand(i, 40u) * TAU;
    let v = vec2<f32>(cos(angle), sin(angle)) * params.motion.x * 0.1 * rand(i, 41u);
    points[i] = vec4<f32>(p, v);
    tints[i] = pack_color(params.color);
}

@compute @workgroup_size(256)
fn clear_field(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x < arrayLength(&field) {
        atomicStore(&field[id.x], 0);
    }
}

fn cell_of(p: vec2<f32>) -> vec2<i32> {
    let cells = vec2<i32>(params.counts.yz);
    let c = vec2<i32>(floor((p + step.b.zw) / params.more.w));
    return clamp(c, vec2<i32>(0), cells - vec2<i32>(1));
}

@compute @workgroup_size(256)
fn fill_field(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= params.counts.x {
        return;
    }
    if step.a.x < release_time(i) {
        return;
    }
    let point = points[i];
    let c = cell_of(point.xy);
    let cell = u32(c.y) * params.counts.y + u32(c.x);
    atomicAdd(&field[cell * 3u], 1);
    atomicAdd(&field[cell * 3u + 1u], i32(round(point.z * VELOCITY_SCALE)));
    atomicAdd(&field[cell * 3u + 2u], i32(round(point.w * VELOCITY_SCALE)));
}

// Count and velocity sum of one grid cell, as floats. Outside the grid is empty.
fn cell_sums(c: vec2<i32>) -> vec3<f32> {
    let cells = vec2<i32>(params.counts.yz);
    if c.x < 0 || c.y < 0 || c.x >= cells.x || c.y >= cells.y {
        return vec3<f32>(0.0);
    }
    let cell = u32(c.y) * params.counts.y + u32(c.x);
    return vec3<f32>(f32(atomicLoad(&field[cell * 3u])),
                     f32(atomicLoad(&field[cell * 3u + 1u])) / VELOCITY_SCALE,
                     f32(atomicLoad(&field[cell * 3u + 2u])) / VELOCITY_SCALE);
}

@compute @workgroup_size(256)
fn move_points(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= params.counts.x {
        return;
    }
    let dt = step.a.y;
    let time = step.a.x;
    // Not released yet: wait at the centre, ready to leave.
    if time < release_time(i) {
        points[i] = vec4<f32>(params.arena.xy, release_velocity(i));
        return;
    }
    let top = max(params.motion.x, 1.0);
    var p = points[i].xy;
    var v = points[i].zw;
    var a = step.b.xy * params.more.z;

    // Swirling flow.
    if params.motion.z > 0.0 {
        let target_v = curl(p, time) * top * params.motion.z;
        a += (target_v - v) * 2.0;
    }

    // Neighbours, from the grid around p, blended between the four nearest cell centres.
    let cell = params.more.w;
    if params.steer.y > 0.0 || params.steer.z > 0.0 {
        let q = (p + step.b.zw) / cell - vec2<f32>(0.5);
        let c = vec2<i32>(floor(q));
        let f = q - floor(q);
        let s00 = cell_sums(c);
        let s10 = cell_sums(c + vec2<i32>(1, 0));
        let s01 = cell_sums(c + vec2<i32>(0, 1));
        let s11 = cell_sums(c + vec2<i32>(1, 1));
        let sums = mix(mix(s00, s10, f.x), mix(s01, s11, f.x), f.y);
        let mean = f32(params.counts.x) / max(f32(params.counts.y * params.counts.z), 1.0);
        let crowd = sums.x / max(mean, 1e-3);
        // How the count changes across the neighbourhood, per cell.
        let slope = vec2<f32>(mix(s10.x - s00.x, s11.x - s01.x, f.y), mix(s01.x - s00.x, s11.x - s10.x, f.x))
            / max(mean, 1e-3);
        if sums.x > 0.5 {
            let average = sums.yz / sums.x;
            a += (average - v) * params.steer.y * 2.5;
        }
        let pull = params.steer.y * 0.6 - params.steer.z * 2.5 * clamp(crowd, 0.0, 4.0);
        a += slope * pull * top;
    }

    // Balls pull (or push) points that are within reach, and shove them out of the way.
    let count = u32(step.a.w);
    var nearest = 1e9;
    var nearest_color = params.color;
    for (var b = 0u; b < count; b++) {
        let ball = step.balls[b];
        let d = ball.place.xy - p;
        let dist = length(d);
        if dist < nearest {
            nearest = dist;
            nearest_color = ball.color;
        }
        let reach = params.more.x + ball.place.z;
        if dist < reach && dist > 1e-3 {
            let dir = d / dist;
            a += dir * params.steer.w * (1.0 - dist / reach) * top * 4.0;
            let solid = ball.place.z + 1.5;
            if dist < solid {
                p = ball.place.xy - dir * solid;
                let into = dot(v - ball.velocity.xy, dir);
                if into > 0.0 {
                    v -= dir * into * 1.8;
                }
            }
        }
    }

    // Walls push points back before they touch.
    let wall = sdf_at(p);
    if params.more.y > 0.0 && wall < 24.0 {
        a += sdf_normal(p) * (1.0 - wall / 24.0) * params.more.y * top * 8.0;
    }

    v += a * dt;
    v *= exp(-max(params.motion.y, 0.0) * dt);
    let speed = length(v);
    if speed > top {
        v *= top / speed;
    }
    p += v * dt;

    // Never inside a wall, never off the frame.
    let d = sdf_at(p);
    if d < 0.5 {
        let n = sdf_normal(p);
        p += n * (0.5 - d);
        let into = dot(v, n);
        if into < 0.0 {
            v -= n * into * 1.6;
        }
    }
    let half = step.b.zw - vec2<f32>(0.5);
    if abs(p.x) > half.x {
        p.x = clamp(p.x, -half.x, half.x);
        v.x = -v.x * 0.5;
    }
    if abs(p.y) > half.y {
        p.y = clamp(p.y, -half.y, half.y);
        v.y = -v.y * 0.5;
    }
    points[i] = vec4<f32>(p, v);

    // Only the nearest ball's colour has to be kept: the other colour modes are worked out when
    // drawing, so colour settings can change without starting the swarm again.
    if count > 0u {
        tints[i] = pack_color(nearest_color);
    }
}

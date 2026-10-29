// Hit particles. Nothing is stored between frames: each particle's place is worked out from
// its burst (where and when it went off) and its age, with a hash for the random parts. So
// scrubbing the timeline and exporting show the same particles.

struct Globals {
    view: vec4<f32>,
    bg_top: vec4<f32>,
    bg_bottom: vec4<f32>,
    post: vec4<f32>,
};

// One burst. Matches `BurstGpu` in particles.rs.
struct Burst {
    // xy: where it went off, zw: the way its particles fly
    place: vec4<f32>,
    // x: seconds since it went off, y: index of its first particle, z: particle count,
    // w: in the first burst, how many bursts are in use
    timing: vec4<f32>,
    color: vec4<f32>,
    // speed, spread (radians), size, life
    motion: vec4<f32>,
    // gravity, drag
    more: vec4<f32>,
    // x: shape, y: seed
    ids: vec4<u32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var<storage, read> bursts: array<Burst>;

const PI: f32 = 3.14159265;
const TAU: f32 = 6.28318531;

const SPARKS: u32 = 0u;
const SMOKE: u32 = 1u;
const CONFETTI: u32 = 2u;
const STARS: u32 = 3u;
const SHARDS: u32 = 4u;
const RINGS: u32 = 5u;

struct Out {
    @builtin(position) pos: vec4<f32>,
    // Position inside the particle's quad, -1 to 1 on each side.
    @location(0) local: vec2<f32>,
    @location(1) color: vec4<f32>,
    // x: how far along its life (0 to 1), y: world pixels per local unit, z: shading, w: ring radius
    @location(2) info: vec4<f32>,
    @location(3) @interpolate(flat) kind: u32,
};

fn to_clip(p: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(p.x * g.view.x, -p.y * g.view.y, 0.0, 1.0);
}

fn quad_corner(i: u32) -> vec2<f32> {
    return vec2<f32>(f32(i & 1u), f32((i >> 1u) & 1u)) * 2.0 - 1.0;
}

fn rotate(p: vec2<f32>, angle: f32) -> vec2<f32> {
    let c = cos(angle);
    let s = sin(angle);
    return vec2<f32>(c * p.x - s * p.y, s * p.x + c * p.y);
}

// A hash of two numbers into 0..1.
fn hash2(a: u32, b: u32) -> f32 {
    var h = (a * 0x9e3779b9u) ^ (b * 0x85ebca6bu);
    h = h ^ (h >> 16u);
    h = h * 0x7feb352du;
    h = h ^ (h >> 15u);
    h = h * 0x846ca68bu;
    h = h ^ (h >> 16u);
    return f32(h >> 8u) / 16777216.0;
}

// The burst holding particle number `i`: the last one whose first particle is at or before it.
fn find_burst(i: u32) -> u32 {
    var lo = 0u;
    var hi = max(u32(bursts[0].timing.w), 1u);
    while hi - lo > 1u {
        let mid = (lo + hi) / 2u;
        if u32(bursts[mid].timing.y) <= i {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    return lo;
}

fn hidden() -> Out {
    var out: Out;
    out.pos = vec4<f32>(2.0, 2.0, 2.0, 1.0);
    out.local = vec2<f32>(0.0);
    out.color = vec4<f32>(0.0);
    out.info = vec4<f32>(1.0, 1.0, 0.0, 0.0);
    out.kind = 0u;
    return out;
}

@vertex
fn vs_particle(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> Out {
    let burst = bursts[find_burst(ii)];
    let k = ii - u32(burst.timing.y);
    if f32(k) >= burst.timing.z {
        return hidden();
    }
    let kind = burst.ids.x;
    let seed = burst.ids.y;
    var h: array<f32, 8>;
    for (var j = 0u; j < 8u; j++) {
        h[j] = hash2(seed, k * 8u + j);
    }
    var life = burst.motion.w * (0.6 + 0.4 * h[2]);
    if kind == RINGS {
        life = burst.motion.w;
    }
    let age = burst.timing.x;
    let t = age / max(life, 1e-4);
    if t >= 1.0 || age < 0.0 {
        return hidden();
    }

    let base = atan2(burst.place.w, burst.place.z);
    let angle = base + (h[0] - 0.5) * burst.motion.y;
    let v0 = vec2<f32>(cos(angle), sin(angle)) * burst.motion.x * (0.35 + 0.65 * h[1]);
    let grav = vec2<f32>(0.0, burst.more.x);
    let drag = max(burst.more.y, 0.0);
    var p = burst.place.xy;
    var v = v0;
    if kind == RINGS {
        v = vec2<f32>(0.0);
    } else if drag > 1e-4 {
        let e1 = exp(-drag * age);
        let terminal = grav / drag;
        p += (v0 - terminal) * (1.0 - e1) / drag + terminal * age;
        v = (v0 - terminal) * e1 + terminal;
    } else {
        p += v0 * age + 0.5 * grav * age * age;
        v = v0 + grav * age;
    }

    let size = burst.motion.z * (0.6 + 0.8 * h[3]);
    let corner = quad_corner(vi);
    var offset: vec2<f32>;
    var scale = size;
    var shade = 1.0;
    var ring = 0.0;
    var fade = 1.0 - t;
    switch kind {
        case SPARKS: {
            let speed = length(v);
            var axis = vec2<f32>(1.0, 0.0);
            if speed > 1e-3 {
                axis = v / speed;
            }
            let half_length = size + speed * 0.025;
            let half_width = max(size * 0.45, g.view.z);
            offset = axis * corner.x * half_length + vec2<f32>(-axis.y, axis.x) * corner.y * half_width;
            scale = half_width;
            fade = fade * fade;
        }
        case SMOKE: {
            scale = size * (1.0 + 2.5 * t);
            offset = rotate(corner, h[4] * TAU) * scale;
            fade = (1.0 - t) * smoothstep(0.0, 0.08, t) * 0.55;
        }
        case CONFETTI: {
            // Paper turning over: the width shrinks to nothing and the other side shows.
            p.x += sin(age * (3.0 + 2.0 * h[5]) + h[6] * TAU) * 14.0 * min(age, 1.0);
            let flip = cos(h[6] * TAU + age * (5.0 + 7.0 * h[7]));
            let spin = h[4] * TAU + (h[5] - 0.5) * 12.0 * age;
            let half = vec2<f32>(size * max(abs(flip), 0.08), size * 0.6);
            offset = rotate(corner * half, spin);
            shade = select(0.6, 1.0, flip > 0.0) * (0.55 + 0.45 * abs(flip));
            fade = min(1.0, (1.0 - t) * 4.0);
        }
        case STARS: {
            let twinkle = 0.75 + 0.25 * sin(age * 18.0 + h[5] * TAU);
            scale = size * 1.6 * twinkle;
            offset = rotate(corner, h[4] * 0.6) * scale;
            fade = (1.0 - t) * smoothstep(0.0, 0.1, t);
        }
        case SHARDS: {
            let spin = h[4] * TAU + (h[5] - 0.5) * 16.0 * age;
            let half = vec2<f32>(size, size * 0.45);
            offset = rotate(corner * half, spin);
            fade = min(1.0, (1.0 - t) * 3.0);
        }
        default: {
            let grow = 1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t);
            ring = burst.motion.z * grow;
            scale = ring + 4.0 * g.view.z + 2.0;
            offset = corner * scale;
            fade = (1.0 - t) * (1.0 - t);
        }
    }

    var out: Out;
    out.pos = to_clip(p + offset);
    out.local = corner;
    out.color = vec4<f32>(burst.color.rgb, burst.color.a * fade);
    out.info = vec4<f32>(t, scale, shade, ring);
    out.kind = kind;
    return out;
}

// How much of the particle covers this pixel, 0 to 1, before fading.
fn coverage(in: Out) -> f32 {
    let px = g.view.z;
    let l = in.local;
    // Smaller than a pixel: dim instead of flickering.
    let tiny = clamp(in.info.y / px, 0.0, 1.0);
    switch in.kind {
        case SPARKS: {
            let across = clamp((1.0 - abs(l.y)) * in.info.y / px, 0.0, 1.0);
            let along = clamp((1.0 - abs(l.x)) * 4.0, 0.0, 1.0);
            // Brightest at the front, fading towards the tail.
            return across * along * (0.25 + 0.75 * (l.x * 0.5 + 0.5)) * tiny;
        }
        case SMOKE: {
            return exp(-3.5 * dot(l, l));
        }
        case CONFETTI, SHARDS: {
            var edge = 1.0 - max(abs(l.x), abs(l.y));
            if in.kind == SHARDS {
                // A splinter: a long thin triangle.
                edge = min(1.0 - abs(l.x), (1.0 - l.x) * 0.5 - abs(l.y));
            }
            return clamp(edge * in.info.y / px + 0.5, 0.0, 1.0) * tiny;
        }
        case STARS: {
            let x = abs(l.x);
            let y = abs(l.y);
            let rays = exp(-7.0 * x) * exp(-90.0 * y * y) + exp(-7.0 * y) * exp(-90.0 * x * x);
            return min(rays + exp(-18.0 * dot(l, l)), 1.0);
        }
        default: {
            let d = abs(length(l) * in.info.y - in.info.w);
            let half = max(1.2, px * 0.75);
            return clamp((half - d) / px + 0.5, 0.0, 1.0);
        }
    }
}

@fragment
fn fs_add(in: Out) -> @location(0) vec4<f32> {
    let a = coverage(in) * abs(in.color.a);
    if a < 1e-4 {
        discard;
    }
    // Glowing particles are brighter than the ball colour, so they bloom.
    return vec4<f32>(in.color.rgb * in.info.z * a * 2.0, 0.0);
}

@fragment
fn fs_alpha(in: Out) -> @location(0) vec4<f32> {
    let a = coverage(in) * abs(in.color.a);
    if a < 1e-4 {
        discard;
    }
    // A negative alpha is an additive colour: it adds its light and covers nothing.
    return vec4<f32>(in.color.rgb * in.info.z * a, select(a, 0.0, in.color.a < 0.0));
}

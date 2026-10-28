// Scene passes, all drawn into the HDR target in linear light.
// World coordinates are video pixels, origin at the frame centre, y down.

struct Globals {
    // x: 2 / video width, y: 2 / video height, z: world units per output pixel, w: time
    view: vec4<f32>,
    bg_top: vec4<f32>,
    bg_bottom: vec4<f32>,
    // x: ball glow, y: trail opacity, z: flash strength, w: bloom strength
    look: vec4<f32>,
    // x: exposure
    look2: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;

fn to_clip(p: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(p.x * g.view.x, -p.y * g.view.y, 0.0, 1.0);
}

// ---- Background gradient (fullscreen) ----

struct BgOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec2<f32>,
};

@vertex
fn vs_background(@builtin(vertex_index) i: u32) -> BgOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    let ndc = uv * 2.0 - 1.0;
    var out: BgOut;
    out.pos = vec4<f32>(ndc, 0.0, 1.0);
    out.world = vec2<f32>(ndc.x / g.view.x, -ndc.y / g.view.y);
    return out;
}

@fragment
fn fs_background(in: BgOut) -> @location(0) vec4<f32> {
    let t = clamp(in.world.y * g.view.y * 0.5 + 0.5, 0.0, 1.0);
    return vec4<f32>(mix(g.bg_top.rgb, g.bg_bottom.rgb, t), 1.0);
}

// ---- Walls, pegs, lines and shards (instanced quads with a distance function) ----

const PI: f32 = 3.14159265;
const TAU: f32 = 6.28318531;
// How far a wall's glow reaches, in world pixels.
const GLOW_REACH: f32 = 36.0;

struct ShapeIn {
    // Outlines: centre xy, angle. Capsules: both end points.
    @location(0) a: vec4<f32>,
    // Ring: radius, sides, half gap. Box: half size.
    @location(1) b: vec4<f32>,
    // Kind (0 ring/polygon, 1 box, 2 capsule), half line width, glow.
    @location(2) c: vec4<f32>,
    @location(3) color: vec4<f32>,
};

struct ShapeOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec2<f32>,
    @location(1) @interpolate(flat) a: vec4<f32>,
    @location(2) @interpolate(flat) b: vec4<f32>,
    @location(3) @interpolate(flat) c: vec4<f32>,
    @location(4) @interpolate(flat) color: vec4<f32>,
};

@vertex
fn vs_shape(@builtin(vertex_index) i: u32, s: ShapeIn) -> ShapeOut {
    let corner = quad_corner(i);
    let px = g.view.z;
    var reach = max(s.c.y, 0.5 * px) + 2.0 * px;
    if s.c.z > 0.0 {
        reach += GLOW_REACH;
    }
    var world: vec2<f32>;
    if s.c.x > 1.5 {
        // A capsule: a box around the segment, lined up with it.
        let along = s.a.zw - s.a.xy;
        let len = length(along);
        var dir = vec2<f32>(1.0, 0.0);
        if len > 1e-4 {
            dir = along / len;
        }
        let side = vec2<f32>(-dir.y, dir.x);
        let middle = (s.a.xy + s.a.zw) * 0.5;
        world = middle + dir * corner.x * (len * 0.5 + reach) + side * corner.y * reach;
    } else {
        var extent = s.b.x;
        if s.c.x > 0.5 {
            extent = length(s.b.xy);
        }
        world = s.a.xy + corner * (extent + reach);
    }
    var out: ShapeOut;
    out.pos = to_clip(world);
    out.world = world;
    out.a = s.a;
    out.b = s.b;
    out.c = s.c;
    out.color = s.color;
    return out;
}

fn rotate(p: vec2<f32>, angle: f32) -> vec2<f32> {
    let c = cos(angle);
    let s = sin(angle);
    return vec2<f32>(c * p.x - s * p.y, s * p.x + c * p.y);
}

fn segment_distance(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let ab = b - a;
    let t = clamp(dot(p - a, ab) / max(dot(ab, ab), 1e-8), 0.0, 1.0);
    return length(p - a - ab * t);
}

// Point on a ring or regular polygon `u` radians clockwise from the gap's middle, relative to its centre.
fn outline_point(radius: f32, sides: f32, angle: f32, u: f32) -> vec2<f32> {
    let d = vec2<f32>(cos(angle + u), sin(angle + u));
    if sides < 2.5 {
        return d * radius;
    }
    let step = TAU / sides;
    let side = round(u / step);
    return d * (radius * cos(PI / sides) / cos(u - side * step));
}

// Distance from p (relative to the centre) to a ring or polygon outline with a gap.
fn ring_distance(p: vec2<f32>, radius: f32, sides: f32, angle: f32, gap_half: f32) -> f32 {
    // Angle of p clockwise from the gap's middle, in -PI..PI.
    let u = atan2(p.y, p.x) - angle;
    let u_wrapped = u - TAU * floor((u + PI) / TAU);
    if gap_half > 0.0 && abs(u_wrapped) < gap_half {
        let a = outline_point(radius, sides, angle, gap_half);
        let b = outline_point(radius, sides, angle, -gap_half);
        return min(length(p - a), length(p - b));
    }
    if sides < 2.5 {
        return abs(length(p) - radius);
    }
    // The nearest side of a regular polygon is the one in p's own sector.
    let step = TAU / sides;
    let side = round(u_wrapped / step);
    let a = outline_point(radius, sides, angle, (side - 0.5) * step);
    let b = outline_point(radius, sides, angle, (side + 0.5) * step);
    return segment_distance(p, a, b);
}

@fragment
fn fs_shape(in: ShapeOut) -> @location(0) vec4<f32> {
    let px = g.view.z;
    var d: f32;
    if in.c.x < 0.5 {
        d = ring_distance(in.world - in.a.xy, in.b.x, in.b.y, in.a.z, in.b.z);
    } else if in.c.x < 1.5 {
        let q = abs(rotate(in.world - in.a.xy, -in.a.z)) - in.b.xy;
        d = abs(length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0));
    } else {
        d = segment_distance(in.world, in.a.xy, in.a.zw);
    }
    let width = in.c.y * 2.0;
    let half_width = max(in.c.y, 0.5 * px);
    // Lines thinner than a pixel get dimmer instead of aliasing.
    let cover = clamp((half_width - d) / px + 0.5, 0.0, 1.0) * min(width / px, 1.0) * in.color.a;
    let glow = exp(-max(d - half_width, 0.0) / 6.0) * in.c.z;
    if cover <= 0.0 && glow < 1e-4 {
        discard;
    }
    return vec4<f32>(in.color.rgb * (cover + glow), cover);
}

// ---- Motion streaks (triangle list built on the CPU) ----

struct TrailIn {
    @location(0) pos: vec2<f32>,
    @location(1) color: vec4<f32>,
    // x: signed distance from the streak centre line, y: half width at this point
    @location(2) edge: vec2<f32>,
};

struct TrailOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) edge: vec2<f32>,
};

@vertex
fn vs_trail(v: TrailIn) -> TrailOut {
    var out: TrailOut;
    out.pos = to_clip(v.pos);
    out.color = v.color;
    out.edge = v.edge;
    return out;
}

@fragment
fn fs_trail(in: TrailOut) -> @location(0) vec4<f32> {
    let px = g.view.z;
    let half_width = max(in.edge.y, 0.5 * px);
    // Lines thinner than a pixel get dimmer instead of aliasing.
    let thin = min(in.edge.y / (0.5 * px), 1.0);
    let cover = clamp((half_width - abs(in.edge.x)) / px + 0.5, 0.0, 1.0) * thin;
    let a = in.color.a * cover * g.look.y;
    return vec4<f32>(in.color.rgb * a, 0.0);
}

// ---- Balls (instanced quads, shaded like small spheres with a soft halo) ----

const BALL_EXTENT: f32 = 3.0;

struct BallIn {
    @location(0) center: vec2<f32>,
    @location(1) radius: f32,
    @location(2) color: vec4<f32>,
};

struct BallOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) radius_px: f32,
};

fn quad_corner(i: u32) -> vec2<f32> {
    return vec2<f32>(f32(i & 1u), f32((i >> 1u) & 1u)) * 2.0 - 1.0;
}

@vertex
fn vs_ball(@builtin(vertex_index) i: u32, b: BallIn) -> BallOut {
    let corner = quad_corner(i) * BALL_EXTENT;
    var out: BallOut;
    out.pos = to_clip(b.center + corner * b.radius);
    out.local = corner;
    out.color = b.color;
    out.radius_px = b.radius / g.view.z;
    return out;
}

@fragment
fn fs_ball(in: BallOut) -> @location(0) vec4<f32> {
    let r = length(in.local);
    let aa = 1.0 / max(in.radius_px, 0.5);
    let cover = clamp((1.0 - r) / aa + 0.5, 0.0, 1.0);
    let n = vec3<f32>(in.local, sqrt(max(1.0 - r * r, 0.0)));
    // Light from the upper left (y is down in world space).
    let l = normalize(vec3<f32>(-0.45, -0.55, 0.7));
    let diffuse = 0.45 + 0.8 * max(dot(n, l), 0.0);
    let h = normalize(l + vec3<f32>(0.0, 0.0, 1.0));
    let spec = pow(max(dot(n, h), 0.0), 24.0) * 0.7;
    let lit = in.color.rgb * diffuse + vec3<f32>(spec);
    let halo_d = max(r - 1.0, 0.0) / 0.8;
    let halo = g.look.x * exp(-halo_d * halo_d) * (1.0 - cover);
    return vec4<f32>(lit * cover + in.color.rgb * halo, cover);
}

// ---- Wall flashes (instanced additive glows) ----

struct FlashIn {
    @location(0) center: vec2<f32>,
    @location(1) size: f32,
    @location(2) color: vec4<f32>,
};

struct FlashOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_flash(@builtin(vertex_index) i: u32, f: FlashIn) -> FlashOut {
    let corner = quad_corner(i);
    var out: FlashOut;
    out.pos = to_clip(f.center + corner * f.size);
    out.local = corner;
    out.color = f.color;
    return out;
}

@fragment
fn fs_flash(in: FlashOut) -> @location(0) vec4<f32> {
    let r2 = dot(in.local, in.local);
    let v = exp(-r2 * 5.0) * (1.0 - smoothstep(0.8, 1.0, sqrt(r2)));
    // Rule flashes (alpha above 1) keep their own strength.
    let strength = select(g.look.z, 1.0, in.color.a > 1.5);
    return vec4<f32>(in.color.rgb * v * strength, 0.0);
}

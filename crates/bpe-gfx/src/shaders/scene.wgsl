// Scene passes, all drawn into the HDR target in linear light.
// World coordinates are video pixels, origin at the frame centre, y down.

struct Globals {
    // x: 2 / video width, y: 2 / video height, z: world units per output pixel, w: time
    view: vec4<f32>,
    bg_top: vec4<f32>,
    bg_bottom: vec4<f32>,
    // x: bloom strength, y: exposure
    post: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;

fn to_clip(p: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(p.x * g.view.x, -p.y * g.view.y, 0.0, 1.0);
}

fn quad_corner(i: u32) -> vec2<f32> {
    return vec2<f32>(f32(i & 1u), f32((i >> 1u) & 1u)) * 2.0 - 1.0;
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

// ---- Walls (instanced quads with a distance function) ----

const PI: f32 = 3.14159265;
const TAU: f32 = 6.28318531;
// How far a wall's glow reaches, in world pixels.
const GLOW_REACH: f32 = 36.0;

struct ShapeIn {
    // Centre xy, angle.
    @location(0) a: vec4<f32>,
    // Ring: radius, sides. Box: half size, corner radius. Capsule: half length.
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
    var extent = s.b.x;
    if s.c.x > 0.5 {
        extent = length(s.b.xy);
    }
    let world = s.a.xy + corner * (extent + reach);
    var out: ShapeOut;
    out.pos = to_clip(world);
    out.world = world;
    out.a = s.a;
    out.b = s.b;
    out.c = s.c;
    out.color = s.color;
    return out;
}

// How much a pixel hides what is behind it. A negative colour alpha is an additive colour: it
// adds its light and hides nothing.
fn covering(cover: f32, color_alpha: f32) -> f32 {
    return select(cover, 0.0, color_alpha < 0.0);
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

// Point on a ring or regular polygon `u` radians clockwise from `angle`, relative to its centre.
fn outline_point(radius: f32, sides: f32, angle: f32, u: f32) -> vec2<f32> {
    let d = vec2<f32>(cos(angle + u), sin(angle + u));
    if sides < 2.5 {
        return d * radius;
    }
    let step = TAU / sides;
    let side = round(u / step);
    return d * (radius * cos(PI / sides) / cos(u - side * step));
}

// Distance from p (relative to the centre) to a ring or polygon outline.
fn ring_distance(p: vec2<f32>, radius: f32, sides: f32, angle: f32) -> f32 {
    if sides < 2.5 {
        return abs(length(p) - radius);
    }
    // The nearest side of a regular polygon is the one in p's own sector.
    let u = atan2(p.y, p.x) - angle;
    let u_wrapped = u - TAU * floor((u + PI) / TAU);
    let step = TAU / sides;
    let side = round(u_wrapped / step);
    let a = outline_point(radius, sides, angle, (side - 0.5) * step);
    let b = outline_point(radius, sides, angle, (side + 0.5) * step);
    return segment_distance(p, a, b);
}

// Distance from a pixel to a wall's centre line.
fn shape_distance(in: ShapeOut) -> f32 {
    if in.c.x < 0.5 {
        return ring_distance(in.world - in.a.xy, in.b.x, in.b.y, in.a.z);
    }
    let local = rotate(in.world - in.a.xy, -in.a.z);
    if in.c.x < 1.5 {
        // A box, its corners rounded by b.z.
        let corner = in.b.z;
        let q = abs(local) - (in.b.xy - vec2<f32>(corner));
        return abs(length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - corner);
    }
    // A capsule: the segment along x from -b.x to b.x.
    return segment_distance(local, vec2<f32>(-in.b.x, 0.0), vec2<f32>(in.b.x, 0.0));
}

@fragment
fn fs_shape(in: ShapeOut) -> @location(0) vec4<f32> {
    let px = g.view.z;
    let d = shape_distance(in);
    let width = in.c.y * 2.0;
    let half_width = max(in.c.y, 0.5 * px);
    // Lines thinner than a pixel get dimmer instead of aliasing.
    let cover = clamp((half_width - d) / px + 0.5, 0.0, 1.0) * min(width / px, 1.0) * abs(in.color.a);
    let glow = exp(-max(d - half_width, 0.0) / 6.0) * in.c.z;
    if cover <= 0.0 && glow < 1e-4 {
        discard;
    }
    return vec4<f32>(in.color.rgb * (cover + glow), covering(cover, in.color.a));
}

// ---- Balls (instanced quads, shaded like small spheres with a soft halo) ----

const BALL_EXTENT: f32 = 3.0;

struct BallIn {
    @location(0) center: vec2<f32>,
    @location(1) radius: f32,
    @location(2) glow: f32,
    @location(3) color: vec4<f32>,
};

struct BallOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) radius_px: f32,
    @location(3) glow: f32,
};

@vertex
fn vs_ball(@builtin(vertex_index) i: u32, b: BallIn) -> BallOut {
    let corner = quad_corner(i) * BALL_EXTENT;
    var out: BallOut;
    out.pos = to_clip(b.center + corner * b.radius);
    out.local = corner;
    out.color = b.color;
    out.radius_px = b.radius / g.view.z;
    out.glow = b.glow;
    return out;
}

// Shiny sphere lighting for a point `p` on a unit disc, lit from the upper left.
fn sphere_light(p: vec2<f32>, color: vec3<f32>) -> vec3<f32> {
    let r = length(p);
    let n = vec3<f32>(p, sqrt(max(1.0 - r * r, 0.0)));
    // Light from the upper left (y is down in world space).
    let l = normalize(vec3<f32>(-0.45, -0.55, 0.7));
    let diffuse = 0.45 + 0.8 * max(dot(n, l), 0.0);
    let h = normalize(l + vec3<f32>(0.0, 0.0, 1.0));
    let spec = pow(max(dot(n, h), 0.0), 24.0) * 0.7;
    return color * diffuse + vec3<f32>(spec);
}

@fragment
fn fs_ball(in: BallOut) -> @location(0) vec4<f32> {
    let r = length(in.local);
    let aa = 1.0 / max(in.radius_px, 0.5);
    let cover = clamp((1.0 - r) / aa + 0.5, 0.0, 1.0);
    let lit = sphere_light(in.local, in.color.rgb);
    let halo_d = max(r - 1.0, 0.0) / 0.8;
    let halo = in.glow * exp(-halo_d * halo_d) * (1.0 - cover);
    if cover <= 0.0 && halo < 1e-4 {
        discard;
    }
    if in.color.a < 0.0 {
        // Additive: the lit ball's light is added at the colour's strength.
        let k = -in.color.a;
        return vec4<f32>((lit * cover + in.color.rgb * halo) * k, 0.0);
    }
    return vec4<f32>(lit * cover + in.color.rgb * halo, cover);
}

// ---- Flashes (instanced additive glows, strength in the colour) ----

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
    return vec4<f32>(in.color.rgb * v, 0.0);
}

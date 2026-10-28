// Scene passes, all drawn into the HDR target in linear light.
// World coordinates are video pixels, origin at the frame centre, y down.

struct Globals {
    // x: 2 / video width, y: 2 / video height, z: world units per output pixel, w: time
    view: vec4<f32>,
    bg_top: vec4<f32>,
    bg_bottom: vec4<f32>,
    // xy: centre, z: radius (circle) or half width (rect), w: line thickness
    container: vec4<f32>,
    // x: 0 circle / 1 rect, y: half height (rect)
    container_extra: vec4<f32>,
    container_color: vec4<f32>,
    // x: ball glow, y: trail opacity, z: flash strength, w: bloom strength
    look: vec4<f32>,
    // x: exposure
    look2: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;

fn to_clip(p: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(p.x * g.view.x, -p.y * g.view.y, 0.0, 1.0);
}

// ---- Background gradient and container outline (fullscreen) ----

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

fn container_distance(p: vec2<f32>) -> f32 {
    let c = g.container.xy;
    if g.container_extra.x < 0.5 {
        return abs(length(p - c) - g.container.z);
    }
    let q = abs(p - c) - vec2<f32>(g.container.z, g.container_extra.y);
    return abs(length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0));
}

@fragment
fn fs_background(in: BgOut) -> @location(0) vec4<f32> {
    let t = clamp(in.world.y * g.view.y * 0.5 + 0.5, 0.0, 1.0);
    var color = mix(g.bg_top.rgb, g.bg_bottom.rgb, t);
    let px = g.view.z;
    let d = container_distance(in.world);
    let half_thickness = max(g.container.w * 0.5, 0.5 * px);
    let line = clamp((half_thickness - d) / px + 0.5, 0.0, 1.0) * min(g.container.w / px, 1.0);
    let glow = exp(-max(d - half_thickness, 0.0) / 6.0) * 0.015;
    color = mix(color, g.container_color.rgb, line * g.container_color.a) + g.container_color.rgb * glow;
    return vec4<f32>(color, 1.0);
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
    return vec4<f32>(in.color.rgb * v * g.look.z, 0.0);
}

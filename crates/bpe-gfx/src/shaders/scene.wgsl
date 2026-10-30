// Scene passes, all drawn into the HDR target in linear light.
// World coordinates are video pixels, origin at the frame centre, y down.

struct Globals {
    // x: 2 / video width, y: 2 / video height, z: world units per output pixel, w: time
    view: vec4<f32>,
    // The background paint: atlas row, start, scroll, repeat.
    bg_paint: vec4<f32>,
    // The background place: direction, span, centre xy.
    bg_place: vec4<f32>,
    // x: bloom strength, y: exposure
    post: vec4<f32>,
    // centre xy, zoom, turn
    camera: vec4<f32>,
    // x: background on screen, y: how its place runs, z: steps, w: 1 to mix the two colours below
    bg: vec4<f32>,
    bg_from: vec4<f32>,
    bg_to: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;

// The camera (centre xy, zoom, turn) turns a world point into screen units; with no camera
// (centre 0, zoom 1, turn 0) the point is unchanged, exactly.
fn to_screen(p: vec2<f32>) -> vec2<f32> {
    let c = g.camera;
    var d = p - c.xy;
    if c.w != 0.0 {
        let s = sin(-c.w);
        let k = cos(-c.w);
        d = vec2<f32>(d.x * k - d.y * s, d.x * s + d.y * k);
    }
    return d * c.z;
}

fn to_clip(p: vec2<f32>) -> vec4<f32> {
    let q = to_screen(p);
    return vec4<f32>(q.x * g.view.x, -q.y * g.view.y, 0.0, 1.0);
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

// The world point under a screen point: `to_screen` backwards.
fn screen_to_world(q: vec2<f32>) -> vec2<f32> {
    let c = g.camera;
    var d = q / c.z;
    if c.w != 0.0 {
        let s = sin(c.w);
        let k = cos(c.w);
        d = vec2<f32>(d.x * k - d.y * s, d.x * s + d.y * k);
    }
    return d + c.xy;
}

@fragment
fn fs_background(in: BgOut) -> @location(0) vec4<f32> {
    if g.bg_paint.x < 0.0 {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    // `in.world` is in screen units here; the place is measured there, or in the world.
    var along = 0.0;
    if g.bg.y > 0.5 {
        var at = in.world;
        if g.bg.x == 0.0 {
            at = screen_to_world(in.world);
        }
        let d = at - g.bg_place.zw;
        let span = max(g.bg_place.y, 0.001);
        if g.bg.y < 1.5 {
            let a = radians(g.bg_place.x);
            along = dot(d, vec2<f32>(cos(a), sin(a))) / span + 0.5;
        } else {
            along = length(d) / span;
        }
    }
    if g.bg.w > 0.5 {
        // A plain blend of two colours, mixed here without the atlas's half floats.
        let t = paint_wrap(along + g.bg_paint.y + g.view.w * g.bg_paint.z, g.bg_paint.w);
        return vec4<f32>(mix(g.bg_from.rgb, g.bg_to.rgb, t), 1.0);
    }
    let p = PaintGpu(g.bg_paint.x, g.bg_paint.y, 0.0, g.bg.z, 0.0, 1.0, g.bg_paint.z, g.bg_paint.w);
    return vec4<f32>(paint_along(p, along, g.view.w).rgb, 1.0);
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
    // The gradient painted on the line, as `PaintGpu`.
    @location(4) paint_a: vec4<f32>,
    @location(5) paint_b: vec4<f32>,
};

struct ShapeOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec2<f32>,
    @location(1) @interpolate(flat) a: vec4<f32>,
    @location(2) @interpolate(flat) b: vec4<f32>,
    @location(3) @interpolate(flat) c: vec4<f32>,
    @location(4) @interpolate(flat) color: vec4<f32>,
    @location(5) @interpolate(flat) paint_a: vec4<f32>,
    @location(6) @interpolate(flat) paint_b: vec4<f32>,
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
        // A box's half size, or a capsule's half length (its b.y is 0).
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
    out.paint_a = s.paint_a;
    out.paint_b = s.paint_b;
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

// Distance from a pixel to a wall's centre line, or to a capsule's spine.
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

// How far round a shape's line a pixel is, 0 to 1, for a gradient painted along it. A ring or
// polygon goes clockwise round its centre from its own angle; a box clockwise round its edge from
// the top-left corner (rounded corners counted as square); a capsule from one end to the other.
fn shape_path(in: ShapeOut) -> f32 {
    let local = rotate(in.world - in.a.xy, -in.a.z);
    if in.c.x < 0.5 {
        return fract(atan2(local.y, local.x) / TAU);
    }
    let half = max(in.b.xy, vec2<f32>(1e-3));
    if in.c.x < 1.5 {
        let p = clamp(local, -half, half);
        let q = abs(local) - half;
        // The perimeter walked so far: top, right, bottom, left.
        var walked = 0.0;
        if q.y >= q.x {
            if local.y < 0.0 {
                walked = p.x + half.x;
            } else {
                walked = 2.0 * half.x + 2.0 * half.y + (half.x - p.x);
            }
        } else if local.x > 0.0 {
            walked = 2.0 * half.x + (p.y + half.y);
        } else {
            walked = 4.0 * half.x + 2.0 * half.y + (half.y - p.y);
        }
        return fract(walked / (4.0 * (half.x + half.y)));
    }
    return clamp((local.x + in.b.x) / (2.0 * half.x), 0.0, 1.0);
}

// A pixel's place across a shape's bounding box, -1 to 1 each way, turned with the shape: what
// an inside pattern paints across.
fn shape_local(in: ShapeOut) -> vec2<f32> {
    var half = vec2<f32>(in.b.x);
    if in.c.x > 1.5 {
        half = vec2<f32>(in.b.x, 0.0);
    } else if in.c.x > 0.5 {
        half = in.b.xy;
    }
    half = max(half + vec2<f32>(in.c.y), vec2<f32>(1e-3));
    return rotate(in.world - in.a.xy, -in.a.z) / half;
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
    let paint = paint_from(in.paint_a, in.paint_b);
    if paint.row < 0.0 {
        return vec4<f32>(in.color.rgb * (cover + glow), covering(cover, in.color.a));
    }
    // A gradient painted on the line takes the place of its colour; the glow keeps the colour.
    var line = vec3<f32>(0.0);
    if paint.pattern > 3.5 {
        line = paint_pattern(paint, shape_path(in) * paint.scale, g.view.w).rgb;
    } else {
        line = paint_colour(paint, shape_local(in), g.view.w).rgb;
    }
    return vec4<f32>(line * cover + in.color.rgb * glow, covering(cover, in.color.a));
}

// ---- Balls (instanced quads, shaded like small spheres with a soft halo) ----

const BALL_EXTENT: f32 = 3.0;

struct BallIn {
    @location(0) center: vec2<f32>,
    @location(1) radius: f32,
    @location(2) glow: f32,
    @location(3) color: vec4<f32>,
    // The thing code plus half the seed, the seed, two spare.
    @location(4) code: vec4<f32>,
    // The gradient painted inside, as `PaintGpu`.
    @location(5) paint_a: vec4<f32>,
    @location(6) paint_b: vec4<f32>,
};

struct BallOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) radius_px: f32,
    @location(3) glow: f32,
    @location(4) @interpolate(flat) code: vec4<f32>,
    @location(5) @interpolate(flat) paint_a: vec4<f32>,
    @location(6) @interpolate(flat) paint_b: vec4<f32>,
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
    out.code = b.code;
    out.paint_a = b.paint_a;
    out.paint_b = b.paint_b;
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
    var base = in.color.rgb;
    let paint = paint_from(in.paint_a, in.paint_b);
    if paint.row >= 0.0 {
        // A gradient painted inside takes the place of the ball's colour; the light stays on top.
        base = paint_colour(paint, in.local, g.view.w).rgb;
    }
    let lit = sphere_light(in.local, base);
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

// ---- Soft bodies (plain triangles whose edges fade over the last pixel) ----

struct SoftIn {
    @location(0) position: vec2<f32>,
    // How far inside the body's edge, in world units.
    @location(1) inside: f32,
    @location(2) pad: f32,
    @location(3) color: vec4<f32>,
    // The gradient painted across the body, as `PaintGpu`; the same at every corner.
    @location(4) paint_a: vec4<f32>,
    @location(5) paint_b: vec4<f32>,
    // Where the corner is in the body's bounding box, -1 to 1.
    @location(6) local: vec2<f32>,
};

struct SoftOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) inside: f32,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) paint_a: vec4<f32>,
    @location(3) @interpolate(flat) paint_b: vec4<f32>,
    @location(4) local: vec2<f32>,
};

@vertex
fn vs_soft(v: SoftIn) -> SoftOut {
    var out: SoftOut;
    out.pos = to_clip(v.position);
    out.inside = v.inside;
    out.color = v.color;
    out.paint_a = v.paint_a;
    out.paint_b = v.paint_b;
    out.local = v.local;
    return out;
}

@fragment
fn fs_soft(in: SoftOut) -> @location(0) vec4<f32> {
    let cover = clamp(in.inside / g.view.z + 0.5, 0.0, 1.0);
    let a = abs(in.color.a) * cover;
    if a <= 0.0 {
        discard;
    }
    let paint = paint_from(in.paint_a, in.paint_b);
    if paint.row < 0.0 {
        return vec4<f32>(in.color.rgb * a, covering(a, in.color.a));
    }
    // A gradient painted across the body takes the place of its colours.
    let rgb = paint_colour(paint, in.local, g.view.w).rgb;
    return vec4<f32>(rgb * a, covering(a, in.color.a));
}

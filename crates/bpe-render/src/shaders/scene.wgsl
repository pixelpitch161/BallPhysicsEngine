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

// Trails of styled balls carry their own brightness in their colour.
@fragment
fn fs_styled_trail(in: TrailOut) -> @location(0) vec4<f32> {
    let px = g.view.z;
    let half_width = max(in.edge.y, 0.5 * px);
    let thin = min(in.edge.y / (0.5 * px), 1.0);
    let cover = clamp((half_width - abs(in.edge.x)) / px + 0.5, 0.0, 1.0) * thin;
    let a = in.color.a * cover;
    return vec4<f32>(in.color.rgb * a, 0.0);
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
    @location(3) color2: vec4<f32>,
    @location(4) outline_color: vec4<f32>,
    // Shape, fill (-1 classic), pattern size, roundness.
    @location(5) form: vec4<f32>,
    // Outline width in pixels, glow, opacity, pattern angle.
    @location(6) look: vec4<f32>,
    // Stretch direction, length along and across it in radii.
    @location(7) stretch: vec4<f32>,
    // Quad reach in radii, glow reach in radii.
    @location(8) extent: vec4<f32>,
};

struct BallOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) radius_px: f32,
    @location(3) @interpolate(flat) color2: vec4<f32>,
    @location(4) @interpolate(flat) outline_color: vec4<f32>,
    @location(5) @interpolate(flat) form: vec4<f32>,
    @location(6) @interpolate(flat) look: vec4<f32>,
    // Stretch direction, glow reach, outline width in radii.
    @location(7) @interpolate(flat) misc: vec4<f32>,
};

fn quad_corner(i: u32) -> vec2<f32> {
    return vec2<f32>(f32(i & 1u), f32((i >> 1u) & 1u)) * 2.0 - 1.0;
}

@vertex
fn vs_ball(@builtin(vertex_index) i: u32, b: BallIn) -> BallOut {
    var reach = BALL_EXTENT;
    if b.form.y >= 0.0 {
        reach = b.extent.x;
    }
    let corner = quad_corner(i) * reach;
    let dir = b.stretch.xy;
    let side = vec2<f32>(-dir.y, dir.x);
    let offset = dir * corner.x * b.stretch.z + side * corner.y * b.stretch.w;
    var out: BallOut;
    out.pos = to_clip(b.center + offset * b.radius);
    out.local = corner;
    out.color = b.color;
    out.radius_px = b.radius / g.view.z;
    out.color2 = b.color2;
    out.outline_color = b.outline_color;
    out.form = b.form;
    out.look = b.look;
    out.misc = vec4<f32>(dir, b.extent.y, b.look.x / max(b.radius, 1e-3));
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
    if in.form.y >= 0.0 {
        return styled_ball(in);
    }
    let r = length(in.local);
    let aa = 1.0 / max(in.radius_px, 0.5);
    let cover = clamp((1.0 - r) / aa + 0.5, 0.0, 1.0);
    let lit = sphere_light(in.local, in.color.rgb);
    let halo_d = max(r - 1.0, 0.0) / 0.8;
    let halo = g.look.x * exp(-halo_d * halo_d) * (1.0 - cover);
    return vec4<f32>(lit * cover + in.color.rgb * halo, cover);
}

// ---- Styled balls: shapes as distance functions on a unit-radius ball, y up, tip on top ----

// A regular polygon with `n` sides and a corner on top, `r` from the centre to each corner.
fn sd_polygon(p: vec2<f32>, r: f32, n: f32) -> f32 {
    let an = PI / n;
    let acs = vec2<f32>(cos(an), sin(an));
    let a = atan2(p.x, p.y);
    let bn = a - 2.0 * an * floor(a / (2.0 * an)) - an;
    var q = length(p) * vec2<f32>(cos(bn), abs(sin(bn)));
    q -= r * acs;
    q.y += clamp(-q.y, 0.0, r * acs.y);
    return length(q) * sign(q.x);
}

fn sd_box(p: vec2<f32>, half: vec2<f32>) -> f32 {
    let q = abs(p) - half;
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0);
}

fn sd_star(p_in: vec2<f32>, r: f32, inner: f32) -> f32 {
    let k1 = vec2<f32>(0.809016994375, -0.587785252292);
    let k2 = vec2<f32>(-k1.x, k1.y);
    var p = vec2<f32>(abs(p_in.x), p_in.y);
    p -= 2.0 * max(dot(k1, p), 0.0) * k1;
    p -= 2.0 * max(dot(k2, p), 0.0) * k2;
    p.x = abs(p.x);
    p.y -= r;
    let ba = inner * vec2<f32>(-k1.y, k1.x) - vec2<f32>(0.0, 1.0);
    let h = clamp(dot(p, ba) / dot(ba, ba), 0.0, r);
    return length(p - ba * h) * sign(p.y * ba.x - p.x * ba.y);
}

fn sd_heart(p_in: vec2<f32>) -> f32 {
    let p = vec2<f32>(abs(p_in.x), p_in.y);
    if p.y + p.x > 1.0 {
        return length(p - vec2<f32>(0.25, 0.75)) - 0.35355339;
    }
    let a = p - vec2<f32>(0.0, 1.0);
    let b = p - 0.5 * max(p.x + p.y, 0.0);
    return sqrt(min(dot(a, a), dot(b, b))) * sign(p.x - p.y);
}

// Signed distance to the edge of a styled ball's shape, in radii; negative inside.
fn ball_shape(p: vec2<f32>, shape: f32, round: f32) -> f32 {
    // Distance functions here are written with y up.
    let q = vec2<f32>(p.x, -p.y);
    let k = round * 0.35;
    if shape < 0.5 {
        return length(q) - 1.0;
    } else if shape < 1.5 {
        return sd_box(q, vec2<f32>(0.84 - k)) - k;
    } else if shape < 2.5 {
        // Moved down a little so the triangle sits in the middle of the ball.
        return sd_polygon(q + vec2<f32>(0.0, 0.18), 1.3 - k * 2.0, 3.0) - k;
    } else if shape < 3.5 {
        // Narrower than tall, and no taller than the ball.
        let s = vec2<f32>(1.4, 1.0);
        return (sd_polygon(q * s, 1.12 - k, 4.0) - k) / 1.2;
    } else if shape < 4.5 {
        return sd_polygon(q, 1.06 - k, 6.0) - k;
    } else if shape < 5.5 {
        return sd_star(q + vec2<f32>(0.0, 0.08), 1.22 - k * 0.5, 0.45) - k * 0.5;
    } else if shape < 6.5 {
        return sd_heart(q / 2.0 + vec2<f32>(0.0, 0.52)) * 2.0;
    }
    return abs(length(q) - 0.72) - 0.28;
}

// 0 or 1 in alternating bands of `x`, softened over a pixel.
fn bands(x: f32) -> f32 {
    return bands_over(x, fwidth(x));
}

// The same, softened over `w`, for values that jump where an angle wraps around.
fn bands_over(x: f32, w_in: f32) -> f32 {
    let w = max(w_in, 1e-4);
    let f = fract(x);
    return clamp((abs(f - 0.5) - 0.25) / w + 0.5, 0.0, 1.0);
}

// 1 inside where signed distance `d` is negative, softened over a pixel.
fn inside(d: f32) -> f32 {
    return clamp(-d / max(fwidth(d), 1e-4) + 0.5, 0.0, 1.0);
}

// How much of the second colour shows at `p` (unit radius, y down) for a fill pattern.
fn pattern(p: vec2<f32>, fill: f32, size: f32) -> f32 {
    let r = length(p);
    let n = size * 0.5;
    if fill < 2.5 {
        return smoothstep(0.0, 1.0, r);
    } else if fill < 3.5 {
        return clamp(p.y * 0.5 + 0.5, 0.0, 1.0);
    } else if fill < 4.5 {
        return bands(p.x * n + 0.25);
    } else if fill < 5.5 {
        return bands(r * n + 0.25);
    } else if fill < 6.5 {
        let cell = fract(p * n + 0.5) - 0.5;
        return inside(length(cell) - 0.28);
    } else if fill < 7.5 {
        let a = bands(p.x * n + 0.25);
        let b = bands(p.y * n + 0.25);
        return a + b - 2.0 * a * b;
    } else if fill < 8.5 {
        return inside(-p.y);
    }
    // Patterns around the middle: a whole number of slice pairs, so the seam where the angle
    // wraps doesn't show.
    let turns = atan2(p.y, p.x) / TAU;
    let turn_width = length(fwidth(p)) / max(r, 1e-3) / TAU;
    let m = max(round(n), 1.0);
    if fill < 9.5 {
        return bands_over(turns * m, turn_width * m);
    } else if fill < 10.5 {
        return bands_over(turns * m + r * 1.2, turn_width * m + fwidth(r) * 1.2);
    }
    // A smiley face: two eyes and a grin.
    let eyes = length(vec2<f32>(abs(p.x) - 0.32, p.y + 0.22) / vec2<f32>(1.0, 1.4)) - 0.11;
    let mouth_ring = abs(length(p - vec2<f32>(0.0, -0.1)) - 0.5) - 0.065;
    let mouth = max(mouth_ring, 0.18 - p.y);
    return inside(min(eyes, mouth));
}

fn styled_ball(in: BallOut) -> vec4<f32> {
    let angle = in.look.w;
    let p = rotate(in.local, -angle);
    let d = ball_shape(p, in.form.x, in.form.w);
    let aa = 1.0 / max(in.radius_px, 0.5);
    let cover = clamp(-d / aa + 0.5, 0.0, 1.0);

    let fill = in.form.y;
    var color = in.color.rgb;
    if fill > 1.5 {
        color = mix(in.color.rgb, in.color2.rgb, pattern(p, fill, in.form.z));
    }
    if fill > 0.5 && fill < 1.5 {
        // Lit as the ball sits on screen, whichever way it is stretched.
        let dir = in.misc.xy;
        let screen = dir * in.local.x + vec2<f32>(-dir.y, dir.x) * in.local.y;
        color = sphere_light(screen / max(length(screen), 1.0), color);
    }
    let outline = in.misc.w;
    var glow_color = in.color.rgb;
    if outline > 0.0 {
        let fill_part = clamp((-d - outline) / aa + 0.5, 0.0, 1.0);
        color = mix(in.outline_color.rgb, color, fill_part);
        glow_color = in.outline_color.rgb;
    }
    let opacity = in.look.z;
    let glow_d = max(d, 0.0) / max(in.misc.z, 0.05);
    let halo = in.look.y * exp(-glow_d * glow_d) * (1.0 - cover);
    if cover <= 0.0 && halo < 1e-4 {
        discard;
    }
    return vec4<f32>((color * cover + glow_color * halo) * opacity, cover * opacity);
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

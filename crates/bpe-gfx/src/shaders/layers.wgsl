// Balls drawn with a stack of layers: each ball is one instance naming a range of layer records
// (`LayerGpu` in frame.rs) that its thing wrote once for the frame. The fragment walks them
// bottom to top and mixes each over the layers under it with that layer's blend (W3C
// compositing, `blended` from blend_maths.wgsl). Appended to scene.wgsl's module, with
// ball_look.wgsl's shapes, paint.wgsl's patterns and each.wgsl.
//
// A pixel on the ball's edge is part inside the shape and part outside. The stack is worked out
// twice, once as if the pixel were wholly inside (`inner`, where the body covers it all) and once
// as if wholly outside (`outer`, where only what reaches past the edge shows), and the two are
// mixed by how much of the pixel the shape covers, so layers on the body never thicken its edge.
// A glow is light added outside the shape with no coverage, as the halo always was.

struct LayerGpu {
    // Kind (0 body, 1 pattern, 2 outline, 3 shine, 4 glow, 5 shading, 6 shader, 7 mask), blend
    // code, flags, how many mask records follow it.
    head: vec4<u32>,
    // Opacity, then per kind (shine and glow: strength; mask: target record, parent angle, shape).
    amount: vec4<f32>,
    // Outline: width in pixels, where it sits (0 inside, 1 centre, 2 outside). Shine: spot xy,
    // size. Glow: size in radii. Shading: kind, softness, space, amount. Mask: from, invert,
    // softness, moves. Shader: speed, scale, space, effect slot.
    p0: vec4<f32>,
    // Shading: linear from xy, to xy; radial centre xy, size; brush tile in w. Mask: angle,
    // size, threshold, brush tile.
    p1: vec4<f32>,
    colour: vec4<f32>,
    paint: PaintGpu,
    // A shader layer's effect settings, packed as its header lists them.
    extra: array<vec4<f32>, 4>,
};

@group(2) @binding(0) var<storage, read> look_layers: array<LayerGpu>;
// Brush strokes, one 128 x 128 tile per layer of the array (see masks.rs).
@group(2) @binding(1) var mask_tiles: texture_2d_array<f32>;
@group(2) @binding(2) var mask_sampler: sampler;

// Matches MAX_LAYERS in frame.rs.
const MAX_LAYERS: u32 = 64u;

struct LayeredIn {
    @location(0) center: vec2<f32>,
    @location(1) radius: f32,
    @location(2) seed: f32,
    @location(3) body: vec4<f32>,
    @location(4) stretch: vec4<f32>,
    // Shape, roundness, angle, reach in radii.
    @location(5) form: vec4<f32>,
    // Opacity, thing code, heading in degrees, 1 when the shape faces its motion.
    @location(6) misc: vec4<f32>,
    @location(7) vary: vec4<f32>,
    // First record, count.
    @location(8) range: vec4<u32>,
};

struct LayeredOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) body: vec4<f32>,
    // The radius in output pixels, then in world units.
    @location(2) @interpolate(flat) radius: vec2<f32>,
    @location(3) @interpolate(flat) form: vec4<f32>,
    @location(4) @interpolate(flat) misc: vec4<f32>,
    @location(5) @interpolate(flat) vary: vec4<f32>,
    @location(6) @interpolate(flat) range: vec4<u32>,
    @location(7) @interpolate(flat) dir: vec2<f32>,
};

@vertex
fn vs_layers(@builtin(vertex_index) i: u32, b: LayeredIn) -> LayeredOut {
    let radius = max(b.radius, 1e-3);
    let margin = 2.0 * g.view.z / radius;
    let corner = quad_corner(i) * (max(b.form.w, 1.0) + margin);
    let dir = b.stretch.xy;
    let side = vec2<f32>(-dir.y, dir.x);
    let offset = dir * corner.x * b.stretch.z + side * corner.y * b.stretch.w;
    var out: LayeredOut;
    out.pos = to_clip(b.center + offset * b.radius);
    out.local = corner;
    out.body = b.body;
    out.radius = vec2<f32>(b.radius / g.view.z, radius);
    out.form = b.form;
    out.misc = b.misc;
    out.vary = b.vary;
    out.range = b.range;
    out.dir = dir;
    return out;
}

// `cs` (straight) at coverage `a` mixed over `acc` (premultiplied) by blend `mode`.
fn layer_over(acc: vec4<f32>, cs: vec3<f32>, a_in: f32, mode: u32) -> vec4<f32> {
    let a = clamp(a_in, 0.0, 1.0);
    let ab = acc.a;
    var mixed = cs;
    if mode != 0u && ab > 1e-5 {
        mixed = (1.0 - ab) * cs + ab * blended(mode, acc.rgb / ab, cs);
    }
    return vec4<f32>(a * mixed + (1.0 - a) * acc.rgb, a + ab * (1.0 - a));
}

// `color` lit like a sphere at `q` (-1 to 1 across the ball as it sits on screen), the light from
// `spot`; at the Shine layer's defaults exactly `sphere_light` in scene.wgsl.
fn shine_light(q: vec2<f32>, color: vec3<f32>, spot: vec2<f32>, size: f32) -> vec3<f32> {
    let r = length(q);
    let n = vec3<f32>(q, sqrt(max(1.0 - r * r, 0.0)));
    let l = normalize(vec3<f32>(spot, 0.7));
    let diffuse = 0.45 + 0.8 * max(dot(n, l), 0.0);
    let h = normalize(l + vec3<f32>(0.0, 0.0, 1.0));
    let spec = pow(max(dot(n, h), 0.0), 24.0 / max(size * size, 0.04)) * 0.7;
    return color * diffuse + vec3<f32>(spec);
}

// How much of a band from `lo` to `hi` (distances to the shape's edge, in radii) covers a point
// at `d`. `inside` works it out for the part of the pixel inside the shape, where an edge at or
// past the shape's own edge is the shape's edge and needs no softening of its own; outside, the
// other way round.
fn band(d: f32, lo: f32, hi: f32, aa: f32, inside: bool) -> f32 {
    if inside {
        let q = min(d, 0.0);
        if lo >= 0.0 {
            return 0.0;
        }
        let top = select(clamp((hi - q) / aa + 0.5, 0.0, 1.0), 1.0, hi >= 0.0);
        return clamp((q - lo) / aa + 0.5, 0.0, 1.0) * top;
    }
    let q = max(d, 0.0);
    if hi <= 0.0 {
        return 0.0;
    }
    let bottom = select(clamp((q - lo) / aa + 0.5, 0.0, 1.0), 1.0, lo <= 0.0);
    return bottom * clamp((hi - q) / aa + 0.5, 0.0, 1.0);
}

// The ball a fragment is drawing, for the walks over its stack (see layers_walk.wgsl): set once
// at the start of each fragment.
struct LayeredBall {
    body: vec4<f32>,
    // The radius in output pixels, then in world units.
    radius: vec2<f32>,
    form: vec4<f32>,
    misc: vec4<f32>,
    vary: vec4<f32>,
    dir: vec2<f32>,
    // One output pixel, in radii.
    aa: f32,
    // The ball's first record and how many it has.
    first: u32,
    count: u32,
};

var<private> layered_ball: LayeredBall;

// What a walk over the stack gives at one point: as if wholly inside the shape, and as if wholly
// outside it.
struct Walked {
    inner: vec4<f32>,
    outer: vec4<f32>,
};

const LAYERS_TAU: f32 = 6.28318531;

// How painted brush tile `tile` (-1: none) is at ball point `q` (-1 to 1 across its square).
fn tile_at(tile: f32, q: vec2<f32>) -> f32 {
    let uv = q * 0.5 + vec2<f32>(0.5);
    if tile < 0.0 || any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return 0.0;
    }
    return textureSampleLevel(mask_tiles, mask_sampler, uv, i32(tile + 0.5), 0.0).r;
}

// How light `c` (linear light) looks, 0 to 1: its luminance as the eye reads it, so a threshold of
// 0.5 sits at a middle grey.
fn layers_luminance(c: vec3<f32>) -> f32 {
    return pow(clamp(dot(c, vec3<f32>(0.2126, 0.7152, 0.0722)), 0.0, 1.0), 1.0 / 2.2);
}

// 1 well inside an edge at distance `d` (negative inside), 0 well outside, fading over `s`.
fn soft_inside(d: f32, s: f32) -> f32 {
    return 1.0 - smoothstep(-s, s, d);
}

// A Shape mask at mask point `m` (already turned by its angle): 0 half (the top), 1 wedge (round
// the top, `size` of a turn), 2 quarters (top left and bottom right), 3 ring, 4 circle, 5 band.
fn shape_mask(shape: f32, m: vec2<f32>, size: f32, s: f32) -> f32 {
    let r = length(m);
    switch u32(shape + 0.5) {
        case 0u: {
            return soft_inside(m.y, s);
        }
        case 1u: {
            let span = clamp(size, 0.0, 1.0) * LAYERS_TAU;
            if span >= LAYERS_TAU - 1e-4 {
                return 1.0;
            }
            let a = atan2(m.x, -m.y);
            return soft_inside((abs(a) - 0.5 * span) * r, s);
        }
        case 2u: {
            return soft_inside(-min(abs(m.x), abs(m.y)) * sign(m.x * m.y), s);
        }
        case 3u: {
            return soft_inside(abs(r - 0.7) - 0.5 * size, s);
        }
        case 4u: {
            return soft_inside(r - size, s);
        }
        default: {
            return soft_inside(abs(m.y) - 0.5 * size, s);
        }
    }
}

// How much a Shading layer `l` darkens at ball point `q` (or `screen_q` when it stays with the
// screen), before its amount.
fn shading_factor(l: LayerGpu, q: vec2<f32>, screen_q: vec2<f32>, aa: f32) -> f32 {
    let pt = select(q, screen_q, l.p0.z > 0.5);
    let soft = clamp(l.p0.y, 0.0, 1.0);
    let kind = u32(l.p0.x + 0.5);
    if kind == 0u {
        let along = l.p1.zw - l.p1.xy;
        let t = dot(pt - l.p1.xy, along) / max(dot(along, along), 1e-6);
        let e0 = 0.5 * (1.0 - soft);
        return 1.0 - smoothstep(e0, 1.0 - e0 + 1e-4, t);
    }
    if kind == 1u {
        let size = max(l.p1.z, 1e-3);
        let w = max(soft * size, aa);
        let f = smoothstep(size - w, size + 0.25 * w, length(pt - l.p1.xy));
        return select(f, 1.0 - f, (l.head.z & 2u) != 0u);
    }
    return tile_at(l.p1.w, pt);
}

// A mask record `m`'s value at the point: `below`, `chosen` the coverage (inside, outside) of the
// layer directly under its layer and of its chosen layer; `under` the stack under its layer.
fn mask_value(
    m: LayerGpu,
    q: vec2<f32>,
    screen_q: vec2<f32>,
    below: vec2<f32>,
    chosen: vec2<f32>,
    under_in: vec4<f32>,
    under_out: vec4<f32>,
    aa: f32,
) -> vec2<f32> {
    let soft = clamp(m.p0.z, 0.0, 1.0);
    var v = vec2<f32>(0.0);
    switch u32(m.p0.x + 0.5) {
        case 0u: {
            v = below;
        }
        case 1u: {
            v = chosen;
        }
        case 2u, 3u: {
            var pt = q;
            let moves = u32(m.p0.w + 0.5);
            if moves == 0u {
                pt = rotate(q, -m.amount.z);
            } else if moves == 2u {
                pt = screen_q;
            }
            if u32(m.p0.x + 0.5) == 2u {
                let s = max(0.5 * soft, aa);
                v = vec2<f32>(shape_mask(m.amount.w, rotate(pt, -m.p1.x), m.p1.y, s));
            } else {
                v = vec2<f32>(tile_at(m.p1.w, pt));
            }
        }
        default: {
            let t = m.p1.z;
            let s = max(0.5 * soft, 1e-3);
            let li = layers_luminance(select(vec3<f32>(0.0), under_in.rgb / under_in.a, under_in.a > 1e-5));
            let lo = layers_luminance(select(vec3<f32>(0.0), under_out.rgb / under_out.a, under_out.a > 1e-5));
            v = vec2<f32>(smoothstep(t - s, t + s, li), smoothstep(t - s, t + s, lo));
            if u32(m.p0.x + 0.5) == 5u {
                v = vec2<f32>(1.0) - v;
            }
        }
    }
    if m.p0.y > 0.5 {
        v = vec2<f32>(1.0) - v;
    }
    return clamp(v, vec2<f32>(0.0), vec2<f32>(1.0));
}

// What `fs_layers` and `fs_layers_each` draw: premultiplied colour and coverage.
fn shade_layers(in: LayeredOut) -> vec4<f32> {
    let aa = 1.0 / max(in.radius.x, 0.5);
    layered_ball = LayeredBall(in.body, in.radius, in.form, in.misc, in.vary, in.dir, aa, in.range.x, in.range.y);
    let p = rotate(in.local, -in.form.z);
    let d = ball_shape(p, in.form.x, in.form.y);
    let cover = clamp(-d / aa + 0.5, 0.0, 1.0);
    let w = walk_layers(in.local, in.range.x, in.range.y);
    var out = cover * w.inner + (1.0 - cover) * w.outer;
    // An additive body colour makes the whole ball add its light.
    if in.body.a < 0.0 {
        out = vec4<f32>(out.rgb * -in.body.a, 0.0);
    }
    out *= clamp(in.misc.x, 0.0, 1.0);
    if out.a < 1e-4 && max(out.r, max(out.g, out.b)) < 1e-4 {
        discard;
    }
    return out;
}

@fragment
fn fs_layers(in: LayeredOut) -> @location(0) vec4<f32> {
    return shade_layers(in);
}

// A layered ball's own position: -1 to 1 across it, turning with its shape.
@fragment
fn fs_layers_object(in: LayeredOut) -> @location(0) vec4<f32> {
    let p = rotate(in.local, -in.form.z);
    let aa = 1.0 / max(in.radius.x, 0.5);
    let cover = clamp(-ball_shape(p, in.form.x, in.form.y) / aa + 0.5, 0.0, 1.0) * step(1e-4, in.misc.x);
    if cover <= 0.0 || in.misc.y < 0.5 {
        discard;
    }
    return vec4<f32>(p, in.misc.y, cover);
}

// In a layer whose things blend with each other (see each.wgsl).
@fragment
fn fs_layers_each(in: LayeredOut) -> @location(0) vec4<f32> {
    return each_out(shade_layers(in), g.post.w);
}

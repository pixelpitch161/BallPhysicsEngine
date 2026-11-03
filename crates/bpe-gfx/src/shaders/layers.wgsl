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
    // Kind (0 body, 1 pattern, 2 outline, 3 shine, 4 glow), blend code, flags, spare.
    head: vec4<u32>,
    // Opacity, then per kind (shine and glow: strength).
    amount: vec4<f32>,
    // Outline: width in pixels, where it sits (0 inside, 1 centre, 2 outside). Shine: spot xy,
    // size. Glow: size in radii.
    p0: vec4<f32>,
    p1: vec4<f32>,
    colour: vec4<f32>,
    paint: PaintGpu,
};

@group(2) @binding(0) var<storage, read> look_layers: array<LayerGpu>;

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

// What `fs_layers` and `fs_layers_each` draw: premultiplied colour and coverage.
fn shade_layers(in: LayeredOut) -> vec4<f32> {
    let p = rotate(in.local, -in.form.z);
    let d = ball_shape(p, in.form.x, in.form.y);
    let aa = 1.0 / max(in.radius.x, 0.5);
    let cover = clamp(-d / aa + 0.5, 0.0, 1.0);
    let body = in.body;
    let base = vec4<f32>(body.rgb, 1.0);
    // Lit as the ball sits on screen, whichever way it is stretched, as the classic ball was.
    let screen = in.dir * in.local.x + vec2<f32>(-in.dir.y, in.dir.x) * in.local.y;
    let lit_at = screen / max(length(screen), 1.0);

    var inner = vec4<f32>(0.0);
    var outer = vec4<f32>(0.0);
    let first = in.range.x;
    let count = min(in.range.y, MAX_LAYERS);
    for (var k = 0u; k < count; k++) {
        let l = look_layers[first + k];
        let mode = l.head.y;
        let flag = (l.head.z & 1u) != 0u;
        let opacity = clamp(l.amount.x, 0.0, 1.0);
        switch l.head.x {
            case 0u: {
                inner = layer_over(inner, body.rgb, opacity, mode);
            }
            case 1u: {
                var paint = l.paint;
                if paint.row >= 0.0 {
                    paint.vary = in.vary;
                }
                if flag {
                    paint.angle += in.misc.z;
                }
                if in.misc.w > 0.5 {
                    paint.angle -= in.misc.z;
                }
                if paint_paints(paint) {
                    inner = layer_over(inner, paint_colour(paint, p, g.view.w, base).rgb, opacity, mode);
                }
            }
            case 2u: {
                let w = max(l.p0.x, 0.0) / in.radius.y;
                if w > 0.0 {
                    var lo = -w;
                    var hi = 0.0;
                    if l.p0.y > 1.5 {
                        lo = 0.0;
                        hi = w;
                    } else if l.p0.y > 0.5 {
                        lo = -0.5 * w;
                        hi = 0.5 * w;
                    }
                    // Same as body: the body's colour a little darker, so a white ball shows it.
                    let c = select(l.colour.rgb, body.rgb * 0.6, flag);
                    inner = layer_over(inner, c, band(d, lo, hi, aa, true) * opacity, mode);
                    outer = layer_over(outer, c, band(d, lo, hi, aa, false) * opacity, mode);
                }
            }
            case 3u: {
                let under = select(body.rgb, inner.rgb / inner.a, inner.a > 1e-5);
                let lit = shine_light(lit_at, under, l.p0.xy, l.p0.z);
                inner = layer_over(inner, mix(under, lit, clamp(l.amount.y, 0.0, 1.0)), opacity, mode);
            }
            case 4u: {
                let glow_d = max(d, 0.0) / max(l.p0.x, 0.05);
                let halo = max(l.amount.y, 0.0) * exp(-glow_d * glow_d) * opacity;
                let c = select(l.colour.rgb, body.rgb, flag);
                // Light, covering nothing: mixed with what reaches past the edge, then added.
                var mixed = c;
                if mode != 0u && outer.a > 1e-5 {
                    mixed = (1.0 - outer.a) * c + outer.a * blended(mode, outer.rgb / outer.a, c);
                }
                outer = vec4<f32>(outer.rgb + mixed * halo, outer.a);
            }
            default: {}
        }
    }
    var out = cover * inner + (1.0 - cover) * outer;
    // An additive body colour makes the whole ball add its light.
    if body.a < 0.0 {
        out = vec4<f32>(out.rgb * -body.a, 0.0);
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

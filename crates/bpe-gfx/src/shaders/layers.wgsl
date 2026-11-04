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
    // Kind (0 body, 1 pattern, 2 outline, 3 shine, 4 glow, 5 shading, 6 shader, 8 group colour,
    // 9 group shader, 20..29 sub-layers: see sub_layers.wgsl), blend code, flags (1 kind's own, 2
    // and 4 see LayerGpu, 8 the group colour mixes into this layer, 16 the ball doesn't wear it),
    // how many sub-layer records follow it.
    head: vec4<u32>,
    // Opacity, then per kind (shine and glow: strength; cut: target record, parent angle, shape;
    // a drawn layer a group shader changes: the first group record and a bit mask of them).
    amount: vec4<f32>,
    // Outline: width in pixels, where it sits (0 inside, 1 centre, 2 outside). Shine: spot xy,
    // size. Glow: size in radii. Shading: kind, softness, space, amount. Cut: from, invert,
    // softness, moves. Shader: speed, scale, space, effect slot.
    p0: vec4<f32>,
    // Shading: linear from xy, to xy; radial centre xy, size; brush tile in w. Cut: angle,
    // size, threshold, brush tile.
    p1: vec4<f32>,
    colour: vec4<f32>,
    paint: PaintGpu,
    // A shader layer's effect settings, packed as its header lists them.
    extra: array<vec4<f32>, 4>,
};

@group(2) @binding(0) var<storage, read> look_layers: array<LayerGpu>;
// Brush strokes, one 128 x 128 tile per layer of the array (see masks.rs): how painted (r) and
// the distance to the painted edge (g).
@group(2) @binding(1) var mask_tiles: texture_2d_array<f32>;
@group(2) @binding(2) var mask_sampler: sampler;

// Matches MAX_LAYERS in frame.rs.
const MAX_LAYERS: u32 = 128u;

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
    // First record, count, spare, group mode (0 none, else 1 + blend code).
    @location(8) range: vec4<u32>,
    // The ball's group colour (straight, linear) and how much of it mixes in.
    @location(9) group: vec4<f32>,
    // Where the ball started from its group's middle, and the group's side (world units).
    @location(10) place: vec4<f32>,
    // Seconds since its last hit, that hit's strength, how many hits, spare.
    @location(11) hit: vec4<f32>,
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
    @location(8) @interpolate(flat) group: vec4<f32>,
    @location(9) @interpolate(flat) place: vec4<f32>,
    @location(10) @interpolate(flat) hit: vec4<f32>,
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
    out.group = b.group;
    out.place = b.place;
    out.hit = b.hit;
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
fn shine_light(q: vec2<f32>, color: vec3<f32>, spot: vec2<f32>, size: f32, light: vec3<f32>) -> vec3<f32> {
    let r = length(q);
    let n = vec3<f32>(q, sqrt(max(1.0 - r * r, 0.0)));
    let l = normalize(vec3<f32>(spot, 0.7));
    let diffuse = 0.45 + 0.8 * max(dot(n, l), 0.0);
    let h = normalize(l + vec3<f32>(0.0, 0.0, 1.0));
    let spec = pow(max(dot(n, h), 0.0), 24.0 / max(size * size, 0.04)) * 0.7;
    return color * diffuse + light * spec;
}

// The Shine layer's light on `color`: `shine_light` for Glossy (material 0) at sharpness 0,
// otherwise its spot's exponent times `1 + sharpness` (above 0 sharper, below softer and wider),
// and the light of Matte (1), Metal (2) or Glass (3). The kit turns Softness into sharpness. The
// light's own colour `light` (white for the classic shine) tints its spot, sheen, glint and rim.
fn shine_material(
    q: vec2<f32>,
    color: vec3<f32>,
    spot: vec2<f32>,
    size: f32,
    material: f32,
    sharpness: f32,
    light: vec3<f32>,
) -> vec3<f32> {
    let m = u32(material + 0.5);
    if m == 0u && abs(sharpness) < 1e-4 {
        return shine_light(q, color, spot, size, light);
    }
    let r = length(q);
    let n = vec3<f32>(q, sqrt(max(1.0 - r * r, 0.0)));
    let l = normalize(vec3<f32>(spot, 0.7));
    let h = normalize(l + vec3<f32>(0.0, 0.0, 1.0));
    let lit = max(dot(n, l), 0.0);
    let facing = max(dot(n, h), 0.0);
    let sharp = max(1.0 + sharpness, 0.05);
    let power = 24.0 / max(size * size, 0.04) * sharp;
    let peak = pow(sharp, 0.35);
    switch m {
        case 1u: {
            // Matte: wide, even light that wraps round, and a faint broad sheen.
            let wrap = clamp((dot(n, l) + 0.4) / 1.4, 0.0, 1.0);
            let sheen = pow(facing, max(power * 0.15, 1.0)) * 0.12 * peak;
            return color * (0.3 + 0.85 * wrap) + light * sheen;
        }
        case 2u: {
            // Metal: dark sides, a reflected horizon across the ball, a spot in the ball's colour.
            let horizon = smoothstep(-0.25, 0.15, n.y + 0.35 * spot.y);
            let env = mix(0.85, 0.12, horizon) + 0.25 * pow(1.0 - n.z, 2.0);
            let spec = pow(facing, power * 1.5) * peak;
            return color * (0.1 + 0.55 * lit + 0.7 * env) + (0.35 + 0.9 * color) * light * spec;
        }
        case 3u: {
            // Glass: a darker middle, a bright rim, a sharp white spot and a faint glint opposite.
            let rim = pow(1.0 - n.z, 3.0);
            let spec = pow(facing, power * 3.0) * 1.1 * peak;
            let back = normalize(vec3<f32>(-spot, 0.9));
            let glint = pow(max(dot(n, normalize(back + vec3<f32>(0.0, 0.0, 1.0))), 0.0), power * 0.8) * 0.25;
            return color * (0.45 + 0.25 * lit) + (0.5 + 0.5 * color) * light * rim * 0.8 + light * (spec + glint);
        }
        default: {
            let diffuse = 0.45 + 0.8 * lit;
            return color * diffuse + light * (pow(facing, power) * 0.7 * peak);
        }
    }
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
    // The ball's group colour (straight, linear) and how much of it mixes in, and the group mode
    // (0 none, else 1 + blend code).
    group: vec4<f32>,
    group_mode: u32,
    // Where the ball started from its group's middle, and the group's side (world units).
    place: vec4<f32>,
    // Seconds since its last hit, that hit's strength, how many hits (for sub-layers).
    hit: vec4<f32>,
};

var<private> layered_ball: LayeredBall;

// `c` (straight) with the ball's group colour mixed in by the instance's group mode and amount.
fn with_group(c: vec3<f32>) -> vec3<f32> {
    let mode = layered_ball.group_mode;
    if mode == 0u {
        return c;
    }
    var r = layered_ball.group.rgb;
    if mode > 1u {
        r = blended(mode - 1u, c, layered_ball.group.rgb);
    }
    return mix(c, r, clamp(layered_ball.group.a, 0.0, 1.0));
}

// The finished `acc` (premultiplied) with the group colour mixed over what it covers; light with
// no coverage (a glow) is tinted as a colour at its own brightness.
fn group_over(acc: vec4<f32>) -> vec4<f32> {
    if acc.a > 1e-5 {
        return vec4<f32>(with_group(acc.rgb / acc.a) * acc.a, acc.a);
    }
    let m = max(acc.r, max(acc.g, acc.b));
    if m <= 1e-5 {
        return acc;
    }
    return vec4<f32>(with_group(acc.rgb / m) * m, acc.a);
}


// The colour of a layer whose own colours are a gradient (`l.paint.row` 0 or more) at ball point
// `p`: straight across the ball (pattern 1), or outward (2) at `out` (0 to 1, worked out for the
// layer); `plain` for a layer of one colour.
fn own_colour(l: LayerGpu, p: vec2<f32>, out: f32, plain: vec3<f32>) -> vec3<f32> {
    if l.paint.row < 0.0 {
        return plain;
    }
    if l.paint.pattern > 1.5 {
        return paint_along(l.paint, out, g.view.w).rgb;
    }
    return paint_colour(l.paint, p, g.view.w, vec4<f32>(plain, 1.0)).rgb;
}

// How much the layers above a split thing's Blend layer cover, inside its shape and outside it,
// left by the walk of its lower part.
var<private> TOP_COVER: vec2<f32>;

// What a thing covering `cover` of the pixel draws there, walked as `w` for part `split` (see
// layers_walk.wgsl): its inside and outside mixed by `cover`. The lower part of a thing split by
// its Blend layer leaves out what its upper part covers (TOP_COVER), so that the upper part drawn
// over it adds up to the thing drawn whole: at an edge the lower part would otherwise show
// through the soft edge of a layer over it, a thin ring round an Inside outline.
fn split_lower_mixed(cover: f32, w: Walked, split: u32) -> vec4<f32> {
    let lower = (split & 0xffffu) > 0u && (split & 0x10000u) == 0u;
    if !lower {
        return cover * w.inner + (1.0 - cover) * w.outer;
    }
    let t = clamp(TOP_COVER, vec2<f32>(0.0), vec2<f32>(1.0));
    let left = 1.0 - (cover * t.x + (1.0 - cover) * t.y);
    if left < 1e-4 {
        return vec4<f32>(0.0);
    }
    return (cover * (1.0 - t.x) * w.inner + (1.0 - cover) * (1.0 - t.y) * w.outer) / left;
}

// `top`, the layers above a Blend layer, with a layer of colour `cs` covering `a_in` of it laid
// over: mixed by `mode` with `under` (the whole stack under the layer), as `layer_over` mixes it,
// then painted normally over the layers above the Blend layer under it.
fn top_over(top: vec4<f32>, under: vec4<f32>, cs: vec3<f32>, a_in: f32, mode: u32) -> vec4<f32> {
    let a = clamp(a_in, 0.0, 1.0);
    let ab = under.a;
    var mixed = cs;
    if mode != 0u && ab > 1e-5 {
        mixed = (1.0 - ab) * cs + ab * blended(mode, under.rgb / ab, cs);
    }
    return vec4<f32>(a * mixed + (1.0 - a) * top.rgb, a + top.a * (1.0 - a));
}

// `top` with colour `cs` mixed into what it covers by `mode` at `a`, covering no more than it did.
fn recoloured_top(top: vec4<f32>, cs: vec3<f32>, a: f32, mode: u32) -> vec4<f32> {
    if top.a <= 1e-5 || a <= 0.0 {
        return top;
    }
    let t = top.rgb / top.a;
    var mixed = cs;
    if mode != 0u {
        mixed = blended(mode, t, cs);
    }
    return vec4<f32>(mix(t, mixed, clamp(a, 0.0, 1.0)) * top.a, top.a);
}

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

// How far ball point `q` is from brush tile `tile`'s painted edge, in radii, negative inside the
// paint (see masks.rs: the distance channel reaches a quarter of a radius). No tile: far outside.
fn tile_d_at(tile: f32, q: vec2<f32>) -> f32 {
    let uv = q * 0.5 + vec2<f32>(0.5);
    if tile < 0.0 || any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return 0.25;
    }
    let g = textureSampleLevel(mask_tiles, mask_sampler, uv, i32(tile + 0.5), 0.0).g;
    return -(g - 0.5) * 0.5;
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

// How far cut point `m` (already turned by its angle) is from a Shape cut's edge, negative
// inside it: 0 half (the top), 1 wedge (round the top, `size` of a turn), 2 quarters (top left and
// bottom right), 3 ring, 4 circle, 5 band. A wedge of the whole turn is everywhere (-1).
fn shape_mask_d(shape: f32, m: vec2<f32>, size: f32) -> f32 {
    let r = length(m);
    switch u32(shape + 0.5) {
        case 0u: {
            return m.y;
        }
        case 1u: {
            let span = clamp(size, 0.0, 1.0) * LAYERS_TAU;
            if span >= LAYERS_TAU - 1e-4 {
                return -1.0;
            }
            let a = atan2(m.x, -m.y);
            return (abs(a) - 0.5 * span) * r;
        }
        case 2u: {
            return -min(abs(m.x), abs(m.y)) * sign(m.x * m.y);
        }
        case 3u: {
            return abs(r - 0.7) - 0.5 * size;
        }
        case 4u: {
            return r - size;
        }
        default: {
            return abs(m.y) - 0.5 * size;
        }
    }
}

// The edge distance of cut record `m` at ball point `q` (`screen_q` on the screen) for a Shape or
// Brush cut, negative where it lets its layer show; far inside (no edge) for the other kinds. A
// shadow re-reads the cuts above it at its shifted point with this.
fn cut_distance(m: LayerGpu, q: vec2<f32>, screen_q: vec2<f32>) -> f32 {
    let cut_from = u32(m.p0.x + 0.5);
    if cut_from != 2u && cut_from != 3u {
        return -1e6;
    }
    var pt = q;
    let moves = u32(m.p0.w + 0.5);
    if moves == 0u {
        pt = rotate(q, -m.amount.z);
    } else if moves == 2u {
        pt = screen_q;
    }
    var dist = 0.0;
    if cut_from == 2u {
        dist = shape_mask_d(m.amount.w, rotate(pt, -m.p1.x), m.p1.y);
    } else {
        dist = tile_d_at(m.p1.w, pt);
    }
    return select(dist, -dist, m.p0.y > 0.5);
}

// How much a Shape cut lets show at cut point `m`, its edge faded over `s`.
fn shape_mask(shape: f32, m: vec2<f32>, size: f32, s: f32) -> f32 {
    if u32(shape + 0.5) == 1u && clamp(size, 0.0, 1.0) * LAYERS_TAU >= LAYERS_TAU - 1e-4 {
        return 1.0;
    }
    return soft_inside(shape_mask_d(shape, m, size), s);
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

// A Cut record `m` at the point: how much it lets show inside and outside the shape, and its
// own distance (negative where it lets show; in cut units, ball radii or a shape's units).
// `below`, `chosen` are the cover (inside, outside) of the layer directly under its layer and of
// its chosen layer; `under` the stack under its layer.
fn cut_value(
    m: LayerGpu,
    q: vec2<f32>,
    screen_q: vec2<f32>,
    below: vec2<f32>,
    chosen: vec2<f32>,
    under_in: vec4<f32>,
    under_out: vec4<f32>,
    aa: f32,
) -> vec3<f32> {
    let soft = clamp(m.p0.z, 0.0, 1.0);
    let cut_from = u32(m.p0.x + 0.5);
    var v = vec2<f32>(0.0);
    var dist = 0.0;
    switch cut_from {
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
            if cut_from == 2u {
                let s = max(0.5 * soft, aa);
                let turned = rotate(pt, -m.p1.x);
                v = vec2<f32>(shape_mask(m.amount.w, turned, m.p1.y, s));
                dist = shape_mask_d(m.amount.w, turned, m.p1.y);
            } else {
                v = vec2<f32>(tile_at(m.p1.w, pt));
                dist = tile_d_at(m.p1.w, pt);
            }
        }
        default: {
            let t = m.p1.z;
            let s = max(0.5 * soft, 1e-3);
            let li = layers_luminance(select(vec3<f32>(0.0), under_in.rgb / under_in.a, under_in.a > 1e-5));
            let lo = layers_luminance(select(vec3<f32>(0.0), under_out.rgb / under_out.a, under_out.a > 1e-5));
            v = vec2<f32>(smoothstep(t - s, t + s, li), smoothstep(t - s, t + s, lo));
            if cut_from == 5u {
                v = vec2<f32>(1.0) - v;
            }
        }
    }
    if cut_from != 2u && cut_from != 3u {
        // No edge of its own: one from how much it lets show.
        dist = (0.5 - max(v.x, v.y)) * 0.1;
    }
    if m.p0.y > 0.5 {
        v = vec2<f32>(1.0) - v;
        dist = -dist;
    }
    return vec3<f32>(clamp(v, vec2<f32>(0.0), vec2<f32>(1.0)), dist);
}

// The distance from a ball layer record `l`'s own edge at a point whose distance to the ball's
// edge is `d` (radii, negative inside): the ball's shape for layers that fill it, the band of an
// outline, and for a glow where its light falls to half.
fn ball_layer_sd(l: LayerGpu, d: f32) -> f32 {
    switch l.head.x {
        case 2u: {
            let w = max(l.p0.x, 0.0) / layered_ball.radius.y;
            var lo = -w;
            var hi = 0.0;
            if l.p0.y > 1.5 {
                lo = 0.0;
                hi = w;
            } else if l.p0.y > 0.5 {
                lo = -0.5 * w;
                hi = 0.5 * w;
            }
            return max(lo - d, d - hi);
        }
        case 4u: {
            let size = max(l.p0.x, 0.05);
            let gd = max(d, 0.0) / size;
            let a = clamp(max(l.amount.y, 0.0) * exp(-gd * gd), 0.0, 1.0);
            return (0.5 - a) * size;
        }
        default: {
            return d;
        }
    }
}

// What `fs_layers` and `fs_layers_each` draw: premultiplied colour and coverage.
fn shade_layers(in: LayeredOut) -> vec4<f32> {
    let aa = 1.0 / max(in.radius.x, 0.5);
    layered_ball =
        LayeredBall(
            in.body,
            in.radius,
            in.form,
            in.misc,
            in.vary,
            in.dir,
            aa,
            in.range.x,
            in.range.y,
            in.group,
            in.range.w,
            in.place,
            in.hit,
        );
    let p = rotate(in.local, -in.form.z);
    let d = ball_shape(p, in.form.x, in.form.y);
    let cover = clamp(-d / aa + 0.5, 0.0, 1.0);
    let w = walk_layers(in.local, in.range.x, in.range.y, in.range.z);
    var out = split_lower_mixed(cover, w, in.range.z);
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

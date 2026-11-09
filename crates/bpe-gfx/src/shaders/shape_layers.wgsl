// Shapes drawn with a stack of layers: a shape instance (paths.wgsl) naming a range of layer
// records (the shape records table on `LayerGpu` in frame.rs) that its thing wrote once for the
// frame. The fragment works out where the pixel is against the outline once (`shape_at`), then a
// walk (shape_walk.wgsl) lays the records over each other bottom to top, as balls' are: worked
// out twice, as if the pixel were wholly inside the shape's area (`inner`) and wholly outside it
// (`outer`), and mixed by how much of the pixel the shape covers. Appended after layers.wgsl,
// whose `Walked`, `band` and `layer_over` it uses.

// The shape a fragment is drawing, for the walks and for `sample`: its instance's values (see
// `PathInstance`), set once at the start of each fragment.
struct LayeredShape {
    a: vec4<f32>,
    b: vec4<f32>,
    c: vec4<f32>,
    layers: vec4<u32>,
    // Seconds since a ball last hit it, that hit's strength, how many hits (for sub-layers).
    hit: vec4<f32>,
};

var<private> layered_shape: LayeredShape;

// Where a point is against the shape: `q` in shape units; `sd` the signed distance to the
// outline's centre line in world pixels (negative inside, even-odd); `edge` the distance to the
// edge of what the shape covers (its fill, or its band), negative inside it; `along` how far
// round the outline the nearest point is (a letter's contour after contour), 0 to 1 clockwise from point 0 (a circle's or an
// ellipse's top; an ellipse measures it by the angle it is drawn with).
struct ShapeAt {
    q: vec2<f32>,
    sd: f32,
    edge: f32,
    along: f32,
};

const SHAPE_TAU: f32 = 6.28318531;

fn shape_at(world: vec2<f32>) -> ShapeAt {
    let s = layered_shape;
    let q = rotate(world - s.a.xy, -s.a.z) / max(s.a.w, 1e-6);
    var sd = 1e30;
    var along = 0.0;
    if s.b.y < 0.5 {
        sd = (length(q) - 1.0) * s.a.w;
        along = fract(atan2(q.x, -q.y) / SHAPE_TAU + 1.0);
    } else if s.b.y > 2.5 {
        // A fast letter (Text part 2): its distance from its tile; it has no place along its edge.
        sd = glyph_signed(q, s.b) * s.a.w;
    } else if s.b.y > 1.5 {
        // An ellipse: round from its own top by the angle it is drawn with.
        let ab = max(s.b.zw, vec2<f32>(1e-6));
        let c = ellipse_nearest(q, ab);
        let level = q / ab;
        sd = select(1.0, -1.0, dot(level, level) < 1.0) * length(q - c) * s.a.w;
        along = fract(atan2(c.x / ab.x, -c.y / ab.y) / SHAPE_TAU + 1.0);
    } else {
        let first = u32(s.b.z);
        let n = u32(s.b.w);
        if n >= 2u {
            var best = 1e30;
            var best_at = 0.0;
            var total = 0.0;
            var inside = false;
            for (var k = 0u; k < n; k++) {
                let pk = path_points[first + k];
                let p0 = pk.xy;
                // A letter contour's last point (z 1) closes back to its first, `w` back; a
                // shape's points close round the whole list.
                var next = (k + 1u) % n;
                if pk.z > 0.5 && pk.z < 1.5 {
                    next = k - u32(pk.w + 0.5);
                }
                let p1 = path_points[first + next].xy;
                let e = p1 - p0;
                let len = length(e);
                let h = clamp(dot(q - p0, e) / max(dot(e, e), 1e-12), 0.0, 1.0);
                let d = length(q - p0 - e * h);
                if d < best {
                    best = d;
                    best_at = total + h * len;
                }
                total += len;
                if (p0.y > q.y) != (p1.y > q.y) {
                    let x = p0.x + (q.y - p0.y) / (p1.y - p0.y) * (p1.x - p0.x);
                    if q.x < x {
                        inside = !inside;
                    }
                }
            }
            sd = select(best, -best, inside) * s.a.w;
            along = best_at / max(total, 1e-6);
        }
    }
    var edge = sd;
    if s.b.x > 0.0 {
        edge = abs(sd) - s.b.x;
    }
    return ShapeAt(q, sd, edge, along);
}

// How much of the pixel at `s` the shape covers: `path_cover`'s maths.
fn shape_cover_at(s: ShapeAt) -> f32 {
    let px = g.view.z;
    let half = layered_shape.b.x;
    if half <= 0.0 {
        return clamp(0.5 - s.sd / px, 0.0, 1.0);
    }
    let half_width = max(half, 0.5 * px);
    return clamp((half_width - abs(s.sd)) / px + 0.5, 0.0, 1.0) * min(half * 2.0 / px, 1.0);
}

// 1 up to four fifths of the way out to where the shape's layers reach past its edge (`c.z`),
// falling to 0 there, so light that reaches far (a big glow) fades out evenly all round and never
// meets the quad's edge (every point that near the edge is inside the quad).
fn shape_reach_fade(s: ShapeAt) -> f32 {
    let past = max(layered_shape.c.z, 1e-3);
    return 1.0 - smoothstep(0.8 * past, past, s.edge);
}

// How far from the middle out `s` is for a Colour and gradient layer's Round: across the band of
// a hollow shape (inner edge to outer), out by its own shape in an ellipse, else to its furthest
// point.
fn shape_from_middle(s: ShapeAt) -> f32 {
    let half = layered_shape.b.x;
    if half > 0.0 {
        return (s.sd + half) / (2.0 * half);
    }
    if layered_shape.b.y > 1.5 && layered_shape.b.y < 2.5 {
        return length(s.q / max(layered_shape.b.zw, vec2<f32>(1e-6)));
    }
    return length(s.q) / max(layered_shape.c.w, 1e-6);
}

// The distance from shape layer record `l`'s own edge at `s`, in world pixels, negative inside:
// the shape's for layers that fill it, the band of an outline, and for a glow where its light
// falls to half.
fn shape_layer_sd(l: LayerGpu, s: ShapeAt) -> f32 {
    switch l.head.x {
        case 2u: {
            let w = max(l.p0.x, 0.0);
            var lo = -w;
            var hi = 0.0;
            if l.p0.y > 1.5 {
                lo = 0.0;
                hi = w;
            } else if l.p0.y > 0.5 {
                lo = -0.5 * w;
                hi = 0.5 * w;
            }
            return max(lo - s.edge, s.edge - hi);
        }
        case 4u: {
            let size = max(l.p0.x, 1.0);
            let gd = max(s.edge, 0.0) / size;
            let a = clamp(max(l.amount.y, 0.0) * exp(-gd * gd), 0.0, 1.0);
            return (0.5 - a) * size;
        }
        default: {
            return s.edge;
        }
    }
}

// Where `world` is in the shape's units, upright on the screen (its middle at 0, its size 1).
fn shape_upright(world: vec2<f32>) -> vec2<f32> {
    return (world - layered_shape.a.xy) / max(layered_shape.a.w, 1e-6);
}

// Record `l`'s own colour at `s`: its one colour, or its gradient laid out by its form in the
// shape's units. A layer of the shape's own (`whole`) runs Straight from edge to edge at its turn
// (`l.p1`: the direction and the outline's extent along it) and a Colour and gradient layer runs
// Round from the middle (a Text's layer over each letter or line: from that one's own middle);
// Round on a layer with a band (`banded`) reads `along`, worked out for it;
// Along the edge runs round the outline.
fn shape_colour(l: LayerGpu, s: ShapeAt, along: f32, banded: bool, whole: bool) -> vec3<f32> {
    if l.paint.row < 0.0 {
        return l.colour.rgb;
    }
    let code = u32(l.paint.pattern + 0.5);
    let k = l.paint.scale;
    if code == 1u && whole {
        let width = max(l.p1.w - l.p1.z, 1e-6);
        let t = (dot(s.q, l.p1.xy) - l.p1.z) / width - 0.5 - dot(l.paint.place.xy, l.p1.xy) / width;
        return paint_along(l.paint, t * k + 0.5, g.view.w).rgb;
    }
    if code == 2u && banded {
        return paint_along(l.paint, along * k, g.view.w).rgb;
    }
    if code == 2u && whole {
        // A Text's layer laid over each letter or line (Text part 4) runs from that one's own
        // middle (`paint.place.zw`) out to its reach (`p0.z`); shapes leave `p0.z` at 0.
        var out = shape_from_middle(s);
        if l.p0.z > 0.0 {
            out = length(s.q - l.paint.place.zw) / l.p0.z;
        }
        return paint_along(l.paint, out * k, g.view.w).rgb;
    }
    if code == 4u {
        return paint_pattern(l.paint, s.along, g.view.w).rgb;
    }
    return paint_colour(l.paint, s.q, g.view.w, vec4<f32>(l.colour.rgb, 1.0)).rgb;
}

// A shape with layers: premultiplied colour and coverage.
fn shade_shape_layers(
    world: vec2<f32>,
    a: vec4<f32>,
    b: vec4<f32>,
    c: vec4<f32>,
    layers: vec4<u32>,
    hit: vec4<f32>,
    keep: f32,
) -> vec4<f32> {
    layered_shape = LayeredShape(a, b, c, layers, hit);
    // For Ripples (paint.wgsl): measured from everything but the shape itself.
    paint_world = world;
    paint_own = vec3<f32>(a.xy, 1.0);
    let s = shape_at(world);
    let cover = shape_cover_at(s);
    let w = walk_shape(world, s, layers.x, layers.y, layers.w);
    var out = split_lower_mixed(cover, w, layers.w);
    out.r = surface_whole(out.r, w, cover);
    // What a broken shape's look leaves of it (its piece's clip, its fade).
    if keep < 1.0 {
        out *= keep;
    }
    // The tint pass: the stack's colour as it tints light, its depth worked out once.
    if tint_pass() {
        if out.a <= 1e-5 || TINT_SKIP {
            discard;
        }
        return tint_out(out.rgb / out.a, tint_presence(out.a, cover), TINT_BLOCKS, 1.0, TINT_THICK);
    }
    // The emission pass may give off darkness (negative light) where it covers nothing.
    let seen = select(max(out.r, max(out.g, out.b)), max(abs(out.r), max(abs(out.g), abs(out.b))), light_pass(g.light.x) >= PASS_EMISSION);
    if out.a < 1e-4 && seen < 1e-4 {
        discard;
    }
    return hard_tags(out);
}

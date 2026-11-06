// Gradient paint inside a shape: the frame's gradients baked one per row of an atlas texture
// (see gradients.rs), and a pattern that turns a point inside the shape into a place along one.
// Prepended to the scene and particle shaders; `paint_wgsl` in gradients.rs fills in the
// bind group each of them reads the atlas from.

// Matches `PaintGpu` in gradients.rs.
struct PaintGpu {
    // The atlas row; -1 paints nothing, -2 paints a shade of the instance's own colour (see
    // `paint_paints`).
    row: f32,
    // Where along the gradient the pattern starts; in shade mode the shade, -1 to 1.
    t0: f32,
    // 0 none, 1 straight, 2 round, 3 sweep, 4 along the edge (a shape works the place along its
    // outline out and calls `paint_pattern`; elsewhere a sweep from the top), 5 stripes, 6 rings,
    // 7 dots, 8 checker, 9 halves, 10 slices, 11 swirl, 12 face, then the gradient forms 13
    // diamond, 14 polygon / star, 15 spiral, 16 kaleido, 17 off-centre round, 18 waves,
    // 19 marble, 20 free points, 21 ripples, 22 turbulence (`GradientForm::code`).
    pattern: f32,
    // Still per-pixel grain, its size in 1/255 steps; 0 none.
    grain: f32,
    // Degrees, from +x towards +y.
    angle: f32,
    scale: f32,
    scroll: f32,
    // 0 clamp, 1 repeat, 2 mirror.
    repeat: f32,
    // The instance's own colour variation: hue turned by x turns, y added to the saturation,
    // value times z; w unused. PAINT_NO_VARY leaves the colour exactly as sampled.
    vary: vec4<f32>,
    // The form's own numbers (see `paint_form_along`): polygon sides and star depth; spiral arms
    // and twist; kaleido mirrors; off-centre focus xy, stretch and its turn; waves count, height,
    // flow; marble detail, bend, seed; free points softness, their atlas row, count and 1 to
    // blend in OKLab; ripples spacing, flow, from; turbulence detail, speed, seed.
    form: vec4<f32>,
    // The form's centre xy (in the instance's own space, before the turn), then spare.
    place: vec4<f32>,
};

const PAINT_NO_VARY: vec4<f32> = vec4<f32>(0.0, 0.0, 1.0, 0.0);

@group(PAINT_GROUP) @binding(0) var paint_atlas: texture_2d<f32>;
@group(PAINT_GROUP) @binding(1) var paint_sampler: sampler;
// The Ripples field (ripples.wgsl; 1×1 and empty when the frame has none) and its sources.
@group(PAINT_GROUP) @binding(2) var paint_ripple_a: texture_2d<u32>;
@group(PAINT_GROUP) @binding(3) var paint_ripple_b: texture_2d<u32>;
@group(PAINT_GROUP) @binding(4) var<storage, read> paint_ripple_sources: array<vec4<f32>>;

const PAINT_TAU: f32 = 6.28318531;
// Samples in one row, as ATLAS_WIDTH.
const PAINT_WIDTH: f32 = 1024.0;

// The pixel being painted, for the grain: each fragment entry point that paints sets it from its
// position first thing. Elsewhere it stays 0 and the grain is one still offset.
var<private> paint_px: vec2<f32>;
// The world point being painted and the thing painting it, for Ripples: its centre and what it
// is (z: 0 a ball, 1 a shape, as `RippleSource.b.z`); far away (3e38) when nothing is painting
// itself (the background). A ball in the middle of a ring still sees the ring.
var<private> paint_world: vec2<f32>;
var<private> paint_own: vec3<f32> = vec3<f32>(3.0e38, 3.0e38, -1.0);

// A paint with no variation read from two vec4s, in field order.
fn paint_from(a: vec4<f32>, b: vec4<f32>) -> PaintGpu {
    return PaintGpu(a.x, a.y, a.z, a.w, b.x, b.y, b.z, b.w, PAINT_NO_VARY, vec4<f32>(0.0), vec4<f32>(0.0));
}

// A paint read from three vec4 instance attributes, in field order, the last its variation.
fn paint_from_varied(a: vec4<f32>, b: vec4<f32>, c: vec4<f32>) -> PaintGpu {
    return PaintGpu(a.x, a.y, a.z, a.w, b.x, b.y, b.z, b.w, c, vec4<f32>(0.0), vec4<f32>(0.0));
}

// `x` brought into 0 to 1 the way `repeat` says.
fn paint_wrap(x: f32, repeat: f32) -> f32 {
    var t = x;
    if repeat > 1.5 {
        t = 1.0 - abs(fract(x * 0.5) * 2.0 - 1.0);
    } else if repeat > 0.5 {
        t = fract(x);
    }
    return clamp(t, 0.0, 1.0);
}

// Whether `p` paints anything: a gradient row, or a shade of the instance's own colour.
fn paint_paints(p: PaintGpu) -> bool {
    return p.row >= 0.0 || paint_is_shade(p);
}

// Whether `p` paints a shade of the instance's own colour (row -2) instead of a gradient.
fn paint_is_shade(p: PaintGpu) -> bool {
    return p.row < -1.5 && p.row > -2.5;
}

// `base` (linear light) shaded by `s`: toward black below 0 (all black at -1), toward white
// above (all white at 1). Alpha is kept.
fn paint_shaded(base: vec4<f32>, s_in: f32) -> vec4<f32> {
    let s = clamp(s_in, -1.0, 1.0);
    if s < 0.0 {
        return vec4<f32>(base.rgb * (1.0 + s), base.a);
    }
    return vec4<f32>(mix(base.rgb, vec3<f32>(1.0), s), base.a);
}

// `q` turned by `degrees`, from +x towards +y.
fn paint_turn(q: vec2<f32>, degrees: f32) -> vec2<f32> {
    let a = radians(degrees);
    let c = cos(a);
    let s = sin(a);
    return vec2<f32>(c * q.x - s * q.y, s * q.x + c * q.y);
}

// The linear-light colour `p` paints at `local`, the point in the instance's own space (-1 to 1
// across it, y down), `time` seconds in, on an instance whose own colour is `base`. The gradient
// forms (straight to turbulence, see `paint_form_along`) turn `local` about `p.place.xy` by
// `p.angle` and scale it by `p.scale`; stripes, rings, along the edge and ripples never run off
// their gradient's end: the paint's Clamp mirrors them (`paint_mirrored`). Free points blend
// their stops' colours by distance. The masks (dots to face) are drawn by `paint_mask` (0 or 1,
// softened at the edges), turned by `p.angle`, always clamped whatever `p.repeat` says.
// A gradient starts at `p.t0`. In shade mode (row -2) the place blends `base`, at 0, to `base`
// shaded by `p.t0`, at 1, with no gradient and no variation. Uses `fwidth`: fragment code only.
fn paint_colour(p: PaintGpu, local: vec2<f32>, time: f32, base: vec4<f32>) -> vec4<f32> {
    let code = p.pattern;
    var q = p;
    var along = 0.0;
    if code > 6.5 && code < 12.5 {
        along = paint_mask(paint_turn(local, -p.angle), code - 1.0, p.scale * 2.0);
        // A mask is already 0 to 1: wrapping would turn its fully-on 1 back to the start.
        q.repeat = 0.0;
    } else if code > 19.5 && code < 20.5 && !paint_is_shade(p) {
        return paint_grain(paint_vary(paint_free(p, local), p.vary), p.grain);
    } else if code > 0.5 {
        along = paint_form_along(p, local, time);
        if paint_form_mirrors(code) {
            q.repeat = paint_mirrored(p.repeat);
        }
    }
    if paint_is_shade(p) {
        let t = paint_wrap(along + time * p.scroll, q.repeat);
        return vec4<f32>(mix(base.rgb, paint_shaded(base, p.t0).rgb, t), base.a);
    }
    return paint_along(q, along, time);
}

// Whether form `code` runs back and forth on Clamp (it never reaches an end of its own).
fn paint_form_mirrors(code: f32) -> bool {
    let c = u32(code + 0.5);
    return c == 4u || c == 5u || c == 6u || c == 21u;
}

// `local` in the form's own space: about its centre, turned back by its turn, scaled.
fn paint_q(p: PaintGpu, local: vec2<f32>) -> vec2<f32> {
    return paint_turn(local - p.place.xy, -p.angle) * p.scale;
}

// Where along its gradient `p`'s form puts `local`, `time` seconds in, before the start, scroll
// and repeat (as `bpe_core::gradient_form::along`).
fn paint_form_along(p: PaintGpu, local: vec2<f32>, time: f32) -> f32 {
    let code = u32(p.pattern + 0.5);
    let q = paint_q(p, local);
    let turns = atan2(q.y, q.x) / PAINT_TAU;
    let f = p.form;
    switch code {
        case 1u: {
            return q.x * 0.5 + 0.5;
        }
        case 2u, 6u: {
            return length(q);
        }
        case 3u: {
            return fract(turns) * p.scale;
        }
        case 4u: {
            return fract(turns + 0.25);
        }
        case 5u: {
            return q.x * 0.5;
        }
        case 13u: {
            return abs(q.x) + abs(q.y);
        }
        case 14u: {
            return paint_polygon(q, f.x, f.y);
        }
        case 15u: {
            return fract(turns * max(round(f.x), 1.0) + length(q) * f.y);
        }
        case 16u: {
            return 1.0 - abs(2.0 * fract(turns * max(round(f.x), 1.0)) - 1.0);
        }
        case 17u: {
            return paint_off_centre(q, f);
        }
        case 18u: {
            let wave = sin(PAINT_TAU * (f.x * (q.y * 0.5 + 0.5) + f.z * time));
            return q.x * 0.5 + 0.5 + f.y * 0.5 * wave;
        }
        case 21u: {
            return paint_ripples(p, time);
        }
        // Marble (one pass of noise bending Straight) and Turbulence (noise warped by two more
        // passes, drifting at its speed): one call of the noise, so it compiles once.
        case 19u, 22u: {
            let turbulent = code == 22u;
            let seed = paint_seed(f.z);
            let t = time * f.y;
            let a = q * 1.5;
            var w = vec2<f32>(0.0);
            var n = 0.0;
            let passes = select(1u, 3u, turbulent);
            for (var k = 0u; k < passes; k += 1u) {
                var at = a;
                if turbulent {
                    if k == 0u {
                        at = a + vec2<f32>(0.0, t);
                    } else if k == 1u {
                        at = a + vec2<f32>(5.2, 1.3 - t);
                    } else {
                        at = a + 1.5 * w;
                    }
                }
                n = paint_fbm(at, f.x, seed + k);
                if k == 0u {
                    w.x = n;
                } else if k == 1u {
                    w.y = n;
                }
            }
            if turbulent {
                return n * 1.6 + 0.5;
            }
            return q.x * 0.5 + 0.5 + f.y * n;
        }
        default: {
            return 0.0;
        }
    }
}

// How far along a polygon or star of `sides` corners (pointing up) `q` is: 1 on its outline.
fn paint_polygon(q: vec2<f32>, sides: f32, star: f32) -> f32 {
    let n = clamp(round(sides), 3.0, 4096.0);
    let step = PAINT_TAU * 0.5 / n;
    let inner = max(cos(step) * (1.0 - star), 0.02);
    let theta = atan2(q.y, q.x) + PAINT_TAU * 0.25;
    let k = floor(theta / step);
    let v0 = paint_corner(k, step, inner);
    let v1 = paint_corner(k + 1.0, step, inner);
    let e = v1 - v0;
    var normal = vec2<f32>(e.y, -e.x);
    let len = length(normal);
    if len < 1e-12 {
        return length(q);
    }
    normal = normal / len;
    if dot(normal, v0) < 0.0 {
        normal = -normal;
    }
    return dot(q, normal) / max(dot(normal, v0), 1e-6);
}

// Corner `k` of a star whose even corners sit at radius 1 and odd ones at `inner`.
fn paint_corner(k: f32, step: f32, inner: f32) -> vec2<f32> {
    let a = -PAINT_TAU * 0.25 + k * step;
    let odd = k - 2.0 * floor(k * 0.5);
    let r = select(1.0, inner, odd > 0.5);
    return vec2<f32>(cos(a), sin(a)) * r;
}

// The focal radial: 0 at the focus `f.xy`, 1 on the unit circle, after squashing `q` by the
// stretch `f.z` along `f.w` degrees.
fn paint_off_centre(q: vec2<f32>, f: vec4<f32>) -> f32 {
    var s = f.z;
    if abs(s) < 1e-3 {
        s = select(1e-3, -1e-3, s < 0.0);
    }
    var p = paint_turn(q, -f.w);
    p.x = p.x / s;
    p = paint_turn(p, f.w);
    var focus = f.xy;
    if length(focus) > 0.99 {
        focus = normalize(focus) * 0.99;
    }
    let d = p - focus;
    let len = length(d);
    if len < 1e-6 {
        return 0.0;
    }
    let dir = d / len;
    let b = dot(focus, dir);
    let c = dot(focus, focus) - 1.0;
    let reach = -b + sqrt(max(b * b - c, 0.0));
    return len / max(reach, 1e-6);
}

// The seed setting as the noise hash takes it.
fn paint_seed(seed: f32) -> u32 {
    return bitcast<u32>(i32(clamp(round(seed), -1e9, 1e9)));
}

// A value in -0.5 to 0.5 at lattice point (x, y) for `seed` (as bpe_core's `lattice`).
fn paint_lattice(x: i32, y: i32, seed: u32) -> f32 {
    let h = vary_pcg(seed ^ vary_pcg(bitcast<u32>(x) ^ vary_pcg(bitcast<u32>(y) ^ 0x9e3779b9u)));
    return f32(h >> 8u) / 16777215.0 - 0.5;
}

// Smooth value noise at `q`, -0.5 to 0.5.
fn paint_noise(q_in: vec2<f32>, seed: u32) -> f32 {
    let q = clamp(q_in, vec2<f32>(-1e6), vec2<f32>(1e6));
    let i = floor(q);
    let f = q - i;
    let s = f * f * (vec2<f32>(3.0) - 2.0 * f);
    let x = i32(i.x);
    let y = i32(i.y);
    let a = paint_lattice(x, y, seed);
    let b = paint_lattice(x + 1, y, seed);
    let c = paint_lattice(x, y + 1, seed);
    let d = paint_lattice(x + 1, y + 1, seed);
    let top = a + (b - a) * s.x;
    let bottom = c + (d - c) * s.x;
    return top + (bottom - top) * s.y;
}

// Octaves of noise, each twice as fine and half as strong, kept to -0.5 to 0.5.
fn paint_fbm(q: vec2<f32>, octaves: f32, seed: u32) -> f32 {
    let n = u32(clamp(round(octaves), 1.0, 8.0));
    var sum = 0.0;
    var amp = 1.0;
    var total = 0.0;
    var at = q;
    for (var i = 0u; i < n; i += 1u) {
        sum += amp * paint_noise(at, seed + i * 0x68e31da4u);
        total += amp;
        amp *= 0.5;
        at *= 2.0;
    }
    return sum / total;
}

// Linear light to OKLab and back (as bpe_core's oklab.rs).
fn paint_oklab(c: vec3<f32>) -> vec3<f32> {
    let l = 0.4122214708 * c.r + 0.5363325363 * c.g + 0.0514459929 * c.b;
    let m = 0.2119034982 * c.r + 0.6806995451 * c.g + 0.1073969566 * c.b;
    let s = 0.0883024619 * c.r + 0.2817188376 * c.g + 0.6299787005 * c.b;
    let lms = vec3<f32>(paint_cbrt(l), paint_cbrt(m), paint_cbrt(s));
    return vec3<f32>(
        0.2104542553 * lms.x + 0.7936177850 * lms.y - 0.0040720468 * lms.z,
        1.9779984951 * lms.x - 2.4285922050 * lms.y + 0.4505937099 * lms.z,
        0.0259040371 * lms.x + 0.7827717662 * lms.y - 0.8086757660 * lms.z,
    );
}

fn paint_cbrt(x: f32) -> f32 {
    return sign(x) * pow(abs(x), 1.0 / 3.0);
}

fn paint_from_oklab(lab: vec3<f32>) -> vec3<f32> {
    let l = lab.x + 0.3963377774 * lab.y + 0.2158037573 * lab.z;
    let m = lab.x - 0.1055613458 * lab.y - 0.0638541728 * lab.z;
    let s = lab.x - 0.0894841775 * lab.y - 1.2914855480 * lab.z;
    let l3 = l * l * l;
    let m3 = m * m * m;
    let s3 = s * s * s;
    return vec3<f32>(
        4.0767416621 * l3 - 3.3077115913 * m3 + 0.2309699292 * s3,
        -1.2684380046 * l3 + 2.6097574011 * m3 - 0.3413193965 * s3,
        -0.0041960863 * l3 - 0.7034186147 * m3 + 1.7076147010 * s3,
    );
}

// The colour Free points give at `local`: each stop's colour (from the atlas row `p.form.y`:
// texel 2i its point, 2i + 1 its colour) weighted by 1 / (distance^softness + 1e-6), mixed in
// OKLab when `p.form.w` is 1 (as `bpe_core::gradient_form::free_colour`).
fn paint_free(p: PaintGpu, local: vec2<f32>) -> vec4<f32> {
    let q = paint_q(p, local);
    let height = i32(textureDimensions(paint_atlas).y);
    let row = clamp(i32(p.form.y), 0, height - 1);
    let n = i32(clamp(p.form.z, 0.0, 16.0));
    let power = clamp(p.form.x, 0.1, 64.0);
    let oklab = p.form.w > 0.5;
    var sum = vec4<f32>(0.0);
    var total = 0.0;
    var nearest = vec4<f32>(0.0);
    var best = 3.4e38;
    for (var i = 0; i < n; i += 1) {
        let at = textureLoad(paint_atlas, vec2<i32>(2 * i, row), 0).xy;
        let c = textureLoad(paint_atlas, vec2<i32>(2 * i + 1, row), 0);
        let d = length(q - at);
        if d < best {
            best = d;
            nearest = c;
        }
        let w = 1.0 / (pow(d, power) + 1e-6);
        let v = select(c.rgb, paint_oklab(max(c.rgb, vec3<f32>(0.0))), oklab);
        sum += w * vec4<f32>(v, c.a);
        total += w;
    }
    if !(total > 0.0) || total > 3.0e38 {
        return nearest;
    }
    let mean = sum / total;
    let rgb = select(mean.rgb, max(paint_from_oklab(mean.rgb), vec3<f32>(0.0)), oklab);
    return vec4<f32>(rgb, mean.a);
}

// Ripples from things: how far the pixel being painted (`paint_world`) is from the nearest ball
// or shape (form numbers: spacing, flow, from 0 everything / 1 balls / 2 shapes) other than the
// thing painting it, over the spacing, moved on by the flow. With nothing to measure from (no
// field, or nothing of that kind in the picture) it sits at the start.
fn paint_ripples(p: PaintGpu, time: f32) -> f32 {
    let d = paint_ripple_distance(u32(clamp(p.form.z, 0.0, 2.0) + 0.5));
    if d < 0.0 {
        return 0.0;
    }
    return d / max(p.form.x, 0.5) - p.form.y * time;
}

// Source `i` (1-based) of the frame's Ripples.
fn paint_ripple_source(i: u32) -> RippleSource {
    let k = 2u + (i - 1u) * 3u;
    return RippleSource(paint_ripple_sources[k], paint_ripple_sources[k + 1u], paint_ripple_sources[k + 2u]);
}

// How far `paint_world` is from the nearest source in list `from` (0 everything, 1 balls, 2
// shapes) not owned by the thing `paint_own` describes, outside it (0 inside); -1 for none.
fn paint_ripple_distance(list: u32) -> f32 {
    let camera = paint_ripple_sources[0];
    let size = paint_ripple_sources[1];
    if size.z < 0.5 {
        return -1.0;
    }
    let t = ripple_texel_of(paint_world, camera, size);
    let a = textureLoad(paint_ripple_a, t, 0);
    let b = textureLoad(paint_ripple_b, t, 0);
    var pair = a.xy;
    if list == 1u {
        pair = a.zw;
    } else if list == 2u {
        pair = b.xy;
    }
    let found = array<u32, 2>(pair.x, pair.y);
    for (var k = 0; k < 2; k++) {
        let i = found[k];
        if i == 0u {
            continue;
        }
        let s = paint_ripple_source(i);
        if all(abs(s.c.xy - paint_own.xy) < vec2<f32>(0.01)) && abs(s.b.z - paint_own.z) < 0.5 {
            continue;
        }
        return max(ripple_distance(s, paint_world), 0.0);
    }
    return -1.0;
}

// The repeat a pattern that never runs off its gradient's end uses: Clamp mirrors.
fn paint_mirrored(repeat: f32) -> f32 {
    return select(repeat, 2.0, repeat < 0.5);
}

// The linear-light colour of an inside pattern or a path `along` from its start, `time` seconds
// in. A pattern never runs off the end of its gradient: the paint's Clamp mirrors it instead.
fn paint_pattern(p: PaintGpu, along: f32, time: f32) -> vec4<f32> {
    var q = p;
    q.repeat = paint_mirrored(p.repeat);
    return paint_along(q, along, time);
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
fn paint_inside_mask(d: f32) -> f32 {
    return clamp(-d / max(fwidth(d), 1e-4) + 0.5, 0.0, 1.0);
}

// How much of the second colour shows at `p` (unit radius, y down) for a mask pattern, 0 to 1.
// Masks: 6 dots, 7 checker, 8 halves, 9 slices, 10 swirl, 11 face (`paint_colour` passes its
// pattern code less one); `size` repeats are two across the shape. Uses `fwidth`: fragment
// code only.
fn paint_mask(p: vec2<f32>, fill: f32, size: f32) -> f32 {
    let r = length(p);
    let n = size * 0.5;
    if fill < 6.5 {
        let cell = fract(p * n + 0.5) - 0.5;
        return paint_inside_mask(length(cell) - 0.28);
    } else if fill < 7.5 {
        let a = bands(p.x * n + 0.25);
        let b = bands(p.y * n + 0.25);
        return a + b - 2.0 * a * b;
    } else if fill < 8.5 {
        return paint_inside_mask(-p.y);
    }
    // Patterns around the middle: a whole number of slice pairs, so the seam where the angle
    // wraps doesn't show.
    let turns = atan2(p.y, p.x) / PAINT_TAU;
    let turn_width = length(fwidth(p)) / max(r, 1e-3) / PAINT_TAU;
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
    return paint_inside_mask(min(eyes, mouth));
}

// The linear-light colour `p` paints `along` its pattern (0 to 1 is one gradient), `time` seconds
// in: after the start, the scroll and the repeat, turned by the paint's variation.
fn paint_along(p: PaintGpu, along: f32, time: f32) -> vec4<f32> {
    return paint_grain(paint_vary(paint_sample(p, along, time), p.vary), p.grain);
}

// `c` (linear light) with still grain of size `grain` (in 1/255 steps) for this pixel: one value
// for all three channels, added in sRGB numbers (where 8-bit stair-steps show) and turned back.
fn paint_grain(c: vec4<f32>, grain: f32) -> vec4<f32> {
    if grain == 0.0 {
        return c;
    }
    let px = vec2<u32>(vec2<i32>(floor(paint_px)));
    let h = vary_pcg(px.x ^ vary_pcg(px.y ^ 0x2545f491u));
    let n = (f32(h >> 8u) / 16777215.0 - 0.5) * abs(grain) / 255.0;
    let s = paint_to_srgb(max(c.rgb, vec3<f32>(0.0))) + vec3<f32>(n);
    return vec4<f32>(paint_from_srgb(max(s, vec3<f32>(0.0))), c.a);
}

// Linear light to sRGB numbers and back, per channel (as Color::to_linear in bpe-core).
fn paint_to_srgb(c: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3<f32>(0.0031308));
}

fn paint_from_srgb(s: vec3<f32>) -> vec3<f32> {
    return select(pow((s + 0.055) / 1.055, vec3<f32>(2.4)), s / 12.92, s <= vec3<f32>(0.04045));
}

// What the atlas holds `along` `p`, before any variation.
fn paint_sample(p: PaintGpu, along: f32, time: f32) -> vec4<f32> {
    let t = paint_wrap(along + p.t0 + time * p.scroll, p.repeat);
    let height = textureDimensions(paint_atlas).y;
    let row = min(u32(max(p.row, 0.0)), height - 1u);
    // Sampled between the first and last texel centres, so 0 and 1 land on the end colours.
    let u = (t * (PAINT_WIDTH - 1.0) + 0.5) / PAINT_WIDTH;
    let v = (f32(row) + 0.5) / f32(height);
    return textureSampleLevel(paint_atlas, paint_sampler, vec2<f32>(u, v), 0.0);
}

// The colour variation, as `apply_vary` and `vary_offsets` in bpe-kit's paint/vary.rs. The
// hashing is the same 32-bit PCG, so the shader and the processor agree.

// One step of the PCG hash.
fn vary_pcg(x: u32) -> u32 {
    let state = x * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

// A value in -1 to 1 for (seed, which of the three values, lattice point).
fn vary_lattice(seed: u32, k: u32, n: u32) -> f32 {
    let h = vary_pcg(seed ^ vary_pcg((k * 0x9e3779b9u) ^ vary_pcg(n)));
    return f32(h >> 8u) / 16777215.0 * 2.0 - 1.0;
}

// Value `k` (0, 1 or 2) of an instance, -1 to 1: fixed when `wobble` is 0, else smooth noise over
// `time * wobble`, offset per instance so they don't all turn together.
fn vary_value(seed: u32, k: u32, time: f32, wobble: f32) -> f32 {
    if wobble <= 0.0 {
        return vary_lattice(seed, k, 0u);
    }
    let phase = (vary_lattice(seed, k + 3u, 0u) + 1.0) * 0.5;
    let x = time * wobble + phase;
    let n = floor(x);
    let f = x - n;
    let i = u32(i32(n));
    let s = f * f * (3.0 - 2.0 * f);
    let a = vary_lattice(seed, k, i);
    let b = vary_lattice(seed, k, i + 1u);
    return a + (b - a) * s;
}

// `c` (linear light) turned by `v`: hue by v.x turns, v.y added to the saturation, value times
// v.z. Alpha is untouched. No variation returns `c` exactly.
fn paint_vary(c: vec4<f32>, v: vec4<f32>) -> vec4<f32> {
    if v.x == 0.0 && v.y == 0.0 && v.z == 1.0 {
        return c;
    }
    let rgb = max(c.rgb, vec3<f32>(0.0));
    let hi = max(max(rgb.r, rgb.g), rgb.b);
    let delta = hi - min(min(rgb.r, rgb.g), rgb.b);
    var hue = 0.0;
    if delta > 0.0 {
        if hi == rgb.r {
            let x = (rgb.g - rgb.b) / delta;
            hue = (x - 6.0 * floor(x / 6.0)) / 6.0;
        } else if hi == rgb.g {
            hue = ((rgb.b - rgb.r) / delta + 2.0) / 6.0;
        } else {
            hue = ((rgb.r - rgb.g) / delta + 4.0) / 6.0;
        }
    }
    let sat = clamp(select(delta / hi, 0.0, hi <= 0.0) + v.y, 0.0, 1.0);
    let val = hi * max(v.z, 0.0);
    let turned = hue + v.x;
    var h = (turned - floor(turned)) * 6.0;
    // A hue a hair below 0 wraps to a hair below 1, which can round to exactly 1: red, not the
    // sector after magenta.
    if h >= 6.0 {
        h = 0.0;
    }
    let f = h - floor(h);
    let p = val * (1.0 - sat);
    let q = val * (1.0 - sat * f);
    let t = val * (1.0 - sat * (1.0 - f));
    var out: vec3<f32>;
    switch u32(h) {
        case 0u: { out = vec3<f32>(val, t, p); }
        case 1u: { out = vec3<f32>(q, val, p); }
        case 2u: { out = vec3<f32>(p, val, t); }
        case 3u: { out = vec3<f32>(p, q, val); }
        case 4u: { out = vec3<f32>(t, p, val); }
        default: { out = vec3<f32>(val, p, q); }
    }
    return vec4<f32>(max(out, vec3<f32>(0.0)), c.a);
}

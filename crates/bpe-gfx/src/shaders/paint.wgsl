// Gradient paint inside a shape: the frame's gradients baked one per row of an atlas texture
// (see gradients.rs), and a pattern that turns a point inside the shape into a place along one.
// Prepended to the scene, glyph and particle shaders; `paint_wgsl` in gradients.rs fills in the
// bind group each of them reads the atlas from.

// Matches `PaintGpu` in gradients.rs.
struct PaintGpu {
    // The atlas row; below 0 paints nothing.
    row: f32,
    t0: f32,
    // 0 none, 1 stripes, 2 rings, 3 sweep, 4 path (along a wall's outline; the shape works the
    // place along it out and calls `paint_pattern`).
    pattern: f32,
    // 1 picks the nearest baked colour.
    steps: f32,
    // Degrees, from +x towards +y.
    angle: f32,
    scale: f32,
    scroll: f32,
    // 0 clamp, 1 repeat, 2 mirror.
    repeat: f32,
    // The instance's own colour variation: hue turned by x turns, y added to the saturation,
    // value times z; w unused. PAINT_NO_VARY leaves the colour exactly as sampled.
    vary: vec4<f32>,
};

const PAINT_NO_VARY: vec4<f32> = vec4<f32>(0.0, 0.0, 1.0, 0.0);

@group(PAINT_GROUP) @binding(0) var paint_atlas: texture_2d<f32>;
@group(PAINT_GROUP) @binding(1) var paint_sampler: sampler;

const PAINT_TAU: f32 = 6.28318531;
// Samples in one row, as ATLAS_WIDTH.
const PAINT_WIDTH: f32 = 256.0;

// A paint with no variation read from two vec4s, in field order.
fn paint_from(a: vec4<f32>, b: vec4<f32>) -> PaintGpu {
    return PaintGpu(a.x, a.y, a.z, a.w, b.x, b.y, b.z, b.w, PAINT_NO_VARY);
}

// A paint read from three vec4 instance attributes, in field order, the last its variation.
fn paint_from_varied(a: vec4<f32>, b: vec4<f32>, c: vec4<f32>) -> PaintGpu {
    return PaintGpu(a.x, a.y, a.z, a.w, b.x, b.y, b.z, b.w, c);
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

// The linear-light colour `p` paints at `local`, the point in the instance's own space (-1 to 1
// across it), `time` seconds in. A pattern runs `p.scale` gradients across the shape (stripes
// across it at `p.angle`, rings out from its centre, sweep round it from `p.angle`), with `p.t0`
// in its middle: on the stripes' centre line, at the rings' centre, half way round the sweep.
fn paint_colour(p: PaintGpu, local: vec2<f32>, time: f32) -> vec4<f32> {
    var along = 0.0;
    if p.pattern > 2.5 {
        // `fract` so the seam can sit at any angle.
        along = (fract(atan2(local.y, local.x) / PAINT_TAU + 0.5 - p.angle / 360.0) - 0.5) * p.scale;
    } else if p.pattern > 1.5 {
        along = length(local) * p.scale;
    } else if p.pattern > 0.5 {
        let a = radians(p.angle);
        along = dot(local, vec2<f32>(cos(a), sin(a))) * 0.5 * p.scale;
    }
    if p.pattern < 0.5 {
        return paint_along(p, 0.0, time);
    }
    return paint_pattern(p, along, time);
}

// The linear-light colour of an inside pattern or a path `along` from its start, `time` seconds
// in. A pattern never runs off the end of its gradient: the paint's Clamp mirrors it instead.
fn paint_pattern(p: PaintGpu, along: f32, time: f32) -> vec4<f32> {
    var q = p;
    q.repeat = select(p.repeat, 2.0, p.repeat < 0.5);
    return paint_along(q, along, time);
}

// The linear-light colour `p` paints `along` its pattern (0 to 1 is one gradient), `time` seconds
// in: after the start, the scroll and the repeat, turned by the paint's variation.
fn paint_along(p: PaintGpu, along: f32, time: f32) -> vec4<f32> {
    return paint_vary(paint_sample(p, along, time), p.vary);
}

// What the atlas holds `along` `p`, before any variation.
fn paint_sample(p: PaintGpu, along: f32, time: f32) -> vec4<f32> {
    let t = paint_wrap(along + p.t0 + time * p.scroll, p.repeat);
    let height = textureDimensions(paint_atlas).y;
    let row = min(u32(max(p.row, 0.0)), height - 1u);
    if p.steps > 0.5 && p.steps < 1.5 {
        return textureLoad(paint_atlas, vec2<u32>(u32(floor(t * (PAINT_WIDTH - 1.0) + 0.5)), row), 0);
    }
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

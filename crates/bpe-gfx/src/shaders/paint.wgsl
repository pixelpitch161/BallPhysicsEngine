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
};

@group(PAINT_GROUP) @binding(0) var paint_atlas: texture_2d<f32>;
@group(PAINT_GROUP) @binding(1) var paint_sampler: sampler;

const PAINT_TAU: f32 = 6.28318531;
// Samples in one row, as ATLAS_WIDTH.
const PAINT_WIDTH: f32 = 256.0;

// A paint read from two vec4 instance attributes, in field order.
fn paint_from(a: vec4<f32>, b: vec4<f32>) -> PaintGpu {
    return PaintGpu(a.x, a.y, a.z, a.w, b.x, b.y, b.z, b.w);
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
// in: after the start, the scroll and the repeat.
fn paint_along(p: PaintGpu, along: f32, time: f32) -> vec4<f32> {
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

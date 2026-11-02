// The canvas composite: draws one canvas's rectangle of the world with the Flat style. The paint
// texture holds the marks' colour times their amount, and the amount; the aux texture's r holds
// the time each texel was painted, read unfiltered. The output is premultiplied colour and
// coverage. A skin is drawn only where its thing is: its mask's shapes, in the skin's own
// pixels, say how much of each pixel shows.

struct Globals {
    // x: 2 / video width, y: 2 / video height, z: world units per output pixel, w: time
    view: vec4<f32>,
    bg_paint: vec4<f32>,
    bg_place: vec4<f32>,
    post: vec4<f32>,
    // centre xy, zoom, turn
    camera: vec4<f32>,
};

struct Canvas {
    // Centre xy in world pixels, turn in radians.
    pose: vec4<f32>,
    // Width and height in world pixels, fade in seconds (0: marks stay), opacity.
    size: vec4<f32>,
    // The colour the canvas starts as, straight; alpha 0 when it starts clear.
    start: vec4<f32>,
    // x: the scene time in seconds; y: how many mask shapes there are; z: 1 when the canvas is
    // a skin, shown only through its mask.
    clock: vec4<f32>,
    // x: the style (0 flat, 1 wet paint, 2 watercolour, 3 neon ink), y: dry time in seconds,
    // z: light angle in radians clockwise from up, w: gloss.
    look: vec4<f32>,
    // Grain, edge darkening, glow size in world pixels, glow strength.
    look2: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var<uniform> canvas: Canvas;
@group(1) @binding(1) var paint: texture_2d<f32>;
@group(1) @binding(2) var aux: texture_2d<f32>;
@group(1) @binding(3) var smooth_sampler: sampler;
// The mask's shapes, nine words each, as `MaskShape`: the kind (0 disc, 1 capsule, 2 ring or
// polygon line, 3 box line or filled box), then a and b.
@group(1) @binding(4) var<storage, read> mask: array<f32>;
// How thick the paint is (ink / 255, added up).
@group(1) @binding(5) var body: texture_2d<f32>;

const PI: f32 = 3.14159265;
const TAU: f32 = 6.28318531;

// The camera (centre xy, zoom, turn) turns a world point into clip space, as in scene.wgsl.
fn to_clip(p: vec2<f32>) -> vec4<f32> {
    let c = g.camera;
    var d = p - c.xy;
    if c.w != 0.0 {
        let s = sin(-c.w);
        let k = cos(-c.w);
        d = vec2<f32>(d.x * k - d.y * s, d.x * s + d.y * k);
    }
    let q = d * c.z;
    return vec4<f32>(q.x * g.view.x, -q.y * g.view.y, 0.0, 1.0);
}

struct CanvasOut {
    @builtin(position) pos: vec4<f32>,
    // 0 to 1 across the canvas, (0, 0) at its top left texel.
    @location(0) uv: vec2<f32>,
    // Canvas pixels from its middle, before it turns.
    @location(1) local: vec2<f32>,
};

@vertex
fn vs_canvas(@builtin(vertex_index) i: u32) -> CanvasOut {
    let corner = vec2<f32>(f32(i & 1u), f32((i >> 1u) & 1u));
    let local = (corner - 0.5) * canvas.size.xy;
    let c = cos(canvas.pose.z);
    let s = sin(canvas.pose.z);
    let world = canvas.pose.xy + vec2<f32>(c * local.x - s * local.y, s * local.x + c * local.y);
    var out: CanvasOut;
    out.pos = to_clip(world);
    out.uv = corner;
    out.local = local;
    return out;
}

fn turn(p: vec2<f32>, angle: f32) -> vec2<f32> {
    let c = cos(angle);
    let s = sin(angle);
    return vec2<f32>(c * p.x - s * p.y, s * p.x + c * p.y);
}

fn segment_distance(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let ab = b - a;
    let t = clamp(dot(p - a, ab) / max(dot(ab, ab), 1e-8), 0.0, 1.0);
    return length(p - a - ab * t);
}

// Point on a ring or regular polygon `u` radians round from `angle`, as scene.wgsl draws it.
fn outline_point(radius: f32, sides: f32, angle: f32, u: f32) -> vec2<f32> {
    let d = vec2<f32>(cos(angle + u), sin(angle + u));
    if sides < 2.5 {
        return d * radius;
    }
    let step = TAU / sides;
    let side = round(u / step);
    return d * (radius * cos(PI / sides) / cos(u - side * step));
}

// Distance from p (from the centre) to a ring's or polygon's line, as scene.wgsl draws it.
fn ring_distance(p: vec2<f32>, radius: f32, sides: f32, angle: f32) -> f32 {
    if sides < 2.5 {
        return abs(length(p) - radius);
    }
    let u = atan2(p.y, p.x) - angle;
    let u_wrapped = u - TAU * floor((u + PI) / TAU);
    let step = TAU / sides;
    let side = round(u_wrapped / step);
    let a = outline_point(radius, sides, angle, (side - 0.5) * step);
    let b = outline_point(radius, sides, angle, (side + 0.5) * step);
    return segment_distance(p, a, b);
}

// How far past mask shape `i`'s edge p is: below 0 inside it.
fn mask_distance(i: u32, p: vec2<f32>) -> f32 {
    let at = i * 9u;
    let kind = bitcast<u32>(mask[at]);
    let a = vec4<f32>(mask[at + 1u], mask[at + 2u], mask[at + 3u], mask[at + 4u]);
    let b = vec4<f32>(mask[at + 5u], mask[at + 6u], mask[at + 7u], mask[at + 8u]);
    if kind == 0u {
        return length(p - a.xy) - a.z;
    }
    if kind == 1u {
        return segment_distance(p, a.xy, a.zw) - b.x;
    }
    if kind == 2u {
        return ring_distance(p - a.xy, a.w, b.x, a.z) - b.y;
    }
    // A box, its corners rounded by a.w: its line, or all of it when filled.
    let q = abs(turn(p - a.xy, -a.z)) - (b.xy - vec2<f32>(a.w));
    let signed = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - a.w;
    if b.w > 0.5 {
        return signed;
    }
    return abs(signed) - b.z;
}

// How much of the pixel at canvas point p its mask lets through: the most any shape covers,
// smoothed over one pixel of the picture.
fn mask_cover(p: vec2<f32>) -> f32 {
    let px = max(g.view.z, 1e-4);
    var cover = 0.0;
    let count = u32(canvas.clock.y);
    for (var i = 0u; i < count; i++) {
        cover = max(cover, clamp(0.5 - mask_distance(i, p) / px, 0.0, 1.0));
        if cover >= 1.0 {
            break;
        }
    }
    return cover;
}

// Flat: the marks' own colours, as they were put down.
fn flat(in: CanvasOut) -> vec4<f32> {
    let p = textureSample(paint, smooth_sampler, in.uv);
    // The time painted is read from the nearest texel, never blended with an older one, so a
    // mark's edge fades with the mark.
    let dims = textureDimensions(aux);
    let texel = min(vec2<u32>(in.uv * vec2<f32>(dims)), dims - vec2<u32>(1u));
    let painted = textureLoad(aux, texel, 0).r;
    var fade = 1.0;
    if canvas.size.z > 0.0 {
        fade = clamp(1.0 - (canvas.clock.x - painted) / canvas.size.z, 0.0, 1.0);
    }
    // Flat: the marks' own colour, as much as was put down.
    let colour = max(p.rgb, vec3<f32>(0.0)) / max(p.a, 1e-4);
    let amount = clamp(p.a, 0.0, 1.0) * fade;
    // The start colour shows under the marks.
    let under = clamp(canvas.start.a, 0.0, 1.0) * (1.0 - amount);
    let rgb = colour * amount + canvas.start.rgb * under;
    var shown = clamp(canvas.size.w, 0.0, 1.0);
    if canvas.clock.z > 0.5 && amount + under > 0.0 {
        shown *= mask_cover(in.local);
    }
    return vec4<f32>(rgb, amount + under) * shown;
}

// How wet paint painted at `painted` still is: 1 just painted, 0 once the dry time has passed.
fn wetness(painted: f32) -> f32 {
    if canvas.look.y <= 0.0 {
        return 0.0;
    }
    return clamp(1.0 - (canvas.clock.x - painted) / canvas.look.y, 0.0, 1.0);
}

// The time painted at uv, from the nearest texel.
fn painted_at(uv: vec2<f32>) -> f32 {
    let dims = textureDimensions(aux);
    let texel = min(vec2<u32>(clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)) * vec2<f32>(dims)), dims - vec2<u32>(1u));
    return textureLoad(aux, texel, 0).r;
}

fn fade_of(painted: f32) -> f32 {
    if canvas.size.z > 0.0 {
        return clamp(1.0 - (canvas.clock.x - painted) / canvas.size.z, 0.0, 1.0);
    }
    return 1.0;
}

fn paint_at(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(paint, smooth_sampler, uv, 0.0);
}

fn hash2(p: vec2<i32>) -> f32 {
    var x = bitcast<u32>(p.x) * 1597334677u ^ bitcast<u32>(p.y) * 3812015801u;
    x = (x ^ (x >> 16u)) * 2246822519u;
    x = x ^ (x >> 13u);
    return f32(x) / 4294967295.0;
}

fn noise(p: vec2<f32>) -> f32 {
    let i = vec2<i32>(floor(p));
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(hash2(i), hash2(i + vec2<i32>(1, 0)), u.x),
        mix(hash2(i + vec2<i32>(0, 1)), hash2(i + vec2<i32>(1, 1)), u.x),
        u.y,
    );
}

// The eight directions round a point, the second ring turned by `turn` radians.
fn ring_dir(i: u32, turn: f32) -> vec2<f32> {
    let a = TAU * f32(i) / 8.0 + turn;
    return vec2<f32>(cos(a), sin(a));
}

const HEIGHT_PX: f32 = 6.0;

fn height(uv: vec2<f32>) -> f32 {
    return (textureSampleLevel(body, smooth_sampler, uv, 0.0).r + 0.35 * paint_at(uv).a) * HEIGHT_PX;
}

// The most any mask shape's glow reaches p, falling off to nothing `reach` past its edge.
fn mask_glow(p: vec2<f32>, reach: f32) -> f32 {
    var glow = 0.0;
    let count = u32(canvas.clock.y);
    for (var i = 0u; i < count; i++) {
        glow = max(glow, clamp(1.0 - max(mask_distance(i, p), 0.0) / max(reach, 1e-3), 0.0, 1.0));
    }
    return glow;
}

fn wet_paint(in: CanvasOut) -> vec4<f32> {
    let p = paint_at(in.uv);
    let painted = painted_at(in.uv);
    let fade = fade_of(painted);
    let colour = max(p.rgb, vec3<f32>(0.0)) / max(p.a, 1e-4);
    let amount = clamp(p.a, 0.0, 1.0) * fade;
    let under = clamp(canvas.start.a, 0.0, 1.0) * (1.0 - amount);
    let dims = vec2<f32>(textureDimensions(paint));
    let texel_uv = 1.0 / dims;
    // The slope of the paint's height, in height pixels per canvas pixel.
    let ex = vec2<f32>(1.5 * texel_uv.x, 0.0);
    let ey = vec2<f32>(0.0, 1.5 * texel_uv.y);
    let tex_px = canvas.size.xy * texel_uv;
    let dx = (height(in.uv + ex) - height(in.uv - ex)) / (3.0 * tex_px.x);
    let dy = (height(in.uv + ey) - height(in.uv - ey)) / (3.0 * tex_px.y);
    let n = normalize(vec3<f32>(-dx, -dy, 1.0));
    // The light, clockwise from up in the world, turned into the canvas's frame.
    let a = canvas.look.z - canvas.pose.z;
    let l = normalize(vec3<f32>(sin(a), -cos(a), 1.0));
    let h = normalize(l + vec3<f32>(0.0, 0.0, 1.0));
    let shine = mix(10.0, 60.0, canvas.look.w);
    let gain = max(pow(max(dot(n, h), 0.0), shine) - pow(h.z, shine), 0.0);
    let wet = wetness(painted);
    let strength = canvas.look.w * mix(0.12, 1.0, wet);
    let base = colour * (1.0 - 0.08 * wet);
    let rgb = base * amount + vec3<f32>(gain * strength * 1.5) * amount + canvas.start.rgb * under;
    var shown = clamp(canvas.size.w, 0.0, 1.0);
    if canvas.clock.z > 0.5 && amount + under > 0.0 {
        shown *= mask_cover(in.local);
    }
    return vec4<f32>(rgb, amount + under) * shown;
}

fn watercolour(in: CanvasOut) -> vec4<f32> {
    let dims = vec2<f32>(textureDimensions(paint));
    let texel_uv = 1.0 / dims;
    let centre = paint_at(in.uv);
    let painted = painted_at(in.uv);
    var wet0 = 0.0;
    if centre.a > 0.01 {
        wet0 = wetness(painted);
    }
    for (var i = 0u; i < 8u; i++) {
        let uv = in.uv + ring_dir(i, 0.0) * 4.0 * texel_uv;
        if paint_at(uv).a > 0.01 {
            wet0 = max(wet0, wetness(painted_at(uv)));
        }
    }
    // Wet washes spread over a wider circle of texels than dry ones.
    let r = mix(1.0, 4.0, wet0);
    var sum_a = centre.a;
    var sum_rgb = centre.rgb;
    var near = 0.0;
    var wide = 0.0;
    for (var i = 0u; i < 8u; i++) {
        let d = ring_dir(i, 0.0);
        let q = paint_at(in.uv + d * r * texel_uv);
        near += q.a;
        sum_a += q.a;
        sum_rgb += q.rgb;
        wide += paint_at(in.uv + d * 3.0 * texel_uv).a;
    }
    let cov = 0.5 * centre.a + 0.5 * near / 8.0;
    let colour = max(sum_rgb, vec3<f32>(0.0)) / max(sum_a, 1e-4);
    let blur = wide / 8.0;
    let edge = clamp((cov - blur) * 3.0, 0.0, 1.0);
    let grain = canvas.look2.x;
    let gn = 0.65 * noise(in.local / 6.0) + 0.35 * noise(in.local / 2.5 + vec2<f32>(17.0));
    let fade = fade_of(painted);
    let alpha = clamp(cov * 0.75 * (1.0 - grain * 0.5 * gn) * fade, 0.0, 1.0);
    let rgb = colour * (1.0 - canvas.look2.y * edge * 0.6) * (1.0 - grain * 0.25 * (1.0 - gn));
    let under = clamp(canvas.start.a, 0.0, 1.0) * (1.0 - alpha);
    let start = canvas.start.rgb * (1.0 - grain * 0.08 * gn);
    var shown = clamp(canvas.size.w, 0.0, 1.0);
    if canvas.clock.z > 0.5 && alpha + under > 0.0 {
        shown *= mask_cover(in.local);
    }
    return vec4<f32>(rgb * alpha + start * under, alpha + under) * shown;
}

fn neon_ink(in: CanvasOut) -> vec4<f32> {
    let p = paint_at(in.uv);
    let painted = painted_at(in.uv);
    let fade = fade_of(painted);
    let colour = max(p.rgb, vec3<f32>(0.0)) / max(p.a, 1e-4);
    let amount = clamp(p.a, 0.0, 1.0) * fade;
    let under = clamp(canvas.start.a, 0.0, 1.0) * (1.0 - amount);
    let wet = wetness(painted);
    let line = colour * (1.5 + 1.5 * wet) * amount + canvas.start.rgb * under;
    // The glow: two rings of eight taps, their light falling off with the distance and with
    // how dry the paint there is.
    let g = max(canvas.look2.z, 1e-3);
    let px_uv = 1.0 / canvas.size.xy;
    var glow = vec3<f32>(0.0);
    for (var ring = 0u; ring < 2u; ring++) {
        let d = g * select(0.8, 0.35, ring == 0u);
        let turn = select(0.3926991, 0.0, ring == 0u);
        for (var i = 0u; i < 8u; i++) {
            let uv = in.uv + ring_dir(i, turn) * d * px_uv;
            if uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0 {
                continue;
            }
            let q = paint_at(uv);
            let tap_painted = painted_at(uv);
            let w = wetness(tap_painted) * fade_of(tap_painted) * (1.0 - d / g) * (1.0 - d / g);
            glow += max(q.rgb, vec3<f32>(0.0)) * w / 16.0;
        }
    }
    glow *= canvas.look2.w * 2.0;
    var shown_line = clamp(canvas.size.w, 0.0, 1.0);
    var shown_glow = shown_line;
    if canvas.clock.z > 0.5 {
        if amount + under > 0.0 {
            shown_line *= mask_cover(in.local);
        }
        if dot(glow, vec3<f32>(1.0)) > 0.0 {
            shown_glow *= mask_glow(in.local, g);
        }
    }
    // The glow adds light and covers nothing behind it.
    return vec4<f32>(line * shown_line + glow * shown_glow, (amount + under) * shown_line);
}

@fragment
fn fs_canvas(in: CanvasOut) -> @location(0) vec4<f32> {
    if canvas.look.x < 0.5 {
        return flat(in);
    }
    if canvas.look.x < 1.5 {
        return wet_paint(in);
    }
    if canvas.look.x < 2.5 {
        return watercolour(in);
    }
    return neon_ink(in);
}

// Post-processing: bloom (downsample and upsample chain) and the final composite.

struct Globals {
    view: vec4<f32>,
    // The background paint: atlas row, start, scroll, repeat.
    bg_paint: vec4<f32>,
    // The background place: direction, span, centre xy.
    bg_place: vec4<f32>,
    // x: bloom strength, y: exposure, z: 1 to run the colour grade
    post: vec4<f32>,
    // centre xy, zoom, turn
    camera: vec4<f32>,
    // x: background on screen, y: how its place runs, z: steps
    bg: vec4<f32>,
    bg_from: vec4<f32>,
    bg_to: vec4<f32>,
    // The colour grade: brightness, contrast, saturation, warmth.
    grade: vec4<f32>,
    // The colour grade: tint, vignette, vignette size, gradient map amount.
    grade_more: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var src: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;
@group(1) @binding(2) var bloom_tex: texture_2d<f32>;
// The grade's gradient map, 256 by 1, linear light.
@group(1) @binding(3) var grade_map: texture_2d<f32>;

struct FsIn {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_fullscreen(@builtin(vertex_index) i: u32) -> FsIn {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: FsIn;
    out.pos = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(uv.x, 1.0 - uv.y);
    return out;
}

fn s(uv: vec2<f32>, offset: vec2<f32>, texel: vec2<f32>) -> vec3<f32> {
    return min(textureSample(src, samp, uv + offset * texel).rgb, vec3<f32>(64.0));
}

// 13-tap downsample (Jimenez 2014), which avoids the flicker of a plain box filter.
@fragment
fn fs_downsample(in: FsIn) -> @location(0) vec4<f32> {
    let t = 1.0 / vec2<f32>(textureDimensions(src));
    let uv = in.uv;
    let a = s(uv, vec2<f32>(-2.0, -2.0), t);
    let b = s(uv, vec2<f32>(0.0, -2.0), t);
    let c = s(uv, vec2<f32>(2.0, -2.0), t);
    let d = s(uv, vec2<f32>(-2.0, 0.0), t);
    let e = s(uv, vec2<f32>(0.0, 0.0), t);
    let f = s(uv, vec2<f32>(2.0, 0.0), t);
    let gg = s(uv, vec2<f32>(-2.0, 2.0), t);
    let h = s(uv, vec2<f32>(0.0, 2.0), t);
    let i = s(uv, vec2<f32>(2.0, 2.0), t);
    let j = s(uv, vec2<f32>(-1.0, -1.0), t);
    let k = s(uv, vec2<f32>(1.0, -1.0), t);
    let l = s(uv, vec2<f32>(-1.0, 1.0), t);
    let m = s(uv, vec2<f32>(1.0, 1.0), t);
    let color = e * 0.125 + (a + c + gg + i) * 0.03125 + (b + d + f + h) * 0.0625 + (j + k + l + m) * 0.125;
    return vec4<f32>(color, 1.0);
}

// 3x3 tent upsample, added onto the next larger level.
@fragment
fn fs_upsample(in: FsIn) -> @location(0) vec4<f32> {
    let t = 1.0 / vec2<f32>(textureDimensions(src));
    let uv = in.uv;
    var color = s(uv, vec2<f32>(0.0, 0.0), t) * 4.0;
    color += (s(uv, vec2<f32>(0.0, -1.0), t) + s(uv, vec2<f32>(-1.0, 0.0), t)
        + s(uv, vec2<f32>(1.0, 0.0), t) + s(uv, vec2<f32>(0.0, 1.0), t)) * 2.0;
    color += s(uv, vec2<f32>(-1.0, -1.0), t) + s(uv, vec2<f32>(1.0, -1.0), t)
        + s(uv, vec2<f32>(-1.0, 1.0), t) + s(uv, vec2<f32>(1.0, 1.0), t);
    return vec4<f32>(color / 16.0, 1.0);
}

// Identity below 0.8, then a soft shoulder that rolls highlights off towards white.
fn shoulder(x: f32) -> f32 {
    if x < 0.8 {
        return x;
    }
    return 0.8 + 0.2 * (1.0 - exp(-(x - 0.8) / 0.2));
}

fn srgb_encode(c: f32) -> f32 {
    if c <= 0.0031308 {
        return c * 12.92;
    }
    return 1.055 * pow(c, 1.0 / 2.4) - 0.055;
}

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// The scene's colour grade on display-ready (sRGB-encoded) colours, in this order: gradient
// map, brightness, contrast, saturation, warmth, tint, vignette; then clamped to 0..1.
fn grade(colour: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    var c = colour;
    let amount = g.grade_more.w;
    if amount > 0.0 {
        // The baked row's first texel is at 0 and its last at 1.
        let t = clamp(luma(c), 0.0, 1.0) * (255.0 / 256.0) + 0.5 / 256.0;
        let m = textureSampleLevel(grade_map, samp, vec2<f32>(t, 0.5), 0.0).rgb;
        c = mix(c, vec3<f32>(srgb_encode(m.r), srgb_encode(m.g), srgb_encode(m.b)), amount);
    }
    c *= 1.0 + g.grade.x;
    c = (c - 0.5) * (1.0 + g.grade.y) + 0.5;
    c = mix(vec3<f32>(luma(c)), c, 1.0 + g.grade.z);
    c.r += 0.1 * g.grade.w;
    c.b -= 0.1 * g.grade.w;
    c.g -= 0.1 * g.grade_more.x;
    // smoothstep(size, 1, d), written out so a size of 1 (equal edges) is no step, not a NaN.
    let d = length((uv - 0.5) * 2.0) / sqrt(2.0);
    let size = g.grade_more.z;
    let s = clamp((d - size) / max(1.0 - size, 1e-5), 0.0, 1.0);
    c *= 1.0 - g.grade_more.y * s * s * (3.0 - 2.0 * s);
    return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fs_composite(in: FsIn) -> @location(0) vec4<f32> {
    let hdr = textureSample(src, samp, in.uv).rgb;
    let bloom = textureSample(bloom_tex, samp, in.uv).rgb;
    let c = (hdr + bloom * g.post.x * 0.35) * g.post.y;
    // Highlights desaturate towards white, like an overexposed light.
    let peak = max(c.r, max(c.g, c.b));
    let over = max(peak - 1.0, 0.0);
    let toward_white = c + vec3<f32>(over * 0.35);
    let mapped = vec3<f32>(shoulder(toward_white.r), shoulder(toward_white.g), shoulder(toward_white.b));
    let dither = (hash(in.pos.xy) - 0.5) / 255.0;
    // The output texture is plain Rgba8Unorm holding sRGB-encoded values (what egui and video encoders expect).
    var out = vec3<f32>(srgb_encode(mapped.r), srgb_encode(mapped.g), srgb_encode(mapped.b));
    // At its defaults the grade is skipped altogether, so the picture is exactly as without it.
    if g.post.z > 0.5 {
        out = grade(out, in.uv);
    }
    return vec4<f32>(out.r + dither, out.g + dither, out.b + dither, 1.0);
}

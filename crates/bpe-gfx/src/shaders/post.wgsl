// Post-processing: bloom (downsample and upsample chain) and the final composite.

struct Globals {
    view: vec4<f32>,
    bg_top: vec4<f32>,
    bg_bottom: vec4<f32>,
    // x: bloom strength, y: exposure
    post: vec4<f32>,
    // centre xy, zoom, turn
    camera: vec4<f32>,
    // x: background on screen, y, z: world heights it spans
    bg: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var src: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;
@group(1) @binding(2) var bloom_tex: texture_2d<f32>;

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
    return vec4<f32>(
        srgb_encode(mapped.r) + dither,
        srgb_encode(mapped.g) + dither,
        srgb_encode(mapped.b) + dither,
        1.0,
    );
}

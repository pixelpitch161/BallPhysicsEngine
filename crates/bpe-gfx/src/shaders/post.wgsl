// Post-processing: bloom (downsample and upsample chain) and the final composite.

struct Globals {
    view: vec4<f32>,
    // The background paint: atlas row, start, scroll, repeat.
    bg_paint: vec4<f32>,
    // The background place: direction, span, centre xy.
    bg_place: vec4<f32>,
    // x: bloom strength, y: exposure, z: 1 to run the colour grade (grade.wgsl)
    post: vec4<f32>,
    // centre xy, zoom, turn
    camera: vec4<f32>,
    // x: background on screen, y: how its place runs, z: grain
    bg: vec4<f32>,
    bg_from: vec4<f32>,
    bg_to: vec4<f32>,
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

// Brighter than the screen can show: up to 1 the colour is untouched (white stays white). Above,
// it is scaled down by its brightest channel, so its hue is kept, and turned toward white by how
// much white light is in it (its least channel: a shine on a red ball, a lamp-lit wall burn
// white) or how far its brightness is past 1 (a hot core burns white). A saturated glow keeps
// its colour: a crowd of glowing purple balls stays purple instead of washing out to lavender.
fn roll_off(c: vec3<f32>) -> vec3<f32> {
    let peak = max(c.r, max(c.g, c.b));
    if peak <= 1.0 {
        return c;
    }
    let white = clamp(min(c.r, min(c.g, c.b)), 0.0, 1.0);
    let over = max(luma(c) - 1.0, 0.0);
    // Eased in over the first quarter past 1, so nothing jumps where the roll-off starts.
    let whiter = max(white * white, over / (over + 0.5)) * min((peak - 1.0) * 4.0, 1.0);
    return mix(c / peak, vec3<f32>(1.0), whiter);
}

fn srgb_encode(c: f32) -> f32 {
    if c <= 0.0031308 {
        return c * 12.92;
    }
    return 1.055 * pow(c, 1.0 / 2.4) - 0.055;
}

// Three independent uniform numbers in 0..1 for pixel `p`, round `k`: a PCG hash.
fn dither_noise(p: vec2<u32>, k: u32) -> vec3<f32> {
    var v = vec3<u32>(p.x, p.y, k) * vec3<u32>(1664525u) + vec3<u32>(1013904223u);
    v.x += v.y * v.z;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    v = v ^ (v >> vec3<u32>(16u));
    v.x += v.y * v.z;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    return vec3<f32>(v >> vec3<u32>(8u)) / 16777216.0;
}

@fragment
fn fs_composite(in: FsIn) -> @location(0) vec4<f32> {
    let hdr = textureSample(src, samp, in.uv).rgb;
    let bloom = textureSample(bloom_tex, samp, in.uv).rgb;
    let c = (hdr + bloom * g.post.x * 0.35) * g.post.y;
    // Clamped to the half floats' largest first: summed glows can reach infinity, and inf / inf
    // would draw the hottest spot black.
    var lin = clamp(c, vec3<f32>(0.0), vec3<f32>(65504.0));
    // The colour grade works on the linear picture, light above white kept, before the roll-off
    // squeezes it into the screen's range. With no grade it is skipped altogether, so the picture
    // is exactly as without it.
    if g.post.z > 0.5 {
        lin = clamp(run_grade(lin, in.uv), vec3<f32>(0.0), vec3<f32>(65504.0));
    }
    let mapped = roll_off(lin);
    // Triangular dither of ±1 step, the same on every channel (two uniform numbers summed): no banding survives the
    // 8-bit output, and no sin() whose precision differs from GPU to GPU.
    let n = dither_noise(vec2<u32>(in.pos.xy), 0u);
    let dither = (n.x + n.y - 1.0) / 255.0;
    // The output texture is plain Rgba8Unorm holding sRGB-encoded values (what egui and video encoders expect).
    let out = vec3<f32>(srgb_encode(mapped.r), srgb_encode(mapped.g), srgb_encode(mapped.b));
    // Pure black and pure white stay exact (no speckle on a black background); the dither fades
    // in over the first and last step.
    let room = clamp(min(out, vec3<f32>(1.0) - out) * 255.0, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(out + dither * room, 1.0);
}

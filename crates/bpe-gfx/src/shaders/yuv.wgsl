// The output picture as video colours (review S23): BT.709, TV range, chroma at half size each way,
// worked out here so the export sends ffmpeg 1.5 bytes a pixel instead of 4 and its CPU does no
// colour conversion. Reads the 8-bit output (gamma-encoded R'G'B', as ffmpeg's own conversion
// read it); every value is rounded to a whole 8-bit step here, so the targets store it exactly.

@group(0) @binding(0) var picture: texture_2d<f32>;

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

const KR: f32 = 0.2126;
const KB: f32 = 0.0722;

fn luma(c: vec3<f32>) -> f32 {
    return KR * c.r + (1.0 - KR - KB) * c.g + KB * c.b;
}

// `v` in 8-bit steps (0 to 255), as a unorm value.
fn byte(v: f32) -> f32 {
    return clamp(round(v), 0.0, 255.0) / 255.0;
}

@fragment
fn fs_luma(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let c = textureLoad(picture, vec2<i32>(pos.xy), 0).rgb;
    return vec4<f32>(byte(16.0 + 219.0 * luma(c)), 0.0, 0.0, 1.0);
}

// Cb and Cr (8-bit steps) of the 2 x 2 block this half-size pixel covers, from its mean colour.
fn chroma(pos: vec4<f32>) -> vec2<f32> {
    let at = vec2<i32>(pos.xy) * 2;
    let c = 0.25 * (textureLoad(picture, at, 0).rgb + textureLoad(picture, at + vec2<i32>(1, 0), 0).rgb
        + textureLoad(picture, at + vec2<i32>(0, 1), 0).rgb + textureLoad(picture, at + vec2<i32>(1, 1), 0).rgb);
    let y = luma(c);
    let cb = (c.b - y) / (2.0 * (1.0 - KB));
    let cr = (c.r - y) / (2.0 * (1.0 - KR));
    return vec2<f32>(byte(128.0 + 224.0 * cb), byte(128.0 + 224.0 * cr));
}

struct Planes {
    @location(0) u: vec4<f32>,
    @location(1) v: vec4<f32>,
};

// yuv420p: Cb and Cr each into a plane of its own.
@fragment
fn fs_chroma_planes(@builtin(position) pos: vec4<f32>) -> Planes {
    let c = chroma(pos);
    return Planes(vec4<f32>(c.x, 0.0, 0.0, 1.0), vec4<f32>(c.y, 0.0, 0.0, 1.0));
}

// nv12: Cb and Cr side by side in one plane.
@fragment
fn fs_chroma_pairs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let c = chroma(pos);
    return vec4<f32>(c.x, c.y, 0.0, 1.0);
}

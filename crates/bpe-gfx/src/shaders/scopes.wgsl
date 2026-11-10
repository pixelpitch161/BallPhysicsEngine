// The scopes (colour grading, grade/scopes.rs), counted from the finished 8-bit picture. Never
// part of an export (the clipping views are clipping.wgsl).
//
// Bins by scope:
// - Histogram: 0..255 red, 256..511 green, 512..767 blue, 768..1023 brightness.
// - Waveform: level * 256 + column (brightness).
// - RGB Parade: channel * 65536 + level * 256 + column (red, green, blue).
// - Vectorscope: v * 256 + u (u from Cb, v from Cr, BT.709 on the encoded values).

@group(0) @binding(0) var picture: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> bins: array<atomic<u32>>;

var<workgroup> counted: array<atomic<u32>, 1024>;

const LUMA: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);

fn bin(v: f32) -> u32 {
    return u32(clamp(v * 255.0 + 0.5, 0.0, 255.0));
}

@compute @workgroup_size(16, 16)
fn cs_histogram(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) li: u32) {
    for (var i = li; i < 1024u; i += 256u) {
        atomicStore(&counted[i], 0u);
    }
    workgroupBarrier();
    let size = textureDimensions(picture);
    if id.x < size.x && id.y < size.y {
        let c = textureLoad(picture, vec2<i32>(id.xy), 0).rgb;
        atomicAdd(&counted[bin(c.r)], 1u);
        atomicAdd(&counted[256u + bin(c.g)], 1u);
        atomicAdd(&counted[512u + bin(c.b)], 1u);
        atomicAdd(&counted[768u + bin(dot(c, LUMA))], 1u);
    }
    workgroupBarrier();
    for (var i = li; i < 1024u; i += 256u) {
        let n = atomicLoad(&counted[i]);
        if n > 0u {
            atomicAdd(&bins[i], n);
        }
    }
}

// The column (0..255) a pixel's x counts in.
fn column(x: u32, width: u32) -> u32 {
    return min(x * 256u / max(width, 1u), 255u);
}

@compute @workgroup_size(16, 16)
fn cs_waveform(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(picture);
    if id.x >= size.x || id.y >= size.y {
        return;
    }
    let c = textureLoad(picture, vec2<i32>(id.xy), 0).rgb;
    atomicAdd(&bins[bin(dot(c, LUMA)) * 256u + column(id.x, size.x)], 1u);
}

@compute @workgroup_size(16, 16)
fn cs_parade(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(picture);
    if id.x >= size.x || id.y >= size.y {
        return;
    }
    let c = textureLoad(picture, vec2<i32>(id.xy), 0).rgb;
    let x = column(id.x, size.x);
    atomicAdd(&bins[bin(c.r) * 256u + x], 1u);
    atomicAdd(&bins[65536u + bin(c.g) * 256u + x], 1u);
    atomicAdd(&bins[131072u + bin(c.b) * 256u + x], 1u);
}

@compute @workgroup_size(16, 16)
fn cs_vectorscope(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(picture);
    if id.x >= size.x || id.y >= size.y {
        return;
    }
    let c = textureLoad(picture, vec2<i32>(id.xy), 0).rgb;
    let y = dot(c, LUMA);
    let cb = (c.b - y) / 1.8556;
    let cr = (c.r - y) / 1.5748;
    atomicAdd(&bins[bin(cr + 0.5) * 256u + bin(cb + 0.5)], 1u);
}

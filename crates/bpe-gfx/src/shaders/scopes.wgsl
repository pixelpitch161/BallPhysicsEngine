// The scopes (colour grading, grade/scopes.rs): the histogram of the finished 8-bit picture.
// Bins 0..255 count red, 256..511 green, 512..767 blue, 768..1023 brightness.

@group(0) @binding(0) var picture: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> bins: array<atomic<u32>, 1024>;

var<workgroup> counted: array<atomic<u32>, 1024>;

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
        atomicAdd(&counted[768u + bin(dot(c, vec3<f32>(0.2126, 0.7152, 0.0722)))], 1u);
    }
    workgroupBarrier();
    for (var i = li; i < 1024u; i += 256u) {
        let n = atomicLoad(&counted[i]);
        if n > 0u {
            atomicAdd(&bins[i], n);
        }
    }
}

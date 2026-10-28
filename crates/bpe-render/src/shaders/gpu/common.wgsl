// Shared by the swarm, fluid and particle shaders: hashing and the wall distance field.
// World coordinates are video pixels, origin at the frame centre, y down.

const PI: f32 = 3.14159265;
const TAU: f32 = 6.28318531;

// Where the wall distance field sits in the world. Matches `SdfInfo` in gpu/sdf.rs.
struct SdfInfo {
    // xy: world position of texel (0, 0)'s corner, z: world size of one texel, w: largest stored distance
    origin: vec4<f32>,
    // xy: size in texels, z: 1 when the field holds walls, 0 when it is empty
    size: vec4<f32>,
};

// A hash of two numbers into 0..1, the same as `particle_hash` on the CPU.
fn hash2(a: u32, b: u32) -> f32 {
    var h = (a * 0x9e3779b9u) ^ (b * 0x85ebca6bu);
    h = h ^ (h >> 16u);
    h = h * 0x7feb352du;
    h = h ^ (h >> 15u);
    h = h * 0x846ca68bu;
    h = h ^ (h >> 16u);
    return f32(h >> 8u) / 16777216.0;
}

fn sdf_texel(t: vec2<i32>) -> f32 {
    let size = vec2<i32>(sdf_info.size.xy);
    let c = clamp(t, vec2<i32>(0), size - vec2<i32>(1));
    return textureLoad(sdf_texture, c, 0).r;
}

// Distance from p to the nearest wall, positive in open space and negative inside walls.
// Far from any wall it is at most `sdf_info.origin.w`.
fn sdf_at(p: vec2<f32>) -> f32 {
    if sdf_info.size.z < 0.5 {
        return sdf_info.origin.w;
    }
    let q = (p - sdf_info.origin.xy) / sdf_info.origin.z - vec2<f32>(0.5);
    let i = vec2<i32>(floor(q));
    let f = q - floor(q);
    let a = sdf_texel(i);
    let b = sdf_texel(i + vec2<i32>(1, 0));
    let c = sdf_texel(i + vec2<i32>(0, 1));
    let d = sdf_texel(i + vec2<i32>(1, 1));
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

// Direction away from the nearest wall, towards open space.
fn sdf_normal(p: vec2<f32>) -> vec2<f32> {
    let e = sdf_info.origin.z;
    let g = vec2<f32>(sdf_at(p + vec2<f32>(e, 0.0)) - sdf_at(p - vec2<f32>(e, 0.0)),
                      sdf_at(p + vec2<f32>(0.0, e)) - sdf_at(p - vec2<f32>(0.0, e)));
    let len = length(g);
    if len < 1e-6 {
        return vec2<f32>(0.0, -1.0);
    }
    return g / len;
}

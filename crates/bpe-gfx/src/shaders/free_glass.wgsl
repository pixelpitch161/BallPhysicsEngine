// Glass and light part 3: free glass (particles and glass liquid that bend lamp light, drawn into
// the glass picture by light pass 9: index of refraction × cover in R, cover in A) marked in the
// surface picture, drawn over a copy of it. Where something solid is it stays as it is; elsewhere
// R (blocking), G (height) and A (cover) stay and B becomes free glass's code (lighting.wgsl's
// `free_glass_of`): -(1 + 16 q_ior + q_cover), whole numbers half floats keep exactly.

@group(0) @binding(0) var glass_pic: texture_2d<f32>;
@group(0) @binding(1) var under: texture_2d<f32>;

// lighting.wgsl's SOLID.
const SOLID: f32 = 0.98;

struct GlassOut {
    @builtin(position) pos: vec4<f32>,
};

@vertex
fn vs_glass(@builtin(vertex_index) i: u32) -> GlassOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: GlassOut;
    out.pos = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    return out;
}

@fragment
fn fs_glass(in: GlassOut) -> @location(0) vec4<f32> {
    let ip = vec2<i32>(in.pos.xy);
    let s = textureLoad(under, ip, 0);
    let g = textureLoad(glass_pic, ip, 0);
    if g.a < 0.02 || min(s.r, 1.0) >= SOLID {
        return s;
    }
    let ior = g.r / g.a;
    if ior <= 1.0001 {
        return s;
    }
    let qi = clamp(round((ior - 1.0) * 64.0), 1.0, 127.0);
    let qc = clamp(round(min(g.a, 1.0) * 15.0), 1.0, 15.0);
    return vec4<f32>(s.r, s.g, -(1.0 + 16.0 * qi + qc), s.a);
}

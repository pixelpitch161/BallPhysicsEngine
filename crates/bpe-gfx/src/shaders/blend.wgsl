// The blend composite: mixes a layer of things drawn with one blend and amount into the picture
// under them. The layer holds premultiplied colour and coverage; light added with no coverage
// (alpha 0) counts as covering as much as its brightest channel. A layer whose things blend with
// each other (see each.wgsl) holds their combined colour instead, and covers wherever its alpha,
// the largest coverage, is above 0.

struct Mix {
    // x: the blend's number (Blend::shader_code), y: the amount, 0 to 1, z: 1 when the layer's
    // things blend with each other, w: 1 for a plain layer, put on as if drawn straight in, plus
    // 2 for the lighting's surface picture (its alpha then gains the layer's cover).
    how: vec4<f32>,
};

@group(0) @binding(0) var<uniform> mixing: Mix;
@group(0) @binding(1) var under: texture_2d<f32>;
@group(0) @binding(2) var layer: texture_2d<f32>;

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_blend(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(pos.xy);
    let d = textureLoad(under, p, 0);
    let l = textureLoad(layer, p, 0);
    var out = mixed(d, l);
    if mixing.how.w > 1.5 {
        // The surface picture keeps how much its blockers cover, the layer's over what is under.
        out.a = d.a + clamp(l.a, 0.0, 1.0) * clamp(mixing.how.y, 0.0, 1.0) * (1.0 - d.a);
    }
    return out;
}

fn mixed(d: vec4<f32>, l: vec4<f32>) -> vec4<f32> {
    let code = u32(mixing.how.x + 0.5);
    let amount = clamp(mixing.how.y, 0.0, 1.0);
    let c = max(l.rgb, vec3<f32>(0.0));
    let plain = mixing.how.w - select(0.0, 2.0, mixing.how.w > 1.5);
    if plain > 0.5 {
        // Premultiplied over: light with no coverage adds, as it does drawn straight in.
        return vec4<f32>(d.rgb * (1.0 - clamp(l.a, 0.0, 1.0)) + c, d.a);
    }
    if mixing.how.z > 0.5 && code != 1u {
        if l.a <= 0.0 {
            return d;
        }
        return vec4<f32>(mix(d.rgb, blended(code, d.rgb, l.rgb), amount), d.a);
    }
    if code == 1u {
        // Add stays additive: its light goes on at the amount, coverage or not.
        return vec4<f32>(d.rgb + c * amount, d.a);
    }
    var a = clamp(l.a, 0.0, 1.0);
    let light = max(c.r, max(c.g, c.b));
    if a < 1e-4 && light > 0.0 {
        a = clamp(light, 0.0, 1.0);
    }
    let s = c / max(a, 1e-4);
    return vec4<f32>(mix(d.rgb, blended(code, d.rgb, s), a * amount), d.a);
}

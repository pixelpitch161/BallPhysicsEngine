// The blend composite: mixes a layer of things drawn with one blend and amount into the picture
// under them. The layer holds premultiplied colour and coverage; light added with no coverage
// (alpha 0) counts as covering as much as its brightest channel. A layer whose things blend with
// each other (see each.wgsl) holds their combined colour instead, and covers wherever its alpha,
// the largest coverage, is above 0.

struct Mix {
    // x: the blend's number (Blend::shader_code), y: the amount, 0 to 1, z: 1 when the layer's
    // things blend with each other.
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

fn lum(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.3, 0.59, 0.11));
}

fn clip_colour(c_in: vec3<f32>) -> vec3<f32> {
    var c = c_in;
    let l = lum(c);
    let n = min(c.r, min(c.g, c.b));
    let x = max(c.r, max(c.g, c.b));
    if n < 0.0 {
        c = l + (c - l) * l / max(l - n, 1e-6);
    }
    if x > 1.0 {
        c = l + (c - l) * (1.0 - l) / max(x - l, 1e-6);
    }
    return c;
}

fn set_lum(c: vec3<f32>, l: f32) -> vec3<f32> {
    return clip_colour(c + (l - lum(c)));
}

fn sat(c: vec3<f32>) -> f32 {
    return max(c.r, max(c.g, c.b)) - min(c.r, min(c.g, c.b));
}

fn set_sat(c: vec3<f32>, s: f32) -> vec3<f32> {
    let n = min(c.r, min(c.g, c.b));
    let x = max(c.r, max(c.g, c.b));
    if x <= n {
        return vec3<f32>(0.0);
    }
    return (c - n) * s / (x - n);
}

fn overlay_one(d: f32, s: f32) -> f32 {
    if d <= 0.5 {
        return 2.0 * s * d;
    }
    return 1.0 - (1.0 - s) * (2.0 - 2.0 * d);
}

fn soft_light_one(d: f32, s: f32) -> f32 {
    if s <= 0.5 {
        return d - (1.0 - 2.0 * s) * d * (1.0 - d);
    }
    var k = sqrt(d);
    if d <= 0.25 {
        k = ((16.0 * d - 12.0) * d + 4.0) * d;
    }
    return d + (2.0 * s - 1.0) * (k - d);
}

// The blend of the picture `d` and the layer's own colour `s`, both straight (not premultiplied).
fn blended(code: u32, d: vec3<f32>, s: vec3<f32>) -> vec3<f32> {
    let dc = clamp(d, vec3<f32>(0.0), vec3<f32>(1.0));
    let sc = clamp(s, vec3<f32>(0.0), vec3<f32>(1.0));
    switch code {
        case 1u: { return d + s; }
        case 2u: { return 1.0 - (1.0 - dc) * (1.0 - sc); }
        case 3u: { return d * s; }
        case 4u: { return max(d, s); }
        case 5u: { return min(d, s); }
        case 6u: { return max(d - s, vec3<f32>(0.0)); }
        case 7u: {
            return vec3<f32>(overlay_one(dc.r, sc.r), overlay_one(dc.g, sc.g), overlay_one(dc.b, sc.b));
        }
        case 8u: {
            return vec3<f32>(soft_light_one(dc.r, sc.r), soft_light_one(dc.g, sc.g), soft_light_one(dc.b, sc.b));
        }
        case 9u: { return abs(d - s); }
        case 10u: { return set_lum(set_sat(sc, sat(dc)), lum(dc)); }
        case 11u: { return set_lum(sc, lum(dc)); }
        default: { return s; }
    }
}

@fragment
fn fs_blend(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(pos.xy);
    let d = textureLoad(under, p, 0);
    let l = textureLoad(layer, p, 0);
    let code = u32(mixing.how.x + 0.5);
    let amount = clamp(mixing.how.y, 0.0, 1.0);
    let c = max(l.rgb, vec3<f32>(0.0));
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

// The lens pre-pass (see lens.rs): a soft mip chain of an effect's bright parts, and streaks
// drawn out of them by Kawase's streak filter.

struct LensUniform {
    // One texel along the streak, in uv, and how many texels apart this pass's taps are.
    way: vec2<f32>,
    step: f32,
    // The weight of the next tap along (a to the power of `step`), and what the sum is scaled by.
    weight: f32,
    scale: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
};

@group(0) @binding(0) var<uniform> lu: LensUniform;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct FsIn {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> FsIn {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: FsIn;
    out.pos = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(uv.x, 1.0 - uv.y);
    return out;
}

fn tap(uv: vec2<f32>, o: vec2<f32>, t: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(src, samp, uv + o * t, 0.0).rgb;
}

// 13-tap downsample (Jimenez 2014), as the bloom's.
@fragment
fn fs_soft_down(in: FsIn) -> @location(0) vec4<f32> {
    let t = 1.0 / vec2<f32>(textureDimensions(src));
    let uv = in.uv;
    let a = tap(uv, vec2<f32>(-2.0, -2.0), t);
    let b = tap(uv, vec2<f32>(0.0, -2.0), t);
    let c = tap(uv, vec2<f32>(2.0, -2.0), t);
    let d = tap(uv, vec2<f32>(-2.0, 0.0), t);
    let e = tap(uv, vec2<f32>(0.0, 0.0), t);
    let f = tap(uv, vec2<f32>(2.0, 0.0), t);
    let g = tap(uv, vec2<f32>(-2.0, 2.0), t);
    let h = tap(uv, vec2<f32>(0.0, 2.0), t);
    let i = tap(uv, vec2<f32>(2.0, 2.0), t);
    let j = tap(uv, vec2<f32>(-1.0, -1.0), t);
    let k = tap(uv, vec2<f32>(1.0, -1.0), t);
    let l = tap(uv, vec2<f32>(-1.0, 1.0), t);
    let m = tap(uv, vec2<f32>(1.0, 1.0), t);
    let colour = e * 0.125 + (a + c + g + i) * 0.03125 + (b + d + f + h) * 0.0625 + (j + k + l + m) * 0.125;
    return vec4<f32>(colour, 1.0);
}

// One pass of a streak: four taps `step` texels apart back along the streak, each weighted a
// further `weight`, so n passes add up to the exponential tail sum of a^m over 4^n texels.
// Taps off the picture give nothing (no streaks out of the picture's edge).
@fragment
fn fs_ray(in: FsIn) -> @location(0) vec4<f32> {
    var sum = vec3<f32>(0.0);
    var w = 1.0;
    for (var k = 0; k < 4; k++) {
        let at = in.uv - lu.way * lu.step * f32(k);
        if all(at >= vec2<f32>(0.0)) && all(at <= vec2<f32>(1.0)) {
            sum += textureSampleLevel(src, samp, at, 0.0).rgb * w;
        }
        w *= lu.weight;
    }
    return vec4<f32>(sum * lu.scale, 1.0);
}

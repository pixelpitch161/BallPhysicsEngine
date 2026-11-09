// Things masks' silhouettes (colour grading part 5, renderer/silhouettes.rs): how far each pixel
// is from the chosen things, by jump flooding. The seed pass marks the pixels the chosen things
// half cover (the world's seen through the moved Stage camera when it is on, under the things
// stuck to the screen); each step looks `step` pixels
// round for a nearer seed; the last pass writes the things field: r the chosen coverage, g the
// distance in pixels to the nearest seed (1e4 with none), a every thing's coverage.

struct JfaStep {
    // x: the step in pixels; y: 1 to read the silhouette through the Stage camera's uv.
    k: vec4<f32>,
};

@group(0) @binding(0) var<uniform> jfa: JfaStep;
// The seed pass: the silhouette; the others: the seeds so far (xy a seed pixel or -1, z the
// chosen coverage, w every thing's).
@group(0) @binding(1) var jfa_in: texture_2d<f32>;
// Where each pixel of the moved Stage camera's picture read the front picture (0..1).
@group(0) @binding(2) var jfa_uv: texture_2d<f32>;
// The seed pass: the silhouette of the things stuck to the screen, over the world's and never
// seen through the Stage camera.
@group(0) @binding(3) var jfa_top: texture_2d<f32>;

struct JfaV {
    @builtin(position) pos: vec4<f32>,
};

@vertex
fn vs_jfa(@builtin(vertex_index) i: u32) -> JfaV {
    var out: JfaV;
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    out.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    return out;
}

@fragment
fn fs_jfa_seed(v: JfaV) -> @location(0) vec4<f32> {
    let pix = vec2<i32>(v.pos.xy);
    let size = vec2<i32>(textureDimensions(jfa_in));
    var at = pix;
    if jfa.k.y > 0.5 {
        let uv_size = vec2<i32>(textureDimensions(jfa_uv));
        let uv = textureLoad(jfa_uv, clamp(pix, vec2<i32>(0), uv_size - 1), 0).xy;
        at = vec2<i32>(floor(uv * vec2<f32>(size)));
    }
    at = clamp(at, vec2<i32>(0), size - 1);
    let world = textureLoad(jfa_in, at, 0);
    let top_size = vec2<i32>(textureDimensions(jfa_top));
    let top = textureLoad(jfa_top, clamp(pix, vec2<i32>(0), top_size - 1), 0) * f32(all(pix < top_size));
    let s = top + world * (1.0 - top.a);
    var seed = vec2<f32>(-1.0);
    if s.r >= 0.5 {
        seed = vec2<f32>(pix);
    }
    return vec4<f32>(seed, s.r, s.a);
}

@fragment
fn fs_jfa_step(v: JfaV) -> @location(0) vec4<f32> {
    let pix = vec2<i32>(v.pos.xy);
    let size = vec2<i32>(textureDimensions(jfa_in));
    let own = textureLoad(jfa_in, pix, 0);
    let here = vec2<f32>(pix);
    var best = own.xy;
    var near = 1e30;
    if best.x >= 0.0 {
        near = dot(here - best, here - best);
    }
    let k = i32(jfa.k.x);
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let q = pix + vec2<i32>(dx, dy) * k;
            if q.x < 0 || q.y < 0 || q.x >= size.x || q.y >= size.y {
                continue;
            }
            let s = textureLoad(jfa_in, q, 0).xy;
            if s.x >= 0.0 {
                let d = dot(here - s, here - s);
                if d < near {
                    near = d;
                    best = s;
                }
            }
        }
    }
    return vec4<f32>(best, own.zw);
}

@fragment
fn fs_jfa_last(v: JfaV) -> @location(0) vec4<f32> {
    let pix = vec2<i32>(v.pos.xy);
    let own = textureLoad(jfa_in, pix, 0);
    var d = 1e4;
    if own.x >= 0.0 {
        d = min(distance(vec2<f32>(pix), own.xy), 1e4);
    }
    return vec4<f32>(own.z, d, 0.0, own.w);
}

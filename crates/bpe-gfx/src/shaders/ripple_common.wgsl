// What the Ripples field and the paint that reads it share (gradients part 2). The sources'
// head: [0] the camera (centre xy, zoom, turn), [1] the video's size and the field's (0 when no
// field was built).

// Matches `RippleSource` in frame.rs: from xy and to xy (a circle: its centre twice); radius,
// owner, 0 a ball or 1 a shape; the owner's centre.
struct RippleSource {
    a: vec4<f32>,
    b: vec4<f32>,
    c: vec4<f32>,
};

// `world` on the screen (video pixels from the middle, y down), as scene.wgsl's `to_screen`.
fn ripple_screen(world: vec2<f32>, camera: vec4<f32>) -> vec2<f32> {
    var d = world - camera.xy;
    if camera.w != 0.0 {
        let s = sin(-camera.w);
        let k = cos(-camera.w);
        d = vec2<f32>(d.x * k - d.y * s, d.x * s + d.y * k);
    }
    return d * camera.z;
}

// The world point at field position `f` (texels, a texel's middle at .5).
fn ripple_world_of(f: vec2<f32>, camera: vec4<f32>, size: vec4<f32>) -> vec2<f32> {
    let screen = (f / max(size.zw, vec2<f32>(1.0)) - 0.5) * size.xy;
    var d = screen / max(camera.z, 1e-6);
    if camera.w != 0.0 {
        let s = sin(camera.w);
        let k = cos(camera.w);
        d = vec2<f32>(d.x * k - d.y * s, d.x * s + d.y * k);
    }
    return d + camera.xy;
}

// The field texel `world` falls in, kept inside the field.
fn ripple_texel_of(world: vec2<f32>, camera: vec4<f32>, size: vec4<f32>) -> vec2<i32> {
    let f = (ripple_screen(world, camera) / max(size.xy, vec2<f32>(1.0)) + 0.5) * size.zw;
    return clamp(vec2<i32>(floor(f)), vec2<i32>(0), vec2<i32>(size.zw) - vec2<i32>(1));
}

// One field texel's side in world pixels.
fn ripple_texel_world(camera: vec4<f32>, size: vec4<f32>) -> f32 {
    return size.x / max(size.z, 1.0) / max(camera.z, 1e-6);
}

// How far `p` is from source `s`'s edge, negative inside it.
fn ripple_distance(s: RippleSource, p: vec2<f32>) -> f32 {
    let e = s.a.zw - s.a.xy;
    let h = clamp(dot(p - s.a.xy, e) / max(dot(e, e), 1e-12), 0.0, 1.0);
    return length(p - s.a.xy - e * h) - s.b.x;
}

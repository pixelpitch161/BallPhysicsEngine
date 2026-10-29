// name: Film
// about: Darkens the corners and adds grain and scanlines, for a filmed or old-TV feel.
// simple vignette = 0.5 [0, 1] How dark the corners get
// simple grain = 0.06 [0, 0.4] Amount of moving grain
// param scanlines = 0 [0, 1] Strength of horizontal TV lines

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    var c = sample(uv);
    let q = (uv - 0.5) * 2.0;
    c *= 1.0 - p.vignette * smoothstep(0.4, 1.5, dot(q, q));
    let video_pixel = floor(uv * fx.resolution / fx.scale);
    let n = hash21(video_pixel + floor(fx.time * 60.0) * vec2<f32>(37.0, 113.0)) - 0.5;
    c = max(c * (1.0 + n * p.grain * 2.0) + n * p.grain * 0.02, vec3<f32>(0.0));
    let line = 0.5 + 0.5 * cos(uv.y * fx.resolution.y / fx.scale * PI * 0.5);
    return c * (1.0 - p.scanlines * 0.6 * line);
}

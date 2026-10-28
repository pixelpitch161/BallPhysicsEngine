// name: Pixels
// about: Turns the picture into big pixels or glowing LED dots.
// param size = 12 [2, 64] int Size of each pixel, in video pixels
// param dots = 0 toggle Draw round LED dots instead of squares
// param gap = 0.15 [0, 0.6] Dark gap between pixels

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    let cell = max(round(p.size), 1.0) * fx.scale;
    let pixel = uv * fx.resolution;
    let center = (floor(pixel / cell) + 0.5) * cell;
    let c = sample(center / fx.resolution);
    let local = (pixel - center) / cell;
    var inside: f32;
    if p.dots > 0.5 {
        let r = 0.5 - p.gap * 0.5;
        inside = 1.0 - smoothstep(r - 0.08, r, length(local));
    } else {
        let e = 0.5 - p.gap * 0.5;
        let d = max(abs(local.x), abs(local.y));
        inside = 1.0 - smoothstep(e - 1.0 / cell, e, d);
    }
    return c * inside;
}

// name: Liquid light
// about: The picture flows and folds like oil in a lava lamp, with a rainbow film on it. On the light alone, the light pools and drifts while the balls stay as they are.
// simple amount = 0.06 [0, 0.3] How far the picture flows
// simple scale = 3 [0.5, 12] How big the blobs are: higher makes smaller ones
// simple speed = 0.2 [0, 2] How fast it flows
// simple oil = 0.5 [0, 1] How much rainbow film shows on it
// param pulse = 0 [0, 1] Pulse: how much each hit stirs it

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    let q = to_centered(uv);
    let t = fx.time * p.speed;
    let s = p.scale;
    // Two rounds of warping, so the flow folds over itself like oil.
    let a = vec2<f32>(fbm(q * s + vec2<f32>(t, 0.0)), fbm(q * s + vec2<f32>(5.2, -t))) - 0.5;
    let b = vec2<f32>(
        fbm(q * s + a * 2.0 + vec2<f32>(1.7, 9.2) + t * 0.5),
        fbm(q * s + a * 2.0 + vec2<f32>(8.3, 2.8) - t * 0.5),
    ) - 0.5;
    let w = q + b * p.amount * 2.0 * (1.0 + p.pulse * bass() * 3.0);
    let c = sample(from_centered(w));
    let film = 0.5 + 0.5 * cos(TAU * (vec3<f32>(fbm(w * s * 1.3 + vec2<f32>(t * 0.3))) + vec3<f32>(0.0, 0.33, 0.67)));
    return c * mix(vec3<f32>(1.0), film * 1.6, p.oil);
}

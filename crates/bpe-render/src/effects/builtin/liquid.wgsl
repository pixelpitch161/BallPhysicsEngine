// name: Liquid
// about: Bends the picture with slowly flowing swirls, as if seen through moving water.
// param amount = 0.03 [0, 0.15] How far the picture bends
// param scale = 3 [0.5, 12] Swirl count; higher makes smaller swirls
// param speed = 0.3 [0, 3] How fast the swirls flow

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    let q = to_centered(uv);
    let t = fx.time * p.speed;
    let n = vec2<f32>(fbm(q * p.scale + vec2<f32>(t, 0.0)), fbm(q * p.scale + vec2<f32>(5.2, 1.3 - t))) - 0.5;
    return sample(from_centered(q + n * p.amount * 2.0));
}

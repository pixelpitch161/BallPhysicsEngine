// name: Shockwaves
// about: Every hit sends a ripple through the picture from where the ball hit.
// simple strength = 0.6 [0, 2] How much the ripples bend the picture
// simple speed = 0.5 [0.05, 2] How fast rings spread, in screen heights per second
// simple glow = 0.4 [0, 2] How much rings light up in the ball's colour
// param width = 0.02 [0.005, 0.2] Thickness of each ring
// param life = 0.6 [0.1, 3] Seconds before a ring fades out
// param pulse = 0 [0, 1] Pulse: how much each hit strengthens the ripples

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    let q = to_centered(uv);
    let boost = 1.0 + p.pulse * bass() * 2.0;
    var offset = vec2<f32>(0.0);
    var light = vec3<f32>(0.0);
    for (var i = 0u; i < fx.hit_count; i++) {
        let age = hit_age(i);
        let d = q - to_centered(hit_uv(i));
        let dist = length(d);
        let x = (dist - age * p.speed) / p.width;
        let fade = hit_strength(i) * max(1.0 - age / p.life, 0.0);
        let ring = exp(-x * x) * fade;
        offset += d / max(dist, 1e-4) * x * ring * p.width * p.strength * boost;
        light += hit_color(i) * ring;
    }
    return sample(from_centered(q - offset)) + light * p.glow * 0.25 * boost;
}

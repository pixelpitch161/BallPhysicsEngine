// name: Light ripples
// about: Every hit sends rings of light racing out. Where rings from different hits cross they interfere into moiré patterns, and they bend the picture as they pass.
// simple speed = 0.35 [0.05, 2] How fast rings spread, in screen heights per second
// simple rings = 90 [5, 300] How close together the ripples in a ring are
// simple glow = 1 [0, 4] How bright the rings are
// simple bend = 0.3 [0, 2] How much the rings bend the picture
// param fade = 1.1 [0, 5] How fast rings fade, per second
// param width = 5 [0.5, 30] How narrow each ring's band of ripples is
// param interfere = 0.6 [0, 3] How brightly crossings glow
// color tint = #99ccff The colour of the crossings

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    let q = to_centered(uv);
    var sum = 0.0;
    var light = vec3<f32>(0.0);
    var offset = vec2<f32>(0.0);
    for (var i = 0u; i < fx.hit_count; i++) {
        let age = hit_age(i);
        let d = q - to_centered(hit_uv(i));
        let dist = length(d);
        let front = age * p.speed;
        let envelope = exp(-age * p.fade) * exp(-abs(dist - front) * p.width) * hit_strength(i);
        let wave = sin((dist - front) * p.rings) * envelope;
        sum += wave;
        light += hit_color(i) * max(wave, 0.0);
        offset += d / max(dist, 1e-4) * wave * 0.01 * p.bend;
    }
    let c = sample(from_centered(q - offset));
    return c + (light * 0.9 + p.tint * pow(abs(sum), 3.0) * p.interfere) * p.glow;
}

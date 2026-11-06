// name: Op-art lens
// about: Black and white stripes bend round every ball as if each one were a lens, and hits splash colour into them.
// simple stripes = 70 [5, 300] Stripes per screen height
// simple bend = 2.5 [-10, 10] How strongly the balls bend the stripes (below 0 they push them away)
// simple speed = 0.5 [-3, 3] How fast the stripes wave
// simple keep_things = 1 [0, 1] How much bright things stay on top of the stripes
// param wave = 6 [0, 30] How much the stripes wave
// param splash = 1 [0, 3] How much colour each hit splashes in
// color light = #ffffff The light stripes
// color dark = #000000 The dark stripes

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    let q = to_centered(uv);
    var w = q;
    for (var i = 0u; i < ball_count(); i++) {
        let d = q - to_centered(ball_uv(i));
        let r = ball_radius(i);
        w += d * r * r * p.bend / (dot(d, d) + r * r * 0.8 + 1e-6);
    }
    let s = sin(w.y * p.stripes + sin(w.x * p.wave + fx.time * p.speed) * 2.0);
    var stripe = mix(p.dark, p.light, smoothstep(-0.15, 0.15, s));
    var tint = vec3<f32>(0.0);
    for (var i = 0u; i < fx.hit_count; i++) {
        let age = hit_age(i);
        let dist = length(q - to_centered(hit_uv(i)));
        tint += hit_color(i) * exp(-age * 1.5) * smoothstep(0.05 + age * 0.2, 0.0, dist) * hit_strength(i) * p.splash;
    }
    stripe = stripe * (1.0 - min(tint * 1.2, vec3<f32>(0.9))) + tint * stripe * 1.5;
    let c = sample(uv);
    return mix(stripe, c, clamp(luma(c) * 4.0, 0.0, 1.0) * p.keep_things);
}

// name: Melting picture
// about: The picture drips and smears downward like wet paint, with what moves painting fresh colour into it.
// simple drip = 0.003 [0, 0.02] How far the paint runs down each 1/60 s, as a share of the screen height
// simple wobble = 0.004 [0, 0.02] How much it smears sideways
// simple stay = 0.99 [0, 1] hard [0, 1e9] How much of the melted paint stays each 1/60 s (a keep below 0 means nothing)
// simple paint = 0.25 [0, 1] How strongly the picture paints into the melt
// param streaks = 30 [1, 200] How many drips run across the screen
// param fresh = 1 [0, 1] How much bright things show crisp on top
// memory: fade stay

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    let steps = fx.dt * 60.0;
    let n = noise(vec2<f32>(uv.x * p.streaks, fx.time * 0.5));
    let side = (noise(vec2<f32>(uv.y * 20.0, fx.time)) - 0.5) * p.wobble;
    let off = vec2<f32>(side / aspect(), p.drip * (1.0 + n * 2.0)) * steps;
    let melted = prev(uv - off) * pow(max(p.stay, 1e-4), steps);
    let now = sample(uv);
    let mixed = max(melted, now * p.paint);
    return mix(mixed, now, clamp(luma(now) * 4.0, 0.0, 1.0) * p.fresh);
}

// name: Lens streaks
// about: Every bright thing throws a long thin streak, like an anamorphic film lens, brighter when balls hit.
// lens: rays
// simple strength = 1 [0, 8] How bright the streaks are
// simple threshold = 2 [0, 8] How bright a spot has to be to streak (1 is plain white, lamps and glows go past it)
// simple ray_length = 0.35 [0, 1] How long the streaks are, of the picture's height
// simple kick = 1 [0, 5] How much brighter the streaks are when balls hit
// simple tint_amount = 0.6 [0, 1] How much the streaks take the tint rather than their light's own colour
// param rays = 2 [1, 8] hard [0, 64] int How many ways each streak runs: 2 is a line through the light, more makes a star
// param ray_turn = 0 [-180, 180] Which way the streaks run, in degrees (0 across, 90 up and down)
// param core = 0.5 [0, 2] How much the streaks keep to the hottest middle of each light
// color tint = #6f9dff The streaks' colour

fn streak_knee(c: vec3<f32>, threshold: f32) -> vec3<f32> {
    let b = max(c.r, max(c.g, c.b));
    let knee = max(threshold * 0.5, 1e-3);
    let q = clamp(b - threshold + knee, 0.0, 2.0 * knee);
    let soft = q * q / (4.0 * knee);
    return c * max(soft, b - threshold) / max(b, 1e-4);
}

fn bright(uv: vec2<f32>, p: Params) -> vec3<f32> {
    let c = sample(uv);
    let b = max(c.r, max(c.g, c.b));
    // The hottest middle of a light counts more, so a big glowing ball still throws a thin line.
    let hot = pow(clamp(b / max(2.0 * p.threshold, 0.25), 0.0, 1.0), max(p.core, 0.0) * 2.0);
    return streak_knee(c, p.threshold) * hot;
}

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    let streak = lens_rays(uv);
    let tinted = mix(streak, vec3<f32>(luma(streak)) * p.tint * 1.5, clamp(p.tint_amount, 0.0, 1.0));
    return sample(uv) + tinted * p.strength * (1.0 + p.kick * fx.pulse) * 4.0;
}

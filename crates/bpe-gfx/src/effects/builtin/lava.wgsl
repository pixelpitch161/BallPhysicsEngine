// name: Lava
// about: Slow molten rock flowing over the thing, in three colours.
// stage: things
// simple speed = 0.35 [0, 2] How fast the lava flows
// simple scale = 3 [0.5, 12] How many swirls fit across the thing
// simple heat = 1.6 [0, 4] How bright it glows
// color dark = #140000 The cooled crust
// color middle = #e61f00 The flowing rock
// color bright = #ffbf26 The hottest spots

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    let o = object_at(uv);
    if o.w <= 0.0 {
        return sample(uv);
    }
    let seed = fract(o.z) * 2.0;
    let at = o.xy * p.scale + seed * 7.0 + vec2<f32>(fx.time * p.speed, fx.time * p.speed * 0.6);
    let t = fbm(at);
    let ramp = mix(mix(p.dark, p.middle, clamp(t * 2.0, 0.0, 1.0)), p.bright, clamp(t * 2.0 - 1.0, 0.0, 1.0));
    return ramp * p.heat;
}

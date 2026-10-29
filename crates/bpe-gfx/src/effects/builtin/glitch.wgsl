// name: Glitch
// about: Tears the picture into shifted bands and split colours, set off by hits.
// simple amount = 0.5 [0, 1] How often and how far bands jump
// simple bands = 24 [4, 120] int Number of horizontal bands
// param react = 1 [0, 1] 1 only glitches on hits, 0 glitches all the time
// param rate = 12 [1, 60] Times per second the glitch pattern changes
// param pulse = 0 [0, 1] Pulse: how much each hit sets off glitches

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    let band = floor(uv.y * round(p.bands));
    let tick = floor(fx.time * p.rate);
    let level = p.amount * (mix(1.0, fx.pulse, p.react) + p.pulse * bass());
    let on = step(1.0 - level * 0.7, hash21(vec2<f32>(band * 0.37, tick)));
    let shift = (hash21(vec2<f32>(band, tick + 7.0)) - 0.5) * 0.2 * level * on;
    let split = 0.01 * level * on;
    let s = vec2<f32>(uv.x + shift, uv.y);
    return vec3<f32>(sample(s + vec2<f32>(split, 0.0)).r, sample(s).g, sample(s - vec2<f32>(split, 0.0)).b);
}

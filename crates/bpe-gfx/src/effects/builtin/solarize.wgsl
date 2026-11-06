// name: Solarized neon
// about: Folds the light back on itself into flat neon bands with glowing outlines, like a 70s poster.
// simple fold = 3 [0, 8] How many times the brightness folds back
// simple bands = 4 [1, 16] hard [1, 1e9] int How many flat colour steps (under one divides by zero)
// simple edges = 1.5 [0, 4] How brightly the neon outlines glow
// param speed = 0.3 [-3, 3] How fast the colours cycle
// param width = 1.5 [0.5, 6] How wide the outlines are, in video pixels
// param exposure = 1.5 [0.1, 6] How bright the picture is before it folds

fn solar_tone(uv: vec2<f32>, exposure: f32) -> vec3<f32> {
    let c = max(sample(uv), vec3<f32>(0.0)) * exposure;
    return c / (vec3<f32>(1.0) + c);
}

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    let tone = solar_tone(uv, p.exposure);
    // The colour offsets and the cycling grow in from black, so the dark stays dark.
    let shift = (vec3<f32>(0.0, 1.0, 2.0) + fx.time * p.speed) * smoothstep(vec3<f32>(0.0), vec3<f32>(0.1), tone);
    var s = abs(sin(tone * PI * p.fold + shift));
    let n = max(round(p.bands), 1.0);
    s = min(floor(s * n) / n, vec3<f32>(1.0));
    let px = vec2<f32>(fx.scale * p.width) / fx.resolution;
    let gx = luma(solar_tone(uv + vec2<f32>(px.x, 0.0), p.exposure)) - luma(solar_tone(uv - vec2<f32>(px.x, 0.0), p.exposure));
    let gy = luma(solar_tone(uv + vec2<f32>(0.0, px.y), p.exposure)) - luma(solar_tone(uv - vec2<f32>(0.0, px.y), p.exposure));
    let edge = length(vec2<f32>(gx, gy));
    let neon = hsv_to_rgb(fract(luma(tone) * 2.0 + fx.time * p.speed * 0.66), 1.0, 1.0);
    return pow(s, vec3<f32>(2.2)) * 0.9 + neon * smoothstep(0.02, 0.2, edge) * p.edges;
}

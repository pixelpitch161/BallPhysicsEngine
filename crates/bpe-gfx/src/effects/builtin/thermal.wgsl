// name: Thermal vision
// about: The picture seen through a heat camera: bright things are hot, the heat they leave fades slowly, and hits flare white-hot.
// simple gain = 1 [0, 4] How hot the picture's light reads
// simple memory = 0.985 [0, 0.999] hard [0, 1] How much heat stays each 1/60 s: higher leaves longer warm trails (more than all of it means nothing)
// simple spread = 1.5 [0, 6] How far heat spreads each frame, in video pixels
// choice palette = iron [iron, rainbow, white_hot] The heat camera's colours
// param hits = 0.5 [0, 1] How hot each hit flares round its ball
// param balls = 0.4 [0, 1] How warm the balls themselves are
// memory: fade memory

// Heat 0 to 1 as the camera shows it, linear light.
fn heat_colour(t_in: f32, palette: f32) -> vec3<f32> {
    let t = clamp(t_in, 0.0, 1.0);
    var c: vec3<f32>;
    if palette < 0.5 {
        c = clamp(vec3<f32>(1.6 * t, 2.2 * t - 0.7, 3.0 * t - 2.0), vec3<f32>(0.0), vec3<f32>(1.0));
        c += vec3<f32>(0.12, 0.0, 0.25) * (1.0 - t) * smoothstep(0.0, 0.3, t);
    } else if palette < 1.5 {
        c = hsv_to_rgb(0.7 * (1.0 - t), 1.0, smoothstep(0.0, 0.15, t));
    } else {
        c = vec3<f32>(t);
    }
    return pow(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(2.2));
}

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    let steps = fx.dt * 60.0;
    let px = vec2<f32>(fx.scale * p.spread) / fx.resolution;
    var h = kept(uv) + kept(uv + vec2<f32>(px.x, 0.0)) + kept(uv - vec2<f32>(px.x, 0.0));
    h = (h + kept(uv + vec2<f32>(0.0, px.y)) + kept(uv - vec2<f32>(0.0, px.y))) / 5.0;
    h *= pow(clamp(p.memory, 1e-6, 1.0), steps);
    h = max(h, luma(sample(uv)) * p.gain);
    let q = to_centered(uv);
    for (var i = 0u; i < ball_count(); i++) {
        let d = length(q - to_centered(ball_uv(i))) / max(ball_radius(i), 1e-4);
        let flare = p.hits * exp(-ball_hit(i) * 3.0);
        h = max(h, smoothstep(1.3, 0.6, d) * (p.balls + flare));
    }
    keep(h);
    return heat_colour(h, p.palette);
}

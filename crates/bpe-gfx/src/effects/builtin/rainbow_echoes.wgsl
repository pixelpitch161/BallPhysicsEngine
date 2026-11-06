// name: Rainbow echoes
// about: Everything leaves ghost copies that drift through the rainbow as they fade.
// simple fade = 0.955 [0, 0.995] hard [0, 1e9] How long the ghosts last (a keep below 0 means nothing)
// simple hue = 0.06 [-0.5, 0.5] How far round the colour wheel the ghosts turn each 1/60 s
// simple drift_y = -0.002 [-0.02, 0.02] How far the ghosts float down each 1/60 s (below 0 they rise), in screen heights
// param drift_x = 0 [-0.02, 0.02] How far the ghosts float sideways each 1/60 s, in screen heights
// param saturate = 1 [0, 2] How colourful the ghosts are
// memory: fade fade

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    let steps = fx.dt * 60.0;
    let src_uv = uv - vec2<f32>(p.drift_x / aspect(), p.drift_y) * steps;
    var ghost = hue_shift(prev(src_uv), p.hue * steps) * pow(max(p.fade, 1e-4), steps);
    ghost = max(mix(vec3<f32>(luma(ghost)), ghost, p.saturate), vec3<f32>(0.0));
    return max(sample(uv), ghost);
}

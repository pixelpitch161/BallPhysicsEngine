// name: Echo
// about: Each frame leaves a fading ghost that zooms and turns, for light tunnels and smears.
// param fade = 0.9 [0, 0.99] How long the ghosts last
// param zoom = 1 [0.9, 1.1] Ghost size change each frame. Above 1 flies into the screen, below 1 falls away
// param spin = 0 [-5, 5] Ghost turn each frame, in degrees
// param hue = 0.004 [-0.05, 0.05] Colour change each frame, in turns of the colour wheel

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    // The sliders are per 1/60 s, so the look is the same at any frame rate.
    let steps = fx.dt * 60.0;
    let zoom = pow(max(p.zoom, 0.01), steps);
    let q = rotate(to_centered(uv), -radians(p.spin) * steps) / zoom;
    let ghost = hue_shift(prev(from_centered(q)), p.hue * steps) * pow(p.fade, steps);
    return max(sample(uv), ghost);
}

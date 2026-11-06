// name: Feedback tunnel
// about: Each frame leaves a fading ghost that zooms, turns, drifts and shifts colour round a centre, for endless light tunnels and smears (the old Echo, grown).
// simple fade = 0.9 [0, 0.99] hard [0, 1e9] How long the ghosts last (a keep below 0 means nothing)
// simple zoom = 1 [0.9, 1.1] hard [0.01, 1e9] Ghost size change each frame. Above 1 flies into the screen, below 1 falls away
// simple spin = 0 [-5, 5] Ghost turn each frame, in degrees
// param hue = 0.004 [-0.05, 0.05] Colour change each frame, in turns of the colour wheel
// param pulse = 0 [0, 1] Pulse: how much each hit pushes the ghosts into the screen
// param drift_x = 0 [-0.01, 0.01] How far the ghosts slide sideways each frame, as a share of the screen height
// param drift_y = 0 [-0.01, 0.01] How far the ghosts slide down each frame, as a share of the screen height
// param centre_x = 0 [-0.5, 0.5] Where the ghosts zoom and turn round, right of the middle, in screen heights
// param centre_y = 0 [-0.5, 0.5] Where the ghosts zoom and turn round, below the middle, in screen heights
// memory: fade fade

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    // The sliders are per 1/60 s, so the look is the same at any frame rate.
    let steps = fx.dt * 60.0;
    let zoom = pow(max(p.zoom + p.pulse * bass() * 0.03, 0.01), steps);
    let centre = vec2<f32>(p.centre_x, p.centre_y);
    let drift = vec2<f32>(p.drift_x, p.drift_y) * steps;
    let q = rotate(to_centered(uv) - centre, -radians(p.spin) * steps) / zoom + centre - drift;
    let ghost = hue_shift(prev(from_centered(q)), p.hue * steps) * pow(max(p.fade, 1e-4), steps);
    return max(sample(uv), ghost);
}

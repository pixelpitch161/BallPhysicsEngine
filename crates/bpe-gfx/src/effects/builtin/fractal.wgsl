// name: Fractal light
// about: A glowing fractal whose shape follows one ball, so every bounce reshapes it.
// simple ball = 1 [1, 20] hard [1, 1e9] int Which ball steers it: its place in the frame's list, 1 the first
// simple zoom = 3.4 [0.5, 20] How far in the fractal is seen
// simple glow = 1.3 [0, 4] How brightly it shines
// simple steps = 64 [8, 256] hard [1, 1024] int How detailed it is: more is slower (past 1024 a frame could stall the graphics card)
// param speed = 0.05 [-1, 1] How fast its colours turn, in turns per second
// param bands = 0.03 [0, 0.3] How quickly its colours change across it
// param steer = 1.8 [0, 5] How much the ball's place changes it
// param size = 0 [0, 1] How far it reaches from the middle, in screen heights (0 everywhere)
// param over = 0 toggle Paint over the picture instead of adding its light

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    let q = to_centered(uv);
    var c = vec2<f32>(-0.4, 0.0);
    if ball_count() > 0u {
        let i = min(u32(max(round(p.ball), 1.0)) - 1u, ball_count() - 1u);
        c += to_centered(ball_uv(i)) * p.steer;
    }
    var z = q * p.zoom;
    var it = 0.0;
    let steps = u32(clamp(round(p.steps), 1.0, 1024.0));
    for (var n = 0u; n < steps; n++) {
        z = vec2<f32>(z.x * z.x - z.y * z.y, 2.0 * z.x * z.y) + c;
        if dot(z, z) > 16.0 {
            break;
        }
        it += 1.0;
    }
    let smooth_it = it - log2(log2(max(dot(z, z), 1.0001))) + 4.0;
    var col = hsv_to_rgb(fract(smooth_it * p.bands + fx.time * p.speed), 0.9, 1.0) * smoothstep(0.0, 12.0, smooth_it);
    col = select(vec3<f32>(0.0), col * p.glow, it < f32(steps));
    if p.size > 0.0 {
        col *= 1.0 - smoothstep(p.size * 0.98, p.size, length(q));
    }
    let base = sample(uv);
    if p.over > 0.5 {
        return mix(base, col, clamp(luma(col), 0.0, 1.0));
    }
    return base + col;
}

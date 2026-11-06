// name: Ball worlds
// about: Every ball becomes a window into the whole picture, and the balls inside it hold the picture again, several worlds deep.
// simple levels = 3 [1, 6] hard [0, 16] int How many worlds deep (each is another pass over the balls)
// simple zoom = 1.05 [0.5, 2] How much of the picture each ball shows: higher shows more
// simple spin = 0.2 [-2, 2] How fast the worlds turn, in turns per second, faster each level
// param tint = 0.5 [0, 1] How much each world takes its ball's colour
// param rim = 1.5 [0, 4] How brightly each window's edge glows
// param edge = 0.15 [0.01, 0.5] How wide the glowing edge is, as a share of the ball

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    var at = uv;
    var tint = vec3<f32>(1.0);
    var glow = vec3<f32>(0.0);
    let levels = u32(clamp(round(p.levels), 0.0, 16.0));
    for (var level = 0u; level < levels; level++) {
        let q = to_centered(at);
        var found = -1;
        var inside = 0.0;
        for (var i = 0u; i < ball_count(); i++) {
            let r = ball_radius(i);
            let d = length(q - to_centered(ball_uv(i)));
            if r > 0.0 && d < r {
                found = i32(i);
                inside = d / r;
                break;
            }
        }
        if found < 0 {
            break;
        }
        let i = u32(found);
        glow += tint * ball_color(i) * smoothstep(1.0 - p.edge, 1.0, inside) * p.rim;
        tint *= mix(vec3<f32>(1.0), ball_color(i), p.tint) * 0.95;
        // Across the ball, -1 to 1, turned: the ball shows the picture's middle.
        let local = (q - to_centered(ball_uv(i))) / ball_radius(i);
        let turned = rotate(local, fx.time * p.spin * TAU * f32(level + 1u));
        at = from_centered(turned * 0.5 * p.zoom);
    }
    return sample(at) * tint + glow;
}

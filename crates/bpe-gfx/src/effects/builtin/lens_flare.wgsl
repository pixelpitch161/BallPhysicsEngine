// name: Lens flare
// about: Bright lights throw ghosts, a halo ring and a starburst across the picture, moving as the lights move.
// lens: rays soft
// simple strength = 1 [0, 4] How bright the flare is
// simple threshold = 1 [0, 4] How bright a spot has to be to flare (1 is plain white)
// simple ghosts = 5 [0, 12] hard [0, 64] int How many ghosts the lens throws through the middle
// simple halo = 0.4 [0, 2] How bright the ring round each light is
// simple star = 0.6 [0, 2] How bright the starburst round each light is
// param spacing = 0.42 [0, 1.5] How far apart the ghosts are along the line through the middle
// param softness = 2 [0, 4] How soft the ghosts and the halo are
// param rainbow = 0.7 [0, 1] How much the ghosts and the halo take rainbow colours
// param halo_size = 0.22 [0, 1] How far the halo ring sits from each light, of the picture's height
// param rays = 6 [0, 12] hard [0, 64] int How many points the starburst has
// param ray_turn = 15 [-180, 180] How far the starburst is turned, in degrees
// param ray_length = 0.05 [0, 0.3] How long the starburst's points are, of the picture's height
// param fringe = 0.4 [0, 1] How much the ghosts and the halo split red from blue
// color tint = #ffe8d0 The flare's colour

// What counts as a light: the picture above the threshold, eased in over a soft knee.
fn flare_knee(c: vec3<f32>, threshold: f32) -> vec3<f32> {
    let b = max(c.r, max(c.g, c.b));
    let knee = max(threshold * 0.5, 1e-3);
    let q = clamp(b - threshold + knee, 0.0, 2.0 * knee);
    let soft = q * q / (4.0 * knee);
    return c * max(soft, b - threshold) / max(b, 1e-4);
}

fn bright(uv: vec2<f32>, p: Params) -> vec3<f32> {
    return flare_knee(sample(uv), p.threshold);
}

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    let base = sample(uv);
    var flare = vec3<f32>(0.0);
    // Ghosts: the bright parts scaled through the middle, the first on the light's own side, the
    // later ones across the middle, each a softened copy in a colour of its own.
    let d = uv - 0.5;
    let split = 1.0 + 0.08 * p.fringe;
    let n = i32(clamp(p.ghosts, 0.0, 64.0));
    for (var k = 0; k < n; k++) {
        let m = 0.5 - p.spacing * f32(k);
        if abs(m) < 0.04 {
            continue;
        }
        let g = vec3<f32>(
            lens_soft(0.5 + d / (m * split), p.softness).r,
            lens_soft(0.5 + d / m, p.softness).g,
            lens_soft(0.5 + d / (m / split), p.softness).b,
        );
        let at_ghost = 0.5 + d / m;
        let fade = 1.0 - smoothstep(0.25, 0.8, length(to_centered(at_ghost)));
        let colour = mix(vec3<f32>(1.0), hsv_to_rgb(fract(f32(k) * 0.17 + 0.55), 0.75, 1.0), p.rainbow);
        flare += g * colour * fade * 0.35;
    }
    // The halo: a ring round every bright spot, red outside and blue inside as far as Rainbow.
    if p.halo != 0.0 {
        var ring = vec3<f32>(0.0);
        let spread = 1.0 + 0.06 * max(p.rainbow, p.fringe);
        for (var i = 0; i < 24; i++) {
            let a = (f32(i) + 0.5) * TAU / 24.0;
            let way = vec2<f32>(cos(a), sin(a)) / vec2<f32>(aspect(), 1.0);
            let level = p.softness + 0.5;
            ring += vec3<f32>(
                lens_soft(uv + way * p.halo_size * spread, level).r,
                lens_soft(uv + way * p.halo_size, level).g,
                lens_soft(uv + way * p.halo_size / spread, level).b,
            );
        }
        flare += ring * (p.halo / 24.0) * 6.0;
    }
    // The starburst round each light.
    flare += lens_rays(uv) * p.star * 4.0;
    return base + flare * p.tint * p.strength;
}

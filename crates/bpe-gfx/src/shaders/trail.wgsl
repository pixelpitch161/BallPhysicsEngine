// Motion streaks: a triangle list built on the CPU (see push_streak), drawn behind everything
// else in the scene. Appended to scene.wgsl's module.

struct TrailIn {
    @location(0) pos: vec2<f32>,
    // x: signed distance from the streak's centre line, y: half width at this point
    @location(1) edge: vec2<f32>,
    @location(2) color: vec4<f32>,
};

struct TrailOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) edge: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_trail(v: TrailIn) -> TrailOut {
    var out: TrailOut;
    out.pos = to_clip(v.pos);
    out.edge = v.edge;
    out.color = v.color;
    return out;
}

// What `fs_trail` and `fs_trail_each` draw.
fn shade_trail(in: TrailOut) -> vec4<f32> {
    let px = g.view.z;
    let half_width = max(in.edge.y, 0.5 * px);
    // Streaks thinner than a pixel get dimmer instead of aliasing.
    let thin = min(in.edge.y / (0.5 * px), 1.0);
    let cover = clamp((half_width - abs(in.edge.x)) / px + 0.5, 0.0, 1.0) * thin;
    if cover <= 0.0 {
        discard;
    }
    if in.color.a < 0.0 {
        // Additive: the light itself was faded along the streak.
        return vec4<f32>(in.color.rgb * cover, 0.0);
    }
    let a = in.color.a * cover;
    return vec4<f32>(in.color.rgb * a, a);
}

@fragment
fn fs_trail(in: TrailOut) -> @location(0) vec4<f32> {
    return shade_trail(in);
}

// In a layer whose things blend with each other (see each.wgsl).
@fragment
fn fs_trail_each(in: TrailOut) -> @location(0) vec4<f32> {
    return each_out(shade_trail(in), g.post.w);
}

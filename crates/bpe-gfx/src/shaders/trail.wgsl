// Motion streaks: a triangle list built on the CPU (see push_streak), drawn behind everything
// else in the scene. Appended to scene.wgsl's module.

struct TrailIn {
    @location(0) pos: vec2<f32>,
    // x: signed distance from the streak's centre line, y: half width at this point
    @location(1) edge: vec2<f32>,
    @location(2) color: vec4<f32>,
    // x: softness (0 crisp, 1 soft from the centre line out), y: band (0 filled, else only the
    // outer share of the half width shows)
    @location(3) style: vec2<f32>,
    // Its layer's Lit and how much it blocks light, read in the light passes.
    @location(4) light: vec2<f32>,
    // The light it gives off (straight, linear), read in the emission pass.
    @location(5) emit: vec4<f32>,
    // UV glow, only under UV, catches beams, link number (lighting part 4).
    @location(6) extra: vec4<f32>,
};

struct TrailOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) edge: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) style: vec2<f32>,
    @location(3) @interpolate(flat) light: vec2<f32>,
    @location(4) emit: vec3<f32>,
    @location(5) @interpolate(flat) extra: vec4<f32>,
};

@vertex
fn vs_trail(v: TrailIn) -> TrailOut {
    var out: TrailOut;
    out.pos = to_clip(v.pos);
    out.edge = v.edge;
    out.color = v.color;
    out.style = v.style;
    out.light = v.light;
    out.emit = v.emit.rgb;
    out.extra = v.extra;
    return out;
}

// What `fs_trail` and `fs_trail_each` draw.
fn shade_trail(in: TrailOut) -> vec4<f32> {
    let px = g.view.z;
    let half_width = max(in.edge.y, 0.5 * px);
    // Streaks thinner than a pixel get dimmer instead of aliasing.
    let thin = min(in.edge.y / (0.5 * px), 1.0);
    var cover = clamp((half_width - abs(in.edge.x)) / px + 0.5, 0.0, 1.0) * thin;
    // The crisp edge's antialiasing (the surface pass takes a thin solid streak whole): a soft
    // streak's softness is its own, as a see-through one's would be.
    var aa = cover;
    let soft = in.style.x;
    if soft > 0.0 {
        // Soft from `soft` of the width in from the edge to as far past it.
        let w = max(px, soft * half_width * 2.0);
        cover = smoothstep(0.0, 1.0, 0.5 - (abs(in.edge.x) - half_width) / w);
        aa = -1.0;
    }
    if in.style.y > 0.0 {
        // Only the outer band: an outline along the edges.
        let band = clamp(0.5 + (abs(in.edge.x) - half_width * (1.0 - in.style.y)) / px, 0.0, 1.0);
        cover = cover * band;
        aa = select(aa * band, -1.0, aa < 0.0);
    }
    AA_COVER = aa;
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
    return light_flat(shade_trail(in), in.light.x, in.light.y, in.emit, in.extra);
}

// In a layer whose things blend with each other (see each.wgsl).
@fragment
fn fs_trail_each(in: TrailOut) -> @location(0) vec4<f32> {
    return each_out(shade_trail(in), g.post.w);
}

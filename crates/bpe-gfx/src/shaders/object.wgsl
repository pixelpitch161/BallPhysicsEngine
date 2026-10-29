// The object picture: what is where, for shader effects on things. Appended to scene.wgsl's
// module, so it shares the vertex shaders and the distance functions. Each pixel gets the
// thing's own position across it, the thing's code (see `thing_code` in frame.rs) and how much
// of the pixel the thing covers; nothing is all zero.

// A wall's own position: -1 to 1 across its size, turning with it.
@fragment
fn fs_shape_object(in: ShapeOut) -> @location(0) vec4<f32> {
    let px = g.view.z;
    let d = shape_distance(in);
    let half_width = max(in.c.y, 0.5 * px);
    let cover = clamp((half_width - d) / px + 0.5, 0.0, 1.0) * min(in.c.y * 2.0 / px, 1.0);
    if cover <= 0.0 || in.c.w < 0.5 {
        discard;
    }
    var size = vec2<f32>(in.b.x);
    if in.c.x > 0.5 {
        size = in.b.xy;
    }
    let local = rotate(in.world - in.a.xy, -in.a.z) / max(size, vec2<f32>(1e-3));
    return vec4<f32>(local, in.c.w, cover);
}

// A ball's own position: -1 to 1 across it.
@fragment
fn fs_ball_object(in: BallOut) -> @location(0) vec4<f32> {
    let aa = 1.0 / max(in.radius_px, 0.5);
    let cover = clamp((1.0 - length(in.local)) / aa + 0.5, 0.0, 1.0);
    if cover <= 0.0 || in.code.x < 0.5 {
        discard;
    }
    return vec4<f32>(in.local, in.code.x, cover);
}

// ---- Soft bodies: jelly balls, cloth and liquid (filled triangles, painted over trails) ----
// Appended to scene.wgsl, so it shares its globals and `vs_trail`. Each vertex's `edge.x` is
// its distance inside the shape's edge, in world units, which gives a smooth outline.

@fragment
fn fs_soft(in: TrailOut) -> @location(0) vec4<f32> {
    let px = g.view.z;
    let cover = clamp(in.edge.x / px + 0.5, 0.0, 1.0);
    let a = in.color.a * cover;
    return vec4<f32>(in.color.rgb * a, a);
}

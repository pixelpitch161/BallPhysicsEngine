// Things masks' silhouettes (colour grading part 5): shapes, text and balls as their bodies
// cover, as their object pictures do (fs_path_object, fs_layers_object), chosen or not.

@fragment
fn fs_path_silhouette(in: PathOut) -> @location(0) vec4<f32> {
    var cover = path_cover(in);
    if clips(in.clip) {
        cover *= clip_keep(path_local(in), in.clip, g.view.z / max(in.a.w, 1e-6));
    }
    // A Colour split's red and blue copies aren't the thing (Text part 5).
    let channel = u32(in.clip.w + 0.5) % 4u;
    if in.clip.w > 0.5 && (channel == 1u || channel == 3u) {
        discard;
    }
    return sil_out(u32(max(in.c.x, 0.0)), cover);
}

@fragment
fn fs_layers_silhouette(in: LayeredOut) -> @location(0) vec4<f32> {
    let p = rotate(in.local, -in.form.z);
    let aa = 1.0 / max(in.radius.x, 0.5);
    var cover = clamp(-ball_shape(p, in.form.x, in.form.y) / aa + 0.5, 0.0, 1.0) * step(1e-4, in.misc.x);
    if clips(in.clip) {
        cover *= clip_keep(in.local, in.clip, aa);
    }
    return sil_out(u32(max(in.misc.y, 0.0)), cover);
}

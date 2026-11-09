// Things masks' silhouettes (colour grading part 5): a Particles thing's own sprites (and its
// Huge swarm's particles) by their shape's coverage, added light or not.

@fragment
fn fs_sprite_silhouette(in: SpriteOut) -> @location(0) vec4<f32> {
    return sil_out(in.code, coverage(in) * min(abs(in.color.a), 1.0));
}

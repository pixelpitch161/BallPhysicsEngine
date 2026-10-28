// name: Mirror
// about: Copies one half of the picture onto the other for perfect symmetry.
// param left_right = 1 toggle Mirror the left half onto the right
// param top_bottom = 0 toggle Mirror the top half onto the bottom
// param swap = 0 toggle Use the right or bottom half as the source instead

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    var m = uv;
    if p.swap > 0.5 {
        m = 1.0 - m;
    }
    if p.left_right > 0.5 {
        m.x = 0.5 - abs(m.x - 0.5);
    }
    if p.top_bottom > 0.5 {
        m.y = 0.5 - abs(m.y - 0.5);
    }
    if p.swap > 0.5 {
        m = 1.0 - m;
    }
    return sample(m);
}

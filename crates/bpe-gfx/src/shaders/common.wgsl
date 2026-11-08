// Helpers several shader programs share, the same maths in each. Put after a program's own
// source (WGSL lets a function be called before it is written): the lighting passes
// (`LIGHTING_WGSL`), the post pass, and the effects (`effects::PRELUDE`, `effects::IN_BALL`).

// How bright a linear colour looks (Rec. 709 weights).
fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

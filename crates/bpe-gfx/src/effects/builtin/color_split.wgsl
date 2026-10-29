// name: Colour split
// about: Pulls the red and blue apart like a cheap lens, and kicks harder on every hit.
// simple amount = 0.004 [0, 0.03] Gap between red and blue, as a fraction of the screen
// simple kick = 2 [0, 10] Extra split when balls hit
// param radial = 1 [0, 1] 0 splits sideways, 1 splits outward from the centre

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    let q = to_centered(uv);
    let dir = mix(vec2<f32>(1.0, 0.0), q * 2.0, p.radial);
    let off = dir * p.amount * (1.0 + p.kick * fx.pulse) / vec2<f32>(aspect(), 1.0);
    return vec3<f32>(sample(uv + off).r, sample(uv).g, sample(uv - off).b);
}

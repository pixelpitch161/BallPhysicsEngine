// name: Hit glow
// about: The thing lights up with every hit and fades back.
// stage: things
// simple punch = 3 [0, 10] How much brighter each hit makes it

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    return sample(uv) * (1.0 + fx.pulse * p.punch);
}

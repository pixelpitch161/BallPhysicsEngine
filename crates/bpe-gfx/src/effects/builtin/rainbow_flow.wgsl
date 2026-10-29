// name: Rainbow flow
// about: Rainbow bands flowing over the thing, lit by its own light and shade.
// stage: things
// simple bands = 0.8 [0, 8] How many colour bands fit down the picture
// simple speed = 0.25 [-2, 2] How fast the bands flow, in turns of the colour wheel per second
// simple glow = 1.5 [0, 4] How bright the colours are

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    let hue = fract(to_centered(uv).y * p.bands + fx.time * p.speed);
    return hsv_to_rgb(hue, 1.0, 1.0) * luma(sample(uv)) * p.glow;
}

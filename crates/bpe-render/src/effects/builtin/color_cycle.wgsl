// name: Colour cycle
// about: Turns every colour slowly round the colour wheel, with saturation and contrast controls.
// param speed = 0.05 [-1, 1] Turns of the colour wheel per second
// param kick = 0 [0, 0.5] Extra colour turn when balls bounce
// param saturation = 1 [0, 2] 0 is black and white
// param contrast = 1 [0.5, 2] Above 1 makes darks darker and brights brighter

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    var c = hue_shift(sample(uv), fx.time * p.speed + fx.pulse * p.kick);
    c = max(mix(vec3<f32>(luma(c)), c, p.saturation), vec3<f32>(0.0));
    return 0.18 * pow(c / 0.18, vec3<f32>(p.contrast));
}

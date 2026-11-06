// name: Hue storm
// about: Colour ripples out from every ball in rainbow waves: each spot turns round the colour wheel by how far it is from the nearest ball, running outward with time.
// simple waves = 3 [0, 12] Rainbow waves per screen height
// simple speed = 0.6 [-3, 3] How fast the waves run outward, in turns of the wheel per second
// simple amount = 1 [0, 1] How far round the wheel each wave turns the colours
// simple paint = 0.5 [0, 1] How much rainbow paints over white and grey light, which turning alone leaves as it is
// param saturate = 1.3 [0, 3] How colourful the storm is: 1 leaves it as it is
// param each = 0.15 [0, 1] How much each ball's waves are its own, set apart by its random number
// param from_balls = 1 toggle Waves start at the balls; off, at the middle of the picture

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    let c = sample(uv);
    var d = length(to_centered(uv));
    var offset = 0.0;
    if p.from_balls > 0.5 && ball_count() > 0u {
        let near = nearest_ball(uv);
        d = max(near.y, 0.0);
        offset = ball_seed(u32(near.x)) * p.each;
    }
    let turn = (d * p.waves - fx.time * p.speed + offset) * p.amount;
    let turned = hue_shift(c, turn);
    let rainbow = hsv_to_rgb(fract(turn), 1.0, 1.0) * luma(c) * 1.6;
    let painted = mix(turned, rainbow, p.paint);
    return max(mix(vec3<f32>(luma(painted)), painted, p.saturate), vec3<f32>(0.0));
}

// name: Polar tunnel
// about: Wraps the picture into a tunnel that spirals endlessly towards you.
// simple speed = 0.12 [-1, 1] How fast you fly into the tunnel
// simple twist = 0.25 [-2, 2] How much the tunnel spirals
// simple arms = 2 [1, 8] hard [1, 1e9] int How many times the picture goes round the tunnel (under one means nothing)
// param depth = 0.5 [0.1, 3] How many copies of the picture fit along the tunnel
// param turn = 0.03 [-1, 1] How fast the tunnel turns, in turns per second
// param dark = 1 [0, 1] How dark the far end gets
// param centre_x = 0 [-0.5, 0.5] Where the tunnel's end is, right of the middle, in screen heights
// param centre_y = 0 [-0.5, 0.5] Where the tunnel's end is, below the middle, in screen heights

// x folded back and forth between 0 and 1, so the picture meets itself without a seam.
fn tunnel_fold(x: f32) -> f32 {
    return 1.0 - abs(1.0 - 2.0 * fract(x * 0.5));
}

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    let q = to_centered(uv) - vec2<f32>(p.centre_x, p.centre_y);
    let r = length(q) + 1e-4;
    let a = atan2(q.y, q.x);
    let u = a / PI * max(round(p.arms), 1.0) + log(r) * p.twist + fx.time * p.turn * 2.0;
    let v = log(r) * p.depth * 2.0 - fx.time * p.speed * 2.0;
    let c = sample(vec2<f32>(tunnel_fold(u), tunnel_fold(v)));
    return c * mix(1.0, smoothstep(0.0, 0.4, r), p.dark);
}

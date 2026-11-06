// name: Colour fringes
// about: Red and blue split apart toward the edges like an old lens, as a smooth rainbow, wider on the beat and on hits.
// simple amount = 0.012 [0, 0.06] How far the colours split at the edges, of the picture's height
// simple beat = 1 [0, 5] How much wider the split is on each beat (and when balls hit)
// simple falloff = 2 [0.5, 4] hard [0, 1e9] How the split grows toward the edges: 1 evenly, 2 like a lens
// param samples = 7 [3, 16] hard [1, 64] int How smooth the rainbow is
// param centre_x = 0.5 [0, 1] Where the lens's middle is, across
// param centre_y = 0.5 [0, 1] Where the lens's middle is, down

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    let centre = vec2<f32>(p.centre_x, p.centre_y);
    let q = (uv - centre) * vec2<f32>(aspect(), 1.0);
    let r = length(q);
    if r < 1e-5 {
        return sample(uv);
    }
    let way = q / r;
    let split = p.amount * pow(min(r / 0.5, 16.0), max(p.falloff, 0.0)) * (1.0 + p.beat * kick(6.0));
    let n = max(i32(clamp(p.samples, 1.0, 64.0)), 1);
    var sum = vec3<f32>(0.0);
    var weight = vec3<f32>(0.0);
    for (var i = 0; i < n; i++) {
        let t = (f32(i) + 0.5) / f32(n);
        // Red is pushed outward, blue drawn in, green in between: a smooth spectrum.
        let w = max(cos((vec3<f32>(t) - vec3<f32>(1.0, 0.5, 0.0)) * PI), vec3<f32>(0.0));
        let at = centre + (q + way * (t - 0.5) * 2.0 * split) / vec2<f32>(aspect(), 1.0);
        sum += sample(at) * w;
        weight += w;
    }
    if min(weight.r, min(weight.g, weight.b)) < 1e-4 {
        return sample(uv);
    }
    return sum / weight;
}

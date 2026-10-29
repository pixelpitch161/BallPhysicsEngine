// name: Kaleidoscope
// about: Mirrors the picture into slices around the centre, like looking through a kaleidoscope.
// simple slices = 6 [2, 24] int Number of mirrored slices
// simple spin = 0.03 [-1, 1] Turns per second
// simple zoom = 1 [0.25, 4] Above 1 zooms in
// param angle = 0 [0, 360] Which part of the picture the slices show, in degrees
// param pulse = 0 [0, 1] Pulse: how much each hit zooms the slices in

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    let q = to_centered(uv);
    let r = length(q) / (p.zoom * (1.0 + p.pulse * bass() * 0.5));
    let slice = TAU / max(round(p.slices), 1.0);
    var a = atan2(q.y, q.x) + fx.time * p.spin * TAU;
    a = a - slice * floor(a / slice);
    a = min(a, slice - a);
    a += radians(p.angle);
    return sample(from_centered(vec2<f32>(cos(a), sin(a)) * r));
}

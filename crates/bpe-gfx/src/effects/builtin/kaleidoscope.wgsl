// name: Kaleidoscope
// about: Mirrors the picture into slices around the centre, like looking through a kaleidoscope.
// simple slices = 6 [2, 24] hard [1, 1e9] int Number of mirrored slices (fewer than one means nothing)
// simple spin = 0.03 [-1, 1] Turns per second
// simple zoom = 1 [0.25, 4] Above 1 zooms in
// param angle = 0 [0, 360] Which part of the picture the slices show, in degrees
// param pulse = 0 [0, 1] Pulse: how much each hit zooms the slices in
// choice fold = slices [slices, rings, grid] How it folds: slices round the centre, slices and rings out from it, or a grid of mirrored tiles
// param rings = 3 [1, 20] Rings out from the centre, or tiles across the screen height for the grid
// param centre_x = 0 [-0.5, 0.5] Where the centre is, right of the middle, in screen heights
// param centre_y = 0 [-0.5, 0.5] Where the centre is, below the middle, in screen heights

// A triangle wave: x folded back and forth between 0 and `size`.
fn mirror_fold(x: f32, size: f32) -> f32 {
    let s = max(size, 1e-4);
    return s - abs(x - 2.0 * s * floor(x / (2.0 * s)) - s);
}

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    let centre = vec2<f32>(p.centre_x, p.centre_y);
    let q = to_centered(uv) - centre;
    let zoom = p.zoom * (1.0 + p.pulse * bass() * 0.5);
    if p.fold > 1.5 {
        // A grid: the tile at the centre, mirrored into every other.
        let cell = 1.0 / max(p.rings, 0.01);
        let g = rotate(q, fx.time * p.spin * TAU + radians(p.angle)) / zoom;
        let m = vec2<f32>(mirror_fold(g.x + 0.5 * cell, cell), mirror_fold(g.y + 0.5 * cell, cell)) - 0.5 * cell;
        return sample(from_centered(m + centre));
    }
    var r = length(q) / zoom;
    if p.fold > 0.5 {
        // Rings: the distance from the centre folded back and forth too.
        r = mirror_fold(r, 0.5 / max(p.rings, 0.01));
    }
    let slice = TAU / max(round(p.slices), 1.0);
    var a = atan2(q.y, q.x) + fx.time * p.spin * TAU;
    a = a - slice * floor(a / slice);
    a = min(a, slice - a);
    a += radians(p.angle);
    return sample(from_centered(vec2<f32>(cos(a), sin(a)) * r + centre));
}

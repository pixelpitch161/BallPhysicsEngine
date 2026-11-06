// name: Gravity lens
// about: Every ball is a tiny black hole: it bends the picture behind it and wears a ring of light where the picture wraps round.
// simple mass = 1.6 [-5, 5] How strongly each ball bends the picture (below 0 pushes it away)
// simple rings = 0.6 [0, 3] How bright the rings of light are
// simple hole = 1 [0, 1] How dark the balls themselves go
// simple grid = 0 [0, 1] How much of a starry grid shows behind, to see the bending
// param ring_size = 1.3 [1, 3] How far out the ring sits, in ball sizes
// param cells = 12 [1, 60] Grid squares per screen height
// color grid_colour = #2633a0 The grid's colour

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    let q = to_centered(uv);
    var w = q;
    var ring = vec3<f32>(0.0);
    var inside = 0.0;
    for (var i = 0u; i < ball_count(); i++) {
        let d = q - to_centered(ball_uv(i));
        let r = max(ball_radius(i), 1e-5);
        let dd = dot(d, d);
        w -= d * (r * r * p.mass) / (dd + 1e-4);
        let rr = r * p.ring_size;
        let band = smoothstep(r * 0.12, 0.0, abs(sqrt(dd) - rr)) * 1.5;
        let halo = r * r * 0.06 / (abs(dd - rr * rr) + r * r * 0.12);
        ring += ball_color(i) * (band + halo);
        inside = max(inside, smoothstep(r * 0.95, r * 0.8, sqrt(dd)));
    }
    var c = sample(from_centered(w));
    if p.grid > 0.0 {
        let g = abs(fract(w * p.cells) - 0.5);
        let line = smoothstep(0.47, 0.5, max(g.x, g.y));
        let cell = floor(w * 90.0);
        let stars = step(0.985, hash21(cell)) * hash21(cell + 1.0);
        c += (p.grid_colour * line * 0.6 + vec3<f32>(stars)) * p.grid;
    }
    c += ring * 0.6 * p.rings;
    return c * (1.0 - inside * p.hole);
}

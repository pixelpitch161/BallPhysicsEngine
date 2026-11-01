// Shapes of styled balls. Appended to scene.wgsl's module, which defines PI and TAU.

// Shapes are distance functions on a unit-radius ball, y up, tip on top.

// A regular polygon with `n` sides and a corner on top, `r` from the centre to each corner.
fn sd_polygon(p: vec2<f32>, r: f32, n: f32) -> f32 {
    let an = PI / n;
    let acs = vec2<f32>(cos(an), sin(an));
    let a = atan2(p.x, p.y);
    let bn = a - 2.0 * an * floor(a / (2.0 * an)) - an;
    var q = length(p) * vec2<f32>(cos(bn), abs(sin(bn)));
    q -= r * acs;
    q.y += clamp(-q.y, 0.0, r * acs.y);
    return length(q) * sign(q.x);
}

fn sd_box(p: vec2<f32>, half: vec2<f32>) -> f32 {
    let q = abs(p) - half;
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0);
}

fn sd_star(p_in: vec2<f32>, r: f32, inner: f32) -> f32 {
    let k1 = vec2<f32>(0.809016994375, -0.587785252292);
    let k2 = vec2<f32>(-k1.x, k1.y);
    var p = vec2<f32>(abs(p_in.x), p_in.y);
    p -= 2.0 * max(dot(k1, p), 0.0) * k1;
    p -= 2.0 * max(dot(k2, p), 0.0) * k2;
    p.x = abs(p.x);
    p.y -= r;
    let ba = inner * vec2<f32>(-k1.y, k1.x) - vec2<f32>(0.0, 1.0);
    let h = clamp(dot(p, ba) / dot(ba, ba), 0.0, r);
    return length(p - ba * h) * sign(p.y * ba.x - p.x * ba.y);
}

fn sd_heart(p_in: vec2<f32>) -> f32 {
    let p = vec2<f32>(abs(p_in.x), p_in.y);
    if p.y + p.x > 1.0 {
        return length(p - vec2<f32>(0.25, 0.75)) - 0.35355339;
    }
    let a = p - vec2<f32>(0.0, 1.0);
    let b = p - 0.5 * max(p.x + p.y, 0.0);
    return sqrt(min(dot(a, a), dot(b, b))) * sign(p.x - p.y);
}

// Signed distance to the edge of a styled ball's shape, in radii; negative inside. Shapes: 0
// circle, 1 square, 2 triangle, 3 diamond, 4 hexagon, 5 star, 6 heart, 7 donut.
fn ball_shape(p: vec2<f32>, shape: f32, round: f32) -> f32 {
    // Distance functions here are written with y up.
    let q = vec2<f32>(p.x, -p.y);
    let k = round * 0.35;
    if shape < 0.5 {
        return length(q) - 1.0;
    } else if shape < 1.5 {
        return sd_box(q, vec2<f32>(0.84 - k)) - k;
    } else if shape < 2.5 {
        // Moved down a little so the triangle sits in the middle of the ball.
        return sd_polygon(q + vec2<f32>(0.0, 0.18), 1.3 - k * 2.0, 3.0) - k;
    } else if shape < 3.5 {
        // Narrower than tall, and no taller than the ball.
        let s = vec2<f32>(1.4, 1.0);
        return (sd_polygon(q * s, 1.12 - k, 4.0) - k) / 1.2;
    } else if shape < 4.5 {
        return sd_polygon(q, 1.06 - k, 6.0) - k;
    } else if shape < 5.5 {
        return sd_star(q + vec2<f32>(0.0, 0.08), 1.22 - k * 0.5, 0.45) - k * 0.5;
    } else if shape < 6.5 {
        return sd_heart(q / 2.0 + vec2<f32>(0.0, 0.52)) * 2.0;
    }
    return abs(length(q) - 0.72) - 0.28;
}

// The patterns painted inside each ball are `paint_colour` in paint.wgsl.

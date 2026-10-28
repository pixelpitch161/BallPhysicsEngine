// Shapes and fill patterns of styled balls, shared by the 2D and 3D views. Pasted after a
// shader that defines PI and TAU.

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

// Signed distance to the edge of a styled ball's shape, in radii; negative inside.
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

// 0 or 1 in alternating bands of `x`, softened over a pixel.
fn bands(x: f32) -> f32 {
    return bands_over(x, fwidth(x));
}

// The same, softened over `w`, for values that jump where an angle wraps around.
fn bands_over(x: f32, w_in: f32) -> f32 {
    let w = max(w_in, 1e-4);
    let f = fract(x);
    return clamp((abs(f - 0.5) - 0.25) / w + 0.5, 0.0, 1.0);
}

// 1 inside where signed distance `d` is negative, softened over a pixel.
fn inside(d: f32) -> f32 {
    return clamp(-d / max(fwidth(d), 1e-4) + 0.5, 0.0, 1.0);
}

// How much of the second colour shows at `p` (unit radius, y down) for a fill pattern.
fn pattern(p: vec2<f32>, fill: f32, size: f32) -> f32 {
    let r = length(p);
    let n = size * 0.5;
    if fill < 2.5 {
        return smoothstep(0.0, 1.0, r);
    } else if fill < 3.5 {
        return clamp(p.y * 0.5 + 0.5, 0.0, 1.0);
    } else if fill < 4.5 {
        return bands(p.x * n + 0.25);
    } else if fill < 5.5 {
        return bands(r * n + 0.25);
    } else if fill < 6.5 {
        let cell = fract(p * n + 0.5) - 0.5;
        return inside(length(cell) - 0.28);
    } else if fill < 7.5 {
        let a = bands(p.x * n + 0.25);
        let b = bands(p.y * n + 0.25);
        return a + b - 2.0 * a * b;
    } else if fill < 8.5 {
        return inside(-p.y);
    }
    // Patterns around the middle: a whole number of slice pairs, so the seam where the angle
    // wraps doesn't show.
    let turns = atan2(p.y, p.x) / TAU;
    let turn_width = length(fwidth(p)) / max(r, 1e-3) / TAU;
    let m = max(round(n), 1.0);
    if fill < 9.5 {
        return bands_over(turns * m, turn_width * m);
    } else if fill < 10.5 {
        return bands_over(turns * m + r * 1.2, turn_width * m + fwidth(r) * 1.2);
    }
    // A smiley face: two eyes and a grin.
    let eyes = length(vec2<f32>(abs(p.x) - 0.32, p.y + 0.22) / vec2<f32>(1.0, 1.4)) - 0.11;
    let mouth_ring = abs(length(p - vec2<f32>(0.0, -0.1)) - 0.5) - 0.065;
    let mouth = max(mouth_ring, 0.18 - p.y);
    return inside(min(eyes, mouth));
}

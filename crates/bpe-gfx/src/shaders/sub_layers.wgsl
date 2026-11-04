// Sub-layers: the records (codes 20..29, see `LayerGpu` in frame.rs) that follow a layer's record
// and change only that layer. The walks (layers_walk.wgsl for balls, shape_walk.wgsl for shapes)
// work out the layer's colour and cover as a `Piece`, run each of its sub-layer records over the
// piece in order with the functions here, then mix the piece into the stack where the layer
// sits, by the layer's opacity and blend. Distances are in the walk's unit: ball radii, or a
// shape's world pixels. Nothing here reads a ball's or a shape's globals: the walks pass what
// each needs. Appended after layers.wgsl, whose `soft_inside`, `band` and `blended` it uses.

// A layer and its sub-layers so far: its colour (straight) and cover as if the pixel were wholly
// inside the thing's shape (`_in`) and wholly outside it (`_out`), the light it adds (already
// mixed with the stack under it by the layer's blend, covering nothing), and its shape: the
// signed distance to its edge, negative inside.
struct Piece {
    c_in: vec3<f32>,
    a_in: f32,
    c_out: vec3<f32>,
    a_out: f32,
    light_in: vec3<f32>,
    light_out: vec3<f32>,
    sd: f32,
};

// `col` covering `b` painted over colour `c` covering `a`: the colour and cover after.
fn sub_paint_over(c: vec3<f32>, a: f32, col: vec3<f32>, b: f32) -> vec4<f32> {
    let bb = clamp(b, 0.0, 1.0);
    let na = bb + a * (1.0 - bb);
    if na <= 1e-6 {
        return vec4<f32>(c, 0.0);
    }
    return vec4<f32>((col * bb + c * a * (1.0 - bb)) / na, na);
}

// The piece with `col` painted over it, covering `b_in` and `b_out`; light it covers is hidden.
fn piece_over(p: Piece, col: vec3<f32>, b_in: f32, b_out: f32) -> Piece {
    var r = p;
    let i = sub_paint_over(p.c_in, p.a_in, col, b_in);
    let o = sub_paint_over(p.c_out, p.a_out, col, b_out);
    r.c_in = i.rgb;
    r.a_in = i.a;
    r.c_out = o.rgb;
    r.a_out = o.a;
    r.light_in *= 1.0 - clamp(b_in, 0.0, 1.0);
    r.light_out *= 1.0 - clamp(b_out, 0.0, 1.0);
    return r;
}

// The piece painted over `col` covering `b_in` and `b_out`: the piece stays on top.
fn piece_under(p: Piece, col: vec3<f32>, b_in: f32, b_out: f32) -> Piece {
    var r = p;
    let bi = clamp(b_in, 0.0, 1.0);
    let bo = clamp(b_out, 0.0, 1.0);
    let ai = bi + p.a_in * (1.0 - bi);
    let ao = bo + p.a_out * (1.0 - bo);
    if ai > 1e-6 {
        r.c_in = (p.c_in * p.a_in + col * bi * (1.0 - p.a_in)) / ai;
    }
    if ao > 1e-6 {
        r.c_out = (p.c_out * p.a_out + col * bo * (1.0 - p.a_out)) / ao;
    }
    r.a_in = ai;
    r.a_out = ao;
    return r;
}

// The piece's colour where it is: inside, or outside where it covers more there.
fn piece_colour(p: Piece) -> vec3<f32> {
    return select(p.c_in, p.c_out, p.a_out > p.a_in);
}

// Light `col` at `amount`, mixed with `under` (premultiplied) by blend `mode` as a glow's light is.
fn sub_light(col: vec3<f32>, amount: f32, under: vec4<f32>, mode: u32) -> vec3<f32> {
    var mixed = col;
    if mode != 0u && under.a > 1e-5 {
        mixed = (1.0 - under.a) * col + under.a * blended(mode, under.rgb / under.a, col);
    }
    return mixed * amount;
}

// `f(c)` mixed into the piece's colours by `amount`.
fn piece_recoloured(p: Piece, c_in: vec3<f32>, c_out: vec3<f32>, amount: f32) -> Piece {
    var r = p;
    let t = clamp(amount, 0.0, 1.0);
    r.c_in = mix(p.c_in, c_in, t);
    r.c_out = mix(p.c_out, c_out, t);
    return r;
}

// A Cut (20): its cover (inside, outside) and its own distance (negative where it lets the layer
// show), worked out by the walk; the piece shows only where it lets it, and takes its edge.
fn sub_cut(p: Piece, v: vec2<f32>, cut_d: f32) -> Piece {
    var r = p;
    r.a_in *= v.x;
    r.a_out *= v.y;
    r.light_in *= v.x;
    r.light_out *= v.y;
    r.sd = max(p.sd, cut_d);
    return r;
}

// How strong a Glow (21) record `m` is at the piece's edge distance `sd`: by where it shines
// (`p0.y`: 0 outside, 1 inside, 2 both) and its size (`p0.x`, walk units).
fn sub_glow_amount(m: LayerGpu, sd: f32) -> f32 {
    let where_ = u32(m.p0.y + 0.5);
    var t = max(sd, 0.0);
    if where_ == 1u {
        t = max(-sd, 0.0);
    } else if where_ == 2u {
        t = abs(sd);
    }
    let x = t / max(m.p0.x, 1e-4);
    return max(m.amount.y, 0.0) * exp(-x * x);
}

// A Glow (21) in colour `col`, faded by `fade` near the quad's edge: light round the piece's
// edge. Outside it shines where the piece doesn't cover, inside only where it does.
fn sub_glow(p: Piece, m: LayerGpu, col: vec3<f32>, fade: f32, under_in: vec4<f32>, under_out: vec4<f32>, mode: u32) -> Piece {
    var r = p;
    let h = sub_glow_amount(m, p.sd) * clamp(m.amount.x, 0.0, 1.0) * fade;
    let where_ = u32(m.p0.y + 0.5);
    var w_in = 1.0 - clamp(p.a_in, 0.0, 1.0);
    var w_out = 1.0 - clamp(p.a_out, 0.0, 1.0);
    if where_ == 1u {
        w_in = clamp(p.a_in, 0.0, 1.0);
        w_out = clamp(p.a_out, 0.0, 1.0);
    } else if where_ == 2u {
        w_in = 1.0;
        w_out = 1.0;
    }
    r.light_in += sub_light(col, h * w_in, under_in, mode);
    r.light_out += sub_light(col, h * w_out, under_out, mode);
    return r;
}

// Where an Outline (22) record `m`'s band runs against the piece's edge, in walk units: inside
// it, centred on it or outside it (`p0.y`), `width` wide.
fn sub_outline_band(m: LayerGpu, width: f32) -> vec2<f32> {
    if m.p0.y > 1.5 {
        return vec2<f32>(0.0, width);
    }
    if m.p0.y > 0.5 {
        return vec2<f32>(-0.5 * width, 0.5 * width);
    }
    return vec2<f32>(-width, 0.0);
}

// How much of an Outline's band from `lo` to `hi` covers a point `sd` from the piece's edge.
fn sub_band_cover(sd: f32, lo: f32, hi: f32, aa: f32) -> f32 {
    return clamp((sd - lo) / aa + 0.5, 0.0, 1.0) * clamp((hi - sd) / aa + 0.5, 0.0, 1.0);
}

// An Outline (22) in colour `col`, its band `lohi`: painted over the piece along its edge.
fn sub_outline(p: Piece, m: LayerGpu, col: vec3<f32>, lohi: vec2<f32>, aa: f32) -> Piece {
    if lohi.y - lohi.x <= 0.0 {
        return p;
    }
    let b = sub_band_cover(p.sd, lohi.x, lohi.y, aa) * clamp(m.amount.x, 0.0, 1.0);
    return piece_over(p, col, b, b);
}

// A Shadow (23): the piece's shape at the shifted point is `shifted_sd`; a soft copy of it goes
// under the piece in the record's colour.
fn sub_shadow(p: Piece, m: LayerGpu, shifted_sd: f32, aa: f32) -> Piece {
    let s = clamp(m.amount.x, 0.0, 1.0) * soft_inside(shifted_sd, max(m.p0.z, aa));
    return piece_under(p, m.colour.rgb, s, s);
}

// Shading (24): the piece's colours darkened towards `shade` by `amount` (its factor, amount and
// opacity together).
fn sub_shading(p: Piece, shade: vec3<f32>, amount: f32) -> Piece {
    return piece_recoloured(p, shade, shade, amount);
}

// The colour `c` (linear light) as sRGB-ish, 0 to 1, and back: recolouring works there, as the
// eye sees it.
fn sub_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let x = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    return select(1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - 0.055, x * 12.92, x <= vec3<f32>(0.0031308));
}

fn sub_to_linear(c: vec3<f32>) -> vec3<f32> {
    let x = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    return select(pow((x + 0.055) / 1.055, vec3<f32>(2.4)), x / 12.92, x <= vec3<f32>(0.04045));
}

// `c` (linear light) turned `turn` turns round the colour wheel, keeping its saturation and
// value: `Color::shift_hue`'s maths, so the CPU and GPU agree. Light above 1 keeps its extra.
fn hue_turn(c: vec3<f32>, turn: f32) -> vec3<f32> {
    let top = max(c.r, max(c.g, c.b));
    let over = max(top, 1.0);
    let s = sub_to_srgb(c / over);
    let mx = max(s.r, max(s.g, s.b));
    let delta = mx - min(s.r, min(s.g, s.b));
    if delta <= 0.0 {
        return c;
    }
    var h = 0.0;
    if mx == s.r {
        h = ((s.g - s.b) / delta + 6.0) % 6.0 / 6.0;
    } else if mx == s.g {
        h = ((s.b - s.r) / delta + 2.0) / 6.0;
    } else {
        h = ((s.r - s.g) / delta + 4.0) / 6.0;
    }
    h = fract(h + turn);
    let sat = delta / mx;
    let k = vec3<f32>(1.0, 2.0 / 3.0, 1.0 / 3.0);
    let q = abs(fract(vec3<f32>(h) + k) * 6.0 - 3.0);
    let rgb = mx * mix(vec3<f32>(1.0), clamp(q - 1.0, vec3<f32>(0.0), vec3<f32>(1.0)), sat);
    return sub_to_linear(rgb) * over;
}

// How light `c` (linear light) looks, 0 to 1.
fn sub_lightness(c: vec3<f32>) -> f32 {
    return pow(clamp(dot(c, vec3<f32>(0.2126, 0.7152, 0.0722)), 0.0, 1.0), 1.0 / 2.2);
}

// One colour through a Recolour (27) record `m`: tinted, turned, mapped onto its gradient by
// lightness, or its brightness, contrast and saturation changed.
fn sub_recolour_one(m: LayerGpu, c: vec3<f32>) -> vec3<f32> {
    switch u32(m.p0.x + 0.5) {
        case 0u: {
            return mix(c, m.colour.rgb, clamp(m.amount.y, 0.0, 1.0));
        }
        case 1u: {
            return hue_turn(c, m.p0.y);
        }
        case 2u: {
            var mapped = m.colour.rgb;
            if m.paint.row >= 0.0 {
                mapped = paint_along(m.paint, sub_lightness(c), g.view.w).rgb;
            }
            return mix(c, mapped, clamp(m.amount.y, 0.0, 1.0));
        }
        default: {
            let s = sub_to_srgb(c);
            var x = (s - vec3<f32>(0.5)) * max(m.p0.w, 0.0) + vec3<f32>(0.5) + vec3<f32>(m.p0.z);
            let grey = dot(x, vec3<f32>(0.2126, 0.7152, 0.0722));
            x = mix(vec3<f32>(grey), x, max(m.p1.x, 0.0));
            return sub_to_linear(x);
        }
    }
}

// Recolour (27): the piece's colours changed, mixed by its opacity.
fn sub_recolour(p: Piece, m: LayerGpu) -> Piece {
    return piece_recoloured(p, sub_recolour_one(m, p.c_in), sub_recolour_one(m, p.c_out), m.amount.x);
}

// Flash on hit (28): the piece's colours lit for a moment after each hit. `hit` is the thing's
// hit data: seconds since its last hit, that hit's strength, how many hits.
fn sub_flash(p: Piece, m: LayerGpu, hit: vec4<f32>) -> Piece {
    let length = max(m.amount.y, 1e-3);
    let impact = select(1.0, max(hit.y, 0.0), m.amount.z > 0.5);
    let level = max(m.amount.x, 0.0) * exp(-3.0 * max(hit.x, 0.0) / length) * impact;
    var r = p;
    r.c_in += m.colour.rgb * level;
    r.c_out += m.colour.rgb * level;
    return r;
}

// Colour on hit (29): the piece's colours turned round the wheel for every hit so far.
fn sub_hue(p: Piece, m: LayerGpu, hit: vec4<f32>) -> Piece {
    let turn = m.amount.x * floor(max(hit.z, 0.0) + 0.5);
    if turn == 0.0 {
        return p;
    }
    var r = p;
    r.c_in = hue_turn(p.c_in, turn);
    r.c_out = hue_turn(p.c_out, turn);
    return r;
}

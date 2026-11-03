// One walk over a ball's stack of layer records, bottom to top, at quad point `local` (-1 to 1
// across the ball before it turns or stretches): records `first` to `first + count`. Pasted in
// by `layers::source` with WALK, COV_IN and COV_OUT named, and SHADER_CASE either empty or the
// shader layers' case: a stack without shader layers, and the walk shader layers sample (which
// can't call itself), leave shader records out.

var<private> COV_IN: array<f32, 64>;
var<private> COV_OUT: array<f32, 64>;

fn WALK(local: vec2<f32>, first: u32, count_in: u32) -> Walked {
    let p = rotate(local, -layered_ball.form.z);
    let d = ball_shape(p, layered_ball.form.x, layered_ball.form.y);
    let aa = layered_ball.aa;
    let body = layered_ball.body;
    let base = vec4<f32>(body.rgb, 1.0);
    // Lit as the ball sits on screen, whichever way it is stretched, as the classic ball was.
    let dir = layered_ball.dir;
    let screen = dir * local.x + vec2<f32>(-dir.y, dir.x) * local.y;
    let lit_at = screen / max(length(screen), 1.0);

    var inner = vec4<f32>(0.0);
    var outer = vec4<f32>(0.0);
    let count = min(count_in, MAX_LAYERS);
    var below = 0u;
    var k = 0u;
    loop {
        if k >= count {
            break;
        }
        let l = look_layers[first + k];
        let mode = l.head.y;
        let flag = (l.head.z & 1u) != 0u;
        let opacity = clamp(l.amount.x, 0.0, 1.0);
        let masks = min(l.head.w, count - k - 1u);
        // The layer's colour and how much it covers, inside the shape and outside it.
        var c = vec3<f32>(0.0);
        var a_in = 0.0;
        var a_out = 0.0;
        switch l.head.x {
            case 0u: {
                c = body.rgb;
                a_in = opacity;
            }
            case 1u: {
                var paint = l.paint;
                if paint.row >= 0.0 {
                    paint.vary = layered_ball.vary;
                }
                if flag {
                    paint.angle += layered_ball.misc.z;
                }
                if layered_ball.misc.w > 0.5 {
                    paint.angle -= layered_ball.misc.z;
                }
                if paint_paints(paint) {
                    c = paint_colour(paint, p, g.view.w, base).rgb;
                    a_in = opacity;
                }
            }
            case 2u: {
                let w = max(l.p0.x, 0.0) / layered_ball.radius.y;
                if w > 0.0 {
                    var lo = -w;
                    var hi = 0.0;
                    if l.p0.y > 1.5 {
                        lo = 0.0;
                        hi = w;
                    } else if l.p0.y > 0.5 {
                        lo = -0.5 * w;
                        hi = 0.5 * w;
                    }
                    // Same as body: the body's colour a little darker, so a white ball shows it.
                    c = select(l.colour.rgb, body.rgb * 0.6, flag);
                    a_in = band(d, lo, hi, aa, true) * opacity;
                    a_out = band(d, lo, hi, aa, false) * opacity;
                }
            }
            case 3u: {
                let under = select(body.rgb, inner.rgb / inner.a, inner.a > 1e-5);
                let lit = shine_light(lit_at, under, l.p0.xy, l.p0.z);
                c = mix(under, lit, clamp(l.amount.y, 0.0, 1.0));
                a_in = opacity;
            }
            case 4u: {
                let glow_d = max(d, 0.0) / max(l.p0.x, 0.05);
                // Faded out over the last fifth of the quad, so a glow bigger than the quad
                // never ends in a hard square.
                let reach = max(layered_ball.form.w, 1.0);
                let quad = max(abs(local.x), abs(local.y));
                let fade = 1.0 - smoothstep(0.8 * reach, reach, quad);
                c = select(l.colour.rgb, body.rgb, flag);
                a_out = max(l.amount.y, 0.0) * exp(-glow_d * glow_d) * opacity * fade;
            }
            case 5u: {
                c = l.colour.rgb;
                a_in = shading_factor(l, p, screen, aa) * clamp(l.p0.w, 0.0, 1.0) * opacity;
            }
            //SHADER_CASE
            default: {}
        }
        // Its masks multiply into how much it covers.
        for (var j = 1u; j <= masks; j++) {
            let m = look_layers[first + k + j];
            let t = i32(m.amount.y);
            var chosen = vec2<f32>(0.0);
            if t >= 0 && u32(t) < k {
                chosen = vec2<f32>(COV_IN[t], COV_OUT[t]);
            }
            let v = mask_value(m, p, screen, vec2<f32>(COV_IN[below], COV_OUT[below]), chosen, inner, outer, aa);
            a_in *= v.x;
            a_out *= v.y;
        }
        if l.head.x == 4u {
            // Light, covering nothing: mixed with what reaches past the edge, then added.
            var mixed = c;
            if mode != 0u && outer.a > 1e-5 {
                mixed = (1.0 - outer.a) * c + outer.a * blended(mode, outer.rgb / outer.a, c);
            }
            outer = vec4<f32>(outer.rgb + mixed * a_out, outer.a);
        } else {
            if a_in > 0.0 {
                inner = layer_over(inner, c, a_in, mode);
            }
            if a_out > 0.0 {
                outer = layer_over(outer, c, a_out, mode);
            }
        }
        COV_IN[k] = clamp(a_in, 0.0, 1.0);
        COV_OUT[k] = clamp(a_out, 0.0, 1.0);
        below = k;
        k += 1u + masks;
    }
    return Walked(inner, outer);
}

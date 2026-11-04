// One walk over a ball's stack of layer records, bottom to top, at quad point `local` (-1 to 1
// across the ball before it turns or stretches): records `first` to `first + count`. `split`
// draws one part of a ball split by its Blend layer: 0 the whole ball; else its low 16 bits are
// the first record above the Blend layer, and with bit 16 clear the walk draws the layers under it
// (and the group records), with bit 16 set the layers from there up as they show over the rest. Pasted in
// by `layers::source` with WALK, COV_IN and COV_OUT named, and SHADER_CASE and GROUP_SHADED either
// empty or the shader layers' case and the group shaders mixed into a layer: a stack without
// shader layers, and the walk shader layers sample (which can't call itself), leave shader and
// group shader records out.

var<private> COV_IN: array<f32, 64>;
var<private> COV_OUT: array<f32, 64>;

fn WALK(local: vec2<f32>, first: u32, count_in: u32, split: u32) -> Walked {
    let p = rotate(local, -layered_ball.form.z);
    let d = ball_shape(p, layered_ball.form.x, layered_ball.form.y);
    let aa = layered_ball.aa;
    let body = layered_ball.body;
    // Lit as the ball sits on screen, whichever way it is stretched, as the classic ball was.
    let dir = layered_ball.dir;
    let screen = dir * local.x + vec2<f32>(-dir.y, dir.x) * local.y;
    let lit_at = screen / max(length(screen), 1.0);

    // The body's colour here: its gradient's, when it runs across each ball, turned by the ball's
    // own variation and its hue turn (`vary.w`, turns).
    var body_c = body.rgb;
    let body_rec = look_layers[first];
    if count_in > 0u && body_rec.head.x == 0u && body_rec.paint.row >= 0.0 {
        var bp = body_rec;
        let v = layered_ball.vary;
        bp.paint.vary = vec4<f32>(v.x + v.w, v.y, v.z, 0.0);
        body_c = own_colour(bp, p, length(p), body.rgb);
    }
    let base_c = vec4<f32>(body_c, 1.0);
    let cut = split & 0xffffu;
    let upper = cut > 0u && (split & 0x10000u) != 0u;
    let lower = cut > 0u && !upper;

    var inner = vec4<f32>(0.0);
    var outer = vec4<f32>(0.0);
    // The layers above the Blend layer, as they show over the rest (upper part only).
    var top_in = vec4<f32>(0.0);
    var top_out = vec4<f32>(0.0);
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
        // The group colour mixes into this layer's own colour.
        let grouped = (l.head.z & 8u) != 0u;
        let opacity = clamp(l.amount.x, 0.0, 1.0);
        let masks = min(l.head.w, count - k - 1u);
        // A layer this ball doesn't wear is passed over: it covers nothing and isn't the layer
        // below the next one.
        // The lower part leaves out the layers above the Blend layer, not the group records.
        let above = cut > 0u && k >= cut && l.head.x != 8u && l.head.x != 9u;
        if (l.head.z & 16u) != 0u || (lower && above) {
            COV_IN[k] = 0.0;
            COV_OUT[k] = 0.0;
            k += 1u + masks;
            continue;
        }
        // The layer's colour and how much it covers, inside the shape and outside it.
        var c = vec3<f32>(0.0);
        var a_in = 0.0;
        var a_out = 0.0;
        switch l.head.x {
            case 0u: {
                c = body_c;
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
                    c = paint_colour(paint, p, g.view.w, base_c).rgb;
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
                    // Own colours run straight across or across the ring from its inner edge.
                    let across = clamp((d - lo) / max(hi - lo, 1e-4), 0.0, 1.0);
                    c = select(own_colour(l, p, across, l.colour.rgb), body_c * 0.6, flag);
                    a_in = band(d, lo, hi, aa, true) * opacity;
                    a_out = band(d, lo, hi, aa, false) * opacity;
                }
            }
            case 3u: {
                let under = select(body_c, inner.rgb / inner.a, inner.a > 1e-5);
                let lit = shine_material(lit_at, under, l.p0.xy, l.p0.z, l.p0.w, l.p1.x);
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
                // Own colours run straight across or out from the edge to the glow's size.
                c = select(own_colour(l, p, clamp(glow_d, 0.0, 1.0), l.colour.rgb), body_c, flag);
                a_out = max(l.amount.y, 0.0) * exp(-glow_d * glow_d) * opacity * fade;
            }
            case 5u: {
                c = own_colour(l, p, length(p), l.colour.rgb);
                a_in = shading_factor(l, p, screen, aa) * clamp(l.p0.w, 0.0, 1.0) * opacity;
            }
            //SHADER_CASE
            case 8u: {
                // The group colour over the whole ball as it stands here.
                inner = group_over(inner);
                outer = group_over(outer);
                top_in = group_over(top_in);
                top_out = group_over(top_out);
            }
            default: {}
        }
        if grouped {
            c = with_group(c);
        }
        //GROUP_SHADED
        // Its masks multiply into how much it covers.
        var masked = vec2<f32>(1.0);
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
            masked *= v;
        }
        // Showing its masks alone (the editor's view): white where they let it show, black elsewhere.
        if (l.head.z & 4u) != 0u {
            return Walked(vec4<f32>(vec3<f32>(masked.x), 1.0), vec4<f32>(0.0));
        }
        if upper && l.head.x == 9u {
            // A group shader over the whole ball recolours the layers above the Blend layer where
            // they are, without covering more of the ball.
            top_in = recoloured_top(top_in, c, a_in, mode);
            top_out = recoloured_top(top_out, c, a_out, mode);
        } else if upper && k >= cut {
            if l.head.x == 4u {
                var mixed = c;
                if mode != 0u && outer.a > 1e-5 {
                    mixed = (1.0 - outer.a) * c + outer.a * blended(mode, outer.rgb / outer.a, c);
                }
                top_out = vec4<f32>(top_out.rgb + mixed * a_out, top_out.a);
            } else {
                if a_in > 0.0 {
                    top_in = top_over(top_in, inner, c, a_in, mode);
                }
                if a_out > 0.0 {
                    top_out = top_over(top_out, outer, c, a_out, mode);
                }
            }
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
    if upper {
        return Walked(top_in, top_out);
    }
    return Walked(inner, outer);
}

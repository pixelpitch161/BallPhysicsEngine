// One walk over a ball's stack of layer records, bottom to top, at quad point `local` (-1 to 1
// across the ball before it turns or stretches): records `first` to `first + count`. `split`
// draws one part of a ball split by its Blend layer: 0 the whole ball; else its low 16 bits are
// the first record above the Blend layer, and with bit 16 clear the walk draws the layers under it
// (and the group records; how much the layers above cover is left in TOP_COVER), with bit 16 set
// the layers from there up as they show over the rest. Pasted in
// by `layers::source` with WALK, COV_IN and COV_OUT named, and SHADER_CASE and GROUP_SHADED either
// empty or the shader layers' (and shader sub-layers') case and the group shaders mixed into a
// layer: a stack without shader layers, and the walk shader layers sample (which can't call
// itself), leave shader and group shader records out.
//
// Each layer's record is followed by its sub-layer records (codes 20 and up): the walk works the
// layer out as a `Piece`, runs its sub-layers over it in order (sub_layers.wgsl), then mixes the
// piece in where the layer sits, once the next layer's record (or the end) comes. It is one loop
// over every record, so each costly piece of maths (a layer's own colours, shading, shine, an
// effect) is written once for layers and sub-layers alike: the shader compiles small. COV_IN and
// COV_OUT keep each layer's piece's final cover, by record, for the cuts of layers above it. A
// shader sub-layer samples its piece alone: the walk it samples starts at the layer's record and
// stops before the shader's, with PIECE_PICTURE set (the layer whole, uncut, over PIECE_UNDER).

var<private> COV_IN: array<f32, 128>;
var<private> COV_OUT: array<f32, 128>;

fn WALK(local: vec2<f32>, first: u32, count_in: u32, split: u32) -> Walked {
    let p = rotate(local, -layered_ball.form.z);
    let d = ball_shape(p, layered_ball.form.x, layered_ball.form.y);
    let aa = layered_ball.aa;
    let body = layered_ball.body;
    // Lit as the ball sits on screen, whichever way it is stretched, as the classic ball was.
    let dir = layered_ball.dir;
    let screen = dir * local.x + vec2<f32>(-dir.y, dir.x) * local.y;
    let lit_at = screen / max(length(screen), 1.0);
    let perp = vec2<f32>(-dir.y, dir.x);
    // Light that reaches far fades out over the last fifth of the quad, so a glow bigger than the
    // quad never ends in a hard square.
    let reach = max(layered_ball.form.w, 1.0);
    let quad = max(abs(local.x), abs(local.y));
    let fade = 1.0 - smoothstep(0.8 * reach, reach, quad);
    // Read before it is written only by a walk that starts at a layer above the body (a shader
    // sub-layer's picture): nothing under that layer then hides it.
    COV_IN[0] = 1.0;
    COV_OUT[0] = 1.0;

    // The body's colour here: its gradient's, when it runs across each ball, turned by the ball's
    // own variation and its hue turn (`vary.w`, turns).
    var body_c = body.rgb;
    let body_rec = look_layers[first];
    if count_in > 0u && body_rec.head.x == 0u && body_rec.paint.row >= 0.0 {
        var bp = body_rec;
        let v = layered_ball.vary;
        bp.paint.vary = vec4<f32>(v.x + v.w, v.y, v.z, 0.0);
        body_c = own_colour(bp, p, length(p), body.rgb);
        // The group colour reaches a gradient body here, so layers the same as the body follow.
        if (body_rec.head.z & 8u) != 0u {
            body_c = with_group(body_c);
        }
    }
    let base_c = vec4<f32>(body_c, 1.0);
    // The surface's height for the light passes: a sphere at Roundness 1 on a round ball.
    if g.light.x > 1.5 {
        let round = select(0.0, body_rec.light.z, count_in > 0u);
        let e = 1.0 + min(d, 0.0);
        LIGHT_HEIGHT = layered_ball.radius.y * max(round, 0.0) * sqrt(max(1.0 - e * e, 0.0)) * step(d, 0.0);
    }
    let cut = split & 0xffffu;
    let upper = cut > 0u && (split & 0x10000u) != 0u;
    let lower = cut > 0u && !upper;

    var inner = vec4<f32>(0.0);
    var outer = vec4<f32>(0.0);
    // The layers above the Blend layer, as they show over the rest (upper part only).
    var top_in = vec4<f32>(0.0);
    var top_out = vec4<f32>(0.0);
    // How much the layers above the Blend layer cover (lower part only).
    var cover_top = vec2<f32>(0.0);
    // The lower part: the whole stack up to here, with the layers above the Blend layer, so their
    // cuts read the same layers as in the upper part.
    var whole_in = vec4<f32>(0.0);
    var whole_out = vec4<f32>(0.0);
    let count = min(count_in, MAX_LAYERS);
    var below = 0u;

    // The open piece: layer record `lk` (`ll`) as worked out (its colour `lc` and covers
    // `la_in`/`la_out` kept for a group shader above a Blend layer), then its sub-layers over it.
    var open = false;
    var lk = 0u;
    var ll = body_rec;
    var lc = vec3<f32>(0.0);
    var la_in = 0.0;
    var la_out = 0.0;
    var piece = Piece(vec3<f32>(0.0), 0.0, vec3<f32>(0.0), 0.0, vec3<f32>(0.0), vec3<f32>(0.0), 0.0);
    // How much light a glow layer gives, after its cuts: the cover cuts above it read.
    var glow_amount = 0.0;
    // Its cuts' covers multiplied, for the editor's view of them alone.
    var cuts = vec2<f32>(1.0);
    var k = 0u;
    loop {
        let end = k >= count;
        var r = ll;
        if !end {
            r = look_layers[first + k];
        }
        let sub = !end && r.head.x >= 20u;
        // ---------- the open piece mixed in, at the next layer or the end ----------
        if open && !sub {
            open = false;
            // The light passes' surface (mode 2) mixes every layer plainly.
            let mode = select(ll.head.y, 0u, g.light.x > 1.5);
            // The lower part leaves out the layers above the Blend layer, not the group records.
            let above = cut > 0u && lk >= cut && ll.head.x != 8u && ll.head.x != 9u;
            // Showing its cuts alone (the editor's view): white where they let it show, black elsewhere.
            if (ll.head.z & 4u) != 0u {
                TOP_COVER = vec2<f32>(0.0);
                return Walked(vec4<f32>(vec3<f32>(cuts.x), 1.0), vec4<f32>(0.0));
            }
            // The layer's opacity, applied once to the piece and everything its sub-layers added
            // (a sub-layer's picture shows the layer whole). A glow layer's light is made here,
            // from its colour after its sub-layers and its strength after its cuts.
            let lo = select(clamp(ll.amount.x, 0.0, 1.0), 1.0, PIECE_PICTURE);
            piece.light_in *= lo;
            piece.light_out *= lo;
            glow_amount *= lo;
            if ll.head.x == 4u {
                piece.light_out += sub_light(piece.c_out, glow_amount, select(outer, whole_out, lower && above), mode);
            }
            if g.light.x > 0.5 {
                piece = light_piece(piece, ll);
            }
            let p_in = clamp(piece.a_in, 0.0, 1.0) * lo;
            let p_out = clamp(piece.a_out, 0.0, 1.0) * lo;
            COV_IN[lk] = p_in;
            COV_OUT[lk] = select(p_out, clamp(glow_amount, 0.0, 1.0), ll.head.x == 4u);
            below = lk;
            if lower && above {
                // Left out of the lower part; only how much it covers counts (light covers
                // nothing). It still counts for the cuts of the layers over it.
                let a = vec2<f32>(p_in, p_out);
                cover_top = a + cover_top * (vec2<f32>(1.0) - a);
                if p_in > 0.0 {
                    whole_in = layer_over(whole_in, piece.c_in, p_in, mode);
                }
                if p_out > 0.0 {
                    whole_out = layer_over(whole_out, piece.c_out, p_out, mode);
                }
                whole_in = vec4<f32>(whole_in.rgb + piece.light_in, whole_in.a);
                whole_out = vec4<f32>(whole_out.rgb + piece.light_out, whole_out.a);
            } else {
                if upper && ll.head.x == 9u {
                    // A group shader over the whole ball recolours the layers above the Blend
                    // layer where they are, without covering more of the ball.
                    top_in = recoloured_top(top_in, lc, la_in, mode);
                    top_out = recoloured_top(top_out, lc, la_out, mode);
                } else if upper && lk >= cut {
                    if p_in > 0.0 {
                        top_in = top_over(top_in, inner, piece.c_in, p_in, mode);
                    }
                    if p_out > 0.0 {
                        top_out = top_over(top_out, outer, piece.c_out, p_out, mode);
                    }
                    top_in = vec4<f32>(top_in.rgb + piece.light_in, top_in.a);
                    top_out = vec4<f32>(top_out.rgb + piece.light_out, top_out.a);
                }
                if p_in > 0.0 {
                    inner = layer_over(inner, piece.c_in, p_in, mode);
                }
                if p_out > 0.0 {
                    outer = layer_over(outer, piece.c_out, p_out, mode);
                }
                inner = vec4<f32>(inner.rgb + piece.light_in, inner.a);
                outer = vec4<f32>(outer.rgb + piece.light_out, outer.a);
                if lower && lk < cut {
                    whole_in = inner;
                    whole_out = outer;
                }
            }
        }
        if end {
            break;
        }
        // A sub-layer of a layer passed over goes with it.
        if sub && !open {
            k += 1u;
            continue;
        }
        // A layer this ball doesn't wear is passed over: it covers nothing and isn't the layer
        // below the next one.
        if !sub && (r.head.z & 16u) != 0u {
            COV_IN[k] = 0.0;
            COV_OUT[k] = 0.0;
            k += 1u;
            continue;
        }
        let flag = (r.head.z & 1u) != 0u;
        // A layer is worked out whole: its opacity applies to its piece once it is done, so what
        // its sub-layers add fades with it. A sub-layer's own opacity applies as it works.
        let opacity = select(1.0, clamp(r.amount.x, 0.0, 1.0), sub);
        // A layer mixes in by its own blend; a sub-layer's light by its layer's.
        let mode = select(r.head.y, ll.head.y, sub);
        // How far along its own colours each kind reads: across an outline's band, out from the
        // edge to a glow's size, from the middle out for the rest.
        var along = length(p);
        switch r.head.x {
            case 2u: {
                let lohi = sub_outline_band(r, max(r.p0.x, 0.0) / layered_ball.radius.y);
                along = clamp((d - lohi.x) / max(lohi.y - lohi.x, 1e-4), 0.0, 1.0);
            }
            case 4u: {
                along = clamp(max(d, 0.0) / max(r.p0.x, 0.05), 0.0, 1.0);
            }
            case 21u: {
                along = clamp(max(piece.sd, 0.0) / max(r.p0.x, 1e-4), 0.0, 1.0);
            }
            case 22u: {
                let lohi = sub_outline_band(r, max(r.p0.x, 0.0) / layered_ball.radius.y);
                along = clamp((piece.sd - lohi.x) / max(lohi.y - lohi.x, 1e-4), 0.0, 1.0);
            }
            default: {}
        }
        // Its own colours there (one place for every kind that has them, so the paint maths
        // compiles once).
        var own_c = r.colour.rgb;
        let x = r.head.x;
        if (x >= 2u && x <= 5u) || x == 21u || x == 22u || x == 24u || x == 25u {
            own_c = own_colour(r, p, along, r.colour.rgb);
        }
        // A layer's colour and how much it covers, inside the shape and outside it.
        var c = vec3<f32>(0.0);
        var a_in = 0.0;
        var a_out = 0.0;
        switch r.head.x {
            case 0u: {
                c = body_c;
                a_in = opacity;
            }
            case 1u: {
                var paint = r.paint;
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
                let w = max(r.p0.x, 0.0) / layered_ball.radius.y;
                if w > 0.0 {
                    let lohi = sub_outline_band(r, w);
                    // Same as body: the body's colour a little darker, so a white ball shows it.
                    // Own colours run straight across or across the ring from its inner edge.
                    c = select(own_c, body_c * 0.6, flag);
                    a_in = band(d, lohi.x, lohi.y, aa, true) * opacity;
                    a_out = band(d, lohi.x, lohi.y, aa, false) * opacity;
                }
            }
            case 3u, 25u: {
                // Shine: white light, or its own colours (straight across the ball, or from its
                // middle), over the layers under it, or over its layer's piece.
                let light = select(vec3<f32>(1.0), own_c, flag);
                let under = select(select(body_c, inner.rgb / inner.a, inner.a > 1e-5), PIECE_UNDER, PIECE_PICTURE);
                let lit_in = shine_material(lit_at, select(under, piece.c_in, sub), r.p0.xy, r.p0.z, r.p0.w, r.p1.x, light);
                let strength = clamp(r.amount.y, 0.0, 1.0);
                if sub {
                    let lit_out = shine_material(lit_at, piece.c_out, r.p0.xy, r.p0.z, r.p0.w, r.p1.x, light);
                    piece = piece_recoloured(piece, lit_in, lit_out, strength * opacity);
                } else {
                    c = mix(under, lit_in, strength);
                    a_in = opacity;
                }
            }
            case 4u: {
                // Own colours run straight across or out from the edge to the glow's size.
                c = select(own_c, body_c, flag);
                let glow_d = max(d, 0.0) / max(r.p0.x, 0.05);
                a_out = max(r.amount.y, 0.0) * exp(-glow_d * glow_d) * opacity * fade;
            }
            case 5u, 24u: {
                // Shading: the layers under it darkened, or its layer's piece.
                let amount = shading_factor(r, p, screen, aa) * clamp(r.p0.w, 0.0, 1.0) * opacity;
                if sub {
                    piece = sub_shading(piece, own_c, amount);
                } else {
                    c = own_c;
                    a_in = amount;
                }
            }
            //SHADER_CASE
            case 8u: {
                // The group colour over the whole ball as it stands here (not over the surface).
                if g.light.x < 1.5 {
                    inner = group_over(inner);
                    outer = group_over(outer);
                    top_in = group_over(top_in);
                    top_out = group_over(top_out);
                }
            }
            case 20u: {
                if !PIECE_PICTURE {
                    let t = i32(r.amount.y);
                    var chosen = vec2<f32>(0.0);
                    if t >= 0 && u32(t) < lk {
                        chosen = vec2<f32>(COV_IN[t], COV_OUT[t]);
                    }
                    let above = cut > 0u && lk >= cut;
                    let under_in = select(inner, whole_in, lower && above);
                    let under_out = select(outer, whole_out, lower && above);
                    let v = cut_value(r, p, screen, vec2<f32>(COV_IN[below], COV_OUT[below]), chosen, under_in, under_out, aa);
                    piece = sub_cut(piece, v.xy, v.z);
                    cuts *= v.xy;
                    glow_amount *= v.y;
                }
            }
            case 21u: {
                let col = select(own_c, piece_colour(piece), flag);
                let above = cut > 0u && lk >= cut;
                piece = sub_glow(piece, r, col, fade, select(inner, whole_in, lower && above), select(outer, whole_out, lower && above), mode);
            }
            case 22u: {
                let lohi = sub_outline_band(r, max(r.p0.x, 0.0) / layered_ball.radius.y);
                let col = select(own_c, piece_colour(piece) * 0.6, flag);
                piece = sub_outline(piece, r, col, lohi, aa);
            }
            case 23u: {
                // The layer's shape (and its shape and brush cuts above this) where the shadow
                // falls from: the point the offset back, on the screen or turning with the ball.
                var q_s = p - r.p0.xy;
                var scr_s = screen - r.p0.xy;
                if r.p0.w < 0.5 {
                    q_s = rotate(vec2<f32>(dot(scr_s, dir), dot(scr_s, perp)), -layered_ball.form.z);
                } else {
                    let loc = rotate(q_s, layered_ball.form.z);
                    scr_s = dir * loc.x + perp * loc.y;
                }
                var sd_s = ball_layer_sd(ll, ball_shape(q_s, layered_ball.form.x, layered_ball.form.y));
                for (var i = lk + 1u; i < k; i++) {
                    let e = look_layers[first + i];
                    if e.head.x == 20u {
                        sd_s = max(sd_s, cut_distance(e, q_s, scr_s));
                    }
                }
                piece = sub_shadow(piece, r, sd_s, aa);
            }
            case 27u: {
                piece = sub_recolour(piece, r);
            }
            case 28u: {
                piece = sub_flash(piece, r, layered_ball.hit);
            }
            case 29u: {
                piece = sub_hue(piece, r, layered_ball.hit);
            }
            default: {}
        }
        if !sub {
            // (A gradient body already has the group colour in `body_c`.)
            let grouped = (r.head.z & 8u) != 0u;
            if grouped && !(r.head.x == 0u && r.paint.row >= 0.0) {
                c = with_group(c);
            }
            let l = r;
            let subs = min(l.head.w, count - k - 1u);
            //GROUP_SHADED
            // The piece: the layer as worked out; its sub-layers go over it next. A glow is
            // light, covering nothing, mixed with what reaches past the edge by its blend.
            open = true;
            lk = k;
            ll = r;
            lc = c;
            la_in = a_in * clamp(r.amount.x, 0.0, 1.0);
            la_out = a_out * clamp(r.amount.x, 0.0, 1.0);
            piece = Piece(c, a_in, c, a_out, vec3<f32>(0.0), vec3<f32>(0.0), ball_layer_sd(r, d));
            glow_amount = 0.0;
            cuts = vec2<f32>(1.0);
            if r.head.x == 4u {
                piece.a_in = 0.0;
                piece.a_out = 0.0;
                glow_amount = a_out;
            }
        }
        k += 1u;
    }
    TOP_COVER = cover_top;
    if upper {
        return Walked(top_in, top_out);
    }
    return Walked(inner, outer);
}

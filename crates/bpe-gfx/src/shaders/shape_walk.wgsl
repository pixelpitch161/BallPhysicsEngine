// One walk over a shape's layer records, bottom to top, at world point `world` (`s` where it is
// against the outline): records `first` to `first + count`. `split` draws one part of a shape
// split by its Blend layer, as balls are (see layers_walk.wgsl): 0 the whole shape; else its low
// 16 bits are the first record above the Blend layer, and with bit 16 clear the walk draws the
// layers under it (and leaves how much the layers above cover in TOP_COVER), with bit 16 set the
// layers from there up as they show over the rest. Pasted in by `layers::source` with SHAPE_WALK
// and SHAPE_COV named, and SHAPE_SHADER_CASE either empty or the shader layers' (and shader
// sub-layers') case: a scene without shader layers, and the walk shader layers sample (which
// can't call itself), leave shader records out.
//
// Each layer's record is followed by its sub-layer records (codes 20 and up), worked over the
// layer as a piece as balls' are (sub_layers.wgsl), in world pixels, in one loop over every
// record so each costly piece of maths compiles once. A cut's shape is in the shape's own units
// (its middle at 0, its size 1).

var<private> SHAPE_COV_IN: array<f32, 128>;
var<private> SHAPE_COV_OUT: array<f32, 128>;

fn SHAPE_WALK(world: vec2<f32>, s: ShapeAt, first: u32, count_in: u32, split: u32) -> Walked {
    // One output pixel, in world pixels, and in the shape's units.
    let aa = g.view.z;
    let scale = max(layered_shape.a.w, 1e-6);
    let unit_aa = aa / scale;
    let upright = shape_upright(world);
    let fade = shape_reach_fade(s);
    // Where the light of a Shine sub-layer falls, -1 to 1 across the shape's reach.
    let lit_at = s.q / max(layered_shape.c.y, 1e-3);
    // A shape stands flat, or bulges like a lens by its Surface's Lens (the body's light.z, part
    // 2b): a half ball's height over its depth inside the outline (in shape units).
    LIGHT_HEIGHT = 0.0;
    if light_pass(g.light.x) >= PASS_SURFACE && count_in > 0u {
        let lens = look_layers[first].light.z;
        if lens > 0.0 && look_layers[first].head.x == 0u {
            let t = clamp(-s.sd / scale, 0.0, 1.0);
            LIGHT_HEIGHT = lens * scale * sqrt(max(1.0 - (1.0 - t) * (1.0 - t), 0.0)) * step(s.sd, 0.0);
        }
        // The tint pass (tint.wgsl): what the Body blocks, and Deeper when thick by how deep inside
        // the outline this is (as a half ball's height over its depth).
        let deep = clamp(-s.sd / scale, 0.0, 1.0);
        TINT_BLOCKS = look_layers[first].light.y;
        TINT_THICK = record_deeper(look_layers[first]) * sqrt(max(1.0 - (1.0 - deep) * (1.0 - deep), 0.0));
    }
    LIGHT_BALL = false;
    let cut = split & 0xffffu;
    let upper = cut > 0u && (split & 0x10000u) != 0u;
    if upper {
        TINT_SKIP = true;
    }
    let lower = cut > 0u && !upper;
    var inner = vec4<f32>(0.0);
    var outer = vec4<f32>(0.0);
    // The layers above the Blend layer, as they show over the rest (upper part only), and how
    // much they cover (lower part only).
    var top_in = vec4<f32>(0.0);
    var top_out = vec4<f32>(0.0);
    var cover_top = vec2<f32>(0.0);
    // The lower part: the whole stack up to here, with the layers above the Blend layer, so what
    // reads the stack (cuts from light or dark, colours the same as under) reads the same layers
    // as in the upper part.
    var whole_in = vec4<f32>(0.0);
    var whole_out = vec4<f32>(0.0);
    let count = min(count_in, MAX_LAYERS);
    // Read before it is written only by a walk that starts at a layer above the body (a shader
    // sub-layer's picture): nothing under that layer then hides it.
    SHAPE_COV_IN[0] = 1.0;
    SHAPE_COV_OUT[0] = 1.0;
    var below = 0u;
    // The open piece: layer record `lk` (`ll`) as worked out, then its sub-layers over it.
    var open = false;
    var lk = 0u;
    var ll = look_layers[first];
    var piece = Piece(vec3<f32>(0.0), 0.0, vec3<f32>(0.0), 0.0, vec3<f32>(0.0), vec3<f32>(0.0), 0.0);
    var glow_amount = 0.0;
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
            let mode = select(ll.head.y, 0u, light_pass(g.light.x) >= PASS_SURFACE);
            let above = cut > 0u && lk >= cut;
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
            piece = uv_hidden(piece, ll);
            if light_pass(g.light.x) >= PASS_LIT {
                piece = light_piece(piece, ll, layered_shape.hit);
            }
            piece = left_behind(ll, piece);
            let p_in = clamp(piece.a_in, 0.0, 1.0) * lo;
            let p_out = clamp(piece.a_out, 0.0, 1.0) * lo;
            SHAPE_COV_IN[lk] = p_in;
            SHAPE_COV_OUT[lk] = select(p_out, clamp(glow_amount, 0.0, 1.0), ll.head.x == 4u);
            below = lk;
            if lower && above {
                // Left out of the lower part; only how much it covers counts (light covers nothing).
                cover_top = vec2<f32>(p_in, p_out) + cover_top * (vec2<f32>(1.0) - vec2<f32>(p_in, p_out));
                if p_in > 0.0 {
                    whole_in = layer_over(whole_in, piece.c_in, p_in, mode);
                }
                if p_out > 0.0 {
                    whole_out = layer_over(whole_out, piece.c_out, p_out, mode);
                }
                whole_in = vec4<f32>(whole_in.rgb + piece.light_in, whole_in.a);
                whole_out = vec4<f32>(whole_out.rgb + piece.light_out, whole_out.a);
            } else {
                if upper && above {
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
        if sub && !open {
            k += 1u;
            continue;
        }
        let flag = (r.head.z & 1u) != 0u;
        // A layer is worked out whole: its opacity applies to its piece once it is done, so what
        // its sub-layers add fades with it. A sub-layer's own opacity applies as it works.
        let opacity = select(1.0, clamp(r.amount.x, 0.0, 1.0), sub);
        let mode = select(r.head.y, ll.head.y, sub);
        // The layers under this one, as the whole shape has them (the lower part of a split shape
        // keeps them with the layers above the Blend layer, so both parts read the same).
        let above_here = cut > 0u && select(k, lk, sub) >= cut;
        let stack_in = select(inner, whole_in, lower && above_here);
        let stack_out = select(outer, whole_out, lower && above_here);
        // Their colour here, as if wholly inside the shape.
        let under = select(select(vec3<f32>(1.0), stack_in.rgb / stack_in.a, stack_in.a > 1e-5), PIECE_UNDER, PIECE_PICTURE);
        // How far along its colours each kind reads, and its colours there (one place for every
        // kind, so the paint maths compiles once): a layer's run along the shape, a sub-layer's
        // across it (or across an outline's band, out to a glow's size).
        var along = 0.0;
        var wants = true;
        switch r.head.x {
            case 10u: {}
            case 2u: {
                let lohi = sub_outline_band(r, max(r.p0.x, 0.0));
                along = clamp((s.edge - lohi.x) / max(lohi.y - lohi.x, 1e-4), 0.0, 1.0);
            }
            case 4u: {
                along = clamp(max(s.edge, 0.0) / max(r.p0.x, 1.0), 0.0, 1.0);
            }
            case 21u: {
                along = clamp(max(piece.sd, 0.0) / max(r.p0.x, 1e-4), 0.0, 1.0);
            }
            case 22u: {
                let lohi = sub_outline_band(r, max(r.p0.x, 0.0));
                along = clamp((piece.sd - lohi.x) / max(lohi.y - lohi.x, 1e-4), 0.0, 1.0);
            }
            case 24u, 25u: {
                along = length(s.q);
            }
            default: {
                wants = false;
            }
        }
        var own_c = r.colour.rgb;
        if wants {
            let x = r.head.x;
            own_c = shape_colour(r, s, along, x == 2u || x == 4u || x == 21u || x == 22u, !sub);
        }
        var c = vec3<f32>(0.0);
        var a_in = 0.0;
        var a_out = 0.0;
        switch r.head.x {
            case 0u: {
                c = r.colour.rgb;
                a_in = opacity;
            }
            case 10u: {
                c = own_c;
                a_in = opacity;
            }
            case 2u: {
                let w = max(r.p0.x, 0.0);
                if w > 0.0 {
                    let lohi = sub_outline_band(r, w);
                    // Same as body: the colour under it a little darker, so a white shape shows it.
                    c = select(own_c, under * 0.6, flag);
                    a_in = band(s.edge, lohi.x, lohi.y, aa, true) * opacity;
                    a_out = band(s.edge, lohi.x, lohi.y, aa, false) * opacity;
                }
            }
            case 4u: {
                let gd = max(s.edge, 0.0) / max(r.p0.x, 1.0);
                c = select(own_c, under, flag);
                a_out = max(r.amount.y, 0.0) * exp(-gd * gd) * opacity * fade;
            }
            //SHAPE_SHADER_CASE
            case 20u: {
                let t = i32(r.amount.y);
                var chosen = vec2<f32>(0.0);
                if t >= 0 && u32(t) < lk {
                    chosen = vec2<f32>(SHAPE_COV_IN[t], SHAPE_COV_OUT[t]);
                }
                if !PIECE_PICTURE {
                    let v = cut_value(r, s.q, upright, vec2<f32>(SHAPE_COV_IN[below], SHAPE_COV_OUT[below]), chosen, stack_in, stack_out, unit_aa);
                    piece = sub_cut(piece, v.xy, v.z * scale);
                    glow_amount *= v.y;
                }
            }
            case 21u: {
                let col = select(own_c, piece_colour(piece), flag);
                piece = sub_glow(piece, r, col, fade, stack_in, stack_out, mode);
            }
            case 22u: {
                let lohi = sub_outline_band(r, max(r.p0.x, 0.0));
                let col = select(own_c, piece_colour(piece) * 0.6, flag);
                piece = sub_outline(piece, r, col, lohi, aa);
            }
            case 23u: {
                // The layer's shape (and its shape cuts above this) where the shadow falls from:
                // the point the offset back, on the screen or turning with the shape.
                var off = r.p0.xy;
                if r.p0.w > 0.5 {
                    off = rotate(off, layered_shape.a.z);
                }
                let world_s = world - off;
                let s_s = shape_at(world_s);
                var sd_s = shape_layer_sd(ll, s_s);
                let up = shape_upright(world_s);
                for (var i = lk + 1u; i < k; i++) {
                    let e = look_layers[first + i];
                    if e.head.x == 20u {
                        sd_s = max(sd_s, cut_distance(e, s_s.q, up) * scale);
                    }
                }
                piece = sub_shadow(piece, r, sd_s, aa);
            }
            case 24u: {
                let amount = shading_factor(r, s.q, upright, unit_aa) * clamp(r.p0.w, 0.0, 1.0) * opacity;
                piece = sub_shading(piece, own_c, amount);
            }
            case 25u: {
                let light = select(vec3<f32>(1.0), own_c, flag);
                let li = shine_material(lit_at, piece.c_in, r.p0.xy, r.p0.z, r.p0.w, r.p1.x, light);
                let lo = shine_material(lit_at, piece.c_out, r.p0.xy, r.p0.z, r.p0.w, r.p1.x, light);
                piece = piece_recoloured(piece, li, lo, clamp(r.amount.y, 0.0, 1.0) * opacity);
            }
            case 27u: {
                piece = sub_recolour(piece, r);
            }
            case 28u: {
                piece = sub_flash(piece, r, layered_shape.hit);
            }
            case 29u: {
                piece = sub_hue(piece, r, layered_shape.hit);
            }
            default: {}
        }
        if !sub {
            // The piece: the layer as worked out; its sub-layers go over it next. A glow is light.
            open = true;
            lk = k;
            ll = r;
            piece = Piece(c, a_in, c, a_out, vec3<f32>(0.0), vec3<f32>(0.0), shape_layer_sd(r, s));
            glow_amount = 0.0;
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

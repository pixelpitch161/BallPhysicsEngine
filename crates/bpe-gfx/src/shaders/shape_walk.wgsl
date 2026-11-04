// One walk over a shape's layer records, bottom to top, at world point `world` (`s` where it is
// against the outline): records `first` to `first + count`. Pasted in by `layers::source` with
// SHAPE_WALK named and SHAPE_SHADER_CASE either empty or the shader layers' case: a scene
// without shader layers, and the walk shader layers sample (which can't call itself), leave
// shader records out.

fn SHAPE_WALK(world: vec2<f32>, s: ShapeAt, first: u32, count_in: u32) -> Walked {
    // One output pixel, in world pixels.
    let aa = g.view.z;
    var inner = vec4<f32>(0.0);
    var outer = vec4<f32>(0.0);
    let count = min(count_in, MAX_LAYERS);
    for (var k = 0u; k < count; k++) {
        let l = look_layers[first + k];
        let mode = l.head.y;
        let flag = (l.head.z & 1u) != 0u;
        let opacity = clamp(l.amount.x, 0.0, 1.0);
        // The colour of the layers under this one here, as if wholly inside the shape.
        let under = select(vec3<f32>(1.0), inner.rgb / inner.a, inner.a > 1e-5);
        var c = vec3<f32>(0.0);
        var a_in = 0.0;
        var a_out = 0.0;
        switch l.head.x {
            case 0u: {
                c = l.colour.rgb;
                a_in = opacity;
            }
            case 10u: {
                c = shape_colour(l, u32(l.p0.x + 0.5), s, 0.0);
                a_in = opacity;
            }
            case 2u: {
                let w = max(l.p0.x, 0.0);
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
                    // Same as body: the colour under it a little darker, so a white shape shows it.
                    let across = clamp((s.edge - lo) / max(hi - lo, 1e-4), 0.0, 1.0);
                    c = select(shape_colour(l, u32(l.p0.z + 0.5), s, across), under * 0.6, flag);
                    a_in = band(s.edge, lo, hi, aa, true) * opacity;
                    a_out = band(s.edge, lo, hi, aa, false) * opacity;
                }
            }
            case 4u: {
                let gd = max(s.edge, 0.0) / max(l.p0.x, 1.0);
                c = select(shape_colour(l, u32(l.p0.z + 0.5), s, clamp(gd, 0.0, 1.0)), under, flag);
                a_out = max(l.amount.y, 0.0) * exp(-gd * gd) * opacity * shape_reach_fade(s);
            }
            //SHAPE_SHADER_CASE
            default: {}
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
    }
    return Walked(inner, outer);
}

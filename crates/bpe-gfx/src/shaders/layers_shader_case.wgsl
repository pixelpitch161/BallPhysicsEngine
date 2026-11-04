            case 6u, 26u: {
                // An effect, run with the layers under it as its picture, or, as a sub-layer, its
                // layer's piece so far (the layer and the sub-layers above this one, over black).
                // A sub-layer's picture: its layer whole and uncut, over the stack under it.
                PIECE_PICTURE = sub;
                PIECE_UNDER = select(body_c, inner.rgb / inner.a, inner.a > 1e-5);
                let uv = in_ball_begin(r, select(first, first + lk, sub), select(k, k - lk, sub), p, screen);
                let run = run_effect(u32(r.p0.w), uv, r);
                PIECE_PICTURE = false;
                let e = select(clamp(run, vec3<f32>(0.0), vec3<f32>(64.0)), vec3<f32>(0.0), run != run);
                if sub {
                    piece = piece_recoloured(piece, e, e, opacity);
                } else {
                    c = e;
                    a_in = opacity;
                }
            }
            case 9u: {
                // An effect across the group, over the whole ball and, Around, past its edge.
                if r.p0.z > 0.5 {
                    let uv = group_begin(r, first, k, screen);
                    let run = run_effect(u32(r.p0.w), uv, r);
                    c = select(clamp(run, vec3<f32>(0.0), vec3<f32>(64.0)), vec3<f32>(0.0), run != run);
                    let covered = opacity * group_covers(r, screen);
                    a_in = covered;
                    let around = r.p1.x;
                    if around > 0.0 {
                        let out = max(d, 0.0) / around;
                        // Faded out before the quad's edge, as a glow is.
                        a_out = covered * exp(-3.0 * out * out) * fade;
                    }
                }
            }

            case 6u, 26u: {
                // An effect, run with the layers under it as its picture, or, as a sub-layer, its
                // layer's piece so far (the layer and the sub-layers above this one, over black).
                // A sub-layer's picture: its layer whole and uncut, over the stack under it.
                PIECE_PICTURE = sub;
                PIECE_UNDER = under;
                let uv = shape_begin(r, select(first, first + lk, sub), select(k, k - lk, sub), s, world);
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

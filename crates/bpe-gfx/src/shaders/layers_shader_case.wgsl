            case 6u: {
                // An effect, run with the layers under it as its picture.
                let uv = in_ball_begin(l, first, k, p, screen);
                let run = run_effect(u32(l.p0.w), uv, l);
                c = select(clamp(run, vec3<f32>(0.0), vec3<f32>(64.0)), vec3<f32>(0.0), run != run);
                a_in = opacity;
            }
            case 9u: {
                // An effect across the group, over the whole ball and, Around, past its edge.
                if l.p0.z > 0.5 {
                    let uv = group_begin(l, first, k, screen);
                    let run = run_effect(u32(l.p0.w), uv, l);
                    c = select(clamp(run, vec3<f32>(0.0), vec3<f32>(64.0)), vec3<f32>(0.0), run != run);
                    a_in = opacity;
                    let around = l.p1.x;
                    if around > 0.0 {
                        let out = max(d, 0.0) / around;
                        // Faded out before the quad's edge, as a glow is.
                        let reach = max(layered_ball.form.w, 1.0);
                        let quad = max(abs(local.x), abs(local.y));
                        let fade = 1.0 - smoothstep(0.8 * reach, reach, quad);
                        a_out = opacity * exp(-3.0 * out * out) * fade;
                    }
                }
            }

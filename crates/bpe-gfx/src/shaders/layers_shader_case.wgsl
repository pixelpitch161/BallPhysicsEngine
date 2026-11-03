            case 6u: {
                // An effect, run with the layers under it as its picture.
                let uv = in_ball_begin(l, first, k, p, screen);
                let run = run_effect(u32(l.p0.w), uv, l);
                c = select(clamp(run, vec3<f32>(0.0), vec3<f32>(64.0)), vec3<f32>(0.0), run != run);
                a_in = opacity;
            }

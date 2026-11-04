        // The group shaders that change this layer mix their effect into its colour, in order.
        let shaded = u32(max(l.amount.w, 0.0));
        if l.head.x <= 6u && shaded != 0u {
            let gfirst = u32(max(l.amount.z, 0.0));
            for (var j = 0u; j < 24u; j++) {
                if (shaded & (1u << j)) == 0u || gfirst + j >= count {
                    continue;
                }
                let gs = look_layers[first + gfirst + j];
                if gs.head.x != 9u {
                    continue;
                }
                let uv = group_begin(gs, first, k + 1u + subs, screen);
                let run = run_effect(u32(gs.p0.w), uv, gs);
                let e = select(clamp(run, vec3<f32>(0.0), vec3<f32>(64.0)), vec3<f32>(0.0), run != run);
                c = mix(c, blended(gs.head.y, c, e), clamp(gs.amount.x, 0.0, 1.0) * group_covers(gs, screen));
            }
        }

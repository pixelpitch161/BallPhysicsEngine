// GGX highlight from a light along `l` (unit), times pi so it sits with the diffuse light (which
// has no 1/pi), its roughness widened by the lamp's size `ang` so small lamps don't sparkle.
fn ggx(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, rough: f32, f0: vec3<f32>, ang: f32) -> vec3<f32> {
    let nl = max(dot(n, l), 0.0);
    if nl <= 0.0 {
        return vec3<f32>(0.0);
    }
    let h = normalize(v + l);
    let nv = max(dot(n, v), 1e-3);
    let nh = max(dot(n, h), 0.0);
    let vh = max(dot(v, h), 0.0);
    let a = rough * rough;
    // At least a hair wide, so a near-mirror under a point lamp doesn't sparkle pixel by pixel.
    let aw = min(a + 0.5 * max(ang, 0.02), 1.0);
    let a2 = aw * aw;
    let dd = nh * nh * (a2 - 1.0) + 1.0;
    let d = a2 / (PI * dd * dd) * (a / aw) * (a / aw);
    let k = (rough + 1.0) * (rough + 1.0) / 8.0;
    let g = nl / (nl * (1.0 - k) + k) * nv / (nv * (1.0 - k) + k);
    let f = f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - vh, 5.0);
    return f * (d * g / (4.0 * nv) * PI);
}

// A toon highlight: a hard dot where the light's mirror is.
fn toon_spot(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, rough: f32, f0: vec3<f32>) -> vec3<f32> {
    if dot(n, l) <= 0.0 {
        return vec3<f32>(0.0);
    }
    let nh = max(dot(n, normalize(v + l)), 0.0);
    let on = select(0.0, 1.5, nh > 1.0 - 0.2 * rough * rough - 0.02);
    return min(f0 * 8.0, vec3<f32>(1.0)) * on;
}

// A stretched GGX highlight (Burley's anisotropic one) round unit tangent `t`: `ax` its
// roughness along `t`, `ay` across it; otherwise as `ggx`.
fn ggx_aniso(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, t: vec3<f32>, ax: f32, ay: f32, rough: f32, f0: vec3<f32>) -> vec3<f32> {
    let nl = max(dot(n, l), 0.0);
    if nl <= 0.0 {
        return vec3<f32>(0.0);
    }
    let h = normalize(v + l);
    let b = cross(n, t);
    let nv = max(dot(n, v), 1e-3);
    let nh = max(dot(n, h), 0.0);
    let vh = max(dot(v, h), 0.0);
    let th = dot(t, h) / ax;
    let bh = dot(b, h) / ay;
    let dd = th * th + bh * bh + nh * nh;
    let d = 1.0 / (PI * ax * ay * dd * dd);
    let k = (rough + 1.0) * (rough + 1.0) / 8.0;
    let g = nl / (nl * (1.0 - k) + k) * nv / (nv * (1.0 - k) + k);
    let f = f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - vh, 5.0);
    return f * (d * g / (4.0 * nv) * PI);
}

// A number 0..1 that looks random, the same for the same cell.
fn hash2(c: vec2<f32>) -> f32 {
    return fract(sin(dot(c, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

// Where world point `p` is in the frame flakes and brushed lines are cut in: the ball's own (in
// its radii) on a ball, else world cells of 24 px.
fn surface_local(p: vec2<f32>) -> vec2<f32> {
    if SHADE_BALL.z > 0.0 {
        return (p - SHADE_BALL.xy) / SHADE_BALL.z;
    }
    return p / 24.0;
}

// The colour a thin film of thickness `film` seen at `nv` (cosine to the eye) gives the shine:
// white with no film.
fn film_colour(film: f32, nv: f32) -> vec3<f32> {
    if film <= 0.0 {
        return vec3<f32>(1.0);
    }
    let phase = film * 4.0 * (1.2 - nv);
    let irid = 0.5 + 0.5 * cos(6.2831853 * (vec3<f32>(phase) + vec3<f32>(0.0, 0.33, 0.67)));
    return mix(vec3<f32>(1.0), irid * 1.6, clamp(film * 1.5, 0.0, 1.0));
}

// Glass and light part 4b, dichroic (dichroic.rs's twin): the hue light meeting a surface of
// dichroic numbers `g` (SurfaceGpu.g: amount, shift and hue in turns) at `cos_t` (to its normal,
// 1 straight on) takes.
fn dichroic_hue(g: vec4<f32>, cos_t: f32) -> vec3<f32> {
    let h = g.z + g.y * (1.0 - clamp(abs(cos_t), 0.0, 1.0));
    return clamp(abs(fract(vec3<f32>(h) + vec3<f32>(0.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - 3.0) - 1.0, vec3<f32>(0.0),
        vec3<f32>(1.0));
}

// What goes through a dichroic surface, per channel (exactly 1 when it isn't dichroic).
fn dichroic_through(g: vec4<f32>, cos_t: f32) -> vec3<f32> {
    let d = clamp(g.x, 0.0, 1.0);
    if d <= 0.0 {
        return vec3<f32>(1.0);
    }
    return mix(vec3<f32>(1.0), dichroic_hue(g, cos_t), d);
}

// The colour light bounced off a dichroic surface takes: the opposite hue at full strength
// (exactly 1 when it isn't dichroic).
fn dichroic_back(g: vec4<f32>, cos_t: f32, share: f32) -> vec3<f32> {
    let d = clamp(g.x, 0.0, 1.0) * share;
    if d <= 0.0 {
        return vec3<f32>(1.0);
    }
    return mix(vec3<f32>(1.0), vec3<f32>(1.0) - dichroic_hue(g, cos_t), d);
}

// What a point takes from the lights and the room.
struct Shaded {
    // Diffuse light (ambient, sky and lamps, with shadows).
    diffuse: vec3<f32>,
    // Highlights and reflections, already coloured by the surface's Fresnel.
    spec: vec3<f32>,
    // Light coming through a see-through surface (no turning to the lamp).
    through: vec3<f32>,
    // Light soaking through from behind (Soak), to be tinted by the colour.
    soaked: vec3<f32>,
    // How much it glows from inside here (Inner glow).
    glow: f32,
};

// Lights the point `spot` (world point `p` on the picture) wearing surface `sf`, whose plain
// colour is `base` (for metal), `here` the blockers' field there.
// `link` is the link number of what is there (Lights up): a light that leaves it out gives none.
fn shade(spot: Spot, p: vec2<f32>, sf: SurfaceGpu, base: vec3<f32>, here: f32, link: f32) -> Shaded {
    let stage = in_stage();
    let px = u.view.z;
    let n = spot.n;
    let v = view_dir();
    let h = spot.p.z;
    let rough = clamp(sf.a.x, 0.02, 1.0);
    let metal = clamp(sf.a.y, 0.0, 1.0);
    let coat = max(sf.a.z, 0.0);
    let toon = sf.b.y;
    let nv0 = max(dot(n, v), 0.0);
    // Part 2b's numbers: thin film, soak, flakes, brushing.
    let film = film_colour(max(sf.c.z, 0.0), nv0);
    // Part 4b: a dichroic surface's shine takes the hue opposite what goes through it.
    let f0 = mix(vec3<f32>(max(sf.b.x, 0.0)), clamp(base, vec3<f32>(0.0), vec3<f32>(1.0)), metal) * film
        * dichroic_back(sf.g, nv0, 1.0);
    let fire = clamp(sf.g.w, 0.0, 1.0);
    let soak = clamp(sf.c.y, 0.0, 1.0);
    let flakes = clamp(sf.c.w, 0.0, 1.0);
    var flake_n = n;
    var flake_on = 0.0;
    if flakes > 0.0 {
        let cell = floor(surface_local(spot.p.xy) / max(sf.d.x, 0.005));
        if hash2(cell) < flakes {
            flake_on = 1.0;
            let tilt = vec3<f32>(hash2(cell + 7.0), hash2(cell + 3.0), hash2(cell + 11.0)) - vec3<f32>(0.5);
            flake_n = normalize(n + tilt * 0.7);
        }
    }
    let flake_f0 = mix(vec3<f32>(0.9), clamp(base, vec3<f32>(0.0), vec3<f32>(1.0)), 0.5);
    let brush = clamp(sf.d.y, 0.0, 1.0);
    var tangent = vec3<f32>(1.0, 0.0, 0.0);
    var lines = 1.0;
    if brush > 0.0 {
        let local = surface_local(spot.p.xy);
        var t2 = vec2<f32>(cos(sf.d.w), sin(sf.d.w));
        var across = dot(local, vec2<f32>(-t2.y, t2.x));
        if sf.d.z < 0.5 {
            // Spun: round and round the middle.
            let r = length(local);
            t2 = select(vec2<f32>(1.0, 0.0), vec2<f32>(-local.y, local.x) / max(r, 1e-4), r > 1e-4);
            across = r;
        }
        let t3 = vec3<f32>(t2, 0.0);
        let along = t3 - n * dot(n, t3);
        tangent = select(vec3<f32>(1.0, 0.0, 0.0), normalize(along), dot(along, along) > 1e-8);
        lines = 1.0 + 0.3 * brush * (hash2(vec2<f32>(floor(across * 70.0), 1.0)) - 0.5);
    }
    let shiny = metal > 0.0 || sf.b.x > 0.0 || coat > 0.0 || flakes > 0.0;
    let style = u.shadows.x;
    var out: Shaded;
    out.diffuse = vec3<f32>(0.0);
    out.spec = vec3<f32>(0.0);
    out.through = vec3<f32>(0.0);
    out.soaked = vec3<f32>(0.0);
    out.glow = max(sf.e.z, 0.0) * (0.35 + 0.65 * nv0 * nv0);
    let near = lights_near(spot.p.xy);
    for (var k = 0u; k < near.count; k++) {
        let i = light_in(near, k);
        let l = lights[i];
        if !lights_up(l, link) {
            continue;
        }
        let kind = l.p0.w;
        var amount = 1.0;
        var toward = vec2<f32>(0.0);
        var dist = 0.0;
        var dir3 = vec3<f32>(0.0, 0.0, 1.0);
        var dist3 = 1e6;
        var angular = 0.0;
        if kind > 1.5 && kind < 2.5 {
            // Sun: from far away against its direction, lifted by its elevation (p0.z: sin).
            let el_sin = clamp(l.p0.z, -1.0, 1.0);
            let el_cos = sqrt(max(1.0 - el_sin * el_sin, 0.0));
            toward = -l.p3.xy;
            dist = 1e6;
            dir3 = normalize(vec3<f32>(toward * el_cos, el_sin));
            angular = max(l.p6.y, 0.0);
        } else {
            let at = source_point(l, spot.p.xy);
            let d = at - spot.p.xy;
            dist = length(d);
            let to_lamp = vec3<f32>(d, l.p0.z - h);
            let len3 = length(to_lamp);
            if stage {
                amount = falloff(len3, l.p2.x, l.p2.y);
            } else {
                amount = falloff(dist, l.p2.x, l.p2.y);
            }
            if amount == 0.0 {
                continue;
            }
            if kind > 0.5 && kind < 1.5 {
                // Spot: inside its cone round where it points, measured across the picture.
                let away = select(vec2<f32>(0.0), -d / dist, dist > 1e-3);
                let cosine = select(dot(away, l.p3.xy), 1.0, dist <= 1e-3);
                // smoothstep written out: a cone with no soft edge has both ends the same.
                let e = clamp((cosine - l.p3.w) / max(l.p3.z - l.p3.w, 1e-6), 0.0, 1.0);
                amount *= e * e * (3.0 - 2.0 * e);
                if amount == 0.0 {
                    continue;
                }
            }
            toward = select(vec2<f32>(0.0), d / dist, dist > 1e-3);
            dir3 = select(vec3<f32>(0.0, 0.0, 1.0), to_lamp / len3, len3 > 1e-6);
            dist3 = len3;
            if stage {
                angular = max(l.p2.z, 0.0) / max(len3, 1e-3);
            } else {
                angular = max(l.p2.z, 0.0) / max(dist, 1e-3);
            }
        }
        // Flat: the picture plane takes the light whole and round surfaces turn to it or away.
        // Stage: as a real surface does.
        var cosine = 1.0 + dot(n, dir3) - dir3.z;
        if stage {
            cosine = dot(n, dir3);
        }
        var turned = max(cosine, 0.0);
        if soak > 0.0 {
            // Light soaks round: wrapped past the edge the light falls on.
            turned = mix(turned, max((cosine + soak) / (1.0 + soak), 0.0), soak);
        }
        var tint = vec3<f32>(1.0);
        if l.p5.w > 0.5 && style < 2.5 && (dist > 1e-3 || stage) {
            var soft = angular * max(u.shadows.y, 0.0) * max(l.p6.w, 0.0);
            if style > 0.5 {
                soft = 0.5 * px / max(select(dist, dist3, stage), 1e-3);
            }
            // The march stops short of the lamp by its size, a ridden ball's radius and however
            // deep in a blocker the lamp sits (a strip along a shape's band, a light in a ball).
            var stop = l.p6.x + max(l.p2.z, 0.0);
            if kind < 1.5 || kind > 2.5 {
                stop = max(stop, -field_at(source_point(l, select(p, spot.p.xy, stage))));
            }
            // An Area or Strip light's shadows come from all of it (as in `lamps`): on a Stage
            // from the middle of what it covers at its height, as wide as that looks from here.
            var s_toward = toward;
            var s_dist = dist;
            var s_dir3 = dir3;
            var s_dist3 = dist3;
            var src = -1;
            if kind > 2.5 && kind < 4.5 {
                let seen_from = select(p, spot.p.xy, stage);
                let ex = extended_seen(l, seen_from);
                if ex.ang > 0.0 {
                    let to3 = vec3<f32>(ex.way * ex.dist, l.p0.z - h);
                    s_dist3 = max(length(to3), 1e-3);
                    s_dir3 = to3 / s_dist3;
                    if stage {
                        // (To its middle, but never into a blocker the light sits in.)
                        stop = max(l.p6.x + 0.5 * px, -field_at(l.p0.xy));
                    } else {
                        stop = ex.stop;
                        s_toward = ex.way;
                        s_dist = ex.dist;
                        src = i32(i);
                    }
                    let wide = ex.ang * select(ex.dist, ex.dist * ex.dist / s_dist3, stage) / ex.dist;
                    if style > 0.5 {
                        soft = 0.5 * px / max(select(ex.dist, s_dist3, stage), 1e-3);
                    } else {
                        soft = min(max(wide, angular), MOST_SEEN) * max(u.shadows.y, 0.0) * max(l.p6.w, 0.0);
                    }
                }
            }
            var v = vec3<f32>(1.0);
            bent_carried = kind < 4.5;
            if stage {
                v = sphere_shadow(spot.p, s_dir3, s_dist3, soft, stop) * slab_visibility(spot.p, s_dir3, s_dist3, soft, stop);
            } else {
                v = visibility(p, s_toward, s_dist, soft, u.shadows.z, stop, src);
            }
            bent_carried = true;
            tint = mix(vec3<f32>(1.0), l.p5.rgb, clamp((vec3<f32>(1.0) - v) * l.p2.w, vec3<f32>(0.0), vec3<f32>(1.0)));
        }
        // Part 4: its gobo, and an ultraviolet light's visible share only.
        amount *= gobo(l, spot.p.xy);
        if l.p7.y > 0.5 {
            amount *= max(l.p7.z, 0.0);
        }
        var strength = amount * turned;
        if style > 1.5 && style < 2.5 {
            // Toon: the light in bands.
            let bands = max(u.more.y, 1.0);
            strength = floor(strength * bands + 0.5) / bands;
        }
        if toon > 0.5 {
            strength = floor(strength * toon + 0.5) / toon;
        }
        out.diffuse += l.p1.rgb * strength * tint;
        out.through += l.p1.rgb * amount * tint;
        if soak > 0.0 {
            // From behind, through the thing (its own turning is the only thing in the way).
            out.soaked += l.p1.rgb * amount * max(-dot(n, dir3), 0.0) * soak;
        }
        if shiny {
            var sp = vec3<f32>(0.0);
            if toon > 0.5 {
                sp = toon_spot(n, v, dir3, rough, f0);
            } else {
                if brush > 0.0 {
                    let ax = clamp(rough * rough + 0.5 * max(angular, 0.02), 0.002, 1.0);
                    let ay = min(ax * (1.0 + 6.0 * brush), 1.0);
                    sp = ggx_aniso(n, v, dir3, tangent, ax, ay, rough, f0) * lines;
                } else {
                    // Part 4b, diamond fire: the highlight split into 3 to 7 colours, each bent a
                    // little its own way across the light, mixed over the plain one (the last of
                    // the loop, so `ggx` has one call site here; without Fire the loop is it).
                    let k = select(0u, 3u + u32(round(4.0 * fire)), fire > 0.0);
                    let across = cross(n, dir3);
                    let t3 = select(vec3<f32>(1.0, 0.0, 0.0), normalize(across), dot(across, across) > 1e-8);
                    var plain = vec3<f32>(0.0);
                    var glints = vec3<f32>(0.0);
                    var sum = vec3<f32>(0.0);
                    for (var j = 0u; j <= k; j++) {
                        var nj = n;
                        var c = vec3<f32>(0.0);
                        if j < k {
                            let w = (f32(j) + 0.5) / f32(k) * 2.0 - 1.0;
                            c = spectral(w);
                            nj = normalize(n + t3 * (w * 0.12 * fire));
                        }
                        let g = ggx(nj, v, dir3, rough, f0, angular);
                        if j < k {
                            glints += c * g;
                            sum += c;
                        } else {
                            plain = g;
                        }
                    }
                    sp = plain;
                    if k > 0u {
                        sp = mix(plain, glints / max(sum, vec3<f32>(1e-4)) * (1.0 + 2.0 * fire), fire);
                    }
                }
                if coat > 0.0 {
                    sp = sp * (1.0 - 0.04 * coat) + ggx(n, v, dir3, 0.05, vec3<f32>(0.04), angular) * coat;
                }
                if flake_on > 0.0 {
                    sp += ggx(flake_n, v, dir3, 0.2, flake_f0, angular) * (2.0 * flakes);
                }
            }
            out.spec += l.p1.rgb * amount * tint * sp;
        }
    }
    // The room: ambient and the sky's light from above.
    var room = u.ambient.rgb;
    if st.sky_top.w != 0.0 {
        room += st.sky_top.w * mix(st.sky_bottom.rgb, st.sky_top.rgb, 0.5 - 0.5 * n.y);
    }
    out.diffuse += room;
    out.through += room;
    // Velvet: a soft bright rim from all round.
    if sf.e.y != 0.0 {
        // Dimmer where it faces the eye, bright where it turns away (fibres seen side on).
        let facing = mix(1.0, 0.5 + 0.5 * (1.0 - nv0), clamp(sf.e.y, 0.0, 1.0));
        out.diffuse = out.diffuse * facing + sf.e.y * pow(1.0 - nv0, 2.0) * out.through;
    }
    // Pearl: the film's colours in the light it takes too.
    out.diffuse *= mix(vec3<f32>(1.0), film, 0.35);
    // Dichroic (part 4b): a sheen of the opposite hue that turns with the angle to the eye, and
    // what comes through takes the hue.
    if sf.g.x > 0.0 {
        out.diffuse *= dichroic_back(sf.g, nv0, 0.35);
        out.through *= dichroic_through(sf.g, nv0);
    }
    // What a shiny surface mirrors: the sky, blurred by roughness, or the scene round it.
    if shiny && sf.b.z > 0.5 {
        let r = reflect(-v, n);
        let blurred = normalize(mix(r, n, rough * rough));
        let edge = pow(1.0 - nv0, 5.0);
        let room_sky = u.ambient.rgb + vec3<f32>(st.sky_top.w);
        var env = sky(blurred) * room_sky;
        if sf.b.z > 1.5 && !NO_MIRRORS {
            if baking() {
                // The bake (part 3): one ray a sample, turned by the roughness, at any roughness;
                // the samples average into a blurred reflection.
                let jit = vec3<f32>(rand(), rand(), rand()) * 2.0 - vec3<f32>(1.0);
                let turned = normalize(r + jit * (rough * rough * 1.5));
                env = trace_scene(spot.p + n * 0.5, select(r, turned, dot(turned, n) > 0.0), SHADE_SPHERE);
            } else {
                // A rough mirror keeps the sky: blurred balls would need many rays.
                let k = 1.0 - smoothstep(0.15, 0.35, rough);
                if k > 0.0 {
                    env = mix(env, trace_scene(spot.p + n * 0.5, r, SHADE_SPHERE), k);
                }
            }
        }
        let fe = f0 + (max(vec3<f32>(1.0 - rough), f0) - f0) * edge;
        out.spec += env * fe;
        if coat > 0.0 {
            out.spec += sky(r) * room_sky * (0.04 + 0.96 * edge) * coat;
        }
    }
    // Contact shading: light dims close to a blocker (and, in Stage, under and beside balls).
    if u.shadows.w != 0.0 {
        let reach = max(u.more.x, 1e-3);
        var keep = 1.0;
        if here > 0.0 {
            keep = 1.0 - u.shadows.w * (1.0 - smoothstep(0.0, reach, here));
        }
        if stage {
            keep *= 1.0 - u.shadows.w * (1.0 - sphere_ao(spot.p, n));
        }
        out.diffuse *= keep;
        out.through *= keep;
        out.spec *= keep;
        out.soaked *= keep;
    }
    return out;
}

// What a reflection sees of a point `q` facing `n`, seen from along `v` (toward the eye),
// wearing `sf`, coloured `c`: the lamps without shadows, the room, highlights and (with `env`) the
// sky's mirror; a mirror that the trace bounces on from leaves that out.
fn hit_light(q: vec3<f32>, n: vec3<f32>, v: vec3<f32>, sf: SurfaceGpu, c: vec3<f32>, env: bool) -> vec3<f32> {
    let rough = clamp(sf.a.x, 0.02, 1.0);
    let metal = clamp(sf.a.y, 0.0, 1.0);
    let f0 = mix(vec3<f32>(max(sf.b.x, 0.0)), clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), metal);
    let shiny = metal > 0.0 || sf.b.x > 0.0;
    var diffuse = u.ambient.rgb;
    if st.sky_top.w != 0.0 {
        diffuse += st.sky_top.w * mix(st.sky_bottom.rgb, st.sky_top.rgb, 0.5 - 0.5 * n.y);
    }
    var spec = vec3<f32>(0.0);
    let near = lights_near(q.xy);
    for (var k = 0u; k < near.count; k++) {
        let i = light_in(near, k);
        let l = lights[i];
        let kind = l.p0.w;
        var amount = 1.0;
        var dir3 = vec3<f32>(0.0, 0.0, 1.0);
        if kind > 1.5 && kind < 2.5 {
            let el_sin = clamp(l.p0.z, -1.0, 1.0);
            dir3 = normalize(vec3<f32>(-l.p3.xy * sqrt(max(1.0 - el_sin * el_sin, 0.0)), el_sin));
        } else {
            let d = source_point(l, q.xy) - q.xy;
            let to_lamp = vec3<f32>(d, l.p0.z - q.z);
            let len3 = length(to_lamp);
            amount = falloff(len3, l.p2.x, l.p2.y);
            if kind > 0.5 && kind < 1.5 {
                let dist = length(d);
                let cosine = select(dot(-d / max(dist, 1e-3), l.p3.xy), 1.0, dist <= 1e-3);
                let e = clamp((cosine - l.p3.w) / max(l.p3.z - l.p3.w, 1e-6), 0.0, 1.0);
                amount *= e * e * (3.0 - 2.0 * e);
            }
            dir3 = select(vec3<f32>(0.0, 0.0, 1.0), to_lamp / len3, len3 > 1e-6);
        }
        if amount == 0.0 {
            continue;
        }
        diffuse += l.p1.rgb * amount * max(dot(n, dir3), 0.0);
        if shiny {
            spec += l.p1.rgb * amount * ggx(n, v, dir3, rough, f0, 0.05);
        }
    }
    if env && shiny && sf.b.z > 0.5 {
        let r = reflect(-v, n);
        let edge = pow(1.0 - max(dot(n, v), 0.0), 5.0);
        let fe = f0 + (max(vec3<f32>(1.0 - rough), f0) - f0) * edge;
        spec += sky(normalize(mix(r, n, rough * rough))) * (u.ambient.rgb + vec3<f32>(st.sky_top.w)) * fe;
    }
    return c * max(diffuse, vec3<f32>(0.0)) * (1.0 - metal) + spec + c * max(sf.e.z, 0.0);
}

// The picture's colour where 3D point `q` shows on screen (black off the picture).
fn picture_at(q: vec3<f32>) -> vec3<f32> {
    // Off the picture the wall and floor go on as they are at its edge.
    let uv = clamp(world_to_uv(vec2<f32>(q.x, q.y + q.z * st.room.w)), vec2<f32>(0.0), vec2<f32>(1.0));
    return textureSampleLevel(picture, samp, uv, 0.0).rgb;
}

// What a mirror ray from `o` along unit `d` sees (Reflects: The scene): the balls (sphere `skip`
// left out: the ray's own), and on a Stage the back wall, the floor and the shapes' fronts; off a
// mirror-like ball the ray bounces on, up to Reflection depth; past everything the sky. What it
// meets takes its colour from the picture where it shows (a ball's far side its Body colour) and
// the light there without shadows.
fn trace_scene(o: vec3<f32>, d: vec3<f32>, skip_first: i32) -> vec3<f32> {
    var ro = o;
    var rd = d;
    var skip = skip_first;
    var carried = vec3<f32>(1.0);
    var found = vec3<f32>(0.0);
    let room_sky = u.ambient.rgb + vec3<f32>(st.sky_top.w);
    let depth = clamp(i32(st.dof.w), 1, 16);
    let stage = in_stage();
    for (var bounce = 0; bounce < 16; bounce++) {
        if bounce >= depth {
            found += carried * sky(rd) * room_sky;
            break;
        }
        let ball = hit_ball(ro, rd, skip);
        var t = ball.x;
        // 0 nothing, 1 a ball, 2 the wall, 3 the floor, 4 a shape's front.
        var what = select(0, 1, ball.y >= 0.0);
        if stage {
            if rd.z < -1e-5 {
                let tw = (-st.room.y - ro.z) / rd.z;
                if tw > 1e-3 && tw < t {
                    t = tw;
                    what = 2;
                }
            }
            if st.floor.x > 0.5 && rd.y > 1e-5 {
                let tf = (floor_y() - ro.y) / rd.y;
                if tf > 1e-3 && tf < t && (ro.z + rd.z * tf) >= -st.room.y {
                    t = tf;
                    what = 3;
                }
            }
            if rd.z < -1e-5 && ro.z > st.room.z {
                let ts = (st.room.z - ro.z) / rd.z;
                if ts > 1e-3 && ts < t && field_on_slab(ro + rd * ts) < 0.0 {
                    t = ts;
                    what = 4;
                }
            }
        }
        if what == 0 {
            found += carried * sky(rd) * room_sky;
            break;
        }
        let q = ro + rd * t;
        let v = -rd;
        if what == 1 {
            let at = u32(ball.y);
            let sp = spheres[at];
            let n = normalize(q - vec3<f32>(sp.a.xy, 0.0));
            let index = u32(max(sp.c.x, 0.0));
            let sf = surfaces[select(0u, index, index < u32(st.more.z))];
            var c = sp.b.rgb;
            if n.z > 0.2 {
                c = picture_at(vec3<f32>(q.xy, 0.0));
            }
            let mirror = sf.b.z > 1.5 && sf.a.x < 0.35 && (sf.a.y > 0.0 || sf.b.x > 0.0);
            if mirror && bounce + 1 < depth {
                // On off this mirror, carrying its Fresnel colour.
                found += carried * hit_light(q, n, v, sf, c, false);
                let f0 = mix(vec3<f32>(max(sf.b.x, 0.0)), clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), clamp(sf.a.y, 0.0, 1.0));
                let edge = pow(1.0 - max(dot(n, v), 0.0), 5.0);
                carried *= f0 + (max(vec3<f32>(1.0 - sf.a.x), f0) - f0) * edge;
                ro = q + n * 0.05;
                rd = reflect(rd, n);
                skip = i32(at);
                continue;
            }
            found += carried * hit_light(q, n, v, sf, c, true);
            break;
        }
        var n = vec3<f32>(0.0, 0.0, 1.0);
        var sf = plane_surface(st.floor.w);
        if what == 3 {
            n = vec3<f32>(0.0, -1.0, 0.0);
            sf = plane_surface(st.floor.z);
        } else if what == 4 {
            let uv = world_to_uv(vec2<f32>(q.x, q.y + q.z * st.room.w));
            let size = vec2<f32>(textureDimensions(surface));
            sf = surface_of(textureLoad(surface, vec2<i32>(clamp(uv, vec2<f32>(0.0), vec2<f32>(0.999)) * size), 0));
        }
        found += carried * hit_light(q, n, v, sf, picture_at(q), true);
        break;
    }
    return found;
}

// The lit colour of a pixel of colour `c` (lit share `w`) shaded as `sh` with surface `sf`, of
// which `share` takes highlights. With `refracts` its see-through share is left to the caller
// (what glass shows behind it).
fn mixed(c: vec3<f32>, w: vec3<f32>, sh: Shaded, sf: SurfaceGpu, share: f32, refracts: bool) -> vec3<f32> {
    let metal = clamp(sf.a.y, 0.0, 1.0);
    let see = clamp(sf.a.w, 0.0, 1.0);
    var diffuse = sh.diffuse;
    if see > 0.0 && !refracts {
        diffuse = sh.diffuse * (1.0 - see) + sh.through * (0.6 * see);
    }
    var out = c + (diffuse - vec3<f32>(1.0)) * w * (1.0 - metal) - w * metal + sh.spec * share;
    // Light soaked through from behind, tinted by the colour, and the glow from inside.
    out += w * (sh.soaked * 0.8 + vec3<f32>(sh.glow));
    return out;
}

// Shaded `sh` with the light field's `field` added. Negative light (darkness) takes light away,
// but the sum of every light stays at 0 or more, as in the Flat mix.
fn with_field(sh: Shaded, field: vec3<f32>) -> Shaded {
    var out = sh;
    out.diffuse = max(sh.diffuse + field, vec3<f32>(0.0));
    out.through = max(sh.through + field, vec3<f32>(0.0));
    out.spec = max(sh.spec, vec3<f32>(0.0));
    return out;
}

// The light an uncovered pixel at world `q` (on screen) takes: the room, the lamps (Stage: on
// the wall or floor it shows, with shadows), the light field and a glowing background.
fn open_light(q: vec2<f32>) -> vec3<f32> {
    let uv = clamp(world_to_uv(q), vec2<f32>(0.0), vec2<f32>(1.0));
    var light = vec3<f32>(0.0);
    if in_stage() {
        let spot = stage_spot(q);
        let floor = st.floor.x > 0.5 && spot.n.y < -0.5;
        let plane = plane_surface(select(st.floor.w, st.floor.z, floor));
        NO_MIRRORS = true;
        let size = vec2<f32>(textureDimensions(surface));
        let link = link_here(vec2<i32>(min(uv * size, size - 1.0)));
        light = shade(spot, q, plane, vec3<f32>(1.0), field_on_slab(spot.p), link).diffuse;
        NO_MIRRORS = false;
        // The background's own light (in Flat the direct light holds it already).
        if st.counts.z > 0.5 {
            light += textureSampleLevel(above, samp, uv, 0.0).rgb;
        }
    } else {
        light = u.ambient.rgb + textureSampleLevel(direct_light, samp, uv, 0.0).rgb;
    }
    if gu.on.x > 0.5 {
        light += field_mean(uv * gu.size.xy);
    }
    return max(light, vec3<f32>(0.0));
}

// The behind picture (the scene without what is see-through) at world `q`, blurred over `frost`.
fn behind_at(q: vec2<f32>, frost: f32, turn: f32) -> vec3<f32> {
    if frost < 0.5 {
        return textureSampleLevel(behind, samp, clamp(world_to_uv(q), vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb;
    }
    var sum = vec3<f32>(0.0);
    for (var k = 0; k < FROST_TAPS; k++) {
        let a = f32(k) * 2.39996 + turn;
        let o = vec2<f32>(cos(a), sin(a)) * frost * sqrt((f32(k) + 0.5) / f32(FROST_TAPS));
        sum += textureSampleLevel(behind, samp, clamp(world_to_uv(q + o), vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb;
    }
    return sum / f32(FROST_TAPS);
}

// What a see-through surface `sf` at world `p` (height `h`, facing `n`) shows behind it: the view
// bent by its Bends light through its thickness (each colour its own way with Rainbow split),
// blurred by Frost, lit as an open pixel is there, tinted by its colour `tint`.
fn transmitted(p: vec2<f32>, n: vec3<f32>, h: f32, sf: SurfaceGpu, tint: vec3<f32>, turn: f32) -> vec3<f32> {
    let v = view_dir();
    let ior = max(sf.e.w, 1.0);
    let thick = 2.0 * max(h, 0.0) + 8.0;
    let split = max(sf.c.x, 0.0) * 0.04;
    let frost = max(sf.e.x, 0.0) * (16.0 + max(SHADE_BALL.z, h));
    if baking() && split > 0.0 {
        // The bake (part 3): one wavelength a sample, which the samples average into a smooth
        // rainbow (red bends least).
        let w = rand() * 2.0 - 1.0;
        let eta = min(1.0 / (ior * (1.0 + split * w)), 1.0);
        var d = refract(-v, n, eta);
        if dot(d, d) < 1e-6 {
            d = reflect(-v, n);
        }
        let q = p + d.xy / max(-d.z, 0.15) * thick;
        return behind_at(q, frost, turn) * spectral(w) * open_light(q) * tint;
    }
    var col = vec3<f32>(0.0);
    var middle = p;
    for (var ch = 0; ch < 3; ch++) {
        let eta = min(1.0 / (ior * (1.0 + split * f32(ch - 1))), 1.0);
        var d = refract(-v, n, eta);
        if dot(d, d) < 1e-6 {
            d = reflect(-v, n);
        }
        let q = p + d.xy / max(-d.z, 0.15) * thick;
        if ch == 1 {
            middle = q;
        }
        if split <= 0.0 && ch != 1 {
            continue;
        }
        col[ch] = behind_at(q, frost, turn)[ch];
    }
    if split <= 0.0 {
        col = behind_at(middle, frost, turn);
    }
    return col * open_light(middle) * tint;
}


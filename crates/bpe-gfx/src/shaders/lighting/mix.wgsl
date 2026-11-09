// ---------- the mix ----------

// The lit share of colour `c` that takes highlights: its lit share (a black thing shines whole).
fn shine_share(c: vec3<f32>, w: vec3<f32>) -> f32 {
    let top = max(c.r, max(c.g, c.b));
    if top <= 1e-4 {
        return 1.0;
    }
    return clamp(max(w.r, max(w.g, w.b)) / top, 0.0, 1.0);
}

// Whether surface `sf` takes light other than as a matte sheet does.
fn shines(sf: SurfaceGpu) -> bool {
    return sf.b.x > 0.0 || sf.a.y > 0.0 || sf.a.z > 0.0 || sf.a.w > 0.0 || sf.b.y > 0.5
        || sf.c.y > 0.0 || sf.c.z > 0.0 || sf.c.w > 0.0 || sf.e.y > 0.0 || sf.e.z > 0.0 || sf.g.x > 0.0;
}

// Part 2a's light at a pixel: the thing's surface (shiny, or on the stage) and, in Stage, the
// wall or floor behind it, each shaded by `shade` (the lamps, the room, contact shading) with the
// light field's `field` added. `diffuse` is the light as the editor's views show it.
struct Surfaced {
    out: vec3<f32>,
    diffuse: vec3<f32>,
};

// The light field's light on a ball in Stage (part 2b: the field sees balls as opaque, so it is
// read just outside the ball all round, each way weighted by how much the surface faces it).
fn field_round_ball(ball: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    var weights = 0.0;
    let out = ball.z + 2.0 * max(u.field.z, 1e-3);
    for (var k = 0; k < 8; k++) {
        let a = f32(k) * 0.7853982;
        let dir = vec2<f32>(cos(a), sin(a));
        let q = ball.xy + dir * out;
        let w = max(dot(n.xy, dir), 0.0) + 0.35 * max(n.z, 0.0) + 1e-3;
        sum += field_mean(clamp(world_to_uv(q), vec2<f32>(0.0), vec2<f32>(1.0)) * gu.size.xy) * w;
        weights += w;
    }
    return sum / weights;
}

// Glass and light part 4a: a mirror ball's tiles, the ball seen from its pole and turned by
// `turn`: rings 2 pi / `facets` wide from the middle out, each of max(1, round(facets sin θ))
// tiles (so the rim ring has `facets`, as the photons' tiles). The normal of the tile unit normal
// `n` (z toward the viewer) falls in, and in w how near the tile's edge it is (0 its middle, 1 its
// edge).
fn facet_normal(n: vec3<f32>, facets: f32, turn: f32) -> vec4<f32> {
    let f = max(facets, 3.0);
    let wide = 6.28318531 / f;
    let theta = acos(clamp(n.z, -1.0, 1.0));
    let ring = floor(theta / wide);
    let tc = min((ring + 0.5) * wide, 1.5707963);
    // The rim ring has the Facets tiles the photons see (seen edge on, a ball is a ring of them).
    let rim = (ring + 1.0) * wide > 1.5707963;
    let count = select(max(1.0, round(f * sin(tc))), f, rim);
    let phi = atan2(n.y, n.x) - turn;
    let sector = 6.28318531 / count;
    let pc = (floor(phi / sector) + 0.5) * sector + turn;
    let across = select(abs(2.0 * fract(phi / sector) - 1.0), 0.0, count < 1.5);
    // The rim ring's outer edge is the ball's own edge, not a gap.
    let up = select(abs(2.0 * fract(theta / wide) - 1.0), 0.0, rim);
    return vec4<f32>(sin(tc) * cos(pc), sin(tc) * sin(pc), cos(tc), max(across, up));
}

// How much of a tile's edge (`edge`, from `facet_normal`) is gap between tiles, of Facet gaps
// `gaps`: 1 in the gap, 0 on the tile, softened over a little.
fn facet_gap(edge: f32, gaps: f32) -> f32 {
    if gaps <= 0.0 {
        return 0.0;
    }
    let g = clamp(gaps, 0.0, 1.0);
    return smoothstep(1.0 - g - 0.04, 1.0 - g + 0.01, edge);
}

fn surfaced(ip: vec2<i32>, p: vec2<f32>, c: vec3<f32>, w: vec3<f32>, sf: SurfaceGpu, field_in: vec3<f32>, turn: f32) -> Surfaced {
    let px = u.view.z;
    let s = textureLoad(surface, ip, 0);
    let share = shine_share(c, w);
    // A glowing background's own light, which 1b adds inside the direct light (part 2b).
    var bg = vec3<f32>(0.0);
    if st.counts.z > 0.5 {
        bg = textureLoad(above, ip, 0).rgb;
    }
    let field = field_in + bg;
    // The thing's surface's normal from its height's slope (flat where nothing round is).
    let h = height_at(ip);
    let hx = height_at(ip + vec2<i32>(1, 0)) - height_at(ip - vec2<i32>(1, 0));
    let hy = height_at(ip + vec2<i32>(0, 1)) - height_at(ip - vec2<i32>(0, 1));
    var slope = vec2<f32>(hx, hy) / (2.0 * px);
    if u.camera.w != 0.0 {
        slope = rotate_by(slope, u.camera.w);
    }
    let is_shape = sf.b.w > 1.5;
    var thing: Spot;
    thing.p = vec3<f32>(p, h);
    thing.n = normalize(vec3<f32>(-slope, 1.0));
    // The ball under the pixel, for flakes, brushed lines and its own reflection.
    SHADE_SPHERE = -1;
    SHADE_BALL = vec3<f32>(0.0);
    if sf.b.w > 0.5 && sf.b.w < 1.5 {
        SHADE_SPHERE = ball_at(p);
        if SHADE_SPHERE >= 0 {
            let sp = spheres[u32(SHADE_SPHERE)];
            SHADE_BALL = sp.a.xyz;
        }
    }
    // A mirror ball (part 4a): each tile a flat mirror facing its own way, dark gaps between.
    var gap = 0.0;
    if sf.f.y >= 3.0 && SHADE_SPHERE >= 0 {
        let tile = facet_normal(thing.n, sf.f.y, spheres[u32(SHADE_SPHERE)].c.z);
        thing.n = normalize(tile.xyz);
        gap = facet_gap(tile.w, sf.f.z);
    }
    let grout = 1.0 - 0.85 * gap;
    let here = field_at(p);
    let link = link_here(ip);
    let base = c / max(share, 1e-3);
    let see = clamp(sf.a.w, 0.0, 1.0);
    let refracts = see > 0.0 && st.counts.y > 0.5;
    let tint = clamp(base / max(max(base.r, max(base.g, base.b)), 1e-3), vec3<f32>(0.0), vec3<f32>(1.0));
    var out: Surfaced;
    if !in_stage() {
        var sh = shade(thing, p, sf, base, here, link);
        sh = with_field(sh, field);
        out.out = mixed(c, w, sh, sf, share, refracts);
        if refracts {
            let body = mixed(c, w, Shaded(sh.diffuse, vec3<f32>(0.0), sh.through, sh.soaked, sh.glow), sf, share, true);
            // Part 4b: seen through dichroic glass, the picture behind takes its hue.
            let seen = transmitted(p, thing.n, h, sf, tint * dichroic_through(sf.g, dot(thing.n, view_dir())), turn);
            out.out = body * (1.0 - see) + seen * see + sh.spec * share;
        }
        out.out *= grout;
        out.diffuse = sh.diffuse;
        return out;
    }
    // A shape's front stands at the shapes' depth (sheared as its slab); a ball's surface at its
    // height.
    thing.p = thing_point(p, h, is_shape);
    let cover = clamp(s.a, 0.0, 1.0);
    var on_thing = Surfaced(vec3<f32>(0.0), vec3<f32>(0.0));
    var on_stage = Surfaced(vec3<f32>(0.0), vec3<f32>(0.0));
    if cover > 0.02 {
        var thing_field = field;
        if st.counts.w > 0.5 && SHADE_BALL.z > 0.0 {
            thing_field = field_round_ball(SHADE_BALL, thing.n) + bg;
        }
        var sh = shade(thing, p, sf, base, here, link);
        sh = with_field(sh, thing_field);
        on_thing = Surfaced(mixed(c, w, sh, sf, share, refracts), sh.diffuse);
        if refracts {
            let body = mixed(c, w, Shaded(sh.diffuse, vec3<f32>(0.0), sh.through, sh.soaked, sh.glow), sf, share, true);
            // Part 4b: seen through dichroic glass, the picture behind takes its hue.
            let seen = transmitted(p, thing.n, h, sf, tint * dichroic_through(sf.g, dot(thing.n, view_dir())), turn);
            on_thing.out = body * (1.0 - see) + seen * see + sh.spec * share;
        }
        on_thing.out *= grout;
    }
    if cover < 0.98 {
        SHADE_SPHERE = -1;
        SHADE_BALL = vec3<f32>(0.0);
        let spot = stage_spot(p);
        let floor = st.floor.x > 0.5 && spot.n.y < -0.5;
        let plane = plane_surface(select(st.floor.w, st.floor.z, floor));
        // How far the shapes are, for contact shading: the floor in front of them is that much
        // farther away; under a shape (between the wall and its front) it is as close as can be.
        let f = field_on_slab(spot.p);
        let dz = max(spot.p.z - st.room.z, 0.0);
        let near = select(max(f, 1e-3), length(vec2<f32>(max(f, 0.0), dz)), dz > 0.0);
        var sh = shade(spot, p, plane, c, near, link);
        sh = with_field(sh, field);
        // The wall and floor take light as much as the background does (its lit share).
        on_stage = Surfaced(mixed(c, w, sh, plane, share, false), sh.diffuse);
    }
    let k = smoothstep(0.02, 0.98, cover);
    out.out = mix(on_stage.out, on_thing.out, k);
    out.diffuse = mix(on_stage.diffuse, on_thing.diffuse, k);
    return out;
}

// How many wavelengths the rainbow is summed over.
const BOW_COLOURS: i32 = 16;

// The rainbow at world `p` (part 4b), per channel, before its strength and the light: the
// primary bow (40.6° violet to 42.3° red), the sky glowing inside it, the secondary (50.4° red to
// 53.6° violet, × Second bow × 0.43, a little wider) and Alexander's dark band between, each
// wavelength coloured by `spectral` and the sum normalised by the colours (so where every
// wavelength is the same it is white). θ is 42° × the distance to the centre / Rainbow size.
fn rainbow_bow(p: vec2<f32>) -> vec3<f32> {
    let size = max(gu.rainbow.y, 1.0);
    let theta = 42.0 * length(p - gu.rainbow_at.xy) / size;
    if theta > 75.0 {
        return vec3<f32>(0.0);
    }
    let second = clamp(gu.rainbow.z, 0.0, 1.0) * 0.43;
    let dark = clamp(gu.rainbow.w, 0.0, 1.0);
    var sum = vec3<f32>(0.0);
    var colours = vec3<f32>(0.0);
    for (var k = 0; k < BOW_COLOURS; k++) {
        let w = -1.0 + 2.0 * (f32(k) + 0.5) / f32(BOW_COLOURS);
        let along = 0.5 * (w + 1.0);
        let tp = 42.3 - 1.7 * along;
        let ts = 50.4 + 3.2 * along;
        let dp = (theta - tp) / 0.6;
        let ds = (theta - ts) / 0.9;
        let peak = exp(-0.5 * dp * dp) + second * exp(-0.5 * ds * ds);
        let inside = 1.0 - smoothstep(tp - 0.6, tp + 0.6, theta);
        let outside = smoothstep(ts - 0.9, ts + 0.9, theta);
        let near = min(abs(theta - tp), abs(theta - ts));
        // The glow fades out before the cut at 75°, so there is no edge.
        let glow = 0.25 * exp(-near / 12.0) * (1.0 - smoothstep(60.0, 75.0, theta))
            * (inside + (1.0 - dark) * (1.0 - inside) * (1.0 - outside) + second * outside);
        let c = spectral(w);
        sum += c * (peak + glow);
        colours += c;
    }
    return sum / max(colours, vec3<f32>(1e-6));
}

// The lit colour at pixel `ip` (screen point `uv`): everything fs_mix does for one pixel.
fn lit_at(ip: vec2<i32>, uv: vec2<f32>, turn: f32) -> vec4<f32> {
    let c = textureLoad(picture, ip, 0);
    // The lit share of the colour: never more than the colour itself.
    let w = clamp(textureLoad(lit_picture, ip, 0).rgb, vec3<f32>(0.0), max(c.rgb, vec3<f32>(0.0)));
    let p = uv_to_world(uv);
    let sv = textureLoad(surface, ip, 0);
    var field = vec3<f32>(0.0);
    var field_air = vec3<f32>(0.0);
    if gu.on.x > 0.5 {
        let fp = uv * gu.size.xy;
        // Blocked or not as the pixel itself is (a field texel can straddle an edge); in Stage a
        // ball blocks the field too (part 2b).
        let cb = cover_block(sv);
        let blocks_here = cb > 0.5 || (st.counts.w > 0.5 && cb > 0.12);
        field = field_light(vec2<i32>(fp), fp, blocks_here);
        field_air = field_mean(uv * gu.size.xy);
    }
    let direct = textureLoad(direct_light, ip, 0);
    // Underwater: caustics play over the light, and deep down the water's colour takes over.
    let water = gu.water.x;
    var under = 0.0;
    var deep = 0.0;
    var water_m = vec3<f32>(1.0);
    if water != 0.0 {
        let below = p.y - gu.water_depth.x;
        if below > 0.0 {
            under = water;
            deep = 1.0 - exp(-below / max(gu.water_depth.y, 1e-3));
            let q = (p - vec2<f32>(0.0, gu.water_depth.x)) / max(gu.water.z, 1e-3);
            let ca = caustics(q, u.view.w * gu.water.w);
            water_m *= max(1.0 + gu.water.y * under * (ca - 0.35) * (1.0 - 0.7 * deep), 0.0);
            water_m *= mix(vec3<f32>(1.0), gu.water_colour.rgb, clamp(under * deep, 0.0, 1.0));
        }
    }
    let sf = surface_of(sv);
    var light = vec3<f32>(0.0);
    var out = vec3<f32>(0.0);
    if in_stage() || shines(sf) {
        // Part 2a: surfaces that shine, and the stage.
        let done = surfaced(ip, p, c.rgb, w, sf, field, turn);
        light = done.diffuse * water_m;
        out = done.out + done.diffuse * (water_m - vec3<f32>(1.0)) * w;
    } else {
        light = max(u.ambient.rgb + direct.rgb + field, vec3<f32>(0.0));
        // Contact shading: light dims close to a blocker.
        let here = field_at(p);
        if u.shadows.w != 0.0 && here > 0.0 {
            let reach = max(u.more.x, 1e-3);
            light *= 1.0 - u.shadows.w * (1.0 - smoothstep(0.0, reach, here));
        }
        light *= water_m;
        out = c.rgb + (light - vec3<f32>(1.0)) * w;
    }
    // The editor's views of the light alone.
    if u.ambient.w > 1.5 {
        return vec4<f32>(heat(dot(light, vec3<f32>(0.2126, 0.7152, 0.0722))), 1.0);
    }
    if u.ambient.w > 0.5 {
        return vec4<f32>(clamp(light, vec3<f32>(-6.0e4), vec3<f32>(6.0e4)), 1.0);
    }
    var ex = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    if gu.more_on.z > 0.5 {
        ex = textureSampleLevel(extras, samp, uv, 0.0);
    }
    if gu.on.z > 0.5 {
        // The haze: the light in the air, over what it veils, as thick as it is here.
        let density = select(1.0, ex.a, gu.more_on.w > 0.5);
        let veil = clamp(gu.air.w * density, 0.0, 1.0) * min(max(gu.air.r, max(gu.air.g, gu.air.b)), 1.0);
        let air = max(textureLoad(air_light, ip, 0).rgb + field_air, vec3<f32>(0.0));
        out = out * (1.0 - veil) + gu.air.rgb * air * density;
        if gu.more_on.y > 0.5 {
            // Smoke that catches beams glows its own colour by the light in the air.
            let smoke = max(textureLoad(tags, ip, 0).r, 0.0);
            out += air * smoke * max(c.rgb, vec3<f32>(0.0));
        }
    }
    if gu.rainbow.x > 0.0 {
        // Part 4b: a rainbow where something catches it, the haze or rain that makes rainbows,
        // as bright as the light falling there.
        var catches = 0.0;
        if gu.on.z > 0.5 {
            let thick = max(gu.air.r, max(gu.air.g, gu.air.b));
            catches += clamp(thick * select(1.0, ex.a, gu.more_on.w > 0.5), 0.0, 1.0);
        }
        if gu.rainbow_at.z > 0.5 {
            catches += max(textureLoad(tags, ip, 0).b, 0.0);
        }
        catches = min(catches, 1.0);
        if catches > 0.0 {
            let lamp = min(dot(max(direct.rgb + field, vec3<f32>(0.0)), vec3<f32>(0.2126, 0.7152, 0.0722)), 2.0);
            out += rainbow_bow(p) * gu.rainbow.x * lamp * catches;
        }
    }
    if under > 0.0 && gu.water_colour.w != 0.0 {
        // Shafts of sunlight down through the water.
        out += gu.water_colour.rgb * gu.water_colour.w * under * (1.0 - deep) * sun_rays(p, gu.water_depth.x, u.view.w);
    }
    if gu.more_on.x > 0.5 {
        // What glows under UV, by the ultraviolet light falling on it.
        out += max(direct.a, 0.0) * max(textureLoad(uv_pic, ip, 0).rgb, vec3<f32>(0.0));
    }
    if gu.memory.x != 0.0 {
        // Glow in the dark: the light remembered, where the background shows (or everywhere).
        let open = select(1.0 - clamp(textureLoad(surface, ip, 0).a, 0.0, 1.0), 1.0, gu.memory.w > 0.5);
        var glow = ex.rgb;
        if gu.memory_colour.w < 0.5 {
            glow = gu.memory_colour.rgb * dot(ex.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
        }
        out += glow * gu.memory.x * open;
    }
    out += laser_beams(p);
    if gu.on.y > 0.5 {
        let b = 0.35 * textureSampleLevel(glow_pic, samp, uv, 0.0).rgb
            + 0.3 * textureSampleLevel(glow_pic, samp, uv, 1.0).rgb
            + 0.2 * textureSampleLevel(glow_pic, samp, uv, 2.0).rgb
            + 0.15 * textureSampleLevel(glow_pic, samp, uv, 3.0).rgb;
        out += gu.glow.z * b;
    }
    // Kept finite (a huge Strength would overflow the half-float picture) and never below black.
    return vec4<f32>(clamp(out, vec3<f32>(0.0), vec3<f32>(6.0e4)), c.a);
}

// Where a mirror wall shows something at world `p` (part 2b, `fs_mirror`): the point it shows, how strongly,
// tinted how; `k` 0 where no mirror shows.
struct Mirrored {
    p: vec2<f32>,
    k: f32,
    tint: vec3<f32>,
};

// The nearest mirror wall on whose mirrored side `p` is: its outline's nearest point q (and
// whether p is inside, by crossings), the depth past the band's face e, squeezed to
// e / (1 + squeeze e / S) (on a circle of radius S at squeeze 1, exactly a circle inversion), and
// the mirrored point that far past the face on the other side.
// A side's unit normal (to its left, going a to b; every side of one outline the same way round).
fn side_normal(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    let d = b - a;
    return normalize(vec2<f32>(-d.y, d.x) + vec2<f32>(1e-9, 0.0));
}

fn mirrored(p: vec2<f32>) -> Mirrored {
    var out = Mirrored(p, 0.0, vec3<f32>(1.0));
    var best = 1e30;
    let count = min(u32(st.counts.x), 16u);
    for (var mi = 0u; mi < count; mi++) {
        let m = mirrors[mi];
        let first = u32(m.a.x);
        let n = min(u32(m.a.y), MOST_MIRROR_SEGMENTS);
        var near = 1e30;
        var q = p;
        var at = 0u;
        var crossings = 0u;
        for (var k = 0u; k < n; k++) {
            let seg = segments[first + k];
            let a = seg.xy;
            let b = seg.zw;
            let c = nearest_on(p, a, b);
            let d = length(p - c);
            if d < near {
                near = d;
                q = c;
                at = k;
            }
            if (a.y > p.y) != (b.y > p.y) {
                let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
                if x > p.x {
                    crossings += 1u;
                }
            }
        }
        let inside = (crossings & 1u) == 1u;
        let side = m.a.z;
        let wants = (inside && side > 1.5) || (!inside && (side < 1.5 || side > 2.5));
        let e = near - m.b.z;
        if !wants || e <= 0.0 || near >= best || near < 1e-4 {
            continue;
        }
        best = near;
        let squeezed = e / (1.0 + max(m.b.x, 0.0) * e / max(m.b.y, 1e-3));
        // The way across is the outline's normal blended between its corners (each corner's the
        // mean of its two sides'), so a circle cut into sides mirrors as one smooth curve, not a
        // fan of flat pieces.
        let seg = segments[first + at];
        let before = segments[first + (at + n - 1u) % n];
        let after = segments[first + (at + 1u) % n];
        let side_n = side_normal(seg.xy, seg.zw);
        let na = normalize(side_normal(before.xy, before.zw) + side_n + vec2<f32>(1e-9, 0.0));
        let nb = normalize(side_n + side_normal(after.xy, after.zw) + vec2<f32>(1e-9, 0.0));
        let ab = seg.zw - seg.xy;
        let t = clamp(dot(q - seg.xy, ab) / max(dot(ab, ab), 1e-12), 0.0, 1.0);
        var way = normalize(mix(na, nb, t) + vec2<f32>(1e-9, 0.0));
        if dot(way, q - p) < 0.0 {
            way = -way;
        }
        out.p = q + way * (m.b.z + squeezed);
        out.k = clamp(m.a.w, 0.0, 1.0);
        out.tint = m.c.rgb;
    }
    return out;
}

@fragment
fn fs_mix(in: FsIn) -> @location(0) vec4<f32> {
    let ip = vec2<i32>(in.pos.xy);
    seed_rng(ip, 1u);
    // Interleaved gradient noise (Jimenez): a different turn of frost's taps at each pixel.
    let turn = 6.2831853 * fract(52.9829189 * fract(dot(in.pos.xy, vec2<f32>(0.06711056, 0.00583715))));
    return lit_at(ip, in.uv, turn);
}

// Mirror walls (part 2b): after the mix, `picture` is the lit picture; a pixel on a mirror's
// mirrored side shows it again where the mirror sends it, sampled smoothly, over its own light.
@fragment
fn fs_mirror(in: FsIn) -> @location(0) vec4<f32> {
    let own = textureLoad(picture, vec2<i32>(in.pos.xy), 0);
    let m = mirrored(uv_to_world(in.uv));
    let uv2 = world_to_uv(m.p);
    if m.k <= 0.0 || any(uv2 < vec2<f32>(0.0)) || any(uv2 > vec2<f32>(1.0)) {
        return own;
    }
    let seen = textureSampleLevel(picture, samp, uv2, 0.0).rgb * m.tint;
    return vec4<f32>(mix(own.rgb, seen, m.k), own.a);
}


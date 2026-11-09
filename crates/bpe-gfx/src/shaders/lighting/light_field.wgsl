// ---------- the light field ----------

// `t` sampled at the field texel `fp` (field texel units), as the mean of four taps across it,
// so a field smaller than the picture sees all of it.
fn over_texel(t: texture_2d<f32>, fp: vec2<f32>) -> vec4<f32> {
    let size = gu.size.xy;
    var sum = vec4<f32>(0.0);
    for (var k = 0; k < 4; k++) {
        let o = vec2<f32>(f32(k & 1) - 0.5, f32(k >> 1u) - 0.5) * 0.5;
        sum += textureSampleLevel(t, samp, (fp + o) / size, 0.0);
    }
    return sum * 0.25;
}

// The light field's light from its four ways at field point `fp` (in texels), smoothly between
// texels.
fn field_ways_at(fp: vec2<f32>) -> array<vec3<f32>, 4> {
    let uv = fp / gu.size.xy;
    let a = textureSampleLevel(gi_a, samp, uv, 0.0);
    let b = textureSampleLevel(gi_b, samp, uv, 0.0);
    let c = textureSampleLevel(gi_c, samp, uv, 0.0);
    return array<vec3<f32>, 4>(a.rgb, b.rgb, c.rgb, vec3<f32>(a.a, b.a, c.a));
}

// The light field's mean light at field point `fp`.
fn field_mean(fp: vec2<f32>) -> vec3<f32> {
    let w = field_ways_at(fp);
    return 0.25 * (w[0] + w[1] + w[2] + w[3]);
}

// The light field's light at field point `fp` (texel `ip`): the mean, from every way, or, where
// something blocks light (`blocked`), the light reaching its nearest edge from outside: the ways
// facing out of it there, read a little way out so the thing's own light (or bounce) never
// lights itself, and so a thing's side facing a glow lights up. Negative light (darkness) stays
// negative; the mix keeps the sum of every light at 0 or more.
fn field_light(ip: vec2<i32>, fp: vec2<f32>, blocked: bool) -> vec3<f32> {
    let size = vec2<i32>(gu.size.xy);
    let at = clamp(ip, vec2<i32>(0), size - 1);
    let s = textureLoad(seeds, at, 0);
    if !blocked || s.z < 0.0 || s.x < 0.0 {
        return field_mean(fp);
    }
    // Where the light is read from, and the way out of the thing there: from inside, the nearest
    // open texel's middle; from a pixel inside it on an open texel (the texel straddles its
    // edge), here, away from the nearest texel that blocks.
    var read = s.zw + 0.5;
    var n = read - fp;
    if !texel_blocked(at) {
        read = fp;
        n = fp - (s.xy + 0.5);
    }
    if dot(n, n) < 1e-6 {
        n = vec2<f32>(0.0, -1.0);
    }
    n = normalize(n);
    let w = field_ways_at(clamp(read + n * 1.5, vec2<f32>(0.5), gu.size.xy - 0.5));
    var sum = vec3<f32>(0.0);
    var weights = 0.0;
    for (var d = 0; d < 4; d++) {
        let angle = 2.0 * PI * (f32(d) + 0.5) / 4.0;
        // Each way is stored as where its light comes from.
        let k = max(dot(vec2<f32>(cos(angle), sin(angle)), n), 0.0);
        sum += w[d] * k;
        weights += k;
    }
    return sum / max(weights, 1e-4);
}

// How much the blocker nearest field texel `ip` blocks (the lighting's own field's G).
fn field_opacity(ip: vec2<i32>) -> f32 {
    let size = vec2<i32>(gu.size.xy);
    return opacity_of(textureLoad(field, clamp(ip, vec2<i32>(0), size - 1), 0).yzw);
}

// Whether field texel `ip` is inside something that blocks light (the lighting's own field).
fn texel_blocked(ip: vec2<i32>) -> bool {
    let size = vec2<i32>(gu.size.xy);
    let at = clamp(ip, vec2<i32>(0), size - 1);
    let s = textureLoad(seeds, at, 0);
    return s.x >= 0.0 && s.x == f32(at.x) && s.y == f32(at.y) && s.z >= 0.0;
}

// What the light field is made from, at each field texel: the light things give off, the light
// lit things that block light bounce, and whether it is opaque (blocks light or gives it).
@fragment
fn fs_gi_scene(in: FsIn) -> @location(0) vec4<f32> {
    let fp = in.pos.xy;
    let ec = over_texel(emission, fp);
    let e = ec.rgb;
    let field_size = gu.size.xy;
    // The pictures at full size, four taps across the texel.
    var w = vec3<f32>(0.0);
    var light = vec3<f32>(0.0);
    var blocks = 0.0;
    // The blocking taps' lit colour and light alone (each by how much it blocks).
    var own_w = vec3<f32>(0.0);
    var own_light = vec3<f32>(0.0);
    for (var k = 0; k < 4; k++) {
        let o = vec2<f32>(f32(k & 1) - 0.5, f32(k >> 1u) - 0.5) * 0.5;
        let uv = (fp + o) / field_size;
        let c = textureSampleLevel(picture, samp, uv, 0.0);
        let lw = textureSampleLevel(lit_picture, samp, uv, 0.0).rgb;
        let wk = clamp(lw, vec3<f32>(0.0), max(c.rgb, vec3<f32>(0.0)));
        let lk = textureSampleLevel(direct_light, samp, uv, 0.0).rgb;
        w += wk;
        light += lk;
        let sv = textureSampleLevel(surface, samp, uv, 0.0);
        var b = cover_block(sv);
        // In Stage a ball blocks light as a sphere, not in the blockers' field: a quarter marks
        // it here (part 2b), so the light field stops at balls too.
        if gu.on.w > 0.5 && b < 0.3 {
            b = min(b * 4.0, 1.0);
        }
        blocks += b;
        own_w += wk * b;
        own_light += lk * b;
    }
    let block_sum = max(blocks, 1e-3);
    own_w /= block_sum;
    own_light /= block_sum;
    w *= 0.25;
    light *= 0.25;
    blocks = clamp(blocks * 0.25, 0.0, 1.0);
    if gu.cascade.w > 0.5 {
        // The second run: the first run's light bounces too.
        let at = vec2<i32>(fp);
        let more = field_light(at, fp, texel_blocked(at));
        light += more;
        own_light += more;
    }
    var bounced = gu.glow.x * w * max(light, vec3<f32>(0.0)) * blocks;
    // Something giving off light is opaque where it covers most of the texel, or where it is
    // light covering nothing (a glow, an added trail); a texel it only grazes stays open, so the
    // light round its edge isn't stopped there by a sliver of it.
    let bright = max(abs(e.r), max(abs(e.g), abs(e.b)));
    let gives = (ec.a > 0.5 && bright > 1e-4) || (ec.a < 0.05 && bright > 0.02);
    // A line thinner than the texel is opaque where the lighting's own field (which keeps any
    // texel a blocking pixel touches) says so, or the light would leak through it.
    // A see-through blocker less than half opaque stays open, as a wide one does.
    let thin = blocks < 0.5 && texel_blocked(vec2<i32>(fp)) && field_opacity(vec2<i32>(fp)) >= 0.5;
    let opaque = select(step(0.5, blocks), 1.0, gives || thin);
    if thin {
        // Opaque though it only grazes the texel: it bounces as the blocker it grazes does, or
        // it would stand dark round the edge of what lights the field and hide it.
        bounced = gu.glow.x * own_w * max(own_light, vec3<f32>(0.0));
    }
    // A thing's light as it gives it where it covers the texel only partly.
    let own = select(e, e / max(ec.a, 0.5), ec.a > 0.5);
    return vec4<f32>(own + bounced, opaque);
}

@fragment
fn fs_seed_gi(in: FsIn) -> @location(0) vec4<f32> {
    let here = floor(in.pos.xy);
    let s = textureLoad(gi_scene, vec2<i32>(here), 0);
    if s.a > 0.5 {
        return vec4<f32>(here, -1.0, -1.0);
    }
    return vec4<f32>(-1.0, -1.0, here);
}

@fragment
fn fs_gi_dist(in: FsIn) -> @location(0) vec4<f32> {
    let here = floor(in.pos.xy);
    let s = textureLoad(seeds, vec2<i32>(here), 0);
    if s.x < 0.0 {
        return vec4<f32>(1e4, 0.0, 0.0, 1.0);
    }
    return vec4<f32>(distance(s.xy, here), 0.0, 0.0, 1.0);
}

// Where cascade `i`'s first ray starts and its last ends, in field texels (the first reaches 1).
fn interval(i: f32) -> vec2<f32> {
    let a = pow(4.0, i);
    return vec2<f32>((a - 1.0) / 3.0, (4.0 * a - 1.0) / 3.0);
}

// The light cascade `i` (as `size` probes across, `side` directions a side) stores at direction
// `d`, probe `probe`.
fn cascade_texel(d: u32, side: u32, probes: vec2<u32>, probe: vec2<u32>) -> vec2<i32> {
    let cell = vec2<u32>(d % side, d / side);
    return vec2<i32>(cell * probes + probe);
}

// One cascade: for each of its probes and directions, the light along that direction's stretch of
// ray (where it first meets something opaque, that thing's light), then, if nothing stopped it,
// what the cascade above sees beyond (its four probes round this one, the four directions inside
// this one's).
@fragment
fn fs_cascade(in: FsIn) -> @location(0) vec4<f32> {
    let i = u32(gu.cascade.x);
    let count = u32(gu.cascade.y);
    let steps = i32(gu.cascade.z);
    let padded = vec2<u32>(gu.size.zw);
    let spacing = 1u << i;
    let probes = padded / spacing;
    let side = 2u << i;
    let t = vec2<u32>(in.pos.xy);
    let cell = t / probes;
    let probe = t % probes;
    let d = cell.y * side + cell.x;
    let dirs = side * side;
    let angle = 2.0 * PI * (f32(d) + 0.5) / f32(dirs);
    let dir = vec2<f32>(cos(angle), sin(angle));
    let origin = (vec2<f32>(probe) + 0.5) * f32(spacing);
    let span = interval(f32(i));
    let field_size = gu.size.xy;
    let texel_world = u.field.z;
    let reach = gu.glow.y;
    var rad = vec3<f32>(0.0);
    var open = 1.0;
    var along = span.x;
    // Glass and light part 1: with the tint picture drawn, what the see-through blockers the ray
    // passes through let through of each channel (the blockers' field's colour, read on the way
    // through; they stay open in the light field), and the colour of the one it is in now.
    let tinted = u.trace.w > 0.5;
    var trans = vec3<f32>(1.0);
    var col = vec3<f32>(1.0);
    var in_glass = false;
    // Longer stretches take more steps; a ray that runs out of them before its stretch ends
    // counts as stopped, so light never leaks through what it didn't reach.
    // Steps held short by coloured glass don't count (so where nothing is coloured the light
    // field is exactly as without the tint picture), within twice the steps.
    let budget = steps + 4 * i32(i);
    var used = 0;
    var done = false;
    for (var k = 0; k < budget + select(0, steps, tinted); k++) {
        if used >= budget {
            break;
        }
        if along >= span.y {
            done = true;
            break;
        }
        let q = origin + dir * along;
        if any(q < vec2<f32>(0.0)) || any(q >= field_size) {
            // Off the picture nothing is drawn: no light comes from beyond.
            open = 0.0;
            break;
        }
        let qi = vec2<i32>(q);
        var to_blocker = 1e4;
        if tinted {
            let fb = textureLoad(field, qi, 0);
            if fb.r < 0.0 {
                col = min(col, colour_of(fb.yzw));
                in_glass = true;
            } else if in_glass {
                trans *= col;
                col = vec3<f32>(1.0);
                in_glass = false;
            }
            // Only a coloured blocker holds the steps short.
            if any(colour_of(fb.yzw) < vec3<f32>(1.0)) {
                to_blocker = fb.r / max(texel_world, 1e-6);
            }
        }
        let gap = textureLoad(gi_dist, qi, 0).r;
        if gap < 0.5 {
            let s = textureLoad(gi_scene, qi, 0);
            // The light fades out over the glow reach (from the probe).
            let x = along * texel_world / max(reach, 1e-3);
            let w = clamp(1.0 - x * x, 0.0, 1.0);
            rad = s.rgb * w * w * select(0.0, 1.0, reach > 0.0) * trans * col;
            open = 0.0;
            break;
        }
        // Tinted, no further than the nearest coloured blocker, so thin glass on the way isn't
        // stepped over.
        let free = max(gap - 1.0, 0.75);
        let held = max(to_blocker, 0.75);
        used += select(1, 0, held < free);
        along += min(free, held);
    }
    if in_glass {
        trans *= col;
    }
    if open > 0.0 && !done && along < span.y {
        open = 0.0;
    }
    if open > 0.0 && i + 1u < count {
        // The four probes of the cascade above round this one, each the mean of its four
        // directions within this one.
        let up_probes = padded / (spacing * 2u);
        let up_side = side * 2u;
        let pos = origin / f32(spacing * 2u) - 0.5;
        let base = floor(pos);
        let f = pos - base;
        var merged = vec3<f32>(0.0);
        for (var c = 0; c < 4; c++) {
            let o = vec2<f32>(f32(c & 1), f32(c >> 1u));
            let pj = vec2<u32>(clamp(base + o, vec2<f32>(0.0), vec2<f32>(up_probes) - 1.0));
            let wgt = select(1.0 - f.x, f.x, o.x > 0.5) * select(1.0 - f.y, f.y, o.y > 0.5);
            var sum = vec3<f32>(0.0);
            for (var k = 0u; k < 4u; k++) {
                sum += textureLoad(above, cascade_texel(d * 4u + k, up_side, up_probes, pj), 0).rgb;
            }
            merged += sum * 0.25 * wgt;
        }
        rad += open * merged * trans;
    }
    return vec4<f32>(rad, 1.0);
}

struct Three {
    @location(0) a: vec4<f32>,
    @location(1) b: vec4<f32>,
    @location(2) c: vec4<f32>,
};

// The light at each field texel from cascade 0 (one probe a texel, four directions), from each
// way (see `gi_a`).
@fragment
fn fs_gi_resolve(in: FsIn) -> Three {
    let probe = vec2<u32>(in.pos.xy);
    let probes = vec2<u32>(gu.size.zw);
    var l: array<vec3<f32>, 4>;
    for (var d = 0u; d < 4u; d++) {
        l[d] = GI_GAIN * textureLoad(above, cascade_texel(d, 2u, probes, probe), 0).rgb;
    }
    return Three(vec4<f32>(l[0], l[3].r), vec4<f32>(l[1], l[3].g), vec4<f32>(l[2], l[3].b));
}


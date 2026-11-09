// ---------- part 4: haze that moves, light that is remembered ----------

// What stirs the haze: each ball's place and velocity (world), then its radius.
struct Stir {
    a: vec4<f32>,
    b: vec4<f32>,
};

@group(0) @binding(12) var<storage, read> stirs: array<Stir>;

struct StirOut {
    @builtin(position) pos: vec4<f32>,
    // Where in its stirring circle (-1 to 1), its velocity.
    @location(0) local: vec2<f32>,
    @location(1) vel: vec2<f32>,
};

// One ball's stirring circle: three radii and a little more the faster it goes.
@vertex
fn vs_stir(@builtin(vertex_index) i: u32, @builtin(instance_index) k: u32) -> StirOut {
    let s = stirs[k];
    let corner = vec2<f32>(f32(i & 1u), f32((i >> 1u) & 1u)) * 2.0 - 1.0;
    let reach = 3.0 * max(s.b.x, 1.0) + 0.15 * length(s.a.zw);
    let uv = world_to_uv(s.a.xy + corner * reach);
    var out: StirOut;
    out.pos = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    out.local = corner;
    out.vel = s.a.zw;
    return out;
}

@fragment
fn fs_stir(in: StirOut) -> @location(0) vec4<f32> {
    let r2 = dot(in.local, in.local);
    if r2 >= 1.0 {
        discard;
    }
    let k = (1.0 - r2) * (1.0 - r2);
    return vec4<f32>(in.vel * k, k, 0.0);
}

// How thick the moving haze is at world point `p` (about 1 on average): drifting or swirling
// clouds, carried along by the balls that stir it (`stir`: their velocity and how near).
fn haze_density(p: vec2<f32>, stir: vec4<f32>) -> f32 {
    let t = u.view.w;
    let size = max(gu.haze.z, 1e-3);
    let speed = gu.haze.y;
    // The balls carry the haze along behind them, and turn it round them.
    let push = stir.xy * 0.35 * gu.haze_more.z;
    // `haze_more.xy` is how far the clouds have drifted by now (world units, worked out on the
    // CPU: along the drift direction at the speed, or as far as the scene's air carried them).
    var q = (p - push - gu.haze_more.xy) / size;
    if gu.haze.x > 1.5 {
        // Swirling: the cloud bent by a slowly turning field of its own.
        let w = vec2<f32>(
            fbm(q * 0.7 + vec2<f32>(0.0, 0.15 * speed * t)),
            fbm(q * 0.7 + vec2<f32>(5.2, 1.3) - vec2<f32>(0.12 * speed * t, 0.0)),
        );
        let turn = stir.z * gu.haze_more.z * 1.2;
        q += 1.8 * (w - 0.5) + vec2<f32>(-push.y, push.x) / size * turn;
    }
    let n = fbm(q);
    let patchy = clamp(gu.haze.w, 0.0, 1.0);
    return max(mix(1.0, 2.2 * n * n + 0.2 * n, patchy), 0.0);
}

// The light that falls on field texel `fp` (world `p`), for glow in the dark: the lamps, what
// glows, twice the ultraviolet (it charges glowing paint best); no ambient.
fn charging_light(uv: vec2<f32>, fp: vec2<f32>) -> vec3<f32> {
    let d = textureSampleLevel(direct_light, samp, uv, 0.0);
    var l = max(d.rgb, vec3<f32>(0.0)) + vec3<f32>(2.0 * max(d.a, 0.0));
    if gu.on.x > 0.5 {
        l += max(field_mean(fp), vec3<f32>(0.0));
    }
    return l;
}

// The extras picture (field size): the remembered light (rgb) and the moving haze's density (a).
@fragment
fn fs_extras(in: FsIn) -> @location(0) vec4<f32> {
    let p = uv_to_world(in.uv);
    var density = 1.0;
    if gu.more_on.w > 0.5 {
        var stir = vec4<f32>(0.0);
        if gu.haze_more.z != 0.0 {
            stir = textureSampleLevel(stir_pic, samp, in.uv, 0.0);
        }
        density = haze_density(p, stir);
    }
    var memory = vec3<f32>(0.0);
    if gu.memory.x != 0.0 {
        var old = vec3<f32>(0.0);
        if gu.memory_step.z > 0.5 {
            // Where this point was in the last frame's picture.
            let was = world_to_uv_by(p, gu.last_camera);
            if all(was >= vec2<f32>(0.0)) && all(was <= vec2<f32>(1.0)) {
                old = textureSampleLevel(above, samp, was, 0.0).rgb;
            }
        }
        let kept = old * gu.memory_step.x;
        let now = charging_light(in.uv, in.pos.xy);
        // Charging never lowers what is remembered.
        memory = max(kept, mix(kept, now, gu.memory_step.y));
    }
    return vec4<f32>(memory, density);
}

// ---------- the lights' bloom ----------

// The lamps' bulbs (each light at its source, as bright as its light, soft over its size) and
// what gives off light, at the bloom's first level.
@fragment
fn fs_glow_source(in: FsIn) -> @location(0) vec4<f32> {
    let p = uv_to_world(in.uv);
    var c = over_texel(emission, in.pos.xy).rgb;
    let count = u32(u.more.w);
    for (var i = 0u; i < count; i++) {
        let l = lights[i];
        if l.p0.w > 1.5 && l.p0.w < 2.5 {
            continue;
        }
        let d = distance(p, source_point(l, p));
        let x = d / max(l.p2.z, 1.0);
        let b = 1.0 / (1.0 + x * x);
        c += l.p1.rgb * b * b;
    }
    return vec4<f32>(c, 1.0);
}

// The next level of the bloom: the one before, blurred by a tent of nine taps.
@fragment
fn fs_glow_down(in: FsIn) -> @location(0) vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(glow_pic));
    var sum = vec3<f32>(0.0);
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let w = f32((2 - abs(x)) * (2 - abs(y)));
            sum += textureSampleLevel(glow_pic, samp, in.uv + vec2<f32>(f32(x), f32(y)) * texel, 0.0).rgb * w;
        }
    }
    return vec4<f32>(sum / 16.0, 1.0);
}

// ---------- part 4 in the mix ----------

// The sun's rays down through water: slanted, slowly moving shafts, 0 to 1.
fn sun_rays(p: vec2<f32>, top: f32, t: f32) -> f32 {
    let s = p.x + 0.28 * (p.y - top);
    let a = 0.5 + 0.5 * sin(s / 97.0 + 0.45 * t);
    let b = 0.5 + 0.5 * sin(s / 41.0 - 0.7 * t + 1.7);
    let c = 0.5 + 0.5 * sin(s / 173.0 + 0.23 * t + 4.1);
    return pow(a * b * 0.6 + c * 0.4, 3.0);
}

// The laser beams at world point `p`: each laser's line (bright in the middle, a soft halo) and a
// bright spot where it bounces or ends on something.
fn laser_beams(p: vec2<f32>) -> vec3<f32> {
    var out = vec3<f32>(0.0);
    let count = u32(u.more.w);
    for (var i = 0u; i < count; i++) {
        let l = lights[i];
        if l.p0.w < 4.5 {
            continue;
        }
        let first = u32(l.p4.z);
        let n = u32(l.p4.w);
        let width = max(l.p11.x, 0.05);
        var line = 0.0;
        var spot = 0.0;
        for (var k = 0u; k < n; k++) {
            let seg = segments[first + k];
            let d = distance(p, nearest_on(p, seg.xy, seg.zw));
            let x = d / width;
            line = max(line, exp(-x * x) + 0.08 / (1.0 + 0.25 * x * x));
            // A bounce at the start of every segment after the first; the end when it hit something.
            let r = 3.0 * width;
            if k > 0u {
                let e = distance(p, seg.xy) / r;
                spot += exp(-e * e);
            }
            if k + 1u == n && l.p11.z > 0.5 {
                let e = distance(p, seg.zw) / r;
                spot += exp(-e * e);
            }
        }
        out += l.p1.rgb * (l.p7.w * line + l.p11.y * spot);
    }
    return out;
}


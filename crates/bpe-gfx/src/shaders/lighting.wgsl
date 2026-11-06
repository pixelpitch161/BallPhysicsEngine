// Lighting (Flat 2D), run only when the scene has a Lighting card (see lighting.rs).
//
// The things were drawn three times before this runs: the picture as always, the lit colour
// (every layer's colour scaled by its Lit) and the surface (R: blocks light, G: height in world
// units, both times cover, A: cover). Here:
// 1. `fs_seed` marks, at the field's size, which texels block light;
// 2. `fs_jump` runs the jump flood, keeping the nearest blocking texel (xy) and the nearest open
//    one (zw) of every texel;
// 3. `fs_field` turns them into a signed distance in world units (negative inside a blocker);
// 4. `fs_direct` adds up the light from Light things at each pixel, each light's shadow marched
//    through the field, on the surface and in the air (for haze);
// 5. with things that give off light (or Bounce light), the light field: `fs_gi_scene` gathers
//    what gives light (and what lit things bounce) and what is opaque, a second jump flood
//    (`fs_seed_gi`, `fs_gi_dist`) how far each texel is from something opaque, `fs_cascade` the
//    radiance cascades from the farthest in, and `fs_gi_resolve` the light at each texel (its
//    mean, and its mean times the cosine and sine of where it comes from);
// 6. with Light blooms, `fs_glow_source` (the lamps' bulbs and what gives off light) and
//    `fs_glow_down` make the bloom's levels;
// 7. `fs_mix` mixes it in: colour + (light - 1) x lit colour, then the haze and the bloom.

struct Lighting {
    // 2 / output width, 2 / output height in screen units, world units per output pixel, time.
    view: vec4<f32>,
    // The camera: centre xy, zoom, turn.
    camera: vec4<f32>,
    // Ambient light (rgb, already times its strength), w what is shown (0 the lit picture, 1 the
    // light alone, 2 the light as a heat map: the editor's views).
    ambient: vec4<f32>,
    // Shadow style (0 soft, 1 hard, 2 toon, 3 off), softness, length, contact shading.
    shadows: vec4<f32>,
    // Contact reach, toon steps, march steps, light count.
    more: vec4<f32>,
    // The field's size in texels, world units per field texel, jump step (fs_jump).
    field: vec4<f32>,
};

struct LightGpu {
    // Place xy (world), height above the picture, kind (0 point, 1 spot, 2 sun, 3 area, 4 strip).
    p0: vec4<f32>,
    // Colour times strength (linear rgb), w unused.
    p1: vec4<f32>,
    // Reach, falloff (0 real, 1 soft, 2 linear, 3 none), size, shadow strength.
    p2: vec4<f32>,
    // Direction xy (where the light points), cos of the cone's inner and outer edge.
    p3: vec4<f32>,
    // Width, length (area), first segment, segment count (strip).
    p4: vec4<f32>,
    // Shadow colour (rgb), casts shadows (1 or 0).
    p5: vec4<f32>,
    // How far short of the lamp shadows stop (a ridden ball's radius), the sun's angular size
    // (tan), area shape (0 rectangle, 1 disc), the light's shadow softness.
    p6: vec4<f32>,
    // How much its light shows in the haze; three spare.
    p7: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Lighting;
@group(0) @binding(1) var picture: texture_2d<f32>;
@group(0) @binding(2) var lit_picture: texture_2d<f32>;
@group(0) @binding(3) var surface: texture_2d<f32>;
@group(0) @binding(4) var field: texture_2d<f32>;
@group(0) @binding(5) var samp: sampler;
@group(0) @binding(6) var<storage, read> lights: array<LightGpu>;
@group(0) @binding(7) var<storage, read> segments: array<vec4<f32>>;
// The jump flood's input (fs_jump, fs_field).
@group(0) @binding(8) var seeds: texture_2d<f32>;

struct FsIn {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> FsIn {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: FsIn;
    out.pos = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(uv.x, 1.0 - uv.y);
    return out;
}

// ---------- where things are ----------

fn rotate_by(d: vec2<f32>, a: f32) -> vec2<f32> {
    let s = sin(a);
    let k = cos(a);
    return vec2<f32>(d.x * k - d.y * s, d.x * s + d.y * k);
}

// The world point at `uv` (0..1 across the output, y down).
fn uv_to_world(uv: vec2<f32>) -> vec2<f32> {
    let q = vec2<f32>((uv.x * 2.0 - 1.0) / u.view.x, (uv.y * 2.0 - 1.0) / u.view.y);
    var d = q / u.camera.z;
    if u.camera.w != 0.0 {
        d = rotate_by(d, u.camera.w);
    }
    return d + u.camera.xy;
}

// `uv_to_world` backwards.
fn world_to_uv(p: vec2<f32>) -> vec2<f32> {
    var d = p - u.camera.xy;
    if u.camera.w != 0.0 {
        d = rotate_by(d, -u.camera.w);
    }
    let q = d * u.camera.z;
    return vec2<f32>(q.x * u.view.x * 0.5 + 0.5, q.y * u.view.y * 0.5 + 0.5);
}

// The signed distance to the nearest blocker at world point `p`; far away off the picture.
fn field_at(p: vec2<f32>) -> f32 {
    let uv = world_to_uv(p);
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        // Off the picture nothing blocks (things there aren't drawn).
        let inside = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
        let edge = textureSampleLevel(field, samp, inside, 0.0).r;
        return max(edge, 0.0) + length(uv - inside) * 2.0 / max(u.view.x, 1e-6) / max(u.camera.z, 1e-6);
    }
    return textureSampleLevel(field, samp, uv, 0.0).r;
}

// ---------- the jump flood ----------

@fragment
fn fs_seed(in: FsIn) -> @location(0) vec4<f32> {
    let s = textureSampleLevel(surface, samp, in.uv, 0.0);
    let here = floor(in.pos.xy);
    if s.r > 0.5 {
        return vec4<f32>(here, -1.0, -1.0);
    }
    return vec4<f32>(-1.0, -1.0, here);
}

@fragment
fn fs_jump(in: FsIn) -> @location(0) vec4<f32> {
    let here = floor(in.pos.xy);
    let size = vec2<i32>(u.field.xy);
    let step = i32(u.field.w);
    var best = vec4<f32>(-1.0);
    var best_in = 1e20;
    var best_out = 1e20;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let at = vec2<i32>(here) + vec2<i32>(dx, dy) * step;
            if any(at < vec2<i32>(0)) || any(at >= size) {
                continue;
            }
            let s = textureLoad(seeds, at, 0);
            if s.x >= 0.0 {
                let d = distance(s.xy, here);
                if d < best_in {
                    best_in = d;
                    best = vec4<f32>(s.xy, best.zw);
                }
            }
            if s.z >= 0.0 {
                let d = distance(s.zw, here);
                if d < best_out {
                    best_out = d;
                    best = vec4<f32>(best.xy, s.zw);
                }
            }
        }
    }
    return best;
}

@fragment
fn fs_field(in: FsIn) -> @location(0) vec4<f32> {
    let here = floor(in.pos.xy);
    let s = textureLoad(seeds, vec2<i32>(here), 0);
    let texel = u.field.z;
    // No blocker anywhere: far from everything.
    if s.x < 0.0 {
        return vec4<f32>(1e4, 0.0, 0.0, 1.0);
    }
    let blocked = s.x == here.x && s.y == here.y;
    if blocked {
        // Inside: how far to the nearest open texel, negative (none open: deep inside).
        var d = 1e4;
        if s.z >= 0.0 {
            d = distance(s.zw, here) - 0.5;
        }
        return vec4<f32>(-d * texel, 0.0, 0.0, 1.0);
    }
    return vec4<f32>((distance(s.xy, here) - 0.5) * texel, 0.0, 0.0, 1.0);
}

// ---------- the light ----------

// How a light fades with distance `d` against its reach.
fn falloff(d: f32, reach: f32, kind: f32) -> f32 {
    if reach <= 0.0 {
        return 0.0;
    }
    let x = d / reach;
    if x >= 1.0 {
        return 0.0;
    }
    if kind < 0.5 {
        // Real: brightest at the lamp, about a fifth at a quarter of the reach, windowed to 0.
        let w = 1.0 - x * x * x * x;
        return w * w / (1.0 + 16.0 * x * x);
    }
    if kind < 1.5 {
        let w = 1.0 - x * x;
        return w * w;
    }
    if kind < 2.5 {
        return 1.0 - x;
    }
    return 1.0;
}

// The nearest point to `p` of segment `a`-`b`.
fn nearest_on(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    let ab = b - a;
    let t = clamp(dot(p - a, ab) / max(dot(ab, ab), 1e-6), 0.0, 1.0);
    return a + ab * t;
}

// Where the light comes from for the pixel at `p`: the lamp, or the nearest point of an area or
// a strip.
fn source_point(l: LightGpu, p: vec2<f32>) -> vec2<f32> {
    let kind = l.p0.w;
    let at = l.p0.xy;
    if kind > 2.5 && kind < 3.5 {
        // Area: the nearest point of its rectangle or disc, turned by its direction.
        let half = 0.5 * vec2<f32>(max(l.p4.x, 0.0), max(l.p4.y, 0.0));
        let dir = l.p3.xy;
        let side = vec2<f32>(-dir.y, dir.x);
        let d = p - at;
        let local = vec2<f32>(dot(d, dir), dot(d, side));
        var q = clamp(local, -half, half);
        if l.p6.z > 0.5 {
            // A disc (an ellipse of the two sizes).
            let r = local / max(half, vec2<f32>(1e-3));
            if dot(r, r) > 1.0 {
                q = local / length(r);
            } else {
                q = local;
            }
        }
        return at + dir * q.x + side * q.y;
    }
    if kind > 3.5 {
        // Strip: the nearest point along its segments.
        let first = u32(l.p4.z);
        let n = u32(l.p4.w);
        var best = at;
        var best_d = 1e30;
        for (var k = 0u; k < n; k++) {
            let s = segments[first + k];
            let q = nearest_on(p, s.xy, s.zw);
            let d = distance(p, q);
            if d < best_d {
                best_d = d;
                best = q;
            }
        }
        return best;
    }
    return at;
}

// How much of the light gets from `target` (or, for a sun, from far along `toward`) to `p`
// through the blockers: 1 all of it, 0 none. `angular` is the lamp's size as seen from `p` (its
// radius over its distance), `reach` how far behind a blocker its shadow reaches, `stop` how far
// short of the lamp the march ends.
fn visibility(p: vec2<f32>, toward: vec2<f32>, dist: f32, angular: f32, reach: f32, stop: f32) -> f32 {
    let steps = i32(u.more.z);
    let texel = max(u.field.z, 1e-3);
    var t = 0.0;
    // Leave the blocker the pixel is on: a thing doesn't shadow itself.
    for (var i = 0; i < steps; i++) {
        let d = field_at(p + toward * t);
        if d >= 0.0 {
            break;
        }
        t += max(-d, texel);
    }
    let end = min(dist - stop, t + max(reach, 0.0));
    let ang = max(angular, 1e-4);
    var v = 1.0;
    for (var i = 0; i < steps; i++) {
        if t >= end {
            break;
        }
        let d = field_at(p + toward * t);
        v = min(v, clamp(0.5 + 0.5 * d / (max(t, texel) * ang), 0.0, 1.0));
        if v <= 0.0 {
            break;
        }
        t += max(abs(d), texel);
    }
    return v * v * (3.0 - 2.0 * v);
}

// The height of the surface at pixel `ip` (0 where nothing covers).
fn height_at(ip: vec2<i32>) -> f32 {
    let size = vec2<i32>(textureDimensions(surface));
    let s = textureLoad(surface, clamp(ip, vec2<i32>(0), size - 1), 0);
    if s.a < 0.02 {
        return 0.0;
    }
    return s.g / s.a;
}

// ---------- part 1b: what it reads ----------

struct Glow {
    // The field's size in texels, then rounded up so every cascade's probes tile it.
    size: vec4<f32>,
    // This cascade, how many there are, steps a ray, which run (0 first, 1 the bounce's).
    cascade: vec4<f32>,
    // Bounce light, glow reach (world units), Light blooms, the bloom level being made.
    glow: vec4<f32>,
    // The haze's colour times its amount (rgb), how much it dims what is behind it.
    air: vec4<f32>,
    // 1 with a light field, 1 with a bloom, 1 with haze, spare.
    on: vec4<f32>,
};

@group(1) @binding(0) var<uniform> gu: Glow;
// The light things give off (full size).
@group(1) @binding(1) var emission: texture_2d<f32>;
// The light from Light things on the surfaces (with the background's own), and in the air.
@group(1) @binding(2) var direct_light: texture_2d<f32>;
@group(1) @binding(3) var air_light: texture_2d<f32>;
// The light field's scene: light given off or bounced (rgb), opaque (a).
@group(1) @binding(4) var gi_scene: texture_2d<f32>;
// How far each field texel is from something opaque, in texels.
@group(1) @binding(5) var gi_dist: texture_2d<f32>;
// The cascade above the one being made.
@group(1) @binding(6) var above: texture_2d<f32>;
// The light field: the light arriving from each of its four ways (from 45°, 135°, 225° and
// 315°, times GI_GAIN), the fourth way's colour split across the three alphas.
@group(1) @binding(7) var gi_a: texture_2d<f32>;
@group(1) @binding(8) var gi_b: texture_2d<f32>;
@group(1) @binding(9) var gi_c: texture_2d<f32>;
// The lights' bloom, its levels.
@group(1) @binding(10) var glow_pic: texture_2d<f32>;

const PI: f32 = 3.14159265;
// The light field's light: twice the mean light arriving, so a glowing ball lights its
// neighbour about as a lamp of Strength 1 does.
const GI_GAIN: f32 = 2.0;

// ---------- the light from Light things ----------

struct Lamps {
    surface: vec3<f32>,
    air: vec3<f32>,
};

// The light from every Light thing at world point `p` (pixel `ip`), on the surface (turned by its
// normal, banded for toon) and in the air (each light's Beam in haze; no normal).
fn lamps(p: vec2<f32>, ip: vec2<i32>, with_air: bool) -> Lamps {
    let px = u.view.z;
    // The surface's normal from its height's slope (flat where nothing round is).
    let h = height_at(ip);
    let hx = height_at(ip + vec2<i32>(1, 0)) - height_at(ip - vec2<i32>(1, 0));
    let hy = height_at(ip + vec2<i32>(0, 1)) - height_at(ip - vec2<i32>(0, 1));
    var slope = vec2<f32>(hx, hy) / (2.0 * px);
    if u.camera.w != 0.0 {
        slope = rotate_by(slope, u.camera.w);
    }
    let n = normalize(vec3<f32>(-slope, 1.0));
    let style = u.shadows.x;
    var total = vec3<f32>(0.0);
    var air = vec3<f32>(0.0);
    let count = u32(u.more.w);
    for (var i = 0u; i < count; i++) {
        let l = lights[i];
        let kind = l.p0.w;
        var amount = 1.0;
        var toward = vec2<f32>(0.0);
        var dist = 0.0;
        var dir3 = vec3<f32>(0.0, 0.0, 1.0);
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
            let at = source_point(l, p);
            let d = at - p;
            dist = length(d);
            amount = falloff(dist, l.p2.x, l.p2.y);
            if amount == 0.0 {
                continue;
            }
            if kind > 0.5 && kind < 1.5 {
                // Spot: inside its cone round where it points.
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
            let to_lamp = vec3<f32>(d, l.p0.z - h);
            let len3 = length(to_lamp);
            dir3 = select(vec3<f32>(0.0, 0.0, 1.0), to_lamp / len3, len3 > 1e-6);
            angular = max(l.p2.z, 0.0) / max(dist, 1e-3);
        }
        // The picture plane takes the light whole; round surfaces turn to it or away.
        let shade = max(1.0 + dot(n, dir3) - dir3.z, 0.0);
        var tint = vec3<f32>(1.0);
        if l.p5.w > 0.5 && style < 2.5 && dist > 1e-3 {
            var soft = angular * max(u.shadows.y, 0.0) * max(l.p6.w, 0.0);
            if style > 0.5 {
                soft = 0.5 * px / max(dist, 1e-3);
            }
            // The march stops short of the lamp by its size, a ridden ball's radius and however
            // deep in a blocker the lamp sits (a strip along a shape's band, a light in a ball).
            var stop = l.p6.x + max(l.p2.z, 0.0);
            if kind < 1.5 || kind > 2.5 {
                stop = max(stop, -field_at(source_point(l, p)));
            }
            let v = visibility(p, toward, dist, soft, u.shadows.z, stop);
            tint = mix(vec3<f32>(1.0), l.p5.rgb, clamp((1.0 - v) * l.p2.w, 0.0, 1.0));
        }
        var strength = amount * shade;
        if style > 1.5 && style < 2.5 {
            // Toon: the light in bands.
            let bands = max(u.more.y, 1.0);
            strength = floor(strength * bands + 0.5) / bands;
        }
        total += l.p1.rgb * strength * tint;
        if with_air {
            air += l.p1.rgb * amount * tint * l.p7.x;
        }
    }
    return Lamps(total, air);
}

struct Two {
    @location(0) a: vec4<f32>,
    @location(1) b: vec4<f32>,
};

// The light from Light things at each pixel (added over the background's own light, when it
// gives any), and the light in the air for the haze.
@fragment
fn fs_direct(in: FsIn) -> Two {
    let l = lamps(uv_to_world(in.uv), vec2<i32>(in.pos.xy), gu.on.z > 0.5);
    return Two(vec4<f32>(l.surface, 0.0), vec4<f32>(l.air, 0.0));
}

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
    for (var k = 0; k < 4; k++) {
        let o = vec2<f32>(f32(k & 1) - 0.5, f32(k >> 1u) - 0.5) * 0.5;
        let uv = (fp + o) / field_size;
        let c = textureSampleLevel(picture, samp, uv, 0.0);
        let lw = textureSampleLevel(lit_picture, samp, uv, 0.0).rgb;
        w += clamp(lw, vec3<f32>(0.0), max(c.rgb, vec3<f32>(0.0)));
        light += textureSampleLevel(direct_light, samp, uv, 0.0).rgb;
        blocks += textureSampleLevel(surface, samp, uv, 0.0).r;
    }
    w *= 0.25;
    light *= 0.25;
    blocks = clamp(blocks * 0.25, 0.0, 1.0);
    if gu.cascade.w > 0.5 {
        // The second run: the first run's light bounces too.
        let at = vec2<i32>(fp);
        light += field_light(at, fp, texel_blocked(at));
    }
    let bounced = gu.glow.x * w * max(light, vec3<f32>(0.0)) * blocks;
    // Something giving off light is opaque where it covers most of the texel, or where it is
    // light covering nothing (a glow, an added trail); a texel it only grazes stays open, so the
    // light round its edge isn't stopped there by a sliver of it.
    let bright = max(abs(e.r), max(abs(e.g), abs(e.b)));
    let gives = (ec.a > 0.5 && bright > 1e-4) || (ec.a < 0.05 && bright > 0.02);
    let opaque = select(step(0.5, blocks), 1.0, gives);
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
    // Longer stretches take more steps; a ray that runs out of them before its stretch ends
    // counts as stopped, so light never leaks through what it didn't reach.
    let budget = steps + 4 * i32(i);
    var done = false;
    for (var k = 0; k < budget; k++) {
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
        let gap = textureLoad(gi_dist, qi, 0).r;
        if gap < 0.5 {
            let s = textureLoad(gi_scene, qi, 0);
            // The light fades out over the glow reach (from the probe).
            let x = along * texel_world / max(reach, 1e-3);
            let w = clamp(1.0 - x * x, 0.0, 1.0);
            rad = s.rgb * w * w * select(0.0, 1.0, reach > 0.0);
            open = 0.0;
            break;
        }
        along += max(gap - 1.0, 0.75);
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
        rad += open * merged;
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

// ---------- the mix ----------

@fragment
fn fs_mix(in: FsIn) -> @location(0) vec4<f32> {
    let ip = vec2<i32>(in.pos.xy);
    let c = textureLoad(picture, ip, 0);
    // The lit share of the colour: never more than the colour itself.
    let w = clamp(textureLoad(lit_picture, ip, 0).rgb, vec3<f32>(0.0), max(c.rgb, vec3<f32>(0.0)));
    let p = uv_to_world(in.uv);
    var field = vec3<f32>(0.0);
    var field_air = vec3<f32>(0.0);
    if gu.on.x > 0.5 {
        let fp = in.uv * gu.size.xy;
        // Blocked or not as the pixel itself is (a field texel can straddle an edge).
        let blocks_here = textureLoad(surface, ip, 0).r > 0.5;
        field = field_light(vec2<i32>(fp), fp, blocks_here);
        field_air = field_mean(in.uv * gu.size.xy);
    }
    var light = max(u.ambient.rgb + textureLoad(direct_light, ip, 0).rgb + field, vec3<f32>(0.0));
    // Contact shading: light dims close to a blocker.
    let here = field_at(p);
    if u.shadows.w != 0.0 && here > 0.0 {
        let reach = max(u.more.x, 1e-3);
        light *= 1.0 - u.shadows.w * (1.0 - smoothstep(0.0, reach, here));
    }
    // The editor's views of the light alone.
    if u.ambient.w > 1.5 {
        return vec4<f32>(heat(dot(light, vec3<f32>(0.2126, 0.7152, 0.0722))), 1.0);
    }
    if u.ambient.w > 0.5 {
        return vec4<f32>(clamp(light, vec3<f32>(-6.0e4), vec3<f32>(6.0e4)), 1.0);
    }
    var out = c.rgb + (light - vec3<f32>(1.0)) * w;
    if gu.on.z > 0.5 {
        // The haze: the light in the air, over what it veils.
        let veil = clamp(gu.air.w, 0.0, 1.0) * min(max(gu.air.r, max(gu.air.g, gu.air.b)), 1.0);
        let air = max(textureLoad(air_light, ip, 0).rgb + field_air, vec3<f32>(0.0));
        out = out * (1.0 - veil) + gu.air.rgb * air;
    }
    if gu.on.y > 0.5 {
        let b = 0.35 * textureSampleLevel(glow_pic, samp, in.uv, 0.0).rgb
            + 0.3 * textureSampleLevel(glow_pic, samp, in.uv, 1.0).rgb
            + 0.2 * textureSampleLevel(glow_pic, samp, in.uv, 2.0).rgb
            + 0.15 * textureSampleLevel(glow_pic, samp, in.uv, 3.0).rgb;
        out += gu.glow.z * b;
    }
    // Kept finite (a huge Strength would overflow the half-float picture) and never below black.
    return vec4<f32>(clamp(out, vec3<f32>(0.0), vec3<f32>(6.0e4)), c.a);
}

fn srgb_decode(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

// How bright light `l` is as a heat colour (linear): the stops are style::HEAT's, in sRGB.
fn heat(l: f32) -> vec3<f32> {
    if l < 0.0 {
        // Negative light: violet, deeper the darker.
        return srgb_decode(mix(vec3<f32>(0.0), vec3<f32>(0.541, 0.302, 1.0), clamp(-l, 0.0, 1.0)));
    }
    var at = array<f32, 7>(0.0, 0.25, 0.5, 1.0, 1.5, 2.0, 3.0);
    var colour = array<vec3<f32>, 7>(
        vec3<f32>(0.0, 0.0, 0.0),
        vec3<f32>(0.102, 0.165, 0.620),
        vec3<f32>(0.098, 0.706, 0.902),
        vec3<f32>(0.235, 0.812, 0.306),
        vec3<f32>(0.949, 0.824, 0.294),
        vec3<f32>(1.0, 0.231, 0.184),
        vec3<f32>(1.0, 1.0, 1.0),
    );
    var out = colour[6];
    for (var k = 0; k < 6; k++) {
        if l < at[k + 1] {
            out = mix(colour[k], colour[k + 1], (l - at[k]) / (at[k + 1] - at[k]));
            break;
        }
    }
    return srgb_decode(out);
}

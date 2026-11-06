// Lighting (Flat 2D), run only when the scene has a Lighting card (see lighting.rs).
//
// The things were drawn three times before this runs: the picture as always, the lit colour
// (every layer's colour scaled by its Lit) and the surface (R: blocks light, G: height in world
// units, both times cover, A: cover). Here:
// 1. `fs_seed` marks, at the field's size, which texels block light;
// 2. `fs_jump` runs the jump flood, keeping the nearest blocking texel (xy) and the nearest open
//    one (zw) of every texel;
// 3. `fs_field` turns them into a signed distance in world units (negative inside a blocker);
// 4. `fs_light` adds up the light at each pixel, each light's shadow marched through the field,
//    and mixes it in: colour + (light - 1) x lit colour.

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

@fragment
fn fs_light(in: FsIn) -> @location(0) vec4<f32> {
    let ip = vec2<i32>(in.pos.xy);
    let c = textureLoad(picture, ip, 0);
    // The lit share of the colour: never more than the colour itself.
    let w = clamp(textureLoad(lit_picture, ip, 0).rgb, vec3<f32>(0.0), max(c.rgb, vec3<f32>(0.0)));
    let p = uv_to_world(in.uv);
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
    let here = field_at(p);
    var total = vec3<f32>(0.0);
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
    }
    var light = u.ambient.rgb + total;
    // Contact shading: light dims close to a blocker.
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
    // Kept finite: a huge Strength would overflow the half-float picture to infinity.
    return vec4<f32>(min(c.rgb + (light - vec3<f32>(1.0)) * w, vec3<f32>(6.0e4)), c.a);
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

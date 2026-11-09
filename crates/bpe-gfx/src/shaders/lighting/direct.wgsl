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
    // 1 with a light field, 1 with a bloom, 1 with haze (or smoke: the light in the air), 1 in
    // Stage (balls block the field, part 2b).
    on: vec4<f32>,
    // Part 4. The haze moving: how (0 still, 1 drifting, 2 swirling), speed, size, patchiness.
    haze: vec4<f32>,
    // Which way it drifts (unit xy), how much balls stir it, spare.
    haze_more: vec4<f32>,
    // Underwater: how much, caustics, caustic size, caustic speed.
    water: vec4<f32>,
    // The water's colour, sun rays.
    water_colour: vec4<f32>,
    // Where the water starts (world y), how deep its colour takes over, spare.
    water_depth: vec4<f32>,
    // Glow in the dark: how bright, fade time, charge speed, what remembers (0 the background).
    memory: vec4<f32>,
    // The glow's own colour, 1 to glow in the light's own colour.
    memory_colour: vec4<f32>,
    // How much the remembered light keeps this frame (e^(-dt/fade)), how far it charges toward
    // the light now (1 - e^(-charge dt)), 1 when there is a memory from the last frame, spare.
    memory_step: vec4<f32>,
    // The last frame's camera: centre xy, zoom, turn (the memory is read where it was).
    last_camera: vec4<f32>,
    // 1 with a UV picture, 1 with a tags picture, 1 with the extras picture (moving haze or
    // memory), 1 when the haze moves.
    more_on: vec4<f32>,
    // Glass and light part 4b: Rainbow (0 none), its size (world: the 42° bow's radius), Second
    // bow, Dark band.
    rainbow: vec4<f32>,
    // The bow's middle (world xy), 1 when rain shows it (the tags picture's B), spare.
    rainbow_at: vec4<f32>,
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
// Part 4 (each pass's layout lists only what it reads). What catches beams (r) and the thing's
// link number (g), both times cover (a), from light pass 6.
@group(1) @binding(11) var tags: texture_2d<f32>;
// What glows under UV light, from light pass 5.
@group(1) @binding(12) var uv_pic: texture_2d<f32>;
// The remembered light (rgb) and the moving haze's density (a), field size.
@group(1) @binding(13) var extras: texture_2d<f32>;
// How the balls stir the haze: their velocity (xy) and how near (z), field size.
@group(1) @binding(14) var stir_pic: texture_2d<f32>;

const PI: f32 = 3.14159265;
// The light field's light: twice the mean light arriving, so a glowing ball lights its
// neighbour about as a lamp of Strength 1 does.
const GI_GAIN: f32 = 2.0;

// ---------- the light from Light things ----------

struct Lamps {
    surface: vec3<f32>,
    air: vec3<f32>,
    // How much ultraviolet light falls here.
    uv: f32,
};

// ---------- part 4: noise, patterns, gobos, links ----------

// A number from 0 to 1 for a whole-number point, the same on every GPU.
fn hash2i(p: vec2<i32>) -> f32 {
    var h = bitcast<u32>(p.x) * 0x8da6b343u ^ bitcast<u32>(p.y) * 0xd8163841u;
    h = (h ^ (h >> 15u)) * 0x2c1b3c6du;
    h = (h ^ (h >> 12u)) * 0x297a2d39u;
    h = h ^ (h >> 15u);
    return f32(h >> 8u) / 16777215.0;
}

// Smooth value noise, 0 to 1.
fn value_noise(q: vec2<f32>) -> f32 {
    let i = vec2<i32>(floor(q));
    let f = fract(q);
    let u2 = f * f * (3.0 - 2.0 * f);
    let a = hash2i(i);
    let b = hash2i(i + vec2<i32>(1, 0));
    let c = hash2i(i + vec2<i32>(0, 1));
    let d = hash2i(i + vec2<i32>(1, 1));
    return mix(mix(a, b, u2.x), mix(c, d, u2.x), u2.y);
}

// Four octaves of value noise, 0 to 1.
fn fbm(q: vec2<f32>) -> f32 {
    var sum = 0.0;
    var amp = 0.5;
    var at = q;
    for (var k = 0; k < 4; k++) {
        sum += amp * value_noise(at);
        at = vec2<f32>(at.x * 1.6 - at.y * 1.2, at.x * 1.2 + at.y * 1.6) + vec2<f32>(3.1, 1.7);
        amp *= 0.5;
    }
    return sum / 0.9375;
}

// Moving cells' edges: 1 on an edge, 0 deep inside a cell (`t` moves the cells' middles).
fn cell_edges(q: vec2<f32>, t: f32) -> f32 {
    let i = floor(q);
    let f = fract(q);
    var f1 = 8.0;
    var f2 = 8.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let o = vec2<f32>(f32(x), f32(y));
            let h = hash2i(vec2<i32>(i + o));
            let h2 = hash2i(vec2<i32>(i + o) + vec2<i32>(17, 59));
            let mid = o + 0.5 + 0.38 * vec2<f32>(sin(t + 6.2831 * h), cos(0.8 * t + 6.2831 * h2)) - f;
            let d = dot(mid, mid);
            if d < f1 {
                f2 = f1;
                f1 = d;
            } else if d < f2 {
                f2 = d;
            }
        }
    }
    let e = sqrt(f2) - sqrt(f1);
    return exp(-e * e * 60.0);
}

// Water caustics: two layers of moving, bright, net-like lines, about 0 to 1.3.
fn caustics(q: vec2<f32>, t: f32) -> f32 {
    return 0.75 * cell_edges(q, t) + 0.55 * cell_edges(q * 1.7 + vec2<f32>(3.1, 7.3), 1.3 * t + 2.0);
}

// A gobo's pattern at `q` (in gobo sizes), 0 dark to 1 open, before its sharpness.
fn gobo_pattern(kind: f32, q: vec2<f32>, t: f32) -> f32 {
    if kind < 1.5 {
        // Stripes.
        return 0.5 + 0.5 * cos(6.2831853 * q.x);
    }
    if kind < 2.5 {
        // Dots.
        let f = fract(q) - 0.5;
        return clamp(1.0 - 2.4 * length(f), 0.0, 1.0) * 2.0;
    }
    if kind < 3.5 {
        // Window: panes with bars between them.
        let f = fract(q);
        let edge = min(min(f.x, 1.0 - f.x), min(f.y, 1.0 - f.y));
        return clamp(edge * 14.0 - 0.3, 0.0, 1.0);
    }
    if kind < 4.5 {
        // Leaves: dappled, swaying.
        let sway = vec2<f32>(sin(0.7 * t), cos(0.53 * t)) * 0.12;
        return fbm(q * 1.3 + sway);
    }
    if kind < 5.5 {
        return caustics(q, t);
    }
    if kind < 6.5 {
        // Rings.
        return 0.5 + 0.5 * cos(6.2831853 * length(q));
    }
    // Clouds.
    return fbm(q * 0.5 + vec2<f32>(0.05 * t, 0.0));
}

// How much of light `l` gets past its gobo at world point `p`: 1 with none.
fn gobo(l: LightGpu, p: vec2<f32>) -> f32 {
    let kind = l.p8.x;
    if kind < 0.5 {
        return 1.0;
    }
    // A sun's pattern is laid over the world; a lamp's turns and drifts round the lamp.
    var d = p - l.p0.xy;
    if l.p0.w > 1.5 && l.p0.w < 2.5 {
        d = p;
    }
    let c = l.p9.x;
    let sn = l.p9.y;
    let local = vec2<f32>(d.x * c + d.y * sn, -d.x * sn + d.y * c) - l.p9.zw;
    let q = local / max(l.p8.y, 1e-3);
    let v = gobo_pattern(kind, q, u.view.w);
    let w = 0.5 * (1.0 - clamp(l.p8.z, 0.0, 1.0)) + 0.02;
    let open = smoothstep(0.5 - w, 0.5 + w, v);
    return 1.0 - l.p8.w * (1.0 - open);
}

// The link number of what covers pixel `ip` (Lights up): 0 for nothing named.
fn link_here(ip: vec2<i32>) -> f32 {
    let size = vec2<i32>(textureDimensions(tags));
    let t = textureLoad(tags, clamp(ip, vec2<i32>(0), size - 1), 0);
    if t.a < 0.02 {
        return 0.0;
    }
    return round(t.g / t.a);
}

// Whether light `l` lights up what has link number `here`.
fn lights_up(l: LightGpu, here: f32) -> bool {
    let mode = l.p10.x;
    if mode < 0.5 {
        return true;
    }
    let first = u32(l.p10.y);
    let n = u32(l.p10.z);
    var named = false;
    for (var k = 0u; k < n; k++) {
        if links[first + k] == here {
            named = true;
            break;
        }
    }
    return select(!named, named, mode < 1.5);
}

// The lights that may reach world point `q` (review S19): the run of `links` listing them, in order,
// for the picture tile `q` is in; every light (`listed` false) where there are no lists or `q` is off
// the picture. A light missing from a tile's list gives nothing anywhere in the tile (the CPU's
// `light_tiles` keeps every light whose reach touches it, the sun always), so the sums are the same.
struct LightRun {
    first: u32,
    count: u32,
    listed: bool,
};

fn lights_near(q: vec2<f32>) -> LightRun {
    let every = LightRun(0u, u32(u.more.w), false);
    if u.tiles.z < 0.5 {
        return every;
    }
    let t = floor(world_to_uv(q) * u.tiles.xy);
    if !(t.x >= 0.0 && t.y >= 0.0 && t.x < u.tiles.z && t.y < u.tiles.w) {
        return every;
    }
    let head = u32(u.tile_list.x) + 2u * (u32(t.y) * u32(u.tiles.z) + u32(t.x));
    return LightRun(u32(links[head]), u32(links[head + 1u]), true);
}

// The `k`th light of `run`.
fn light_in(run: LightRun, k: u32) -> u32 {
    return select(k, u32(links[run.first + k]), run.listed);
}

// The light from every Light thing at world point `p` (pixel `ip`), on the surface (turned by its
// normal, banded for toon) and in the air (each light's Beam in haze; no normal).
// `surface_wanted`: whether anything reads the light on the surface (`Lamps.surface`): on a Stage
// every pixel lights itself in the mix (`surfaced`), so only the bounce, glow in the dark and
// rainbows read it there. `uv_wanted`: whether anything reads the ultraviolet light. A light
// none of them (nor the haze, `with_air`) takes is skipped before its shadow march (review S20).
fn lamps(p: vec2<f32>, ip: vec2<i32>, with_air: bool, surface_wanted: bool, uv_wanted: bool) -> Lamps {
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
    var uv = 0.0;
    let near = lights_near(p);
    // What covers this pixel, for the lights that light up only some things.
    var here = 0.0;
    if gu.more_on.y > 0.5 {
        here = link_here(ip);
    }
    for (var k = 0u; k < near.count; k++) {
        let i = light_in(near, k);
        let l = lights[i];
        let kind = l.p0.w;
        // Lights up: the light in the air shows whatever it lights.
        let on_surface = lights_up(l, here);
        let wanted_on_surface = surface_wanted && on_surface;
        let wanted_uv = uv_wanted && on_surface && l.p7.y > 0.5;
        let wanted_air = with_air && l.p7.x != 0.0;
        if !(wanted_on_surface || wanted_uv || wanted_air) {
            continue;
        }
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
            // An Area or Strip light's shadows come from all of it.
            var s_toward = toward;
            var s_dist = dist;
            var src = -1;
            if kind > 2.5 && kind < 4.5 {
                let ex = extended_seen(l, p);
                if ex.ang > 0.0 {
                    s_toward = ex.way;
                    s_dist = ex.dist;
                    src = i32(i);
                    stop = ex.stop;
                    if style > 0.5 {
                        soft = 0.5 * px / ex.dist;
                    } else {
                        soft = min(max(ex.ang, angular), MOST_SEEN) * max(u.shadows.y, 0.0) * max(l.p6.w, 0.0);
                    }
                }
            }
            // Glass that bends light is solid for the lights whose photons carry its light.
            bent_carried = kind < 4.5;
            let v = visibility(p, s_toward, s_dist, soft, u.shadows.z, stop, src);
            bent_carried = true;
            tint = mix(vec3<f32>(1.0), l.p5.rgb, clamp((vec3<f32>(1.0) - v) * l.p2.w, vec3<f32>(0.0), vec3<f32>(1.0)));
        }
        let through = gobo(l, p);
        var strength = amount * shade * through;
        if style > 1.5 && style < 2.5 {
            // Toon: the light in bands.
            let bands = max(u.more.y, 1.0);
            strength = floor(strength * bands + 0.5) / bands;
        }
        // An ultraviolet light lights only what glows under UV, but for its visible share.
        var seen = 1.0;
        if l.p7.y > 0.5 {
            seen = l.p7.z;
            if on_surface {
                uv += dot(l.p1.rgb, vec3<f32>(0.2126, 0.7152, 0.0722)) * amount * through
                    * dot(tint, vec3<f32>(0.3333));
            }
        }
        if on_surface {
            total += l.p1.rgb * strength * tint * seen;
        }
        if with_air {
            air += l.p1.rgb * amount * through * tint * l.p7.x * seen;
        }
    }
    return Lamps(total, air, uv);
}

struct Two {
    @location(0) a: vec4<f32>,
    @location(1) b: vec4<f32>,
};

// The light from Light things at each pixel (added over the background's own light, when it
// gives any), and the light in the air for the haze.
@fragment
fn fs_direct(in: FsIn) -> Two {
    // On a Stage the mix lights every pixel itself; the bounce (light field), glow in the dark and
    // rainbows still read this light. Glow in the dark reads the ultraviolet light too.
    let surface_wanted = u.trace.y < 0.5 || gu.glow.x != 0.0 || gu.memory.x != 0.0 || gu.rainbow.x > 0.0;
    let uv_wanted = gu.more_on.x > 0.5 || gu.memory.x != 0.0;
    let l = lamps(uv_to_world(in.uv), vec2<i32>(in.pos.xy), gu.on.z > 0.5, surface_wanted, uv_wanted);
    // The ultraviolet light rides in the alpha (the blend keeps this pass's alpha).
    return Two(vec4<f32>(l.surface, l.uv), vec4<f32>(l.air, 0.0));
}


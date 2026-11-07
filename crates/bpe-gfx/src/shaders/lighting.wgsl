// Lighting (Flat and Stage), run only when the scene has a Lighting card (see lighting.rs).
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
// 7. `fs_mix` mixes it in: colour + (light - 1) x lit colour, then the haze and the bloom; a
//    surface that shines, and anything on a Stage (part 2a), is shaded there by `surfaced`;
// 8. a Glossy or Mirror floor on a Stage mirrors that picture (`fs_floor`).
//
// Part 4 adds, each only when used: `vs_stir`/`fs_stir` (balls stirring moving haze), `fs_extras`
// (the moving haze's density and the remembered light, glow in the dark), gobos, flicker (on the
// CPU), Lights up and ultraviolet light in `lamps()`, and in the mix the haze's density, smoke
// that catches beams, underwater light, UV glow, glow in the dark and laser beams.

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
    // Lighting part 3: this sample's index, how many, 1 when baking, light bounces.
    sample: vec4<f32>,
    // The bake's path-traced bounce: Bounce light, 1 on a Stage (whose balls mark the surface
    // with a quarter, kept out of the blockers' field), two spare.
    trace: vec4<f32>,
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
    // How much its light shows in the haze, 1 when ultraviolet, an ultraviolet light's visible
    // share, a laser's glow.
    p7: vec4<f32>,
    // Gobo: kind, size, sharpness, depth.
    p8: vec4<f32>,
    // The gobo's turn (cos, sin), its drift (xy, world units).
    p9: vec4<f32>,
    // Lights up: mode (0 everything, 1 only, 2 all but), first link, how many, spare.
    p10: vec4<f32>,
    // A laser's width, its hit glow, 1 when its beam ends on something, spare.
    p11: vec4<f32>,
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
// The link numbers the lights' Lights up name (part 4).
@group(0) @binding(9) var<storage, read> links: array<f32>;

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
    return world_to_uv_by(p, u.camera);
}

// `world_to_uv` through `camera` (centre xy, zoom, turn).
fn world_to_uv_by(p: vec2<f32>, camera: vec4<f32>) -> vec2<f32> {
    var d = p - camera.xy;
    if camera.w != 0.0 {
        d = rotate_by(d, -camera.w);
    }
    let q = d * camera.z;
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

// The least share of a picture pixel's cover that blocks light: a line thinner than a pixel, or
// thinner than a field texel, still blocks.
const BLOCK_LEAST: f32 = 0.1;

// The picture pixel under world point `p`; -1 off the picture.
fn pixel_of(p: vec2<f32>) -> vec2<i32> {
    let uv = world_to_uv(p);
    if any(uv < vec2<f32>(0.0)) || any(uv >= vec2<f32>(1.0)) {
        return vec2<i32>(-1);
    }
    let size = vec2<i32>(textureDimensions(surface));
    return min(vec2<i32>(uv * vec2<f32>(size)), size - 1);
}

// How much the picture pixel `ip` blocks light (its cover times how much its things block).
fn pixel_block(ip: vec2<i32>) -> f32 {
    if ip.x < 0 {
        return 0.0;
    }
    return textureLoad(surface, ip, 0).r;
}

// Whether field texel `t` blocks light: any picture pixel under it blocks (the field is smaller
// than the picture below Ultra, and one sample at its middle would miss a thin line).
fn foot_blocks(t: vec2<i32>) -> bool {
    let fsize = vec2<i32>(u.field.xy);
    if any(t < vec2<i32>(0)) || any(t >= fsize) {
        return false;
    }
    let size = vec2<i32>(textureDimensions(surface));
    let scale = vec2<f32>(size) / vec2<f32>(fsize);
    let lo = vec2<i32>(floor(vec2<f32>(t) * scale));
    // On a Stage a ball blocks as a sphere and marks the surface with a quarter (layers.wgsl):
    // only more than that seeds there.
    let least = select(BLOCK_LEAST, 0.3, u.trace.y > 0.5);
    let hi = min(max(vec2<i32>(ceil(vec2<f32>(t + 1) * scale)), lo + 1), size);
    for (var y = lo.y; y < hi.y; y++) {
        for (var x = lo.x; x < hi.x; x++) {
            if textureLoad(surface, vec2<i32>(x, y), 0).r >= least {
                return true;
            }
        }
    }
    return false;
}

@fragment
fn fs_seed(in: FsIn) -> @location(0) vec4<f32> {
    let here = floor(in.pos.xy);
    let t = vec2<i32>(here);
    var blocked = foot_blocks(t);
    if !blocked && foot_blocks(t + vec2<i32>(0, 1)) {
        // Two blocked texels touching only at a corner (a thin diagonal line) leave a gap the
        // light would slip through: one of the open pair closes it, this one when it is the pair
        // member above the lower texel.
        let left = foot_blocks(t + vec2<i32>(-1, 0)) && !foot_blocks(t + vec2<i32>(-1, 1));
        let right = foot_blocks(t + vec2<i32>(1, 0)) && !foot_blocks(t + vec2<i32>(1, 1));
        blocked = left || right;
    }
    if blocked {
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

// One ray's march toward a light: the signed distance estimate of how much of the light gets
// through (`v`, before smoothing), how clear of every blocker the ray stays (`clear`: the least
// distance to one over the distance along the ray, an angle) and whether it crossed one.
struct Ray {
    v: f32,
    clear: f32,
    crossed: bool,
    // How far along the ray it first crossed a blocker.
    at: f32,
};

// How many rays across the lamp, from one edge to the other, the light is worked out from when
// the middle one crosses a blocker (odd: the middle one is the ray already marched).
const LAMP_RAYS: i32 = 5;

// Where the field says a ray at `t` is inside a blocker: walks the picture itself, half a pixel
// at a time, from the ray's last sample outside one (`open`; the field's edge can lie up to a
// step before `t`, and a line in the near part of the texel lies there too) through that stretch,
// at most two field texels past `t`. x: 1 when a blocking pixel was met (at y), else 0 and y
// where the stretch ends. The field is coarser than the picture and blocks wherever any of its
// pixels does, so beside a thin line it says "inside" on both of the line's sides; only the
// picture tells whether the ray really meets the line.
fn confirm(p: vec2<f32>, toward: vec2<f32>, open: f32, t_in: f32, end: f32) -> vec2<f32> {
    let px = max(u.view.z, 1e-3);
    let texel = max(u.field.z, 1e-3);
    let start = max(open, t_in - texel);
    let most = i32(ceil((t_in - start + 4.0 * texel) / px)) + 2;
    // The pixel's own pixel doesn't count: at a blocker's soft edge it blocks a little, and the
    // way to the light tells whether the pixel is on the blocker's lit side or its dark one.
    let own = pixel_of(p);
    var t = start;
    for (var i = 0; i < most; i++) {
        if t >= end {
            break;
        }
        let q = p + toward * t;
        let at = pixel_of(q);
        if any(at != own) && pixel_block(at) >= BLOCK_LEAST {
            return vec2<f32>(1.0, t);
        }
        if t > t_in && field_at(q) >= 0.0 {
            break;
        }
        t += 0.5 * px;
    }
    return vec2<f32>(0.0, max(t, t_in));
}

// `visibility` for one ray. `across`: one of the rays across the lamp, which stops as soon as it
// crosses a blocker (only whether it did and how clear it stayed count).
fn march(p: vec2<f32>, toward: vec2<f32>, dist: f32, angular: f32, reach: f32, stop: f32, across: bool) -> Ray {
    let steps = i32(u.more.z);
    let texel = max(u.field.z, 1e-3);
    var t = 0.0;
    if pixel_block(pixel_of(p)) > 0.5 {
        // Leave the blocker the pixel is on: a thing doesn't shadow itself.
        for (var i = 0; i < steps; i++) {
            let d = field_at(p + toward * t);
            if d >= 0.0 {
                break;
            }
            t += max(-d, texel);
        }
        // Just out of it, the field is still about 0: start a texel and a half on, or the edge
        // of the pixel's own blocker half-shadows it, in speckles where the exit lands (part 2b).
        if t > 0.0 {
            t += 1.5 * texel;
        }
    }
    let end = min(dist - stop, t + max(reach, 0.0));
    let ang = max(angular, 1e-4);
    var v = 1.0;
    var clear = 1e4;
    var crossed = false;
    // Just past a stretch only the field's coarseness blocked, how near the ray is to that
    // blocker means nothing (the field can't tell within a texel): it counts from a texel away.
    var beside = false;
    var open = t;
    var cross_at = 0.0;
    // Skipping what only the field's coarseness blocked doesn't use up the steps (a ray along a
    // thin line would run out of them and count the rest of its way as open), within a cap.
    var used = 0;
    for (var i = 0; i < 4 * steps; i++) {
        if t >= end || used >= steps {
            break;
        }
        let d = field_at(p + toward * t);
        if d < 0.0 && !crossed {
            let c = confirm(p, toward, open, t, end);
            if c.x < 0.5 {
                // Only the field's coarseness: carry on from past it.
                t = max(c.y, t + 0.5 * texel);
                open = t;
                beside = true;
                continue;
            }
            crossed = true;
            cross_at = c.y;
            if across {
                return Ray(0.0, 0.0, true, c.y);
            }
            t = c.y;
        }
        if beside && d >= 0.0 && d < texel {
            t += 0.5 * texel;
            open = t;
            continue;
        }
        beside = false;
        used += 1;
        if d >= 0.0 {
            open = t;
        }
        clear = min(clear, max(d, 0.0) / max(t, texel));
        v = min(v, clamp(0.5 + 0.5 * d / (max(t, texel) * ang), 0.0, 1.0));
        if v <= 0.0 {
            break;
        }
        // At most half a texel: a wall one texel thick always has a step inside it.
        t += max(abs(d), 0.5 * texel);
    }
    return Ray(v, clear, crossed, cross_at);
}

fn smoothed(v: f32) -> f32 {
    return v * v * (3.0 - 2.0 * v);
}

// How much of the light gets from `target` (or, for a sun, from far along `toward`) to `p`
// through the blockers: 1 all of it, 0 none. `angular` is the lamp's size as seen from `p` (its
// radius over its distance), `reach` how far behind a blocker its shadow reaches, `stop` how far
// short of the lamp the march ends.
//
// A ray that crosses nothing keeps the signed distance estimate (half the light where it grazes
// a blocker). One that crosses something thinner than the shadow's soft edge would let half the
// light through that way (inside, the distance is only as deep as half the blocker), so then
// rays go to points across the lamp, edge to edge: one that crosses a blocker (a line one pixel
// thick or a whole shape) shows nothing, one that gets past shows the lamp as far round its point
// as it stays clear of blockers, and the light is the share of the lamp they show together. Where
// the middle ray grazes a blocker that is half, as outside it, so the soft edge has no step.
fn visibility(p: vec2<f32>, toward: vec2<f32>, dist: f32, angular: f32, reach: f32, stop: f32) -> f32 {
    let middle = march(p, toward, dist, angular, reach, stop, false);
    if !middle.crossed || middle.v <= 0.0 {
        return smoothed(middle.v);
    }
    let ang = max(angular, 1e-4);
    // Where it crossed, the soft edge is narrower than half a pixel: dark, no more rays needed.
    if middle.at * ang < 0.5 * u.view.z {
        return 0.0;
    }
    // Draft (16 steps) makes do with the lamp's two edges.
    let rays = select(LAMP_RAYS, 3, u.more.z <= 16.0);
    let gap = 2.0 * ang / f32(rays - 1);
    let side = vec2<f32>(-toward.y, toward.x);
    // What each ray shows of the lamp: from its point less how clear it stays to its point plus
    // that, within the lamp (nothing when it crosses a blocker, as the middle one did).
    var lo: array<f32, LAMP_RAYS>;
    var hi: array<f32, LAMP_RAYS>;
    for (var k = 0; k < LAMP_RAYS; k++) {
        let off = f32(k) * gap - ang;
        lo[k] = 0.0;
        hi[k] = 0.0;
        if k >= rays || k == rays / 2 {
            continue;
        }
        let r = march(p, normalize(toward + side * off), dist, gap, reach, stop, true);
        let c = select(min(r.clear, 2.0 * ang), -1.0, r.crossed);
        lo[k] = max(off - c, -ang);
        hi[k] = min(off + c, ang);
    }
    // The length of their union.
    var shown = 0.0;
    var reached = -ang;
    for (var k = 0; k < LAMP_RAYS; k++) {
        // The stretch that starts first of those left (they are few: look through them all).
        var pick = -1;
        var least = 1e9;
        for (var j = 0; j < LAMP_RAYS; j++) {
            if hi[j] > lo[j] && lo[j] < least {
                least = lo[j];
                pick = j;
            }
        }
        if pick < 0 {
            break;
        }
        let a = max(lo[pick], reached);
        if hi[pick] > a {
            shown += hi[pick] - a;
            reached = hi[pick];
        }
        hi[pick] = lo[pick];
    }
    return clamp(shown / (2.0 * ang), 0.0, 1.0);
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
    var uv = 0.0;
    let count = u32(u.more.w);
    // What covers this pixel, for the lights that light up only some things.
    var here = 0.0;
    if gu.more_on.y > 0.5 {
        here = link_here(ip);
    }
    for (var i = 0u; i < count; i++) {
        let l = lights[i];
        let kind = l.p0.w;
        // Lights up: the light in the air shows whatever it lights.
        let on_surface = lights_up(l, here);
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
    let l = lamps(uv_to_world(in.uv), vec2<i32>(in.pos.xy), gu.on.z > 0.5);
    // The ultraviolet light rides in the alpha (the blend keeps this pass's alpha).
    return Two(vec4<f32>(l.surface, l.uv), vec4<f32>(l.air, 0.0));
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
        var b = sv.r;
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
    let thin = blocks < 0.5 && texel_blocked(vec2<i32>(fp));
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
    var q = (p - push) / size - gu.haze_more.xy * speed * t * 100.0 / size;
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

// ---------- surfaces and the stage (part 2a) ----------
//
// Every lit pixel has a surface: the surface picture's B channel names its entry in `surfaces`
// (0 a plain sheet: trails, particles, the background in Flat). In Stage (`st.room.x` 1) the 2D
// world stands on a stage: balls are spheres round the picture plane (z = 0, z toward the
// camera), the background is a wall at z = -back, an optional floor runs under them, shapes stand
// out of the wall to their front, and the lights hang at their Height. The wall and floor are seen
// through a small oblique view: a point (x, y, z) shows at (x, y + z k); the picture plane is
// untouched.

struct Stage {
    // 1 in Stage (0 Flat), the back wall's depth, the shapes' front (z), k = tan(looking down)
    // (0 with no floor).
    room: vec4<f32>,
    // Floor kind (0 none, 1 at a world y, 2 at the picture's bottom), its y (kind 1), the floor's
    // surface (0 matte, 1 glossy, 2 mirror), the wall's (0 matte, 1 glossy).
    floor: vec4<f32>,
    // Floor blur, floor reflection, how many surfaces, how many spheres.
    more: vec4<f32>,
    // The sky's top colour (linear), Sky light.
    sky_top: vec4<f32>,
    // The sky's bottom colour (linear), w unused.
    sky_bottom: vec4<f32>,
    // The sphere grid: its corner xy (world), cell size, the largest radius.
    grid: vec4<f32>,
    // Cells across, cells down, how far past a sphere its cells reach, w unused.
    grid_size: vec4<f32>,
    // Part 2b: depth of field on (1/0), focus (z), blur, Reflection depth.
    dof: vec4<f32>,
    // How many mirror walls, 1 with a behind picture (glass), 1 with a glowing background's own
    // light picture, 1 in Stage with a light field (balls block it).
    counts: vec4<f32>,
};

struct SurfaceGpu {
    // Roughness, metal, clear coat, see-through.
    a: vec4<f32>,
    // F0 (how much a straight-on look reflects), toon steps (0 smooth), what it mirrors (0
    // nothing, 1 the sky, 2 the scene), kind (0 sheet, 1 ball, 2 shape).
    b: vec4<f32>,
    // Part 2b: rainbow split, soak, film, flakes.
    c: vec4<f32>,
    // Flake size (of a ball's radius), brushing, brush lines (0 spun, 1 straight), brush angle.
    d: vec4<f32>,
    // Frost, rim, inner glow, bends light (index of refraction).
    e: vec4<f32>,
};

struct SphereGpu {
    // Centre xy (world, on the picture plane), radius, see-through.
    a: vec4<f32>,
    // The ball's colour (linear), for the light it lets through; w unused.
    b: vec4<f32>,
    // Its surface index, three unused.
    c: vec4<f32>,
};

struct MirrorGpu {
    // First segment, how many, which side shows the mirror (1 outside, 2 inside, 3 both),
    // strength.
    a: vec4<f32>,
    // Squeeze, the shape's half-size, half its band's thickness, w unused.
    b: vec4<f32>,
    // Tint (linear rgb), w unused.
    c: vec4<f32>,
};

@group(2) @binding(0) var<uniform> st: Stage;
@group(2) @binding(1) var<storage, read> surfaces: array<SurfaceGpu>;
@group(2) @binding(2) var<storage, read> spheres: array<SphereGpu>;
@group(2) @binding(3) var<storage, read> cells: array<vec2<u32>>;
@group(2) @binding(4) var<storage, read> items: array<u32>;
// Part 2b: mirror walls and the picture without what is see-through (what glass shows, bent). A
// glowing background's own light is bound in the mix as `above` (which only the cascades read
// otherwise: a fragment stage takes at most 16 pictures).
@group(2) @binding(5) var<storage, read> mirrors: array<MirrorGpu>;
@group(2) @binding(6) var behind: texture_2d<f32>;

// The flattest the floor is ever seen: tan(0.5 degrees), so it always has some area.
const LEAST_K: f32 = 0.0087;
// The most balls a shadow ray tests in one cell of the sphere grid.
const MOST_IN_CELL: u32 = 96u;
// How many samples a floor reflection's blur takes.
const FLOOR_TAPS: i32 = 24;
// With at most this many balls a ray tests every one (no grid: penumbras of any width).
const BRUTE_BALLS: u32 = 128u;
// How many samples frosted glass blurs what is behind it with, and depth of field a pixel with.
const FROST_TAPS: i32 = 12;
const DOF_TAPS: i32 = 24;
// The widest depth of field blur, in output pixels.
const DOF_MOST: f32 = 32.0;
// The most segments of a mirror wall's outline tested.
const MOST_MIRROR_SEGMENTS: u32 = 512u;

// Set while lighting what a reflection or glass sees: no mirror of its own is traced from there.
var<private> NO_MIRRORS: bool;
// The ball the shaded point is on: its centre xy and radius (radius 0 for none). Flakes and
// brushed lines sit in the ball's own frame.
var<private> SHADE_BALL: vec3<f32>;
// The sphere the shaded point is on (its index; -1 none): its own reflection skips it.
var<private> SHADE_SPHERE: i32;

fn in_stage() -> bool {
    return st.room.x > 0.5;
}

// Where the floor line is (world y), when there is a floor.
fn floor_y() -> f32 {
    if st.floor.x > 1.5 {
        return uv_to_world(vec2<f32>(0.5, 1.0)).y;
    }
    return st.floor.y;
}

// Where the camera looks from, as seen from any point (toward the camera and a little above).
fn view_dir() -> vec3<f32> {
    if !in_stage() {
        return vec3<f32>(0.0, 0.0, 1.0);
    }
    return normalize(vec3<f32>(0.0, -st.room.w, 1.0));
}

// The sky's colour looking along `d` (y down: up is -y).
fn sky(d: vec3<f32>) -> vec3<f32> {
    return mix(st.sky_bottom.rgb, st.sky_top.rgb, smoothstep(-0.35, 0.35, -d.y));
}

// The surface code on a surface-picture texel: its table index, plus 2048 where a Lit 0 layer
// dulls it (part 2b); 0 where nothing is.
fn surface_code(s: vec4<f32>) -> f32 {
    if s.a < 0.02 {
        return 0.0;
    }
    let f = s.b / s.a;
    // Where two things' edges mix, the index is between theirs: no surface rather than a third.
    if abs(f - round(f)) > 0.2 {
        return 0.0;
    }
    return max(round(f), 0.0);
}

// The table entry of the surface on a surface-picture texel.
fn surface_index(s: vec4<f32>) -> u32 {
    let i = u32(surface_code(s) % 2048.0);
    return select(0u, i, i < u32(st.more.z));
}

// The surface on a surface-picture texel; dulled by a Lit 0 layer, it keeps only its kind (and
// takes light as a matte sheet does).
fn surface_of(s: vec4<f32>) -> SurfaceGpu {
    var sf = surfaces[surface_index(s)];
    if surface_code(s) >= 2047.5 {
        let kind = sf.b.w;
        sf = surfaces[0];
        sf.b.w = kind;
    }
    return sf;
}

// A surface the stage's own planes wear: Matte, Glossy or (for the floor) Mirror.
fn plane_surface(kind: f32) -> SurfaceGpu {
    var s: SurfaceGpu;
    s.a = vec4<f32>(1.0, 0.0, 0.0, 0.0);
    s.b = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    if kind > 0.5 {
        let rough = select(0.25, 0.08, kind > 1.5);
        s.a = vec4<f32>(rough, 0.0, 0.0, 0.0);
        s.b = vec4<f32>(0.04, 0.0, 0.0, 0.0);
    }
    return s;
}

// A point being lit: where it is, which way it faces, and the field's distance there (Flat:
// for contact shading; Stage: the shapes' field at its spot).
struct Spot {
    p: vec3<f32>,
    n: vec3<f32>,
};

// The wall or floor point a pixel at world `p` shows in Stage.
fn stage_spot(p: vec2<f32>) -> Spot {
    var out: Spot;
    let back = st.room.y;
    let k = st.room.w;
    if st.floor.x > 0.5 {
        let f = floor_y();
        if p.y >= f - back * k {
            out.p = vec3<f32>(p.x, f, (p.y - f) / max(k, LEAST_K));
            out.n = vec3<f32>(0.0, -1.0, 0.0);
            return out;
        }
    }
    out.p = vec3<f32>(p.x, p.y + back * k, -back);
    out.n = vec3<f32>(0.0, 0.0, 1.0);
    return out;
}

// The blockers' field on the shapes' slab at 3D point `q` (part 2b): the slab is sheared so its
// outline at every depth shows exactly where the shape is drawn (a point (x, y, z) shows at
// (x, y + z k)), so a shape's foot on the wall, and its shadow there, start at its drawn edge.
fn field_on_slab(q: vec3<f32>) -> f32 {
    return field_at(vec2<f32>(q.x, q.y + q.z * st.room.w));
}

// Where a pixel at world `p` showing a thing is in 3D: a shape's front (sheared as its slab), a
// ball's surface at its height `h`.
fn thing_point(p: vec2<f32>, h: f32, shape: bool) -> vec3<f32> {
    if shape {
        return vec3<f32>(p.x, p.y - st.room.z * st.room.w, st.room.z + h);
    }
    return vec3<f32>(p, h);
}

// The sphere whose disc holds world point `p` (the nearest middle), -1 for none.
fn ball_at(p: vec2<f32>) -> i32 {
    let n = u32(st.more.w);
    if n == 0u {
        return -1;
    }
    var best = -1;
    var near = 1e30;
    if n <= BRUTE_BALLS {
        for (var k = 0u; k < n; k++) {
            let sp = spheres[k];
            let d = length(p - sp.a.xy);
            if d <= sp.a.z * 1.02 + 0.5 && d < near {
                near = d;
                best = i32(k);
            }
        }
        return best;
    }
    let cell = st.grid.z;
    let size = vec2<i32>(st.grid_size.xy);
    let c = vec2<i32>(floor((p - st.grid.xy) / cell));
    if any(c < vec2<i32>(0)) || any(c >= size) {
        return -1;
    }
    let range = cells[c.y * size.x + c.x];
    for (var k = 0u; k < min(range.y, MOST_IN_CELL); k++) {
        let i = items[range.x + k];
        let sp = spheres[i];
        let d = length(p - sp.a.xy);
        if d <= sp.a.z * 1.02 + 0.5 && d < near {
            near = d;
            best = i32(i);
        }
    }
    return best;
}

// How far along unit `d` from `o` the ray meets sphere `sp` from outside (1e30 for a miss).
fn ray_sphere(o: vec3<f32>, d: vec3<f32>, sp: SphereGpu) -> f32 {
    let oc = o - vec3<f32>(sp.a.xy, 0.0);
    let r = sp.a.z;
    let b = dot(oc, d);
    let c = dot(oc, oc) - r * r;
    if c <= 0.0 {
        return 1e30;
    }
    let h = b * b - c;
    if h < 0.0 {
        return 1e30;
    }
    let t = -b - sqrt(h);
    return select(1e30, t, t > 1e-3);
}

// The first ball a ray from `o` along unit `d` meets (but ball `skip`): its distance (1e30 for
// none) and index. Every ball with few of them; else the sphere grid, cell by cell, only where the
// ray is within the biggest ball's reach of the picture plane.
fn hit_ball(o: vec3<f32>, d: vec3<f32>, skip: i32) -> vec2<f32> {
    var best = 1e30;
    var index = -1.0;
    let n = u32(st.more.w);
    if n == 0u {
        return vec2<f32>(best, index);
    }
    if n <= BRUTE_BALLS {
        for (var k = 0u; k < n; k++) {
            if i32(k) == skip {
                continue;
            }
            let t = ray_sphere(o, d, spheres[k]);
            if t < best {
                best = t;
                index = f32(k);
            }
        }
        return vec2<f32>(best, index);
    }
    let reach = st.grid.w;
    var s0 = 0.0;
    var s1 = 1e6;
    if abs(d.z) > 1e-6 {
        let a = (-reach - o.z) / d.z;
        let b = (reach - o.z) / d.z;
        s0 = max(s0, min(a, b));
        s1 = min(s1, max(a, b));
    } else if abs(o.z) > reach {
        return vec2<f32>(best, index);
    }
    let cell = st.grid.z;
    let lo = st.grid.xy;
    let hi = lo + st.grid_size.xy * cell;
    for (var axis = 0; axis < 2; axis++) {
        let oo = o[axis];
        let dd = d[axis];
        if abs(dd) > 1e-7 {
            let a = (lo[axis] - oo) / dd;
            let b = (hi[axis] - oo) / dd;
            s0 = max(s0, min(a, b));
            s1 = min(s1, max(a, b));
        } else if oo < lo[axis] || oo > hi[axis] {
            return vec2<f32>(best, index);
        }
    }
    if s1 < s0 {
        return vec2<f32>(best, index);
    }
    let size = vec2<i32>(st.grid_size.xy);
    let q = o.xy + d.xy * s0;
    var c = clamp(vec2<i32>(floor((q - lo) / cell)), vec2<i32>(0), size - 1);
    let go = vec2<i32>(select(-1, 1, d.x >= 0.0), select(-1, 1, d.y >= 0.0));
    var next = vec2<f32>(1e30);
    var delta = vec2<f32>(1e30);
    if abs(d.x) > 1e-7 {
        next.x = (lo.x + f32(c.x + select(0, 1, d.x > 0.0)) * cell - o.x) / d.x;
        delta.x = cell / abs(d.x);
    }
    if abs(d.y) > 1e-7 {
        next.y = (lo.y + f32(c.y + select(0, 1, d.y > 0.0)) * cell - o.y) / d.y;
        delta.y = cell / abs(d.y);
    }
    for (var i = 0; i < 1024; i++) {
        let range = cells[c.y * size.x + c.x];
        for (var k = 0u; k < min(range.y, MOST_IN_CELL); k++) {
            let at = items[range.x + k];
            if i32(at) == skip {
                continue;
            }
            let t = ray_sphere(o, d, spheres[at]);
            if t < best {
                best = t;
                index = f32(at);
            }
        }
        // A ball met in this cell is the nearest once the ray has left the cell.
        if best <= min(next.x, next.y) || min(next.x, next.y) > s1 {
            break;
        }
        if next.x < next.y {
            c.x += go.x;
            next.x += delta.x;
        } else {
            c.y += go.y;
            next.y += delta.y;
        }
        if any(c < vec2<i32>(0)) || any(c >= size) {
            break;
        }
    }
    return vec2<f32>(best, index);
}

// How much of a light gets to `p` past one sphere, going along unit `l` for `dist`: 1 all of it,
// the ball's see-through colour where it is fully in the way. `ang` is the lamp's size as seen
// from `p`.
fn sphere_block(p: vec3<f32>, l: vec3<f32>, dist: f32, ang: f32, sp: SphereGpu) -> vec3<f32> {
    let oc = vec3<f32>(sp.a.xy, 0.0) - p;
    let r = sp.a.z;
    // A point on or in the sphere is its own: a ball shades itself by its normal.
    if dot(oc, oc) <= r * r * 1.0201 + 0.25 {
        return vec3<f32>(1.0);
    }
    // A lamp in the ball (a light riding it) shines out of it.
    let lamp = oc - l * dist;
    if dot(lamp, lamp) <= r * r * 1.0201 {
        return vec3<f32>(1.0);
    }
    let tc = dot(oc, l);
    if tc <= 0.0 || tc >= dist {
        return vec3<f32>(1.0);
    }
    let miss = length(oc - l * tc) - r;
    let v = clamp(0.5 + 0.5 * miss / (tc * max(ang, 1e-4)), 0.0, 1.0);
    let through = sp.b.rgb * clamp(sp.a.w, 0.0, 1.0);
    return mix(through, vec3<f32>(1.0), v * v * (3.0 - 2.0 * v));
}

// How much of a light gets to `p` past every ball, going along unit `l` for `dist` (the lamp
// `stop` beyond where the march ends): the grid's cells are walked one by one along the ray, only
// where it is within the biggest ball's reach of the picture plane.
fn sphere_shadow(p: vec3<f32>, l: vec3<f32>, dist: f32, ang: f32, stop: f32) -> vec3<f32> {
    var vis = vec3<f32>(1.0);
    if st.more.w < 0.5 {
        return vis;
    }
    // Few balls: every one, so no penumbra is cut short at a cell's edge.
    let n = u32(st.more.w);
    if n <= BRUTE_BALLS {
        for (var k = 0u; k < n; k++) {
            vis = min(vis, sphere_block(p, l, dist, ang, spheres[k]));
        }
        return vis;
    }
    let reach = st.grid.w;
    var s0 = 0.0;
    var s1 = dist - max(stop, 0.0);
    if abs(l.z) > 1e-6 {
        let a = (-reach - p.z) / l.z;
        let b = (reach - p.z) / l.z;
        s0 = max(s0, min(a, b));
        s1 = min(s1, max(a, b));
    } else if abs(p.z) > reach {
        return vis;
    }
    let cell = st.grid.z;
    let lo = st.grid.xy;
    let hi = lo + st.grid_size.xy * cell;
    for (var axis = 0; axis < 2; axis++) {
        let o = p[axis];
        let d = l[axis];
        if abs(d) > 1e-7 {
            let a = (lo[axis] - o) / d;
            let b = (hi[axis] - o) / d;
            s0 = max(s0, min(a, b));
            s1 = min(s1, max(a, b));
        } else if o < lo[axis] || o > hi[axis] {
            return vis;
        }
    }
    if s1 < s0 {
        return vis;
    }
    // Amanatides and Woo: from the cell the ray starts in to the next across x or y, whichever
    // boundary comes first, so no cell it crosses is missed.
    let size = vec2<i32>(st.grid_size.xy);
    let q = p.xy + l.xy * s0;
    var c = clamp(vec2<i32>(floor((q - lo) / cell)), vec2<i32>(0), size - 1);
    let go = vec2<i32>(select(-1, 1, l.x >= 0.0), select(-1, 1, l.y >= 0.0));
    var next = vec2<f32>(1e30);
    var delta = vec2<f32>(1e30);
    if abs(l.x) > 1e-7 {
        let edge = lo.x + f32(c.x + select(0, 1, l.x > 0.0)) * cell;
        next.x = (edge - p.x) / l.x;
        delta.x = cell / abs(l.x);
    }
    if abs(l.y) > 1e-7 {
        let edge = lo.y + f32(c.y + select(0, 1, l.y > 0.0)) * cell;
        next.y = (edge - p.y) / l.y;
        delta.y = cell / abs(l.y);
    }
    for (var i = 0; i < 1024; i++) {
        let range = cells[c.y * size.x + c.x];
        // A crowded cell is tested as far as MOST_IN_CELL balls (the grid lists the nearest
        // first-come; a frame-long freeze would be worse than a missing shadow).
        for (var k = 0u; k < min(range.y, MOST_IN_CELL); k++) {
            vis = min(vis, sphere_block(p, l, dist, ang, spheres[items[range.x + k]]));
        }
        if max(vis.x, max(vis.y, vis.z)) <= 0.0 || min(next.x, next.y) > s1 {
            break;
        }
        if next.x < next.y {
            c.x += go.x;
            next.x += delta.x;
        } else {
            c.y += go.y;
            next.y += delta.y;
        }
        if any(c < vec2<i32>(0)) || any(c >= size) {
            break;
        }
    }
    return vis;
}

// How much light the balls near `p` leave it (contact shading in Stage): 1 none taken.
fn sphere_ao(p: vec3<f32>, n: vec3<f32>) -> f32 {
    if st.more.w < 0.5 {
        return 1.0;
    }
    let cell = st.grid.z;
    let size = vec2<i32>(st.grid_size.xy);
    let c = vec2<i32>(floor((p.xy - st.grid.xy) / cell));
    if any(c < vec2<i32>(0)) || any(c >= size) {
        return 1.0;
    }
    let range = cells[c.y * size.x + c.x];
    var occ = 0.0;
    for (var k = 0u; k < min(range.y, 64u); k++) {
        let sp = spheres[items[range.x + k]];
        let d = vec3<f32>(sp.a.xy, 0.0) - p;
        let len = length(d);
        let r = sp.a.z;
        if len <= r * 1.01 {
            continue;
        }
        // A ball darkens out to twice its radius from its edge (the grid lists it that far).
        let near = clamp(1.0 - (len - r) / (2.0 * r), 0.0, 1.0);
        occ += (r * r) / (len * len) * max(dot(n, d / len), 0.0) * near;
    }
    return clamp(1.0 - occ, 0.0, 1.0);
}

// How much light gets to `p` past the shapes in Stage: the blockers' field stood up from the wall
// to the shapes' front (sheared, see `field_on_slab`), marched along the ray while it is within
// that depth.
fn slab_visibility(p: vec3<f32>, l: vec3<f32>, dist: f32, ang: f32, stop: f32) -> f32 {
    let across = length(l.xy);
    let front = st.room.z;
    let back = -st.room.y;
    if across < 1e-5 || (p.z >= front - 0.01 && l.z >= 0.0) {
        return 1.0;
    }
    let toward = l.xy / across;
    let rise = l.z / across;
    var end = dist * across - max(stop, 0.0);
    if rise > 1e-6 {
        end = min(end, (front - p.z) / rise);
    } else if rise < -1e-6 {
        end = min(end, (back - p.z) / rise);
    }
    // In front of the shapes (the floor before them), the ray only meets them once it comes back
    // down to their front: the march starts there, and a ray coming down onto a shape's front face
    // is stopped by it.
    var start = 0.0;
    if p.z > front && rise < -1e-6 {
        start = (front - p.z) / rise;
    }
    if end <= start {
        return 1.0;
    }
    let a = max(ang / max(across, 0.05), 1e-4);
    let texel = max(u.field.z, 1e-3);
    var face = 1.0;
    if start > 0.0 {
        let d = field_on_slab(vec3<f32>(p.xy + toward * start, front));
        face = clamp(0.5 + 0.5 * d / max(start * a, texel), 0.0, 1.0);
        face = face * face * (3.0 - 2.0 * face);
        if face <= 0.0 {
            return 0.0;
        }
    }
    let steps = i32(u.more.z);
    var t = start;
    // Leave the shape the point is on: a thing doesn't shadow itself.
    for (var i = 0; i < steps; i++) {
        let d = field_on_slab(vec3<f32>(p.xy + toward * t, p.z + rise * t));
        if d >= 0.0 {
            break;
        }
        t += max(-d, texel);
    }
    var v = 1.0;
    for (var i = 0; i < steps; i++) {
        if t >= end {
            break;
        }
        let d = field_on_slab(vec3<f32>(p.xy + toward * t, p.z + rise * t));
        v = min(v, clamp(0.5 + 0.5 * d / (max(t - start, texel) * a), 0.0, 1.0));
        if v <= 0.0 {
            break;
        }
        t += max(abs(d), texel);
    }
    return face * v * v * (3.0 - 2.0 * v);
}

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
    let f0 = mix(vec3<f32>(max(sf.b.x, 0.0)), clamp(base, vec3<f32>(0.0), vec3<f32>(1.0)), metal) * film;
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
    let count = u32(u.more.w);
    for (var i = 0u; i < count; i++) {
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
            var v = vec3<f32>(1.0);
            if stage {
                v = sphere_shadow(spot.p, dir3, dist3, soft, stop) * slab_visibility(spot.p, dir3, dist3, soft, stop);
            } else {
                v = vec3<f32>(visibility(p, toward, dist, soft, u.shadows.z, stop));
            }
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
                    sp = ggx(n, v, dir3, rough, f0, angular);
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
    let count = u32(u.more.w);
    for (var i = 0u; i < count; i++) {
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
        || sf.c.y > 0.0 || sf.c.z > 0.0 || sf.c.w > 0.0 || sf.e.y > 0.0 || sf.e.z > 0.0;
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
            let seen = transmitted(p, thing.n, h, sf, tint, turn);
            out.out = body * (1.0 - see) + seen * see + sh.spec * share;
        }
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
            let seen = transmitted(p, thing.n, h, sf, tint, turn);
            on_thing.out = body * (1.0 - see) + seen * see + sh.spec * share;
        }
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
        let blocks_here = sv.r > 0.5 || (st.counts.w > 0.5 && sv.r > 0.12);
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

// ---------- depth of field (part 2b, Stage) ----------

// How deep (z, toward the camera) the stage is at pixel `ip`: a ball's surface, a shape's front,
// or the wall or floor it shows.
fn depth_at(ip: vec2<i32>) -> f32 {
    let size = vec2<i32>(textureDimensions(surface));
    let at = clamp(ip, vec2<i32>(0), size - 1);
    let uv = (vec2<f32>(at) + 0.5) / vec2<f32>(size);
    let s = textureLoad(surface, at, 0);
    var z = stage_spot(uv_to_world(uv)).p.z;
    if s.a > 0.02 {
        var zt = s.g / s.a;
        if surface_of(s).b.w > 1.5 {
            zt += st.room.z;
        }
        z = mix(z, zt, smoothstep(0.02, 0.98, s.a));
    }
    return z;
}

// How wide the blur is at depth `z`, in output pixels.
fn blur_at(z: f32) -> f32 {
    return min(abs(z - st.dof.y) * st.dof.z / max(u.view.z, 1e-6), DOF_MOST);
}

@fragment
fn fs_dof(in: FsIn) -> @location(0) vec4<f32> {
    let ip = vec2<i32>(in.pos.xy);
    let c = textureLoad(picture, ip, 0);
    let own = blur_at(depth_at(ip));
    if own < 0.5 {
        return c;
    }
    let size = vec2<i32>(textureDimensions(picture));
    let turn = 6.2831853 * fract(52.9829189 * fract(dot(in.pos.xy, vec2<f32>(0.06711056, 0.00583715))));
    var sum = c.rgb;
    var weights = 1.0;
    // The bake takes more taps: smoother bokeh.
    let taps = select(DOF_TAPS, 4 * DOF_TAPS, baking());
    for (var k = 0; k < taps; k++) {
        let a = f32(k) * 2.39996 + turn;
        let r = own * sqrt((f32(k) + 0.5) / f32(taps));
        let at = clamp(ip + vec2<i32>(round(vec2<f32>(cos(a), sin(a)) * r)), vec2<i32>(0), size - 1);
        let tc = textureLoad(picture, at, 0).rgb;
        // A tap counts only where its own blur reaches this pixel, so a sharp thing isn't smeared
        // by a blurred one behind it; bright taps count more (round bokeh).
        let reach = clamp(blur_at(depth_at(at)) - r + 1.0, 0.0, 1.0);
        let bright = 1.0 + max(dot(tc, vec3<f32>(0.2126, 0.7152, 0.0722)) - 1.0, 0.0);
        let wt = reach * bright;
        sum += tc * wt;
        weights += wt;
    }
    return vec4<f32>(sum / weights, c.a);
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

// ---------- the glossy floor (Stage, a floor that isn't Matte) ----------

// The lit picture round world point `q`, blurred over `radius`: a Vogel disc of FLOOR_TAPS
// samples turned by a per-pixel noise `turn`, so a wide blur is soft grain, not copies. With
// `things` false the things on the picture are left out (the wall mirrors only the wall). Its
// alpha is how much was found.
fn lit_near(q: vec2<f32>, radius: f32, turn: f32, things: bool) -> vec4<f32> {
    var sum = vec3<f32>(0.0);
    var weight = 0.0;
    for (var k = 0; k < FLOOR_TAPS; k++) {
        let a = f32(k) * 2.39996 + turn;
        let o = vec2<f32>(cos(a), sin(a)) * radius * sqrt((f32(k) + 0.5) / f32(FLOOR_TAPS));
        let uv = world_to_uv(q + o);
        let inside = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
        let off = length(uv - inside);
        var wt = clamp(1.0 - off * 20.0, 0.0, 1.0);
        if !things {
            wt *= 1.0 - clamp(textureSampleLevel(surface, samp, inside, 0.0).a, 0.0, 1.0);
        }
        sum += textureSampleLevel(picture, samp, inside, 0.0).rgb * wt;
        weight += wt;
    }
    return vec4<f32>(sum / max(weight, 1e-4), clamp(weight / f32(FLOOR_TAPS) * 2.0, 0.0, 1.0));
}

@fragment
fn fs_floor(in: FsIn) -> @location(0) vec4<f32> {
    let ip = vec2<i32>(in.pos.xy);
    let lit = textureLoad(picture, ip, 0);
    let s = textureLoad(surface, ip, 0);
    let p = uv_to_world(in.uv);
    let back = st.room.y;
    let f = floor_y();
    let seam = f - back * st.room.w;
    let open = 1.0 - clamp(s.a, 0.0, 1.0);
    if st.floor.x < 0.5 || p.y < seam || open <= 0.0 {
        return lit;
    }
    let k = max(st.room.w, LEAST_K);
    let mirror = st.floor.z > 1.5;
    let blur = select(max(st.more.x, 0.0), 0.0, mirror);
    // In front of the picture plane the floor mirrors what stands on it; behind it, the wall.
    var refl = vec4<f32>(0.0);
    // Interleaved gradient noise (Jimenez): a different turn of the blur's taps at each pixel.
    let turn = 6.2831853 * fract(52.9829189 * fract(dot(in.pos.xy, vec2<f32>(0.06711056, 0.00583715))));
    let wall_q = vec2<f32>(p.x, 2.0 * seam - p.y);
    let wall = lit_near(wall_q, blur * (p.y - seam) * 0.5, turn, false);
    if p.y >= f {
        let q = vec2<f32>(p.x, 2.0 * f - p.y);
        let near = lit_near(q, blur * (p.y - f) * 0.5, turn, true);
        let held = textureSampleLevel(surface, samp, clamp(world_to_uv(q), vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).a;
        refl = mix(wall, near, clamp(held, 0.0, 1.0));
    } else {
        refl = wall;
    }
    // Fresnel at the floor's one viewing angle.
    let cosine = k / sqrt(1.0 + k * k);
    var amount = 0.04 + 0.96 * pow(1.0 - cosine, 5.0);
    if mirror {
        amount = 0.9;
    }
    amount *= max(st.more.y, 0.0) * open * refl.a;
    return vec4<f32>(mix(lit.rgb, refl.rgb, clamp(amount, 0.0, 1.0)), lit.a);
}

// ---------- lighting part 3: the bake ----------
//
// A baked frame is drawn several times and averaged (Renderer::render_samples); each sample's
// light passes get its index in `u.sample`, and these random numbers (only used when baking, so
// the live picture never changes) differ from sample to sample: blurred reflections, rainbows,
// the path-traced bounce light (`fs_trace`, Stage) and more caustic photons.

var<private> RNG: u32;

fn baking() -> bool {
    return u.sample.z > 0.5;
}

// PCG hash (Jarzynski and Olano).
fn pcg(v: u32) -> u32 {
    let state = v * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

// Starts this pixel's random numbers for this sample (`salt` tells passes apart).
fn seed_rng(ip: vec2<i32>, salt: u32) {
    RNG = pcg(u32(ip.x) * 1973u + u32(ip.y) * 9277u + u32(u.sample.x) * 26699u + salt * 7919u);
}

// A random number, 0 to just under 1.
fn rand() -> f32 {
    RNG = pcg(RNG);
    return f32(RNG >> 8u) / 16777216.0;
}

// A wavelength's colour for `w` from -1 (red) to 1 (violet); it averages to white over w.
fn spectral(w: f32) -> vec3<f32> {
    let r = max(1.0 - abs(w + 0.6) / 0.6, 0.0);
    let g = max(1.0 - abs(w) / 0.6, 0.0);
    let b = max(1.0 - abs(w - 0.6) / 0.6, 0.0);
    return vec3<f32>(r * 3.5294, g * 3.3333, b * 3.5294);
}

// A direction round unit `n`, more often near it (cosine-weighted).
fn cosine_dir(n: vec3<f32>) -> vec3<f32> {
    let u1 = rand();
    let phi = 6.2831853 * rand();
    let r = sqrt(u1);
    let up = select(vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(1.0, 0.0, 0.0), abs(n.z) > 0.9);
    let t = normalize(cross(up, n));
    let b = cross(n, t);
    return normalize(t * (r * cos(phi)) + b * (r * sin(phi)) + n * sqrt(max(1.0 - u1, 0.0)));
}

// What a ray meets: 0 nothing, 1 a ball, 2 the wall, 3 the floor, 4 a shape's front; where, its
// normal, its colour, the ball (-1 none) and its surface.
struct Hit {
    what: i32,
    q: vec3<f32>,
    n: vec3<f32>,
    c: vec3<f32>,
    ball: i32,
    sf: SurfaceGpu,
};

// The first thing a ray from `o` along unit `d` meets (ball `skip` left out), as trace_scene finds
// it.
fn scene_hit(o: vec3<f32>, d: vec3<f32>, skip: i32) -> Hit {
    var h: Hit;
    h.what = 0;
    h.ball = -1;
    let ball = hit_ball(o, d, skip);
    var t = ball.x;
    var what = select(0, 1, ball.y >= 0.0);
    if in_stage() {
        if d.z < -1e-5 {
            let tw = (-st.room.y - o.z) / d.z;
            if tw > 1e-3 && tw < t {
                t = tw;
                what = 2;
            }
        }
        if st.floor.x > 0.5 && d.y > 1e-5 {
            let tf = (floor_y() - o.y) / d.y;
            if tf > 1e-3 && tf < t && (o.z + d.z * tf) >= -st.room.y {
                t = tf;
                what = 3;
            }
        }
        if d.z < -1e-5 && o.z > st.room.z {
            let ts = (st.room.z - o.z) / d.z;
            if ts > 1e-3 && ts < t && field_on_slab(o + d * ts) < 0.0 {
                t = ts;
                what = 4;
            }
        }
    }
    if what == 0 {
        return h;
    }
    let q = o + d * t;
    h.what = what;
    h.q = q;
    if what == 1 {
        let at = u32(ball.y);
        let sp = spheres[at];
        h.n = normalize(q - vec3<f32>(sp.a.xy, 0.0));
        let index = u32(max(sp.c.x, 0.0));
        h.sf = surfaces[select(0u, index, index < u32(st.more.z))];
        h.c = sp.b.rgb;
        if h.n.z > 0.2 {
            h.c = picture_at(vec3<f32>(q.xy, 0.0));
        }
        h.ball = i32(at);
        return h;
    }
    h.n = vec3<f32>(0.0, 0.0, 1.0);
    h.sf = plane_surface(st.floor.w);
    if what == 3 {
        h.n = vec3<f32>(0.0, -1.0, 0.0);
        h.sf = plane_surface(st.floor.z);
    } else if what == 4 {
        let uv = world_to_uv(vec2<f32>(q.x, q.y + q.z * st.room.w));
        let size = vec2<f32>(textureDimensions(surface));
        h.sf = surface_of(textureLoad(surface, vec2<i32>(clamp(uv, vec2<f32>(0.0), vec2<f32>(0.999)) * size), 0));
    }
    h.c = picture_at(q);
    return h;
}

// Light `l` toward 3D point `q`: the unit direction to the lamp, how far (1e5 for the sun) and
// how much of it arrives (falloff, cone, an ultraviolet light's visible share); lasers none.
struct Toward {
    dir: vec3<f32>,
    dist: f32,
    amount: f32,
};

fn light_toward(l: LightGpu, q: vec3<f32>) -> Toward {
    var out: Toward;
    out.amount = 0.0;
    out.dir = vec3<f32>(0.0, 0.0, 1.0);
    out.dist = 1e5;
    let kind = l.p0.w;
    if kind > 4.5 {
        return out;
    }
    if kind > 1.5 && kind < 2.5 {
        let el_sin = clamp(l.p0.z, -1.0, 1.0);
        out.dir = normalize(vec3<f32>(-l.p3.xy * sqrt(max(1.0 - el_sin * el_sin, 0.0)), el_sin));
        out.amount = 1.0;
    } else {
        let d = source_point(l, q.xy) - q.xy;
        let to_lamp = vec3<f32>(d, l.p0.z - q.z);
        let len3 = length(to_lamp);
        out.amount = falloff(len3, l.p2.x, l.p2.y);
        if kind > 0.5 && kind < 1.5 {
            let dist = length(d);
            let cosine = select(dot(-d / max(dist, 1e-3), l.p3.xy), 1.0, dist <= 1e-3);
            let e = clamp((cosine - l.p3.w) / max(l.p3.z - l.p3.w, 1e-6), 0.0, 1.0);
            out.amount *= e * e * (3.0 - 2.0 * e);
        }
        out.dir = select(vec3<f32>(0.0, 0.0, 1.0), to_lamp / len3, len3 > 1e-6);
        out.dist = len3;
    }
    if l.p7.y > 0.5 {
        out.amount *= max(l.p7.z, 0.0);
    }
    return out;
}

// The lamps' light falling on 3D point `q` facing `n` (no ambient: that is light from everywhere
// already), each shadowed by the balls when it casts shadows.
fn lamps_on(q: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    let count = u32(u.more.w);
    for (var i = 0u; i < count; i++) {
        let l = lights[i];
        let to = light_toward(l, q);
        let ndl = dot(n, to.dir);
        if to.amount == 0.0 || ndl <= 0.0 {
            continue;
        }
        var vis = vec3<f32>(1.0);
        if l.p5.w > 0.5 {
            let ang = max(l.p2.z / max(to.dist, 1.0), 0.01);
            vis = sphere_shadow(q + n * 0.5, to.dir, to.dist, ang, l.p6.x);
        }
        sum += l.p1.rgb * to.amount * ndl * vis;
    }
    return sum;
}

// How many rays a pixel the bake's bounce light takes each sample.
const TRACE_RAYS: i32 = 2;

// The bake's bounce light in Stage (part 3): from the point each pixel shows (a ball's surface, a
// shape's front, the wall or floor), cosine-weighted rays walk up to Light bounces hits, each hit
// lit by the lamps (with the balls' shadows) and carrying its colour on. The light arriving,
// times Bounce light; the mix adds it as it adds the background's own light.
@fragment
fn fs_trace(in: FsIn) -> @location(0) vec4<f32> {
    let ip = vec2<i32>(in.pos.xy);
    seed_rng(ip, 17u);
    let p = uv_to_world(in.uv);
    let s = textureLoad(surface, ip, 0);
    var q: vec3<f32>;
    var n: vec3<f32>;
    var skip = -1;
    if s.a > 0.5 {
        let sf = surface_of(s);
        let px = u.view.z;
        let h = height_at(ip);
        let hx = height_at(ip + vec2<i32>(1, 0)) - height_at(ip - vec2<i32>(1, 0));
        let hy = height_at(ip + vec2<i32>(0, 1)) - height_at(ip - vec2<i32>(0, 1));
        var slope = vec2<f32>(hx, hy) / (2.0 * px);
        if u.camera.w != 0.0 {
            slope = rotate_by(slope, u.camera.w);
        }
        n = normalize(vec3<f32>(-slope, 1.0));
        q = thing_point(p, h, sf.b.w > 1.5);
        if sf.b.w > 0.5 && sf.b.w < 1.5 {
            skip = ball_at(p);
        }
    } else {
        let spot = stage_spot(p);
        q = spot.p;
        n = spot.n;
    }
    let bounces = clamp(i32(u.sample.w), 1, 16);
    var sum = vec3<f32>(0.0);
    for (var r = 0; r < TRACE_RAYS; r++) {
        var ro = q + n * 0.75;
        var nn = n;
        var carried = vec3<f32>(1.0);
        var sk = skip;
        for (var b = 0; b < 16; b++) {
            if b >= bounces {
                break;
            }
            let hit = scene_hit(ro, cosine_dir(nn), sk);
            if hit.what == 0 {
                break;
            }
            let c = clamp(hit.c, vec3<f32>(0.0), vec3<f32>(0.95));
            let metal = clamp(hit.sf.a.y, 0.0, 1.0);
            sum += carried * c * (lamps_on(hit.q, hit.n) * (1.0 - metal) + vec3<f32>(max(hit.sf.e.z, 0.0)));
            carried *= c * (1.0 - metal);
            ro = hit.q + hit.n * 0.75;
            nn = hit.n;
            sk = hit.ball;
        }
    }
    let out = sum / f32(TRACE_RAYS) * u.trace.x;
    return vec4<f32>(clamp(out, vec3<f32>(-6.0e4), vec3<f32>(6.0e4)), 0.0);
}

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// The bake's noise clean-up (part 3): one à-trous pass over `picture` (the traced light), 5 x 5
// taps `u.field.w` texels apart, each weighed down where the surface picture changes (cover,
// surface, height) or the light itself jumps; a step of 0 copies.
fn denoised(pos: vec2<f32>) -> vec4<f32> {
    let ip = vec2<i32>(pos);
    let step = i32(u.field.w);
    let c0 = textureLoad(picture, ip, 0);
    if step <= 0 {
        return vec4<f32>(c0.rgb, 0.0);
    }
    let s0 = textureLoad(surface, ip, 0);
    let code0 = surface_code(s0);
    let l0 = luma(c0.rgb);
    let size = vec2<i32>(textureDimensions(picture));
    let kernel = array<f32, 3>(0.375, 0.25, 0.0625);
    let height_scale = u.view.z * 3.0 * f32(step) + 1.0;
    var sum = vec3<f32>(0.0);
    var weights = 0.0;
    for (var dy = -2; dy <= 2; dy++) {
        for (var dx = -2; dx <= 2; dx++) {
            let at = clamp(ip + vec2<i32>(dx, dy) * step, vec2<i32>(0), size - 1);
            let c = textureLoad(picture, at, 0).rgb;
            let sq = textureLoad(surface, at, 0);
            var w = kernel[abs(dx)] * kernel[abs(dy)];
            w *= exp(-abs(sq.a - s0.a) * 8.0);
            w *= select(0.1, 1.0, abs(surface_code(sq) - code0) < 0.5);
            w *= exp(-abs(sq.g - s0.g) / height_scale);
            w *= exp(-abs(luma(c) - l0) / (0.5 * abs(l0) + 0.05));
            sum += c * w;
            weights += w;
        }
    }
    return vec4<f32>(sum / max(weights, 1e-6), 0.0);
}

@fragment
fn fs_denoise(in: FsIn) -> @location(0) vec4<f32> {
    return denoised(in.pos.xy);
}

// The last clean-up pass (or the copy), added into the background-light picture.
@fragment
fn fs_trace_add(in: FsIn) -> @location(0) vec4<f32> {
    return denoised(in.pos.xy);
}

// ---------- caustics (part 3): light focused through glass balls ----------

struct CausticU {
    // Photons a (light, glass ball) pair, glass balls, lights, 0.
    counts: vec4<f32>,
    // World units per output pixel, this sample's turn of the photons, 1 when baking, Glass
    // caustics.
    more: vec4<f32>,
};

struct CausticGlass {
    // Centre xy, radius, see-through.
    a: vec4<f32>,
    // The colour it lets through (normalised, linear rgb), bends light.
    b: vec4<f32>,
    // Rainbow split, frost, two spare.
    c: vec4<f32>,
};

@group(3) @binding(0) var<uniform> cu: CausticU;
@group(3) @binding(1) var<storage, read> glass: array<CausticGlass>;

struct CausticOut {
    @builtin(position) pos: vec4<f32>,
    // Where in the photon's soft disc (-1 to 1 across).
    @location(0) local: vec2<f32>,
    // Its light, already over the disc's area.
    @location(1) power: vec3<f32>,
};

// Photon `k`: from its light toward a point of its glass ball's disc (a Fibonacci spiral), bent
// in and out of the sphere (its own wavelength with Rainbow split), landing on the wall, the floor
// (Stage) or the plane under the ball (Flat); drawn as a small soft disc where that point shows.
@vertex
fn vs_caustic(@builtin(vertex_index) vi: u32, @builtin(instance_index) k: u32) -> CausticOut {
    var out: CausticOut;
    out.pos = vec4<f32>(4.0, 4.0, 0.5, 1.0);
    out.local = vec2<f32>(0.0);
    out.power = vec3<f32>(0.0);
    let per = max(u32(cu.counts.x), 1u);
    let balls = max(u32(cu.counts.y), 1u);
    let pair = k / per;
    let j = k % per;
    let li = pair / balls;
    let gi = pair % balls;
    if li >= u32(cu.counts.z) {
        return out;
    }
    let l = lights[li];
    let g = glass[gi];
    let c = vec3<f32>(g.a.xy, 0.0);
    let r = g.a.z;
    let to = light_toward(l, c);
    if to.amount == 0.0 || r <= 0.0 {
        return out;
    }
    let sun = to.dist >= 1e5 - 1.0;
    if !sun && to.dist <= r * 1.02 {
        return out;
    }
    let dir = -to.dir;
    let up = select(vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(1.0, 0.0, 0.0), abs(dir.z) > 0.9);
    let ax = normalize(cross(dir, up));
    let ay = cross(dir, ax);
    let ang = f32(j) * 2.39996323 + cu.more.y;
    let rad = r * 0.999 * sqrt((f32(j) + 0.5) / f32(per));
    let aim_at = c + (ax * cos(ang) + ay * sin(ang)) * rad;
    var o = c - dir * (4.0 * r) + (aim_at - c);
    var rd = dir;
    if !sun {
        o = c + to.dir * to.dist;
        rd = normalize(aim_at - o);
    }
    let oc = o - c;
    let b = dot(oc, rd);
    let disc = b * b - (dot(oc, oc) - r * r);
    if disc < 0.0 {
        return out;
    }
    let p1 = o + rd * (-b - sqrt(disc));
    let n1 = (p1 - c) / r;
    // Its wavelength: -1 red to 1 violet, red bending least.
    RNG = pcg(k * 7919u + u32(cu.more.y * 1000.0));
    let w = select(0.0, rand() * 2.0 - 1.0, g.c.x > 0.0);
    let eta = max(g.b.w * (1.0 + max(g.c.x, 0.0) * 0.04 * w), 1.0001);
    let d2 = refract(rd, n1, 1.0 / eta);
    if dot(d2, d2) < 1e-6 {
        return out;
    }
    let p2 = p1 + d2 * (-2.0 * dot(p1 - c, d2));
    let n2 = (p2 - c) / r;
    let d3 = refract(d2, -n2, eta);
    if dot(d3, d3) < 1e-6 {
        return out;
    }
    // Where it lands.
    var t = 1e30;
    var on_floor = false;
    if in_stage() {
        if d3.z < -1e-5 {
            let tw = (-st.room.y - p2.z) / d3.z;
            if tw > 0.0 {
                t = tw;
            }
        }
        if st.floor.x > 0.5 && d3.y > 1e-5 && st.room.w > LEAST_K {
            let tf = (floor_y() - p2.y) / d3.y;
            if tf > 0.0 && tf < t && (p2.z + d3.z * tf) >= -st.room.y {
                t = tf;
                on_floor = true;
            }
        }
    } else if d3.z < -1e-5 {
        t = (-r - p2.z) / d3.z;
    }
    if t >= 1e29 {
        return out;
    }
    let q = p2 + d3 * t;
    let px = cu.more.x;
    let radius = max(2.0 * r * sqrt(3.14159265 / f32(per)), 1.5 * px) * (1.0 + 4.0 * max(g.c.y, 0.0));
    var power = l.p1.rgb * to.amount * (3.14159265 * r * r / f32(per));
    power *= clamp(g.a.w, 0.0, 1.0) * 0.92 * g.b.rgb * cu.more.w;
    if g.c.x > 0.0 {
        power *= spectral(w);
    }
    power *= 3.0 / (3.14159265 * radius * radius);
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u)) * 2.0 - vec2<f32>(1.0);
    let squash = select(1.0, st.room.w, on_floor);
    let at = vec2<f32>(q.x, q.y + q.z * st.room.w) + vec2<f32>(corner.x, corner.y * squash) * radius;
    let uv = world_to_uv(at);
    out.pos = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.5, 1.0);
    out.local = corner;
    out.power = power;
    return out;
}

@fragment
fn fs_caustic(in: CausticOut) -> @location(0) vec4<f32> {
    let d = dot(in.local, in.local);
    if d >= 1.0 {
        discard;
    }
    let s = textureLoad(surface, vec2<i32>(in.pos.xy), 0);
    // Nothing in front: in Stage anything covering the wall, in Flat a ball (it has height).
    var open = 1.0 - clamp(s.g * 0.5, 0.0, 1.0);
    if in_stage() {
        open = 1.0 - clamp(s.a, 0.0, 1.0);
    }
    let k = (1.0 - d) * (1.0 - d);
    return vec4<f32>(in.power * k * open, 0.0);
}

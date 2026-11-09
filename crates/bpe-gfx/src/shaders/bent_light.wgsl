
// ---------- light bent by glass (glass and light part 3, bent_light.rs) ----------
//
// Pasted after the lighting shader. For each (light, bender) pair `cs_bent` sends photons from the light
// toward the bender's box: each steps through the picture (by the blockers' field in the open, half
// a pixel near and in things), bends in and out of glass that bends light (`bender_at`), takes the
// colour of what it passes through, and from where it first leaves glass keeps each straight
// stretch of its way as a leg. `vs_bent`/`fs_bent` splat the legs as soft lines into the direct
// light and the light in the air, where the direct pass counted the glass as solid.

const BENT_LEGS: u32 = 8u;
// How many wavelengths a fan's photons take in turn (once glass splits light): a photon's
// neighbours of its own colour are this many apart in its fan.
const BENT_COLOURS: u32 = 16u;
// Added to a leg's path key once its photon's light is split into colours.
const RAINBOW_KEY: u32 = 2097152u;
// What a photon finds at a pixel (a bender id, 0 and up, is glass that bends).
const BENT_OPEN: i32 = -1;
const BENT_OFF: i32 = -2;
const BENT_SEE: i32 = -3;
const BENT_SOLID: i32 = -4;
// Glass and light part 4a: a thing that throws light is `BENT_THROW - its surface index` (every
// kind at or under this).
const BENT_THROW: i32 = -16;
const MOST_THROWERS: u32 = 64u;

struct ThrowerGpu {
    // Its surface index, 1 for balls, Facets, Facet gaps.
    a: vec4<f32>,
    // What it gives back (linear rgb, times Throws light), how fast it blurs (Roughness squared).
    b: vec4<f32>,
};

struct BentU {
    // Photons a pair, pairs, the most steps a photon takes, spare.
    counts: vec4<f32>,
    // World units per output pixel, this sample's turn of the photons, half a photon line's width
    // (world), Glass caustics.
    more: vec4<f32>,
    // Part 4a: Mirror light, how many things throw light, two spare.
    extra: vec4<f32>,
    // Which surface indices throw light (a bit each).
    throws: array<vec4<u32>, 16>,
    // The things that throw light.
    table: array<ThrowerGpu, 64>,
};

struct BentPair {
    // The bender's box: lo xy, hi xy (world).
    a: vec4<f32>,
    // Its bender id (a thrower's surface index), the light's index, 1 for a thrower's pair, spare.
    b: vec4<f32>,
};

struct Leg {
    // From xy, to xy (world).
    a: vec4<f32>,
    // The path length from the light at each end, its share in the haze, the light's index + 1
    // (0: an empty leg).
    b: vec4<f32>,
    // Its light (linear rgb, before the falloff), its path key.
    c: vec4<f32>,
    // Part 4a: how much a rough mirror has blurred it at each end (world, added to its half
    // width), two spare.
    d: vec4<f32>,
};

@group(3) @binding(4) var<uniform> bu: BentU;
@group(3) @binding(5) var<storage, read> bent_pairs: array<BentPair>;
@group(3) @binding(6) var<storage, read_write> legs_out: array<Leg>;
@group(3) @binding(7) var<storage, read> legs_in: array<Leg>;

struct BentAt {
    // BENT_OPEN, BENT_OFF, BENT_SEE, BENT_SOLID or a bender id.
    kind: i32,
    // How much the pixel blocks.
    block: f32,
};

// The surface index of the thing that throws light covering picture pixel `ip` (part 4a), -1 for
// none: at least half covered by a surface whose bit is set.
fn thrower_at(ip: vec2<i32>) -> i32 {
    return thrower_covering(ip, 0.5);
}

// `thrower_at` for a pixel covered at least `least`: a mirror's antialiased edge blocks whole, so
// a photon meeting it there meets the mirror (wherever the edge falls on the pixels).
fn thrower_covering(ip: vec2<i32>, least: f32) -> i32 {
    if bu.extra.y < 0.5 {
        return -1;
    }
    let s = textureLoad(surface, ip, 0);
    if s.a < least || s.b < 0.0 {
        return -1;
    }
    let f = s.b / s.a;
    let code = round(f);
    if abs(f - code) > 0.2 || code < 0.5 {
        return -1;
    }
    let i = u32(code) % 2048u;
    let word = bu.throws[i / 128u][(i / 32u) % 4u];
    if (word & (1u << (i % 32u))) == 0u {
        return -1;
    }
    return i32(i);
}

// The table entry of the thing that throws light with surface index `id`.
fn thrower_entry(id: i32) -> ThrowerGpu {
    let n = min(u32(bu.extra.y), MOST_THROWERS);
    for (var k = 0u; k < n; k++) {
        if i32(bu.table[k].a.x + 0.5) == id {
            return bu.table[k];
        }
    }
    return ThrowerGpu(vec4<f32>(f32(id), 0.0, 0.0, 0.0), vec4<f32>(0.0));
}

// What a photon at world point `q` is in (`fd` the field there).
fn bent_at(q: vec2<f32>, fd: vec4<f32>) -> BentAt {
    let ip = pixel_of(q);
    if ip.x < 0 {
        return BentAt(BENT_OFF, 0.0);
    }
    let b = pixel_block(ip);
    let g = bender_at(ip);
    if g >= 0 {
        return BentAt(g, b);
    }
    let t = thrower_at(ip);
    if t >= 0 {
        return BentAt(BENT_THROW - t, b);
    }
    if u.trace.y > 0.5 {
        // A Stage's things block whole (its balls, which mark a quarter, are spheres).
        return BentAt(select(BENT_OPEN, BENT_SOLID, b >= 0.3), b);
    }
    if b >= BLOCK_LEAST {
        let edge = thrower_covering(ip, 0.02);
        if edge >= 0 {
            return BentAt(BENT_THROW - edge, b);
        }
    }
    if b >= SOLID {
        return BentAt(BENT_SOLID, b);
    }
    if b >= BLOCK_LEAST {
        // A bending glass's soft edge is open: the glass itself bends and colours the light.
        if bender_covering(ip, 0.02) >= 0 {
            return BentAt(BENT_OPEN, b);
        }
        return BentAt(BENT_SEE, b);
    }
    if fd.x < 0.0 && clear_tint(fd.yzw) {
        // Clear coloured glass: only the field knows it.
        return BentAt(BENT_SEE, 0.0);
    }
    return BentAt(BENT_OPEN, b);
}

// How much of picture pixel `ip` is glass of bender `id`.
fn glass_cover(ip: vec2<i32>, id: i32) -> f32 {
    let size = vec2<i32>(textureDimensions(surface));
    if any(ip < vec2<i32>(0)) || any(ip >= size) {
        return 0.0;
    }
    let s = textureLoad(surface, ip, 0);
    if id == FREE_BENDER {
        return free_glass_of(s).y;
    }
    if s.a < 0.02 || s.b < 0.0 {
        return 0.0;
    }
    let f = s.b / s.a;
    let code = round(f);
    if abs(f - code) > 0.2 || code < 0.5 {
        return 0.0;
    }
    return select(0.0, s.a, i32(u32(code) % 2048u) == id);
}

// The outward normal (world) of bender `id`'s glass at world point `q`: down the slope of its cover
// over the 5 x 5 pixels round, or the blockers' field's slope where that is flat.
fn glass_normal(q: vec2<f32>, id: i32) -> vec2<f32> {
    let uv = clamp(world_to_uv(q), vec2<f32>(0.0), vec2<f32>(0.9999));
    let size = vec2<i32>(textureDimensions(surface));
    let ip = vec2<i32>(uv * vec2<f32>(size));
    var g = vec2<f32>(0.0);
    for (var dy = -2; dy <= 2; dy++) {
        for (var dx = -2; dx <= 2; dx++) {
            g += vec2<f32>(f32(dx), f32(dy)) * glass_cover(ip + vec2<i32>(dx, dy), id);
        }
    }
    if dot(g, g) > 1e-4 {
        var n = -normalize(g);
        if u.camera.w != 0.0 {
            n = rotate_by(n, u.camera.w);
        }
        return n;
    }
    let e = max(u.field.z, u.view.z);
    let f = vec2<f32>(
        field_at(q + vec2<f32>(e, 0.0)) - field_at(q - vec2<f32>(e, 0.0)),
        field_at(q + vec2<f32>(0.0, e)) - field_at(q - vec2<f32>(0.0, e)),
    );
    if dot(f, f) > 1e-12 {
        return normalize(f);
    }
    return vec2<f32>(0.0, -1.0);
}

// Bender `id`'s index of refraction at pixel `ip` (free glass carries its own in the surface
// picture), rainbow split and frost.
fn glass_of(id: i32, ip: vec2<i32>) -> vec3<f32> {
    if id == FREE_BENDER {
        let size = vec2<i32>(textureDimensions(surface));
        let n = free_glass_of(textureLoad(surface, clamp(ip, vec2<i32>(0), size - 1), 0)).x;
        return vec3<f32>(max(n, BENDS_FROM), 0.0, 0.0);
    }
    if u32(id) >= u32(st.more.z) {
        return vec3<f32>(BENDS_FROM, 0.0, 0.0);
    }
    let sf = surfaces[u32(id)];
    return vec3<f32>(max(sf.e.w, BENDS_FROM), max(sf.c.x, 0.0), max(sf.e.x, 0.0));
}

// Glass and light part 4b: what a dichroic surface `id` lets through of light meeting it going
// `dir` against `facing` (1 for free glass or a surface that isn't dichroic).
fn dichroic_into(id: i32, dir: vec2<f32>, facing: vec2<f32>) -> vec3<f32> {
    if id == FREE_BENDER || id < 0 || u32(id) >= u32(st.more.z) {
        return vec3<f32>(1.0);
    }
    return dichroic_through(surfaces[u32(id)].g, abs(dot(dir, facing)));
}

// The colour light thrown off mirror `id` takes (part 4b): the opposite hue where it's dichroic.
fn dichroic_off(id: i32, dir: vec2<f32>, facing: vec2<f32>) -> vec3<f32> {
    if id < 0 || u32(id) >= u32(st.more.z) {
        return vec3<f32>(1.0);
    }
    return dichroic_back(surfaces[u32(id)].g, abs(dot(dir, facing)), 1.0);
}

// How soft light `l`'s shadows are: half its lamp's width (a sun's: the tangent of half its
// angle), 0 unless shadows are soft.
fn lamp_soft(l: LightGpu) -> f32 {
    if u.shadows.x >= 0.5 {
        return 0.0;
    }
    let size = select(max(l.p2.z, 0.0), max(l.p6.y, 0.0), l.p0.w > 1.5 && l.p0.w < 2.5);
    return size * max(u.shadows.y, 0.0) * max(l.p6.w, 0.0);
}

// How many points across its lamp a pair of light `l` sends its `per` photons from: a fan from
// each (so neighbouring photons in one fan tell how far apart they have spread), at least 256
// photons a fan, at most 16 fans.
fn lamp_copies(l: LightGpu, per: u32) -> u32 {
    if lamp_soft(l) <= 0.0 {
        return 1u;
    }
    return clamp(per / 256u, 1u, 16u);
}

// The way a photon's path has gone so far, as a number: the same for two photons that met the
// same things in the same order (`what` the medium or event met next).
fn path_key(key: u32, what: i32) -> u32 {
    return (key * 31u + u32(what + 16)) % 1048573u;
}

// Writes a leg (from `a` to `b`, the photon's path so far `key`) into slot `n` of a photon's slots
// from `slot`; the next free slot.
fn put_leg(slot: u32, n: u32, a: vec2<f32>, b: vec2<f32>, s0: f32, s1: f32, haze: f32, light: u32, power: vec3<f32>, key: u32) -> u32 {
    if n >= BENT_LEGS || distance(a, b) < 1e-4 || all(power <= vec3<f32>(0.0)) {
        return n;
    }
    legs_out[slot + n] = Leg(
        vec4<f32>(a, b),
        vec4<f32>(s0, s1, haze, f32(light + 1u)),
        vec4<f32>(power, f32(key)),
        vec4<f32>(leg_blur(s0), leg_blur(s1), 0.0, 0.0),
    );
    return n + 1u;
}

// Part 4a: how much rough mirrors have blurred a photon's light by path length `s` (world, added to
// its line's half width): `BLUR.x` at path length `BLUR.y`, growing by `BLUR.z` a unit after.
var<private> BLUR: vec3<f32>;

fn leg_blur(s: f32) -> f32 {
    return BLUR.x + max(s - BLUR.y, 0.0) * BLUR.z;
}

// The share of the light that passes a glass edge from index `n1` into `n2` (Fresnel's, for
// unpolarised light), going `dir` against the edge's normal `facing` and leaving `bent`: grazing
// light mostly reflects, and none does where the index doesn't change.
fn passes_edge(dir: vec2<f32>, bent: vec2<f32>, facing: vec2<f32>, n1: f32, n2: f32) -> f32 {
    let ci = clamp(abs(dot(dir, facing)), 0.0, 1.0);
    let ct = clamp(abs(dot(bent, facing)), 0.0, 1.0);
    let rs = (n1 * ci - n2 * ct) / max(n1 * ci + n2 * ct, 1e-6);
    let rp = (n1 * ct - n2 * ci) / max(n1 * ct + n2 * ci, 1e-6);
    return clamp(1.0 - 0.5 * (rs * rs + rp * rp), 0.0, 1.0);
}

// A leg's path key: the photon's, marked once its light is split into colours (its neighbours of
// its own colour are then BENT_COLOURS apart).
fn leg_key(key: u32, rainbow: bool) -> u32 {
    return key + select(0u, RAINBOW_KEY, rainbow);
}

// How far past where it last left glass (at path length `s`) a photon of light `l` carries the
// light: as far as the direct light counts the glass as solid. In the flat that is Shadow length;
// on a Stage the glass shadows the wall only as far as its slab reaches (its depth over the lamp's
// rise above the wall).
fn carried_past(l: LightGpu, sun: bool, s: f32) -> f32 {
    if u.trace.y < 0.5 {
        return max(u.shadows.z, 0.0);
    }
    let depth = max(st.room.z + st.room.y, 0.0);
    if sun {
        let el = clamp(l.p0.z, -1.0, 1.0);
        if el > 1e-4 {
            return depth * sqrt(max(1.0 - el * el, 0.0)) / el;
        }
        return 1e30;
    }
    let rise = l.p0.z + st.room.y - depth;
    if rise > 1e-3 {
        return s * depth / rise;
    }
    return 1e30;
}

// `dir` turned a little at random for frosted glass (at most half `frost` radians either way).
fn frosted(dir: vec2<f32>, frost: f32, k: u32, i: u32) -> vec2<f32> {
    if frost <= 0.0 {
        return dir;
    }
    let r = f32(pcg(k * 9781u + i * 6271u + 17u) >> 8u) / 16777216.0;
    return rotate_by(dir, (r - 0.5) * frost);
}

@compute @workgroup_size(64)
fn cs_bent(@builtin(global_invocation_id) gid: vec3<u32>) {
    let per = max(u32(bu.counts.x), 1u);
    let k = gid.x;
    if k >= per * u32(bu.counts.y) {
        return;
    }
    let slot = k * BENT_LEGS;
    for (var i = 0u; i < BENT_LEGS; i++) {
        legs_out[slot + i] = Leg(vec4<f32>(0.0), vec4<f32>(0.0), vec4<f32>(0.0), vec4<f32>(0.0));
    }
    BLUR = vec3<f32>(0.0);
    let pair = bent_pairs[k / per];
    let j = k % per;
    let li = u32(pair.b.y);
    let own = i32(pair.b.x);
    // Part 4a: a thrower's pair: its photons are drawn from where they first meet its mirror.
    let throw_pair = pair.b.z > 0.5;
    let l = lights[li];
    let kind = l.p0.w;
    let sun = kind > 1.5 && kind < 2.5;
    let lo = pair.a.xy;
    let hi = pair.a.zw;
    let mid = 0.5 * (lo + hi);
    let px = max(bu.more.x, 1e-3);
    let texel = max(u.field.z, 1e-3);
    // Which fan (a point across the lamp) and where in it (the middle of its stratum live; a baked
    // sample turns them all).
    let copies = lamp_copies(l, per);
    let fan = per / copies;
    if j >= fan * copies {
        return;
    }
    let f = (f32(j % fan) + fract(0.5 + bu.more.y)) / f32(fan);
    let across = (f32(j / fan) + fract(0.5 + bu.more.y * 0.381966)) / f32(copies) * 2.0 - 1.0;
    let photons = f32(fan * copies);
    let corners = array<vec2<f32>, 4>(lo, vec2<f32>(hi.x, lo.y), hi, vec2<f32>(lo.x, hi.y));
    var q = vec2<f32>(0.0);
    var dir = vec2<f32>(1.0, 0.0);
    var weight = 0.0;
    var s = 0.0;
    var amount = 1.0;
    if sun {
        // Parallel, from a line across the box well before it (so what shades the glass is met).
        var way = l.p3.xy;
        if dot(way, way) < 1e-8 {
            way = vec2<f32>(0.0, 1.0);
        }
        way = normalize(way);
        let side = vec2<f32>(-way.y, way.x);
        var a0 = 1e30;
        var a1 = -1e30;
        var b0 = 1e30;
        var b1 = -1e30;
        for (var c = 0; c < 4; c++) {
            a0 = min(a0, dot(corners[c], way));
            a1 = max(a1, dot(corners[c], way));
            b0 = min(b0, dot(corners[c], side));
            b1 = max(b1, dot(corners[c], side));
        }
        let soft = lamp_soft(l);
        let back = min(max(u.shadows.z, 0.0), 4096.0) + 4.0 * px;
        let grow = soft * (a1 - a0 + back) + 2.0 * px;
        b0 -= grow;
        b1 += grow;
        dir = normalize(way + side * (across * soft));
        q = way * (a0 - back) + side * mix(b0, b1, f);
        weight = (b1 - b0) / photons;
    } else {
        var at = l.p0.xy;
        if kind > 2.5 {
            at = source_point(l, mid);
        }
        let to_mid = mid - at;
        let dm = length(to_mid);
        let base = select(vec2<f32>(1.0, 0.0), to_mid / max(dm, 1e-6), dm > 1e-4);
        let r = lamp_soft(l);
        var a0 = -3.14159265;
        var a1 = 3.14159265;
        if any(at < lo - vec2<f32>(r)) || any(at > hi + vec2<f32>(r)) {
            a0 = 1e9;
            a1 = -1e9;
            for (var c = 0; c < 4; c++) {
                let d = corners[c] - at;
                let ang = atan2(base.x * d.y - base.y * d.x, dot(base, d));
                a0 = min(a0, ang);
                a1 = max(a1, ang);
            }
            let grow = (r + 2.0 * px) / max(dm, 1e-3);
            a0 = max(a0 - grow, -3.14159265);
            a1 = min(a1 + grow, 3.14159265);
        }
        let ang = mix(a0, a1, f);
        dir = rotate_by(base, ang);
        q = at + vec2<f32>(-dir.y, dir.x) * (across * r);
        weight = (a1 - a0) / photons;
        if kind > 0.5 && kind < 1.5 {
            // A spot's cone, as the direct light's.
            let e = clamp((dot(dir, l.p3.xy) - l.p3.w) / max(l.p3.z - l.p3.w, 1e-6), 0.0, 1.0);
            amount = e * e * (3.0 - 2.0 * e);
        }
        // Start short of the lamp by its size, a ridden ball's radius and however deep in a
        // blocker it sits, as the direct light's march stops.
        s = max(l.p6.x + max(l.p2.z, 0.0), -field_at(at));
        q += dir * s;
    }
    // Its light: what the direct light dropped (the shadow's strength, less what the shadow's
    // colour already fills in), an ultraviolet light's visible share, Glass caustics.
    var power = l.p1.rgb * weight * amount * clamp(l.p2.w, 0.0, 1.0)
        * max(vec3<f32>(1.0) - l.p5.rgb, vec3<f32>(0.0)) * select(bu.more.w, 1.0, throw_pair);
    if l.p7.y > 0.5 {
        power *= l.p7.z;
    }
    if all(power <= vec3<f32>(0.0)) {
        return;
    }
    let haze = l.p7.x;
    let reach = select(max(l.p2.x, 0.0), 1e30, sun);
    // How far past its last glass it carries the light (worked out when it leaves the glass).
    var carried = 0.0;
    // Its wavelength, used once it meets glass that splits light (red -1 to violet 1): the fan's
    // photons take BENT_COLOURS bands in turn, each a little further along its band than the last
    // of its colour.
    let m = j % fan;
    let w = (f32(m % BENT_COLOURS) + fract(f32(m / BENT_COLOURS) * 0.618034 + bu.more.y)) / f32(BENT_COLOURS) * 2.0
        - 1.0;
    var rainbow = false;
    var medium = BENT_OPEN;
    var eta = 1.0;
    var frost = 0.0;
    var met = false;
    var drawn = false;
    var in_solid = false;
    // A see-through stretch: the most it blocks (-1 outside one) and its colour; the same for the
    // glass the photon is in.
    var see = -1.0;
    var see_col = vec3<f32>(1.0);
    var glass_block = 0.0;
    var glass_col = vec3<f32>(1.0);
    var leg_from = q;
    var leg_s = s;
    var legs = 0u;
    var key = 1u;
    var last_exit = 0.0;
    var prev = q;
    // The mirror it was last thrown off and where (so it isn't thrown off it again as it leaves).
    var last_tid = -1;
    var last_throw = -1e30;
    let most = u32(bu.counts.z);
    for (var i = 0u; i < most; i++) {
        if s >= reach || legs >= BENT_LEGS {
            break;
        }
        if drawn && medium < 0 && !in_solid && s - last_exit > carried {
            break;
        }
        if !met && medium < 0 {
            // Outside its bender's box and going away from it: it never meets it.
            let m = 2.0 * px;
            if (q.x < lo.x - m && dir.x <= 0.0) || (q.x > hi.x + m && dir.x >= 0.0)
                || (q.y < lo.y - m && dir.y <= 0.0) || (q.y > hi.y + m && dir.y >= 0.0) {
                break;
            }
        }
        let fd = field_all(q);
        let here = bent_at(q, fd);
        let ip = pixel_of(q);
        var c = here.kind;
        if medium < 0 && c >= 0 && drawn && legs + 1u >= BENT_LEGS {
            // One leg left: other glass is open (its light still straight on).
            c = BENT_OPEN;
        }
        if medium >= 0 {
            if c == medium {
                glass_block = max(glass_block, here.block);
                glass_col = min(glass_col, colour_of(fd.yzw));
            } else {
                // Leaving its glass: bent out (or back in, wholly reflected).
                let n_out = glass_normal(prev, medium);
                var next = vec3<f32>(1.0, 0.0, 0.0);
                if c >= 0 {
                    next = glass_of(c, ip);
                }
                var next_eta = next.x;
                if rainbow {
                    next_eta *= 1.0 + next.y * 0.04 * w;
                }
                let facing = select(n_out, -n_out, dot(dir, n_out) > 0.0);
                let bent = refract(vec3<f32>(dir, 0.0), vec3<f32>(facing, 0.0), eta / next_eta).xy;
                if dot(bent, bent) < 1e-8 {
                    if drawn {
                        legs = put_leg(slot, legs, leg_from, prev, leg_s, s, haze, li, power, leg_key(key, rainbow));
                        leg_from = prev;
                        leg_s = s;
                    }
                    key = path_key(key, 100);
                    dir = normalize(reflect(dir, facing));
                    q = prev + dir * (0.25 * px);
                    s += 0.25 * px;
                    continue;
                }
                if drawn {
                    legs = put_leg(slot, legs, leg_from, q, leg_s, s, haze, li, power, leg_key(key, rainbow));
                }
                key = path_key(key, 50 + c);
                // What the glass lets through, in its colour (part 1), less what its edge reflects.
                power *= (1.0 - clamp(glass_block, 0.0, 1.0)) * glass_col
                    * passes_edge(dir, normalize(bent), facing, eta, next_eta);
                if c >= 0 {
                    // Straight into other glass: a dichroic one colours it on the way in.
                    power *= dichroic_into(c, dir, facing);
                }
                dir = frosted(normalize(bent), frost, k, i);
                if !drawn && medium == own {
                    drawn = true;
                }
                leg_from = q;
                leg_s = s;
                last_exit = s;
                carried = carried_past(l, sun, s);
                medium = BENT_OPEN;
                eta = 1.0;
                // With one leg left it bends no more: the rest of its way is that leg.
                if c >= 0 && drawn && legs + 1u >= BENT_LEGS {
                    c = BENT_OPEN;
                }
                if c >= 0 {
                    // Straight into other glass.
                    if !rainbow && next.y > 0.0 {
                        rainbow = true;
                        power *= spectral(w);
                    }
                    medium = c;
                    eta = next_eta;
                    frost = next.z;
                    glass_block = here.block;
                    glass_col = colour_of(fd.yzw);
                    c = medium;
                }
            }
        }
        if in_solid {
            if c != BENT_SOLID {
                // Out of the solid thing it lit: it ends.
                break;
            }
        } else if medium < 0 {
            if see >= 0.0 && c != BENT_SEE {
                // Out of a see-through stretch (which doesn't shade itself): what it lets through,
                // in its colour.
                if drawn && legs + 1u < BENT_LEGS {
                    legs = put_leg(slot, legs, leg_from, q, leg_s, s, haze, li, power, leg_key(key, rainbow));
                    leg_from = q;
                    leg_s = s;
                    key = path_key(key, 40);
                }
                power *= (1.0 - clamp(see, 0.0, 1.0)) * see_col;
                see = -1.0;
                see_col = vec3<f32>(1.0);
            }
            if c == BENT_SEE {
                see = max(see, here.block);
                see_col = min(see_col, colour_of(fd.yzw));
            } else if c == BENT_SOLID {
                if !drawn {
                    break;
                }
                in_solid = true;
            } else if c == BENT_OFF {
                if drawn {
                    break;
                }
            } else if c <= BENT_THROW {
                // Part 4a: a mirror throws it back.
                let tid = BENT_THROW - c;
                // The mirror's normal, outward: a ball's from its sphere, else down the slope of
                // its cover.
                let e = thrower_entry(tid);
                var n = vec2<f32>(0.0, -1.0);
                var turn = 0.0;
                var found = false;
                if e.a.y > 0.5 {
                    let b = ball_at(q);
                    if b >= 0 {
                        let sp = spheres[u32(b)];
                        let d = q - sp.a.xy;
                        if dot(d, d) > 1e-8 {
                            n = normalize(d);
                            turn = sp.c.z;
                            found = true;
                        }
                    }
                }
                if !found {
                    n = glass_normal(q, tid);
                }
                // Only light coming in is thrown (light just thrown, leaving through the mirror's
                // soft edge, goes on its way).
                if dot(dir, n) < 0.0 && (tid != last_tid || s - last_throw > 2.0 * px) {
                    if !met {
                        if !throw_pair || tid != own {
                            // Its first glass or mirror is another pair's: that pair carries it.
                            break;
                        }
                        met = true;
                        power *= gobo(l, q);
                    }
                    var facing = n;
                    var tile = 0;
                    var ends = false;
                    if e.a.z >= 3.0 {
                        // A tiled mirror: off the middle of its tile (into a gap: it ends).
                        let sector = 6.28318531 / e.a.z;
                        let a = atan2(facing.y, facing.x) - turn;
                        let k = floor(a / sector);
                        if e.a.w > 0.0 && abs(2.0 * fract(a / sector) - 1.0) > 1.0 - e.a.w {
                            ends = true;
                        }
                        let ca = (k + 0.5) * sector + turn;
                        facing = vec2<f32>(cos(ca), sin(ca));
                        tile = (i32(k) + 1024) % 256;
                        if dot(dir, facing) >= 0.0 {
                            ends = true;
                        }
                    }
                    if drawn {
                        legs = put_leg(slot, legs, leg_from, q, leg_s, s, haze, li, power, leg_key(key, rainbow));
                    }
                    if ends || legs >= BENT_LEGS {
                        drawn = false;
                        break;
                    }
                    key = path_key(path_key(key, 400 + tid), 300 + tile);
                    BLUR = vec3<f32>(leg_blur(s), s, BLUR.z + max(e.b.w, 0.0));
                    power *= dichroic_off(tid, dir, facing);
                    dir = normalize(reflect(dir, facing));
                    power *= max(e.b.rgb, vec3<f32>(0.0)) * max(bu.extra.x, 0.0);
                    drawn = true;
                    last_tid = tid;
                    last_throw = s;
                    leg_from = q;
                    leg_s = s;
                    last_exit = s;
                    // Thrown light is new light: carried as far as the light reaches.
                    carried = 1e30;
                    prev = q;
                    q += dir * (0.25 * px);
                    s += 0.25 * px;
                    continue;
                }
            } else if c >= 0 {
                // Into glass.
                if !met {
                    if c != own || throw_pair {
                        // Its first glass is another bender's: that pair carries it.
                        break;
                    }
                    met = true;
                    power *= gobo(l, q);
                }
                let g = glass_of(c, ip);
                if !rainbow && g.y > 0.0 {
                    rainbow = true;
                    power *= spectral(w);
                }
                var next_eta = g.x;
                if rainbow {
                    next_eta *= 1.0 + g.y * 0.04 * w;
                }
                let n_out = glass_normal(q, c);
                let facing = select(n_out, -n_out, dot(dir, n_out) > 0.0);
                let bent = refract(vec3<f32>(dir, 0.0), vec3<f32>(facing, 0.0), eta / next_eta).xy;
                if drawn {
                    legs = put_leg(slot, legs, leg_from, q, leg_s, s, haze, li, power, leg_key(key, rainbow));
                    leg_from = q;
                    leg_s = s;
                }
                key = path_key(key, c);
                if dot(bent, bent) > 1e-8 {
                    power *= passes_edge(dir, normalize(bent), facing, eta, next_eta)
                        * dichroic_into(c, dir, facing);
                    dir = frosted(normalize(bent), g.z, k, i);
                }
                medium = c;
                eta = next_eta;
                frost = g.z;
                glass_block = here.block;
                glass_col = colour_of(fd.yzw);
            }
        }
        // The next step: by the field in the open (and before the picture), by how deep it is
        // deep inside something solid, else half a pixel. In glass or a see-through stretch at
        // most a pixel: the field doesn't tell one blocker from another, so a deeper step could
        // pass something solid inside (a bubble over a wall).
        var step = 0.5 * px;
        let d = fd.x;
        if medium < 0 && !in_solid && see < 0.0 && d > 2.0 * texel {
            step = max(d - texel, 0.5 * px);
        } else if d < -2.0 * texel {
            step = -d - texel;
            if !in_solid {
                step = clamp(step, 0.5 * px, px);
            }
        }
        // Not past where its light stops being carried, nor past the light's reach.
        if drawn && medium < 0 && !in_solid {
            step = min(step, max(carried - (s - last_exit), 0.01 * px));
        }
        step = min(step, max(reach - s, 0.01 * px));
        prev = q;
        q += dir * step;
        s += step;
    }
    if drawn {
        legs = put_leg(slot, legs, leg_from, q, leg_s, s, haze, li, power, leg_key(key, rainbow));
    }
}

struct BentOut {
    @builtin(position) pos: vec4<f32>,
    // Where (world).
    @location(0) at: vec2<f32>,
    // The leg: from xy, its way (unit) xy.
    @location(1) @interpolate(flat) leg: vec4<f32>,
    // Its length, the path length from the light at its start and end, its least soft half width.
    @location(2) @interpolate(flat) lengths: vec4<f32>,
    // Its soft half width at its start and end (signed: where it changes sign the photon crossed
    // its neighbours' way, a focus), the light's index, its share in the haze.
    @location(3) @interpolate(flat) widths: vec4<f32>,
    // Its light.
    @location(4) @interpolate(flat) power: vec3<f32>,
};

// How far leg `mine`'s ends are from the line of the same leg of a neighbouring photon `other`,
// signed (the side its normal points), and 1 in z when there is such a leg (0 when there is none,
// or the neighbour's way went otherwise).
fn spread_from(mine: Leg, other: Leg) -> vec3<f32> {
    if other.b.w != mine.b.w || other.c.w != mine.c.w {
        return vec3<f32>(0.0);
    }
    let a = other.a.xy;
    let d = other.a.zw - a;
    let len = length(d);
    if len < 1e-6 {
        return vec3<f32>(0.0);
    }
    let n = vec2<f32>(-d.y, d.x) / len;
    return vec3<f32>(dot(mine.a.xy - a, n), dot(mine.a.zw - a, n), 1.0);
}

// Leg `k` as a quad along it, its soft width (and a pixel) either side. Its width is how far apart
// its neighbouring photons in the same fan (of its own colour, once split) are along it (1.5
// times, so the lines blend evenly), at least the photons' own softness: where light spreads,
// each line spreads with it, and where the photons cross (a focus) it narrows to its softness.
@vertex
fn vs_bent(@builtin(vertex_index) vi: u32, @builtin(instance_index) k: u32) -> BentOut {
    var out: BentOut;
    out.pos = vec4<f32>(4.0, 4.0, 0.5, 1.0);
    out.at = vec2<f32>(0.0);
    out.leg = vec4<f32>(0.0);
    out.lengths = vec4<f32>(0.0);
    out.widths = vec4<f32>(0.0);
    out.power = vec3<f32>(0.0);
    let leg = legs_in[k];
    if leg.b.w < 0.5 {
        return out;
    }
    let a = leg.a.xy;
    let b = leg.a.zw;
    let len = distance(a, b);
    if len < 1e-6 {
        return out;
    }
    let along = (b - a) / len;
    let side = vec2<f32>(-along.y, along.x);
    let least = max(bu.more.z, 1e-3);
    // Its neighbours in its fan: the same leg of the photons either side.
    let per = max(u32(bu.counts.x), 1u);
    let photon = k / BENT_LEGS;
    let j = photon % per;
    let fan = max(per / lamp_copies(lights[u32(leg.b.w) - 1u], per), 1u);
    let stride = select(1u, BENT_COLOURS, leg.c.w >= f32(RAINBOW_KEY));
    // The one before it is on one side, the one after on the other: their distances count
    // against each other.
    var sum = vec2<f32>(0.0);
    var n = 0.0;
    if j % fan >= stride {
        let e = spread_from(leg, legs_in[k - stride * BENT_LEGS]);
        sum += e.xy;
        n += e.z;
    }
    if j % fan + stride < fan {
        let e = spread_from(leg, legs_in[k + stride * BENT_LEGS]);
        sum -= e.xy;
        n += e.z;
    }
    var h = vec2<f32>(least);
    if n > 0.0 {
        let e = 1.5 * sum / n;
        h = sign(e) * min(abs(e), vec2<f32>(MOST_SPREAD * least));
    }
    // Part 4a: wider where a rough mirror has blurred it (its kernel stays the same light across).
    // A leg no mirror blurred keeps part 3's widths exactly.
    if any(leg.d.xy > vec2<f32>(0.0)) {
        let mag = max(abs(h), vec2<f32>(least)) + max(leg.d.xy, vec2<f32>(0.0));
        h = select(mag, -mag, h < vec2<f32>(0.0));
    }
    let x = f32(vi & 1u);
    let y = f32((vi >> 1u) & 1u) * 2.0 - 1.0;
    let pad = bu.more.x;
    let reach = max(abs(h), vec2<f32>(least));
    let at = mix(a, b, x) + side * (y * (mix(reach.x, reach.y, x) + pad));
    let uv = world_to_uv(at);
    out.pos = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.5, 1.0);
    out.at = at;
    out.leg = vec4<f32>(a, along);
    out.lengths = vec4<f32>(len, leg.b.x, leg.b.y, least);
    out.widths = vec4<f32>(h, leg.b.w - 1.0, leg.b.z);
    out.power = leg.c.rgb;
    return out;
}

// How many times the photons' own softness a line may spread to.
const MOST_SPREAD: f32 = 48.0;

// A photon line's light here: the soft kernel across it (whose integral across is 1), times a
// lamp's falloff over the 1 / s its fan spreads by.
@fragment
fn fs_bent(in: BentOut) -> Two {
    let d = in.at - in.leg.xy;
    let t = clamp(dot(d, in.leg.zw) / max(in.lengths.x, 1e-6), 0.0, 1.0);
    let h = max(abs(mix(in.widths.x, in.widths.y, t)), in.lengths.w);
    let y = dot(d, vec2<f32>(-in.leg.w, in.leg.z)) / h;
    if abs(y) >= 1.0 {
        discard;
    }
    let l = lights[u32(in.widths.z + 0.5)];
    let s = mix(in.lengths.y, in.lengths.z, t);
    var f = 1.0;
    if !(l.p0.w > 1.5 && l.p0.w < 2.5) {
        f = falloff(s, l.p2.x, l.p2.y) * s;
    }
    let kernel = (1.0 - y * y) * (1.0 - y * y) * 15.0 / (16.0 * h);
    let c = in.power * (kernel * f);
    return Two(vec4<f32>(c, 0.0), vec4<f32>(c * in.widths.w, 0.0));
}

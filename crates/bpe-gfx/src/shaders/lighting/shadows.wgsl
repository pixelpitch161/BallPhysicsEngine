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

// The widest a light covers for its shadows, seen from a point: the tangent of a half angle of 1.3.
const MOST_SEEN: f32 = 3.6;

// How an Area or Strip light looks from a point, for its shadows: the way to the middle of what it
// covers (`way`), how far that middle is (`dist`), its half width seen from there as an angle
// (`ang`, the tangent of the half angle; 0 when the point is on the light, which casts no shadow
// there) and how far short of that middle the march ends (`stop`: at the light's edge, the light's
// own Size not counted, or however deep in a blocker it sits).
struct Extended {
    way: vec2<f32>,
    dist: f32,
    ang: f32,
    stop: f32,
};

// The light's light comes from its nearest point (`source_point`), but its shadows come from all
// of it: a wide panel's shadow narrows to a point under a ball smaller than it, and its soft edge
// widens with distance.
fn extended_seen(l: LightGpu, p: vec2<f32>) -> Extended {
    var out = Extended(vec2<f32>(1.0, 0.0), 0.0, 0.0, 0.0);
    let px = max(u.view.z, 1e-3);
    if l.p0.w < 3.5 {
        // Area: aimed at its middle; its half width seen across that way, a rectangle's corners
        // or a disc's (an ellipse's) edge, over the distance; the march ends at its edge.
        let d = l.p0.xy - p;
        let len = length(d);
        if len < 1e-3 || distance(source_point(l, p), p) < 0.5 * px {
            return out;
        }
        let half = 0.5 * vec2<f32>(max(l.p4.x, 0.0), max(l.p4.y, 0.0));
        let dir = l.p3.xy;
        let side = vec2<f32>(-dir.y, dir.x);
        let way = d / len;
        let across = vec2<f32>(-way.y, way.x);
        let w = vec2<f32>(abs(dot(across, dir)), abs(dot(across, side))) * half;
        var seen = w.x + w.y;
        // How far from its middle its edge is, going back along `way`.
        let back = vec2<f32>(abs(dot(way, dir)), abs(dot(way, side)));
        var edge = min(select(1e9, half.x / back.x, back.x > 1e-5), select(1e9, half.y / back.y, back.y > 1e-5));
        if l.p6.z > 0.5 {
            seen = length(w);
            edge = 1.0 / max(length(back / max(half, vec2<f32>(1e-3))), 1e-6);
        }
        edge = min(edge, len);
        out.way = way;
        out.dist = len;
        out.ang = min(seen / len, MOST_SEEN);
        out.stop = max(l.p6.x + edge + 0.5 * u.view.z, -field_at(l.p0.xy - way * edge));
        return out;
    }
    // Strip: the angles of its segments' ends round the way to its nearest point; aimed at the
    // strip's point nearest the middle of those (a strip round the point is capped at MOST_SEEN).
    let c0 = source_point(l, p);
    let r0 = distance(c0, p);
    if r0 < max(0.5 * l.p4.x, 0.5 * px) {
        return out;
    }
    let w0 = (c0 - p) / r0;
    let n0 = vec2<f32>(-w0.y, w0.x);
    let first = u32(l.p4.z);
    let n = u32(l.p4.w);
    var lo = 0.0;
    var hi = 0.0;
    for (var k = 0u; k < n; k++) {
        let sg = segments[first + k];
        for (var e = 0; e < 2; e++) {
            let q = select(sg.zw, sg.xy, e == 0) - p;
            let a = atan2(dot(q, n0), dot(q, w0));
            lo = min(lo, a);
            hi = max(hi, a);
        }
    }
    let mid = 0.5 * (lo + hi);
    let half = min(0.5 * (hi - lo), 1.3);
    let aim = w0 * cos(mid) + n0 * sin(mid);
    let c = source_point(l, p + aim * r0);
    let far = max(distance(c, p), 1e-3);
    out.way = (c - p) / far;
    out.dist = far;
    // (End on, a straight strip is a point: the lamp's Size then makes its soft edge.)
    out.ang = clamp(tan(half), 1e-4, MOST_SEEN);
    out.stop = max(l.p6.x + 0.5 * max(l.p4.x, 0.0) + 0.5 * u.view.z, -field_at(c));
    return out;
}

// How far short of an Area or Strip light's point a ray across it ends: the light's own place
// (a ridden ball's radius, a strip's tube), half a pixel, or however deep in a blocker it sits.
fn extended_stop(l: LightGpu, c: vec2<f32>) -> f32 {
    let tube = select(0.0, 0.5 * max(l.p4.x, 0.0), l.p0.w > 3.5);
    return max(l.p6.x + tube + 0.5 * u.view.z, -field_at(c));
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
    // The share of the light the see-through blockers it crossed let through, per channel (glass
    // and light part 1: coloured ones tint it; 0 once it crossed a solid one, 1 when it crossed
    // nothing).
    lit: vec3<f32>,
    // The soft edge of the blockers it passed near, each darkening it only by how much that
    // blocker blocks of each channel: the least `1 - o (1 - smoothstep(v))` over its samples
    // (exactly smoothstep(v) when every blocker is solid).
    e: vec3<f32>,
};

// How many rays across the lamp, from one edge to the other, the light is worked out from when
// the middle one crosses a blocker (odd: the middle one is the ray already marched).
const LAMP_RAYS: i32 = 5;
// As many for an Area or Strip light, whose soft edges are wide enough to show the steps between
// fewer: the most rays across a lamp.
const MOST_LAMP_RAYS: i32 = 9;

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
        if any(at != own) && pixel_meets(at) {
            return vec2<f32>(1.0, t);
        }
        if t > t_in && field_at(q) >= 0.0 {
            break;
        }
        t += 0.5 * px;
    }
    return vec2<f32>(0.0, max(t, t_in));
}

// Glass and light part 4b: what a dichroic see-through blocker the ray going `toward` enters at
// `q` lets through, per channel (its hue at the angle it meets it, the field's slope its normal);
// 1 where the surface there isn't dichroic or no surface is.
fn dichroic_entering(q: vec2<f32>, toward: vec2<f32>) -> vec3<f32> {
    let n_list = i32(u.bent.y);
    if n_list <= 0 {
        return vec3<f32>(1.0);
    }
    let ip = pixel_of(q);
    if ip.x < 0 {
        return vec3<f32>(1.0);
    }
    let code = surface_code(textureLoad(surface, ip, 0));
    if code < 0.5 || code >= 2047.5 {
        return vec3<f32>(1.0);
    }
    var g = vec4<f32>(0.0);
    for (var i = 0; i < min(n_list, 16); i++) {
        if abs(u.dichroic[i].x - code) < 0.5 {
            g = vec4<f32>(u.dichroic[i].yzw, 0.0);
            break;
        }
    }
    if g.x <= 0.0 {
        return vec3<f32>(1.0);
    }
    let e = max(u.field.z, u.view.z);
    let slope = vec2<f32>(
        field_at(q + vec2<f32>(e, 0.0)) - field_at(q - vec2<f32>(e, 0.0)),
        field_at(q + vec2<f32>(0.0, e)) - field_at(q - vec2<f32>(0.0, e)),
    );
    let cos_t = select(1.0, abs(dot(normalize(slope), toward)), length(slope) > 1e-6);
    return dichroic_through(g, cos_t);
}

// `visibility` for one ray. `across`: one of the rays across the lamp, which stops as soon as it
// crosses a solid blocker (only whether it did, how clear it stayed and what the see-through ones
// let through count). A see-through blocker (its pixels blocking less than SOLID) is walked
// through, keeping the most any pixel on the way blocks, and lets the rest of the light on.
//
// `wide`: the ray ends on an Area or Strip light. Once a sample is clearer of every blocker than
// the rest of the way is long, nothing can block the rest, and the blockers near it are beside
// or behind the light (a wall round a wide panel), not in front of it: it stops there, or the
// soft edge would darken everything near such a wall in contours.
fn march(p: vec2<f32>, toward: vec2<f32>, dist: f32, angular: f32, reach: f32, stop: f32, across: bool, wide: bool) -> Ray {
    let steps = i32(u.more.z);
    let texel = max(u.field.z, 1e-3);
    let px = max(u.view.z, 1e-3);
    // A Stage's blockers all block whole (its surface picture is cover-weighted).
    let stage = u.trace.y > 0.5;
    var t = 0.0;
    let ip = pixel_of(p);
    var leaves = false;
    if ip.x >= 0 {
        // On a blocker that covers more than half the pixel (in Flat R is the blocker's opacity
        // and A its cover times that).
        let s0 = textureLoad(surface, ip, 0);
        let b0 = min(s0.r, 1.0);
        leaves = select(b0 >= BLOCK_LEAST && cover_block(s0) > 0.5 * b0, s0.r > 0.5, stage);
        // On clear coloured glass: it doesn't colour its own light either.
        if !leaves && u.trace.w > 0.5 {
            let f0 = field_all(p);
            leaves = f0.x < 0.0 && clear_tint(f0.yzw);
        }
        // On glass that bends (part 3): it lights itself as it is (the photons start where the
        // light leaves it).
        leaves = leaves || bends_at(ip);
    }
    if leaves {
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
    let fair = (end - t) / f32(max(steps, 1));
    let ang = max(angular, 1e-4);
    var v = 1.0;
    var e = vec3<f32>(1.0);
    var clear = 1e4;
    var crossed = false;
    var solid = false;
    var lit = vec3<f32>(1.0);
    // Inside a see-through blocker it crossed: the most it blocks so far (-1 outside one), and the
    // colour it tints the light (the deepest the field says on the way through).
    var glass = -1.0;
    var col = vec3<f32>(1.0);
    // What a dichroic one lets through (part 4b), found where the ray went in.
    var dich = vec3<f32>(1.0);
    // Just past a stretch only the field's coarseness blocked, how near the ray is to that
    // blocker means nothing (the field can't tell within a texel): it counts from a texel away.
    var beside = false;
    var open = t;
    var cross_at = 0.0;
    // Skipping what only the field's coarseness blocked doesn't use up the steps (a ray along a
    // thin line would run out of them and count the rest of its way as open), and neither do steps
    // that only creep past a blocker (closer than two texels: a ray grazing a ball or running along
    // a thin wall would stop there and count the rest of its way as open, a bright line along every
    // shadow edge that lies inside another shadow), within a cap.
    var used = 0;
    // The last sample counted (its distance, -1 when the one before was skipped, and where).
    var last = -1.0;
    var last_t = 0.0;
    // (At least 400 rounds: Draft's 16 steps ran out along a long thin wall a low sun skims.)
    for (var i = 0; i < max(12 * steps, 400); i++) {
        if t >= end || used >= steps {
            break;
        }
        let before = last;
        last = -1.0;
        let q = p + toward * t;
        let fd = field_all(q);
        let d = fd.x;
        if glass >= 0.0 {
            let qi = pixel_of(q);
            let b = pixel_block(qi);
            if b >= BLOCK_LEAST {
                glass = max(glass, b);
            }
            // Glass that bends inside the stretch (part 3): its light comes by photons instead.
            if bends_at(qi) {
                glass = 1.0;
            }
            col = min(col, colour_of(fd.yzw));
            if glass < SOLID {
                if d < 0.0 || b >= BLOCK_LEAST {
                    // Still in it: half a pixel at a time (so a thin solid line in it or along
                    // its edge is met), or, deep inside, on by how deep the field says it is.
                    t += select(0.5 * px, -d - texel, -d > 2.0 * texel);
                    continue;
                }
                // Out of it: it lets through what it doesn't block, in its colour.
                lit *= (1.0 - glass) * col * dich;
                glass = -1.0;
                col = vec3<f32>(1.0);
                dich = vec3<f32>(1.0);
                open = t;
                beside = true;
            } else {
                // Something solid inside it: the ray crossed a solid blocker here.
                glass = -1.0;
                solid = true;
                lit = vec3<f32>(0.0);
                if across {
                    return Ray(0.0, 0.0, true, cross_at, vec3<f32>(0.0), vec3<f32>(0.0));
                }
            }
        } else if d < 0.0 && !solid {
            let c = confirm(p, toward, open, t, end);
            if c.x < 0.5 && clear_tint(fd.yzw) {
                // Clear coloured glass, which the picture can't tell (it blocks next to nothing):
                // walked through like any see-through blocker, its colour taken from the field.
                if !crossed {
                    cross_at = t;
                }
                crossed = true;
                glass = 0.0;
                col = colour_of(fd.yzw);
                dich = dichroic_entering(q, toward);
                t += 0.5 * px;
                continue;
            }
            if c.x < 0.5 {
                // Only the field's coarseness: carry on from past it.
                t = max(c.y, t + 0.5 * texel);
                open = t;
                beside = true;
                continue;
            }
            if !crossed {
                cross_at = c.y;
            }
            crossed = true;
            let met = pixel_of(p + toward * c.y);
            let b = pixel_block(met);
            // Glass that bends lamp light (part 3) is solid here: the photon pass carries what it
            // lets through.
            if stage || b >= SOLID || bends_at(met) {
                solid = true;
                lit = vec3<f32>(0.0);
                if across {
                    return Ray(0.0, 0.0, true, cross_at, vec3<f32>(0.0), vec3<f32>(0.0));
                }
                t = c.y;
            } else {
                glass = b;
                col = colour_of(field_all(p + toward * c.y).yzw);
                dich = dichroic_entering(p + toward * c.y, toward);
                t = c.y + 0.5 * px;
                continue;
            }
        }
        if beside && d >= 0.0 && d < texel {
            t += 0.5 * texel;
            open = t;
            continue;
        }
        beside = false;
        if wide && d >= dist - stop - t {
            break;
        }
        // (Nor do steps shorter than an even share of the way: a ray down a lane between rows of
        // balls, a few texels from them all the way, would use up Draft's 16 steps halfway.)
        if abs(d) >= max(2.0 * texel, fair) {
            used += 1;
        }
        if d >= 0.0 {
            open = t;
        }
        // The soft edge where the ray came nearest a blocker between this sample and the last
        // (Aaltonen's estimate), not only at the samples, so where they fall doesn't show.
        let nearest = nearest_between(d, t, before, last_t);
        last = d;
        last_t = t;
        // How clear it stays: for a wide light from the nearest-between too (its soft edges are its
        // rays' clear stretches, streaky from the samples alone); for a lamp from the samples (the
        // nearest-between of a ray just past a thin wall's end is 0 there, a step in its soft edge).
        let near_clear = select(vec2<f32>(d, t), nearest, wide);
        clear = min(clear, max(near_clear.x, 0.0) / max(near_clear.y, texel));
        let vk = clamp(0.5 + 0.5 * nearest.x / (max(nearest.y, texel) * ang), 0.0, 1.0);
        v = min(v, vk);
        let o = select(clamp(fd.yzw, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(1.0), stage || opacity_of(fd.yzw) >= SOLID);
        e = min(e, select(vec3<f32>(1.0) - o * (1.0 - smoothed(vk)), vec3<f32>(smoothed(vk)), o >= vec3<f32>(1.0)));
        if v <= 0.0 {
            break;
        }
        // At most half a texel: a wall one texel thick always has a step inside it.
        t += max(abs(d), 0.5 * texel);
    }
    if glass >= 0.0 {
        // It ran out of steps inside one: what it blocks still counts.
        lit *= (1.0 - min(glass, 1.0)) * col * dich;
    }
    return Ray(v, clear, crossed, cross_at, lit, e);
}

// How near a blocker a ray came between its last counted sample (`d0` clear at `t0`; `d0` -1 when
// there is none) and this one (`d` clear at `t`), and where (x: the distance, y: how far along):
// the two samples' clear circles meet `y` back along the ray, at `sqrt(d² - y²)` from it
// (Aaltonen's improved soft shadows). Just this sample when the circles don't meet between them.
fn nearest_between(d: f32, t: f32, d0: f32, t0: f32) -> vec2<f32> {
    let gone = t - t0;
    if d > 0.0 && d0 > 0.0 && gone > 1e-4 {
        let y = (gone * gone + d * d - d0 * d0) / (2.0 * gone);
        if y > 0.0 && y < gone {
            return vec2<f32>(sqrt(max(d * d - y * y, 0.0)), max(t - y, 0.0));
        }
    }
    return vec2<f32>(d, t);
}

fn smoothed(v: f32) -> f32 {
    return v * v * (3.0 - 2.0 * v);
}

// The share of a lamp `ang` wide (from -ang to ang) that rays across it show together: the
// length of the union of their stretches `lo[k]` to `hi[k]` (empty where `hi` <= `lo`) over its width.
fn lamp_share(lo_in: array<f32, MOST_LAMP_RAYS>, hi_in: array<f32, MOST_LAMP_RAYS>, ang: f32) -> f32 {
    var lo = lo_in;
    var hi = hi_in;
    var shown = 0.0;
    var reached = -ang;
    for (var k = 0; k < MOST_LAMP_RAYS; k++) {
        // The stretch that starts first of those left (they are few: look through them all).
        var pick = -1;
        var least = 1e9;
        for (var j = 0; j < MOST_LAMP_RAYS; j++) {
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
//
// See-through blockers (Flat): what the rays that crossed something let through makes up the
// rest of the lamp, `1 - o (1 - shown)` with `o` the mean of what those rays' blockers block, so
// behind a half see-through wall half the light gets through, and its soft edge runs from there
// to all of it (grazing it, `e` is `1 - o/2` as `1 - o (1 - 1/2)` is just inside). Solid blockers
// only: `o` is 1, as before. Per channel (glass and light part 1): a coloured blocker lets through
// more of its own colour.
//
// `src`: an Area or Strip light's index (-1 for any other): its rays across the lamp each end on
// the light, at its point nearest where they would have ended (`extended_seen` aims the middle one).
fn visibility(p: vec2<f32>, toward: vec2<f32>, dist: f32, angular: f32, reach: f32, stop: f32, src: i32) -> vec3<f32> {
    let middle = march(p, toward, dist, angular, reach, stop, false, src >= 0);
    if !middle.crossed {
        return middle.e;
    }
    if all(middle.lit <= vec3<f32>(0.0)) && middle.v <= 0.0 {
        return vec3<f32>(0.0);
    }
    let ang = max(angular, 1e-4);
    // Where it crossed, the soft edge is narrower than half a pixel: what the blockers let
    // through, no more rays needed.
    if middle.at * ang < 0.5 * u.view.z {
        return middle.lit;
    }
    // Draft (16 steps) makes do with the lamp's two edges; a wide light takes the most.
    let rays = select(select(LAMP_RAYS, 3, u.more.z <= 16.0), MOST_LAMP_RAYS, src >= 0);
    let gap = 2.0 * ang / f32(rays - 1);
    let side = vec2<f32>(-toward.y, toward.x);
    // What each ray shows of the lamp: from its point less how clear it stays to its point plus
    // that, within the lamp (nothing when it crosses a blocker, as the middle one did).
    var lo: array<f32, MOST_LAMP_RAYS>;
    var hi: array<f32, MOST_LAMP_RAYS>;
    // How much the blockers block on the rays that crossed one (the middle one's first).
    var blocked = vec3<f32>(1.0) - middle.lit;
    var crossings = 1.0;
    for (var k = 0; k < MOST_LAMP_RAYS; k++) {
        let off = f32(k) * gap - ang;
        lo[k] = 0.0;
        hi[k] = 0.0;
        if k >= rays || k == rays / 2 {
            continue;
        }
        var way = normalize(toward + side * off);
        var far = dist;
        var short = stop;
        if src >= 0 {
            let l = lights[src];
            let c = source_point(l, p + toward * dist + side * (off * dist));
            far = max(distance(c, p), 1e-3);
            way = (c - p) / far;
            short = extended_stop(l, c);
        }
        let r = march(p, way, far, gap, reach, short, true, src >= 0);
        if r.crossed {
            blocked += vec3<f32>(1.0) - r.lit;
            crossings += 1.0;
        }
        let c = select(min(r.clear, 2.0 * ang), -1.0, r.crossed);
        lo[k] = max(off - c, -ang);
        hi[k] = min(off + c, ang);
    }
    let share = lamp_share(lo, hi, ang);
    let o = blocked / crossings;
    return select(vec3<f32>(1.0) - o * (1.0 - share), vec3<f32>(share), o >= vec3<f32>(1.0));
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


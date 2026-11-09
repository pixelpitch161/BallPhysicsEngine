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

// The signed distance to the nearest blocker at world point `p` (x) and how much that blocker
// blocks of each channel (yzw, from 0 for something wholly see-through to 1 for something solid;
// all three the same unless it is coloured, glass and light part 1); far away off the picture.
fn field_all(p: vec2<f32>) -> vec4<f32> {
    let uv = world_to_uv(p);
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        let inside = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
        let edge = textureSampleLevel(field, samp, inside, 0.0);
        let far = length(uv - inside) * 2.0 / max(u.view.x, 1e-6) / max(u.camera.z, 1e-6);
        return vec4<f32>(max(edge.x, 0.0) + far, edge.yzw);
    }
    return textureSampleLevel(field, samp, uv, 0.0);
}

// How much a blocker that blocks `b` of each channel (the field's yzw) blocks: its least, the share
// it lets through of its brightest channel (exactly how much it blocks when it isn't coloured).
fn opacity_of(b: vec3<f32>) -> f32 {
    return min(b.x, min(b.y, b.z));
}

// The colour a blocker that blocks `b` of each channel tints the light it lets through: what it
// lets through of each channel over what it lets through of its brightest (exactly 1 when it isn't
// coloured, and for a solid one).
fn colour_of(b: vec3<f32>) -> vec3<f32> {
    let o = opacity_of(b);
    if o >= 1.0 {
        return vec3<f32>(1.0);
    }
    return clamp((vec3<f32>(1.0) - b) / (1.0 - o), vec3<f32>(0.0), vec3<f32>(1.0));
}

// tint.wgsl's THICK (the draw passes' Deeper when thick, here for the spheres of a Stage) and the
// grey depth from which a blocker on a Stage counts as solid (e^-10 of the light gets through).
const THICK_DEPTH: f32 = 2.0;
const TINT_SOLID_DEPTH: f32 = 10.0;

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

// How much the picture pixel `ip` blocks light. In Flat that is how much its things block by how
// see-through they are, their antialiased edges taken whole (scene.wgsl's `hard_block`: a pixel
// half covered by a solid line reads 1, one under a half see-through wall 0.5); on a Stage, their
// cover times how much they block.
fn pixel_block(ip: vec2<i32>) -> f32 {
    if ip.x < 0 {
        return 0.0;
    }
    return min(textureLoad(surface, ip, 0).r, 1.0);
}

// How much a surface-picture texel `s` blocks weighted by how much of it its blockers cover: the
// surface's R as it was before see-through blockers (R is then blocks times opacity and A cover
// times opacity, so for one thing this is exactly blocks times cover times opacity; on a Stage R
// already is, and is never more than A).
fn cover_block(s: vec4<f32>) -> f32 {
    if u.trace.y > 0.5 {
        return s.r;
    }
    return min(s.r, s.a);
}

// How much a blocking pixel blocks for it to count as solid: less lets the rest of the light
// through (see-through blockers).
const SOLID: f32 = 0.98;

// Glass and light part 3: the bender id of free glass (bending particles and glass liquid, which
// free_glass.wgsl marks in the surface picture's B, see `free_glass_of`).
const FREE_BENDER: i32 = 4096;
// The least index of refraction that bends (frame.rs's BENDS_FROM).
const BENDS_FROM: f32 = 1.0001;

// Whether the light being marched now has its light through bending glass carried by photons
// (lasers' isn't: the glass is see-through for them, as before part 3).
var<private> bent_carried: bool = true;

// Free glass on a surface-picture texel: its index of refraction and cover (0 where there is
// none). free_glass.wgsl writes B = -(1 + 16 q_ior + q_cover), q_ior the index's 64ths above 1
// (1 to 127), q_cover its cover's 15ths (1 to 15); any other B is 0 or more.
fn free_glass_of(s: vec4<f32>) -> vec2<f32> {
    if s.b > -1.5 {
        return vec2<f32>(1.0, 0.0);
    }
    let v = round(-s.b) - 1.0;
    let qi = floor(v / 16.0);
    return vec2<f32>(1.0 + qi / 64.0, (v - 16.0 * qi) / 15.0);
}

// Which bender the picture pixel `ip` is glass of: its surface index (one whose bit is set in
// `u.bends`), FREE_BENDER for free glass, or -1 (nothing that bends, or no photon pass). A pixel is
// glass where it is mostly covered and lets some light through.
fn bender_at(ip: vec2<i32>) -> i32 {
    return bender_covering(ip, 0.5);
}

// `bender_at` for a pixel covered at least `least` (a glass's soft edge is still its glass).
fn bender_covering(ip: vec2<i32>, least: f32) -> i32 {
    if u.bent.x < 0.5 || ip.x < 0 || !bent_carried {
        return -1;
    }
    let size = vec2<i32>(textureDimensions(surface));
    if any(ip >= size) || ip.y < 0 {
        return -1;
    }
    let s = textureLoad(surface, ip, 0);
    if min(s.r, 1.0) >= SOLID {
        return -1;
    }
    let free = free_glass_of(s);
    if free.y > 0.0 {
        return select(-1, FREE_BENDER, free.y >= least && free.x > BENDS_FROM);
    }
    if s.a < least {
        return -1;
    }
    let f = s.b / s.a;
    let code = round(f);
    if abs(f - code) > 0.2 || code < 0.5 {
        return -1;
    }
    let i = u32(code) % 2048u;
    let word = i / 32u;
    if ((u.bends[word / 4u][word % 4u] >> (i % 32u)) & 1u) == 0u {
        return -1;
    }
    return i32(i);
}

// Whether the picture pixel `ip` is glass that bends lamp light (only while the photon pass runs).
fn bends_at(ip: vec2<i32>) -> bool {
    return bender_at(ip) >= 0;
}

// Whether a picture pixel blocks (or bends) enough for a ray to meet it there.
fn pixel_meets(ip: vec2<i32>) -> bool {
    return pixel_block(ip) >= BLOCK_LEAST || bends_at(ip);
}

// Whether the blocker nearest world point `p` (by the blockers' jump flood, bound as `seeds` after
// the field pass) is glass that bends lamp light: any picture pixel under its field texel bends.
fn nearest_bends(p: vec2<f32>) -> bool {
    if u.bent.x < 0.5 {
        return false;
    }
    let uv = world_to_uv(p);
    let fsize = vec2<i32>(u.field.xy);
    let fp = clamp(vec2<i32>(uv * u.field.xy), vec2<i32>(0), fsize - 1);
    let s = textureLoad(seeds, fp, 0);
    if s.x < 0.0 {
        return false;
    }
    let t = vec2<i32>(s.xy);
    let size = vec2<i32>(textureDimensions(surface));
    let scale = vec2<f32>(size) / vec2<f32>(fsize);
    let lo = vec2<i32>(floor(vec2<f32>(t) * scale));
    let hi = min(max(vec2<i32>(ceil(vec2<f32>(t + 1) * scale)), lo + 1), size);
    for (var y = lo.y; y < hi.y; y++) {
        for (var x = lo.x; x < hi.x; x++) {
            if bends_at(vec2<i32>(x, y)) {
                return true;
            }
        }
    }
    return false;
}

// Glass and light part 1: how much a pixel's colour must hold back of some channel (its colour depth
// in the tint picture, deepest channel less shallowest) to count as a blocker though it blocks less
// than BLOCK_LEAST: clear stained glass (See-through 1) still throws its colour.
const TINT_LEAST: f32 = 0.1;

// Whether picture pixel `ip` is coloured glass the field should hold (only in the seed and field
// passes, which have the tint picture bound in `picture`'s place, and only when it was drawn): it
// blocks some light (clear glass blocks 1/1024; a thing that doesn't block light none) and colours it.
fn pixel_tints(ip: vec2<i32>) -> bool {
    if u.trace.w < 0.5 || u.trace.y > 0.5 || textureLoad(surface, ip, 0).r <= 0.0 {
        return false;
    }
    let d = textureLoad(picture, ip, 0).rgb;
    return max(d.r, max(d.g, d.b)) - min(d.r, min(d.g, d.b)) >= TINT_LEAST;
}

// Whether the blocker the field says is at `b` (its yzw) is clear coloured glass: it blocks too little
// to be met in the picture (`confirm`), but colours the light.
fn clear_tint(b: vec3<f32>) -> bool {
    return u.trace.w > 0.5 && u.trace.y < 0.5 && opacity_of(b) < BLOCK_LEAST && any(colour_of(b) < vec3<f32>(1.0 - TINT_LEAST));
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
            let ip = vec2<i32>(x, y);
            if textureLoad(surface, ip, 0).r >= least || pixel_tints(ip) || bends_at(ip) {
                return true;
            }
        }
    }
    return false;
}

// How much of field texel `t` its blockers cover, 0 to 1: the mean of its picture pixels' smooth
// cover (`cover_block`, antialiased edges counted by how much they cover).
fn foot_cover(t: vec2<i32>) -> f32 {
    let fsize = vec2<i32>(u.field.xy);
    let size = vec2<i32>(textureDimensions(surface));
    let scale = vec2<f32>(size) / vec2<f32>(fsize);
    let lo = vec2<i32>(floor(vec2<f32>(t) * scale));
    let hi = min(max(vec2<i32>(ceil(vec2<f32>(t + 1) * scale)), lo + 1), size);
    var sum = 0.0;
    for (var y = lo.y; y < hi.y; y++) {
        for (var x = lo.x; x < hi.x; x++) {
            sum += clamp(cover_block(textureLoad(surface, vec2<i32>(x, y), 0)), 0.0, 1.0);
        }
    }
    let n = f32((hi.x - lo.x) * (hi.y - lo.y));
    return clamp(sum / max(n, 1.0), 0.0, 1.0);
}

// How much farther a blocker's edge lies than its seed texel's own edge, in texels, seen from the
// open texel at `here` (the antialiased distance transform's idea). A seed texel blocks whole
// wherever any of its pixels does, so a ball's edge in the field jumps a texel at a time as it
// moves (shadow edges that crawl, most on Draft); where the seed is the edge of something thicker
// than a texel (the texel behind it, away from `here`, is wholly covered), its edge is taken to cut the
// texel by how much of it the blocker covers, so it slides smoothly. A thin line (nothing behind
// it) keeps its whole texel, or rays would step past it. Not on Draft (its 16 steps): a texel of
// 4 × 4 video pixels is too coarse for one cut to say where a round edge is, and measured, it made
// Draft's edges wobble more (and the preview, whose field matches the video's, must agree). Only
// for solid blockers: see-through glass's cover is its opacity, not how much of the texel it fills.
// (The cut is per seed, so where two edge seeds' nearest regions meet the distance can step by up
// to a texel; looked for at full size, no creases seen.)
fn edge_inset(seed: vec2<i32>, here: vec2<f32>) -> f32 {
    if u.more.z <= 16.0 || foot_most(seed) < SOLID {
        return 0.0;
    }
    let g = here - vec2<f32>(seed);
    let away = select(vec2<i32>(0, i32(sign(g.y))), vec2<i32>(i32(sign(g.x)), 0), abs(g.x) >= abs(g.y));
    // (Behind it, wholly covered: a thin wall seen past its end has blocking texels behind its end
    // too, only a sliver of each covered, and cutting its end in stepped its soft shadow edge.)
    if !foot_blocks(seed - away) || foot_cover(seed - away) < 0.9 {
        return 0.0;
    }
    return 1.0 - foot_cover(seed);
}

// The most any blocking picture pixel under field texel `t` blocks (-1 when none does; clear
// coloured glass blocks next to nothing).
fn foot_most(t: vec2<i32>) -> f32 {
    let fsize = vec2<i32>(u.field.xy);
    if any(t < vec2<i32>(0)) || any(t >= fsize) {
        return -1.0;
    }
    let size = vec2<i32>(textureDimensions(surface));
    let scale = vec2<f32>(size) / vec2<f32>(fsize);
    let lo = vec2<i32>(floor(vec2<f32>(t) * scale));
    let least = select(BLOCK_LEAST, 0.3, u.trace.y > 0.5);
    let hi = min(max(vec2<i32>(ceil(vec2<f32>(t + 1) * scale)), lo + 1), size);
    var most = -1.0;
    for (var y = lo.y; y < hi.y; y++) {
        for (var x = lo.x; x < hi.x; x++) {
            let ip = vec2<i32>(x, y);
            let b = textureLoad(surface, ip, 0).r;
            if b >= least || pixel_tints(ip) || bends_at(ip) {
                most = max(most, clamp(b, 0.0, 1.0));
            }
        }
    }
    return most;
}

// How much the blocker at blocked field texel `t` blocks: its own pixels' most, or (a texel that
// only closes a diagonal gap has none) its neighbours'. Whole on a Stage, whose march takes every
// blocker as solid.
fn blocker_opacity(t: vec2<i32>) -> f32 {
    if u.trace.y > 0.5 {
        return 1.0;
    }
    var o = foot_most(t);
    if o >= 0.0 {
        return o;
    }
    o = max(max(foot_most(t + vec2<i32>(1, 0)), foot_most(t - vec2<i32>(1, 0))), max(foot_most(t + vec2<i32>(0, 1)), foot_most(t - vec2<i32>(0, 1))));
    return select(1.0, o, o >= 0.0);
}

// Glass and light part 1: what the tint picture (bound in `picture`'s place in the field pass)
// says of the blocking picture pixels under field texel `t`. `k` sums each pixel's colour depth
// (its depth less its least channel: the rest is the grey share it lets through, which the
// surface picture already tells), `grey` is the most grey depth any has (how much a Stage's
// see-through shape lets through), `n` how many pixels.
struct FootTint {
    k: vec3<f32>,
    grey: f32,
    n: f32,
};

fn foot_tint(t: vec2<i32>) -> FootTint {
    var out = FootTint(vec3<f32>(0.0), 0.0, 0.0);
    let fsize = vec2<i32>(u.field.xy);
    if any(t < vec2<i32>(0)) || any(t >= fsize) {
        return out;
    }
    let size = vec2<i32>(textureDimensions(surface));
    let scale = vec2<f32>(size) / vec2<f32>(fsize);
    let lo = vec2<i32>(floor(vec2<f32>(t) * scale));
    let least = select(BLOCK_LEAST, 0.3, u.trace.y > 0.5);
    let hi = min(max(vec2<i32>(ceil(vec2<f32>(t + 1) * scale)), lo + 1), size);
    for (var y = lo.y; y < hi.y; y++) {
        for (var x = lo.x; x < hi.x; x++) {
            let ip = vec2<i32>(x, y);
            if textureLoad(surface, ip, 0).r >= least || pixel_tints(ip) || bends_at(ip) {
                let d = max(textureLoad(picture, ip, 0).rgb, vec3<f32>(0.0));
                let grey = min(d.r, min(d.g, d.b));
                out.k += d - vec3<f32>(grey);
                out.grey = max(out.grey, grey);
                out.n += 1.0;
            }
        }
    }
    return out;
}

// How much the blocker at blocked field texel `t` blocks of each channel: `o` (its opacity, as
// `blocker_opacity` says) where it isn't coloured, and where it is, `1 - (1 - o) e^-k` with `k` the
// mean colour depth of its pixels (or, for a texel that only closes a diagonal gap, its
// neighbours'). On a Stage a see-through shape lets through what its greyest pixel does.
fn blocker_channels(t: vec2<i32>, o: f32) -> vec3<f32> {
    if u.trace.w < 0.5 {
        return vec3<f32>(o);
    }
    var f = foot_tint(t);
    if f.n <= 0.0 {
        for (var k = 0; k < 4; k++) {
            let step = select(vec2<i32>(0, 1 - 2 * (k & 1)), vec2<i32>(1 - 2 * (k & 1), 0), k < 2);
            let near = foot_tint(t + step);
            f = FootTint(f.k + near.k, max(f.grey, near.grey), f.n + near.n);
        }
    }
    if f.n <= 0.0 {
        return vec3<f32>(o);
    }
    var opacity = o;
    if u.trace.y > 0.5 && f.grey < TINT_SOLID_DEPTH {
        opacity = 1.0 - exp(-f.grey);
    }
    let k = f.k / f.n;
    return select(vec3<f32>(1.0) - (1.0 - opacity) * exp(-k), vec3<f32>(opacity), k <= vec3<f32>(0.0));
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
        return vec4<f32>(1e4, 1.0, 1.0, 1.0);
    }
    // GBA: how much the nearest blocker blocks of each channel (it is this texel's own inside one).
    let b = blocker_channels(vec2<i32>(s.xy), blocker_opacity(vec2<i32>(s.xy)));
    let blocked = s.x == here.x && s.y == here.y;
    if blocked {
        // Inside: how far to the nearest open texel, negative (none open: deep inside).
        var d = 1e4;
        if s.z >= 0.0 {
            d = distance(s.zw, here) - 0.5;
        }
        return vec4<f32>(-d * texel, b);
    }
    return vec4<f32>((distance(s.xy, here) - 0.5 + edge_inset(vec2<i32>(s.xy), here)) * texel, b);
}


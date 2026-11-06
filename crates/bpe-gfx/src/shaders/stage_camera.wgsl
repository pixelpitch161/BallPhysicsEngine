// ---------- Stage camera moves (lighting part 5; stage_camera_pass.rs, stage_camera.rs) ----------
//
// Pasted after lighting.wgsl, so its structs, `world_to_uv`, `field_at`, `floor_y` and the sphere
// grid are this pass's too. Group 0 is the light pass's (`picture` is the picture before
// lighting, `surface` the surface picture, `field` the blockers' field), group 2 the stage's
// (spheres and their grid); group 3 is this pass's own. A ray from the moved camera is coloured
// from the lit front picture where the rest camera saw what it hits (`cam_rest_uv`), so with no
// move the picture is the front picture exactly.

struct CamGpu {
    // Its place (world), and the picture plane's distance at rest.
    origin: vec4<f32>,
    // The way it looks (unit), and zoom × fill.
    forward: vec4<f32>,
    // Its right (unit), and how far from rest it is (0 to 1).
    right: vec4<f32>,
    // Its down (unit), and how many rays a pixel takes at edges (1 or 5).
    down: vec4<f32>,
    // Every ray's shift (world), and the shade of a shape's side.
    shift: vec4<f32>,
    // The rest camera's place; w spare.
    rest: vec4<f32>,
    // The picture in screen units, then in pixels.
    size: vec4<f32>,
};

@group(3) @binding(0) var<uniform> cam: CamGpu;
// Seed: the lit picture, the background alone. Pull: the finer level. Push: the level and the
// coarser filled one. View: the lit picture, the background alone, the filled wall, the lit colour.
@group(3) @binding(1) var cam_a: texture_2d<f32>;
@group(3) @binding(2) var cam_b: texture_2d<f32>;
@group(3) @binding(3) var cam_c: texture_2d<f32>;
@group(3) @binding(4) var cam_d: texture_2d<f32>;

// Under this share of things a front pixel counts as the bare wall or floor.
const CAM_BARE: f32 = 0.02;
// The most steps a ray marches through the shapes.
const CAM_SLAB_STEPS: i32 = 160;
// The most cells of the sphere grid a ray walks.
const CAM_MOST_CELLS: i32 = 1024;

fn cam_max3(v: vec3<f32>) -> f32 {
    return max(v.r, max(v.g, v.b));
}

// How much of the front picture at `uv` is things rather than the bare wall: the cover of things
// with a surface, or how far the picture before lighting differs from the background alone.
fn cam_things(uv: vec2<f32>) -> f32 {
    let unlit = textureSampleLevel(picture, samp, uv, 0.0).rgb;
    let back = textureSampleLevel(cam_b, samp, uv, 0.0).rgb;
    let differs = cam_max3(abs(unlit - back)) / max(cam_max3(max(abs(unlit), abs(back))), 1e-3);
    let cover = textureSampleLevel(surface, samp, uv, 0.0).a;
    return clamp(max(differs, cover), 0.0, 1.0);
}

@fragment
fn fs_cam_seed(in: FsIn) -> @location(0) vec4<f32> {
    let lit = textureSampleLevel(cam_a, samp, in.uv, 0.0).rgb;
    let bare = select(0.0, 1.0, cam_things(in.uv) < CAM_BARE);
    return vec4<f32>(lit * bare, bare);
}

// One level coarser: the average of the 2 x 2 texels under this one (premultiplied).
@fragment
fn fs_cam_pull(in: FsIn) -> @location(0) vec4<f32> {
    return textureSampleLevel(cam_a, samp, in.uv, 0.0);
}

// A level filled in: what it knows, and the coarser filled level where it doesn't.
@fragment
fn fs_cam_push(in: FsIn) -> @location(0) vec4<f32> {
    let fine = textureLoad(cam_a, vec2<i32>(in.pos.xy), 0);
    let coarse = textureSampleLevel(cam_c, samp, in.uv, 0.0);
    return fine + (1.0 - clamp(fine.a, 0.0, 1.0)) * coarse;
}

struct CamRay {
    o: vec3<f32>,
    d: vec3<f32>,
};

// The ray through screen point `s` (screen units, middle 0, y down).
fn cam_ray(s: vec2<f32>) -> CamRay {
    let k = max(cam.forward.w, 1e-6);
    let d = cam.forward.xyz * cam.origin.w + cam.right.xyz * (s.x / k) + cam.down.xyz * (s.y / k) + cam.shift.xyz;
    return CamRay(cam.origin.xyz, d);
}

// The front picture's uv where the rest camera saw world point `x`.
fn cam_rest_uv(x: vec3<f32>) -> vec2<f32> {
    let dist = cam.origin.w;
    let along = dist / max(dist - x.z, 1e-3);
    let c0 = cam.rest.xy;
    return world_to_uv(c0 + (x.xy - c0) * along);
}

// The picture-plane point the rest camera saw `x` at (world).
fn cam_rest_point(x: vec3<f32>) -> vec2<f32> {
    let dist = cam.origin.w;
    let along = dist / max(dist - x.z, 1e-3);
    return cam.rest.xy + (x.xy - cam.rest.xy) * along;
}

fn cam_lit(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(cam_a, samp, uv, 0.0).rgb;
}

// The bare wall or floor at `uv`, filled in under things.
fn cam_wall(uv: vec2<f32>) -> vec3<f32> {
    let w = textureSampleLevel(cam_c, samp, uv, 0.0);
    return select(vec3<f32>(0.0), w.rgb / max(w.a, 1e-6), w.a > 1e-6);
}

// What a ray hits first among the solid parts of the stage. `kind`: 1 a ball, 2 a shape's front,
// 3 a shape's side, 4 the wall, 5 the floor; `id` the ball's index.
struct CamHit {
    t: f32,
    kind: u32,
    id: u32,
    p: vec3<f32>,
};

fn cam_spheres(r: CamRay, best: CamHit) -> CamHit {
    var out = best;
    if st.more.w < 0.5 {
        return out;
    }
    let reach = st.grid.w;
    var s0 = 0.0;
    var s1 = out.t;
    if abs(r.d.z) > 1e-9 {
        let a = (-reach - r.o.z) / r.d.z;
        let b = (reach - r.o.z) / r.d.z;
        s0 = max(s0, min(a, b));
        s1 = min(s1, max(a, b));
    } else if abs(r.o.z) > reach {
        return out;
    }
    let cell = st.grid.z;
    let lo = st.grid.xy;
    let hi = lo + st.grid_size.xy * cell;
    for (var axis = 0; axis < 2; axis++) {
        let o = r.o[axis];
        let d = r.d[axis];
        if abs(d) > 1e-9 {
            let a = (lo[axis] - o) / d;
            let b = (hi[axis] - o) / d;
            s0 = max(s0, min(a, b));
            s1 = min(s1, max(a, b));
        } else if o < lo[axis] || o > hi[axis] {
            return out;
        }
    }
    if s1 < s0 {
        return out;
    }
    let size = vec2<i32>(st.grid_size.xy);
    let q = r.o.xy + r.d.xy * s0;
    var c = clamp(vec2<i32>(floor((q - lo) / cell)), vec2<i32>(0), size - 1);
    let go = vec2<i32>(select(-1, 1, r.d.x >= 0.0), select(-1, 1, r.d.y >= 0.0));
    var next = vec2<f32>(1e30);
    var delta = vec2<f32>(1e30);
    if abs(r.d.x) > 1e-9 {
        let edge = lo.x + f32(c.x + select(0, 1, r.d.x > 0.0)) * cell;
        next.x = (edge - r.o.x) / r.d.x;
        delta.x = cell / abs(r.d.x);
    }
    if abs(r.d.y) > 1e-9 {
        let edge = lo.y + f32(c.y + select(0, 1, r.d.y > 0.0)) * cell;
        next.y = (edge - r.o.y) / r.d.y;
        delta.y = cell / abs(r.d.y);
    }
    let dd = dot(r.d, r.d);
    for (var i = 0; i < CAM_MOST_CELLS; i++) {
        let range = cells[c.y * size.x + c.x];
        for (var k = 0u; k < min(range.y, MOST_IN_CELL); k++) {
            let index = items[range.x + k];
            let sp = spheres[index];
            let oc = r.o - vec3<f32>(sp.a.xy, 0.0);
            let b = dot(oc, r.d);
            let cc = dot(oc, oc) - sp.a.z * sp.a.z;
            let disc = b * b - dd * cc;
            if disc >= 0.0 {
                let t = (-b - sqrt(disc)) / dd;
                if t > 0.0 && t < out.t {
                    out = CamHit(t, 1u, index, r.o + r.d * t);
                }
            }
        }
        // Past the nearest ball found, or past the band: nothing nearer is left.
        if min(next.x, next.y) > min(s1, out.t) {
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
    return out;
}

// Whether world point `p` on the picture is inside something that blocks light, read from the
// full-size surface picture (the field is coarser, and its steps would show along a shape's side).
fn cam_blocks(p: vec2<f32>) -> bool {
    let uv = world_to_uv(p);
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return false;
    }
    return textureSampleLevel(surface, samp, uv, 0.0).r >= 0.5;
}

// The shapes: what blocks light standing out of the wall up to the shapes' front. The field says
// how far a ray may safely step; the surface picture says where the outline is, found exactly by
// halving the last step.
fn cam_slab(r: CamRay, best: CamHit) -> CamHit {
    var out = best;
    let back = -st.room.y;
    let front = st.room.z;
    if front <= back + 1e-3 || r.d.z >= -1e-9 {
        return out;
    }
    let enter = (front - r.o.z) / r.d.z;
    let leave = min((back - r.o.z) / r.d.z, out.t);
    var t = max(enter, 0.0);
    if t >= leave {
        return out;
    }
    let px = u.view.z;
    let flat = max(length(r.d.xy), 1e-9);
    // The field is a few texels rough: step short of it by that much, but at least 1.5 px.
    let margin = 1.5 * max(u.field.y, px);
    let least = 1.5 * px / max(length(r.d), 1e-9);
    var before = t;
    for (var i = 0; i < CAM_SLAB_STEPS; i++) {
        let p = r.o + r.d * min(t, leave);
        if cam_blocks(p.xy) {
            var a = before;
            var b = min(t, leave);
            if i > 0 {
                for (var k = 0; k < 8; k++) {
                    let m = 0.5 * (a + b);
                    if cam_blocks((r.o + r.d * m).xy) {
                        b = m;
                    } else {
                        a = m;
                    }
                }
            }
            let face = select(3u, 2u, i == 0 && enter >= 0.0);
            return CamHit(b, face, 0u, r.o + r.d * b);
        }
        if t >= leave {
            return out;
        }
        before = t;
        t += max((field_at(p.xy) - margin) / flat, least);
    }
    return out;
}

// The back wall, or the floor in front of it.
fn cam_far(r: CamRay) -> CamHit {
    var out = CamHit(1e30, 0u, 0u, vec3<f32>(0.0));
    if r.d.z >= -1e-9 {
        return out;
    }
    let back = st.room.y;
    let tw = (-back - r.o.z) / r.d.z;
    if tw > 0.0 {
        out = CamHit(tw, 4u, 0u, r.o + r.d * tw);
    }
    if st.floor.x > 0.5 && r.d.y > 1e-9 {
        let tf = (floor_y() - r.o.y) / r.d.y;
        let x = r.o + r.d * tf;
        if tf > 0.0 && tf < out.t && x.z >= -back {
            out = CamHit(tf, 5u, 0u, x);
        }
    }
    return out;
}

// The solid colour a hit takes, and where the lit colour and the picture before lighting are
// read for it.
struct CamSolid {
    colour: vec3<f32>,
    uv: vec2<f32>,
};

fn cam_solid(h: CamHit) -> CamSolid {
    let px = u.view.z;
    let moving = clamp(cam.right.w, 0.0, 1.0);
    let uv = cam_rest_uv(h.p);
    if h.kind == 1u {
        // A ball: once the camera moves, read a little inside its drawn edge, so the picture round
        // it doesn't ring a turning ball.
        let sp = spheres[h.id];
        let at = cam_rest_point(h.p);
        let off = at - sp.a.xy;
        let most = max(sp.a.z - 1.5 * px, 0.0);
        let inside = select(at, sp.a.xy + off * (most / max(length(off), 1e-6)), length(off) > most);
        let read = world_to_uv(mix(at, inside, moving));
        return CamSolid(cam_lit(read), read);
    }
    if h.kind == 3u {
        // A shape's side: the colour just inside its edge, shaded, once the camera moves.
        let e = 2.0 * px;
        let g = vec2<f32>(field_at(h.p.xy + vec2<f32>(e, 0.0)) - field_at(h.p.xy - vec2<f32>(e, 0.0)),
            field_at(h.p.xy + vec2<f32>(0.0, e)) - field_at(h.p.xy - vec2<f32>(0.0, e)));
        let n = g / max(length(g), 1e-9);
        let side_uv = cam_rest_uv(vec3<f32>(h.p.xy - n * e, st.room.z));
        let side = cam_lit(side_uv) * cam.shift.w;
        return CamSolid(mix(cam_lit(uv), side, moving), mix(uv, side_uv, moving));
    }
    if h.kind == 4u && field_at(h.p.xy) <= 0.0 {
        // A shape painted flat on the wall (no Shape depth).
        return CamSolid(cam_lit(uv), uv);
    }
    if h.kind >= 4u {
        return CamSolid(cam_wall(uv), uv);
    }
    return CamSolid(cam_lit(uv), uv);
}

struct CamSeen {
    colour: vec3<f32>,
    // Where the picture before lighting and the lit colour are read for this ray.
    uv: vec2<f32>,
    // What it hit: the kind, and the ball's index (so edges between things can be told).
    what: u32,
};

// Everything the ray through screen point `s` sees: the nearest solid, and the sheet of flat
// things on the picture plane over it.
fn cam_see(s: vec2<f32>) -> CamSeen {
    let r = cam_ray(s);
    var h = cam_far(r);
    h = cam_slab(r, h);
    h = cam_spheres(r, h);
    var solid = CamSolid(vec3<f32>(0.0), vec2<f32>(0.5));
    if h.kind != 0u {
        solid = cam_solid(h);
    }
    var seen = CamSeen(solid.colour, solid.uv, h.kind * 65536u + h.id);
    if r.d.z < -1e-9 {
        let t0 = -r.o.z / r.d.z;
        if t0 > 0.0 && t0 <= h.t {
            let uv0 = world_to_uv((r.o + r.d * t0).xy);
            let a = cam_things(uv0);
            if a > 0.0 {
                // The flat things there over what is behind them: in the front view what is behind is
                // the wall at the same pixel, so this is the lit picture exactly.
                let sheet = cam_lit(uv0) - (1.0 - a) * cam_wall(uv0);
                seen.colour = sheet + (1.0 - a) * solid.colour;
                if a >= 0.5 {
                    seen.uv = uv0;
                    seen.what = 7u * 65536u;
                }
            }
        }
    }
    return seen;
}

struct CamOut {
    @location(0) picture: vec4<f32>,
    @location(1) unlit: vec4<f32>,
    @location(2) lit: vec4<f32>,
};

@fragment
fn fs_cam_view(in: FsIn) -> CamOut {
    let s = (in.uv - 0.5) * cam.size.xy;
    let pixel = cam.size.xy / max(cam.size.zw, vec2<f32>(1.0));
    let mid = cam_see(s);
    var colour = mid.colour;
    if cam.down.w > 1.5 {
        // Where the pixel's corners see something else, it takes the mean of its middle and corners.
        var corners = array<vec2<f32>, 4>(
            vec2<f32>(-0.375, -0.125), vec2<f32>(0.125, -0.375), vec2<f32>(0.375, 0.125), vec2<f32>(-0.125, 0.375));
        var sum = mid.colour;
        var edge = false;
        for (var i = 0; i < 4; i++) {
            let c = cam_see(s + corners[i] * pixel);
            sum += c.colour;
            edge = edge || c.what != mid.what;
        }
        if edge {
            colour = sum / 5.0;
        }
    }
    var out: CamOut;
    out.picture = vec4<f32>(clamp(colour, vec3<f32>(0.0), vec3<f32>(6.0e4)), 1.0);
    out.unlit = textureSampleLevel(picture, samp, mid.uv, 0.0);
    out.lit = textureSampleLevel(cam_d, samp, mid.uv, 0.0);
    return out;
}

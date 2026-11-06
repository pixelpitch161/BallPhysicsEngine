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
    // Ambient light (rgb, already times its strength), w unused.
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
};

struct SurfaceGpu {
    // Roughness, metal, clear coat, see-through.
    a: vec4<f32>,
    // F0 (how much a straight-on look reflects), toon steps (0 smooth), reflects the sky (1) or
    // nothing (0), kind (0 sheet, 1 ball, 2 shape).
    b: vec4<f32>,
};

struct SphereGpu {
    // Centre xy (world, on the picture plane), radius, see-through.
    a: vec4<f32>,
    // The ball's colour (linear), for the light it lets through; w unused.
    b: vec4<f32>,
};

@group(1) @binding(0) var<uniform> st: Stage;
@group(1) @binding(1) var<storage, read> surfaces: array<SurfaceGpu>;
@group(1) @binding(2) var<storage, read> spheres: array<SphereGpu>;
@group(1) @binding(3) var<storage, read> cells: array<vec2<u32>>;
@group(1) @binding(4) var<storage, read> items: array<u32>;

const PI: f32 = 3.14159265;
// The flattest the floor is ever seen: tan(0.5 degrees), so it always has some area.
const LEAST_K: f32 = 0.0087;

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

// The table entry of the surface on a surface-picture texel.
fn surface_index(s: vec4<f32>) -> u32 {
    if s.a < 0.02 {
        return 0u;
    }
    let i = u32(max(round(s.b / s.a), 0.0));
    return select(0u, i, i < u32(st.more.z));
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
    let tc = dot(oc, l);
    if tc <= 0.0 || tc >= dist {
        return vec3<f32>(1.0);
    }
    let miss = length(oc - l * tc) - r;
    let v = clamp(0.5 + 0.5 * miss / (tc * max(ang, 1e-4)), 0.0, 1.0);
    let through = sp.b.rgb * clamp(sp.a.w, 0.0, 1.0);
    return mix(through, vec3<f32>(1.0), v * v * (3.0 - 2.0 * v));
}

// How much of a light gets to `p` past every ball, going along unit `l` for `dist`: the grid is
// walked only where the ray is within a ball's reach of the picture plane.
fn sphere_shadow(p: vec3<f32>, l: vec3<f32>, dist: f32, ang: f32) -> vec3<f32> {
    var vis = vec3<f32>(1.0);
    if st.more.w < 0.5 {
        return vis;
    }
    let reach = st.grid.w + st.grid_size.z;
    var s0 = 0.0;
    var s1 = dist;
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
    let across = length(l.xy);
    let step = select(s1 - s0 + 1.0, 0.5 * cell / across, across > 1e-6);
    let size = vec2<i32>(st.grid_size.xy);
    var last = -1;
    var s = s0;
    for (var i = 0; i < 512; i++) {
        let q = p.xy + l.xy * min(s, s1);
        let c = clamp(vec2<i32>(floor((q - lo) / cell)), vec2<i32>(0), size - 1);
        let ci = c.y * size.x + c.x;
        if ci != last {
            last = ci;
            let range = cells[ci];
            for (var k = 0u; k < range.y; k++) {
                vis = min(vis, sphere_block(p, l, dist, ang, spheres[items[range.x + k]]));
            }
            if max(vis.x, max(vis.y, vis.z)) <= 0.0 {
                break;
            }
        }
        if s >= s1 {
            break;
        }
        s += step;
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
    let margin = max(st.grid_size.z, 1e-3);
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
        let near = clamp(1.0 - (len - r) / margin, 0.0, 1.0);
        occ += (r * r) / (len * len) * max(dot(n, d / len), 0.0) * near;
    }
    return clamp(1.0 - occ, 0.0, 1.0);
}

// How much light gets to `p` past the shapes in Stage: the blockers' field stood up from the wall
// to the shapes' front, marched along the ray while it is within that depth.
fn slab_visibility(p: vec3<f32>, l: vec3<f32>, dist: f32, ang: f32) -> f32 {
    let across = length(l.xy);
    let front = st.room.z;
    let back = -st.room.y;
    if across < 1e-5 || (p.z >= front - 0.01 && l.z >= 0.0) {
        return 1.0;
    }
    let toward = l.xy / across;
    let rise = l.z / across;
    var end = dist * across;
    if rise > 1e-6 {
        end = min(end, (front - p.z) / rise);
    } else if rise < -1e-6 {
        end = min(end, (back - p.z) / rise);
    }
    if end <= 0.0 {
        return 1.0;
    }
    return visibility(p.xy, toward, end, ang / max(across, 0.05), 1e9, 0.0);
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
    let aw = min(a + 0.5 * ang, 1.0);
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

// What a point takes from the lights and the room.
struct Shaded {
    // Diffuse light (ambient, sky and lamps, with shadows).
    diffuse: vec3<f32>,
    // Highlights and reflections, already coloured by the surface's Fresnel.
    spec: vec3<f32>,
    // Light coming through a see-through surface (no turning to the lamp).
    through: vec3<f32>,
};

// Lights the point `spot` (world point `p` on the picture) wearing surface `sf`, whose plain
// colour is `base` (for metal), `here` the blockers' field there.
fn shade(spot: Spot, p: vec2<f32>, sf: SurfaceGpu, base: vec3<f32>, here: f32) -> Shaded {
    let stage = in_stage();
    let px = u.view.z;
    let n = spot.n;
    let v = view_dir();
    let h = spot.p.z;
    let rough = clamp(sf.a.x, 0.02, 1.0);
    let metal = clamp(sf.a.y, 0.0, 1.0);
    let coat = max(sf.a.z, 0.0);
    let toon = sf.b.y;
    let f0 = mix(vec3<f32>(max(sf.b.x, 0.0)), clamp(base, vec3<f32>(0.0), vec3<f32>(1.0)), metal);
    let shiny = metal > 0.0 || sf.b.x > 0.0 || coat > 0.0;
    let style = u.shadows.x;
    var out: Shaded;
    out.diffuse = vec3<f32>(0.0);
    out.spec = vec3<f32>(0.0);
    out.through = vec3<f32>(0.0);
    let count = u32(u.more.w);
    for (var i = 0u; i < count; i++) {
        let l = lights[i];
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
        var turned = max(1.0 + dot(n, dir3) - dir3.z, 0.0);
        if stage {
            turned = max(dot(n, dir3), 0.0);
        }
        var tint = vec3<f32>(1.0);
        if l.p5.w > 0.5 && style < 2.5 && (dist > 1e-3 || stage) {
            var soft = angular * max(u.shadows.y, 0.0) * max(l.p6.w, 0.0);
            if style > 0.5 {
                soft = 0.5 * px / max(select(dist, dist3, stage), 1e-3);
            }
            var v = vec3<f32>(1.0);
            if stage {
                v = sphere_shadow(spot.p, dir3, dist3, soft) * slab_visibility(spot.p, dir3, dist3, soft);
            } else {
                // The march stops short of the lamp by its size, a ridden ball's radius and
                // however deep in a blocker the lamp sits (a strip along a shape's band, a light
                // in a ball).
                var stop = l.p6.x + max(l.p2.z, 0.0);
                if kind < 1.5 || kind > 2.5 {
                    stop = max(stop, -field_at(source_point(l, p)));
                }
                v = vec3<f32>(visibility(p, toward, dist, soft, u.shadows.z, stop));
            }
            tint = mix(vec3<f32>(1.0), l.p5.rgb, clamp((vec3<f32>(1.0) - v) * l.p2.w, vec3<f32>(0.0), vec3<f32>(1.0)));
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
        if shiny {
            var s = vec3<f32>(0.0);
            if toon > 0.5 {
                s = toon_spot(n, v, dir3, rough, f0);
            } else {
                s = ggx(n, v, dir3, rough, f0, angular);
                if coat > 0.0 {
                    s = s * (1.0 - 0.04 * coat) + ggx(n, v, dir3, 0.05, vec3<f32>(0.04), angular) * coat;
                }
            }
            out.spec += l.p1.rgb * amount * tint * s;
        }
    }
    // The room: ambient and the sky's light from above.
    var room = u.ambient.rgb;
    if st.sky_top.w != 0.0 {
        room += st.sky_top.w * mix(st.sky_bottom.rgb, st.sky_top.rgb, 0.5 - 0.5 * n.y);
    }
    out.diffuse += room;
    out.through += room;
    // What a shiny surface mirrors of the room: the sky, blurred by roughness.
    if shiny && sf.b.z > 0.5 {
        let r = reflect(-v, n);
        let blurred = normalize(mix(r, n, rough * rough));
        let nv = max(dot(n, v), 0.0);
        let edge = pow(1.0 - nv, 5.0);
        let env = sky(blurred) * (u.ambient.rgb + vec3<f32>(st.sky_top.w));
        let fe = f0 + (max(vec3<f32>(1.0 - rough), f0) - f0) * edge;
        out.spec += env * fe;
        if coat > 0.0 {
            out.spec += sky(r) * (u.ambient.rgb + vec3<f32>(st.sky_top.w)) * (0.04 + 0.96 * edge) * coat;
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
    }
    return out;
}

// The lit colour of a pixel of colour `c` (lit share `w`) shaded as `sh` with surface `sf`, of
// which `share` takes highlights.
fn mixed(c: vec3<f32>, w: vec3<f32>, sh: Shaded, sf: SurfaceGpu, share: f32) -> vec3<f32> {
    let metal = clamp(sf.a.y, 0.0, 1.0);
    let see = clamp(sf.a.w, 0.0, 1.0);
    var diffuse = sh.diffuse;
    if see > 0.0 {
        diffuse = sh.diffuse * (1.0 - see) + sh.through * (0.6 * see);
    }
    return c + (diffuse - vec3<f32>(1.0)) * w * (1.0 - metal) - w * metal + sh.spec * share;
}

@fragment
fn fs_light(in: FsIn) -> @location(0) vec4<f32> {
    let ip = vec2<i32>(in.pos.xy);
    let c = textureLoad(picture, ip, 0);
    // The lit share of the colour: never more than the colour itself.
    let w = clamp(textureLoad(lit_picture, ip, 0).rgb, vec3<f32>(0.0), max(c.rgb, vec3<f32>(0.0)));
    let p = uv_to_world(in.uv);
    let px = u.view.z;
    let s = textureLoad(surface, ip, 0);
    let sf = surfaces[surface_index(s)];
    // How much of the colour takes highlights: its lit share (a black thing shines whole).
    let top = max(c.r, max(c.g, c.b));
    var share = 1.0;
    if top > 1e-4 {
        share = clamp(max(w.r, max(w.g, w.b)) / top, 0.0, 1.0);
    }
    // The thing's surface's normal from its height's slope (flat where nothing round is).
    let h = height_at(ip);
    let hx = height_at(ip + vec2<i32>(1, 0)) - height_at(ip - vec2<i32>(1, 0));
    let hy = height_at(ip + vec2<i32>(0, 1)) - height_at(ip - vec2<i32>(0, 1));
    var slope = vec2<f32>(hx, hy) / (2.0 * px);
    if u.camera.w != 0.0 {
        slope = rotate_by(slope, u.camera.w);
    }
    var thing: Spot;
    thing.p = vec3<f32>(p, h);
    thing.n = normalize(vec3<f32>(-slope, 1.0));
    let here = field_at(p);
    var out = vec3<f32>(0.0);
    if !in_stage() {
        out = mixed(c.rgb, w, shade(thing, p, sf, c.rgb / max(share, 1e-3), here), sf, share);
    } else {
        // A shape's front stands at the shapes' depth; a ball's surface at its height.
        if sf.b.w > 1.5 {
            thing.p.z = st.room.z;
        }
        let cover = clamp(s.a, 0.0, 1.0);
        var on_thing = vec3<f32>(0.0);
        var on_stage = vec3<f32>(0.0);
        if cover > 0.02 {
            on_thing = mixed(c.rgb, w, shade(thing, p, sf, c.rgb / max(share, 1e-3), here), sf, share);
        }
        if cover < 0.98 {
            let spot = stage_spot(p);
            let floor = st.floor.x > 0.5 && spot.n.y < -0.5;
            let plane = plane_surface(select(st.floor.w, st.floor.z, floor));
            let lit = clamp(u.ambient.w, 0.0, 1.0);
            on_stage = mixed(c.rgb, w, shade(spot, p, plane, c.rgb, field_at(spot.p.xy)), plane, lit);
        }
        out = mix(on_stage, on_thing, smoothstep(0.02, 0.98, cover));
    }
    // Kept finite: a huge Strength would overflow the half-float picture to infinity.
    return vec4<f32>(min(out, vec3<f32>(6.0e4)), c.a);
}

// ---------- the glossy floor (Stage, a floor that isn't Matte) ----------

// The lit picture (bound as `picture`) at world point `q`, blurred over `radius` world units;
// its weight falls to 0 off the picture.
fn lit_near(q: vec2<f32>, radius: f32) -> vec4<f32> {
    var sum = vec3<f32>(0.0);
    var weight = 0.0;
    for (var k = 0; k < 9; k++) {
        var o = vec2<f32>(0.0);
        if k > 0 {
            let a = f32(k) * 2.39996;
            o = vec2<f32>(cos(a), sin(a)) * radius * sqrt(f32(k) / 8.0);
        }
        let uv = world_to_uv(q + o);
        let inside = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
        let off = length(uv - inside);
        let wt = clamp(1.0 - off * 20.0, 0.0, 1.0);
        sum += textureSampleLevel(picture, samp, inside, 0.0).rgb * wt;
        weight += wt;
    }
    return vec4<f32>(sum / max(weight, 1e-4), weight / 9.0);
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
    let wall_q = vec2<f32>(p.x, 2.0 * seam - p.y);
    let wall = lit_near(wall_q, blur * (p.y - seam) * 0.5);
    if p.y >= f {
        let q = vec2<f32>(p.x, 2.0 * f - p.y);
        let near = lit_near(q, blur * (p.y - f) * 0.5);
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

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
    // Glass and light part 4a: throws light, facets (0 smooth), facet gaps, spare.
    f: vec4<f32>,
    // Part 4b: Dichroic, Dichroic shift and Dichroic hue (in turns), Fire.
    g: vec4<f32>,
};

struct SphereGpu {
    // Centre xy (world, on the picture plane), radius, see-through.
    a: vec4<f32>,
    // The ball's colour (linear), for the light it lets through; w unused.
    b: vec4<f32>,
    // Its surface index, its Tints light, its turn (radians: a mirror ball's tiles turn with it),
    // its thing's z-order (a ball with a higher one, or the same and listed later, is drawn over
    // it).
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
// Depth of field's small copy is this many times smaller each way (`DOF_SMALL` in lighting.rs).
const DOF_SMALL: f32 = 4.0;
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

// `field_on_slab` with how much the blocker there blocks of each channel (`field_all`).
fn field_on_slab_all(q: vec3<f32>) -> vec4<f32> {
    return field_all(vec2<f32>(q.x, q.y + q.z * st.room.w));
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
    let m = length(oc - l * tc);
    let miss = m - r;
    let v = clamp(0.5 + 0.5 * miss / (tc * max(ang, 1e-4)), 0.0, 1.0);
    var through = sp.b.rgb * clamp(sp.a.w, 0.0, 1.0);
    if u.trace.z > 0.5 {
        // Glass and light part 1: its colour by its Tints light, deeper where the ray passes
        // through more of it (its middle) by its Deeper when thick.
        let f = sqrt(max(1.0 - (m / max(r, 1e-4)) * (m / max(r, 1e-4)), 0.0));
        // Tints light 1 and Deeper when thick 0: exactly the colour, as before part 1.
        let tinted = mix(vec3<f32>(1.0), sp.b.rgb, clamp(sp.c.y, 0.0, 1.0));
        let deeper = 1.0 + THICK_DEPTH * max(sp.b.w, 0.0) * f;
        let deep = pow(max(tinted, vec3<f32>(1.0 / 1024.0)), vec3<f32>(deeper));
        through = clamp(sp.a.w, 0.0, 1.0) * select(deep, tinted, deeper <= 1.0);
    }
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

// One ray of `slab_visibility`: how much of the light gets through past the solid shapes (`v`,
// before smoothing; `face` the front face's share for a point in front of the shapes), how clear
// of them it stays (`clear`: the least distance to one over the distance along the ray, an angle
// like the lamp's), whether it went into a solid one (`crossed`, and how wide the soft edge is
// there, `edge`, in world units), what the see-through ones let through (`see`) and, where it
// went into one just below the shapes' front on its way up and out, the share of the lamp that
// shows over that front (`over`: half at the front, nothing a soft edge's width below it).
struct SlabRay {
    v: f32,
    clear: f32,
    crossed: bool,
    edge: f32,
    see: vec3<f32>,
    face: vec3<f32>,
    over: f32,
};

// One ray of `slab_visibility`: along unit `l` for `dist`, the lamp `ang` wide. `across`: one of
// the rays across the lamp, which stops as soon as it goes into a solid shape.
fn slab_march(p: vec3<f32>, l: vec3<f32>, dist: f32, ang: f32, stop: f32, across: bool) -> SlabRay {
    var out = SlabRay(1.0, 1e4, false, 0.0, vec3<f32>(1.0), vec3<f32>(1.0), 0.0);
    let flat_len = length(l.xy);
    let front = st.room.z;
    let back = -st.room.y;
    if flat_len < 1e-5 || (p.z >= front - 0.01 && l.z >= 0.0) {
        return out;
    }
    let toward = l.xy / flat_len;
    let rise = l.z / flat_len;
    var end = dist * flat_len - max(stop, 0.0);
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
        return out;
    }
    let a = max(ang / max(flat_len, 0.05), 1e-4);
    let texel = max(u.field.z, 1e-3);
    if start > 0.0 {
        let fd = field_on_slab_all(vec3<f32>(p.xy + toward * start, front));
        var f = clamp(0.5 + 0.5 * fd.x / max(start * a, texel), 0.0, 1.0);
        f = f * f * (3.0 - 2.0 * f);
        if opacity_of(fd.yzw) >= 1.0 {
            if f <= 0.0 || (across && fd.x < 0.0) {
                out.v = 0.0;
                out.crossed = true;
                out.edge = start * a;
                return out;
            }
            out.face = vec3<f32>(f);
        } else {
            out.face = vec3<f32>(1.0) - clamp(fd.yzw, vec3<f32>(0.0), vec3<f32>(1.0)) * (1.0 - f);
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
    // As in `march`: creeping past a shape (closer than a texel) doesn't use up the steps, within
    // a cap, and the soft edge is taken where the ray came nearest a shape between samples.
    var used = 0;
    var last = -1.0;
    var last_t = 0.0;
    // (Nor do steps shorter than an even share of the way, as in `march`.)
    let fair = (end - t) / f32(max(steps, 1));
    for (var i = 0; i < max(8 * steps, 300); i++) {
        if t >= end || used >= steps {
            break;
        }
        let fd = field_on_slab_all(vec3<f32>(p.xy + toward * t, p.z + rise * t));
        let d = fd.x;
        let nearest = nearest_between(d, t - start, last, last_t);
        last = d;
        last_t = t - start;
        if abs(d) >= max(texel, fair) {
            used += 1;
        }
        // How far into the soft edge, in lamp half widths from its middle.
        let k = nearest.x / (max(nearest.y, texel) * a);
        let raw = clamp(0.5 + 0.5 * k, 0.0, 1.0);
        let solid = opacity_of(fd.yzw) >= 1.0
            || nearest_bends(vec2<f32>(p.x + toward.x * t, p.y + toward.y * t + (p.z + rise * t) * st.room.w));
        if solid {
            // (Glass that bends lamp light, part 3, is solid here: the photons carry its light.)
            out.clear = min(out.clear, max(k, 0.0) * ang);
            if d < 0.0 {
                // Into a solid shape: deeper than its soft edge, or a thin shape whose middle is
                // only half its width deep, the light across the lamp tells.
                if !out.crossed {
                    out.edge = max(t - start, texel) * a;
                    if rise > 1e-6 {
                        let below = front - (p.z + rise * t);
                        out.over = clamp(0.5 - 0.5 * below / out.edge, 0.0, 1.0);
                    }
                }
                out.crossed = true;
                if across {
                    out.v = 0.0;
                    return out;
                }
            }
            out.v = min(out.v, raw);
            if out.v <= 0.0 {
                break;
            }
        } else {
            let b = clamp(fd.yzw, vec3<f32>(0.0), vec3<f32>(1.0));
            out.see = min(out.see, vec3<f32>(1.0) - b * (1.0 - smoothed(raw)));
        }
        // At most half a texel near a shape (as `march`): a whole texel stepped over a thin
        // wall's slab, and the light got through in stripes.
        t += max(abs(d), 0.5 * texel);
    }
    return out;
}

// How much light gets to `p` past the shapes in Stage: the blockers' field stood up from the wall
// to the shapes' front (sheared, see `field_on_slab`), marched along the ray while it is within
// that depth. Per channel (glass and light part 1): a see-through shape (the field's blocking under
// 1, drawn when the tint picture is) lets through its share in its colour.
//
// As `visibility` in Flat: a ray that goes into a solid shape would let half the light through a
// thin one (inside it the distance is only half its width deep), so then rays go across the lamp,
// side to side in the picture, and the light is the share of the lamp they show together.
fn slab_visibility(p: vec3<f32>, l: vec3<f32>, dist: f32, ang: f32, stop: f32) -> vec3<f32> {
    let middle = slab_march(p, l, dist, ang, stop, false);
    let lit = middle.face * middle.see;
    if !middle.crossed {
        return middle.face * min(vec3<f32>(smoothed(middle.v)), middle.see);
    }
    // Deep in its shadow, or a soft edge narrower than half a pixel where it went in: dark, but
    // for what shows over the shapes' front.
    if middle.v <= 0.0 || middle.edge < 0.5 * u.view.z {
        return vec3<f32>(middle.over) * lit;
    }
    let a = max(ang, 1e-4);
    let rays = select(LAMP_RAYS, 3, u.more.z <= 16.0);
    let gap = 2.0 * a / f32(rays - 1);
    let flat_len = length(l.xy);
    let side = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(-l.y, l.x, 0.0) / flat_len, flat_len > 1e-5);
    var lo: array<f32, MOST_LAMP_RAYS>;
    var hi: array<f32, MOST_LAMP_RAYS>;
    for (var k = 0; k < MOST_LAMP_RAYS; k++) {
        let off = f32(k) * gap - a;
        lo[k] = 0.0;
        hi[k] = 0.0;
        if k >= rays || k == rays / 2 {
            continue;
        }
        let r = slab_march(p, normalize(l + side * off), dist, gap, stop, true);
        let c = select(min(r.clear, 2.0 * a), -1.0, r.crossed);
        lo[k] = max(off - c, -a);
        hi[k] = min(off + c, a);
    }
    return vec3<f32>(max(lamp_share(lo, hi, a), middle.over)) * lit;
}


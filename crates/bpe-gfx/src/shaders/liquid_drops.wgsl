// A Liquid thing's drops (liquid part 1), splatted straight from the card's buffer into the
// liquid field (colour × colour weight, and the weight) and the colour weights. See liquid.rs.

struct Globals {
    view: vec4<f32>,
    bg_paint: vec4<f32>,
    bg_place: vec4<f32>,
    post: vec4<f32>,
    // centre xy, zoom, turn
    camera: vec4<f32>,
    bg: vec4<f32>,
    bg_from: vec4<f32>,
    bg_to: vec4<f32>,
    light: vec4<f32>,
};

struct Style {
    a: vec4<f32>,
    b: vec4<f32>,
    c: vec4<f32>,
    sky_top: vec4<f32>,
    sky_bottom: vec4<f32>,
    d: vec4<f32>,
    // the grid's origin xy (world), its cell, each drop's reach (world units)
    e: vec4<f32>,
    // Swirl detail (0 to 1), colours from the weights (1)
    f: vec4<f32>,
    // Drops on the card: the one colour.
    g: vec4<f32>,
    // Liquid part 3 (see liquid.wgsl): only `m` (foam) is read here.
    h: vec4<f32>,
    i: vec4<f32>,
    j: vec4<f32>,
    k: vec4<f32>,
    l: vec4<f32>,
    // Foam: amount, from speed (world units a second), splashes foam, on top.
    m: vec4<f32>,
    n: vec4<f32>,
    o: vec4<f32>,
    p: vec4<f32>,
    q: vec4<f32>,
    r: vec4<f32>,
    s: vec4<f32>,
    t: vec4<f32>,
    u: vec4<f32>,
    // The wet grid's columns, rows and newest half (the renderer fills these), 1 for a full look.
    v: vec4<f32>,
};

// Numbers a drop takes in the card's buffer (liquid_sim.wgsl's `Drop`): x at 0, v at 2, colour at
// 8, j at 12, kind at 13, serial at 14, kick at 15.
const DROP_FLOATS: u32 = 24u;

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var<uniform> st: Style;
// The drops, DROP_FLOATS numbers each (liquid_sim.wgsl's `Drop`): x, v, c, colour, j, kind, serial, ...
@group(1) @binding(1) var<storage, read> drops: array<f32>;
// The body's wet grid (liquid_wet.wgsl): two halves of wetness, then each node's distance from its
// wall's face on the liquid's side (grid units; 4 off the walls band, -4 where the side isn't known).
@group(1) @binding(2) var<storage, read> wet_grid: array<f32>;

fn to_clip(p: vec2<f32>) -> vec4<f32> {
    let c = g.camera;
    var d = p - c.xy;
    if c.w != 0.0 {
        let s = sin(-c.w);
        let k = cos(-c.w);
        d = vec2<f32>(d.x * k - d.y * s, d.x * s + d.y * k);
    }
    let q = d * c.z;
    return vec4<f32>(q.x * g.view.x, -q.y * g.view.y, 0.0, 1.0);
}

// Liquid fixes part B: how far grid node (x, y) is from its wall's face on the liquid's side.
fn face_node(x: i32, y: i32) -> f32 {
    let cols = i32(st.v.x);
    let rows = i32(st.v.y);
    if cols < 1 || x < 0 || y < 0 || x >= cols || y >= rows {
        return 4.0;
    }
    return wet_grid[2u * u32(cols * rows) + u32(y * cols + x)];
}

// The face distance at grid point `q` (between its four nodes), and 1 when all four are in the
// walls band with their side known (else 0).
fn face_at(q: vec2<f32>) -> vec2<f32> {
    let i = floor(q);
    let f = q - i;
    let x = i32(i.x);
    let y = i32(i.y);
    let a = face_node(x, y);
    let b = face_node(x + 1, y);
    let c = face_node(x, y + 1);
    let d = face_node(x + 1, y + 1);
    let ok = max(max(abs(a), abs(b)), max(abs(c), abs(d))) < 3.5;
    return vec2<f32>(mix(mix(a, b, f.x), mix(c, d, f.x), f.y), select(0.0, 1.0, ok));
}

// The wall face a drop at grid point `q` is within `reach` cells of (on the liquid's side): xy a
// point on the face, zw its normal into the liquid (grid units); zw 0 when there is none.
fn wall_face(q: vec2<f32>, reach: f32) -> vec4<f32> {
    let f = face_at(q);
    if f.y < 0.5 || f.x < -0.25 || f.x > reach {
        return vec4<f32>(0.0);
    }
    let e = 0.5;
    let fx0 = face_at(q - vec2<f32>(e, 0.0));
    let fx1 = face_at(q + vec2<f32>(e, 0.0));
    let fy0 = face_at(q - vec2<f32>(0.0, e));
    let fy1 = face_at(q + vec2<f32>(0.0, e));
    if min(min(fx0.y, fx1.y), min(fy0.y, fy1.y)) < 0.5 {
        return vec4<f32>(0.0);
    }
    let grad = vec2<f32>(fx1.x - fx0.x, fy1.x - fy0.x) / (2.0 * e);
    let len = length(grad);
    if len < 0.5 || len > 1.5 {
        return vec4<f32>(0.0);
    }
    let n = grad / len;
    return vec4<f32>(q - n * f.x, n);
}

struct DropOut {
    @builtin(position) pos: vec4<f32>,
    // Where in the drop, its reach 1.
    @location(0) at: vec2<f32>,
    @location(1) colour: vec3<f32>,
    // Liquid part 3: its foam (0 to 1), speed (world units a second / 1000) and kick.
    @location(2) more: vec3<f32>,
    // How much stronger its splat is: a drop with little liquid round it would never reach the
    // Edge level on its own, so it is lifted to draw as a small droplet.
    @location(3) boost: f32,
    // Liquid fixes part B: where this pixel is (world), and the wall face near the drop (a point
    // on it and its normal into the liquid, world; the normal 0 when no wall is near). Nothing is
    // splatted past the face, so a drop and its mirror image fill the liquid right up to the wall.
    @location(4) world: vec2<f32>,
    @location(5) @interpolate(flat) face: vec4<f32>,
};

@vertex
fn vs_drop(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> DropOut {
    return drop_out(vi, ii, false);
}

// The drops near a wall's face, mirrored across it (liquid fixes part B): without them the field
// thins out short of the wall, which showed as a gap and a rounded "mercury" corner. Drops not
// near a wall draw nothing here.
@vertex
fn vs_mirror(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> DropOut {
    return drop_out(vi, ii, true);
}

fn drop_out(vi: u32, ii: u32, mirror: bool) -> DropOut {
    var out: DropOut;
    let at = ii * DROP_FLOATS;
    var x = vec2<f32>(drops[at], drops[at + 1u]);
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u)) * 2.0 - 1.0;
    let r = max(st.e.w, 0.0);
    let cell = max(st.e.z, 1e-6);
    let wall = wall_face(x, r / cell);
    out.face = vec4<f32>(0.0);
    if wall.z != 0.0 || wall.w != 0.0 {
        out.face = vec4<f32>(st.e.xy + wall.xy * cell, wall.zw);
        if mirror {
            x = x - 2.0 * dot(x - wall.xy, wall.zw) * wall.zw;
        }
    }
    let world = st.e.xy + x * cell;
    out.world = world + corner * r;
    out.pos = to_clip(out.world);
    if mirror && out.face.z == 0.0 && out.face.w == 0.0 {
        out.pos = vec4<f32>(2.0, 2.0, 2.0, 1.0);
    }
    out.at = corner;
    var c = vec3<f32>(drops[at + 8u], drops[at + 9u], drops[at + 10u]);
    if st.f.w > 0.5 {
        c = st.g.rgb;
    }
    out.colour = select(vec3<f32>(0.0), c, c == c);
    // How fast it moves, and how much it foams: from its speed and from being splashed.
    let v = vec2<f32>(drops[at + 2u], drops[at + 3u]) * st.e.z;
    let speed = select(0.0, length(v), v.x == v.x && v.y == v.y);
    let kick = clamp(select(0.0, drops[at + 15u], drops[at + 15u] == drops[at + 15u]), 0.0, 1.0);
    var foam = 0.0;
    if st.m.x > 0.0 {
        let least = max(st.m.y, 1.0);
        foam = clamp(smoothstep(least, 2.0 * least, speed) + st.m.z * kick, 0.0, 1.0);
    }
    out.more = vec3<f32>(foam, min(speed, 1e5) / 1000.0, kick);
    // Alone (the volume round it, 1 / j, low: spray, a thin stream), it reaches the level by
    // itself: its peak is 1.6 times the level (st.a.x), so it draws as a droplet about half its
    // reach across. Inside the liquid (about 1) and at its surface (about 0.5) nothing changes.
    let j = drops[at + 12u];
    let vd = select(1.0, 1.0 / max(j, 1e-3), j == j && j > 0.0);
    let alone = 1.0 - smoothstep(0.25, 0.45, vd);
    out.boost = mix(1.0, max(1.6 * st.a.x, 1.0), alone);
    // Another liquid's drop: nowhere.
    let kind = bitcast<u32>(drops[at + 13u]) & 255u;
    if st.f.z >= 0.0 && kind != u32(st.f.z + 0.5) {
        out.pos = vec4<f32>(2.0, 2.0, 2.0, 1.0);
    }
    return out;
}

struct Fields {
    @location(0) field: vec4<f32>,
    @location(1) weights: vec4<f32>,
};

@fragment
fn fs_drop(in: DropOut) -> Fields {
    let d2 = dot(in.at, in.at);
    if d2 >= 1.0 {
        discard;
    }
    let k = 1.0 - d2;
    var keep = 1.0;
    // Cut at the wall's face, softly over a pixel (so the edge along it is smooth).
    if in.face.z != 0.0 || in.face.w != 0.0 {
        let past = dot(in.world - in.face.xy, in.face.zw) / max(g.view.z, 1e-4);
        keep = clamp(past + 0.5, 0.0, 1.0);
        if keep <= 0.0 {
            discard;
        }
    }
    let w = k * k * k * in.boost * keep;
    // Sharper swirls: the colour counts mostly near each drop's middle (a little all over, so the
    // edge between drops keeps a colour in half floats).
    let cw = (pow(k, 3.0 + 21.0 * clamp(st.f.x, 0.0, 1.0)) + 0.002 * k * k * k) * in.boost * keep;
    var out: Fields;
    out.field = vec4<f32>(in.colour * cw, w);
    out.weights = vec4<f32>(cw, w * in.more.x, w * in.more.y, w * in.more.z);
    return out;
}

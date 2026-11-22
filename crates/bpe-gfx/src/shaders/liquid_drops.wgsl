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
    // w: Streaks (see liquid.wgsl)
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
const DROP_FLOATS: u32 = 28u;

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var<uniform> st: Style;
// The drops, DROP_FLOATS numbers each (liquid_sim.wgsl's `Drop`): x, v, c, colour, j, kind, serial, ...
@group(1) @binding(1) var<storage, read> drops: array<f32>;
// The body's wet grid (liquid_wet.wgsl): six numbers a node, of which the drops read the last two
// (each node's signed distance from its wall's line, and its band).
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

// Liquid look (walls and streams): grid node (x, y)'s signed distance from its wall's line (grid
// units, below 0 inside the outline) and its band (0 off the walls band, 1 a filled wall, 2 + its
// half thickness a hollow one). See liquid_wet.wgsl.
fn wall_node(x: i32, y: i32) -> vec2<f32> {
    let cols = i32(st.v.x);
    let rows = i32(st.v.y);
    if cols < 1 || x < 0 || y < 0 || x >= cols || y >= rows {
        return vec2<f32>(0.0);
    }
    let cells = u32(cols * rows);
    let n = u32(y * cols + x);
    return vec2<f32>(wet_grid[4u * cells + n], wet_grid[5u * cells + n]);
}

// The wall at grid point `q` (between its four nodes): x the signed distance from its line, y its
// half thickness (below 0 for a filled wall), z 1 when all four nodes are in the walls band and
// are all filled or all hollow (else 0: no wall is known here).
fn wall_at(q: vec2<f32>) -> vec3<f32> {
    let i = floor(q);
    let f = q - i;
    let x = i32(i.x);
    let y = i32(i.y);
    let a = wall_node(x, y);
    let b = wall_node(x + 1, y);
    let c = wall_node(x, y + 1);
    let d = wall_node(x + 1, y + 1);
    let least = min(min(a.y, b.y), min(c.y, d.y));
    let most = max(max(a.y, b.y), max(c.y, d.y));
    if least < 0.5 || (least < 1.5) != (most < 1.5) {
        return vec3<f32>(0.0);
    }
    let v = mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
    return vec3<f32>(v.x, select(v.y - 2.0, -1.0, most < 1.5), 1.0);
}

// How far a point is from the face of wall `w` (`wall_at`) on its own side (grid units, below 0 in
// the wall): a filled wall's liquid is outside its outline, a hollow one's on either side of it.
fn face_of(w: vec3<f32>) -> f32 {
    return select(abs(w.x) - w.y, w.x, w.y < 0.0);
}

// Where drops' mirror images are mirrored: this far (grid units) off a wall's face, on the drops'
// side. Drops stop half a cell off the face and rest half a cell apart, so mirrored a quarter cell
// off it they and their images are evenly spaced and the field is whole through the face (it is
// the plane the physics mirrors them in, liquid_sim.wgsl's WALL_GAP). Mirrored in the face itself
// a row was missing, and the field at the wall was barely over the Edge level: a scalloped, pale
// edge along every wall.
const MIRROR_OFF: f32 = 0.25;

// The wall face a drop at grid point `q` is near (within `reach` cells of where its mirror image's
// splat still reaches its side): xy a point on the face, zw its normal towards the drop (grid
// units); zw 0 when there is none. The drop's side is its own (the sign of its distance from the
// wall's line), so nothing has to know where the liquid is. The normal is the slope of the
// distance between the drop's own four nodes, so a drop needs only those in the walls band.
fn wall_face(q: vec2<f32>, reach: f32) -> vec4<f32> {
    let w = wall_at(q);
    if w.z < 0.5 {
        return vec4<f32>(0.0);
    }
    let face = face_of(w);
    if face < -0.25 || face > reach + 2.0 * MIRROR_OFF {
        return vec4<f32>(0.0);
    }
    let i = floor(q);
    let f = q - i;
    let x = i32(i.x);
    let y = i32(i.y);
    let a = wall_node(x, y).x;
    let b = wall_node(x + 1, y).x;
    let c = wall_node(x, y + 1).x;
    let d = wall_node(x + 1, y + 1).x;
    let grad = vec2<f32>(mix(b - a, d - c, f.y), mix(c - a, d - b, f.x));
    let len = length(grad);
    if len < 0.5 || len > 1.5 {
        return vec4<f32>(0.0);
    }
    let side = select(select(-1.0, 1.0, w.x >= 0.0), 1.0, w.y < 0.0);
    let n = grad / len * side;
    return vec4<f32>(q - n * face, n);
}

struct DropOut {
    @builtin(position) pos: vec4<f32>,
    // Where in the drop, its reach 1.
    @location(0) at: vec2<f32>,
    @location(1) colour: vec3<f32>,
    // Liquid part 3: its foam (0 to 1), speed (world units a second / 1000) and kick.
    @location(2) more: vec3<f32>,
    // How much stronger its splat is: liquid thinner than the Edge level would never reach it on
    // its own, so it is lifted (a lone drop draws as a small droplet).
    @location(3) boost: f32,
    // Liquid fixes part B: where this pixel is (world), and the wall face near the drop (a point
    // on it and its normal towards the drop, world; the normal 0 when no wall is near). A drop and
    // its mirror image are splatted up to SPILL pixels past the face, so the field on the drop's
    // side is what it would be if the liquid carried on behind the wall; the surface pass cuts it
    // at the face (liquid.wgsl, `wall_cover`).
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

// How far past a wall's face a drop and its mirror image are still splatted (picture pixels): the
// surface pass reads the field a pixel either side for its slope and 3 either side for Mixing shows.
const SPILL: f32 = 3.0;
// Liquid look (walls and streams): liquid thinner than this share of full is lifted to SPARSE_TO of
// full (so it is over the Edge level whatever its density), fading out by SPARSE_END. The Edge
// level is half of full by default, and a flat surface's own drops read about half, so the lift
// must be over by there: with these the lifted field is never under 0.53 of full.
const SPARSE: f32 = 0.38;
const SPARSE_END: f32 = 0.54;
const SPARSE_TO: f32 = 0.9;
// Streaks: a drop with less than THIN_END of full liquid round it (wholly so under THIN) is drawn
// stretched along its way, by at most MOST_STRETCH times, and narrowed by that to the power
// -NARROWS.
const THIN: f32 = 0.35;
const THIN_END: f32 = 0.65;
const MOST_STRETCH: f32 = 6.0;
const NARROWS: f32 = 0.4;

fn drop_out(vi: u32, ii: u32, mirror: bool) -> DropOut {
    var out: DropOut;
    let at = ii * DROP_FLOATS;
    var x = vec2<f32>(drops[at], drops[at + 1u]);
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u)) * 2.0 - 1.0;
    let r = max(st.e.w, 0.0);
    let cell = max(st.e.z, 1e-6);
    let wall = wall_face(x, r / cell);
    // How fast it moves (world units a second).
    var v = vec2<f32>(drops[at + 2u], drops[at + 3u]) * st.e.z;
    v = select(vec2<f32>(0.0), v, v.x == v.x && v.y == v.y);
    out.face = vec4<f32>(0.0);
    if wall.z != 0.0 || wall.w != 0.0 {
        out.face = vec4<f32>(st.e.xy + wall.xy * cell, wall.zw);
        if mirror {
            x = x - 2.0 * max(dot(x - wall.xy, wall.zw) - MIRROR_OFF, 0.0) * wall.zw;
            v = v - 2.0 * dot(v, wall.zw) * wall.zw;
        }
    }
    let speed = min(length(v), 1e5);
    // Alone (the volume round it, 1 / j, low: spray, a thin stream), it reaches the level by
    // itself: a lone drop's peak is 1.6 times the level (st.a.x), so it draws as a droplet about
    // half its reach across, and liquid between "alone" and "enough to reach the level" is lifted
    // to most of full. Inside the liquid (about 1) and at its surface (about 0.5) nothing changes.
    let j = drops[at + 12u];
    let vd = select(1.0, 1.0 / max(j, 1e-3), j == j && j > 0.0);
    let sparse = 1.0 - smoothstep(SPARSE, SPARSE_END, vd);
    out.boost = mix(1.0, clamp(SPARSE_TO / max(vd, 1e-3), 1.0, max(1.6 * st.a.x, 1.0)), sparse);
    // Streaks: thin liquid is smeared along its way by how far it goes in the shutter's time
    // (Streaks / 120 s) against a droplet's width, and narrowed a little, as a camera sees a
    // falling stream. Liquid at rest and the inside of a body are round, as ever.
    let thin = 1.0 - smoothstep(THIN, THIN_END, vd);
    let across = max(0.76 * r, 1e-6);
    var stretch = 1.0;
    var way = vec2<f32>(1.0, 0.0);
    if speed > 1e-3 && st.c.w > 0.0 {
        stretch = 1.0 + thin * min(speed * st.c.w / 120.0 / across, MOST_STRETCH - 1.0);
        way = v / length(v);
    }
    let narrow = pow(stretch, -NARROWS);
    let world = st.e.xy + x * cell;
    out.world = world + way * (corner.x * r * stretch) + vec2<f32>(-way.y, way.x) * (corner.y * r * narrow);
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
    // How much it foams: from its speed and from being splashed.
    let kick = clamp(select(0.0, drops[at + 15u], drops[at + 15u] == drops[at + 15u]), 0.0, 1.0);
    var foam = 0.0;
    if st.m.x > 0.0 {
        let least = max(st.m.y, 1.0);
        foam = clamp(smoothstep(least, 2.0 * least, speed) + st.m.z * kick, 0.0, 1.0);
    }
    out.more = vec3<f32>(foam, speed / 1000.0, kick);
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
    // Kept up to SPILL pixels past the wall's face (softly over the last one): the surface pass
    // makes the cut at the face itself.
    if in.face.z != 0.0 || in.face.w != 0.0 {
        let past = dot(in.world - in.face.xy, in.face.zw) / max(g.view.z, 1e-4);
        keep = clamp(past + SPILL + 0.5, 0.0, 1.0);
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

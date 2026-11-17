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
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var<uniform> st: Style;
// The drops, 24 numbers each (liquid_sim.wgsl's `Drop`): x, v, c, colour, j, kind, serial, ...
@group(1) @binding(1) var<storage, read> drops: array<f32>;

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

struct DropOut {
    @builtin(position) pos: vec4<f32>,
    // Where in the drop, its reach 1.
    @location(0) at: vec2<f32>,
    @location(1) colour: vec3<f32>,
};

@vertex
fn vs_drop(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> DropOut {
    var out: DropOut;
    let at = ii * 24u;
    let x = vec2<f32>(drops[at], drops[at + 1u]);
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u)) * 2.0 - 1.0;
    let r = max(st.e.w, 0.0);
    let world = st.e.xy + x * st.e.z;
    out.pos = to_clip(world + corner * r);
    out.at = corner;
    var c = vec3<f32>(drops[at + 8u], drops[at + 9u], drops[at + 10u]);
    if st.f.w > 0.5 {
        c = st.g.rgb;
    }
    out.colour = select(vec3<f32>(0.0), c, c == c);
    // Another liquid's drop: nowhere.
    let kind = bitcast<u32>(drops[at + 13u]) & 255u;
    if st.f.z >= 0.0 && kind != u32(st.f.z + 0.5) {
        out.pos = vec4<f32>(2.0, 2.0, 2.0, 1.0);
    }
    return out;
}

struct Fields {
    @location(0) field: vec4<f32>,
    @location(1) weights: f32,
};

@fragment
fn fs_drop(in: DropOut) -> Fields {
    let d2 = dot(in.at, in.at);
    if d2 >= 1.0 {
        discard;
    }
    let k = 1.0 - d2;
    let w = k * k * k;
    // Sharper swirls: the colour counts mostly near each drop's middle (a little all over, so the
    // edge between drops keeps a colour in half floats).
    let cw = pow(k, 3.0 + 21.0 * clamp(st.f.x, 0.0, 1.0)) + 0.002 * w;
    var out: Fields;
    out.field = vec4<f32>(in.colour * cw, w);
    out.weights = cw;
    return out;
}

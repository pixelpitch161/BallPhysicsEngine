// Liquid part 3: a Liquid thing's drops drawn one by one, after its surface: **Things inside**
// (a share of the drops, chosen by their birth number, drawn as glinting flecks where the
// surface is) and **Mist** (every drop outside the surface, drawn as a fine soft dot: spray).
// Read straight from the card's buffer, as liquid_drops.wgsl does. See liquid.rs.

struct Globals {
    view: vec4<f32>,
    bg_paint: vec4<f32>,
    bg_place: vec4<f32>,
    post: vec4<f32>,
    camera: vec4<f32>,
    bg: vec4<f32>,
    bg_from: vec4<f32>,
    bg_to: vec4<f32>,
    light: vec4<f32>,
};

// liquid.wgsl's Style (its fields are documented there).
struct Style {
    a: vec4<f32>,
    b: vec4<f32>,
    c: vec4<f32>,
    sky_top: vec4<f32>,
    sky_bottom: vec4<f32>,
    d: vec4<f32>,
    e: vec4<f32>,
    f: vec4<f32>,
    g: vec4<f32>,
    h: vec4<f32>,
    i: vec4<f32>,
    j: vec4<f32>,
    k: vec4<f32>,
    l: vec4<f32>,
    m: vec4<f32>,
    n: vec4<f32>,
    o: vec4<f32>,
    p: vec4<f32>,
    // Caustics: speed, deep; Things inside: share, size (world units).
    q: vec4<f32>,
    // Things inside: one colour, 1 to use it.
    r: vec4<f32>,
    // Things inside: sparkle; Mist: amount, size (world units).
    s: vec4<f32>,
};

const DROP_FLOATS: u32 = 24u;

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var<uniform> st: Style;
@group(1) @binding(1) var<storage, read> drops: array<f32>;
@group(1) @binding(2) var field: texture_2d<f32>;

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

fn pcg(v: u32) -> u32 {
    let s = v * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

fn unit_of(v: u32) -> f32 {
    return f32(pcg(v) >> 8u) / 16777216.0;
}

struct DotOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) at: vec2<f32>,
    // rgb, how much it covers (0 to 1)
    @location(1) colour: vec4<f32>,
};

@vertex
fn vs_dot(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> DotOut {
    var out: DotOut;
    out.pos = vec4<f32>(2.0, 2.0, 2.0, 1.0);
    out.at = vec2<f32>(0.0);
    out.colour = vec4<f32>(0.0);
    let at = ii * DROP_FLOATS;
    let kind = bitcast<u32>(drops[at + 13u]) & 255u;
    if st.f.z >= 0.0 && kind != u32(st.f.z + 0.5) {
        return out;
    }
    let world = st.e.xy + vec2<f32>(drops[at], drops[at + 1u]) * st.e.z;
    if !(abs(world.x) < 1e9 && abs(world.y) < 1e9) {
        return out;
    }
    let centre = to_clip(world);
    let dims = vec2<i32>(textureDimensions(field));
    let px = vec2<i32>(vec2<f32>((centre.x + 1.0) * 0.5, (1.0 - centre.y) * 0.5) * vec2<f32>(dims));
    if px.x < 0 || px.y < 0 || px.x >= dims.x || px.y >= dims.y {
        return out;
    }
    let inside = textureLoad(field, px, 0).a >= st.a.x;
    var own = vec3<f32>(drops[at + 8u], drops[at + 9u], drops[at + 10u]);
    if st.f.w > 0.5 {
        own = st.g.rgb;
    }
    own = select(vec3<f32>(0.0), own, own == own);
    let serial = bitcast<u32>(drops[at + 14u]);
    var size = 0.0;
    if inside && st.q.z > 0.0 && unit_of(serial ^ 0x9e3779b9u) < st.q.z {
        // A fleck: its colour, glinting now and then.
        size = st.q.w;
        var c = select(own, st.r.rgb, st.r.w > 0.5);
        let blink = unit_of(serial ^ pcg(u32(max(g.view.w, 0.0) * 8.0)));
        c *= 1.0 + clamp(st.s.x, 0.0, 1.0) * pow(blink, 6.0) * 6.0;
        out.colour = vec4<f32>(c, 1.0);
    } else if !inside && st.s.y > 0.0 {
        // Mist: spray outside the surface.
        size = st.s.z;
        out.colour = vec4<f32>(own, clamp(st.s.y, 0.0, 1.0));
    }
    if size <= 0.0 {
        return out;
    }
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u)) * 2.0 - 1.0;
    // At least half a picture pixel across, so it never flickers out.
    let r = max(size, 0.75 * g.view.z);
    out.pos = to_clip(world + corner * r);
    out.at = corner;
    return out;
}

const PASS_PICTURE: i32 = 0;
const PASS_LIT: i32 = 1;

@fragment
fn fs_dot(in: DotOut) -> @location(0) vec4<f32> {
    let d = length(in.at);
    let a = smoothstep(1.0, 0.35, d) * in.colour.a;
    if a <= 0.0 {
        discard;
    }
    let mode = i32(round(g.light.x));
    if mode == PASS_LIT && st.d.x > 0.0 {
        return vec4<f32>(in.colour.rgb * st.d.x * a, a);
    }
    if mode > PASS_PICTURE {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(in.colour.rgb * a, a);
}

// The look drawing (Water mode part 6; docs/superpowers/plans/water-mode/part6-light-and-looks.md):
// lamps, shadows, light through the liquid and caustics, glow and bounced light, the back, the
// solids' materials and the liquid's look, drawn in linear light for bpe-gfx's finish (effects,
// bloom, grade). Compiled after plain.wgsl into one module: plain's uniform `u`, its drops, its
// solid field and its depth ladder are read as plain reads them, and its functions are called.
//
// 1. `vs_look_splat`/`fs_look_splat` (and `fs_look_weights` past four liquids): every drop a splat
//    of its liquid's own size (Smoothing), stretched along its speed (Streaks), summed into the look
//    pictures: speed, air, the speed's way, and each liquid's share.
// 2. Plain's depth ladder (its passes, run by `Plain::record_ladder`).
// 3. `vs_look_nodes`/`fs_look_nodes`: the splats summed at the layout's nodes again, with values:
//    how fast the liquid takes light away (by colour), the light it gives off, its colour; then
//    `fs_half`, each a mip level of 2 x 2 averages.
// 4. `fs_light`: the light map, on a grid fixed to the course: each lamp's light at the texel,
//    marched back to the lamp through the solid field (soft shadows, glass tints) and the liquid
//    (its colour taken away, caustics from its surface's curve). Once for the shot, once over the
//    whole course at the spread light's texel (lighting the faces, for bounced light).
// 5. `fs_spread_in`, `fs_spread_down`, `fs_spread_gather`: the spread light over the whole course:
//    what glows and what bounces lamp light, with its mips, gathered at each texel by cones all round
//    marched through the solid field (cone tracing: wider cones read coarser mips).
// 6. `fs_look`: the picture, back to front: the back (its contact shadow, paper's cut-out shadow),
//    the liquid (see-through over the bent back, its surface, glow, bands, outline), shafts, the
//    solids (lit at their face, rounded by Depth, their outlines), in linear light.
//
// Everything is a function of the drops and the cards at one moment: no random numbers, no
// atomics, nothing kept from the picture before, and every world-space grid is fixed to the
// course, so the light at a world point doesn't depend on the view.

struct LiquidL {
    // linear Colour, Clarity (world px)
    colour: vec4<f32>,
    // linear Deep colour, Solid
    deep: vec4<f32>,
    // linear Glow colour times Gives off light, Glow from (0 Always, 1 Speed)
    glow: vec4<f32>,
    // linear Outline colour, Outline (world px)
    edge: vec4<f32>,
    // Shine, Shine size, Edge sheen, Thin film
    surface: vec4<f32>,
    // Bend (world px), Rainbow split, Ripples, Colour from (0 its own, 1 speed, 2 depth)
    body: vec4<f32>,
    // its splat's radius (picture px), its peak weight, Streaks, Bands
    shape: vec4<f32>,
    // Band softness, its splat's radius (cells), its light taken away per world px (largest), 0
    more: vec4<f32>,
};

struct MaterialL {
    // linear Colour, kind (0 Rock, 1 Glass, 2 Paper, 3 Basalt, 4 Flat)
    colour: vec4<f32>,
    // linear Second colour, Texture size (world px)
    second: vec4<f32>,
    // Depth (world px), Roughness, Outline (world px), Wet marks
    shape: vec4<f32>,
    // linear Outline colour, 0
    edge: vec4<f32>,
};

struct LampL {
    // world place, kind (0 Spot, 1 Strip, 2 Sun), Shadows (0 or 1)
    place: vec4<f32>,
    // linear Colour times Brightness, Softness
    colour: vec4<f32>,
    // the way it shines (unit), half its Spread (radians; π or more: all round), Length (world px)
    aim: vec4<f32>,
};

struct LU {
    // Ambient, Bounce, Caustics, Shafts
    light: vec4<f32>,
    // how many liquids, lamps, the back's kind (0 Darker rock, 1 Colour, 2 Gradient, 3 Picture),
    // whether the spread light is drawn
    counts: vec4<u32>,
    // linear back Colour, Darker
    back: vec4<f32>,
    // the gradient's way (unit), its length across the course (world px), 0
    back_way: vec4<f32>,
    // the course's world box: lo, hi
    course: vec4<f32>,
    // the back picture's box in the world: lo, size
    picture: vec4<f32>,
    // the shot's light map: its lo (world), its texel (world px), whether it is drawn
    shot: vec4<f32>,
    // the spread light: its lo (world), its base texel (world px), its levels
    spread: vec4<f32>,
    // water seconds, the largest Ripples, the paper's largest Depth (world px), 0
    time: vec4<f32>,
    liquids: array<LiquidL, 16>,
    lamps: array<LampL, 16>,
    // by owner: 0 the rock, 1 to 64 the pieces and walls
    materials: array<MaterialL, 65>,
};

// One light map's box: its lo (world), its texel (world px), its kind (0 the shot's, 1 the faces'
// for bounced light); its size in texels.
struct LB {
    lo: vec4<f32>,
    size: vec4<u32>,
};

@group(0) @binding(10) var<uniform> lu: LU;
// The look pictures: (Σ k |v|, Σ k |v|², Σ k air, Σ k) and (Σ k vx, Σ k vy, Σ k lone, 0), speeds in
// SPEED_UNIT; each liquid's Σ k in channel n % 4 of wt[n / 4].
@group(0) @binding(11) var look0: texture_2d<f32>;
@group(0) @binding(12) var look1: texture_2d<f32>;
@group(0) @binding(13) var wt0: texture_2d<f32>;
@group(0) @binding(14) var wt1: texture_2d<f32>;
@group(0) @binding(15) var wt2: texture_2d<f32>;
@group(0) @binding(16) var wt3: texture_2d<f32>;
// Gradients: row n (64 linear samples) for liquid n, row 16 the back's.
@group(0) @binding(17) var<storage, read> grads: array<vec4<f32>>;
// The node fields with their mips: (Σ k σ rgb, Σ k), (Σ k glow rgb, Σ k), (Σ k colour rgb, Σ k).
@group(0) @binding(18) var dens: texture_2d<f32>;
@group(0) @binding(19) var glows: texture_2d<f32>;
@group(0) @binding(20) var tints: texture_2d<f32>;
// `fs_half`'s sources: one level of each.
@group(0) @binding(21) var half_d: texture_2d<f32>;
@group(0) @binding(22) var half_g: texture_2d<f32>;
@group(0) @binding(23) var half_c: texture_2d<f32>;
// The shot's light map: (light rgb, 1 where its texel is open) and (Σ w way, Σ w, 0).
@group(0) @binding(24) var lmap: texture_2d<f32>;
@group(0) @binding(25) var ldir: texture_2d<f32>;
@group(0) @binding(26) var<uniform> lb: LB;
// The spread light (rgb times its texel's openness, openness), at the base texel.
@group(0) @binding(27) var spread_s: texture_2d<f32>;
@group(0) @binding(28) var backpic: texture_2d<f32>;
// What the course gives off (rgb) and how much liquid covers it, with its mips.
@group(0) @binding(29) var spread_eo: texture_2d<f32>;
// The mip pass's source level.
@group(0) @binding(30) var spread_src: texture_2d<f32>;
// The faces' light map, at the spread light's base texel over the whole course.
@group(0) @binding(32) var bounce_map: texture_2d<f32>;
// The shot's light as if there were no liquid.
@group(0) @binding(33) var lfree: texture_2d<f32>;

// Speeds go into the half floats in thousands of world px a second.
const SPEED_UNIT: f32 = 1000.0;
// A 2D lamp's gentle falloff: half its light this far away.
const REACH: f32 = 600.0;
// The most steps a march takes to a lamp.
const MOST_STEPS: i32 = 160;
// The tank's depth into the screen (world px): clear liquid shows what is behind it through this.
const TANK: f32 = 100.0;
// The surface rounds over this far in from its outline (world px).
const ROUND: f32 = 6.0;
// Water's index of refraction (caustics).
const ETA: f32 = 1.33;
// How far from a solid's face the field's distances hold (world px; `SolidField::BAND`, less a
// little): a shadow's soft edge reaches no further.
const SHADOW_REACH: f32 = 44.0;
// The nearest a caustic's focusing reads 1 / |1 - t / f| at: its brightest is 1 over it.
const CAUSTIC_NEAREST: f32 = 1.0 / 6.0;
// The liquid's surface in the coarse amount (the node fields' level 1).
const COARSE_LEVEL: f32 = 0.5;
// How far the back's contact shadow reaches from a solid (world px) and how dark it gets.
const AO_REACH: f32 = 24.0;
const AO_DARK: f32 = 0.35;
// How dark paper's cut-out shadow is.
const PAPER_DARK: f32 = 0.45;
// The spread light: its gain (a face beside a wide glow, half the cones seeing it, is lit as the glow
// shines), its cones and
// the tangent of their half width, and the most steps a cone takes.
const SPREAD_GAIN: f32 = 2.0;
const CONES: i32 = 24;
const CONE_TAN: f32 = 0.1317;
const CONE_STEPS: i32 = 48;
// The spread pyramid's openness of glass.
const GLASS_OPEN: f32 = 0.7;
// The surface's bend and depth are read from the ladder at about this blur (world px).
const SMOOTH_SURFACE: f32 = 24.0;
// The least splat radius, picture px (plain's `LEAST_R`).
const LEAST_R_PX: f32 = 1.25;

// ---- noise, all a hash of the world point ----

fn hash_u(x: u32) -> u32 {
    var h = x;
    h ^= h >> 16u;
    h *= 0x7feb352du;
    h ^= h >> 15u;
    h *= 0x846ca68bu;
    h ^= h >> 16u;
    return h;
}

fn hash2(c: vec2<i32>) -> f32 {
    let h = hash_u(bitcast<u32>(c.x) * 0x9e3779b9u ^ hash_u(bitcast<u32>(c.y) + 0x632be5abu));
    return f32(h >> 8u) / 16777216.0;
}

fn vnoise(q: vec2<f32>) -> f32 {
    let i = vec2<i32>(floor(q));
    let f = q - floor(q);
    let s = f * f * (3.0 - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + vec2<i32>(1, 0));
    let c = hash2(i + vec2<i32>(0, 1));
    let d = hash2(i + vec2<i32>(1, 1));
    return mix(mix(a, b, s.x), mix(c, d, s.x), s.y);
}

fn fbm(q: vec2<f32>) -> f32 {
    var sum = 0.0;
    var a = 0.5;
    var p = q;
    for (var k = 0; k < 4; k++) {
        sum += a * vnoise(p);
        p = p * 2.03 + vec2<f32>(17.1, 3.7);
        a *= 0.5;
    }
    return sum / 0.9375;
}

// How near a cell edge of a hashed grid (the second nearest point less the nearest, cells).
fn cell_edge(q: vec2<f32>) -> f32 {
    let i = vec2<i32>(floor(q));
    var f1 = 9.0;
    var f2 = 9.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let c = i + vec2<i32>(x, y);
            let at = vec2<f32>(c) + vec2<f32>(hash2(c), hash2(c + vec2<i32>(91, 37)));
            let d = length(q - at);
            if d < f1 {
                f2 = f1;
                f1 = d;
            } else if d < f2 {
                f2 = d;
            }
        }
    }
    return f2 - f1;
}

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// ---- reading textures texel by texel ----

// Level `k`'s size, from level 0's (a size asked for at a level that differs from pixel to pixel
// comes back wrong on the cloud's software card).
fn size_at(t: texture_2d<f32>, k: i32) -> vec2<i32> {
    return max(vec2<i32>(textureDimensions(t, 0)) >> vec2<u32>(u32(k)), vec2<i32>(1));
}

fn load0(t: texture_2d<f32>, at: vec2<i32>, k: i32) -> vec4<f32> {
    let size = size_at(t, k);
    if any(at < vec2<i32>(0)) || any(at >= size) {
        return vec4<f32>(0.0);
    }
    return textureLoad(t, at, k);
}

fn load_clamped(t: texture_2d<f32>, at: vec2<i32>, k: i32) -> vec4<f32> {
    let size = size_at(t, k);
    return textureLoad(t, clamp(at, vec2<i32>(0), size - 1), k);
}

// Level `k` of `t` between its texels at texel point `t_at` (texel i's middle at i), 0 past it.
fn bilinear0(t: texture_2d<f32>, t_at: vec2<f32>, k: i32) -> vec4<f32> {
    let b = vec2<i32>(floor(t_at));
    let f = t_at - floor(t_at);
    let top = mix(load0(t, b, k), load0(t, b + vec2<i32>(1, 0), k), f.x);
    let bottom = mix(load0(t, b + vec2<i32>(0, 1), k), load0(t, b + vec2<i32>(1, 1), k), f.x);
    return mix(top, bottom, f.y);
}

fn bilinear_clamped(t: texture_2d<f32>, t_at: vec2<f32>, k: i32) -> vec4<f32> {
    let b = vec2<i32>(floor(t_at));
    let f = t_at - floor(t_at);
    let top = mix(load_clamped(t, b, k), load_clamped(t, b + vec2<i32>(1, 0), k), f.x);
    let bottom = mix(load_clamped(t, b + vec2<i32>(0, 1), k), load_clamped(t, b + vec2<i32>(1, 1), k), f.x);
    return mix(top, bottom, f.y);
}

// A node field's level `k` at world point `p`: level k's texel i covers nodes i 2^k to
// (i + 1) 2^k - 1, so its middle is at node (i + 1/2) 2^k - 1/2.
fn node_t(p: vec2<f32>, k: i32) -> vec2<f32> {
    let g = (p - u.grid.xy) / u.grid.z;
    return (g + 0.5) / exp2(f32(k)) - 0.5;
}

fn density_at(p: vec2<f32>, k: i32) -> vec4<f32> {
    return bilinear0(dens, node_t(p, k), k);
}

// Level `k`'s share of a read between levels at `level`. A level that differs from pixel to pixel is
// read as a loop over the level numbers with each level's share, never as one read at a level
// worked out per pixel: some cards (the cloud's software one) take one level for a whole block of
// pixels, sampled or loaded (`docs/notes/water.md`, part 2b).
fn level_share(level: f32, k: i32) -> f32 {
    return max(0.0, 1.0 - abs(level - f32(k)));
}

// The field's outward way at world point `p` (unit; up where it is flat).
fn field_normal(p: vec2<f32>) -> vec2<f32> {
    let h = u.grid.z;
    let g = vec2<f32>(
        field_at(p + vec2<f32>(h, 0.0)) - field_at(p - vec2<f32>(h, 0.0)),
        field_at(p + vec2<f32>(0.0, h)) - field_at(p - vec2<f32>(0.0, h)),
    );
    let l = length(g);
    if l < 1e-6 {
        return vec2<f32>(0.0, -1.0);
    }
    return g / l;
}

// The owner of the solid at world point `p`: of the four nodes round it inside a solid, the one
// with the most weight (none inside: the nearest node's).
fn owner_at(p: vec2<f32>) -> u32 {
    let cols = u.size.z;
    let rows = u.size.w;
    let g = clamp((p - u.grid.xy) / u.grid.z, vec2<f32>(0.0), vec2<f32>(f32(cols) - 1.001, f32(rows) - 1.001));
    let x = u32(floor(g.x));
    let y = u32(floor(g.y));
    let f = g - vec2<f32>(f32(x), f32(y));
    let i = y * cols + x;
    let at = array<u32, 4>(i, i + 1u, i + cols, i + cols + 1u);
    let w = array<f32, 4>((1.0 - f.x) * (1.0 - f.y), f.x * (1.0 - f.y), (1.0 - f.x) * f.y, f.x * f.y);
    var best = -1.0;
    var nearest = -1.0;
    var owner = 0u;
    var near_owner = 0u;
    for (var k = 0; k < 4; k++) {
        let o = u32(max(field[cols * rows + at[k]], 0.0) + 0.5);
        if field[at[k]] < 0.0 && w[k] > best {
            best = w[k];
            owner = o;
        }
        if w[k] > nearest {
            nearest = w[k];
            near_owner = o;
        }
    }
    let o = select(near_owner, owner, best >= 0.0);
    return select(0u, o, o <= 64u);
}

fn material_kind(m: u32) -> u32 {
    return u32(lu.materials[m].colour.w + 0.5);
}

// ---- 1. the look splats ----

struct LookSplatOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) off: vec2<f32>,
    // its peak, whether it is air
    @location(1) @interpolate(flat) peak: vec2<f32>,
    // its speed (SPEED_UNIT): vx, vy, |v|; its liquid
    @location(2) @interpolate(flat) v: vec4<f32>,
};

@vertex
fn vs_look_splat(@builtin(vertex_index) v: u32, @builtin(instance_index) i: u32) -> LookSplatOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
    );
    let c = corners[v];
    var out: LookSplatOut;
    out.off = c;
    let g = place(i);
    if !alive(g) {
        out.pos = vec4<f32>(-2.0, -2.0, 0.0, 1.0);
        out.peak = vec2<f32>(0.0);
        out.v = vec4<f32>(0.0);
        return out;
    }
    let kind = ids[i].x;
    let air = (kind & AIR) != 0u;
    let n = min(kind & 15u, max(lu.counts.x, 1u) - 1u);
    let l = lu.liquids[n];
    let vel = xv[i].zw * u.grid.z;
    let speed = length(vel);
    let r = l.shape.x;
    // Streaks: stretched along its speed by half a 1/60 s frame's travel, at most 4 cells.
    let stretch = select(min(l.shape.z * speed / 120.0, 4.0 * u.grid.z) * u.view.z, 0.0, air);
    let along = select(vec2<f32>(1.0, 0.0), vel / max(speed, 1e-6), speed > 1e-6);
    let across = vec2<f32>(-along.y, along.x);
    let a = r + 0.5 * stretch;
    let world = u.grid.xy + g * u.grid.z;
    let p = (world - u.view.xy) * u.view.z + along * (c.x * a) + across * (c.y * r);
    let size = vec2<f32>(f32(u.size.x), f32(u.size.y));
    out.pos = vec4<f32>(p.x / size.x * 2.0 - 1.0, 1.0 - p.y / size.y * 2.0, 0.0, 1.0);
    out.peak = vec2<f32>(l.shape.y * r / a, select(0.0, 1.0, air));
    out.v = vec4<f32>(vel / SPEED_UNIT, speed / SPEED_UNIT, f32(n));
    return out;
}

struct LookSums {
    @location(0) a: vec4<f32>,
    @location(1) b: vec4<f32>,
    @location(2) w: vec4<f32>,
};

fn splat_k(off: vec2<f32>, peak: f32) -> f32 {
    let q2 = dot(off, off);
    if q2 >= 1.0 {
        return -1.0;
    }
    return peak * (1.0 - q2) * (1.0 - q2);
}

// The liquid's share in channel `n` % 4.
fn channel(n: u32, k: f32) -> vec4<f32> {
    return select(vec4<f32>(0.0), vec4<f32>(k), vec4<u32>(0u, 1u, 2u, 3u) == vec4<u32>(n % 4u));
}

@fragment
fn fs_look_splat(in: LookSplatOut) -> LookSums {
    let k = splat_k(in.off, in.peak.x);
    if k < 0.0 {
        discard;
    }
    var out: LookSums;
    if in.peak.y > 0.5 {
        out.a = vec4<f32>(0.0, 0.0, k, 0.0);
        out.b = vec4<f32>(0.0);
        out.w = vec4<f32>(0.0);
        return out;
    }
    let n = u32(in.v.w + 0.5);
    out.a = vec4<f32>(k * in.v.z, k * in.v.z * in.v.z, 0.0, k);
    out.b = vec4<f32>(k * in.v.xy, 0.0, 0.0);
    out.w = select(vec4<f32>(0.0), channel(n, k), n < 4u);
    return out;
}

struct WeightSums {
    @location(0) w1: vec4<f32>,
    @location(1) w2: vec4<f32>,
    @location(2) w3: vec4<f32>,
};

@fragment
fn fs_look_weights(in: LookSplatOut) -> WeightSums {
    let k = splat_k(in.off, in.peak.x);
    if k < 0.0 || in.peak.y > 0.5 {
        discard;
    }
    let n = u32(in.v.w + 0.5);
    var out: WeightSums;
    out.w1 = select(vec4<f32>(0.0), channel(n, k), n >= 4u && n < 8u);
    out.w2 = select(vec4<f32>(0.0), channel(n, k), n >= 8u && n < 12u);
    out.w3 = select(vec4<f32>(0.0), channel(n, k), n >= 12u);
    return out;
}

// ---- 3. the node fields ----

struct LookNodeOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) off: vec2<f32>,
    @location(1) @interpolate(flat) sigma: vec3<f32>,
    @location(2) @interpolate(flat) glow: vec3<f32>,
    @location(3) @interpolate(flat) colour: vec3<f32>,
};

// How fast liquid `l` takes light away, per world px, by colour: light through Clarity px of it keeps
// about a third of its own colour and less of the others; Solid stops it.
fn sigma_of(l: LiquidL) -> vec3<f32> {
    return (1.0 + 2.0 * (vec3<f32>(1.0) - clamp(l.colour.rgb, vec3<f32>(0.0), vec3<f32>(1.0))))
        * (1.0 + 20.0 * l.deep.w) / max(l.colour.w, 1.0);
}

// The light liquid `l` gives off at `speed` world px a second.
fn glow_of(l: LiquidL, speed: f32) -> vec3<f32> {
    let by_speed = smoothstep(0.0, 1500.0, speed);
    return l.glow.rgb * select(1.0, by_speed, l.glow.w > 0.5);
}

@vertex
fn vs_look_nodes(@builtin(vertex_index) v: u32, @builtin(instance_index) i: u32) -> LookNodeOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
    );
    let c = corners[v];
    var out: LookNodeOut;
    out.off = c;
    let g = place(i);
    if !drawn(i) {
        out.pos = vec4<f32>(-2.0, -2.0, 0.0, 1.0);
        out.sigma = vec3<f32>(0.0);
        out.glow = vec3<f32>(0.0);
        out.colour = vec3<f32>(0.0);
        return out;
    }
    let n = min(ids[i].x & 15u, max(lu.counts.x, 1u) - 1u);
    let l = lu.liquids[n];
    let t = g + 0.5 + c * u.depth.x;
    let size = vec2<f32>(f32(u.size.z), f32(u.size.w));
    out.pos = vec4<f32>(t.x / size.x * 2.0 - 1.0, 1.0 - t.y / size.y * 2.0, 0.0, 1.0);
    out.sigma = sigma_of(l);
    out.glow = glow_of(l, length(xv[i].zw) * u.grid.z);
    out.colour = l.colour.rgb;
    return out;
}

struct NodeSums {
    @location(0) d: vec4<f32>,
    @location(1) g: vec4<f32>,
    @location(2) c: vec4<f32>,
};

@fragment
fn fs_look_nodes(in: LookNodeOut) -> NodeSums {
    let k = splat_k(in.off, u.depth.y);
    if k < 0.0 {
        discard;
    }
    var out: NodeSums;
    out.d = vec4<f32>(in.sigma * k, k);
    out.g = vec4<f32>(in.glow * k, k);
    out.c = vec4<f32>(in.colour * k, k);
    return out;
}

@fragment
fn fs_half(in: FullOut) -> NodeSums {
    let b = 2 * vec2<i32>(floor(in.pos.xy));
    var out: NodeSums;
    out.d = vec4<f32>(0.0);
    out.g = vec4<f32>(0.0);
    out.c = vec4<f32>(0.0);
    for (var j = 0; j < 2; j++) {
        for (var i = 0; i < 2; i++) {
            let at = b + vec2<i32>(i, j);
            out.d += 0.25 * load0(half_d, at, 0);
            out.g += 0.25 * load0(half_g, at, 0);
            out.c += 0.25 * load0(half_c, at, 0);
        }
    }
    return out;
}

// ---- ripples ----

// The Ripples' height, slope and curve at world x, at water time `t` (world px): five travelling
// waves of fixed wavelengths, speeds from the water-wave relation, phases from a hash.
fn ripple(x: f32, t: f32) -> vec3<f32> {
    let lengths = array<f32, 5>(18.0, 27.0, 38.0, 52.0, 70.0);
    var out = vec3<f32>(0.0);
    for (var i = 0; i < 5; i++) {
        let k = 2.0 * PI / lengths[i];
        let w = sqrt(2000.0 * k) * select(1.0, -1.0, (i & 1) == 1);
        let phase = 2.0 * PI * hash2(vec2<i32>(i, 7));
        let a = 0.012 * lengths[i];
        let s = sin(k * x - w * t + phase);
        let c = cos(k * x - w * t + phase);
        out += vec3<f32>(a * s, a * k * c, -a * k * k * s);
    }
    return out;
}

// The ripples' curvature at `x` (as `ripple(x, t).z`), each wave's blurred over `w` world px: seen
// from deep under it, a lamp's size smooths away the small waves' caustics.
fn ripple_kappa(x: f32, t: f32, w: f32) -> f32 {
    let lengths = array<f32, 5>(18.0, 27.0, 38.0, 52.0, 70.0);
    var out = 0.0;
    for (var i = 0; i < 5; i++) {
        let k = 2.0 * PI / lengths[i];
        let wk = sqrt(2000.0 * k) * select(1.0, -1.0, (i & 1) == 1);
        let phase = 2.0 * PI * hash2(vec2<i32>(i, 7));
        let a = 0.012 * lengths[i];
        out += -a * k * k * sin(k * x - wk * t + phase) * exp(-0.5 * (k * w) * (k * w));
    }
    return out;
}

// ---- 4. the light map ----

struct LightOut {
    @location(0) l: vec4<f32>,
    @location(1) d: vec4<f32>,
    // the light as if there were no liquid (opaque liquid is lit on its front by it)
    @location(2) f: vec4<f32>,
};

fn glass_at(p: vec2<f32>) -> bool {
    return material_kind(owner_at(p)) == 1u;
}

// How far from `p` along `way` (unit) to the course's edge.
fn to_edge(p: vec2<f32>, way: vec2<f32>) -> f32 {
    let lo = lu.course.xy;
    let hi = lu.course.zw;
    var t = 1e9;
    if way.x > 1e-6 {
        t = min(t, (hi.x - p.x) / way.x);
    } else if way.x < -1e-6 {
        t = min(t, (lo.x - p.x) / way.x);
    }
    if way.y > 1e-6 {
        t = min(t, (hi.y - p.y) / way.y);
    } else if way.y < -1e-6 {
        t = min(t, (lo.y - p.y) / way.y);
    }
    return max(t, 0.0);
}

// The surface's curvature at `q` read from the amount's level `k` (the outward way's divergence:
// above 0 where the liquid bulges out), and its outward way (unit); zero where it is flat.
fn surface_bend(q: vec2<f32>, k: i32) -> vec3<f32> {
    let h = u.grid.z * exp2(f32(k));
    let a = density_at(q, k).a;
    let ax = (density_at(q + vec2<f32>(h, 0.0), k).a - density_at(q - vec2<f32>(h, 0.0), k).a) / (2.0 * h);
    let ay = (density_at(q + vec2<f32>(0.0, h), k).a - density_at(q - vec2<f32>(0.0, h), k).a) / (2.0 * h);
    let axx = (density_at(q + vec2<f32>(h, 0.0), k).a - 2.0 * a + density_at(q - vec2<f32>(h, 0.0), k).a) / (h * h);
    let ayy = (density_at(q + vec2<f32>(0.0, h), k).a - 2.0 * a + density_at(q - vec2<f32>(0.0, h), k).a) / (h * h);
    let axy = (density_at(q + vec2<f32>(h, h), k).a - density_at(q + vec2<f32>(h, -h), k).a
        - density_at(q + vec2<f32>(-h, h), k).a + density_at(q + vec2<f32>(-h, -h), k).a) / (4.0 * h * h);
    let g2 = ax * ax + ay * ay;
    if g2 < 1e-8 {
        return vec3<f32>(0.0);
    }
    let kappa = -(axx * ay * ay - 2.0 * axy * ax * ay + ayy * ax * ax) / (g2 * sqrt(g2));
    return vec3<f32>(kappa, -vec2<f32>(ax, ay) / sqrt(g2));
}

// The surface's curvature at `q` (with the ripples on a surface that faces up or down) and its
// outward way, read from the amount at the level a blur of `blur` world px spans, at least level 1,
// mixed between the two nearest levels, so the drops' grain and a deep point's wide view of the
// surface neither speckle it nor step it. (Never finer than 12 world px: the drops' own jagged
// outline is no lens.)
fn bend_at(q: vec2<f32>, blur: f32) -> vec3<f32> {
    let level = clamp(log2(max(blur, 12.0) / u.grid.z), 1.0, 4.0);
    var b = vec3<f32>(0.0);
    for (var k = 1; k <= 4; k++) {
        let w = level_share(level, k);
        if w > 0.0 {
            b += w * surface_bend(q, k);
        }
    }
    let n = b.yz / max(length(b.yz), 1e-6);
    let kappa = b.x + lu.time.y * smoothstep(0.5, 0.9, abs(n.y)) * ripple_kappa(q.x, lu.time.x, blur);
    return vec3<f32>(kappa, n);
}

// The antiderivative of 1 / max(|x|, CAUSTIC_NEAREST): the focusing's sum over a stretch of x.
fn focus_sum(x: f32) -> f32 {
    let a = abs(x) / CAUSTIC_NEAREST;
    return sign(x) * select(a, 1.0 + log(a), a > 1.0);
}

// How much the surface crossed at `q` focuses light `t` px under it, seen blurred over `blur` px,
// the light coming in along `towards` reversed, as the light texel of `texel` world px sees it.
fn caustic_at(q: vec2<f32>, t: f32, blur: f32, towards: vec2<f32>, texel: f32) -> f32 {
    let here = bend_at(q, blur);
    let n = here.yz;
    // A curved surface focuses as a lens: 1 / |1 - t / f|, f = η / ((η - 1) κ). Its bright lines are
    // much thinner than a light texel where they come to a focus, so the texel takes the focusing
    // over all of itself (from the curvature a texel along the surface, and its own depth across
    // it), not at its middle alone: a line read at texel middles only beads where it slants across
    // them.
    let along = bend_at(q + vec2<f32>(-n.y, n.x) * texel, blur).x;
    let lens = (ETA - 1.0) / ETA;
    let x = 1.0 - t * here.x * lens;
    let spans = lens * (texel * abs(here.x) + t * abs(along - here.x));
    var c = 1.0 / max(abs(x), CAUSTIC_NEAREST);
    if spans > 1e-4 {
        c = (focus_sum(x + 0.5 * spans) - focus_sum(x - 0.5 * spans)) / spans;
    }
    // Light grazing the surface is mostly thrown off it, and what gets in isn't focused.
    let facing = smoothstep(0.15, 0.5, abs(dot(n, towards)));
    return clamp(mix(1.0, c, lu.light.z * facing), 0.0, 8.0);
}

// The amount's level the light finds the surface it comes in through on: about 12 world px, so a
// drop flying just over the water, or one a lamp's ray barely grazes, is not taken for the surface
// (its tiny curve would focus the light at random).
fn surface_level() -> i32 {
    return clamp(i32(round(log2(12.0 / u.grid.z))), 1, 3);
}

// Where along `towards` from `p`, between `lo` (in the liquid) and `hi` (out of it), the surface is.
fn crossing(p: vec2<f32>, towards: vec2<f32>, lo: f32, hi: f32) -> f32 {
    var a = lo;
    var b = hi;
    for (var i = 0; i < 5; i++) {
        let mid = 0.5 * (a + b);
        let out_of_it = density_at(p + towards * mid, surface_level()).a < COARSE_LEVEL;
        a = select(mid, a, out_of_it);
        b = select(b, mid, out_of_it);
    }
    return 0.5 * (a + b);
}

// A lamp's light at a point: through the liquid, and as if there were none.
struct LampLight {
    light: vec3<f32>,
    free: vec3<f32>,
};

// Lamp `n`'s light at world point `p` (the march's least step `texel`).
fn lamp_light(n: u32, p: vec2<f32>, texel: f32) -> LampLight {
    let lamp = lu.lamps[n];
    let kind = u32(lamp.place.z + 0.5);
    let aim = lamp.aim.xy;
    let softness = lamp.colour.w;
    var towards = vec2<f32>(0.0, -1.0);
    var dist = 0.0;
    var shine = 1.0;
    var k_soft = mix(32.0, 2.0, softness);
    if kind == 2u {
        // Sun: from far away against its aim, to the course's edge.
        towards = -aim;
        dist = to_edge(p, towards);
    } else if kind == 1u {
        // Strip: the nearest point of its line, shining to its aim's side.
        let across = vec2<f32>(-aim.y, aim.x);
        let half_len = 0.5 * lamp.aim.w;
        let s = clamp(dot(p - lamp.place.xy, across), -half_len, half_len);
        let d = lamp.place.xy + across * s - p;
        dist = length(d);
        towards = select(-aim, d / max(dist, 1e-6), dist > 1e-3);
        let facing = dot(-towards, aim);
        shine = smoothstep(-0.05, 0.05 + 0.6 * softness, facing) * REACH / (REACH + dist);
        k_soft = k_soft / (1.0 + 0.5 * lamp.aim.w / max(dist, 1.0));
    } else {
        // Spot: a cone round its aim.
        let d = lamp.place.xy - p;
        dist = length(d);
        towards = select(-aim, d / max(dist, 1e-6), dist > 1e-3);
        let half_angle = lamp.aim.z;
        if half_angle < PI {
            let angle = acos(clamp(dot(-towards, aim), -1.0, 1.0));
            let edge = max(radians(2.0), softness * half_angle);
            shine = 1.0 - smoothstep(half_angle - 0.5 * edge, half_angle + 0.5 * edge, angle);
        }
        shine *= REACH / (REACH + dist);
    }
    var out: LampLight;
    out.light = vec3<f32>(0.0);
    out.free = vec3<f32>(0.0);
    if shine <= 1e-4 || luma(lamp.colour.rgb) <= 0.0 {
        return out;
    }
    let shadows = lamp.place.w > 0.5;
    var vis = 1.0;
    var tau = vec3<f32>(0.0);
    var tint = vec3<f32>(1.0);
    var caustic = 1.0;
    // Whether the march has been in the liquid yet (a point in a bubble deep under the surface is
    // lit through the surface too), and has come out of it.
    var been_in = density_at(p, surface_level()).a > COARSE_LEVEL;
    var surface = false;
    // How wide the lamp looks from a point (radians): a deep point sees the surface's caustics blurred
    // over this times its depth.
    let seen = 0.02 + 0.2 * softness;
    var t = 0.5 * texel;
    var last_t = 0.0;
    for (var step_n = 0; step_n < MOST_STEPS; step_n++) {
        if t >= dist {
            break;
        }
        let q = p + towards * t;
        var stride = 1e9;
        if shadows {
            let d = field_at(q);
            if d < 0.0 {
                if glass_at(q) {
                    let g = lu.materials[owner_at(q)].colour.rgb;
                    tint *= pow(max(g, vec3<f32>(1e-3)), vec3<f32>(texel / 64.0));
                    stride = texel;
                } else {
                    vis = 0.0;
                    break;
                }
            } else {
                // (Never softer than the field's reach allows: past SHADOW_REACH it says only
                // "far", and a penumbra cut off there would end in a hard edge. Glass, which the light
                // goes through, casts no soft edge.)
                let soft = max(k_soft * d / t, d / SHADOW_REACH);
                if soft < vis && !glass_at(q) {
                    vis = soft;
                }
                stride = max(d, texel);
            }
        }
        // The liquid: a coarse texel with none in it is skipped whole.
        let any_here = density_at(q, 4).a > 1e-3;
        if any_here {
            let s = max(texel, 0.04 * t);
            // (Level 1 at least: the drops' own grain would speckle the light texel by texel.)
            let level = clamp(log2(max(s / u.grid.z, 1.0)), 1.0, 5.0);
            var sigma = vec3<f32>(0.0);
            for (var k = 1; k <= 5; k++) {
                let w = level_share(level, k);
                if w > 0.0 {
                    sigma += w * density_at(q, k).rgb;
                }
            }
            tau += sigma * s;
            stride = min(stride, s);
        } else {
            stride = min(stride, max(8.0 * u.grid.z, texel));
        }
        // Where the light came into the liquid: the first point out of it on the way to the lamp,
        // found exactly between it and the last point in it. (Into a coarse texel with none in it
        // is out of it too.)
        let out_of_it = !any_here || density_at(q, surface_level()).a < COARSE_LEVEL;
        if !surface && been_in && out_of_it {
            surface = true;
            let tc = crossing(p, towards, last_t, t);
            caustic = caustic_at(p + towards * tc, tc, tc * seen, towards, texel);
        }
        been_in = been_in || !out_of_it;
        last_t = t;
        t += stride;
    }
    let free = lamp.colour.rgb * shine * clamp(vis, 0.0, 1.0) * tint;
    out.light = free * exp(-tau) * caustic;
    out.free = free;
    return out;
}

// The way to lamp `n` from `p` (unit).
fn lamp_way(n: u32, p: vec2<f32>) -> vec2<f32> {
    let lamp = lu.lamps[n];
    let kind = u32(lamp.place.z + 0.5);
    let aim = lamp.aim.xy;
    if kind == 2u {
        return -aim;
    }
    var d = lamp.place.xy - p;
    if kind == 1u {
        let across = vec2<f32>(-aim.y, aim.x);
        let half_len = 0.5 * lamp.aim.w;
        let s = clamp(dot(p - lamp.place.xy, across), -half_len, half_len);
        d = lamp.place.xy + across * s - p;
    }
    let l = length(d);
    return select(-aim, d / max(l, 1e-6), l > 1e-3);
}

@fragment
fn fs_light(in: FullOut) -> LightOut {
    let texel = lb.lo.z;
    var p = lb.lo.xy + (floor(in.pos.xy) + 0.5) * texel;
    var d0 = field_at(p);
    // The faces' map lights a texel inside a solid at the face nearest it.
    if lb.lo.w > 0.5 && d0 < 0.0 {
        p = p + field_normal(p) * (-d0 + texel);
        d0 = field_at(p);
    }
    var out: LightOut;
    if d0 < 0.0 && !glass_at(p) {
        out.l = vec4<f32>(0.0);
        out.d = vec4<f32>(0.0);
        out.f = vec4<f32>(0.0);
        return out;
    }
    var sum = vec3<f32>(0.0);
    var free = vec3<f32>(0.0);
    var way = vec3<f32>(0.0);
    for (var n = 0u; n < lu.counts.y; n++) {
        let light = lamp_light(n, p, texel);
        let w = luma(light.free);
        sum += light.light;
        free += light.free;
        way += vec3<f32>(lamp_way(n, p) * w, w);
    }
    out.l = vec4<f32>(sum, 1.0);
    out.d = vec4<f32>(way, 0.0);
    out.f = vec4<f32>(free, 1.0);
    return out;
}

// ---- 5. the spread light ----

// The spread light's base texel `at`'s middle (world).
fn spread_point(at: vec2<i32>) -> vec2<f32> {
    return lu.spread.xy + (vec2<f32>(at) + 0.5) * lu.spread.z;
}

// How open the world point `p` is for the spread light, over a texel of `size` world px.
fn openness(p: vec2<f32>, size: f32) -> f32 {
    let d = field_at(p);
    var o = clamp(0.5 + d / size, 0.0, 1.0);
    if d < 0.0 && glass_at(p) {
        o = max(o, GLASS_OPEN);
    }
    // Past the course is closed.
    let inside = all(p >= lu.course.xy) && all(p <= lu.course.zw);
    return select(0.0, o, inside);
}

// What a base texel gives off (its liquid's glow, and the lamp light its faces and liquid bounce)
// and how much of it the liquid covers (which stops light passing it).
@fragment
fn fs_spread_in(in: FullOut) -> @location(0) vec4<f32> {
    let at = vec2<i32>(floor(in.pos.xy));
    let p = spread_point(at);
    let size = lu.spread.z;
    let o = openness(p, size);
    // The node fields' level nearest the texel.
    let k = clamp(i32(round(log2(max(size / u.grid.z, 1.0)))), 0, i32(textureNumLevels(glows)) - 1);
    let glow = bilinear0(glows, node_t(p, k), k);
    let tint = bilinear0(tints, node_t(p, k), k);
    let liquid = clamp(tint.a, 0.0, 1.0) * o;
    var e = glow.rgb * o;
    let lit = load0(bounce_map, at, 0).rgb;
    if lu.light.y > 0.0 && luma(lit) > 0.0 {
        let liquid_colour = tint.rgb / max(tint.a, 1e-4);
        let solid = 1.0 - o;
        let solid_colour = lu.materials[owner_at(p)].colour.rgb;
        e += lu.light.y * lit * (liquid * liquid_colour + solid * solid_colour);
    }
    return vec4<f32>(e, liquid);
}

// The next mip level: 2 x 2 averages.
@fragment
fn fs_spread_down(in: FullOut) -> @location(0) vec4<f32> {
    let b = 2 * vec2<i32>(floor(in.pos.xy));
    var sum = vec4<f32>(0.0);
    for (var j = 0; j < 2; j++) {
        for (var i = 0; i < 2; i++) {
            sum += 0.25 * load0(spread_src, b + vec2<i32>(i, j), 0);
        }
    }
    return sum;
}

// What is given off round world point `p` over a texel `size` world px across (between the mips).
fn given_off(p: vec2<f32>, size: f32) -> vec4<f32> {
    let top = f32(textureNumLevels(spread_eo)) - 1.0;
    let m = clamp(log2(max(size / lu.spread.z, 1.0)), 0.0, top);
    let k0 = i32(floor(m));
    let k1 = min(k0 + 1, i32(top));
    let t0 = (p - lu.spread.xy) / (lu.spread.z * exp2(f32(k0))) - 0.5;
    let t1 = (p - lu.spread.xy) / (lu.spread.z * exp2(f32(k1))) - 0.5;
    return mix(bilinear0(spread_eo, t0, k0), bilinear0(spread_eo, t1, k1), m - floor(m));
}

// The spread light at a base texel: cones all round, each marched out through the solid field (which
// stops it, glass only partly) gathering what is given off along it from the mip that matches its
// width, until what it has passed covers it.
@fragment
fn fs_spread_gather(in: FullOut) -> @location(0) vec4<f32> {
    let at = vec2<i32>(floor(in.pos.xy));
    let base = lu.spread.z;
    let p = spread_point(at);
    let o = openness(p, base);
    if o <= 0.0 {
        return vec4<f32>(0.0);
    }
    var sum = vec3<f32>(0.0);
    for (var c = 0; c < CONES; c++) {
        let angle = 2.0 * PI * (f32(c) + 0.5) / f32(CONES);
        let way = vec2<f32>(cos(angle), sin(angle));
        var t = 0.5 * base;
        var through = 1.0;
        var light = vec3<f32>(0.0);
        for (var n = 0; n < CONE_STEPS; n++) {
            let q = p + way * t;
            if any(q < lu.course.xy) || any(q > lu.course.zw) {
                break;
            }
            let r = max(t * CONE_TAN, 0.5 * base);
            let d = field_at(q);
            var block = clamp(0.5 - d / r, 0.0, 1.0);
            if block > 0.0 && glass_at(q) {
                block *= 1.0 - GLASS_OPEN;
            }
            let e = given_off(q, 2.0 * r);
            let share = clamp(r / max(2.0 * r, base), 0.0, 1.0);
            light += through * e.rgb * share;
            through *= (1.0 - block) * (1.0 - e.a * share);
            if through < 0.02 {
                break;
            }
            t += r;
        }
        sum += light;
    }
    return vec4<f32>(sum / f32(CONES) * SPREAD_GAIN * o, o);
}

// ---- 6. the picture ----

// The shot's light at world point `p`, between its texels by how open they are.
fn light_at(p: vec2<f32>) -> vec3<f32> {
    if lu.shot.w < 0.5 {
        return vec3<f32>(0.0);
    }
    let l = bilinear0(lmap, (p - lu.shot.xy) / lu.shot.z - 0.5, 0);
    return select(vec3<f32>(0.0), l.rgb / max(l.a, 1e-3), l.a > 1e-3);
}

// The shot's light at world point `p` as if there were no liquid.
fn free_light_at(p: vec2<f32>) -> vec3<f32> {
    if lu.shot.w < 0.5 {
        return vec3<f32>(0.0);
    }
    let l = bilinear0(lfree, (p - lu.shot.xy) / lu.shot.z - 0.5, 0);
    return select(vec3<f32>(0.0), l.rgb / max(l.a, 1e-3), l.a > 1e-3);
}

// The way to the lamps at world point `p` (unit), and how much light comes from them.
fn light_way_at(p: vec2<f32>) -> vec3<f32> {
    if lu.shot.w < 0.5 {
        return vec3<f32>(0.0);
    }
    let d = bilinear0(ldir, (p - lu.shot.xy) / lu.shot.z - 0.5, 0);
    let l = length(d.xy);
    return select(vec3<f32>(0.0), vec3<f32>(d.xy / max(l, 1e-6), d.z), l > 1e-6);
}

// The spread light at world point `p`, between its texels by how open they are.
fn spread_at(p: vec2<f32>) -> vec3<f32> {
    if lu.counts.w == 0u {
        return vec3<f32>(0.0);
    }
    let s = bilinear0(spread_s, (p - lu.spread.xy) / lu.spread.z - 0.5, 0);
    return select(vec3<f32>(0.0), s.rgb / max(s.a, 1e-3), s.a > 1e-3);
}

// The lamps' way tilted towards you: the light that rounded surfaces catch.
fn lamp_3d(way: vec3<f32>) -> vec3<f32> {
    if way.z <= 1e-5 {
        return vec3<f32>(0.0, 0.0, 1.0);
    }
    return normalize(vec3<f32>(way.xy, 0.6));
}

// Gradient row `row` at `t` (0 to 1).
fn gradient_at(row: u32, t: f32) -> vec3<f32> {
    let x = clamp(t, 0.0, 1.0) * 63.0;
    let i = u32(floor(x));
    let j = min(i + 1u, 63u);
    return mix(grads[row * 64u + i].rgb, grads[row * 64u + j].rgb, x - floor(x));
}

// Material `m`'s own colour at world point `p` (its texture; glass and flat: its Colour).
fn material_albedo(m: u32, p: vec2<f32>) -> vec3<f32> {
    let mat = lu.materials[m];
    let kind = u32(mat.colour.w + 0.5);
    let colour = mat.colour.rgb;
    let second = mat.second.rgb;
    let q = p / max(mat.second.w, 1.0);
    if kind == 0u {
        // Rock: patches and veins of the Second colour, a fine grain.
        let patches = fbm(q);
        let veins = 1.0 - smoothstep(0.0, 0.08, abs(fbm(q * 0.7 + vec2<f32>(7.3, 1.9)) - 0.5));
        let grain = vnoise(p / 1.7) - 0.5;
        let mixing = clamp(smoothstep(0.35, 0.75, patches) * 0.7 + veins * 0.5, 0.0, 1.0);
        return mix(colour, second, mixing) * (1.0 + 0.12 * grain);
    }
    if kind == 2u {
        // Paper: fibres of the Second colour.
        let fibre = vnoise(vec2<f32>(q.x * 9.0, q.y * 1.3)) * 0.6 + vnoise(p / 1.3) * 0.4;
        return mix(colour, second, 0.35 * smoothstep(0.4, 0.9, fibre));
    }
    if kind == 3u {
        // Basalt: cracks of the Second colour between its columns.
        let crack = 1.0 - smoothstep(0.02, 0.07, cell_edge(q * 1.5));
        return mix(colour * (0.92 + 0.16 * vnoise(q * 3.0)), second, crack);
    }
    return colour;
}

// The back's own colour at world point `p`.
fn back_albedo(p: vec2<f32>) -> vec3<f32> {
    let kind = lu.counts.z;
    if kind == 1u {
        return lu.back.rgb;
    }
    if kind == 2u {
        let mid = 0.5 * (lu.course.xy + lu.course.zw);
        let t = dot(p - mid, lu.back_way.xy) / max(lu.back_way.z, 1.0) + 0.5;
        return gradient_at(16u, t);
    }
    if kind == 3u {
        let size = vec2<f32>(textureDimensions(backpic, 0));
        let t = (p - lu.picture.xy) / lu.picture.zw * size - 0.5;
        return bilinear_clamped(backpic, t, 0).rgb;
    }
    return material_albedo(0u, p) * (1.0 - lu.back.w);
}

// Paper's cut-out shadow on what is behind it at world point `p`: 1 lit, less in its shadow.
fn paper_shadow(p: vec2<f32>) -> f32 {
    let depth = lu.time.z;
    if depth <= 0.0 {
        return 1.0;
    }
    let q = p - vec2<f32>(0.5, 0.7) * depth;
    let d = field_at(q);
    if d >= 0.5 * depth || material_kind(owner_at(q)) != 2u {
        return 1.0;
    }
    return 1.0 - PAPER_DARK * (1.0 - smoothstep(-0.5 * depth, 0.5 * depth, d));
}

// The back at world point `p`, lit.
fn back_lit(p: vec2<f32>) -> vec3<f32> {
    let far = max(field_at(p), 0.0);
    let ao = 1.0 - AO_DARK * (1.0 - smoothstep(0.0, AO_REACH, far));
    let shade = paper_shadow(p);
    return back_albedo(p) * shade * (lu.light.x * ao + light_at(p) + spread_at(p));
}

// The liquids' shares at picture pixel `px` (summing to 1; none: liquid 0's).
fn shares(px: vec2<i32>) -> array<f32, 16> {
    var w = array<f32, 16>(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    let n = max(lu.counts.x, 1u);
    let a = textureLoad(wt0, px, 0);
    w[0] = a.x;
    w[1] = a.y;
    w[2] = a.z;
    w[3] = a.w;
    if n > 4u {
        let b = textureLoad(wt1, px, 0);
        let c = textureLoad(wt2, px, 0);
        let d = textureLoad(wt3, px, 0);
        w[4] = b.x;
        w[5] = b.y;
        w[6] = b.z;
        w[7] = b.w;
        w[8] = c.x;
        w[9] = c.y;
        w[10] = c.z;
        w[11] = c.w;
        w[12] = d.x;
        w[13] = d.y;
        w[14] = d.z;
        w[15] = d.w;
    }
    var sum = 0.0;
    for (var k = 0u; k < 16u; k++) {
        if k >= n {
            w[k] = 0.0;
        }
        sum += w[k];
    }
    if sum <= 1e-8 {
        w[0] = 1.0;
        return w;
    }
    for (var k = 0u; k < 16u; k++) {
        w[k] = w[k] / sum;
    }
    return w;
}

// The amount at picture pixel `px`, made up near a face as plain's `amount_filled`, with a splat of
// `r_world` world px.
fn look_amount(px: vec2<i32>, far: f32, r_world: f32) -> f32 {
    let size = vec2<i32>(i32(u.size.x), i32(u.size.y));
    let a = textureLoad(look0, clamp(px, vec2<i32>(0), size - 1), 0).a;
    let missing = side((far - u.grid.w * u.grid.z) / r_world);
    return a / max(1.0 - missing, 1.0 / MOST_FILL);
}

fn look_amount_at(px: vec2<i32>, r_world: f32) -> f32 {
    return look_amount(px, face_at(world_of(vec2<f32>(px) + 0.5)), r_world);
}

// Air's share of the open part round grid point `g` at the ladder's level `k`.
fn air_share(g: vec2<f32>, k: i32) -> f32 {
    let v = ladder_at(g, k);
    return clamp(v.x / max(v.y, 1e-4), 0.0, 1.0);
}

// Shades `x` (0 to 1) in `bands` flat steps, each step's edge softened by `softness`.
fn banded(x: f32, bands: f32, softness: f32) -> f32 {
    let y = clamp(x, 0.0, 1.0) * bands;
    let f = y - floor(y);
    let s = max(softness, 0.02) * 0.5;
    return (floor(y) + smoothstep(0.5 - s, 0.5 + s, f)) / bands;
}

@fragment
fn fs_look(in: FullOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(floor(in.pos.xy));
    let at = vec2<f32>(px) + 0.5;
    let w = world_of(at);
    let far = field_at(w);
    let face = face_at(w);
    let scale = u.view.z;
    let ambient = lu.light.x;
    let lamp_l = light_at(w);
    let spread_l = spread_at(w);
    var colour = back_lit(w);
    var in_liquid = 0.0;

    let s0 = textureLoad(look0, px, 0);
    // The liquid, where its splats or its outline reach.
    if s0.a > 0.0 {
        let share = shares(px);
        var l_colour = vec3<f32>(0.0);
        var deep = vec3<f32>(0.0);
        var clarity = 0.0;
        var solid = 0.0;
        var glow = vec3<f32>(0.0);
        var edge_colour = vec3<f32>(0.0);
        var outline = 0.0;
        var surface = vec4<f32>(0.0);
        var bend = 0.0;
        var split = 0.0;
        var ripples = 0.0;
        var radius = 0.0;
        var bands = 0.0;
        var band_soft = 0.0;
        let speed = s0.r / s0.a * SPEED_UNIT;
        for (var n = 0u; n < 16u; n++) {
            let k = share[n];
            if k > 0.0 {
                let l = lu.liquids[n];
                l_colour += k * l.colour.rgb;
                deep += k * l.deep.rgb;
                clarity += k * l.colour.w;
                solid += k * l.deep.w;
                glow += k * glow_of(l, speed);
                edge_colour += k * l.edge.rgb;
                outline += k * l.edge.w;
                surface += k * l.surface;
                bend += k * l.body.x;
                split += k * l.body.y;
                ripples += k * l.body.z;
                radius += k * l.more.y;
                bands += k * l.shape.w;
                band_soft += k * l.more.x;
            }
        }
        let r_world = max(radius * u.grid.z, LEAST_R_PX / scale);
        let a = look_amount(px, face, r_world);
        let gx = 0.5 * (look_amount_at(px + vec2<i32>(1, 0), r_world) - look_amount_at(px - vec2<i32>(1, 0), r_world));
        let gy = 0.5 * (look_amount_at(px + vec2<i32>(0, 1), r_world) - look_amount_at(px - vec2<i32>(0, 1), r_world));
        let slope = length(vec2<f32>(gx, gy));
        let level = u.splat.z;
        // How far inside the outline, picture px.
        let inside = select(select(-1e4, 1e4, a >= level), (a - level) / slope, slope > 1e-6);
        let outline_px = select(0.0, max(outline * scale, 1.0), outline > 0.0);
        let alpha = clamp(0.5 + inside + 0.5 * outline_px, 0.0, 1.0);
        if alpha > 0.0 {
            // The surface seen smoothly: air's share of the open part round the point, from the
            // depth ladder at about SMOOTH_SURFACE world px, its slope pointing out to the air.
            let ks = clamp(i32(round(log2(max(SMOOTH_SURFACE / u.grid.z, 1.0)))), 0, i32(u.widths.z) - 1);
            let hs = 0.5 * exp2(f32(ks));
            let g0 = (w - u.grid.xy) / u.grid.z;
            let o = air_share(g0, ks);
            let grad_o = vec2<f32>(
                air_share(g0 + vec2<f32>(hs, 0.0), ks) - air_share(g0 - vec2<f32>(hs, 0.0), ks),
                air_share(g0 + vec2<f32>(0.0, hs), ks) - air_share(g0 - vec2<f32>(0.0, hs), ks),
            ) / (2.0 * hs * u.grid.z);
            let go = length(grad_o);
            let smooth_out = select(vec2<f32>(0.0, -1.0), grad_o / max(go, 1e-8), go > 1e-6);
            let smooth_in = select(1e4, max(0.5 - o, 0.0) / go, go > 1e-6);
            // How far inside its outline (world px): the picture's own within a splat of the outline,
            // where its slope says it; past two splats the smooth surface's (but no nearer), as the
            // amount's slope between the drops deep in a body would make up an outline at every pixel.
            let near = inside / scale;
            let past = smoothstep(r_world, 2.0 * r_world, near);
            let inside_w = mix(near, max(smooth_in, 2.0 * r_world), past);
            // The outward way: the amount's slope over 2 px near the outline, else the smooth one.
            let g2x = look_amount_at(px + vec2<i32>(2, 0), r_world) - look_amount_at(px - vec2<i32>(2, 0), r_world);
            let g2y = look_amount_at(px + vec2<i32>(0, 2), r_world) - look_amount_at(px - vec2<i32>(0, 2), r_world);
            let gl = length(vec2<f32>(g2x, g2y));
            let out_way = select(smooth_out, -vec2<f32>(g2x, g2y) / max(gl, 1e-6), gl > 0.05);
            let absorbed_share = absorbed(at, clarity);
            // Its colour: each liquid's from what its Colour from says, mixed by their shares.
            var albedo = vec3<f32>(0.0);
            for (var n = 0u; n < 16u; n++) {
                let k = share[n];
                if k > 0.0 {
                    let l = lu.liquids[n];
                    let source = u32(l.body.w + 0.5);
                    var own = l.colour.rgb;
                    if source == 1u {
                        own = gradient_at(n, smoothstep(0.0, 1500.0, speed));
                    }
                    var c = mix(own, l.deep.rgb, absorbed_share);
                    if source == 2u {
                        c = gradient_at(n, absorbed_share);
                    }
                    albedo += k * c;
                }
            }
            // The surface: rounded over ROUND in from its outline, tilted by the ripples near the top.
            let round_w = max(ROUND, 1.5 / scale);
            let tilt = 1.0 - smoothstep(0.0, round_w, inside_w);
            var normal2 = out_way * tilt;
            var wave_shift = 0.0;
            if ripples > 0.0 {
                let r = ripple(w.x, lu.time.x);
                let near_top = exp(-max(inside_w, 0.0) / 40.0) * smoothstep(0.3, 0.8, -smooth_out.y);
                normal2.x += -ripples * r.y * near_top;
                wave_shift = -ripples * r.y * near_top;
            }
            let normal = normalize(vec3<f32>(normal2, 1.0 - tilt + 0.15));
            let way = light_way_at(w);
            let l3 = lamp_3d(way);
            // Lit: see-through liquid by the light that comes through it; solid liquid on its front,
            // as if it weren't there, rounded by its surface and a little darker deep in.
            let facing = clamp(max(dot(normal, l3), 0.0) / max(l3.z, 0.3), 0.0, 2.0);
            let front = free_light_at(w) * facing * (1.0 - 0.4 * absorbed(at, 60.0));
            // (A glowing liquid isn't lit by its own glow.)
            let own_glow = 1.0 - smoothstep(0.0, 0.5, luma(glow));
            var body = albedo * (ambient + mix(lamp_l, front, solid) + spread_l * own_glow);
            // See-through: what is behind, bent at its edge, tinted by its colour.
            let trans = select(0.0, (1.0 - solid) * exp(-TANK / clarity) * (1.0 - 0.5 * absorbed_share), clarity > 0.0);
            if trans > 0.0 {
                let edge = 1.0 - smoothstep(0.0, 40.0, max(inside_w, 0.0));
                let shift = (-smooth_out * edge + vec2<f32>(wave_shift, 0.0)) * bend;
                let tint = pow(max(l_colour, vec3<f32>(1e-4)), vec3<f32>(TANK / clarity));
                var behind: vec3<f32>;
                if split > 0.0 {
                    behind = vec3<f32>(
                        back_lit(w + shift * (1.0 - 0.15 * split)).r,
                        back_lit(w + shift).g,
                        back_lit(w + shift * (1.0 + 0.15 * split)).b,
                    );
                } else {
                    behind = back_lit(w + shift);
                }
                body = mix(body, behind * tint, trans);
            }
            // Bands: the light on it in flat steps.
            if bands >= 1.0 {
                let base = max(luma(albedo), 1e-4);
                let lit_share = luma(body) / base;
                body = body * (2.0 * banded(0.5 * lit_share, bands, band_soft) / max(lit_share, 1e-4));
            }
            // Shine, from the lamps and faintly from the sky above.
            let shine = surface.x;
            let power = mix(400.0, 8.0, surface.y);
            if shine > 0.0 {
                let h = normalize(l3 + vec3<f32>(0.0, 0.0, 1.0));
                let h_sky = normalize(normalize(vec3<f32>(0.0, -0.8, 0.6)) + vec3<f32>(0.0, 0.0, 1.0));
                let spec = pow(max(dot(normal, h), 0.0), power);
                let sky = pow(max(dot(normal, h_sky), 0.0), power);
                body += shine * (lamp_l * spec * select(0.0, 1.0, way.z > 0.0) + vec3<f32>(0.2 * ambient * sky));
            }
            // Edge sheen where the surface turns away.
            let turn = 1.0 - normal.z;
            body += surface.z * turn * turn * (vec3<f32>(ambient) + lamp_l);
            // Thin film: oil-on-water colours where it turns, and in a slow drift.
            if surface.w > 0.0 {
                let t = 1.5 * turn + vnoise(w / 60.0 + vec2<f32>(lu.time.x * 0.05, 0.0));
                let film = 0.5 + 0.5 * cos(2.0 * PI * (t + vec3<f32>(0.0, 1.0 / 3.0, 2.0 / 3.0)));
                body += surface.w * film * (vec3<f32>(ambient) + lamp_l) * (0.15 + 0.6 * turn);
            }
            // Shafts in the liquid, and its glow.
            body += lu.light.w * 0.6 * lamp_l * l_colour + glow;
            // The outline, centred on its edge.
            if outline_px > 0.0 {
                let on = 1.0 - smoothstep(0.5 * outline_px - 0.5, 0.5 * outline_px + 0.5, abs(inside));
                body = mix(body, edge_colour, on);
            }
            colour = mix(colour, body, alpha);
            in_liquid = alpha;
        }
    }

    // Shafts in the air.
    if far > 0.0 {
        colour += lu.light.w * 0.12 * lamp_l * (1.0 - in_liquid);
    }

    // The solids on top, lit at their face.
    let cover = rock_cover(far);
    if cover > 0.0 {
        let m = owner_at(w);
        let mat = lu.materials[m];
        let kind = u32(mat.colour.w + 0.5);
        let depth = mat.shape.x;
        let rough = mat.shape.y;
        let deep_in = max(-far, 0.0);
        let n = field_normal(w);
        let spill = 2.0 * max(depth, 8.0);
        // (Falling off fast past the spill: the light on a face changes sharply where its own
        // shadow starts, and carried deep in it would draw lines out from the solid's middle.)
        let fade = exp(-(deep_in / spill) * (deep_in / spill));
        let at_face = w + n * (deep_in + 1.5 * max(lu.shot.z, u.grid.z));
        let lit = light_at(at_face) * fade;
        let spread_lit = spread_at(w + n * (deep_in + 1.5 * lu.spread.z)) * fade;
        let l3 = lamp_3d(light_way_at(at_face));
        var normal = vec3<f32>(0.0, 0.0, 1.0);
        if kind != 4u && depth > 0.0 {
            let tilt = 1.0 - smoothstep(0.0, max(depth, 1.5 / scale), deep_in);
            normal = normalize(vec3<f32>(n * tilt, 1.0 - tilt + 0.15));
        }
        let facing = clamp(max(dot(normal, l3), 0.0) / max(l3.z, 0.3), 0.0, 2.0);
        let albedo = material_albedo(m, w);
        var solid_c = albedo * (ambient * (0.7 + 0.3 * normal.z) + lit * facing + spread_lit);
        if kind != 4u {
            let h = normalize(l3 + vec3<f32>(0.0, 0.0, 1.0));
            solid_c += (1.0 - rough) * pow(max(dot(normal, h), 0.0), mix(80.0, 4.0, rough)) * lit;
        }
        if kind == 1u {
            // Glass: frosted, the back blurred through it and tinted, its edge catching the lamps.
            let o = max(mat.second.w, 1.0) / 20.0;
            let frosted = 0.25 * (back_lit(w + vec2<f32>(o, o)) + back_lit(w + vec2<f32>(-o, o))
                + back_lit(w + vec2<f32>(o, -o)) + back_lit(w + vec2<f32>(-o, -o)));
            solid_c = mix(solid_c, frosted * mat.colour.rgb, 0.55);
            solid_c += 0.8 * lit * (1.0 - smoothstep(0.0, max(2.0, 1.5 / scale), deep_in));
        }
        let outline = mat.shape.z;
        if outline > 0.0 {
            let on = 1.0 - smoothstep(outline - 0.5 / scale, outline + 0.5 / scale, deep_in);
            solid_c = mix(solid_c, mat.edge.rgb, on);
        }
        colour = mix(colour, solid_c, cover);
    }
    return vec4<f32>(max(colour, vec3<f32>(0.0)), 1.0);
}

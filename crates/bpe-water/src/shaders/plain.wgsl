// The plain drawing (docs/superpowers/plans/water-mode/core-c3-plain-drawing.md): Water mode's
// picture of the water, straight from a body's drops.
//
// 1. `vs_splat`/`fs_splat`: every drop a soft round splat, summed (additive blending, in the drops'
//    order, so the sums repeat) into two half-float pictures: its liquid's colour and Deep colour
//    weighted by the splat, the splat's weight, and its Clarity.
// 2. `fs_base`: at half size, how much of each texel is liquid (the amount cut at its level) or rock:
//    the first level of a ladder. (Light comes in only through the air: rock is not a way in.)
// 3. `fs_down`: each level of the ladder from the one before, half the size, blurred with the
//    [1 3 3 1] / 8 kernel (four bilinear taps).
// 4. `fs_show`: the picture: the back, the liquid (its outline cut from the amount and smoothed by
//    its slope; its depth from the ladder, read at the pixel's Clarity, taking it from Colour to Deep
//    colour; a thin lighter rim along its surface), the rock on top (its edge smoothed, a darker rim
//    inside it), a fixed dither, sRGB.

struct Liquid {
    // linear colour, Clarity (world px); linear Deep colour, 0
    colour: vec4<f32>,
    deep: vec4<f32>,
};

struct U {
    // the world point at the picture's top left, picture px a world px, 0
    view: vec4<f32>,
    // the picture's width and height, the layout's nodes across and down
    size: vec4<u32>,
    // the layout's origin (world px), a cell (px), how far off a face drops rest (cells)
    grid: vec4<f32>,
    // a splat's radius (picture px), its peak weight, the level the amount is cut at, how many liquids
    splat: vec4<f32>,
    // the liquid's rim (picture px), the rock's rim (picture px), the ladder's levels, the dither's size
    widths: vec4<f32>,
    // linear back colour, linear rock colour
    back: vec4<f32>,
    rock: vec4<f32>,
    liquids: array<Liquid, 16>,
};

const PI: f32 = 3.14159265;
// How much lighter the liquid's rim is (towards white) and how much darker the rock's.
const RIM_LIGHT: f32 = 0.22;
const ROCK_DARK: f32 = 0.25;
// Clarity goes into the sums divided by this (half floats).
const CLARITY_SCALE: f32 = 1024.0;
// The blur (sigma, picture px) of the ladder's level 0, read with a cubic B-spline: level k is
// about this times 2^k.
const LADDER_SIGMA: f32 = 1.53;
// The most the face fill makes of the amount (a splat mostly past the face).
const MOST_FILL: f32 = 5.0;

@group(0) @binding(0) var<uniform> u: U;
// The drops: place and speed (grid units), kind, serial, colour, 0.
@group(0) @binding(1) var<storage, read> xv: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> ids: array<vec4<u32>>;
// The two amount pictures: (Σ k colour, Σ k) and (Σ k deep, Σ k clarity / CLARITY_SCALE).
@group(0) @binding(3) var amount0: texture_2d<f32>;
@group(0) @binding(4) var amount1: texture_2d<f32>;
// The solid field: each node's distance from the nearest face, world px (below 0 inside).
@group(0) @binding(5) var<storage, read> field: array<f32>;
// The ladder (all levels in `fs_show`, the level before in `fs_down`) and its sampler.
@group(0) @binding(6) var ladder: texture_2d<f32>;
@group(0) @binding(7) var smp: sampler;

fn finite2(v: vec2<f32>) -> bool {
    return abs(v.x) < 1e30 && abs(v.y) < 1e30;
}

// The liquid a drop of `kind` is: past the looks given, the last given.
fn liquid_of(kind: u32) -> Liquid {
    let n = max(u32(u.splat.w), 1u);
    return u.liquids[min(kind & 15u, n - 1u)];
}

// ---- 1. splats ----

struct SplatOut {
    @builtin(position) pos: vec4<f32>,
    // where in the splat, in radii
    @location(0) off: vec2<f32>,
    @location(1) @interpolate(flat) colour: vec3<f32>,
    @location(2) @interpolate(flat) deep: vec4<f32>,
};

@vertex
fn vs_splat(@builtin(vertex_index) v: u32, @builtin(instance_index) i: u32) -> SplatOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
    );
    let c = corners[v];
    var out: SplatOut;
    out.off = c;
    let g = xv[i].xy;
    let last = vec2<f32>(f32(u.size.z) - 1.0, f32(u.size.w) - 1.0);
    // A drop that is not a number or has left the course (the engine's `alive`) is not drawn: all six
    // corners on one point off the picture make nothing.
    if !finite2(g) || any(g < vec2<f32>(0.0)) || any(g > last) {
        out.pos = vec4<f32>(-2.0, -2.0, 0.0, 1.0);
        out.colour = vec3<f32>(0.0);
        out.deep = vec4<f32>(0.0);
        return out;
    }
    let world = u.grid.xy + g * u.grid.z;
    let p = (world - u.view.xy) * u.view.z + c * u.splat.x;
    let size = vec2<f32>(f32(u.size.x), f32(u.size.y));
    out.pos = vec4<f32>(p.x / size.x * 2.0 - 1.0, 1.0 - p.y / size.y * 2.0, 0.0, 1.0);
    let l = liquid_of(ids[i].x);
    out.colour = l.colour.rgb;
    out.deep = vec4<f32>(l.deep.rgb, l.colour.w / CLARITY_SCALE);
    return out;
}

struct Sums {
    @location(0) a: vec4<f32>,
    @location(1) b: vec4<f32>,
};

@fragment
fn fs_splat(in: SplatOut) -> Sums {
    let q2 = dot(in.off, in.off);
    if q2 >= 1.0 {
        discard;
    }
    let k = u.splat.y * (1.0 - q2) * (1.0 - q2);
    var out: Sums;
    out.a = vec4<f32>(in.colour * k, k);
    out.b = in.deep * k;
    return out;
}

// ---- the field, the face fill, the amount ----

// The solid field at world point `p` (between the nodes round it), world px.
fn field_at(p: vec2<f32>) -> f32 {
    let cols = u.size.z;
    let rows = u.size.w;
    let g = clamp((p - u.grid.xy) / u.grid.z, vec2<f32>(0.0), vec2<f32>(f32(cols) - 1.001, f32(rows) - 1.001));
    let x = u32(floor(g.x));
    let y = u32(floor(g.y));
    let f = g - vec2<f32>(f32(x), f32(y));
    let at = y * cols + x;
    let top = mix(field[at], field[at + 1u], f.x);
    let bottom = mix(field[at + cols], field[at + cols + 1u], f.x);
    return mix(top, bottom, f.y);
}

// The world point at the middle of picture pixel `px` (or any picture point).
fn world_of(px: vec2<f32>) -> vec2<f32> {
    return u.view.xy + px / u.view.z;
}

// The share of a splat's weight further than `q` radii to one side of its middle: its profile
// across, (1 - s^2)^2.5, summed (with s = sin t it is cos^6 t).
fn side(q: f32) -> f32 {
    let t = asin(clamp(q, -1.0, 1.0));
    let f = 5.0 * t / 16.0 + 15.0 / 64.0 * sin(2.0 * t) + 3.0 / 64.0 * sin(4.0 * t) + sin(6.0 * t) / 192.0;
    return (5.0 * PI / 32.0 - f) / (5.0 * PI / 16.0);
}

// The amount at picture pixel `px`, made up near a face: drops rest `rest` cells off a face, so a
// splat's share beyond that line is missing there and is added back, and liquid that rests on a
// face is drawn up to it. `far`: the field at the pixel.
fn amount_filled(px: vec2<i32>, far: f32) -> f32 {
    let size = vec2<i32>(i32(u.size.x), i32(u.size.y));
    let a = textureLoad(amount0, clamp(px, vec2<i32>(0), size - 1), 0).a;
    let r_world = u.splat.x / u.view.z;
    let missing = side((far - u.grid.w * u.grid.z) / r_world);
    return a / max(1.0 - missing, 1.0 / MOST_FILL);
}

fn amount_at(px: vec2<i32>) -> f32 {
    return amount_filled(px, field_at(world_of(vec2<f32>(px) + 0.5)));
}

// How much of picture pixel `px` the rock covers (its edge smoothed over a pixel).
fn rock_cover(far: f32) -> f32 {
    return clamp(0.5 - far * u.view.z, 0.0, 1.0);
}

// ---- full-picture passes ----

struct FullOut {
    @builtin(position) pos: vec4<f32>,
};

@vertex
fn vs_full(@builtin(vertex_index) v: u32) -> FullOut {
    var corners = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    var out: FullOut;
    out.pos = vec4<f32>(corners[v], 0.0, 1.0);
    return out;
}

// ---- 2. the ladder's first level ----

@fragment
fn fs_base(in: FullOut) -> @location(0) vec4<f32> {
    // Texel i covers picture pixels 2i and 2i + 1 each way.
    let t = vec2<i32>(floor(in.pos.xy));
    var closed = 0.0;
    let level = u.splat.z;
    for (var k = 0; k < 4; k++) {
        let px = 2 * t + vec2<i32>(k & 1, k >> 1);
        let far = field_at(world_of(vec2<f32>(px) + 0.5));
        let liquid = smoothstep(0.75 * level, 1.25 * level, amount_filled(px, far));
        closed += max(liquid, rock_cover(far));
    }
    return vec4<f32>(closed * 0.25, 0.0, 0.0, 0.0);
}

// ---- 3. the ladder's halvings ----

@fragment
fn fs_down(in: FullOut) -> @location(0) vec4<f32> {
    // This texel's middle is at 2i + 1 in the level before's texels; [1 3 3 1] each way is four
    // bilinear taps 0.75 texel either side of it.
    let src = vec2<f32>(textureDimensions(ladder, 0));
    let c = 2.0 * floor(in.pos.xy) + 1.0;
    var sum = 0.0;
    for (var k = 0; k < 4; k++) {
        let o = vec2<f32>(select(-0.75, 0.75, (k & 1) == 1), select(-0.75, 0.75, (k >> 1) == 1));
        sum += textureSampleLevel(ladder, smp, (c + o) / src, 0.0).r;
    }
    return vec4<f32>(sum * 0.25, 0.0, 0.0, 0.0);
}

// ---- 4. the picture ----

// The cubic B-spline's four weights at `f` (0 to 1 past the second of four texels).
fn bspline(f: f32) -> vec4<f32> {
    let f2 = f * f;
    let f3 = f2 * f;
    return vec4<f32>(1.0 - 3.0 * f + 3.0 * f2 - f3, 4.0 - 6.0 * f2 + 3.0 * f3, 1.0 + 3.0 * f + 3.0 * f2 - 3.0 * f3, f3)
        / 6.0;
}

// Level `k` of the ladder read with a cubic B-spline at picture point `px`, texel by texel (exact,
// where a bilinear sampler's weights are rounded).
fn ladder_at(px: vec2<f32>, k: i32) -> f32 {
    let last = vec2<i32>(textureDimensions(ladder, k)) - 1;
    // Level k's texel i covers picture pixels i 2^(k+1) up to (i + 1) 2^(k+1).
    let t = px / exp2(f32(k + 1)) - 0.5;
    let base = vec2<i32>(floor(t));
    let wx = bspline(t.x - floor(t.x));
    let wy = bspline(t.y - floor(t.y));
    var sum = 0.0;
    for (var j = 0; j < 4; j++) {
        var row = 0.0;
        for (var i = 0; i < 4; i++) {
            let at = clamp(base + vec2<i32>(i - 1, j - 1), vec2<i32>(0), last);
            row += wx[i] * textureLoad(ladder, at, k).r;
        }
        sum += wy[j] * row;
    }
    return sum;
}

// The inverse of the error function (Giles 2010, single precision).
fn erfinv(x: f32) -> f32 {
    var w = -log((1.0 - x) * (1.0 + x));
    var p: f32;
    if w < 5.0 {
        w = w - 2.5;
        p = 2.81022636e-08;
        p = 3.43273939e-07 + p * w;
        p = -3.5233877e-06 + p * w;
        p = -4.39150654e-06 + p * w;
        p = 0.00021858087 + p * w;
        p = -0.00125372503 + p * w;
        p = -0.00417768164 + p * w;
        p = 0.246640727 + p * w;
        p = 1.50140941 + p * w;
    } else {
        w = sqrt(w) - 3.0;
        p = -0.000200214257;
        p = 0.000100950558 + p * w;
        p = 0.00134934322 + p * w;
        p = -0.00367342844 + p * w;
        p = 0.00573950773 + p * w;
        p = -0.0076224613 + p * w;
        p = 0.00943887047 + p * w;
        p = 1.00167406 + p * w;
        p = 2.83297682 + p * w;
    }
    return p * x;
}

// How much light the liquid has lost at picture point `px`, from 0 (at its surface, and in thin
// water) towards 1, over `clarity` world px. The ladder, blurred by about `clarity`, says what share
// of the room round the point is not air (liquid or rock): b. For liquid that goes on below a flat
// surface, b at depth x is Phi(x / sigma); so x = sigma Phi^-1(b), and the light left is exp(-x /
// clarity). Light comes in only through the air, so a pool is deep at its floor and walls, and
// thin water (a film on rock, a stream by a wall, a drop) is at the edge of liquid and rock
// together: b is about a half there, and it stays its Colour.
fn absorbed(px: vec2<f32>, clarity: f32) -> f32 {
    if clarity <= 0.0 {
        return 0.0;
    }
    let top = u.widths.z - 1.0;
    let level = clamp(log2(max(clarity * u.view.z, 1e-3) / LADDER_SIGMA), 0.0, top);
    let k0 = i32(floor(level));
    let k1 = min(k0 + 1, i32(top));
    let f = level - f32(k0);
    // (Each level read with the same level number across the picture: where liquids of different
    // Clarity meet, neighbouring pixels want different levels, and some drivers take one level for
    // a whole block of pixels.)
    var b = 0.0;
    for (var k = 0; k <= i32(top); k++) {
        let w = select(0.0, 1.0 - f, k == k0) + select(0.0, f, k == k1);
        if w > 0.0 {
            b += w * ladder_at(px, k);
        }
    }
    b = clamp(b, 0.0, 1.0);
    let x = 1.41421356 * erfinv(clamp(2.0 * b - 1.0, -0.99999, 0.99999));
    // (A soft floor at 0 rather than a corner: no line where b passes a half.)
    let depth = 0.5 * (x + sqrt(x * x + 0.04));
    let sigma = exp2(level) * LADDER_SIGMA / u.view.z;
    return 1.0 - exp(-depth * sigma / clarity);
}

fn srgb_of(l: vec3<f32>) -> vec3<f32> {
    let c = clamp(l, vec3<f32>(0.0), vec3<f32>(1.0));
    return select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3<f32>(0.0031308));
}

@fragment
fn fs_show(in: FullOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(floor(in.pos.xy));
    let at = vec2<f32>(px) + 0.5;
    let far = field_at(world_of(at));
    var colour = u.back.rgb;

    let s0 = textureLoad(amount0, px, 0);
    let level = u.splat.z;
    if s0.a > 0.0 {
        let a = amount_filled(px, far);
        // The amount's slope by central differences: the outline smoothed over one picture pixel.
        let gx = 0.5 * (amount_at(px + vec2<i32>(1, 0)) - amount_at(px - vec2<i32>(1, 0)));
        let gy = 0.5 * (amount_at(px + vec2<i32>(0, 1)) - amount_at(px - vec2<i32>(0, 1)));
        let slope = length(vec2<f32>(gx, gy));
        // How far inside the outline, picture px (large inside a body, where the slope is small).
        let inside = select(select(-1e4, 1e4, a >= level), (a - level) / slope, slope > 1e-6);
        let alpha = clamp(0.5 + inside, 0.0, 1.0);
        if alpha > 0.0 {
            let s1 = textureLoad(amount1, px, 0);
            let own = s0.rgb / s0.a;
            let deep = s1.rgb / s0.a;
            let clarity = s1.a / s0.a * CLARITY_SCALE;
            var body = mix(own, deep, absorbed(at, clarity));
            // A thin lighter rim along the surface, not along the rock it rests on: the liquid's own
            // Colour a little lightened, so the edge of a hole deep in a pool is lit water, not grey.
            let rim_w = u.widths.x;
            let off_rock = smoothstep(rim_w, 2.0 * rim_w, far * u.view.z);
            let rim = (1.0 - smoothstep(0.0, rim_w, inside)) * off_rock;
            body = mix(body, own + (vec3<f32>(1.0) - own) * RIM_LIGHT, rim);
            colour = mix(colour, body, alpha);
        }
    }

    // The rock on top: its edge smoothed, darker just inside its face.
    let cover = rock_cover(far);
    if cover > 0.0 {
        let deep_in = -far * u.view.z;
        let rock = u.rock.rgb * (1.0 - ROCK_DARK * (1.0 - smoothstep(0.0, u.widths.y, deep_in)));
        colour = mix(colour, rock, cover);
    }

    // A fixed dither of under half a step (4 x 4 Bayer), so long ramps don't band.
    var bayer = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    let d = (bayer[(px.y & 3) * 4 + (px.x & 3)] + 0.5) / 16.0 - 0.5;
    let out = srgb_of(colour) + d * u.widths.w;
    return vec4<f32>(clamp(out, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}

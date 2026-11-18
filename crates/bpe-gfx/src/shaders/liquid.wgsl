// Liquid (particles part 6): blobs splatted into a field, then the field cut at a level into a
// surface and shaded. See liquid.rs.

struct Globals {
    // x: 2 / video width, y: 2 / video height, z: world units per output pixel, w: time
    view: vec4<f32>,
    bg_paint: vec4<f32>,
    bg_place: vec4<f32>,
    post: vec4<f32>,
    // centre xy, zoom, turn
    camera: vec4<f32>,
    bg: vec4<f32>,
    bg_from: vec4<f32>,
    bg_to: vec4<f32>,
    // x: the light pass (0 the picture, 1 lit colour, 2 surface, 3 light given off, 5 UV,
    // 6 tags, -1 the picture behind glass)
    light: vec4<f32>,
};

struct Style {
    // level, edge softness, roundness, surface code
    a: vec4<f32>,
    // light angle, shine, bend, tint
    b: vec4<f32>,
    // rim, opacity
    c: vec4<f32>,
    sky_top: vec4<f32>,
    sky_bottom: vec4<f32>,
    // Shaded by the real lights (W15): Lit (0: its own light), surface stamp (index, + 4096 when
    // see-through), how much it blocks light.
    d: vec4<f32>,
    // Drops on the card (liquid part 1): the grid's origin xy, its cell, each drop's reach.
    e: vec4<f32>,
    // Drops on the card: Swirl detail, colours from the colour weights (1).
    f: vec4<f32>,
    // Drops on the card: the one colour.
    g: vec4<f32>,
    // Liquid part 3 (a Liquid thing's full look; all 0 for the Particles Liquid layer, which then
    // draws as before): Clarity (world units), Light soaks in, Reflects, Rainbow split.
    h: vec4<f32>,
    // Deep colour (linear, multiplied), outline width (world units).
    i: vec4<f32>,
    // Ripples, ripple size (world units), Thin film, film size (world units).
    j: vec4<f32>,
    // Gives off light, Mixing shows, shows as (0 shimmer, 1 glow, 2 foam), 0.
    k: vec4<f32>,
    // Outline colour, 0.
    l: vec4<f32>,
    // Foam: amount, from speed (world units a second), splashes foam, on top (world units).
    m: vec4<f32>,
    // Foam colour, froth size (world units).
    n: vec4<f32>,
    // Bubbles: amount, size, size variety, rise speed (world units a second).
    o: vec4<f32>,
    // Bubbles: wobble, shine; Caustics: amount, size.
    p: vec4<f32>,
    // Caustics: speed, deep (world units); Things inside: share, size.
    q: vec4<f32>,
    // Things inside: one colour, 1 to use it (else each drop's own).
    r: vec4<f32>,
    // Things inside: sparkle; Mist: amount, size; 0.
    s: vec4<f32>,
    // Caustics colour, 0.
    t: vec4<f32>,
    // Wet walls: amount, the wet grid's dry floor (exp(-dries / 6 s)), streaks, 0.
    u: vec4<f32>,
    // The wet grid's columns, rows and newest half (the renderer fills these), 1 for a full look.
    v: vec4<f32>,
    // Wet walls' colour, 0.
    w: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var<uniform> st: Style;
@group(1) @binding(1) var field: texture_2d<f32>;
@group(1) @binding(2) var behind: texture_2d<f32>;
@group(1) @binding(3) var smp: sampler;
// A Liquid thing's colour weights: what its field's colour is over (r), and its drops' foam (g),
// speed (b, world units a second / 1000) and kick (a), each times its weight.
@group(1) @binding(4) var weights: texture_2d<f32>;
// Liquid fixes part B: how far below the top each coarse texel is (world units, `fs_depth`), a
// DEPTH_DIV-th of the field's size each way.
@group(1) @binding(6) var depth_map: texture_2d<f32>;
// The field's mean round each coarse texel (`fs_coarse`), which `fs_depth` marches.
@group(1) @binding(7) var coarse: texture_2d<f32>;

// How many field pixels a coarse texel is across.
const DEPTH_DIV: f32 = 4.0;

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

struct BlobIn {
    // centre xy, radius, weight
    @location(0) place: vec4<f32>,
    @location(1) colour: vec4<f32>,
};

struct BlobOut {
    @builtin(position) pos: vec4<f32>,
    // Where in the blob, its radius 1.
    @location(0) at: vec2<f32>,
    @location(1) colour: vec4<f32>,
};

@vertex
fn vs_blob(@builtin(vertex_index) vi: u32, b: BlobIn) -> BlobOut {
    var out: BlobOut;
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u)) * 2.0 - 1.0;
    let r = max(b.place.z, 0.0);
    out.pos = to_clip(b.place.xy + corner * r);
    out.at = corner;
    out.colour = vec4<f32>(b.colour.rgb, max(b.place.w, 0.0));
    return out;
}

@fragment
fn fs_blob(in: BlobOut) -> @location(0) vec4<f32> {
    let d2 = dot(in.at, in.at);
    if d2 >= 1.0 {
        discard;
    }
    let k = 1.0 - d2;
    let w = k * k * k * in.colour.a;
    return vec4<f32>(in.colour.rgb * w, w);
}

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(vi & 1u) * 4 - 1);
    let y = f32(i32(vi & 2u) * 2 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

// The liquid's colour at pixel `px`, from its field `f`: over the colour weights for a Liquid
// thing's drops, else over the field's weight.
fn colour_of(f: vec4<f32>, px: vec2<i32>) -> vec3<f32> {
    if st.f.y > 0.5 {
        let w = textureLoad(weights, px, 0).r;
        return f.rgb / max(w, 1e-5);
    }
    return f.rgb / max(f.a, 1e-5);
}

fn density(p: vec2<i32>, size: vec2<i32>) -> f32 {
    return textureLoad(field, clamp(p, vec2<i32>(0), size - 1), 0).a;
}

const MATTE: i32 = 0;
const PLASTIC: i32 = 1;
const CHROME: i32 = 2;
const METAL: i32 = 3;
const GLASS: i32 = 4;
const TOON: i32 = 5;

@fragment
fn fs_surface(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(field));
    let px = vec2<i32>(pos.xy);
    let f = textureLoad(field, px, 0);
    let level = max(st.a.x, 1e-4);
    let here = f.a;
    // Far outside: nothing (the cheap way out for most of the picture).
    if here < level * 0.25 {
        return vec4<f32>(0.0);
    }
    let grad = vec2<f32>(
        density(px + vec2<i32>(1, 0), size) - density(px - vec2<i32>(1, 0), size),
        density(px + vec2<i32>(0, 1), size) - density(px - vec2<i32>(0, 1), size),
    ) * 0.5;
    let slope = length(grad);
    // World units a picture pixel covers.
    let unit = max(g.view.z, 1e-4);
    // How far inside the cut, in pixels (the field's slope tells how fast it rises).
    var inside = (here - level) / max(slope, 1e-5);
    // Liquid fixes part B: the ripples ruffle the waterline (where the edge faces up).
    if st.v.w > 0.5 && st.j.x > 0.0 && st.j.y > 0.0 && slope > 1e-6 {
        let w = g.camera.w;
        let facing = clamp(dot(-grad / slope, vec2<f32>(-sin(w), -cos(w))), 0.0, 1.0);
        // Only near the top (not the lower rims of holes deep in it).
        let top = 1.0 - smoothstep(st.j.y, 3.0 * st.j.y, depth_map_at(pos.xy));
        if facing * top > 0.0 {
            let world = world_of(pos.xy, vec2<f32>(size));
            inside += ripple_height(world, g.view.w) / max(g.view.z, 1e-4) * facing * top;
        }
    }
    let soft = max(st.a.y / unit, 0.75);
    var cover = clamp(0.5 + inside / soft, 0.0, 1.0);
    cover = cover * cover * (3.0 - 2.0 * cover);
    if cover <= 0.0 {
        return vec4<f32>(0.0);
    }
    let shown = cover * clamp(st.c.y, 0.0, 1.0);
    let code = i32(st.a.w + 0.5);
    let mode = light_pass(g.light.x);
    if st.v.w > 0.5 {
        return full_surface(pos.xy, px, f, grad, slope, inside, unit, shown, code, mode);
    }
    if st.d.x > 0.0 {
        return real_lit(colour_of(f, px), vec3<f32>(0.0), 0.0, shown, inside, unit, mode);
    }
    // The light passes: covering black (lighting leaves it as drawn), nothing in the surface
    // picture (it blocks no light), whole where it covers in the tags.
    if mode >= PASS_LIT {
        // Light pass 9, the glass picture (glass and light part 3): glass that bends lamp light
        // is free glass there, its index of refraction over its cover.
        if mode >= PASS_GLASS {
            if st.c.z > 1.0001 {
                return vec4<f32>(st.c.z * shown, 0.0, 0.0, shown);
            }
            return vec4<f32>(0.0);
        }
        // Nothing in the surface or the tint picture (pass 8): it blocks no light.
        if mode == PASS_SURFACE || mode >= PASS_TINT {
            return vec4<f32>(0.0);
        }
        if mode >= PASS_TAGS {
            return select(vec4<f32>(0.0), vec4<f32>(0.0, 0.0, 0.0, 1.0), shown >= 0.5);
        }
        return vec4<f32>(0.0, 0.0, 0.0, shown);
    }
    // Glass is see-through: left out of the picture behind glass.
    if mode <= PASS_BEHIND && code == GLASS {
        return vec4<f32>(0.0);
    }
    let colour = colour_of(f, px);
    // The normal: the edge rounds over within Roundness, flat inside it.
    let round_px = max(st.a.z / unit, 0.5);
    let x = clamp(max(inside, 0.0) / round_px, 0.0, 1.0);
    let out = select(vec2<f32>(0.0), -grad / slope, slope > 1e-6);
    let tilt = 1.0 - x;
    let n = normalize(vec3<f32>(out * tilt, sqrt(max(1.0 - tilt * tilt, 0.0)) + 1e-3));
    // The light: the angle on the screen (y down), raised towards the viewer.
    let angle = st.b.x;
    let l = normalize(vec3<f32>(cos(angle), sin(angle), 1.2));
    let v = vec3<f32>(0.0, 0.0, 1.0);
    let h = normalize(l + v);
    let shine = clamp(st.b.y, 0.0, 1.0);
    let lambert = max(dot(n, l), 0.0);
    let diffuse = 0.25 + 0.75 * lambert;
    let spec = pow(max(dot(n, h), 0.0), 8.0 + 120.0 * shine) * shine;
    let rim = clamp(st.c.x, 0.0, 1.0) * pow(1.0 - n.z, 2.0);
    // The sky Chrome and Metal reflect: top above (screen up is -y).
    let r = reflect(-v, n);
    let sky = mix(st.sky_bottom.rgb, st.sky_top.rgb, clamp(0.5 - r.y * 0.5 + r.z * 0.25, 0.0, 1.0));
    let fresnel = 0.04 + 0.96 * pow(1.0 - n.z, 5.0);
    var c = colour * diffuse;
    switch code {
        case MATTE: {
            c = colour * diffuse;
        }
        case PLASTIC: {
            c = colour * diffuse + vec3<f32>(spec * 1.5) + sky * fresnel * 0.3;
        }
        case CHROME: {
            c = sky * mix(vec3<f32>(1.0), colour, 0.2) + vec3<f32>(spec * 3.0);
        }
        case METAL: {
            c = sky * colour * (0.6 + 0.4 * lambert) + colour * spec * 3.0;
        }
        case GLASS: {
            let dims = vec2<f32>(textureDimensions(behind));
            let bend = n.xy * st.b.z / unit;
            let uv = (pos.xy + bend) / dims;
            let seen = textureSampleLevel(behind, smp, uv, 0.0).rgb;
            let tint = clamp(st.b.w, 0.0, 1.0);
            c = mix(seen, seen * colour, tint) + vec3<f32>(spec * 2.0) + sky * fresnel * 0.6;
        }
        case TOON: {
            let band = select(select(0.35, 0.7, lambert > 0.25), 1.0, lambert > 0.7);
            c = colour * band + vec3<f32>(step(0.5, spec) * shine);
        }
        default: {}
    }
    c = c + vec3<f32>(rim);
    return vec4<f32>(max(c, vec3<f32>(0.0)) * shown, shown);
}

// A liquid the real lights shade (W15), `colour` its colour, `shown` how much of it shows here,
// `inside` how far in from its edge (pixels) and `unit` the world units a pixel covers: its plain
// colour in the picture, and in the light passes its colour × Lit, its surface (how much it
// blocks, its height, its Surface's table index) and the rest as a pretend liquid's, so the
// lighting mix lights and shades it as it does a ball.
//
// Liquid part 3: `glow` is the light it gives off (added to the picture and written to the
// emission pass) and `bump` its ripples' height (world units) on top of the rim's.
fn real_lit(colour: vec3<f32>, glow: vec3<f32>, bump: f32, shown: f32, inside: f32, unit: f32, mode: i32) -> vec4<f32> {
    if mode >= PASS_LIT {
        // Nothing in the glass picture (its glass bends lamp light through its surface, as a
        // shape's does); in the tint picture (pass 8) its colour's depth where it blocks light,
        // as a sprite's (tint.wgsl).
        if mode >= PASS_GLASS {
            return vec4<f32>(0.0);
        }
        if mode >= PASS_TINT {
            return tint_out(colour, shown, st.d.z, 1.0, 0.0);
        }
        if mode <= PASS_LIT {
            return vec4<f32>(max(colour, vec3<f32>(0.0)) * st.d.x * shown, shown);
        }
        if mode <= PASS_SURFACE {
            // The height: the edge rounds over within Roundness (a quarter circle), flat inside,
            // the same shape the pretend light's normal has.
            let round_px = max(st.a.z / unit, 0.5);
            let x = clamp(max(inside, 0.0) / round_px, 0.0, 1.0);
            let h = st.a.z * sqrt(max(1.0 - (1.0 - x) * (1.0 - x), 0.0)) + bump;
            // A Stage takes what blocks whole, as a ball's layers do.
            var blocks = clamp(st.d.z, 0.0, 1.0);
            if g.light.z > 0.5 {
                blocks = select(0.0, 1.0, blocks > 0.0);
            }
            let index = st.d.y % 4096.0;
            return vec4<f32>(blocks, h, index, 1.0) * shown;
        }
        if mode >= PASS_TAGS {
            // The tags: whole where it covers half or more, with its thing's link number.
            return select(vec4<f32>(0.0), vec4<f32>(0.0, st.d.w, 0.0, 1.0), shown >= 0.5);
        }
        if mode == PASS_EMISSION {
            return vec4<f32>(max(glow, vec3<f32>(0.0)) * shown, shown);
        }
        return vec4<f32>(0.0, 0.0, 0.0, shown);
    }
    // See-through: left out of the picture behind glass, which the mix shows through it.
    if mode <= PASS_BEHIND && st.d.y >= 4095.5 {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(max(colour + glow, vec3<f32>(0.0)) * shown, shown);
}

// ---- Liquid part 3: a Liquid thing's full look ---------------------------------------------

fn pcg(v: u32) -> u32 {
    let s = v * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

// A number 0 to 1 for cell `i` and salt `k`.
fn hash_cell(i: vec2<f32>, k: u32) -> f32 {
    let a = bitcast<u32>(i32(i.x));
    let b = bitcast<u32>(i32(i.y));
    return f32(pcg(a ^ pcg(b ^ pcg(k))) >> 8u) / 16777216.0;
}

// Smooth value noise, 0 to 1.
fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash_cell(i, 7u);
    let b = hash_cell(i + vec2<f32>(1.0, 0.0), 7u);
    let c = hash_cell(i + vec2<f32>(0.0, 1.0), 7u);
    let d = hash_cell(i + vec2<f32>(1.0, 1.0), 7u);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// The world point at picture pixel `px` (to_clip backwards).
fn world_of(px: vec2<f32>, dims: vec2<f32>) -> vec2<f32> {
    let ndc = vec2<f32>(px.x / dims.x * 2.0 - 1.0, 1.0 - px.y / dims.y * 2.0);
    let q = vec2<f32>(ndc.x / g.view.x, -ndc.y / g.view.y);
    var d = q / max(g.camera.z, 1e-6);
    let w = g.camera.w;
    if w != 0.0 {
        let s = sin(w);
        let k = cos(w);
        d = vec2<f32>(d.x * k - d.y * s, d.x * s + d.y * k);
    }
    return g.camera.xy + d;
}

// How far below the liquid's top pixel `px` is (world units), looking up to `range` world units.
fn depth_below(px: vec2<i32>, size: vec2<i32>, level: f32, unit: f32, range: f32) -> f32 {
    let w = g.camera.w;
    let up = vec2<f32>(-sin(w), -cos(w));
    let reach = range / unit;
    let step = reach / 24.0;
    for (var k = 1; k <= 24; k++) {
        let at = vec2<f32>(px) + up * step * f32(k);
        let q = vec2<i32>(round(at));
        if q.y < 0 || q.x < 0 || q.x >= size.x {
            return range;
        }
        let here = density(q, size);
        if here < level {
            // Between the last step and this one: halve the gap down to a pixel (a straight guess
            // between the two is off by up to a step, and by an amount that repeats every step,
            // which showed as stripes along the top).
            var a = f32(k - 1) * step;
            var b = f32(k) * step;
            for (var i = 0; i < 8; i++) {
                if b - a <= 0.5 {
                    break;
                }
                let m = 0.5 * (a + b);
                let qm = vec2<i32>(round(vec2<f32>(px) + up * m));
                if density(qm, size) < level {
                    b = m;
                } else {
                    a = m;
                }
            }
            return 0.5 * (a + b) * unit;
        }
    }
    return range;
}

// How far in from the edge pixel `px` is (pixels), from `near` (the field's height over the level
// by its slope, which only holds near the edge: drops resting in rows leave dips between the rows
// where it comes out small, which showed as stripes). Further in, the most of 3, 6, 12 and 24 px
// that has the field over the level all round it, in eight directions.
fn inside_far(px: vec2<i32>, size: vec2<i32>, level: f32, near: f32) -> f32 {
    var best = near;
    var r = 3.0;
    for (var k = 0; k < 4; k++) {
        if r <= best {
            r = r * 2.0;
            continue;
        }
        var all_in = true;
        for (var d = 0; d < 8; d++) {
            let a = f32(d) * 0.7853982;
            let q = px + vec2<i32>(round(vec2<f32>(cos(a), sin(a)) * r));
            if q.x < 0 || q.y < 0 || q.x >= size.x || q.y >= size.y || density(q, size) < level {
                all_in = false;
                break;
            }
        }
        if !all_in {
            break;
        }
        best = r;
        r = r * 2.0;
    }
    return best;
}

// Ridged noise for the caustics' bright lines.
fn ridge(p: vec2<f32>) -> f32 {
    return 1.0 - abs(vnoise(p) * 2.0 - 1.0);
}

// The caustics' brightness at world point `p` at time `t`.
fn caustic(p: vec2<f32>, t: f32) -> f32 {
    let size = max(st.p.w, 1.0);
    let speed = st.q.x;
    let warp = vec2<f32>(vnoise(p / size * 0.7 + vec2<f32>(t * speed * 0.2, 0.0)), vnoise(p / size * 0.7 + 3.1)) - 0.5;
    let a = ridge(p / size + warp + vec2<f32>(t * speed * 0.31, t * speed * 0.17));
    let b = ridge(p / size * 1.37 - warp + vec2<f32>(-t * speed * 0.23, t * speed * 0.29) + 5.7);
    return pow(a * b, 8.0) * 1.6;
}

// A bubble at world point `p` `depth` below the top at time `t`: its brightness and its cover.
fn bubble(p: vec2<f32>, depth: f32, t: f32) -> vec2<f32> {
    let size = max(st.o.y, 0.5);
    let cell = 4.0 * size;
    let q = p + vec2<f32>(0.0, t * st.o.w);
    let ci = floor(q / cell);
    let h = hash_cell(ci, 11u);
    if h >= clamp(st.o.x, 0.0, 1.0) {
        return vec2<f32>(0.0);
    }
    var c = (ci + vec2<f32>(hash_cell(ci, 12u), hash_cell(ci, 13u)) * 0.5 + 0.25) * cell;
    c.x += clamp(st.p.x, 0.0, 1.0) * size * sin(t * 3.0 + h * 40.0);
    let r = size * (1.0 - clamp(st.o.z, 0.0, 1.0) * hash_cell(ci, 14u) * 0.7);
    let d = length(q - c) / r;
    if d > 1.05 {
        return vec2<f32>(0.0);
    }
    // Popping at the top: gone within its own size of it.
    let pop = smoothstep(r * 0.5, r * 2.5, depth);
    let ring = smoothstep(1.0, 0.82, d) * smoothstep(0.5, 0.95, d);
    let spot = smoothstep(0.4, 0.0, length(q - c + vec2<f32>(0.35, 0.35) * r) / r) * clamp(st.p.y, 0.0, 2.0);
    let cover = smoothstep(1.0, 0.9, d);
    return vec2<f32>((ring * 0.8 + spot * 1.5) * pop, cover * pop);
}

// A thin film's rainbow at world point `p`, `depth` world units below the top, at time `t`: rings
// along the top within 40 pixels of it.
fn film_colour(p: vec2<f32>, depth: f32, t: f32) -> vec3<f32> {
    let size = max(st.j.w, 1.0);
    let k = vnoise(p / size + vec2<f32>(t * 0.07, t * 0.05)) * 1.6 + smoothstep(0.0, 40.0 * max(g.view.z, 1e-4), depth) * 0.5 + t * 0.05;
    return 0.5 + 0.5 * cos(6.2832 * (k + vec3<f32>(0.0, 0.33, 0.67)));
}

// The ripples' height (world units, about -0.75 to 0.75 x Ripples x ripple size x 0.15) at world
// point `p` at time `t`: two octaves of value noise drifting slowly (liquid fixes part B: no longer
// by the flow's speed, which slid a stream's ripples past the rest as a band).
fn ripple_noise(p: vec2<f32>, t: f32) -> f32 {
    let q = p / max(st.j.y, 1.0) + vec2<f32>(t * 0.35, t * 0.22);
    return vnoise(q) + 0.5 * vnoise(q * 2.1 + 7.3);
}

fn ripple_height(p: vec2<f32>, t: f32) -> f32 {
    return (ripple_noise(p, t) - 0.75) * st.j.x * st.j.y * 0.15;
}

// How far below the top anything in a full look looks (world units).
fn depth_range() -> f32 {
    var range = max(3.0 * max(st.h.x, 0.0), 32.0);
    if st.p.z > 0.0 {
        range = max(range, st.q.y);
    }
    if st.m.x > 0.0 {
        range = max(range, 2.0 * st.m.w);
    }
    if st.o.x > 0.0 {
        range = max(range, 4.0 * st.o.y);
    }
    return min(range, 4000.0);
}

// Liquid fixes part B, depth without artifacts. First the field's mean over 13 x 13 pixels round
// each coarse texel (every other pixel): gaps in a splash, thin spikes and holes under ~8 px wide
// and rows of resting drops all smooth away.
@fragment
fn fs_coarse(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(field));
    let mid = vec2<i32>(floor(pos.xy * DEPTH_DIV));
    var sum = 0.0;
    for (var y = -6; y <= 6; y += 2) {
        for (var x = -6; x <= 6; x += 2) {
            sum += density(mid + vec2<i32>(x, y), size);
        }
    }
    return vec4<f32>(sum / 49.0, 0.0, 0.0, 1.0);
}

// Then how far below the top each coarse texel is: up the screen's world up a texel a step through
// the smooth coarse field until it falls under the level (between the last two samples, where the
// smooth field crosses it), up to the range (also where the picture's top is passed).
@fragment
fn fs_depth(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(coarse));
    let level = max(st.a.x, 1e-4);
    let range = depth_range();
    // A coarse texel a step, or longer when the range is longer than 256 of them.
    let per = max(max(g.view.z, 1e-4) * DEPTH_DIV, range / 256.0);
    let texels = per / (max(g.view.z, 1e-4) * DEPTH_DIV);
    let w = g.camera.w;
    let up = vec2<f32>(-sin(w), -cos(w));
    var before = textureSampleLevel(coarse, smp, pos.xy / dims, 0.0).r;
    if before < level {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let steps = min(i32(ceil(range / per)), 256);
    for (var k = 1; k <= steps; k++) {
        let at = pos.xy + up * f32(k) * texels;
        if at.y < 0.0 || at.x < 0.0 || at.x > dims.x {
            break;
        }
        let here = textureSampleLevel(coarse, smp, at / dims, 0.0).r;
        if here < level {
            let f = clamp((before - level) / max(before - here, 1e-6), 0.0, 1.0);
            return vec4<f32>(min((f32(k - 1) + f) * per, range), 0.0, 0.0, 1.0);
        }
        before = here;
    }
    return vec4<f32>(range, 0.0, 0.0, 1.0);
}

// The depth map at picture point `xy` (world units), between the coarse texels round it that are
// inside the body (those outside hold 0, which would pull the depth down along its sides).
fn depth_map_at(xy: vec2<f32>) -> f32 {
    let size = vec2<i32>(textureDimensions(depth_map));
    let q = xy / DEPTH_DIV - 0.5;
    let i = floor(q);
    let f = q - i;
    var sum = 0.0;
    var wsum = 0.0;
    for (var k = 0; k < 4; k++) {
        let o = vec2<i32>(k & 1, k >> 1u);
        let at = clamp(vec2<i32>(i) + o, vec2<i32>(0), size - 1);
        let d = textureLoad(depth_map, at, 0).r;
        let w = select(1.0 - f.x, f.x, o.x == 1) * select(1.0 - f.y, f.y, o.y == 1);
        if d > 0.0 {
            sum += d * w;
            wsum += w;
        }
    }
    return select(0.0, sum / max(wsum, 1e-6), wsum > 1e-4);
}

// A Liquid thing's surface (liquid part 3): its colour with depth, see-through and bent, its own
// pretend light or the real lights, ripples, thin film, foam, bubbles, caustics, outline, light
// it gives off and where colours are mixing.
fn full_surface(
    xy: vec2<f32>,
    px: vec2<i32>,
    f: vec4<f32>,
    grad: vec2<f32>,
    slope: f32,
    near_edge: f32,
    unit: f32,
    shown: f32,
    code: i32,
    mode: i32,
) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(field));
    let dims = vec2<f32>(size);
    let level = max(st.a.x, 1e-4);
    let inside = inside_far(px, size, level, near_edge);
    let t = g.view.w;
    let world = world_of(xy, dims);
    let real = st.d.x > 0.0;
    let wt = textureLoad(weights, px, 0);
    let colour = colour_of(f, px);
    let clarity = max(st.h.x, 0.0);
    // How far below the top (liquid fixes part B): from the smooth coarse depth, and within 24 px
    // of the top from the exact march up the field itself (foam's On top, the film's rings and
    // bubbles popping need the top to the pixel). Deep down only the smooth one counts, so holes,
    // gaps and rows of drops make no rings, columns or bands.
    var depth = depth_map_at(xy);
    let near = 24.0 * unit;
    if depth < near {
        let fine = depth_below(px, size, level, unit, near);
        depth = mix(fine, depth, smoothstep(0.5 * near, near, depth));
    }

    // The normal: the edge rounds over within Roundness, flat inside it, with ripples on top.
    let round_px = max(st.a.z / unit, 0.5);
    let x = clamp(max(inside, 0.0) / round_px, 0.0, 1.0);
    let out = select(vec2<f32>(0.0), -grad / slope, slope > 1e-6);
    let tilt = 1.0 - x;
    var bump = vec2<f32>(0.0);
    var bump_h = 0.0;
    // Ripples (liquid fixes part B): on the top only, gone within two ripple sizes of it (all over
    // the body they read as orange peel), a little stronger where it flows fast.
    let top = 1.0 - smoothstep(0.5 * st.j.y, 2.0 * st.j.y, depth);
    if st.j.x > 0.0 && st.j.y > 0.0 && top > 0.0 {
        let s = st.j.y;
        let flow = wt.b / max(f.a, 1e-5) * 1000.0;
        let k = st.j.x * top * (1.0 + 0.5 * clamp(flow / 400.0, 0.0, 1.0));
        let e = 0.15 * s;
        let h0 = ripple_noise(world, t);
        let hx = ripple_noise(world + vec2<f32>(e, 0.0), t);
        let hy = ripple_noise(world + vec2<f32>(0.0, e), t);
        bump = vec2<f32>(hx - h0, hy - h0) / 0.15 * k * 0.2;
        // For the real lights the top's ripples are steeper within a ripple size of the top, so
        // their slopes catch a lamp's light as glints (the height is all the lights see of them).
        let crest = 1.0 + 4.0 * (1.0 - smoothstep(0.0, s, depth));
        bump_h = (h0 - 0.75) * k * s * 0.15 * crest;
    }
    let n = normalize(vec3<f32>(out * tilt + bump, sqrt(max(1.0 - tilt * tilt, 0.0)) + 1e-3));

    // Depth: shallow shows through, deep takes the Deep colour.
    var see = 0.0;
    var through = 1.0;
    if clarity > 0.0 {
        through = exp(-depth / clarity);
        see = 0.85;
    }
    var body = colour * mix(vec3<f32>(1.0), max(st.i.rgb, vec3<f32>(0.0)), 1.0 - through);
    let behind_too = !real && (code == GLASS || (st.b.z > 0.0 && clarity > 0.0));
    // The light passes draw it flat: its own look only in the picture.
    let lighting_pass = mode >= PASS_LIT;

    // Its own pretend light (a lit scene's real lights replace it).
    let angle = st.b.x;
    let l = normalize(vec3<f32>(cos(angle), sin(angle), 1.2));
    let v = vec3<f32>(0.0, 0.0, 1.0);
    let hv = normalize(l + v);
    let shine = clamp(st.b.y, 0.0, 1.0);
    let lambert = max(dot(n, l), 0.0);
    let spec = pow(max(dot(n, hv), 0.0), 8.0 + 120.0 * shine) * shine;
    let fresnel = 0.04 + 0.96 * pow(1.0 - n.z, 5.0);
    let r = reflect(-v, n);
    let sky = mix(st.sky_bottom.rgb, st.sky_top.rgb, clamp(0.5 - r.y * 0.5 + r.z * 0.25, 0.0, 1.0));
    let reflects = max(st.h.z, 0.0);

    var c = body;
    var alpha = shown * (1.0 - see * through);
    if !real {
        let diffuse = 0.25 + 0.75 * lambert;
        switch code {
            case MATTE: {
                c = body * diffuse;
            }
            case PLASTIC: {
                c = body * diffuse + vec3<f32>(spec * 1.5) + sky * fresnel * reflects;
            }
            case CHROME: {
                c = sky * mix(vec3<f32>(1.0), body, 0.2) + vec3<f32>(spec * 3.0);
            }
            case METAL: {
                c = sky * body * (0.6 + 0.4 * lambert) + body * spec * 3.0;
            }
            case GLASS: {
                c = body + vec3<f32>(spec * 2.0) + sky * fresnel * reflects * 2.0;
            }
            case TOON: {
                let band = select(select(0.35, 0.7, lambert > 0.25), 1.0, lambert > 0.7);
                c = body * band + vec3<f32>(step(0.5, spec) * shine);
            }
            default: {}
        }
        c = c + vec3<f32>(clamp(st.c.x, 0.0, 1.0) * pow(1.0 - n.z, 2.0));
    }
    // What is behind, bent (and split into a rainbow) through it.
    if behind_too && mode == PASS_PICTURE {
        let bdims = vec2<f32>(textureDimensions(behind));
        let bend = (n.xy) * st.b.z / unit;
        let split = clamp(st.h.w, 0.0, 1.0);
        let uv = (xy + bend) / bdims;
        var seen = textureSampleLevel(behind, smp, uv, 0.0).rgb;
        if split > 0.0 {
            seen.r = textureSampleLevel(behind, smp, (xy + bend * (1.0 + split)) / bdims, 0.0).r;
            seen.b = textureSampleLevel(behind, smp, (xy + bend * (1.0 - split)) / bdims, 0.0).b;
        }
        var cover = 1.0 - see * through;
        if code == GLASS {
            seen = mix(seen, seen * colour, clamp(st.b.w, 0.0, 1.0));
            if clarity <= 0.0 {
                cover = 0.0;
            }
            c = mix(seen, body, cover) + vec3<f32>(spec * 2.0) + sky * fresnel * reflects * 2.0;
        } else {
            c = mix(seen * mix(vec3<f32>(1.0), colour, 0.5), c, cover);
        }
        alpha = shown;
    }
    var glow = vec3<f32>(0.0);

    // Light soaks in: soft bright edges.
    let soak = clamp(st.h.y, 0.0, 2.0);
    if soak > 0.0 {
        let s = colour * soak * (1.0 - x) * 0.8;
        c += s;
        glow += s * 0.5;
    }
    // Thin film: a rainbow sliding over it.
    if st.j.z > 0.0 {
        let film = clamp(st.j.z, 0.0, 1.0) * (0.35 + 0.65 * (1.0 - n.z));
        c = mix(c, c * 0.5 + film_colour(world, depth, t) * 0.8, film);
    }
    // Caustics: bright moving ripples, strongest near the top.
    if st.p.z > 0.0 {
        let k = caustic(world, t) * st.p.z * exp(-depth / max(st.q.y, 1.0)) * smoothstep(0.0, 6.0 * unit, depth);
        glow += st.t.rgb * k;
    }
    // Bubbles rising and popping at the top.
    if st.o.x > 0.0 {
        let b = bubble(world, depth, t);
        let tone = mix(colour, vec3<f32>(1.0), 0.6);
        c = mix(c, tone * (0.6 + 0.4 * lambert), b.y * 0.35) + vec3<f32>(b.x * 0.6);
        alpha = max(alpha, b.y * shown * 0.6);
    }
    // White water and foam: where its drops are fast or splashed, near the top.
    if st.m.x > 0.0 {
        let foamy = clamp(wt.g / max(f.a, 1e-5), 0.0, 1.0);
        let top = 1.0 - smoothstep(0.0, max(st.m.w, 1.0), depth);
        let froth = smoothstep(0.25, 0.75, vnoise(world / max(st.n.w, 0.5)) * 0.6 + foamy * 0.7);
        let k = clamp(st.m.x * foamy * mix(0.35, 1.0, top) * (0.5 + 0.5 * froth), 0.0, 1.0);
        c = mix(c, st.n.rgb * (0.75 + 0.25 * lambert), k);
        alpha = max(alpha, k * shown);
    }
    // Where colours are mixing: a shimmer, a glow or foam.
    if st.k.y > 0.0 {
        let fx0 = textureLoad(field, clamp(px + vec2<i32>(3, 0), vec2<i32>(0), size - 1), 0);
        let fx1 = textureLoad(field, clamp(px - vec2<i32>(3, 0), vec2<i32>(0), size - 1), 0);
        let fy0 = textureLoad(field, clamp(px + vec2<i32>(0, 3), vec2<i32>(0), size - 1), 0);
        let fy1 = textureLoad(field, clamp(px - vec2<i32>(0, 3), vec2<i32>(0), size - 1), 0);
        let dx = colour_of(fx0, clamp(px + vec2<i32>(3, 0), vec2<i32>(0), size - 1))
            - colour_of(fx1, clamp(px - vec2<i32>(3, 0), vec2<i32>(0), size - 1));
        let dy = colour_of(fy0, clamp(px + vec2<i32>(0, 3), vec2<i32>(0), size - 1))
            - colour_of(fy1, clamp(px - vec2<i32>(0, 3), vec2<i32>(0), size - 1));
        let m = clamp((length(dx) + length(dy)) * 2.0, 0.0, 1.0) * st.k.y * step(4.0, inside);
        let as_ = i32(st.k.z + 0.5);
        if as_ == 1 {
            glow += colour * m * 1.5;
        } else if as_ == 2 {
            c = mix(c, vec3<f32>(1.0), m * 0.7);
        } else {
            let sparkle = pow(vnoise(world / 3.0 + vec2<f32>(t * 3.0, -t * 2.0)), 4.0);
            c += vec3<f32>(m * sparkle * 2.5);
        }
    }
    // The outline: a band just inside the edge.
    if st.i.w > 0.0 {
        let w = st.i.w / unit;
        let band = 1.0 - smoothstep(w - 0.75, w + 0.75, inside);
        c = mix(c, st.l.rgb, band);
        alpha = max(alpha, band * shown);
    }
    // The light it gives off.
    if st.k.x > 0.0 {
        glow += body * st.k.x;
    }
    if real {
        return real_lit(c, glow, bump_h, alpha, inside, unit, mode);
    }
    if lighting_pass {
        // Its own light: covering black where it covers (lighting leaves it as drawn), nothing in
        // the surface, tint or glass pictures, whole in the tags.
        if mode == PASS_SURFACE || mode >= PASS_TINT {
            return vec4<f32>(0.0);
        }
        if mode >= PASS_TAGS {
            return select(vec4<f32>(0.0), vec4<f32>(0.0, 0.0, 0.0, 1.0), alpha >= 0.5);
        }
        if mode == PASS_EMISSION {
            return vec4<f32>(glow * shown, alpha);
        }
        return vec4<f32>(0.0, 0.0, 0.0, alpha);
    }
    // See-through glass is left out of the picture behind glass.
    if mode <= PASS_BEHIND && code == GLASS {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(max(c, vec3<f32>(0.0)) * alpha + max(glow, vec3<f32>(0.0)) * shown, alpha);
}

// Wet walls (liquid part 3): the wet grid (`wet_grid`, a storage buffer of `4 × cols × rows`
// floats: two halves of wetness, the newest `st.v.z`, then each node's distance from its wall's
// face on the liquid's side) drawn as a film in the wet colour, in streaks, before the liquid
// itself, only within a cell or two of the face (so it follows the wall smoothly).
@group(1) @binding(5) var<storage, read> wet_grid: array<f32>;

fn wet_at(x: i32, y: i32) -> f32 {
    let cols = i32(st.v.x);
    let rows = i32(st.v.y);
    if x < 0 || y < 0 || x >= cols || y >= rows {
        return 0.0;
    }
    let half = u32(st.v.z + 0.5) * u32(cols * rows);
    return wet_grid[half + u32(y * cols + x)];
}

// How far node (x, y) is from its wall's face on the liquid's side (grid units; far off the grid).
fn face_at(x: i32, y: i32) -> f32 {
    let cols = i32(st.v.x);
    let rows = i32(st.v.y);
    if x < 0 || y < 0 || x >= cols || y >= rows {
        return 4.0;
    }
    return wet_grid[2u * u32(cols * rows) + u32(y * cols + x)];
}

@fragment
fn fs_wet(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    if st.u.x <= 0.0 || st.v.x < 1.0 {
        return vec4<f32>(0.0);
    }
    let dims = vec2<f32>(textureDimensions(field));
    let world = world_of(pos.xy, dims);
    let q = (world - st.e.xy) / max(st.e.z, 1e-6);
    let i = floor(q);
    let fr = q - i;
    let x = i32(i.x);
    let y = i32(i.y);
    let wet = mix(mix(wet_at(x, y), wet_at(x + 1, y), fr.x), mix(wet_at(x, y + 1), wet_at(x + 1, y + 1), fr.x), fr.y);
    let face = mix(mix(face_at(x, y), face_at(x + 1, y), fr.x), mix(face_at(x, y + 1), face_at(x + 1, y + 1), fr.x), fr.y);
    let near = smoothstep(-0.2, 0.15, face) * (1.0 - smoothstep(0.9, 2.2, face));
    let floor_ = clamp(st.u.y, 0.0, 0.999);
    let shown = clamp((wet - floor_) / (1.0 - floor_), 0.0, 1.0) * near;
    if shown <= 0.0 {
        return vec4<f32>(0.0);
    }
    let unit = max(g.view.z, 1e-4);
    let streak = 0.45 + 0.55 * vnoise(vec2<f32>(world.x / (6.0 * max(unit, 0.5)), world.y / 90.0));
    let a = clamp(st.u.x * shown * mix(1.0, streak, clamp(st.u.z, 0.0, 1.0)) * 0.6, 0.0, 1.0);
    let colour = max(st.w.rgb, vec3<f32>(0.0));
    let mode = light_pass(g.light.x);
    if mode >= PASS_LIT {
        if st.d.x > 0.0 && mode == PASS_LIT {
            return vec4<f32>(colour * st.d.x * a, a);
        }
        if st.d.x <= 0.0 && mode < PASS_TAGS && mode != PASS_SURFACE {
            return vec4<f32>(0.0, 0.0, 0.0, a);
        }
        return vec4<f32>(0.0);
    }
    return vec4<f32>(colour * a, a);
}

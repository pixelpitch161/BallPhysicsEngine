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
    grade: vec4<f32>,
    grade_more: vec4<f32>,
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
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var<uniform> st: Style;
@group(1) @binding(1) var field: texture_2d<f32>;
@group(1) @binding(2) var behind: texture_2d<f32>;
@group(1) @binding(3) var smp: sampler;

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
    let inside = (here - level) / max(slope, 1e-5);
    let soft = max(st.a.y / unit, 0.75);
    var cover = clamp(0.5 + inside / soft, 0.0, 1.0);
    cover = cover * cover * (3.0 - 2.0 * cover);
    if cover <= 0.0 {
        return vec4<f32>(0.0);
    }
    let shown = cover * clamp(st.c.y, 0.0, 1.0);
    let code = i32(st.a.w + 0.5);
    let mode = light_pass(g.light.x);
    if st.d.x > 0.0 {
        return real_lit(f.rgb / max(here, 1e-5), shown, inside, unit, mode);
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
    let colour = f.rgb / max(here, 1e-5);
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
fn real_lit(colour: vec3<f32>, shown: f32, inside: f32, unit: f32, mode: i32) -> vec4<f32> {
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
            let h = st.a.z * sqrt(max(1.0 - (1.0 - x) * (1.0 - x), 0.0));
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
        return vec4<f32>(0.0, 0.0, 0.0, shown);
    }
    // See-through: left out of the picture behind glass, which the mix shows through it.
    if mode <= PASS_BEHIND && st.d.y >= 4095.5 {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(max(colour, vec3<f32>(0.0)) * shown, shown);
}

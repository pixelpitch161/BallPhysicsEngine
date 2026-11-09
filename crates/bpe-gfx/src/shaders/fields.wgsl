// Soft fields of particles (Soft smoke, Flame): pasted after sprites.wgsl and each.wgsl, so it
// shares their Globals, to_clip, light_flat and each_out. Each field is first splatted into a
// picture of its own at half size (vs_splat/fs_splat, added up: rgb the colour times the weight,
// a the weight), then shaded where it sits among its list's sprites by one triangle over the
// whole picture (vs_field/fs_field). See `Field` in frame.rs.

struct SplatIn {
    // centre xy (world pixels), radius (world pixels), weight
    @location(0) place: vec4<f32>,
    // straight linear colour, w spare
    @location(1) color: vec4<f32>,
};

struct SplatOut {
    @builtin(position) pos: vec4<f32>,
    // Where in the splat, in radii from its centre.
    @location(0) at: vec2<f32>,
    @location(1) @interpolate(flat) color: vec4<f32>,
};

// How far a splat's quad reaches, in radii: e^(-3 × 1.6²) is under a thousandth.
const SPLAT_REACH: f32 = 1.6;

@vertex
fn vs_splat(@builtin(vertex_index) vi: u32, v: SplatIn) -> SplatOut {
    var out: SplatOut;
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u)) * 2.0 - 1.0;
    // A field texel is two output pixels: a splat narrower than one and a half of them is
    // widened to that, its weight cut by the area, so a small one doesn't flicker in and out.
    let least = 3.0 * g.view.z;
    let r = max(v.place.z, least);
    let k = (v.place.z * v.place.z) / max(r * r, 1e-8);
    out.pos = to_clip(v.place.xy + corner * r * SPLAT_REACH);
    out.at = corner * SPLAT_REACH;
    out.color = vec4<f32>(v.color.rgb, v.place.w * k);
    return out;
}

@fragment
fn fs_splat(in: SplatOut) -> @location(0) vec4<f32> {
    let w = in.color.a * exp(-3.0 * dot(in.at, in.at));
    return vec4<f32>(in.color.rgb * w, w);
}

struct FieldLook {
    // mode (0 smoke, 1 flame), adds, amount, strength
    kind: vec4<f32>,
    // amount, size, drift x, drift y
    detail: vec4<f32>,
    // smoke: shading, light from (radians), dark inside, relief (world pixels: above 0 the real
    // lights shade it, W15); flame: bend, flicker, flicker speed, brightness
    shade: vec4<f32>,
    light: vec4<f32>,
    emit: vec4<f32>,
    extra: vec4<f32>,
    heat: array<vec4<f32>, 32>,
};

@group(1) @binding(0) var field_picture: texture_2d<f32>;
@group(1) @binding(1) var field_sampler: sampler;
@group(1) @binding(2) var<uniform> look: FieldLook;

struct FieldOut {
    @builtin(position) pos: vec4<f32>,
    // Clip-space place, -1 to 1.
    @location(0) clip: vec2<f32>,
};

@vertex
fn vs_field(@builtin(vertex_index) vi: u32) -> FieldOut {
    var out: FieldOut;
    let p = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u)) * 2.0 - 1.0;
    out.pos = vec4<f32>(p, 0.0, 1.0);
    out.clip = p;
    return out;
}

// The world place at clip place `clip`, back through the camera.
fn field_world(clip: vec2<f32>) -> vec2<f32> {
    let q = vec2<f32>(clip.x / g.view.x, -clip.y / g.view.y);
    let c = g.camera;
    return c.xy + rotate(q / max(c.z, 1e-6), c.w);
}

// A way in the world, as a move in the field picture's uv.
fn field_uv_of(world_move: vec2<f32>) -> vec2<f32> {
    let c = g.camera;
    let s = rotate(world_move, -c.w) * c.z;
    return vec2<f32>(s.x * g.view.x * 0.5, s.y * g.view.y * 0.5);
}

fn field_hash(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(0.1031, 0.1030));
    let r = q + dot(q, q.yx + 33.33);
    return fract((r.x + r.y) * r.x);
}

// Smooth value noise, 0 to 1.
fn field_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = field_hash(i);
    let b = field_hash(i + vec2<f32>(1.0, 0.0));
    let c = field_hash(i + vec2<f32>(0.0, 1.0));
    let d = field_hash(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Three octaves of it, 0 to 1.
fn field_fbm(p: vec2<f32>) -> f32 {
    return field_noise(p) * 0.5333 + field_noise(p * 2.03 + 17.1) * 0.2667 + field_noise(p * 4.01 + 41.7) * 0.2;
}

fn field_at(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(field_picture, field_sampler, uv, 0.0);
}

// The flame's heat colour at x, 0 to 1.
fn heat_colour(x: f32) -> vec4<f32> {
    let t = clamp(x, 0.0, 1.0) * 31.0;
    let i = u32(floor(t));
    let j = min(i + 1u, 31u);
    return mix(look.heat[i], look.heat[j], t - f32(i));
}

// The field's premultiplied colour and cover here.
fn field_colour(in: FieldOut) -> vec4<f32> {
    let uv = vec2<f32>(in.clip.x * 0.5 + 0.5, 0.5 - in.clip.y * 0.5);
    let world = field_world(in.clip);
    let time = g.view.w;
    let amount = clamp(look.kind.z, 0.0, 1.0);
    let adds = look.kind.y > 0.5;
    let size = max(look.detail.y, 1.0);
    let drift = look.detail.zw * time;
    if look.kind.x > 0.5 {
        // Flame: read where rising noise bends it, so tongues lick upwards and flicker.
        let n0 = field_fbm((world - drift) / size);
        let n1 = field_fbm((world - drift) / size + vec2<f32>(5.2, 1.3));
        let bend = (vec2<f32>(n0, n1) - 0.5) * 2.0 * look.shade.x;
        let f = field_at(uv + field_uv_of(bend));
        if f.a <= 1e-5 {
            discard;
        }
        let tint = f.rgb / f.a;
        let detail = mix(1.0, 0.35 + 1.3 * n0, clamp(look.detail.x, 0.0, 1.0));
        let flick = 1.0
            - clamp(look.shade.y, 0.0, 1.0) * field_noise(world / (size * 2.0) + vec2<f32>(0.0, time * look.shade.z));
        let heat = f.a * look.kind.w * detail * flick;
        let x = 1.0 - exp(-max(heat, 0.0));
        let c = heat_colour(x);
        let rgb = c.rgb * tint * look.shade.w;
        if adds {
            return vec4<f32>(rgb * x * amount, 0.0);
        }
        let cover = smoothstep(0.02, 0.35, x) * c.a * amount;
        return vec4<f32>(rgb * cover, cover);
    }
    // Smoke: thick where many puffs meet, broken up by drifting noise, lit from one side.
    let f = field_at(uv);
    if f.a <= 1e-5 {
        discard;
    }
    let tint = f.rgb / f.a;
    let n = field_fbm((world - drift) / size);
    let detail = mix(1.0, 0.25 + 1.5 * n, clamp(look.detail.x, 0.0, 1.0));
    let dens = f.a * look.kind.w * detail;
    let cover = (1.0 - exp(-max(dens, 0.0))) * amount;
    // The slope of the density against the light's way, on the screen.
    let texel = 1.0 / vec2<f32>(textureDimensions(field_picture));
    let gx = field_at(uv + vec2<f32>(texel.x, 0.0)).a - field_at(uv - vec2<f32>(texel.x, 0.0)).a;
    let gy = field_at(uv + vec2<f32>(0.0, texel.y)).a - field_at(uv - vec2<f32>(0.0, texel.y)).a;
    let towards = rotate(vec2<f32>(cos(look.shade.y), sin(look.shade.y)), -g.camera.w);
    // Per world pixel (the samples are two field texels, four output pixels, apart), times 4 px:
    // the same at any picture size or zoom.
    let slope = vec2<f32>(gx, gy) / max(g.view.z, 1e-6) * look.kind.w;
    // Lit where the density falls off towards the light, shaded where it rises; in a lit scene
    // (a relief) the real lights do that through its height instead.
    var facing = clamp(-dot(slope, towards) * 2.0, -1.0, 1.0);
    if look.shade.w > 0.0 {
        facing = 0.0;
    }
    var shade = 1.0 + clamp(look.shade.x, 0.0, 1.0) * facing * 0.6;
    shade = shade * mix(1.0, exp(-dens * 0.6), clamp(look.shade.z, 0.0, 1.0));
    let rgb = tint * max(shade, 0.0);
    if adds {
        return vec4<f32>(rgb * cover, 0.0);
    }
    return vec4<f32>(rgb * cover, cover);
}

@fragment
fn fs_field(in: FieldOut) -> @location(0) vec4<f32> {
    let c = field_colour(in);
    // Smoke the real lights shade (W15): in the surface picture it stands up by its cover, so the
    // lighting mix turns each puff's side to the lights; a matte sheet (surface 0).
    let m = light_pass(g.light.x);
    if look.kind.x < 0.5 && look.shade.w > 0.0 && look.extra.y < 0.5 && m == PASS_SURFACE {
        let a = clamp(c.a, 0.0, 1.0);
        var blocks = clamp(look.light.y, 0.0, 1.0);
        if g.light.z > 0.5 {
            blocks = select(0.0, 1.0, blocks > 0.0);
        }
        return vec4<f32>(blocks * a, look.shade.w * a * a, 0.0, a);
    }
    return light_flat(c, look.light.x, look.light.y, look.emit.rgb, look.extra);
}

@fragment
fn fs_field_each(in: FieldOut) -> @location(0) vec4<f32> {
    return each_out(field_colour(in), g.post.w);
}

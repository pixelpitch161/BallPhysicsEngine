// The colour grade (colour grading, docs/notes/grading.md): the stack compiled by
// grade/compile.rs, run as a program over one pixel of the linear HDR picture, before the
// roll-off. Word 0 is [segments, split, 0, 0], then a word per segment [first word, steps,
// pyramid, 0]; each step is [op, space, words, extra] then its data. A segment is one pass: the
// last runs inside the composite, the others in fs_grade_pass (post.wgsl) into an HDR pair.
// The numbers below match grade/mod.rs.

@group(2) @binding(0) var<storage, read> grade_prog: array<vec4<f32>>;
// The previous pass's colour and kept colour (what a weaker mask started from), and the blur
// pyramid of that colour (tone space, the least channel's tone value in alpha). A grade of one
// pass binds 1x1 stand-ins.
@group(2) @binding(1) var grade_in: texture_2d<f32>;
@group(2) @binding(2) var grade_kept_in: texture_2d<f32>;
@group(2) @binding(3) var grade_pyr: texture_2d<f32>;
// The things field (colour grading part 5, silhouettes.rs): a layer for each choice of things
// the Things parts read: r the chosen things' coverage, g the distance (output pixels) to the
// nearest pixel they half cover, a every thing's coverage. A 1x1 stand-in without Things parts.
@group(2) @binding(4) var grade_things: texture_2d_array<f32>;

const GRADE_SAMPLES: u32 = 1024u;

const SPACE_LINEAR: u32 = 0u;
const SPACE_TONE: u32 = 1u;
const SPACE_OKLAB: u32 = 2u;

const OP_MATRIX: u32 = 1u;
const OP_CURVE: u32 = 2u;
const OP_CHROMA: u32 = 3u;
const OP_BEGIN: u32 = 4u;
const OP_END: u32 = 5u;
const OP_HUE: u32 = 6u;
const OP_POINT: u32 = 7u;
const OP_WHEELS: u32 = 8u;
const OP_DARK_TINT: u32 = 9u;
const OP_SHARPEN: u32 = 10u;
const OP_TEXTURE: u32 = 11u;
const OP_CLARITY: u32 = 12u;
const OP_DEHAZE: u32 = 13u;
const OP_VIGNETTE: u32 = 14u;
const OP_GRAIN: u32 = 15u;
const OP_GRADIENT_MAP: u32 = 16u;

const MAP_SAMPLES: u32 = 256u;

// A mask's parts (grade/mod.rs PART_*, COMBINE_*).
const PART_LINEAR: u32 = 1u;
const PART_RADIAL: u32 = 2u;
const PART_COLOUR: u32 = 3u;
const PART_LUMINANCE: u32 = 4u;
const PART_THINGS: u32 = 5u;
const PART_BRUSH: u32 = 6u;
const COMBINE_SUBTRACT: u32 = 1u;
const COMBINE_INTERSECT: u32 = 2u;
const MAX_BALLS: u32 = 4096u;
// The most copies of one gradient a pixel looks at (grade/mod.rs `MAX_SCAN`): hundreds of big
// gradients over one row would otherwise make a frame slow enough to lose the GPU.
const MAX_SCAN: u32 = 512u;
// The blur of the pyramid's mips read through the B-spline: about 1.9 · 2^k output pixels for
// mip k (measured on an impulse; mip 0 is 1.76).
const PYR_SIGMA: f32 = 1.9;
// Lift's softening near black (grade/maths.rs LIFT_SOFT).
const LIFT_SOFT: f32 = 0.05;

const HUE_SAMPLES: u32 = 360u;
const TAU: f32 = 6.2831853;

// The tone space: the sRGB transfer curve, the same formula carried on above 1, mirrored below 0.
fn tone1(x: f32) -> f32 {
    let a = abs(x);
    var e = a * 12.92;
    if a > 0.0031308 {
        e = 1.055 * pow(a, 1.0 / 2.4) - 0.055;
    }
    return select(e, -e, x < 0.0);
}

fn untone1(p: f32) -> f32 {
    let a = abs(p);
    var l = a / 12.92;
    if a > 0.04045 {
        l = pow((a + 0.055) / 1.055, 2.4);
    }
    return select(l, -l, p < 0.0);
}

fn cbrt1(x: f32) -> f32 {
    let r = pow(abs(x), 1.0 / 3.0);
    return select(r, -r, x < 0.0);
}

fn lin_to_oklab(c: vec3<f32>) -> vec3<f32> {
    let l = cbrt1(0.4122214708 * c.r + 0.5363325363 * c.g + 0.0514459929 * c.b);
    let m = cbrt1(0.2119034982 * c.r + 0.6806995451 * c.g + 0.1073969566 * c.b);
    let s = cbrt1(0.0883024619 * c.r + 0.2817188376 * c.g + 0.6299787005 * c.b);
    return vec3<f32>(
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    );
}

fn oklab_to_lin(lab: vec3<f32>) -> vec3<f32> {
    let l0 = lab.x + 0.3963377774 * lab.y + 0.2158037573 * lab.z;
    let m0 = lab.x - 0.1055613458 * lab.y - 0.0638541728 * lab.z;
    let s0 = lab.x - 0.0894841775 * lab.y - 1.2914855480 * lab.z;
    let l = l0 * l0 * l0;
    let m = m0 * m0 * m0;
    let s = s0 * s0 * s0;
    return vec3<f32>(
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    );
}

// `c` from space `was` to space `to`, through linear light.
fn grade_space(c: vec3<f32>, was: u32, to: u32) -> vec3<f32> {
    if was == to {
        return c;
    }
    var lin = c;
    if was == SPACE_TONE {
        lin = vec3<f32>(untone1(c.r), untone1(c.g), untone1(c.b));
    } else if was == SPACE_OKLAB {
        lin = oklab_to_lin(c);
    }
    if to == SPACE_TONE {
        return vec3<f32>(tone1(lin.r), tone1(lin.g), tone1(lin.b));
    }
    if to == SPACE_OKLAB {
        return lin_to_oklab(lin);
    }
    return lin;
}

// Channel `ch` of the curve whose step starts at `at`, at tone value `p`.
fn grade_curve(at: u32, p: f32, ch: u32) -> f32 {
    let first = at + 2u;
    if p < 0.0 {
        return grade_prog[first][ch] + p * grade_prog[at + 1u][ch];
    }
    let t = p / (p + 1.0) * f32(GRADE_SAMPLES);
    let i = u32(t);
    if i >= GRADE_SAMPLES - 1u {
        return p + grade_prog[first + GRADE_SAMPLES - 1u][ch];
    }
    return p + mix(grade_prog[first + i][ch], grade_prog[first + i + 1u][ch], t - f32(i));
}

// Vibrance and Saturation on an Oklab colour (see grade/maths.rs `chroma_scale`).
fn grade_chroma(lab: vec3<f32>, vibrance: f32, saturation: f32) -> vec3<f32> {
    let c = length(lab.yz);
    // A grey (black too) has no hue to read; atan2(0, 0) may not be a number on every card.
    if c < 1e-7 {
        return lab;
    }
    let dull = clamp(1.0 - (c / 0.25) * (c / 0.25), 0.0, 1.0);
    let hue = atan2(lab.z, lab.y);
    let tau = 6.2831853;
    let skin_hue = 0.9599311;
    var off = hue - skin_hue + 3.1415927;
    off = off - floor(off / tau) * tau - 3.1415927;
    let skin = exp(-(off / 0.35) * (off / 0.35));
    let scale = (1.0 + vibrance * dull * (1.0 - 0.5 * skin)) * (1.0 + saturation);
    return vec3<f32>(lab.x, lab.yz * scale);
}

// Oklab to lightness, chroma, hue (0 for a grey: atan2(0, 0) may not be a number on every card).
fn grade_lch(lab: vec3<f32>) -> vec3<f32> {
    let c = length(lab.yz);
    var h = 0.0;
    if c >= 1e-7 {
        h = atan2(lab.z, lab.y);
    }
    return vec3<f32>(lab.x, c, h);
}

fn grade_from_lch(lch: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(lch.x, lch.y * cos(lch.z), lch.y * sin(lch.z));
}

// The HSL panel (see grade/maths.rs `hsl`): its row over the hue at `at`.
fn grade_hsl(at: u32, lab: vec3<f32>) -> vec3<f32> {
    let lch = grade_lch(lab);
    if lch.y < 1e-7 {
        return lab;
    }
    let turns = lch.z / TAU;
    let t = (turns - floor(turns)) * f32(HUE_SAMPLES);
    let i = min(u32(t), HUE_SAMPLES - 1u);
    let j = (i + 1u) % HUE_SAMPLES;
    let row = mix(grade_prog[at + 1u + i], grade_prog[at + 1u + j], t - f32(i));
    let k = clamp(lch.y / 0.08, 0.0, 1.0);
    return grade_from_lch(vec3<f32>(lch.x * exp2(0.8 * row.z * k), lch.y * max(1.0 + row.y, 0.0), lch.z + row.x));
}

fn grade_smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = clamp((x - a) / (b - a), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

// Point Colour (see grade/maths.rs `point_colour`): `n` picks of three words from `at + 1`, every
// weight from the colour as it came.
fn grade_point(at: u32, n: u32, lab: vec3<f32>) -> vec3<f32> {
    // Past white is matched as the colour it shows as (grade/maths.rs `tamed`; roll_off is the
    // composite's, post.wgsl).
    var tamed = lab;
    let lin = oklab_to_lin(lab);
    if max(lin.r, max(lin.g, lin.b)) > 1.0 {
        tamed = lin_to_oklab(roll_off(lin));
    }
    let lch = grade_lch(tamed);
    var out = lab;
    for (var k = 0u; k < n; k++) {
        let p = at + 1u + 3u * k;
        let pick = grade_lch(grade_prog[p].xyz);
        let shift = grade_prog[p + 1u].xyz;
        let inv = grade_prog[p + 2u].xyz;
        let dh = 2.0 * sqrt(lch.y * pick.y) * sin((lch.z - pick.z) * 0.5);
        let d = length(vec3<f32>((lch.x - pick.x) * inv.x, (lch.y - pick.y) * inv.y, dh * inv.z));
        let w = 1.0 - grade_smooth(0.35, 1.0, d);
        if w > 0.0 {
            let now = grade_lch(out);
            out = grade_from_lch(vec3<f32>(now.x * exp2(0.8 * shift.z * w), now.y * max(1.0 + shift.y * w, 0.0), now.z + shift.x * w));
        }
    }
    return out;
}

// Colour Grading (see grade/maths.rs `colour_wheels`).
fn grade_wheels(at: u32, lab: vec3<f32>) -> vec3<f32> {
    let head = grade_prog[at + 1u];
    let x0 = clamp(lab.x, 0.0, 1.0);
    let pivot = head.x;
    var x = 0.5 + 0.5 * (x0 - pivot) / (1.0 - pivot);
    if x0 < pivot {
        x = 0.5 * x0 / pivot;
    }
    let ws = 1.0 - grade_smooth(1.0 / 3.0 - head.y, 1.0 / 3.0 + head.y, x);
    let wh = grade_smooth(2.0 / 3.0 - head.y, 2.0 / 3.0 + head.y, x);
    let wm = max(1.0 - ws - wh, 0.0);
    let s = grade_prog[at + 2u];
    let m = grade_prog[at + 3u];
    let h = grade_prog[at + 4u];
    let g = grade_prog[at + 5u];
    let tint = clamp(lab.x / 0.25, 0.0, 1.0);
    let ab = lab.yz + (s.xy * ws + m.xy * wm + h.xy * wh + g.xy) * tint;
    let l = max((lab.x + s.z * ws + m.z * wm + h.z * wh) * exp2(head.z), 0.0);
    return vec3<f32>(l, ab);
}

// Calibration's Shadows Tint (see grade/maths.rs `dark_tint`).
fn grade_dark_tint(gain: vec3<f32>, c: vec3<f32>) -> vec3<f32> {
    let y = dot(vec3<f32>(0.2126, 0.7152, 0.0722), c);
    let w = 1.0 - grade_smooth(0.0, 0.18, y);
    return c * (vec3<f32>(1.0) + (gain - vec3<f32>(1.0)) * w);
}


fn tone3(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(tone1(c.r), tone1(c.g), tone1(c.b));
}

fn least(c: vec3<f32>) -> f32 {
    return min(c.r, min(c.g, c.b));
}

// Where the grade is working: across the picture (0..1), the output pixel, the output's size and
// output pixels per pixel of the video.
struct GradeAt {
    uv: vec2<f32>,
    pix: vec2<i32>,
    size: vec2<f32>,
    scale: f32,
};

struct GradeOut {
    colour: vec3<f32>,
    kept: vec3<f32>,
    // How much the mask the editor's overlay shows covers here (0 without one).
    overlay: f32,
};

// A tone-space colour with its tone luma moved by `delta` (grade/maths.rs `lift`).
fn grade_lift(c: vec3<f32>, delta: f32) -> vec3<f32> {
    let y = luma(c);
    let ratio = max(y + delta + LIFT_SOFT, 0.0) / max(y + LIFT_SOFT, 1e-4);
    return (c + LIFT_SOFT) * ratio - LIFT_SOFT;
}

fn grade_soft(d: f32, limit: f32) -> f32 {
    // Clamped first: some back ends work tanh out from exp, and inf / inf is not a number.
    return limit * tanh(clamp(d / limit, -10.0, 10.0));
}

// Mip `level` of the pyramid at `uv` through a cubic B-spline (four bilinear taps), so the
// blur has no blocks.
fn grade_bspline(uv: vec2<f32>, level: u32) -> vec4<f32> {
    let size = vec2<f32>(textureDimensions(grade_pyr, level));
    let p = uv * size - 0.5;
    let i = floor(p);
    let f = p - i;
    let f2 = f * f;
    let f3 = f2 * f;
    let w0 = (-f3 + 3.0 * f2 - 3.0 * f + 1.0) / 6.0;
    let w1 = (3.0 * f3 - 6.0 * f2 + 4.0) / 6.0;
    let w2 = (-3.0 * f3 + 3.0 * f2 + 3.0 * f + 1.0) / 6.0;
    let w3 = f3 / 6.0;
    let g0 = w0 + w1;
    let g1 = w2 + w3;
    let c0 = (i - 0.5 + w1 / g0) / size;
    let c1 = (i + 1.5 + w3 / g1) / size;
    let l = f32(level);
    let a = textureSampleLevel(grade_pyr, samp, c0, l) * g0.x + textureSampleLevel(grade_pyr, samp, vec2<f32>(c1.x, c0.y), l) * g1.x;
    let b = textureSampleLevel(grade_pyr, samp, vec2<f32>(c0.x, c1.y), l) * g0.x + textureSampleLevel(grade_pyr, samp, c1, l) * g1.x;
    return a * g0.y + b * g1.y;
}

// The pass input blurred by a Gaussian of about `sigma` output pixels (tone space, the least
// channel in alpha): between two mips, or below mip 0 between the pixel itself (`own`) and mip 0.
fn grade_blur(uv: vec2<f32>, sigma: f32, own: vec4<f32>) -> vec4<f32> {
    if sigma <= PYR_SIGMA {
        return mix(own, grade_bspline(uv, 0u), max(sigma, 0.0) / PYR_SIGMA);
    }
    let top = f32(textureNumLevels(grade_pyr) - 1u);
    let l = min(log2(sigma / PYR_SIGMA), top);
    let l0 = floor(l);
    let a = grade_bspline(uv, u32(l0));
    if l0 >= top {
        return a;
    }
    return mix(a, grade_bspline(uv, u32(l0) + 1u), l - l0);
}

// The pass input at the pixel as the pyramid holds it (clamped as its first level is).
fn grade_own(src: vec3<f32>) -> vec4<f32> {
    let p = tone3(clamp(src, vec3<f32>(-64.0), vec3<f32>(64.0)));
    return vec4<f32>(p, least(p));
}

// Sharpening (Detail): the input's tone luma (the pass wrote it in alpha) against a Gaussian
// round it, held under the Detail limit, added to `c` (tone space). d: [gain, limit, radius in
// video pixels, 0]. The blur is kept to 3 output pixels, so its taps reach 2σ.
fn grade_sharpen(c: vec3<f32>, d: vec4<f32>, at: GradeAt) -> vec3<f32> {
    let s = min(d.z * at.scale, 3.0);
    if s < 0.05 {
        return c;
    }
    let k = i32(ceil(2.0 * s));
    let last = vec2<i32>(textureDimensions(grade_in)) - vec2<i32>(1);
    var sum = 0.0;
    var weights = 0.0;
    for (var dy = -k; dy <= k; dy++) {
        for (var dx = -k; dx <= k; dx++) {
            let q = clamp(at.pix + vec2<i32>(dx, dy), vec2<i32>(0), last);
            let w = exp(-f32(dx * dx + dy * dy) / (2.0 * s * s));
            sum += w * textureLoad(grade_in, q, 0).a;
            weights += w;
        }
    }
    let diff = textureLoad(grade_in, at.pix, 0).a - sum / weights;
    return grade_lift(c, d.x * grade_soft(diff, d.y));
}

// Texture: the band of detail between two blurs. d: [gain, limit, fine σ, coarse σ].
fn grade_texture(c: vec3<f32>, src: vec3<f32>, d: vec4<f32>, at: GradeAt) -> vec3<f32> {
    let own = grade_own(src);
    let band = luma(grade_blur(at.uv, d.z * at.scale, own).rgb) - luma(grade_blur(at.uv, d.w * at.scale, own).rgb);
    return grade_lift(c, d.x * grade_soft(band, d.y));
}

// Clarity: the pixel against a wide blur round it, mostly in the midtones. d: [gain, limit, σ, 0].
fn grade_clarity(c: vec3<f32>, src: vec3<f32>, d: vec4<f32>, at: GradeAt) -> vec3<f32> {
    let own = grade_own(src);
    let y = luma(own.rgb);
    let x = 2.0 * y - 1.0;
    let w = clamp(1.0 - 0.75 * x * x, 0.25, 1.0);
    let diff = y - luma(grade_blur(at.uv, d.z * at.scale, own).rgb);
    return grade_lift(c, d.x * w * grade_soft(diff, d.y));
}

// Dehaze (see grade/maths.rs `dehaze`) on linear `c`. d: [amount, σ, 0, 0].
fn grade_dehaze(c: vec3<f32>, src: vec3<f32>, d: vec4<f32>, at: GradeAt) -> vec3<f32> {
    let b = grade_blur(at.uv, d.y * at.scale, grade_own(src));
    if d.x >= 0.0 {
        let veil = clamp(min(least(src), untone1(b.a)), 0.0, 0.8);
        let v = 0.75 * d.x * veil;
        return max(c - vec3<f32>(v), vec3<f32>(0.0)) / (1.0 - v);
    }
    let fog = 0.55 + 0.45 * clamp(untone1(luma(b.rgb)), 0.0, 1.0);
    return mix(c, vec3<f32>(fog), -0.6 * d.x);
}

// The post-crop vignette (see grade/maths.rs `vignette_weight`, `vignette`) on linear `c`.
fn grade_vignette(c: vec3<f32>, src: vec3<f32>, w: vec4<f32>, more: vec4<f32>, at: GradeAt) -> vec3<f32> {
    let p = (at.uv - 0.5) * 2.0;
    let circle = p * at.size / min(at.size.x, at.size.y);
    let q = mix(p, circle, max(w.w, 0.0));
    let n = 2.0 - 6.0 * min(w.w, 0.0);
    let dist = pow(pow(abs(q.x), n) + pow(abs(q.y), n), 1.0 / n);
    let v = grade_smooth(w.y - 0.5 * w.z, w.y + 0.5 * w.z, dist);
    if w.x < 0.0 {
        var keep = 0.0;
        if more.x > 0.0 {
            let local = untone1(luma(grade_blur(at.uv, more.y * at.scale, grade_own(src)).rgb));
            keep = more.x * grade_smooth(0.3, 1.0, local);
        }
        return c * max(1.0 + w.x * v * (1.0 - keep), 0.0);
    }
    let t = tone3(c);
    // Past white stays where it is.
    let lifted = t + max(vec3<f32>(1.0) - t, vec3<f32>(0.0)) * (w.x * v);
    return vec3<f32>(untone1(lifted.r), untone1(lifted.g), untone1(lifted.b));
}

// Smooth value noise in -1..1 on the lattice of `p`, from the composite's hash, for `seed`.
fn grade_noise(p: vec2<f32>, seed: u32) -> f32 {
    let i = floor(p);
    let f = p - i;
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let b = vec2<i32>(i);
    let h00 = dither_noise(bitcast<vec2<u32>>(b), seed).x;
    let h10 = dither_noise(bitcast<vec2<u32>>(b + vec2<i32>(1, 0)), seed).x;
    let h01 = dither_noise(bitcast<vec2<u32>>(b + vec2<i32>(0, 1)), seed).x;
    let h11 = dither_noise(bitcast<vec2<u32>>(b + vec2<i32>(1, 1)), seed).x;
    return mix(mix(h00, h10, u.x), mix(h01, h11, u.x), u.y) * 2.0 - 1.0;
}

// Grain on linear `c`: a new pattern each frame (the frame's time is the seed), the same for the
// same frame. d: [amount, cell in video pixels, roughness, 0].
fn grade_grain(c: vec3<f32>, d: vec4<f32>, at: GradeAt) -> vec3<f32> {
    let seed = bitcast<u32>(g.view.w) * 747796405u;
    let p = (vec2<f32>(at.pix) + 0.5) / max(at.scale, 1e-4) / d.y;
    let r = d.z;
    let n1 = grade_noise(p, seed ^ 0x68bc21ebu);
    let n2 = grade_noise(p * 2.1 + 17.3, seed ^ 0x02e5be93u);
    let n3 = grade_noise(p * 0.37 + 5.1, seed ^ 0x967a889bu);
    // A grain smaller than an output pixel would average out in the export scaled down: weaker.
    let n = (n1 + 0.6 * r * n2) * (1.0 + 0.8 * r * n3) / (1.0 + 0.6 * r) * min(d.y * at.scale, 1.0);
    // Like film, it fades out in the brightest parts: past white a ±30% grain would burn to
    // speckles of white where the roll-off whitens hot light.
    let fade = 1.0 - grade_smooth(0.6, 1.5, max(c.r, max(c.g, c.b)));
    return max(c * exp2(1.2 * d.x * n * fade) + vec3<f32>(0.0015 * d.x * n), vec3<f32>(0.0));
}

// The gradient map (see grade/maths.rs `gradient_map`) on tone-space `c`.
fn grade_map(at: u32, amount: f32, c: vec3<f32>) -> vec3<f32> {
    let y = luma(c);
    let t = clamp(y, 0.0, 1.0) * f32(MAP_SAMPLES - 1u);
    let i = min(u32(t), MAP_SAMPLES - 2u);
    let m = mix(grade_prog[at + 1u + i].xyz, grade_prog[at + 2u + i].xyz, t - f32(i)) * max(y, 1.0);
    return mix(c, m, amount);
}

// Point `(x, y)` of a gradient at place `q` ([at x, at y, turn, scale]), on the screen.
fn grade_placed(q: vec4<f32>, x: f32, y: f32) -> vec2<f32> {
    let s = sin(q.z);
    let co = cos(q.z);
    let v = vec2<f32>(x, y) * q.w;
    return q.xy + vec2<f32>(v.x * co - v.y * s, v.x * s + v.y * co);
}

// One copy of a Linear Gradient (see grade/maths.rs `linear_cover`). n: [x, y, angle, feather].
fn grade_linear_one(n: vec4<f32>, q: vec4<f32>, p: vec2<f32>, px: f32) -> f32 {
    let c = grade_placed(q, n.x, n.y);
    let a = n.z + q.z;
    let f = max(n.w * q.w, px);
    let t = dot(p - c, vec2<f32>(cos(a), sin(a))) / f + 0.5;
    return 1.0 - grade_smooth(0.0, 1.0, t);
}

// One copy of a Radial Gradient (see grade/maths.rs `radial_cover`). n0: [x, y, half width, half
// height], n1: [angle, roundness, feather, 0].
fn grade_radial_one(n0: vec4<f32>, n1: vec4<f32>, q: vec4<f32>, p: vec2<f32>, px: f32) -> f32 {
    let c = grade_placed(q, n0.x, n0.y);
    let a = n1.x + q.z;
    let s = sin(a);
    let co = cos(a);
    let d = p - c;
    let local = vec2<f32>(d.x * co + d.y * s, -d.x * s + d.y * co);
    let half = n0.zw * q.w;
    let short = min(half.x, half.y);
    let qq = mix(local / half, local / short, max(n1.y, 0.0));
    if max(abs(qq.x), abs(qq.y)) > 1.0 {
        return 0.0;
    }
    let e = 2.0 - 6.0 * min(n1.y, 0.0);
    let dist = pow(pow(max(abs(qq.x), 1e-7), e) + pow(max(abs(qq.y), 1e-7), e), 1.0 / e);
    let inner = min(1.0 - n1.z, 1.0 - px / short);
    return 1.0 - grade_smooth(inner, 1.0, dist);
}

// Texel `(x, y)` of a brush grid `w` texels wide whose words start at `g` (three bytes a lane,
// see grade/brush.rs `words`), 0..1; none outside its `size`.
fn grade_brush_texel(g: u32, size: vec2<f32>, x: i32, y: i32) -> f32 {
    if x < 0 || y < 0 || f32(x) >= size.x || f32(y) >= size.y {
        return 0.0;
    }
    let i = u32(y) * u32(size.x) + u32(x);
    let lane = (i / 3u) % 4u;
    let word = grade_prog[g + i / 12u];
    // A whole number below 2^24, exact as it is (adding a half would round 2^24 - 1 up to 2^24).
    let packed = u32(max(word[lane], 0.0));
    return f32((packed >> (8u * (i % 3u))) & 255u) / 255.0;
}

// One copy of a Brush at place `q` (see grade/maths.rs `brush_cover`). n0: [x0, y0, 1 / texel,
// 0], n1: [width, height, grid word, 0]; `g` the grid's first word.
fn grade_brush_one(n0: vec4<f32>, n1: vec4<f32>, g: u32, q: vec4<f32>, p: vec2<f32>) -> f32 {
    let d = p - q.xy;
    let s = sin(q.z);
    let co = cos(q.z);
    let local = vec2<f32>(d.x * co + d.y * s, -d.x * s + d.y * co) / q.w;
    let t = (local - n0.xy) * n0.z - 0.5;
    let base = floor(t);
    let f = t - base;
    let x = i32(base.x);
    let y = i32(base.y);
    let top = mix(grade_brush_texel(g, n1.xy, x, y), grade_brush_texel(g, n1.xy, x + 1, y), f.x);
    let bottom = mix(grade_brush_texel(g, n1.xy, x, y + 1), grade_brush_texel(g, n1.xy, x + 1, y + 1), f.x);
    return mix(top, bottom, f.y);
}

// A gradient's or brush's copies from word `at` ([count, reach, 0, 0], then one place a word,
// sorted down the screen), joined with Add: only those whose row is within reach of the pixel's
// are looked at. `g` is a brush's grid word.
fn grade_copies(kind: u32, n0: vec4<f32>, n1: vec4<f32>, at: u32, g: u32, p: vec2<f32>, px: f32) -> f32 {
    let head = grade_prog[at];
    let count = min(u32(head.x), MAX_BALLS);
    let reach = head.y;
    var lo = 0u;
    if reach >= 0.0 {
        var hi = count;
        let want = p.y - reach;
        while lo < hi {
            let mid = (lo + hi) / 2u;
            if grade_prog[at + 1u + mid].y < want {
                lo = mid + 1u;
            } else {
                hi = mid;
            }
        }
    }
    var best = 0.0;
    let end = min(count, lo + MAX_SCAN);
    for (var k = lo; k < end; k++) {
        let q = grade_prog[at + 1u + k];
        if reach >= 0.0 && q.y > p.y + reach {
            break;
        }
        if kind == PART_LINEAR {
            best = max(best, grade_linear_one(n0, q, p, px));
        } else if kind == PART_BRUSH {
            best = max(best, grade_brush_one(n0, n1, g, q, p));
        } else {
            best = max(best, grade_radial_one(n0, n1, q, p, px));
        }
    }
    return best;
}

// A Colour Range on linear `c` (see grade/maths.rs `colour_range_cover`): its nearest pick.
fn grade_colour_range(d: u32, c: vec3<f32>) -> f32 {
    let head = grade_prog[d];
    var lin = c;
    if max(c.r, max(c.g, c.b)) > 1.0 {
        lin = roll_off(c);
    }
    let lab = lin_to_oklab(lin);
    var near = 1e9;
    for (var k = 0u; k < u32(head.x); k++) {
        let off = lab - grade_prog[d + 1u + k].xyz;
        near = min(near, length(vec3<f32>(0.5 * off.x, off.y, off.z)));
    }
    if near > 1e8 {
        return 0.0;
    }
    return 1.0 - grade_smooth(head.y, head.z, near);
}

// A Luminance Range on linear `c` (see grade/maths.rs `luminance_cover`). n: [low, high, fade, 0].
fn grade_luminance(n: vec4<f32>, c: vec3<f32>) -> f32 {
    let y = clamp(luma(tone3(roll_off(max(c, vec3<f32>(0.0))))), 0.0, 1.0);
    return grade_smooth(n.x - n.z, n.x, y) * (1.0 - grade_smooth(n.y, n.y + n.z, y));
}

// A Things part (see grade/maths.rs `things_cover`). n: [layer, expand, feather, background],
// in pixels of the video; `scale` output pixels each.
fn grade_things_part(n: vec4<f32>, here: GradeAt) -> f32 {
    var v = 0.0;
    if n.x >= 0.0 {
        let size = vec2<i32>(textureDimensions(grade_things));
        let at = clamp(here.pix, vec2<i32>(0), size - 1);
        let f = textureLoad(grade_things, at, i32(n.x + 0.5), 0);
        let e = n.y * here.scale;
        let fe = n.z * here.scale;
        v = clamp(f.r, 0.0, 1.0);
        if e + fe > 0.0 {
            v = max(v, 1.0 - grade_smooth(e - 0.5, e + fe + 0.5, f.g - 0.5));
        }
    }
    if n.w > 0.5 {
        v = 1.0 - v;
    }
    return v;
}

// One part of kind `kind` whose words start at `d`, at screen point `p`, over the colour the mask
// started from.
fn grade_part(kind: u32, d: u32, c: vec3<f32>, p: vec2<f32>, px: f32, here: GradeAt) -> f32 {
    if kind == PART_LINEAR {
        return grade_copies(kind, grade_prog[d], vec4<f32>(0.0), d + 1u, 0u, p, px);
    }
    if kind == PART_RADIAL {
        return grade_copies(kind, grade_prog[d], grade_prog[d + 1u], d + 2u, 0u, p, px);
    }
    if kind == PART_BRUSH {
        let n1 = grade_prog[d + 1u];
        return grade_copies(kind, grade_prog[d], n1, d + 2u, d + u32(n1.z), p, px);
    }
    if kind == PART_THINGS {
        return grade_things_part(grade_prog[d], here);
    }
    if kind == PART_COLOUR {
        return grade_colour_range(d, c);
    }
    if kind == PART_LUMINANCE {
        return grade_luminance(grade_prog[d], c);
    }
    return 0.0;
}

// How much the mask whose area words start at `at` covers `here` (see grade/maths.rs
// `area_cover`), `c` the linear colour it started from.
fn grade_cover(at: u32, c: vec3<f32>, here: GradeAt) -> f32 {
    let head = grade_prog[at];
    let n = u32(head.x);
    let unit = 1.0 / max(here.scale, 1e-4);
    // Screen units: pixels of the video from the picture's middle, y down.
    let p = (here.uv - 0.5) * here.size * unit;
    var cov = 0.0;
    var w = at + 1u;
    for (var i = 0u; i < n; i++) {
        let ph = grade_prog[w];
        var v = grade_part(u32(ph.x), w + 1u, c, p, unit, here);
        if ph.z > 0.5 {
            v = 1.0 - v;
        }
        if i == 0u {
            cov = v;
        } else if u32(ph.y) == COMBINE_SUBTRACT {
            cov = cov * (1.0 - v);
        } else if u32(ph.y) == COMBINE_INTERSECT {
            cov = cov * v;
        } else {
            cov = max(cov, v);
        }
        w += max(u32(ph.w), 1u);
    }
    if head.y > 0.5 && n > 0u {
        cov = 1.0 - cov;
    }
    return clamp(cov, 0.0, 1.0);
}

// Segment `seg` of the grade over one linear colour (and the colour a weaker mask started from,
// and the overlay so far) at `here`.
fn run_grade(colour: vec3<f32>, kept: vec3<f32>, overlay: f32, here: GradeAt, seg: u32) -> GradeOut {
    var result: GradeOut;
    result.colour = colour;
    result.kept = kept;
    result.overlay = overlay;
    // Left of the editor's split the picture is shown as it was.
    if here.uv.x < grade_prog[0].y {
        return result;
    }
    let table = grade_prog[1u + seg];
    let steps = u32(table.y);
    var c = colour;
    var space = SPACE_LINEAR;
    var saved = kept;
    var at = u32(table.x);
    for (var k = 0u; k < steps; k++) {
        let h = grade_prog[at];
        let op = u32(h.x);
        let want = u32(h.y);
        c = grade_space(c, space, want);
        space = want;
        if op == OP_MATRIX {
            c = vec3<f32>(
                dot(grade_prog[at + 1u].xyz, c),
                dot(grade_prog[at + 2u].xyz, c),
                dot(grade_prog[at + 3u].xyz, c),
            );
        } else if op == OP_CURVE {
            c = vec3<f32>(grade_curve(at, c.r, 0u), grade_curve(at, c.g, 1u), grade_curve(at, c.b, 2u));
        } else if op == OP_CHROMA {
            let n = grade_prog[at + 1u];
            c = grade_chroma(c, n.x, n.y);
        } else if op == OP_HUE {
            c = grade_hsl(at, c);
        } else if op == OP_POINT {
            c = grade_point(at, u32(h.w), c);
        } else if op == OP_WHEELS {
            c = grade_wheels(at, c);
        } else if op == OP_DARK_TINT {
            c = grade_dark_tint(grade_prog[at + 1u].xyz, c);
        } else if op == OP_SHARPEN {
            c = grade_sharpen(c, grade_prog[at + 1u], here);
        } else if op == OP_TEXTURE {
            c = grade_texture(c, colour, grade_prog[at + 1u], here);
        } else if op == OP_CLARITY {
            c = grade_clarity(c, colour, grade_prog[at + 1u], here);
        } else if op == OP_DEHAZE {
            c = grade_dehaze(c, colour, grade_prog[at + 1u], here);
        } else if op == OP_VIGNETTE {
            c = grade_vignette(c, colour, grade_prog[at + 1u], grade_prog[at + 2u], here);
        } else if op == OP_GRAIN {
            c = grade_grain(c, grade_prog[at + 1u], here);
        } else if op == OP_GRADIENT_MAP {
            c = grade_map(at, h.w, c);
        } else if op == OP_BEGIN {
            saved = c;
        } else if op == OP_END {
            // A mask over part of the picture mixes back by its coverage, worked out from the
            // colour it started from.
            var cover = 1.0;
            if u32(h.z) > 1u {
                cover = grade_cover(at + 1u, saved, here);
                if grade_prog[at + 1u].z > 0.5 {
                    result.overlay = cover;
                }
            }
            c = mix(saved, c, h.w * cover);
        }
        at += max(u32(h.z), 1u);
    }
    let out = grade_space(c, space, SPACE_LINEAR);
    // Anything that came out not a number is black, never a speck of garbage.
    result.colour = select(out, vec3<f32>(0.0), out != out);
    result.kept = select(saved, vec3<f32>(0.0), saved != saved);
    return result;
}

// The colour grade (colour grading, docs/notes/grading.md): the stack compiled by
// grade/compile.rs, run as a program over one pixel of the linear HDR picture, inside the
// composite before the roll-off. Word 0 is [steps, split, 0, 0]; each step is [op, space,
// words, extra] then its data. The numbers below match grade/mod.rs.

@group(2) @binding(0) var<storage, read> grade_prog: array<vec4<f32>>;

const GRADE_SAMPLES: u32 = 1024u;

const SPACE_LINEAR: u32 = 0u;
const SPACE_TONE: u32 = 1u;
const SPACE_OKLAB: u32 = 2u;

const OP_MATRIX: u32 = 1u;
const OP_CURVE: u32 = 2u;
const OP_CHROMA: u32 = 3u;
const OP_BEGIN: u32 = 4u;
const OP_END: u32 = 5u;

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

// The grade over one linear colour at `uv` across the picture.
fn run_grade(colour: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let head = grade_prog[0];
    // Left of the editor's split the picture is shown as it was.
    if uv.x < head.y {
        return colour;
    }
    let steps = u32(head.x);
    var c = colour;
    var space = SPACE_LINEAR;
    var saved = colour;
    var at = 1u;
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
        } else if op == OP_BEGIN {
            saved = c;
        } else if op == OP_END {
            c = mix(saved, c, h.w);
        }
        at += max(u32(h.z), 1u);
    }
    return grade_space(c, space, SPACE_LINEAR);
}

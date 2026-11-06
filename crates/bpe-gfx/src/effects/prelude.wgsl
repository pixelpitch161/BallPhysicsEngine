// Shared by every effect. The effect file is pasted below this, and must define
//     fn effect(uv: vec2<f32>, p: Params) -> vec3<f32>
// which returns the colour for the pixel at `uv` (0,0 top left, 1,1 bottom right).
// `p` holds the values of the settings declared with `// param`, `// simple`, `// choice` and
// `// color` lines at the top of the file. What the effect sees through `sample` is what it
// takes: the whole picture, the light falling on lit things, or the glow (see `bpe_taken`).

const PI: f32 = 3.14159265;
const TAU: f32 = 6.28318531;
const MAX_HITS: u32 = 16u;

struct Fx {
    // Size of the picture in pixels.
    resolution: vec2<f32>,
    // Seconds since the start of the video.
    time: f32,
    // The module's Blend setting; the result is mixed with the input by this much.
    blend: f32,
    // 0 to 1, jumps up when balls hit and fades back down.
    pulse: f32,
    // How many recent hits are in `hits`.
    hit_count: u32,
    // Frames since the effect started; 0 on the first frame, when prev() is still black.
    frame: f32,
    // Seconds since the previous frame of this effect.
    dt: f32,
    // Picture pixels per video pixel (below 1 in the editor preview). Inside a ball or a shape:
    // output pixels per world unit, so its pixel sizes also grow as the camera zooms in.
    scale: f32,
    // How strong the hits are right now, 0 to 1.
    level: f32,
    // Unused; always 0.
    view: u32,
    // What the effect changes: 0 the whole picture, else one thing's code (its id plus one).
    // Outside that thing the picture is left as it was.
    apply_to: f32,
    // 1 when the picture is a blend layer (premultiplied, its alpha how much the layer covers),
    // else 0.
    in_layer: f32,
    // The hit pulse three times over (read by bass, mid and treble), and 0.
    audio: vec4<f32>,
    params: array<vec4<f32>, 4>,
    // Newest hits first: position (uv), seconds since the hit, strength 0 to 1.
    hits: array<vec4<f32>, 16>,
    hit_colors: array<vec4<f32>, 16>,
    // What the effect takes (0 the whole picture, 1 the light, 2 the glow), the glow's
    // brightness, the Blend's code (see blend_maths.wgsl), and how many balls the list holds.
    takes: vec4<f32>,
    // The beat's phase (0 to 1), the bar's phase, beats per second, and 1 when there is a tempo.
    beat: vec4<f32>,
};

// One ball of the frame: its centre (uv), its radius (in screen heights), its random number;
// its colour (linear) and the seconds since its last hit.
struct FxBall {
    at: vec4<f32>,
    colour: vec4<f32>,
};

@group(0) @binding(0) var<uniform> fx: Fx;
@group(0) @binding(1) var src_tex: texture_2d<f32>;
@group(0) @binding(2) var prev_tex: texture_2d<f32>;
@group(0) @binding(3) var samp: sampler;
@group(0) @binding(4) var object_tex: texture_2d<f32>;
// A lit frame's picture before the light was mixed in, and the lit share of its colour (black
// on a frame with no Lighting card).
@group(0) @binding(5) var unlit_tex: texture_2d<f32>;
@group(0) @binding(6) var lit_tex: texture_2d<f32>;
@group(0) @binding(7) var<storage, read> fx_balls: array<FxBall>;
// The lens pre-pass of an effect with a `// lens:` line (black for any other): the streaks from
// its bright parts, and those bright parts softened over five levels.
@group(0) @binding(8) var lens_rays_tex: texture_2d<f32>;
@group(0) @binding(9) var lens_soft_tex: texture_2d<f32>;

// What the effect keeps at this pixel for its next frame (see keep and kept); below 0 when it
// keeps nothing.
var<private> bpe_keep: f32 = -1.0;

struct FsIn {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> FsIn {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: FsIn;
    out.pos = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(uv.x, 1.0 - uv.y);
    return out;
}

// Folds coordinates outside 0..1 back in like a mirror, so warped edges never smear.
fn fold_uv(uv: vec2<f32>) -> vec2<f32> {
    return 1.0 - abs(1.0 - abs(uv) % vec2<f32>(2.0));
}

// How much a blend layer's pixel counts as covering: its coverage, or for light added with
// little or no coverage (a glow, a flash) its brightest channel, as the blend composite counts it.
fn bpe_cover(c: vec4<f32>) -> f32 {
    return clamp(max(c.a, max(c.r, max(c.g, c.b))), 0.0, 1.0);
}

// A blend layer's picture holds colour already multiplied by how much it covers; effects see
// the colour itself, and their result is multiplied back (see fs_main).
fn bpe_straight(c: vec4<f32>) -> vec3<f32> {
    if fx.in_layer > 0.5 {
        let k = bpe_cover(c);
        return select(vec3<f32>(0.0), c.rgb / k, k > 1e-4);
    }
    return c.rgb;
}

// The light at a pixel of a lit picture `c`: `c = unlit + (light - 1) * lit share` (the
// lighting's mix), so light = 1 + (c - unlit) / share, and 1 where nothing takes light.
// Where the lit share is a sliver of the picture (a lit ball's soft edge over a bright
// background) the picture's own rounding swamps it, so the light found there eases to 1.
fn bpe_light_of(c: vec3<f32>, unlit: vec3<f32>, lit: vec3<f32>) -> vec3<f32> {
    let w = clamp(lit, vec3<f32>(0.0), max(unlit, vec3<f32>(0.0)));
    let some = w > vec3<f32>(1e-4);
    let raw = select(vec3<f32>(1.0), vec3<f32>(1.0) + (c - unlit) / max(w, vec3<f32>(1e-4)), some);
    let biggest = max(max(unlit.r, unlit.g), max(unlit.b, 1e-4));
    let trust = smoothstep(vec3<f32>(1.0 / 64.0), vec3<f32>(1.0 / 16.0), w / biggest);
    return mix(vec3<f32>(1.0), raw, trust);
}

// The lit share of the unlit picture, as the lighting's mix clamps it.
fn bpe_share(unlit: vec3<f32>, lit: vec3<f32>) -> vec3<f32> {
    return clamp(lit, vec3<f32>(0.0), max(unlit, vec3<f32>(0.0)));
}

// What the effect takes from picture colour `c` at `uv`: all of it, its light, or its glow.
fn bpe_taken(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    if fx.takes.x < 0.5 {
        return c;
    }
    if fx.takes.x < 1.5 {
        let at = fold_uv(uv);
        let unlit = textureSampleLevel(unlit_tex, samp, at, 0.0).rgb;
        let lit = textureSampleLevel(lit_tex, samp, at, 0.0).rgb;
        return bpe_light_of(c, unlit, lit);
    }
    return max(c - vec3<f32>(fx.takes.y), vec3<f32>(0.0));
}

// The picture coming into this effect: what it takes of it.
fn sample(uv: vec2<f32>) -> vec3<f32> {
    let c = bpe_straight(textureSampleLevel(src_tex, samp, fold_uv(uv), 0.0));
    if fx.takes.x < 0.5 {
        return c;
    }
    return bpe_taken(c, uv);
}

// This effect's own output from the previous frame, for feedback and echo effects (of what it
// takes: the light's for an effect on the light).
fn prev(uv: vec2<f32>) -> vec3<f32> {
    return bpe_straight(textureSampleLevel(prev_tex, samp, uv, 0.0));
}

// Keeps `v` (0 or more) at this pixel for the next frame, read back with kept(). On the main
// picture only; inside a blend layer nothing is kept.
fn keep(v: f32) {
    bpe_keep = max(v, 0.0);
}

// What the effect kept at `uv` last frame (0 on its first frame).
fn kept(uv: vec2<f32>) -> f32 {
    if fx.in_layer > 0.5 {
        return 0.0;
    }
    return max(textureSampleLevel(prev_tex, samp, uv, 0.0).a, 0.0);
}

// How many balls the frame has (its ball list).
fn ball_count() -> u32 {
    return u32(fx.takes.w);
}

// Ball `i`'s centre, in uv.
fn ball_uv(i: u32) -> vec2<f32> {
    return fx_balls[i].at.xy;
}

// Ball `i`'s radius, as a share of the picture's height (as `to_centered` measures).
fn ball_radius(i: u32) -> f32 {
    return fx_balls[i].at.z;
}

// Ball `i`'s random number, 0 to 1, the same in every frame.
fn ball_seed(i: u32) -> f32 {
    return fx_balls[i].at.w;
}

// Ball `i`'s colour, linear light.
fn ball_color(i: u32) -> vec3<f32> {
    return fx_balls[i].colour.rgb;
}

// Seconds since ball `i` last hit something (large when it never has).
fn ball_hit(i: u32) -> f32 {
    return fx_balls[i].colour.w;
}

// The ball nearest `uv`: its index and how far `uv` is from its edge (negative inside), in
// screen heights. (0, 1e6) when there are no balls.
fn nearest_ball(uv: vec2<f32>) -> vec2<f32> {
    let q = to_centered(uv);
    var best = 1e6;
    var which = 0u;
    for (var i = 0u; i < ball_count(); i++) {
        let d = length(q - to_centered(ball_uv(i))) - ball_radius(i);
        if d < best {
            best = d;
            which = i;
        }
    }
    return vec2<f32>(f32(which), best);
}

// The beat's phase, 0 at each beat to 1 just before the next (0 with no tempo).
fn beat_phase() -> f32 {
    return fx.beat.x;
}

// The bar's phase, 0 at each bar's first beat.
fn bar_phase() -> f32 {
    return fx.beat.y;
}

// 1 on each beat, falling away before the next (`sharp` how fast), or the hits' pulse when it
// is stronger.
fn kick(sharp: f32) -> f32 {
    let on_beat = select(0.0, exp(-fx.beat.x * max(sharp, 0.0)), fx.beat.w > 0.5);
    return max(on_beat, fx.pulse);
}

fn aspect() -> f32 {
    return fx.resolution.x / fx.resolution.y;
}

// uv to coordinates centred on the middle of the frame, where 1.0 is the frame height.
fn to_centered(uv: vec2<f32>) -> vec2<f32> {
    return (uv - 0.5) * vec2<f32>(aspect(), 1.0);
}

fn from_centered(q: vec2<f32>) -> vec2<f32> {
    return q / vec2<f32>(aspect(), 1.0) + 0.5;
}

fn rotate(v: vec2<f32>, angle: f32) -> vec2<f32> {
    let c = cos(angle);
    let s = sin(angle);
    return vec2<f32>(c * v.x - s * v.y, s * v.x + c * v.y);
}

// A random number from 0 to 1 for each point.
fn hash21(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

// Smooth value noise from 0 to 1.
fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash21(i);
    let b = hash21(i + vec2<f32>(1.0, 0.0));
    let c = hash21(i + vec2<f32>(0.0, 1.0));
    let d = hash21(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Four octaves of noise, for clouds and flowing shapes.
fn fbm(p: vec2<f32>) -> f32 {
    var total = 0.0;
    var amplitude = 0.5;
    var q = p;
    for (var i = 0; i < 4; i++) {
        total += noise(q) * amplitude;
        q = rotate(q, 0.6) * 2.03 + 17.0;
        amplitude *= 0.5;
    }
    return total / 0.9375;
}

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// Turns a colour around the colour wheel by `turns` (1.0 is all the way round). Works on bright HDR colours too.
fn hue_shift(c: vec3<f32>, turns: f32) -> vec3<f32> {
    let k = vec3<f32>(0.57735027);
    let a = turns * TAU;
    let ca = cos(a);
    return max(c * ca + cross(k, c) * sin(a) + k * dot(k, c) * (1.0 - ca), vec3<f32>(0.0));
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> vec3<f32> {
    let k = vec3<f32>(1.0, 2.0 / 3.0, 1.0 / 3.0);
    let p = abs(fract(vec3<f32>(h) + k) * 6.0 - 3.0);
    return v * mix(vec3<f32>(1.0), clamp(p - 1.0, vec3<f32>(0.0), vec3<f32>(1.0)), s);
}

fn hit_uv(i: u32) -> vec2<f32> {
    return fx.hits[i].xy;
}

fn hit_age(i: u32) -> f32 {
    return fx.hits[i].z;
}

fn hit_strength(i: u32) -> f32 {
    return fx.hits[i].w;
}

fn hit_color(i: u32) -> vec3<f32> {
    return fx.hit_colors[i].rgb;
}

fn bass() -> f32 {
    return fx.audio.x;
}

fn mid() -> f32 {
    return fx.audio.y;
}

fn treble() -> f32 {
    return fx.audio.z;
}

// What is at `uv`: x and y are a position of its own (-1 to 1 across a ball, turning with its
// pattern; across a wall's own size), z says which thing it is (its id plus one) plus half its
// random number, and w how much of the pixel it covers. All zero where there is nothing.
fn object_at(uv: vec2<f32>) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(object_tex));
    let p = clamp(vec2<i32>(floor(uv * vec2<f32>(size))), vec2<i32>(0), size - 1);
    return textureLoad(object_tex, p, 0);
}

// 0 nothing, else the thing's id plus one.
fn object_code(uv: vec2<f32>) -> f32 {
    return floor(object_at(uv).z);
}

// A random number from 0 to 1 for each ball, the same in every frame.
fn object_seed(uv: vec2<f32>) -> f32 {
    return fract(object_at(uv).z) * 2.0;
}

// How much of the effect shows at `uv`: 1 everywhere for the whole picture, else how much of
// the pixel the effect's thing covers.
fn target_weight(uv: vec2<f32>) -> f32 {
    if fx.apply_to < 0.5 {
        return 1.0;
    }
    let o = object_at(uv);
    return select(0.0, o.w, abs(floor(o.z) - fx.apply_to) < 0.5);
}

// The streaks the lens pre-pass drew from the bright parts (`// lens: rays`), at `uv`.
fn lens_rays(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(lens_rays_tex, samp, uv, 0.0).rgb;
}

// The bright parts (`// lens: soft`) at `uv`, softened: level 0 is half the picture's size, each
// level up half as sharp again (up to 4; in between mixes the two).
fn lens_soft(uv: vec2<f32>, level: f32) -> vec3<f32> {
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return vec3<f32>(0.0);
    }
    return textureSampleLevel(lens_soft_tex, samp, uv, clamp(level, 0.0, 4.0)).rgb;
}

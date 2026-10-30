// Shared by every effect. The effect file is pasted below this, and must define
//     fn effect(uv: vec2<f32>, p: Params) -> vec3<f32>
// which returns the colour for the pixel at `uv` (0,0 top left, 1,1 bottom right).
// `p` holds the values of the settings declared with `// param`, `// simple` and `// color`
// lines at the top of the file.

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
    // Picture pixels per video pixel (below 1 in the editor preview).
    scale: f32,
    // How strong the hits are right now, 0 to 1 (the music's loudness in a later stage).
    level: f32,
    // Which block a node effect shows in the Shaders workspace; 0 for the finished effect.
    view: u32,
    // What the effect changes: 0 the whole picture, else one thing's code (its id plus one).
    // Outside that thing the picture is left as it was.
    apply_to: f32,
    // 1 when the picture is a blend layer (premultiplied, its alpha how much the layer covers),
    // else 0.
    in_layer: f32,
    // The hit pulse three times over and 0 (bass, mid, treble and beat phase once there is
    // music).
    audio: vec4<f32>,
    params: array<vec4<f32>, 4>,
    // Newest hits first: position (uv), seconds since the hit, strength 0 to 1.
    hits: array<vec4<f32>, 16>,
    hit_colors: array<vec4<f32>, 16>,
};

@group(0) @binding(0) var<uniform> fx: Fx;
@group(0) @binding(1) var src_tex: texture_2d<f32>;
@group(0) @binding(2) var prev_tex: texture_2d<f32>;
@group(0) @binding(3) var samp: sampler;
@group(0) @binding(4) var object_tex: texture_2d<f32>;

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

// The picture coming into this effect.
fn sample(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(src_tex, samp, fold_uv(uv), 0.0).rgb;
}

// This effect's own output from the previous frame, for feedback and echo effects.
fn prev(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(prev_tex, samp, uv, 0.0).rgb;
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

fn beat_phase() -> f32 {
    return fx.audio.w;
}

// 1 on each beat, fading quickly to 0 before the next.
fn beat_hit() -> f32 {
    return pow(1.0 - fx.audio.w, 4.0);
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

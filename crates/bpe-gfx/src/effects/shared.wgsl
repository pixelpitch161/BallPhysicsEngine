// The helpers effects call that read the same in a full-picture effect (after prelude.wgsl) and
// in a Shader layer inside a ball or a shape (after in_ball.wgsl): the beat, the sound, hits,
// things, hashing and noise, hue turns. Put after either one (see `effects::PRELUDE` and
// `effects::IN_BALL`), with shaders/common.wgsl.

// Folds coordinates outside 0..1 back in like a mirror, so warped edges never smear.
fn fold_uv(uv: vec2<f32>) -> vec2<f32> {
    return 1.0 - abs(1.0 - abs(uv) % vec2<f32>(2.0));
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

// 0 nothing, else the thing's id plus one.
fn object_code(uv: vec2<f32>) -> f32 {
    return floor(object_at(uv).z);
}

// A random number from 0 to 1 for each ball, the same in every frame.
fn object_seed(uv: vec2<f32>) -> f32 {
    return fract(object_at(uv).z) * 2.0;
}

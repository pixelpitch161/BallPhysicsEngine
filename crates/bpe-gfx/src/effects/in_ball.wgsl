// What effects call (see prelude.wgsl), for a Shader layer inside a ball. The effect's picture is
// the stack under the shader layer: `sample(uv)` walks the records under it again at the ball
// point `uv` maps to (leaving out other shader layers, as a walk can't run itself). `uv` covers
// the ball's square, 0 to 1, scaled by the layer's Scale about the middle; it turns with the ball
// or stays upright on the screen. For a group shader `uv` covers the group's square instead
// (upright), so one effect spreads across every ball; `sample` still sees only this ball. `prev`
// has no history here and samples the same picture.

const MAX_HITS: u32 = 16u;

// Matches `Fx` in prelude.wgsl and chain.rs.
struct Fx {
    resolution: vec2<f32>,
    time: f32,
    blend: f32,
    pulse: f32,
    hit_count: u32,
    frame: f32,
    dt: f32,
    scale: f32,
    level: f32,
    view: u32,
    apply_to: f32,
    in_layer: f32,
    audio: vec4<f32>,
    params: array<vec4<f32>, 4>,
    hits: array<vec4<f32>, 16>,
    hit_colors: array<vec4<f32>, 16>,
};

// The frame's values: its time, its hits and how they pulse.
@group(2) @binding(3) var<uniform> fx_frame: Fx;

// The values the running effect sees: the frame's, with its own time and a square picture.
var<private> fx: Fx;

// The shader layer being run: its ball's first record, how many records are under it, its
// Scale, whether it stays upright on the screen, and whether it spreads across the group.
struct InBall {
    first: u32,
    below: u32,
    scale: f32,
    screen: f32,
    group: f32,
    // 1 for a shader layer inside a shape (see in_shape.wgsl).
    shape: f32,
};

var<private> in_ball: InBall;

// Sets up the effect of shader record `l`, the `k`th record of a ball whose records start at
// `first`, and gives the uv of ball point `q` (turning with it) or `screen` (upright).
fn in_ball_begin(l: LayerGpu, first: u32, k: u32, q: vec2<f32>, screen: vec2<f32>) -> vec2<f32> {
    in_ball = InBall(first, k, max(l.p0.y, 0.1), l.p0.z, 0.0, 0.0);
    fx = fx_frame;
    fx.time = fx_frame.time * l.p0.x;
    let side = max(2.0 * layered_ball.radius.x, 1.0);
    fx.resolution = vec2<f32>(side, side);
    // Output pixels per world unit (a video pixel at zoom 1), so pixel-sized effects (Pixels,
    // Film grain) look the same in the preview and the export.
    fx.scale = 1.0 / max(g.view.z, 1e-6);
    fx.blend = 1.0;
    fx.apply_to = 0.0;
    fx.in_layer = 0.0;
    let pt = select(q, screen, in_ball.screen > 0.5);
    return pt * 0.5 / in_ball.scale + vec2<f32>(0.5);
}

// Sets up group shader record `l` of a ball whose records start at `first`, with `below`
// records under it for `sample`, and gives the uv of upright ball point `screen` (in radii) in
// the group's square.
fn group_begin(l: LayerGpu, first: u32, below: u32, screen: vec2<f32>) -> vec2<f32> {
    in_ball = InBall(first, below, max(l.p0.y, 0.1), 1.0, 1.0, 0.0);
    fx = fx_frame;
    fx.time = fx_frame.time * l.p0.x;
    let side = max(layered_ball.place.z, 1e-3);
    // Output pixels per world unit.
    let px = layered_ball.radius.x / max(layered_ball.radius.y, 1e-3);
    fx.resolution = vec2<f32>(max(side * px, 1.0), max(side * px, 1.0));
    // Output pixels per world unit (a video pixel at zoom 1), so pixel-sized effects (Pixels,
    // Film grain) look the same in the preview and the export.
    fx.scale = 1.0 / max(g.view.z, 1e-6);
    fx.blend = 1.0;
    fx.apply_to = 0.0;
    fx.in_layer = 0.0;
    let world = layered_ball.place.xy + screen * layered_ball.radius.y;
    return world / side / in_ball.scale + vec2<f32>(0.5);
}

// How much of group shader `l` reaches upright ball point `screen` (in radii): 1 where it
// covers the group, 0 where it doesn't. `p1.y` is how much of the group it leaves uncovered (0:
// the whole group is covered), `p1.z` the side it covers from (radians: 0 from the left), `p1.w`
// how soft the edge is (as a share of the group).
fn group_covers(l: LayerGpu, screen: vec2<f32>) -> f32 {
    let covers = 1.0 - clamp(l.p1.y, 0.0, 1.0);
    if covers >= 1.0 {
        return 1.0;
    }
    let side = max(layered_ball.place.z, 1e-3);
    let at = (layered_ball.place.xy + screen * layered_ball.radius.y) / side;
    let dir = vec2<f32>(cos(l.p1.z), sin(l.p1.z));
    // 0 on the side it covers from to 1 on the far side, across the group's square.
    let t = dot(at, dir) / max(abs(dir.x) + abs(dir.y), 1e-3) + 0.5;
    let e = max(0.5 * clamp(l.p1.w, 0.0, 1.0), 1e-3);
    // The edge runs from wholly before the group (covers 0) to wholly past it (covers 1), so
    // animating Covers wipes smoothly from end to end.
    let cut = mix(-e, 1.0 + e, covers);
    return 1.0 - smoothstep(cut - e, cut + e, t);
}

// The quad point (see layers_walk.wgsl) a shader uv stands for.
fn in_ball_local(uv: vec2<f32>) -> vec2<f32> {
    var pt = (uv - vec2<f32>(0.5)) * 2.0 * in_ball.scale;
    if in_ball.group > 0.5 {
        // From the group's square back to this ball, in its radii.
        let world = (uv - vec2<f32>(0.5)) * in_ball.scale * max(layered_ball.place.z, 1e-3);
        pt = (world - layered_ball.place.xy) / max(layered_ball.radius.y, 1e-3);
    }
    if in_ball.screen > 0.5 {
        let dir = layered_ball.dir;
        return vec2<f32>(dot(pt, dir), dot(pt, vec2<f32>(-dir.y, dir.x)));
    }
    return rotate(pt, layered_ball.form.z);
}

// How much of the ball's shape covers quad point `local`.
fn in_ball_cover(local: vec2<f32>) -> f32 {
    let d = ball_shape(rotate(local, -layered_ball.form.z), layered_ball.form.x, layered_ball.form.y);
    return clamp(-d / layered_ball.aa + 0.5, 0.0, 1.0);
}

fn fold_uv(uv: vec2<f32>) -> vec2<f32> {
    return 1.0 - abs(1.0 - abs(uv) % vec2<f32>(2.0));
}

// The layers under the shader layer at `uv`. A group shader sees them as if the ball's shape
// went on past its edge, so a ring Around it is lit by the ball's own colours.
fn sample(uv: vec2<f32>) -> vec3<f32> {
    if in_ball.shape > 0.5 {
        return shape_sample(uv);
    }
    let local = in_ball_local(uv);
    let w = walk_plain(local, in_ball.first, in_ball.below, 0u);
    if in_ball.group > 0.5 {
        return w.inner.rgb;
    }
    let cover = in_ball_cover(local);
    return (cover * w.inner + (1.0 - cover) * w.outer).rgb;
}

fn prev(uv: vec2<f32>) -> vec3<f32> {
    return sample(uv);
}

fn aspect() -> f32 {
    return fx.resolution.x / fx.resolution.y;
}

fn to_centered(uv: vec2<f32>) -> vec2<f32> {
    return (uv - 0.5) * vec2<f32>(aspect(), 1.0);
}

fn from_centered(q: vec2<f32>) -> vec2<f32> {
    return q / vec2<f32>(aspect(), 1.0) + 0.5;
}

fn hash21(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

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

// The ball itself at `uv`: its own position there (-1 to 1, turning with it), its thing code and
// number, and how much its shape covers.
fn object_at(uv: vec2<f32>) -> vec4<f32> {
    if in_ball.shape > 0.5 {
        return shape_object_at(uv);
    }
    let local = in_ball_local(uv);
    let q = rotate(local, -layered_ball.form.z);
    return vec4<f32>(q, layered_ball.misc.y, in_ball_cover(local));
}

fn object_code(uv: vec2<f32>) -> f32 {
    return floor(object_at(uv).z);
}

fn object_seed(uv: vec2<f32>) -> f32 {
    return fract(object_at(uv).z) * 2.0;
}

fn target_weight(uv: vec2<f32>) -> f32 {
    return 1.0;
}

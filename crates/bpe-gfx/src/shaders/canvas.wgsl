// The canvas composite: draws one canvas's rectangle of the world with the Flat style. The paint
// texture holds the marks' colour times their amount, and the amount; the aux texture's r holds
// the time each texel was painted. The output is premultiplied colour and coverage.

struct Globals {
    // x: 2 / video width, y: 2 / video height, z: world units per output pixel, w: time
    view: vec4<f32>,
    bg_paint: vec4<f32>,
    bg_place: vec4<f32>,
    post: vec4<f32>,
    // centre xy, zoom, turn
    camera: vec4<f32>,
};

struct Canvas {
    // Centre xy in world pixels, turn in radians.
    pose: vec4<f32>,
    // Width and height in world pixels, fade in seconds (0: marks stay), opacity.
    size: vec4<f32>,
    // The colour the canvas starts as, straight; alpha 0 when it starts clear.
    start: vec4<f32>,
    // x: the scene time in seconds.
    clock: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var<uniform> canvas: Canvas;
@group(1) @binding(1) var paint: texture_2d<f32>;
@group(1) @binding(2) var aux: texture_2d<f32>;
@group(1) @binding(3) var smooth_sampler: sampler;

// The camera (centre xy, zoom, turn) turns a world point into clip space, as in scene.wgsl.
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

struct CanvasOut {
    @builtin(position) pos: vec4<f32>,
    // 0 to 1 across the canvas, (0, 0) at its top left texel.
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_canvas(@builtin(vertex_index) i: u32) -> CanvasOut {
    let corner = vec2<f32>(f32(i & 1u), f32((i >> 1u) & 1u));
    let local = (corner - 0.5) * canvas.size.xy;
    let c = cos(canvas.pose.z);
    let s = sin(canvas.pose.z);
    let world = canvas.pose.xy + vec2<f32>(c * local.x - s * local.y, s * local.x + c * local.y);
    var out: CanvasOut;
    out.pos = to_clip(world);
    out.uv = corner;
    return out;
}

@fragment
fn fs_canvas(in: CanvasOut) -> @location(0) vec4<f32> {
    let p = textureSample(paint, smooth_sampler, in.uv);
    let painted = textureSample(aux, smooth_sampler, in.uv).r;
    var fade = 1.0;
    if canvas.size.z > 0.0 {
        fade = clamp(1.0 - (canvas.clock.x - painted) / canvas.size.z, 0.0, 1.0);
    }
    // Flat: the marks' own colour, as much as was put down.
    let colour = max(p.rgb, vec3<f32>(0.0)) / max(p.a, 1e-4);
    let amount = clamp(p.a, 0.0, 1.0) * fade;
    // The start colour shows under the marks.
    let under = clamp(canvas.start.a, 0.0, 1.0) * (1.0 - amount);
    let rgb = colour * amount + canvas.start.rgb * under;
    return vec4<f32>(rgb, amount + under) * clamp(canvas.size.w, 0.0, 1.0);
}

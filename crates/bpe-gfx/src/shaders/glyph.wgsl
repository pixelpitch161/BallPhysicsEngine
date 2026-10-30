// Text: instanced quads reading a signed-distance atlas, drawn last into the HDR target.
// World coordinates are video pixels, origin at the frame centre, y down.

struct Globals {
    // x: 2 / video width, y: 2 / video height, z: world units per output pixel, w: time
    view: vec4<f32>,
    bg_top: vec4<f32>,
    bg_bottom: vec4<f32>,
    post: vec4<f32>,
    // centre xy, zoom, turn
    camera: vec4<f32>,
    // x: background on screen, y, z: world heights it spans
    bg: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var atlas: texture_2d<f32>;
@group(1) @binding(1) var atlas_sampler: sampler;

// The camera (centre xy, zoom, turn) turns a world point into screen units; with no camera
// (centre 0, zoom 1, turn 0) the point is unchanged, exactly.
fn to_screen(p: vec2<f32>) -> vec2<f32> {
    let c = g.camera;
    var d = p - c.xy;
    if c.w != 0.0 {
        let s = sin(-c.w);
        let k = cos(-c.w);
        d = vec2<f32>(d.x * k - d.y * s, d.x * s + d.y * k);
    }
    return d * c.z;
}

fn to_clip(p: vec2<f32>) -> vec4<f32> {
    let q = to_screen(p);
    return vec4<f32>(q.x * g.view.x, -q.y * g.view.y, 0.0, 1.0);
}

struct GlyphIn {
    // World rectangle: left, top, right, bottom.
    @location(0) rect: vec4<f32>,
    // Atlas rectangle: left, top, right, bottom.
    @location(1) uv: vec4<f32>,
    @location(2) color: vec4<f32>,
    @location(3) outline: vec4<f32>,
    // Outline width (world), glow, world units per distance unit, unused.
    @location(4) params: vec4<f32>,
    // The gradient painted inside the fill, as `PaintGpu`.
    @location(5) paint_a: vec4<f32>,
    @location(6) paint_b: vec4<f32>,
    // The world rectangle the paint spans, -1 to 1 across it: left, top, right, bottom.
    @location(7) span: vec4<f32>,
};

struct GlyphOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) color: vec4<f32>,
    @location(2) @interpolate(flat) outline: vec4<f32>,
    @location(3) @interpolate(flat) params: vec4<f32>,
    @location(4) @interpolate(flat) paint_a: vec4<f32>,
    @location(5) @interpolate(flat) paint_b: vec4<f32>,
    // Where the pixel is in the paint's rectangle, -1 to 1.
    @location(6) local: vec2<f32>,
};

@vertex
fn vs_glyph(@builtin(vertex_index) i: u32, q: GlyphIn) -> GlyphOut {
    let c = vec2<f32>(f32(i & 1u), f32((i >> 1u) & 1u));
    let world = mix(q.rect.xy, q.rect.zw, c);
    var out: GlyphOut;
    out.pos = to_clip(world);
    out.uv = mix(q.uv.xy, q.uv.zw, c);
    out.color = q.color;
    out.outline = q.outline;
    out.params = q.params;
    out.paint_a = q.paint_a;
    out.paint_b = q.paint_b;
    let half = max((q.span.zw - q.span.xy) * 0.5, vec2<f32>(1e-3));
    out.local = (world - (q.span.xy + q.span.zw) * 0.5) / half;
    return out;
}

@fragment
fn fs_glyph(in: GlyphOut) -> @location(0) vec4<f32> {
    let sample = textureSample(atlas, atlas_sampler, in.uv).r;
    let px = g.view.z;
    // Signed distance inside the glyph's edge, in world units.
    let d = (sample - 0.5) * in.params.z;
    let aa = 0.7 * px;
    let fill = smoothstep(-aa, aa, d);
    let outline = max(in.params.x, 0.0);
    let edge = smoothstep(-outline - aa, -outline + aa, d);
    let glow = in.params.y;
    var halo = 0.0;
    if glow > 1e-4 {
        halo = exp(-max(-d, 0.0) / (glow * 24.0)) * glow * 0.6;
    }
    let fill_a = abs(in.color.a) * fill;
    let outline_a = abs(in.outline.a) * edge * (1.0 - fill_a);
    // A gradient painted inside takes the place of the fill's colour; the halo keeps the colour.
    var fill_rgb = in.color.rgb;
    let paint = paint_from(in.paint_a, in.paint_b);
    if paint.row >= 0.0 {
        fill_rgb = paint_colour(paint, in.local, g.view.w).rgb;
    }
    let rgb = fill_rgb * fill_a + in.outline.rgb * outline_a + in.color.rgb * halo;
    var a = fill_a + outline_a;
    if a <= 0.0 && halo < 1e-4 {
        discard;
    }
    // An additive colour adds its light and hides nothing.
    if in.color.a < 0.0 {
        a = select(outline_a, 0.0, in.outline.a < 0.0);
    }
    return vec4<f32>(rgb, a);
}

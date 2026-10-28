// Captions: one quad per letter, cut out of a signed distance field atlas so letters stay sharp
// at any size and can have an outline and a soft glow.

struct TextGlobals {
    // 2 / video width, 2 / video height, world units per output pixel, unused.
    view: vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: TextGlobals;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

struct Glyph {
    // Corners in world units: left, top, right, bottom.
    @location(0) rect: vec4<f32>,
    // Atlas coordinates of the same corners.
    @location(1) uv: vec4<f32>,
    // Straight (not premultiplied) colours.
    @location(2) color: vec4<f32>,
    @location(3) outline_color: vec4<f32>,
    // Output pixels per atlas pixel, outline width in output pixels, glow strength, glow reach in
    // output pixels.
    @location(4) params: vec4<f32>,
};

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) outline_color: vec4<f32>,
    @location(3) params: vec4<f32>,
};

// Distance field values: 0.5 is the letter's edge, and the field reaches this many atlas pixels
// either side of it. Must match SPREAD in text.rs.
const SPREAD: f32 = 10.0;

@vertex
fn vs_glyph(@builtin(vertex_index) index: u32, glyph: Glyph) -> VertexOut {
    let corner = vec2<f32>(f32(index & 1u), f32(index >> 1u));
    let world = mix(glyph.rect.xy, glyph.rect.zw, corner);
    var out: VertexOut;
    out.position = vec4<f32>(world.x * globals.view.x, -world.y * globals.view.y, 0.0, 1.0);
    out.uv = mix(glyph.uv.xy, glyph.uv.zw, corner);
    out.color = glyph.color;
    out.outline_color = glyph.outline_color;
    out.params = glyph.params;
    return out;
}

@fragment
fn fs_glyph(in: VertexOut) -> @location(0) vec4<f32> {
    let field = textureSample(atlas, atlas_sampler, in.uv).r;
    // Distance from the letter's edge in output pixels, positive inside.
    let inside = (field - 0.5) * 2.0 * SPREAD * in.params.x;
    let outline = in.params.y;
    let fill = clamp(inside + 0.5, 0.0, 1.0);
    let edge = clamp(inside + outline + 0.5, 0.0, 1.0);
    let rgb = mix(in.outline_color.rgb, in.color.rgb, select(fill, 1.0, outline <= 0.0));
    let body = select(edge * mix(in.outline_color.a, in.color.a, fill), fill * in.color.a, outline <= 0.0);
    // Glow fades out from the outer edge, reaching nothing before the edge of the quad. It only
    // adds light, so it never darkens what is behind it.
    let outside = max(-(inside + outline), 0.0);
    let reach = max(in.params.w, 0.5);
    let fade = 1.0 - smoothstep(0.0, 1.0, outside / reach);
    let glow = in.params.z * exp(-outside / reach * 2.0) * fade * (1.0 - edge);
    let color = rgb * body + in.color.rgb * glow * in.color.a;
    return vec4<f32>(color, clamp(body, 0.0, 1.0));
}

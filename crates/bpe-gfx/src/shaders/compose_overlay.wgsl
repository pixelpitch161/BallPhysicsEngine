// The On top lane (video part 4): one clip's finished picture drawn over the composed video inside
// a rounded frame with a border, blended by its alpha. Line for line `compose::overlay_reference`.

struct Overlay {
    // The frame in output pixels: x, y, width, height.
    rect: vec4<f32>,
    // The part of the picture shown: x, y, width, height (0 to 1).
    crop: vec4<f32>,
    border_colour: vec4<f32>,
    // Corner radius (px), border width (px), alpha, unused.
    shape: vec4<f32>,
}

@group(0) @binding(0) var picture: texture_2d<f32>;
@group(0) @binding(1) var pictures: sampler;
@group(0) @binding(2) var<uniform> overlay: Overlay;

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_overlay(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let rect = overlay.rect;
    let half = rect.zw * 0.5;
    let centre = rect.xy + half;
    let r = clamp(overlay.shape.x, 0.0, min(half.x, half.y));
    let q = abs(pos.xy - centre) - half + vec2<f32>(r);
    let d = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
    let cover = clamp(0.5 - d, 0.0, 1.0) * overlay.shape.z;
    if cover <= 0.0 {
        discard;
    }
    let uv = clamp((pos.xy - rect.xy) / rect.zw, vec2<f32>(0.0), vec2<f32>(1.0));
    var colour = textureSampleLevel(picture, pictures, overlay.crop.xy + uv * overlay.crop.zw, 0.0).rgb;
    let border = overlay.shape.y;
    if border > 0.0 {
        let b = clamp(border + d + 0.5, 0.0, 1.0);
        colour = mix(colour, overlay.border_colour.rgb, b);
    }
    return vec4<f32>(clamp(colour, vec3<f32>(0.0), vec3<f32>(1.0)), cover);
}

// Styled balls: a shape, a fill, an outline and a halo per ball, stretched along the way it
// moves. Appended to scene.wgsl's module (and ball_look.wgsl's shapes and patterns).

struct LookIn {
    @location(0) center: vec2<f32>,
    @location(1) radius: f32,
    @location(2) seed: f32,
    @location(3) color: vec4<f32>,
    @location(4) color2: vec4<f32>,
    @location(5) outline_color: vec4<f32>,
    // Shape, fill, pattern size, roundness.
    @location(6) form: vec4<f32>,
    // Outline width in pixels, glow, opacity, angle.
    @location(7) look: vec4<f32>,
    // Stretch direction, scale along it and across it.
    @location(8) stretch: vec4<f32>,
    // Glow size in radii, thing code.
    @location(9) extent: vec4<f32>,
};

struct LookOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) radius_px: f32,
    @location(3) @interpolate(flat) color2: vec4<f32>,
    @location(4) @interpolate(flat) outline_color: vec4<f32>,
    @location(5) @interpolate(flat) form: vec4<f32>,
    @location(6) @interpolate(flat) look: vec4<f32>,
    // Stretch direction, glow size in radii, outline width in radii.
    @location(7) @interpolate(flat) misc: vec4<f32>,
    // Thing code and seed, for the object picture.
    @location(8) @interpolate(flat) code: vec2<f32>,
};

// How far past the shape's edge the halo reaches, in glow sizes.
const HALO_REACH: f32 = 2.2;

@vertex
fn vs_look(@builtin(vertex_index) i: u32, b: LookIn) -> LookOut {
    let radius = max(b.radius, 1e-3);
    let outline = b.look.x / radius;
    let margin = 2.0 * g.view.z / radius;
    // Shapes reach a little past one radius; the halo reaches further.
    let reach = 1.35 + HALO_REACH * max(b.extent.x, 0.0) * step(1e-4, b.look.y) + outline + margin;
    let corner = quad_corner(i) * reach;
    let dir = b.stretch.xy;
    let side = vec2<f32>(-dir.y, dir.x);
    let offset = dir * corner.x * b.stretch.z + side * corner.y * b.stretch.w;
    var out: LookOut;
    out.pos = to_clip(b.center + offset * b.radius);
    out.local = corner;
    out.color = b.color;
    out.radius_px = b.radius / g.view.z;
    out.color2 = b.color2;
    out.outline_color = b.outline_color;
    out.form = b.form;
    out.look = b.look;
    out.misc = vec4<f32>(dir, b.extent.x, outline);
    out.code = vec2<f32>(b.extent.y, b.seed);
    return out;
}

@fragment
fn fs_look(in: LookOut) -> @location(0) vec4<f32> {
    let angle = in.look.w;
    let p = rotate(in.local, -angle);
    let d = ball_shape(p, in.form.x, in.form.w);
    let aa = 1.0 / max(in.radius_px, 0.5);
    let cover = clamp(-d / aa + 0.5, 0.0, 1.0);

    let fill = in.form.y;
    var color = in.color.rgb;
    if fill > 1.5 {
        color = mix(in.color.rgb, in.color2.rgb, pattern(p, fill, in.form.z));
    }
    if fill > 0.5 && fill < 1.5 {
        // Lit as the ball sits on screen, whichever way it is stretched.
        let dir = in.misc.xy;
        let screen = dir * in.local.x + vec2<f32>(-dir.y, dir.x) * in.local.y;
        color = sphere_light(screen / max(length(screen), 1.0), color);
    }
    let outline = in.misc.w;
    var glow_color = in.color.rgb;
    if outline > 0.0 {
        let fill_part = clamp((-d - outline) / aa + 0.5, 0.0, 1.0);
        color = mix(in.outline_color.rgb, color, fill_part);
        glow_color = in.outline_color.rgb;
    }
    // An additive fill colour makes the whole ball add its light.
    let opacity = in.look.z * select(1.0, -in.color.a, in.color.a < 0.0);
    let glow_d = max(d, 0.0) / max(in.misc.z, 0.05);
    let halo = in.look.y * exp(-glow_d * glow_d) * (1.0 - cover);
    if cover <= 0.0 && halo < 1e-4 {
        discard;
    }
    return vec4<f32>((color * cover + glow_color * halo) * opacity, covering(cover * opacity, in.color.a));
}

// A styled ball's own position: -1 to 1 across it, turning with its pattern.
@fragment
fn fs_look_object(in: LookOut) -> @location(0) vec4<f32> {
    let p = rotate(in.local, -in.look.w);
    let aa = 1.0 / max(in.radius_px, 0.5);
    let cover = clamp(-ball_shape(p, in.form.x, in.form.w) / aa + 0.5, 0.0, 1.0) * step(1e-4, in.look.z);
    if cover <= 0.0 || in.code.x < 0.5 {
        discard;
    }
    return vec4<f32>(p, in.code.x, cover);
}

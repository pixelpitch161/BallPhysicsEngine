// The liquid's surface, drawn in screen space: particles are splatted as soft blobs into a
// thickness picture, blurred, and where the thickness passes a threshold the liquid is shaded
// with its colour, a shine, and a bent view of what is behind it. Its colour can come from a
// gradient, by each pixel's place in the world or by the mean speed of the particles under it
// (splatted and blurred alongside the thickness). paint.wgsl comes first, at group 1.

// Matches `SurfaceUniform` in flow/surface.rs.
struct Surface {
    // xy: 2 / frame size, z: blob radius in world pixels, w: blob strength
    view: vec4<f32>,
    // rgb: liquid colour (linear), a: clarity
    color: vec4<f32>,
    // x: refraction, y: highlight, z: blur step in texels, w: particles
    look: vec4<f32>,
    // xy: blur direction, z: steps over which the liquid pours in, w: current step
    blur: vec4<f32>,
    // x: glow, y: what the colour follows (0 the one colour above, 1 place along a direction,
    // 2 distance from a centre, 3 speed)
    extra: vec4<f32>,
    // centre xy, zoom, turn
    camera: vec4<f32>,
    // The paint, as `PaintGpu`'s fields in order.
    paint_a: vec4<f32>,
    paint_b: vec4<f32>,
    // x: direction in degrees, y: span, zw: centre
    place: vec4<f32>,
};

@group(0) @binding(0) var<uniform> surface: Surface;
@group(0) @binding(1) var<storage, read> particles: array<vec4<f32>>;
@group(0) @binding(2) var source: texture_2d<f32>;
@group(0) @binding(3) var behind: texture_2d<f32>;
@group(0) @binding(4) var linear: sampler;
// The speed picture that goes with `source`.
@group(0) @binding(5) var source_speed: texture_2d<f32>;

struct SplatOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    // The particle's speed over the paint's span.
    @location(1) speed: f32,
};

@vertex
fn vs_splat(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> SplatOut {
    var out: SplatOut;
    // A particle still waiting to pour is not drawn.
    let poured = surface.blur.z <= 0.0 || surface.blur.w >= floor(f32(ii) / surface.look.w * surface.blur.z);
    if !poured {
        out.pos = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        return out;
    }
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u)) * 2.0 - 1.0;
    let world = particles[ii].xy + corner * surface.view.z;
    // Through the camera; with none (centre 0, zoom 1, turn 0) the point is unchanged, exactly.
    let c = surface.camera;
    var q = world - c.xy;
    if c.w != 0.0 {
        let s = sin(-c.w);
        let k = cos(-c.w);
        q = vec2<f32>(q.x * k - q.y * s, q.x * s + q.y * k);
    }
    q = q * c.z;
    out.pos = vec4<f32>(q.x * surface.view.x, -q.y * surface.view.y, 0.0, 1.0);
    out.local = corner;
    // Capped, so the weighted sum cannot overflow a half float however small the span.
    out.speed = min(length(particles[ii].zw) / max(surface.place.y, 1e-3), 64.0);
    return out;
}

@fragment
fn fs_splat(in: SplatOut) -> @location(0) vec4<f32> {
    let r2 = dot(in.local, in.local);
    if r2 >= 1.0 {
        discard;
    }
    let w = exp(-4.0 * r2) - exp(-4.0);
    return vec4<f32>(w * surface.view.w, 0.0, 0.0, 0.0);
}

struct TwoOut {
    @location(0) thickness: vec4<f32>,
    @location(1) speed: vec4<f32>,
};

// The thickness, and the speed weighted by it.
@fragment
fn fs_splat_speed(in: SplatOut) -> TwoOut {
    let r2 = dot(in.local, in.local);
    if r2 >= 1.0 {
        discard;
    }
    let w = (exp(-4.0 * r2) - exp(-4.0)) * surface.view.w;
    return TwoOut(vec4<f32>(w, 0.0, 0.0, 0.0), vec4<f32>(w * in.speed, 0.0, 0.0, 0.0));
}

struct FullOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> FullOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: FullOut;
    out.pos = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(uv.x, 1.0 - uv.y);
    return out;
}

@fragment
fn fs_blur(in: FullOut) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(source));
    let step = surface.blur.xy * surface.look.z / size;
    var sum = 0.0;
    var total = 0.0;
    for (var k = -4; k <= 4; k++) {
        let w = exp(-f32(k * k) / 8.0);
        sum += textureSampleLevel(source, linear, in.uv + step * f32(k), 0.0).r * w;
        total += w;
    }
    return vec4<f32>(sum / total, 0.0, 0.0, 0.0);
}

// The same blur over the thickness and the speed.
@fragment
fn fs_blur_speed(in: FullOut) -> TwoOut {
    let size = vec2<f32>(textureDimensions(source));
    let step = surface.blur.xy * surface.look.z / size;
    var sum = 0.0;
    var speed = 0.0;
    var total = 0.0;
    for (var k = -4; k <= 4; k++) {
        let w = exp(-f32(k * k) / 8.0);
        let uv = in.uv + step * f32(k);
        sum += textureSampleLevel(source, linear, uv, 0.0).r * w;
        speed += textureSampleLevel(source_speed, linear, uv, 0.0).r * w;
        total += w;
    }
    return TwoOut(vec4<f32>(sum / total, 0.0, 0.0, 0.0), vec4<f32>(speed / total, 0.0, 0.0, 0.0));
}

// The liquid's colour at `uv` on the screen, whose thickness is `t`: its one colour, or its
// paint at the pixel's place in the world or at the mean speed there.
fn liquid_colour(uv: vec2<f32>, t: f32) -> vec3<f32> {
    let follow = surface.extra.y;
    if follow < 0.5 {
        return surface.color.rgb;
    }
    let paint = paint_from(surface.paint_a, surface.paint_b);
    let span = max(surface.place.y, 1e-3);
    if follow > 2.5 {
        return paint_along(paint, textureSampleLevel(source_speed, linear, uv, 0.0).r / max(t, 1e-4), 0.0).rgb;
    }
    // Back through the camera to the world (see vs_splat).
    let c = surface.camera;
    var q = (uv - 0.5) * vec2<f32>(2.0 / surface.view.x, 2.0 / surface.view.y) / c.z;
    if c.w != 0.0 {
        let s = sin(c.w);
        let k = cos(c.w);
        q = vec2<f32>(q.x * k - q.y * s, q.x * s + q.y * k);
    }
    let d = q + c.xy - surface.place.zw;
    if follow > 1.5 {
        return paint_along(paint, length(d) / span, 0.0).rgb;
    }
    let a = radians(surface.place.x);
    return paint_along(paint, dot(d, vec2<f32>(cos(a), sin(a))) / span + 0.5, 0.0).rgb;
}

fn thickness(uv: vec2<f32>) -> f32 {
    return textureSampleLevel(source, linear, uv, 0.0).r;
}

// Premultiplied: the liquid's light times how much of the pixel it covers, and that cover as
// alpha, so several liquids stack and the balls drawn later sit on top.
@fragment
fn fs_composite(in: FullOut) -> @location(0) vec4<f32> {
    let t = thickness(in.uv);
    let cover = smoothstep(0.32, 0.55, t);
    if cover <= 0.0 {
        discard;
    }
    let colour = liquid_colour(in.uv, t);
    let texel = 1.5 / vec2<f32>(textureDimensions(source));
    let slope = vec2<f32>(thickness(in.uv + vec2<f32>(texel.x, 0.0)) - thickness(in.uv - vec2<f32>(texel.x, 0.0)),
                          thickness(in.uv + vec2<f32>(0.0, texel.y)) - thickness(in.uv - vec2<f32>(0.0, texel.y)));
    let normal = normalize(vec3<f32>(-slope * 3.0, 1.0));
    // What is behind, seen bent through the surface and tinted by the liquid.
    let bent = in.uv + normal.xy * surface.look.x * 0.03;
    let seen = textureSampleLevel(behind, linear, bent, 0.0).rgb;
    let depth = clamp((t - 0.45) * 0.9, 0.0, 1.0);
    let tint = mix(vec3<f32>(1.0), colour, 0.6);
    let solid = (1.0 - surface.color.a) * (0.55 + 0.45 * depth);
    var liquid = mix(seen * tint, colour, solid);
    // A shine from the upper left, and a brighter rim at the edge.
    let light = normalize(vec3<f32>(-0.4, 0.55, 0.75));
    let half_vector = normalize(light + vec3<f32>(0.0, 0.0, 1.0));
    let shine = pow(max(dot(normal, half_vector), 0.0), 48.0) * surface.look.y;
    let rim = cover * (1.0 - cover) * 4.0 * surface.look.y * 0.35;
    liquid += vec3<f32>(shine) + colour * (rim + surface.extra.x);
    return vec4<f32>(liquid * cover, cover);
}

// The same surface drawn into a blend layer: only the liquid's own light (its colour where it
// is solid, the shine and the rim), covering as much as it is solid. Nothing of what is behind
// is bent in: the layer's composite mixes in the picture under it, and the clear part leaves
// that as it is.
@fragment
fn fs_composite_layer(in: FullOut) -> @location(0) vec4<f32> {
    let t = thickness(in.uv);
    let cover = smoothstep(0.32, 0.55, t);
    if cover <= 0.0 {
        discard;
    }
    let colour = liquid_colour(in.uv, t);
    let texel = 1.5 / vec2<f32>(textureDimensions(source));
    let slope = vec2<f32>(thickness(in.uv + vec2<f32>(texel.x, 0.0)) - thickness(in.uv - vec2<f32>(texel.x, 0.0)),
                          thickness(in.uv + vec2<f32>(0.0, texel.y)) - thickness(in.uv - vec2<f32>(0.0, texel.y)));
    let normal = normalize(vec3<f32>(-slope * 3.0, 1.0));
    let depth = clamp((t - 0.45) * 0.9, 0.0, 1.0);
    let solid = (1.0 - surface.color.a) * (0.55 + 0.45 * depth);
    let light = normalize(vec3<f32>(-0.4, 0.55, 0.75));
    let half_vector = normalize(light + vec3<f32>(0.0, 0.0, 1.0));
    let shine = pow(max(dot(normal, half_vector), 0.0), 48.0) * surface.look.y;
    let rim = cover * (1.0 - cover) * 4.0 * surface.look.y * 0.35;
    let own = colour * (solid + rim + surface.extra.x) + vec3<f32>(shine);
    return vec4<f32>(own * cover, solid * cover);
}

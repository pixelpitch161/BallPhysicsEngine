// The liquid's surface, drawn in screen space: particles are splatted as soft blobs into a
// thickness picture, blurred, and where the thickness passes a threshold the liquid is shaded
// with its colour, a shine, and a bent view of what is behind it.

// Matches `SurfaceUniform` in fluid.rs.
struct Surface {
    // xy: 2 / video size, z: blob radius in world pixels, w: blob strength
    view: vec4<f32>,
    // rgb: liquid colour (linear), a: clarity
    color: vec4<f32>,
    // x: refraction, y: highlight, z: blur step in texels, w: particles
    look: vec4<f32>,
    // xy: blur direction, z: steps over which the liquid pours in, w: current step
    blur: vec4<f32>,
};

@group(0) @binding(0) var<uniform> surface: Surface;
@group(0) @binding(1) var<storage, read> particles: array<vec4<f32>>;
@group(0) @binding(2) var source: texture_2d<f32>;
@group(0) @binding(3) var behind: texture_2d<f32>;
@group(0) @binding(4) var linear: sampler;

struct SplatOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
};

@vertex
fn vs_splat(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> SplatOut {
    var out: SplatOut;
    let poured = surface.blur.w >= floor(f32(ii) / surface.look.w * surface.blur.z);
    if !poured {
        out.pos = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        return out;
    }
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u)) * 2.0 - 1.0;
    let world = particles[ii].xy + corner * surface.view.z;
    out.pos = vec4<f32>(world.x * surface.view.x, -world.y * surface.view.y, 0.0, 1.0);
    out.local = corner;
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

fn thickness(uv: vec2<f32>) -> f32 {
    return textureSampleLevel(source, linear, uv, 0.0).r;
}

@fragment
fn fs_composite(in: FullOut) -> @location(0) vec4<f32> {
    let t = thickness(in.uv);
    let back = textureSampleLevel(behind, linear, in.uv, 0.0).rgb;
    let cover = smoothstep(0.32, 0.55, t);
    if cover <= 0.0 {
        discard;
    }
    let texel = 1.5 / vec2<f32>(textureDimensions(source));
    let slope = vec2<f32>(thickness(in.uv + vec2<f32>(texel.x, 0.0)) - thickness(in.uv - vec2<f32>(texel.x, 0.0)),
                          thickness(in.uv + vec2<f32>(0.0, texel.y)) - thickness(in.uv - vec2<f32>(0.0, texel.y)));
    let normal = normalize(vec3<f32>(-slope * 3.0, 1.0));
    // What is behind, seen bent through the surface and tinted by the liquid.
    let bent = in.uv + normal.xy * surface.look.x * 0.03;
    let seen = textureSampleLevel(behind, linear, bent, 0.0).rgb;
    let depth = clamp((t - 0.45) * 0.9, 0.0, 1.0);
    let tint = mix(vec3<f32>(1.0), surface.color.rgb, 0.6);
    let solid = (1.0 - surface.color.a) * (0.55 + 0.45 * depth);
    var liquid = mix(seen * tint, surface.color.rgb, solid);
    // A shine from the upper left, and a brighter rim at the edge.
    let light = normalize(vec3<f32>(-0.4, 0.55, 0.75));
    let half_vector = normalize(light + vec3<f32>(0.0, 0.0, 1.0));
    let shine = pow(max(dot(normal, half_vector), 0.0), 48.0) * surface.look.y;
    let rim = cover * (1.0 - cover) * 4.0 * surface.look.y * 0.35;
    liquid += vec3<f32>(shine) + surface.color.rgb * rim;
    return vec4<f32>(mix(back, liquid, cover), 1.0);
}

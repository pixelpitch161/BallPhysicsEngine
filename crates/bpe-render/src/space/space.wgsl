// The 3D view, drawn into the HDR target in linear light.
// World coordinates are video pixels: x right, y down, z away from the default camera.
// Depth is reversed: 1 at the near plane, 0 infinitely far away.

const PI: f32 = 3.14159265;
const TAU: f32 = 6.28318531;
const MAX_GATES: u32 = 24u;
const MAX_HITS: u32 = 24u;

struct Space {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    light_view_proj: mat4x4<f32>,
    // xyz camera position, w time
    camera: vec4<f32>,
    // picture width, height, tan(half vertical field of view), flash size
    screen: vec4<f32>,
    // xyz towards the key light, w intensity
    key_dir: vec4<f32>,
    // rgb key colour, w 1 when shadows are on
    key_color: vec4<f32>,
    // rgb light from above (times ambient), w rim strength
    ambient_top: vec4<f32>,
    // rgb light from below (times ambient), w ball glow
    ambient_bottom: vec4<f32>,
    // roughness, metalness, glass, edge glow
    material: vec4<f32>,
    // rgb glass colour, w inside wall brightness
    glass_color: vec4<f32>,
    // kind (0 sphere, 1 cube, 2 tunnel), size, turn, back wall
    container: vec4<f32>,
    bg_top: vec4<f32>,
    bg_bottom: vec4<f32>,
    // shadow depth per world pixel, world pixels per shadow texel, ring count, hit count
    light: vec4<f32>,
    // z, gap direction, half gap, ring spacing
    gates: array<vec4<f32>, MAX_GATES>,
    gate_colors: array<vec4<f32>, MAX_GATES>,
    // xyz, strength
    hits: array<vec4<f32>, MAX_HITS>,
    hit_colors: array<vec4<f32>, MAX_HITS>,
};

@group(0) @binding(0) var<uniform> u: Space;
@group(1) @binding(0) var shadow_map: texture_depth_2d;
@group(1) @binding(1) var shadow_sampler: sampler_comparison;
@group(1) @binding(2) var scene_depth: texture_depth_2d;

fn quad_corner(i: u32) -> vec2<f32> {
    return vec2<f32>(f32(i & 1u), f32((i >> 1u) & 1u)) * 2.0 - 1.0;
}

fn depth_of(p: vec3<f32>) -> f32 {
    let clip = u.view_proj * vec4<f32>(p, 1.0);
    return clamp(clip.z / clip.w, 0.0, 1.0);
}

// The direction from the camera through a pixel (in framebuffer coordinates).
fn ray_dir(frag: vec2<f32>) -> vec3<f32> {
    let ndc = vec2<f32>(frag.x / u.screen.x * 2.0 - 1.0, 1.0 - frag.y / u.screen.y * 2.0);
    let far = u.inv_view_proj * vec4<f32>(ndc, 0.5, 1.0);
    return normalize(far.xyz / far.w - u.camera.xyz);
}

// Size of one picture pixel, in world pixels, at a distance from the camera.
fn pixel_at(distance: f32) -> f32 {
    return 2.0 * distance * u.screen.z / u.screen.y;
}

fn rotate_y(p: vec3<f32>, angle: f32) -> vec3<f32> {
    let c = cos(angle);
    let s = sin(angle);
    return vec3<f32>(c * p.x + s * p.z, p.y, -s * p.x + c * p.z);
}

// Light from above and below, by direction.
fn sky_light(n: vec3<f32>) -> vec3<f32> {
    return mix(u.ambient_top.rgb, u.ambient_bottom.rgb, n.y * 0.5 + 0.5);
}

// What a mirror would show in direction r: the background, lit by the ambient light.
fn environment(r: vec3<f32>) -> vec3<f32> {
    let bg = mix(u.bg_top.rgb, u.bg_bottom.rgb, clamp(r.y * 0.5 + 0.5, 0.0, 1.0));
    return bg + sky_light(r) * 0.6;
}

fn shadow_at(p: vec3<f32>, n: vec3<f32>) -> f32 {
    if u.key_color.w < 0.5 {
        return 1.0;
    }
    let texel = u.light.y;
    let biased = p + n * texel * 1.5 + u.key_dir.xyz * texel * 1.5;
    let c = u.light_view_proj * vec4<f32>(biased, 1.0);
    let uv = vec2<f32>(c.x * 0.5 + 0.5, 0.5 - c.y * 0.5);
    if uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || c.z > 1.0 {
        return 1.0;
    }
    let size = vec2<f32>(textureDimensions(shadow_map));
    var lit = 0.0;
    for (var x = -1; x <= 1; x++) {
        for (var y = -1; y <= 1; y++) {
            let offset = vec2<f32>(f32(x), f32(y)) / size;
            lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + offset, c.z);
        }
    }
    return lit / 9.0;
}

// Physically based shading with one key light, sky light and a rim.
fn shade(albedo: vec3<f32>, n: vec3<f32>, v: vec3<f32>, shadow: f32) -> vec3<f32> {
    return shade_with(albedo, n, v, shadow, u.material.x, u.material.y);
}

// The same, with its own roughness and metalness.
fn shade_with(albedo: vec3<f32>, n: vec3<f32>, v: vec3<f32>, shadow: f32, roughness: f32, metalness: f32) -> vec3<f32> {
    let l = u.key_dir.xyz;
    let h = normalize(l + v);
    let rough = clamp(roughness, 0.04, 1.0);
    let metal = clamp(metalness, 0.0, 1.0);
    let a = rough * rough;
    let a2 = a * a;
    let ndl = max(dot(n, l), 0.0);
    let ndv = max(dot(n, v), 1e-4);
    let ndh = max(dot(n, h), 0.0);
    let vdh = max(dot(v, h), 0.0);
    let dd = ndh * ndh * (a2 - 1.0) + 1.0;
    let distribution = a2 / (PI * dd * dd);
    let k = (rough + 1.0) * (rough + 1.0) / 8.0;
    let geometry = ndv / (ndv * (1.0 - k) + k) * ndl / (ndl * (1.0 - k) + k);
    let f0 = mix(vec3<f32>(0.04), albedo, metal);
    let fresnel = f0 + (1.0 - f0) * pow(1.0 - vdh, 5.0);
    let specular = distribution * geometry * fresnel / (4.0 * ndv * ndl + 1e-4);
    let diffuse = (1.0 - fresnel) * (1.0 - metal) * albedo / PI;
    let key = u.key_color.rgb * u.key_dir.w;
    let direct = (diffuse + specular) * key * ndl * shadow;
    let ambient = sky_light(n) * albedo * (1.0 - metal);
    let reflect_f = f0 + (max(vec3<f32>(1.0 - rough), f0) - f0) * pow(1.0 - ndv, 5.0);
    let reflection = environment(reflect(-v, n)) * reflect_f * (1.0 - rough * 0.7);
    let rim = pow(1.0 - ndv, 3.0) * u.ambient_top.w * (key * 0.15 + u.ambient_top.rgb);
    return direct + ambient + reflection + rim;
}

// Light a recent hit throws onto a nearby surface.
fn hit_glow(p: vec3<f32>) -> vec3<f32> {
    var glow = vec3<f32>(0.0);
    let reach = max(u.screen.w, 1.0) * 2.5;
    let count = u32(u.light.w);
    for (var i = 0u; i < count; i++) {
        let d = distance(p, u.hits[i].xyz) / reach;
        let c = u.hit_colors[i].rgb;
        glow += (c + vec3<f32>(0.6)) * u.hits[i].w * exp(-d * d);
    }
    // Many hits in one spot saturate instead of blowing out.
    return glow / (1.0 + max(max(glow.r, glow.g), glow.b) * 0.5);
}

// Thin bright lines every `spacing`, about `width` wide, anti-aliased by `blur`.
fn lines(value: f32, spacing: f32, width: f32, blur: f32) -> f32 {
    let d = abs(fract(value / spacing + 0.5) - 0.5) * spacing;
    return 1.0 - smoothstep(width, width + blur, d);
}

// Where a ray from `ro` along `rd` meets a sphere around the origin: (near, far), or far < 0.
fn hit_sphere(ro: vec3<f32>, rd: vec3<f32>, radius: f32) -> vec2<f32> {
    let b = dot(ro, rd);
    let c = dot(ro, ro) - radius * radius;
    let disc = b * b - c;
    if disc < 0.0 {
        return vec2<f32>(-1.0, -1.0);
    }
    let s = sqrt(disc);
    return vec2<f32>(-b - s, -b + s);
}

// Where a ray meets a box of half size `half`, in the box's own frame: (near, far), or far < near.
fn hit_box(ro: vec3<f32>, rd: vec3<f32>, half: f32) -> vec2<f32> {
    let inv = 1.0 / select(rd, vec3<f32>(1e-6), abs(rd) < vec3<f32>(1e-6));
    let t1 = (vec3<f32>(-half) - ro) * inv;
    let t2 = (vec3<f32>(half) - ro) * inv;
    let lo = min(t1, t2);
    let hi = max(t1, t2);
    return vec2<f32>(max(max(lo.x, lo.y), lo.z), min(min(hi.x, hi.y), hi.z));
}

// Where a ray meets the tunnel's tube, which runs along z: (near, far), or far < 0.
fn hit_tube(ro: vec3<f32>, rd: vec3<f32>, radius: f32) -> vec2<f32> {
    let a = dot(rd.xy, rd.xy);
    if a < 1e-8 {
        return vec2<f32>(-1.0, -1.0);
    }
    let b = dot(ro.xy, rd.xy);
    let c = dot(ro.xy, ro.xy) - radius * radius;
    let disc = b * b - a * c;
    if disc < 0.0 {
        return vec2<f32>(-1.0, -1.0);
    }
    let s = sqrt(disc);
    return vec2<f32>((-b - s) / a, (-b + s) / a);
}

// How close a point on a box's surface is to one of its edges: the second smallest distance to a face.
fn box_edge(local: vec3<f32>, half: f32) -> f32 {
    let d = vec3<f32>(half) - abs(local);
    let lo = min(min(d.x, d.y), d.z);
    let hi = max(max(d.x, d.y), d.z);
    return d.x + d.y + d.z - lo - hi;
}

// ---- Shadow map: balls seen from the key light ----

struct BallIn {
    @location(0) center_radius: vec4<f32>,
    @location(1) color: vec4<f32>,
};

struct ShadowOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) corner: vec2<f32>,
    @location(1) @interpolate(flat) depth_radius: vec2<f32>,
};

@vertex
fn vs_shadow(@builtin(vertex_index) i: u32, b: BallIn) -> ShadowOut {
    let corner = quad_corner(i);
    let l = u.key_dir.xyz;
    var side = cross(l, vec3<f32>(0.0, -1.0, 0.0));
    if dot(side, side) < 1e-6 {
        side = vec3<f32>(1.0, 0.0, 0.0);
    }
    let right = normalize(side);
    let up = cross(right, l);
    let r = b.center_radius.w;
    let world = b.center_radius.xyz + (right * corner.x + up * corner.y) * r;
    let center = u.light_view_proj * vec4<f32>(b.center_radius.xyz, 1.0);
    var out: ShadowOut;
    out.pos = u.light_view_proj * vec4<f32>(world, 1.0);
    out.corner = corner;
    out.depth_radius = vec2<f32>(center.z, r);
    return out;
}

@fragment
fn fs_shadow(in: ShadowOut) -> @builtin(frag_depth) f32 {
    let q = dot(in.corner, in.corner);
    if q > 1.0 {
        discard;
    }
    return clamp(in.depth_radius.x - sqrt(1.0 - q) * in.depth_radius.y * u.light.x, 0.0, 1.0);
}

// ---- Inside walls of the container, and the sky, ray traced ----

struct ScreenOut {
    @builtin(position) pos: vec4<f32>,
};

@vertex
fn vs_screen(@builtin(vertex_index) i: u32) -> ScreenOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: ScreenOut;
    out.pos = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    return out;
}

struct EnvOut {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

fn wall_shade(p: vec3<f32>, n: vec3<f32>, rd: vec3<f32>, t: f32, emissive: f32) -> vec3<f32> {
    let base = u.glass_color.rgb * u.glass_color.w;
    let l = u.key_dir.xyz;
    let key = u.key_color.rgb * u.key_dir.w * max(dot(n, l), 0.0) * shadow_at(p, n) / PI;
    let lit = base * (key + sky_light(n));
    return lit + u.glass_color.rgb * u.material.w * emissive + hit_glow(p);
}

@fragment
fn fs_env(in: ScreenOut) -> EnvOut {
    let ro = u.camera.xyz;
    let rd = ray_dir(in.pos.xy);
    let kind = u32(u.container.x);
    let size = u.container.y;
    let sky = environment(rd) - sky_light(rd) * 0.6;
    var out: EnvOut;
    out.color = vec4<f32>(sky, 1.0);
    out.depth = 0.0;

    if kind == 0u {
        let t = hit_sphere(ro, rd, size);
        if t.y > 0.0 {
            let p = ro + rd * t.y;
            let n = -p / size;
            let px = pixel_at(t.y);
            let lat = acos(clamp(-p.y / size, -1.0, 1.0)) * size;
            let lon = atan2(p.z, p.x) * size * sin(lat / size);
            let grid = max(lines(lat, size * PI / 12.0, px * 0.6, px), lines(lon, size * PI / 12.0, px * 0.6, px));
            out.color = vec4<f32>(wall_shade(p, n, rd, t.y, grid * 0.12), 1.0);
            out.depth = depth_of(p);
        }
    } else if kind == 1u {
        let angle = u.container.z;
        let lro = rotate_y(ro, -angle);
        let lrd = rotate_y(rd, -angle);
        let t = hit_box(lro, lrd, size);
        if t.y > max(t.x, 0.0) {
            let local = lro + lrd * t.y;
            let a = abs(local) / size;
            var ln = vec3<f32>(0.0);
            if a.x >= a.y && a.x >= a.z {
                ln = vec3<f32>(-sign(local.x), 0.0, 0.0);
            } else if a.y >= a.z {
                ln = vec3<f32>(0.0, -sign(local.y), 0.0);
            } else {
                ln = vec3<f32>(0.0, 0.0, -sign(local.z));
            }
            let p = ro + rd * t.y;
            let n = rotate_y(ln, angle);
            let px = pixel_at(t.y);
            let edge = 1.0 - smoothstep(px * 1.2, px * 2.5, box_edge(local, size));
            // Grid lines across the face, not along the axis the face looks down.
            let across = vec3<f32>(1.0) - abs(ln);
            let gx = lines(local.x, size * 0.5, px * 0.5, px) * across.x;
            let gy = lines(local.y, size * 0.5, px * 0.5, px) * across.y;
            let gz = lines(local.z, size * 0.5, px * 0.5, px) * across.z;
            let grid = max(gx, max(gy, gz));
            out.color = vec4<f32>(wall_shade(p, n, rd, t.y, edge * 3.0 + grid * 0.25), 1.0);
            out.depth = depth_of(p);
        }
    } else {
        let t = hit_tube(ro, rd, size);
        if t.y > 0.0 {
            let p = ro + rd * t.y;
            let n = -vec3<f32>(p.xy / size, 0.0);
            let px = pixel_at(t.y);
            let spacing = u.gates[0].w;
            var glow = 0.0;
            // A bright band around the tube at each unbroken ring.
            let count = u32(u.light.z);
            for (var i = 0u; i < count; i++) {
                let d = abs(p.z - u.gates[i].x);
                glow += (1.0 - smoothstep(px * 1.5, px * 3.0 + 4.0, d)) * 2.5;
            }
            // Faint lines along and around the tube, so its motion shows.
            let around = atan2(p.y, p.x) * size;
            glow += lines(around, TAU * size / 16.0, px * 0.5, px) * 0.15;
            glow += lines(p.z, max(spacing, 1.0) * 0.25, px * 0.5, px) * 0.1;
            var color = wall_shade(p, n, rd, t.y, glow);
            // Darker behind the back wall, and fading into the distance.
            if p.z < u.container.w {
                color *= 0.35;
            }
            let fog = exp(-t.y / (size * 14.0));
            color = mix(sky, color, fog);
            out.color = vec4<f32>(color, 1.0);
            out.depth = depth_of(p);
        }
    }
    return out;
}

// ---- Balls: camera-facing quads ray traced as spheres ----

struct LookBallIn {
    @location(0) center_radius: vec4<f32>,
    @location(1) color: vec4<f32>,
    @location(2) color2: vec4<f32>,
    @location(3) outline_color: vec4<f32>,
    // Shape, fill (-1 classic), pattern size, roundness.
    @location(4) form: vec4<f32>,
    // Outline width in pixels, unused, opacity, turn around the up axis.
    @location(5) look: vec4<f32>,
    // Group plus one (0 for none), random number, for effects on things.
    @location(6) tag: vec4<f32>,
};

struct BallOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) @interpolate(flat) center_radius: vec4<f32>,
    @location(2) @interpolate(flat) color: vec4<f32>,
    @location(3) @interpolate(flat) color2: vec4<f32>,
    @location(4) @interpolate(flat) outline_color: vec4<f32>,
    @location(5) @interpolate(flat) form: vec4<f32>,
    @location(6) @interpolate(flat) look: vec4<f32>,
    @location(7) @interpolate(flat) tag: vec4<f32>,
};

@vertex
fn vs_ball(@builtin(vertex_index) i: u32, b: LookBallIn) -> BallOut {
    let corner = quad_corner(i);
    let c = b.center_radius.xyz;
    let r = b.center_radius.w;
    let to = c - u.camera.xyz;
    let d = length(to);
    var out: BallOut;
    out.center_radius = b.center_radius;
    out.color = b.color;
    out.color2 = b.color2;
    out.outline_color = b.outline_color;
    out.form = b.form;
    out.look = b.look;
    out.tag = b.tag;
    if d <= r * 1.02 + 1.0 {
        // The camera is inside this ball: skip it.
        out.pos = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        out.world = c;
        return out;
    }
    let forward = to / d;
    var side = cross(forward, vec3<f32>(0.0, -1.0, 0.0));
    if dot(side, side) < 1e-6 {
        side = vec3<f32>(1.0, 0.0, 0.0);
    }
    let right = normalize(side);
    let up = cross(right, forward);
    // Half size of the sphere's outline on a plane through its centre, plus room to smooth the edge.
    var s = r / sqrt(1.0 - (r / d) * (r / d)) + pixel_at(d) * 2.0;
    if b.form.y >= 0.0 && b.form.x > 0.5 {
        // Stars and hearts poke a little past the radius.
        s = r * 1.3 + pixel_at(d) * 2.0;
    }
    let world = c + (right * corner.x + up * corner.y) * s;
    out.pos = u.view_proj * vec4<f32>(world, 1.0);
    out.world = world;
    return out;
}

struct BallFrag {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

@fragment
fn fs_ball(in: BallOut) -> BallFrag {
    if in.form.y >= 0.0 {
        if in.form.x > 0.5 {
            return sticker_ball(in);
        }
        return styled_ball(in);
    }
    let ro = u.camera.xyz;
    let rd = normalize(in.world - ro);
    let c = in.center_radius.xyz;
    let r = in.center_radius.w;
    let oc = ro - c;
    let b = dot(oc, rd);
    let miss = sqrt(max(dot(oc, oc) - b * b, 0.0));
    let px = pixel_at(max(-b, 1.0));
    let coverage = clamp((r - miss) / px + 0.5, 0.0, 1.0);
    if coverage <= 0.0 {
        discard;
    }
    var p: vec3<f32>;
    let disc = b * b - (dot(oc, oc) - r * r);
    if disc >= 0.0 {
        p = ro + rd * (-b - sqrt(disc));
    } else {
        // Just outside the outline: shade the edge point nearest the ray.
        let closest = ro + rd * (-b);
        p = c + normalize(closest - c) * r;
    }
    let n = normalize(p - c);
    let albedo = in.color.rgb;
    var color = shade(albedo, n, -rd, shadow_at(p, n));
    color += albedo * u.ambient_bottom.w * 2.0 + hit_glow(p) * 0.15;
    var out: BallFrag;
    out.depth = depth_of(p);
    if in.color.a < 0.0 {
        // Additive: the lit ball's light is added at the colour's strength.
        out.color = vec4<f32>(color * coverage * -in.color.a, 0.0);
        return out;
    }
    out.color = vec4<f32>(color * coverage, coverage);
    return out;
}

// The albedo of a styled ball's fill at a point of its surface. `m` is the point's direction
// from the centre, turned with the ball; `facing` how squarely the surface faces the camera.
fn look_albedo(in: BallOut, m: vec3<f32>, facing: f32) -> vec3<f32> {
    let fill = in.form.y;
    if fill < 1.5 {
        return in.color.rgb;
    }
    var t: f32;
    if fill < 2.5 {
        // Radial: the ball's colour where it faces the camera, the second colour at its rim.
        t = smoothstep(0.0, 1.0, sqrt(max(1.0 - facing * facing, 0.0)));
    } else if fill > 10.5 {
        // The face is on the front, and turns away as the ball spins.
        t = pattern(m.xy, fill, in.form.z) * step(m.z, 0.0);
    } else {
        t = pattern(m.xy, fill, in.form.z);
    }
    return mix(in.color.rgb, in.color2.rgb, t);
}

// Lights a styled ball's surface point; shiny balls get a polished finish.
fn look_shade(in: BallOut, albedo: vec3<f32>, p: vec3<f32>, n: vec3<f32>, v: vec3<f32>) -> vec3<f32> {
    let shadow = shadow_at(p, n);
    var color: vec3<f32>;
    if in.form.y > 0.5 && in.form.y < 1.5 {
        color = shade_with(albedo, n, v, shadow, 0.12, u.material.y);
        let h = normalize(u.key_dir.xyz + v);
        color += u.key_color.rgb * pow(max(dot(n, h), 0.0), 60.0) * 0.6 * shadow;
    } else {
        color = shade(albedo, n, v, shadow);
    }
    return color + albedo * u.ambient_bottom.w * 2.0 + hit_glow(p) * 0.15;
}

// The finished colour of a styled ball's pixel: outline, opacity and additive colours.
fn look_finish(in: BallOut, lit: vec3<f32>, cover: f32, edge_px: f32) -> vec4<f32> {
    var color = lit;
    let outline = in.look.x;
    if outline > 0.0 {
        color = mix(in.outline_color.rgb, color, clamp(edge_px - outline + 0.5, 0.0, 1.0));
    }
    // An additive fill colour makes the whole ball add its light.
    let opacity = in.look.z * select(1.0, -in.color.a, in.color.a < 0.0);
    let a = cover * opacity;
    return vec4<f32>(color * a, select(a, 0.0, in.color.a < 0.0));
}

// A round styled ball: a sphere with its pattern wrapped around it.
fn styled_ball(in: BallOut) -> BallFrag {
    let ro = u.camera.xyz;
    let rd = normalize(in.world - ro);
    let c = in.center_radius.xyz;
    let r = in.center_radius.w;
    let oc = ro - c;
    let b = dot(oc, rd);
    let miss = sqrt(max(dot(oc, oc) - b * b, 0.0));
    let px = pixel_at(max(-b, 1.0));
    let coverage = clamp((r - miss) / px + 0.5, 0.0, 1.0);
    if coverage <= 0.0 || in.look.z <= 0.0 {
        discard;
    }
    var p: vec3<f32>;
    let disc = b * b - (dot(oc, oc) - r * r);
    if disc >= 0.0 {
        p = ro + rd * (-b - sqrt(disc));
    } else {
        let closest = ro + rd * (-b);
        p = c + normalize(closest - c) * r;
    }
    let n = normalize(p - c);
    let m = rotate_y(n, -in.look.w);
    let albedo = look_albedo(in, m, dot(n, -rd));
    let lit = look_shade(in, albedo, p, n, -rd);
    var out: BallFrag;
    out.color = look_finish(in, lit, coverage, (r - miss) / px);
    out.depth = depth_of(p);
    return out;
}

// A styled ball of another shape: a puffy sticker facing the camera, turned by the look's turn.
fn sticker_ball(in: BallOut) -> BallFrag {
    let ro = u.camera.xyz;
    let rd = normalize(in.world - ro);
    let c = in.center_radius.xyz;
    let r = in.center_radius.w;
    let forward = normalize(c - ro);
    var side = cross(forward, vec3<f32>(0.0, -1.0, 0.0));
    if dot(side, side) < 1e-6 {
        side = vec3<f32>(1.0, 0.0, 0.0);
    }
    let right = normalize(side);
    let up = cross(right, forward);
    // Where the ray meets the plane through the centre, facing the camera.
    let t = dot(c - ro, forward) / max(dot(rd, forward), 1e-4);
    let hit = ro + rd * t;
    let local = vec2<f32>(dot(hit - c, right), -dot(hit - c, up)) / max(r, 1e-3);
    let q = rotate2(local, -in.look.w);
    let d = ball_shape(q, in.form.x, in.form.w);
    let px = pixel_at(max(t, 1.0)) / max(r, 1e-3);
    let cover = clamp(-d / px + 0.5, 0.0, 1.0);
    if cover <= 0.0 || in.look.z <= 0.0 {
        discard;
    }
    // Rounded towards the camera in the middle, flatter near the edge.
    let bulge = sqrt(clamp(-d, 0.0, 1.0));
    let n = normalize(right * local.x * 0.6 - up * local.y * 0.6 - forward * (0.4 + bulge));
    let p = hit - forward * bulge * r * 0.5;
    var albedo = in.color.rgb;
    if in.form.y > 1.5 {
        albedo = mix(in.color.rgb, in.color2.rgb, pattern(q, in.form.y, in.form.z));
    }
    let lit = look_shade(in, albedo, p, n, -rd);
    var out: BallFrag;
    out.color = look_finish(in, lit, cover, -d / px);
    out.depth = depth_of(p);
    return out;
}

fn rotate2(p: vec2<f32>, angle: f32) -> vec2<f32> {
    let c = cos(angle);
    let s = sin(angle);
    return vec2<f32>(c * p.x - s * p.y, s * p.x + c * p.y);
}

// ---- Trails: ribbons facing the camera ----

struct RibbonIn {
    @location(0) position: vec3<f32>,
    @location(1) across: f32,
    @location(2) color: vec4<f32>,
};

struct RibbonOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) across: f32,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_ribbon(v: RibbonIn) -> RibbonOut {
    var out: RibbonOut;
    out.pos = u.view_proj * vec4<f32>(v.position, 1.0);
    out.across = v.across;
    out.color = v.color;
    return out;
}

@fragment
fn fs_ribbon(in: RibbonOut) -> @location(0) vec4<f32> {
    let soft = 1.0 - in.across * in.across;
    return vec4<f32>(in.color.rgb * soft, 0.0);
}

// ---- Glows: sparks at hits and pieces of broken rings ----

struct GlowOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) corner: vec2<f32>,
    @location(1) @interpolate(flat) color: vec4<f32>,
};

@vertex
fn vs_glow(@builtin(vertex_index) i: u32, b: BallIn) -> GlowOut {
    let corner = quad_corner(i);
    let size = b.center_radius.w;
    let to = b.center_radius.xyz - u.camera.xyz;
    let d = max(length(to), 1e-3);
    let forward = to / d;
    var side = cross(forward, vec3<f32>(0.0, -1.0, 0.0));
    if dot(side, side) < 1e-6 {
        side = vec3<f32>(1.0, 0.0, 0.0);
    }
    let right = normalize(side);
    let up = cross(right, forward);
    // Pulled towards the camera so a glow on a wall isn't cut in half by it.
    let center = b.center_radius.xyz - forward * min(size, d * 0.5);
    let world = center + (right * corner.x + up * corner.y) * size;
    var out: GlowOut;
    out.pos = u.view_proj * vec4<f32>(world, 1.0);
    out.corner = corner;
    out.color = b.color;
    return out;
}

@fragment
fn fs_glow(in: GlowOut) -> @location(0) vec4<f32> {
    let q = dot(in.corner, in.corner);
    let core = exp(-q * 9.0) + exp(-q * 3.0) * 0.25;
    return vec4<f32>(in.color.rgb * core * (1.0 - smoothstep(0.8, 1.0, q)), 0.0);
}

// ---- What is where, for effects on things (see fs_ball_object in the 2D scene shader) ----

// Where the scene drew a wall: the frame's position, kind 2, fully covered.
@fragment
fn fs_wall_object(in: ScreenOut) -> @location(0) vec4<f32> {
    let depth = textureLoad(scene_depth, vec2<i32>(in.pos.xy), 0);
    if depth <= 0.0 {
        discard;
    }
    let uv = in.pos.xy / u.screen.xy;
    return vec4<f32>((uv - 0.5) * vec2<f32>(u.screen.x / u.screen.y, 1.0), 2.0, 1.0);
}

struct ObjectFrag {
    @location(0) value: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

// A ball: its own position, -0.5 to 0.5 across it, turning with its pattern.
@fragment
fn fs_ball_object(in: BallOut) -> ObjectFrag {
    let ro = u.camera.xyz;
    let rd = normalize(in.world - ro);
    let c = in.center_radius.xyz;
    let r = in.center_radius.w;
    var local: vec2<f32>;
    var cover: f32;
    var p: vec3<f32>;
    if in.form.y >= 0.0 && in.form.x > 0.5 {
        let forward = normalize(c - ro);
        var side = cross(forward, vec3<f32>(0.0, -1.0, 0.0));
        if dot(side, side) < 1e-6 {
            side = vec3<f32>(1.0, 0.0, 0.0);
        }
        let right = normalize(side);
        let up = cross(right, forward);
        let t = dot(c - ro, forward) / max(dot(rd, forward), 1e-4);
        let hit = ro + rd * t;
        local = rotate2(vec2<f32>(dot(hit - c, right), -dot(hit - c, up)) / max(r, 1e-3), -in.look.w);
        let d = ball_shape(local, in.form.x, in.form.w);
        cover = clamp(-d / (pixel_at(max(t, 1.0)) / max(r, 1e-3)) + 0.5, 0.0, 1.0);
        p = hit - forward * sqrt(clamp(-d, 0.0, 1.0)) * r * 0.5;
    } else {
        let oc = ro - c;
        let b = dot(oc, rd);
        let miss = sqrt(max(dot(oc, oc) - b * b, 0.0));
        cover = clamp((r - miss) / pixel_at(max(-b, 1.0)) + 0.5, 0.0, 1.0);
        let disc = b * b - (dot(oc, oc) - r * r);
        if disc >= 0.0 {
            p = ro + rd * (-b - sqrt(disc));
        } else {
            let closest = ro + rd * (-b);
            p = c + normalize(closest - c) * r;
        }
        let n = normalize(p - c);
        local = rotate_y(n, -in.look.w).xy;
    }
    if cover <= 0.0 || r <= 0.0 || (in.form.y >= 0.0 && in.look.z <= 0.0) {
        discard;
    }
    let kind = select(0.0, 9.0 + in.tag.x, in.tag.x > 0.5);
    var out: ObjectFrag;
    out.value = vec4<f32>(local * 0.5, kind + in.tag.y * 0.5, cover);
    // A hair nearer than the ball drawn in the scene pass, so it passes the depth test.
    out.depth = min(depth_of(p) * 1.0001, 1.0);
    return out;
}

// ---- Halos: the glow of balls whose look has one, outside the ball ----

@vertex
fn vs_halo(@builtin(vertex_index) i: u32, b: BallIn) -> GlowOut {
    let corner = quad_corner(i);
    let size = b.center_radius.w;
    let to = b.center_radius.xyz - u.camera.xyz;
    let d = max(length(to), 1e-3);
    let forward = to / d;
    var side = cross(forward, vec3<f32>(0.0, -1.0, 0.0));
    if dot(side, side) < 1e-6 {
        side = vec3<f32>(1.0, 0.0, 0.0);
    }
    let right = normalize(side);
    let up = cross(right, forward);
    let world = b.center_radius.xyz + (right * corner.x + up * corner.y) * size;
    var out: GlowOut;
    out.pos = u.view_proj * vec4<f32>(world, 1.0);
    out.corner = corner;
    out.color = b.color;
    return out;
}

@fragment
fn fs_halo(in: GlowOut) -> @location(0) vec4<f32> {
    let q = length(in.corner);
    let inner = in.color.a;
    // Distance past the ball's edge, in the halo's reach (2.5 glow sizes, as in 2D).
    let d = max(q - inner, 0.0) / max(1.0 - inner, 1e-3) * 2.5;
    let halo = exp(-d * d) * smoothstep(inner * 0.97, inner, q) * (1.0 - smoothstep(0.9, 1.0, q));
    return vec4<f32>(in.color.rgb * halo, 0.0);
}

// ---- Glass: the container's front surface, its edges and the tunnel's rings ----

// Adds a layer behind what is already gathered: premultiplied colour, and how much it covers.
fn under(acc: vec4<f32>, color: vec3<f32>, alpha: f32) -> vec4<f32> {
    let keep = 1.0 - acc.a;
    return vec4<f32>(acc.rgb + keep * color, acc.a + keep * alpha);
}

fn glass_layer(acc: vec4<f32>, p: vec3<f32>, n: vec3<f32>, rd: vec3<f32>, edge: f32, tint: vec3<f32>) -> vec4<f32> {
    let glass = u.material.z;
    let v = -rd;
    let ndv = clamp(abs(dot(n, v)), 0.0, 1.0);
    let fresnel = 0.04 + 0.96 * pow(1.0 - ndv, 5.0);
    let h = normalize(u.key_dir.xyz + v);
    let sparkle = pow(max(dot(n, h), 0.0), 900.0) * u.key_dir.w * 0.8;
    let reflection = environment(reflect(rd, n)) * fresnel + u.key_color.rgb * sparkle;
    let alpha = clamp(glass * (0.05 + 0.85 * fresnel), 0.0, 1.0);
    let body = (reflection + tint * 0.03) * glass + hit_glow(p) * 0.6 * glass;
    let line = tint * u.material.w * edge * 2.5;
    return under(acc, body + line, alpha);
}

@fragment
fn fs_glass(in: ScreenOut) -> @location(0) vec4<f32> {
    let ro = u.camera.xyz;
    let rd = ray_dir(in.pos.xy);
    let kind = u32(u.container.x);
    let size = u.container.y;
    let tint = u.glass_color.rgb;
    let stored = textureLoad(scene_depth, vec2<i32>(in.pos.xy), 0);
    var limit = 1e30;
    if stored > 0.0 {
        let ndc = vec2<f32>(in.pos.x / u.screen.x * 2.0 - 1.0, 1.0 - in.pos.y / u.screen.y * 2.0);
        let world = u.inv_view_proj * vec4<f32>(ndc, stored, 1.0);
        limit = distance(world.xyz / world.w, ro) * 1.0005;
    }
    var acc = vec4<f32>(0.0);

    if kind == 0u {
        let t = hit_sphere(ro, rd, size);
        let miss = sqrt(max(dot(ro, ro) - dot(ro, rd) * dot(ro, rd), 0.0));
        if t.x > 0.0 && t.x < limit {
            let p = ro + rd * t.x;
            let px = pixel_at(t.x);
            // A thin bright outline where the glass is seen edge-on.
            let edge = 1.0 - smoothstep(px * 1.0, px * 2.5, size - miss);
            acc = glass_layer(acc, p, p / size, rd, edge, tint);
        } else if t.x <= 0.0 && t.y > 0.0 && t.y < limit {
            // From inside the sphere only the far side shows, already drawn as the wall.
        }
    } else if kind == 1u {
        let angle = u.container.z;
        let lro = rotate_y(ro, -angle);
        let lrd = rotate_y(rd, -angle);
        let t = hit_box(lro, lrd, size);
        if t.x > 0.0 && t.y > t.x && t.x < limit {
            let local = lro + lrd * t.x;
            let a = abs(local);
            var ln = vec3<f32>(0.0, 0.0, sign(local.z));
            if a.x >= a.y && a.x >= a.z {
                ln = vec3<f32>(sign(local.x), 0.0, 0.0);
            } else if a.y >= a.z {
                ln = vec3<f32>(0.0, sign(local.y), 0.0);
            }
            let px = pixel_at(t.x);
            let edge = (1.0 - smoothstep(px * 1.2, px * 2.5, box_edge(local, size))) * 1.5;
            acc = glass_layer(acc, ro + rd * t.x, rotate_y(ln, angle), rd, edge, tint);
        }
    } else {
        // The back wall: a faint pane with a glowing rim.
        let back = u.container.w;
        if ro.z < back && rd.z > 1e-5 {
            let t = (back - ro.z) / rd.z;
            let p = ro + rd * t;
            let r = length(p.xy);
            if t < limit && r < size {
                let px = pixel_at(t);
                let rim = 1.0 - smoothstep(px * 1.5, px * 4.0, size - r);
                acc = under(acc, tint * u.material.w * rim * 2.0 + tint * 0.02, 0.05 * u.material.z);
            }
        }
        // The rings, nearest first: tinted glass discs with glowing rims and gap edges.
        let count = u32(u.light.z);
        for (var i = 0u; i < count; i++) {
            let gate = u.gates[i];
            if abs(rd.z) < 1e-5 {
                break;
            }
            let t = (gate.x - ro.z) / rd.z;
            if t <= 0.0 {
                continue;
            }
            if t >= limit {
                break;
            }
            let p = ro + rd * t;
            let r = length(p.xy);
            if r >= size {
                continue;
            }
            let px = pixel_at(t);
            let fog = exp(-t / (size * 14.0));
            let color = u.gate_colors[i].rgb * fog;
            let offset = abs((atan2(p.y, p.x) - gate.y + PI) - floor((atan2(p.y, p.x) - gate.y + PI) / TAU) * TAU - PI);
            let to_gap_edge = abs(offset - gate.z) * r;
            let gap_edge = (1.0 - smoothstep(px * 1.5, px * 3.5, to_gap_edge)) * step(px * 2.0, r);
            let rim = 1.0 - smoothstep(px * 2.0, px * 5.0, size - r);
            let glow = color * u.material.w * (rim * 2.5 + gap_edge * 3.0);
            if offset <= gate.z {
                acc = under(acc, glow, 0.0);
            } else {
                let fresnel = 0.04 + 0.96 * pow(1.0 - abs(rd.z), 5.0);
                let swirl = lines(r, size * 0.125, px * 0.5, px) * 0.15;
                let body = color * (0.12 + swirl) * u.material.z + environment(reflect(rd, vec3<f32>(0.0, 0.0, -1.0))) * fresnel * 0.5 + hit_glow(p) * 0.4;
                acc = under(acc, body + glow, clamp(u.material.z * 0.55 + fresnel * 0.3, 0.0, 0.95) * fog);
            }
        }
    }
    return acc;
}

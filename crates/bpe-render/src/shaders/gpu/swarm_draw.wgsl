// Matches `SwarmParams` in swarm.rs.
struct Params {
    // x: points, y: grid cells across, z: grid cells down, w: seed
    counts: vec4<u32>,
    // x: top speed, y: drag, z: flow strength, w: swirl size
    motion: vec4<f32>,
    // x: swirl change speed, y: flock, z: separation, w: attraction to balls
    steer: vec4<f32>,
    // x: ball reach, y: wall avoidance, z: gravity, w: grid cell size
    more: vec4<f32>,
    // x: colour mode (0 speed, 1 nearest ball, 2 gradient), y: colour stops, z: start (0 fill, 1 centre, 2 ring)
    look: vec4<u32>,
    // xy: arena centre, z: arena size, w: point size (drawing)
    arena: vec4<f32>,
    // x: brightness (drawing)
    draw: vec4<f32>,
    colors: array<vec4<f32>, 6>,
};

// Draws the swarm as tiny glowing dots, added on top of each other.


struct Globals {
    view: vec4<f32>,
    bg_top: vec4<f32>,
    bg_bottom: vec4<f32>,
    look: vec4<f32>,
    look2: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var<uniform> draw_params: Params;
@group(1) @binding(1) var<storage, read> draw_points: array<vec4<f32>>;
@group(1) @binding(2) var<storage, read> draw_tints: array<vec2<u32>>;

// Matches hash2 in common.wgsl and `rand` in swarm.wgsl.
fn draw_rand(i: u32, k: u32) -> f32 {
    var h = ((draw_params.counts.w ^ (i * 0x632be5abu)) * 0x9e3779b9u) ^ (k * 0x85ebca6bu);
    h = h ^ (h >> 16u);
    h = h * 0x7feb352du;
    h = h ^ (h >> 15u);
    h = h * 0x846ca68bu;
    h = h ^ (h >> 16u);
    return f32(h >> 8u) / 16777216.0;
}

fn draw_gradient(t: f32) -> vec4<f32> {
    let stops = max(draw_params.look.y, 1u);
    let x = clamp(t, 0.0, 1.0) * f32(stops - 1u);
    let a = u32(floor(x));
    let b = min(a + 1u, stops - 1u);
    return mix(draw_params.colors[a], draw_params.colors[b], fract(x));
}

struct PointOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) color: vec3<f32>,
};

@vertex
fn vs_point(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> PointOut {
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u)) * 2.0 - 1.0;
    let point = draw_points[ii];
    let p = point.xy;
    var color: vec4<f32>;
    switch draw_params.look.x {
        case 0u: {
            color = draw_gradient(length(point.zw) / max(draw_params.motion.x, 1.0));
        }
        case 1u: {
            let tint = draw_tints[ii];
            color = vec4<f32>(unpack2x16float(tint.x), unpack2x16float(tint.y));
        }
        default: {
            color = draw_gradient(draw_rand(ii, 42u));
        }
    }
    let px = g.view.z;
    let size = draw_params.arena.w;
    // Points smaller than a pixel get dimmer instead of vanishing between pixels.
    let half = max(size * 0.5, px * 0.75);
    let cover = min(size * size / (px * px), 1.0);
    var out: PointOut;
    let world = p + corner * half;
    out.pos = vec4<f32>(world.x * g.view.x, -world.y * g.view.y, 0.0, 1.0);
    out.local = corner;
    out.color = color.rgb * draw_params.draw.x * cover;
    return out;
}

@fragment
fn fs_point(in: PointOut) -> @location(0) vec4<f32> {
    let r2 = dot(in.local, in.local);
    let a = max(1.0 - r2, 0.0);
    if a <= 0.0 {
        discard;
    }
    return vec4<f32>(in.color * a * 1.6, 0.0);
}

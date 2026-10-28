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

struct PointOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) color: vec3<f32>,
};

@vertex
fn vs_point(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> PointOut {
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u)) * 2.0 - 1.0;
    let p = draw_points[ii].xy;
    let tint = draw_tints[ii];
    let color = vec4<f32>(unpack2x16float(tint.x), unpack2x16float(tint.y));
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

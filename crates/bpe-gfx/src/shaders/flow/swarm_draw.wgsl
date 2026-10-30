// Draws the swarm as tiny glowing dots, added on top of each other.

// Matches `SwarmParams` in flow/swarm.rs.
struct Params {
    // x: points, y: grid cells across, z: grid cells down, w: seed
    counts: vec4<u32>,
    // x: top speed, y: drag, z: flow strength, w: swirl size
    motion: vec4<f32>,
    // x: swirl change speed, y: flock, z: separation, w: attraction to balls
    steer: vec4<f32>,
    // x: ball reach, y: wall avoidance, z: gravity scale, w: grid cell size
    more: vec4<f32>,
    // x: colour follow (0 speed, 1 nearest ball, 2 random, 3 place along a direction, 4 distance
    // from a centre, 5 one colour), z: start (0 fill, 1 centre, 2 ring)
    look: vec4<u32>,
    // xy: swarm centre, z: half size of the area a Fill start covers, w: point size (drawing)
    arena: vec4<f32>,
    // x: brightness (drawing), y: seconds the release from the centre takes (0: all at once)
    draw: vec4<f32>,
    // The one colour, and a point's colour before it has a nearest ball.
    color: vec4<f32>,
    // The paint, as `PaintGpu`'s fields in order.
    paint_a: vec4<f32>,
    paint_b: vec4<f32>,
    // x: direction in degrees, y: span (also of a speed), zw: centre
    place: vec4<f32>,
};

// Matches `Globals` in lib.rs.
struct Globals {
    // x, y: 2 / frame size, z: world size of one output pixel, w: time
    view: vec4<f32>,
    // The background paint: atlas row, start, scroll, repeat.
    bg_paint: vec4<f32>,
    // The background place: direction, span, centre xy.
    bg_place: vec4<f32>,
    post: vec4<f32>,
    // centre xy, zoom, turn
    camera: vec4<f32>,
    // x: background on screen, y: how its place runs, z: steps
    bg: vec4<f32>,
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

// The colour of the paint `along` it (0 to 1 is one gradient), after its start and repeat.
fn draw_gradient(along: f32) -> vec4<f32> {
    return paint_along(paint_from(draw_params.paint_a, draw_params.paint_b), along, 0.0);
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
    let place = draw_params.place;
    switch draw_params.look.x {
        case 0u: {
            color = draw_gradient(length(point.zw) / max(place.y, 1e-3));
        }
        case 1u: {
            let tint = draw_tints[ii];
            color = vec4<f32>(unpack2x16float(tint.x), unpack2x16float(tint.y));
        }
        case 2u: {
            color = draw_gradient(draw_rand(ii, 42u));
        }
        case 3u: {
            let a = radians(place.x);
            color = draw_gradient(dot(p - place.zw, vec2<f32>(cos(a), sin(a))) / max(place.y, 1e-3) + 0.5);
        }
        case 4u: {
            color = draw_gradient(length(p - place.zw) / max(place.y, 1e-3));
        }
        default: {
            color = draw_params.color;
        }
    }
    let px = g.view.z;
    let size = draw_params.arena.w;
    // Points smaller than a pixel get dimmer instead of vanishing between pixels.
    var half = max(size * 0.5, px * 0.75);
    let cover = min(size * size / (px * px), 1.0);
    // A point still waiting at the centre is not drawn.
    let release = draw_params.draw.y * f32(ii) / f32(max(draw_params.counts.x, 1u));
    if g.view.w < release {
        half = 0.0;
    }
    var out: PointOut;
    let world = p + corner * half;
    var q = world - g.camera.xy;
    if g.camera.w != 0.0 {
        let sn = sin(-g.camera.w);
        let k = cos(-g.camera.w);
        q = vec2<f32>(q.x * k - q.y * sn, q.x * sn + q.y * k);
    }
    q = q * g.camera.z;
    out.pos = vec4<f32>(q.x * g.view.x, -q.y * g.view.y, 0.0, 1.0);
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

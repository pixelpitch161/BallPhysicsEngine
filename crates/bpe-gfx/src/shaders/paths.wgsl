
// ---- Shapes: closed outlines, a circle or a list of points, filled or as a band ----
// Appended to scene.wgsl's module. The points live in a storage buffer beside the layer records
// (group 2, binding 4). A shape with layers walks its records (see shape_layers.wgsl); one with
// none is drawn plain in its colour.

struct PathIn {
    // Centre xy, angle, scale (world pixels per shape unit).
    @location(0) a: vec4<f32>,
    // Half thickness (0 = filled), kind (0 circle, 1 points), first point, point count.
    @location(1) b: vec4<f32>,
    // Thing code, reach from the centre (shape units), how far its layers reach past the outline
    // (world pixels), the reach again for layers running from the middle.
    @location(2) c: vec4<f32>,
    @location(3) color: vec4<f32>,
    // First record, how many (0: plain), program (CPU only), spare.
    @location(4) layers: vec4<u32>,
    // Seconds since a ball last hit it, that hit's strength, how many hits, spare.
    @location(5) hit: vec4<f32>,
};

struct PathOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec2<f32>,
    @location(1) @interpolate(flat) a: vec4<f32>,
    @location(2) @interpolate(flat) b: vec4<f32>,
    @location(3) @interpolate(flat) c: vec4<f32>,
    @location(4) @interpolate(flat) color: vec4<f32>,
    @location(5) @interpolate(flat) layers: vec4<u32>,
    @location(6) @interpolate(flat) hit: vec4<f32>,
};

@group(2) @binding(4) var<storage, read> path_points: array<vec4<f32>>;

@vertex
fn vs_path(@builtin(vertex_index) i: u32, s: PathIn) -> PathOut {
    let corner = quad_corner(i);
    let reach = path_quad_reach(s.a, s.b, s.c);
    let turned = rotate(corner * reach, s.a.z);
    let world = s.a.xy + turned;
    var out: PathOut;
    out.pos = to_clip(world);
    out.world = world;
    out.a = s.a;
    out.b = s.b;
    out.c = s.c;
    out.color = s.color;
    out.layers = s.layers;
    out.hit = s.hit;
    return out;
}

// Half the side of a shape's quad, in world pixels: its outline's reach, its band, what its
// layers reach past it, and two pixels for the soft edge.
fn path_quad_reach(a: vec4<f32>, b: vec4<f32>, c: vec4<f32>) -> f32 {
    let px = g.view.z;
    return c.y * a.w + max(b.x, 0.5 * px) + max(c.z, 0.0) + 2.0 * px;
}

// The point in the shape's own units.
fn path_local(in: PathOut) -> vec2<f32> {
    return rotate(in.world - in.a.xy, -in.a.z) / max(in.a.w, 1e-6);
}

// Distance from the outline in world pixels: negative inside (even-odd, so a self-crossing
// outline still has an inside).
fn path_signed(in: PathOut) -> f32 {
    let q = path_local(in);
    if in.b.y < 0.5 {
        return (length(q) - 1.0) * in.a.w;
    }
    let first = u32(in.b.z);
    let n = u32(in.b.w);
    if n < 2u {
        return 1e30;
    }
    var best = 1e30;
    var inside = false;
    var prev = path_points[first + n - 1u].xy;
    for (var k = 0u; k < n; k++) {
        let p = path_points[first + k].xy;
        best = min(best, segment_distance(q, prev, p));
        if (p.y > q.y) != (prev.y > q.y) {
            let x = p.x + (q.y - p.y) / (prev.y - p.y) * (prev.x - p.x);
            if q.x < x {
                inside = !inside;
            }
        }
        prev = p;
    }
    let d = best * in.a.w;
    return select(d, -d, inside);
}

// How much of the pixel the shape covers: a filled shape up to its outline, a band within half
// its thickness of it (bands thinner than a pixel get dimmer instead of aliasing).
fn path_cover(in: PathOut) -> f32 {
    let px = g.view.z;
    let sd = path_signed(in);
    if in.b.x <= 0.0 {
        return clamp(0.5 - sd / px, 0.0, 1.0);
    }
    let half_width = max(in.b.x, 0.5 * px);
    return clamp((half_width - abs(sd)) / px + 0.5, 0.0, 1.0) * min(in.b.x * 2.0 / px, 1.0);
}

fn shade_path(in: PathOut) -> vec4<f32> {
    if in.layers.y > 0u {
        return shade_shape_layers(in.world, in.a, in.b, in.c, in.layers, in.hit);
    }
    let cover = path_cover(in) * abs(in.color.a);
    if cover <= 0.0 {
        discard;
    }
    return vec4<f32>(in.color.rgb * cover, covering(cover, in.color.a));
}

@fragment
fn fs_path(in: PathOut) -> @location(0) vec4<f32> {
    return shade_path(in);
}

// In a layer whose things blend with each other (see each.wgsl).
@fragment
fn fs_path_each(in: PathOut) -> @location(0) vec4<f32> {
    return each_out(shade_path(in), g.post.w);
}

// The object picture: the shape's own position, -1 to 1 across its reach, turning with it.
@fragment
fn fs_path_object(in: PathOut) -> @location(0) vec4<f32> {
    let cover = path_cover(in);
    if cover <= 0.0 || in.c.x < 0.5 {
        discard;
    }
    let local = path_local(in) / max(in.c.y, 1e-3);
    return vec4<f32>(local, in.c.x, cover);
}

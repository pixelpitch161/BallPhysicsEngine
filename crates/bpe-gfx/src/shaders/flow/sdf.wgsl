// Bakes the wall distance field in the wall's own frame (centre at the origin, unturned): one
// workgroup per 8x8 tile of texels. Open space inside the wall is positive, the wall negative.

struct Bake {
    // xy: position of texel (0, 0)'s corner, z: size of one texel, w: largest distance
    origin: vec4<f32>,
    // x, y: size in texels
    size: vec4<u32>,
    // The closed wall. x: kind (0 none, 1 round, 2 polygon, 3 box), y: half wall thickness, zw: centre
    closed_a: vec4<f32>,
    // Round/polygon: radius, sides, angle. Box: half width, half height, angle.
    closed_b: vec4<f32>,
};

@group(0) @binding(0) var<uniform> bake: Bake;
@group(0) @binding(1) var field: texture_storage_2d<r32float, write>;

const PI_: f32 = 3.14159265;
const TAU_: f32 = 6.28318531;

// Distance from p to the closed wall's inner face, positive inside the arena.
fn closed_distance(p: vec2<f32>) -> f32 {
    let kind = bake.closed_a.x;
    let o = p - bake.closed_a.zw;
    let half = bake.closed_a.y;
    if kind < 1.5 {
        return bake.closed_b.x - length(o) - half;
    } else if kind < 2.5 {
        let sides = bake.closed_b.y;
        let step = TAU_ / sides;
        let apothem = bake.closed_b.x * cos(PI_ / sides);
        let u = atan2(o.y, o.x) - bake.closed_b.z;
        let k = round(u / step);
        var best = -1e9;
        // The nearest side is in p's own sector, but its neighbours matter near corners.
        for (var j = -1; j <= 1; j++) {
            let a = bake.closed_b.z + (k + f32(j)) * step;
            best = max(best, dot(o, vec2<f32>(cos(a), sin(a))));
        }
        return apothem - best - half;
    }
    let a = bake.closed_b.z;
    let x = vec2<f32>(cos(a), sin(a));
    let y = vec2<f32>(-x.y, x.x);
    let q = abs(vec2<f32>(dot(o, x), dot(o, y))) - bake.closed_b.xy;
    let outside = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0);
    return -outside - half;
}

@compute @workgroup_size(8, 8)
fn bake_field(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= bake.size.x || id.y >= bake.size.y {
        return;
    }
    let p = bake.origin.xy + (vec2<f32>(id.xy) + vec2<f32>(0.5)) * bake.origin.z;
    var d = bake.origin.w;
    if bake.closed_a.x > 0.5 {
        d = min(d, closed_distance(p));
    }
    textureStore(field, vec2<i32>(id.xy), vec4<f32>(clamp(d, -bake.origin.w, bake.origin.w), 0.0, 0.0, 0.0));
}

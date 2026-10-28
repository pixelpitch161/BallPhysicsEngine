// Bakes the wall distance field: one workgroup per 8x8 tile of texels, each looking only at
// the wall pieces the CPU found near that tile.

struct Bake {
    // xy: world position of texel (0, 0)'s corner, z: world size of one texel, w: largest distance
    origin: vec4<f32>,
    // x, y: size in texels, z: tiles across
    size: vec4<u32>,
    // The closed arena. x: kind (0 none, 1 round, 2 polygon, 3 box), y: half wall thickness, zw: centre
    closed_a: vec4<f32>,
    // Round/polygon: radius, sides, angle. Box: half width, half height, angle.
    closed_b: vec4<f32>,
};

// One wall piece. Matches `PieceGpu` in gpu/sdf.rs.
struct Piece {
    // Segment: both ends. Arc and disc: centre, radius.
    a: vec4<f32>,
    // x: kind (0 segment, 1 arc, 2 disc), y: half thickness, z: arc start, w: arc sweep
    b: vec4<f32>,
};

@group(0) @binding(0) var<uniform> bake: Bake;
@group(0) @binding(1) var<storage, read> pieces: array<Piece>;
// Tile t's pieces are tile_pieces[tile_start[t] .. tile_start[t + 1]].
@group(0) @binding(2) var<storage, read> tile_start: array<u32>;
@group(0) @binding(3) var<storage, read> tile_pieces: array<u32>;
@group(0) @binding(4) var field: texture_storage_2d<r32float, write>;

const PI_: f32 = 3.14159265;
const TAU_: f32 = 6.28318531;

fn segment_distance(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let ab = b - a;
    let t = clamp(dot(p - a, ab) / max(dot(ab, ab), 1e-8), 0.0, 1.0);
    return length(p - a - ab * t);
}

fn arc_distance(p: vec2<f32>, c: vec2<f32>, r: f32, start: f32, sweep: f32) -> f32 {
    let o = p - c;
    let along = atan2(o.y, o.x) - start;
    let wrapped = along - TAU_ * floor(along / TAU_);
    if wrapped <= sweep {
        return abs(length(o) - r);
    }
    let e0 = c + vec2<f32>(cos(start), sin(start)) * r;
    let e1 = c + vec2<f32>(cos(start + sweep), sin(start + sweep)) * r;
    return min(length(p - e0), length(p - e1));
}

fn piece_distance(p: vec2<f32>, piece: Piece) -> f32 {
    let kind = piece.b.x;
    if kind < 0.5 {
        return segment_distance(p, piece.a.xy, piece.a.zw) - piece.b.y;
    } else if kind < 1.5 {
        return arc_distance(p, piece.a.xy, piece.a.z, piece.b.z, piece.b.w) - piece.b.y;
    }
    return length(p - piece.a.xy) - piece.a.z;
}

// Distance from p to the closed arena's wall, positive inside the arena.
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
fn bake_field(@builtin(workgroup_id) wg: vec3<u32>, @builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= bake.size.x || id.y >= bake.size.y {
        return;
    }
    let p = bake.origin.xy + (vec2<f32>(id.xy) + vec2<f32>(0.5)) * bake.origin.z;
    var d = bake.origin.w;
    let tile = wg.y * bake.size.z + wg.x;
    for (var i = tile_start[tile]; i < tile_start[tile + 1u]; i++) {
        d = min(d, piece_distance(p, pieces[tile_pieces[i]]));
    }
    if bake.closed_a.x > 0.5 {
        d = min(d, closed_distance(p));
    }
    textureStore(field, vec2<i32>(id.xy), vec4<f32>(clamp(d, -bake.origin.w, bake.origin.w), 0.0, 0.0, 0.0));
}

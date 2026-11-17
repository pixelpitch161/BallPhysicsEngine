// Liquid part 3: the wet grid (Wet walls). Once a liquid step, after its last substep: a grid
// node in the walls band with liquid on it is wet (1); any other node keeps `keep` of what it had
// and runs `drip` of the way towards most of the node above it (world y is down), so wet runs
// slowly down the walls in streaks and dries. Two halves, read one and write the other, so no
// node reads what another writes in the same pass. See liquid_sim/wet.rs.
//
// Each band node also keeps which side of its wall the liquid is on (`side`: +1 along the wall's
// normal, −1 against it, 0 not known yet) and how far it is from the wall's face on that side
// (`face`, grid units: below 0 inside the wall or past it). Only a node on the liquid's side gets
// wet, so a thin wall the drops' weights reach across stays dry on its far side, and the draw
// shows the film only near the face (a smooth line along the wall, not the grid's steps).

struct Wet {
    // cols, rows, the half written (0 or 1), 0
    grid: vec4<u32>,
    // keep, drip, mass that wets a node (fixed point), 0
    rates: vec4<f32>,
};

@group(0) @binding(0) var<uniform> w: Wet;
// per node: mass, momentum x, momentum y, volume (fixed point; the middle two are speeds by now)
@group(0) @binding(1) var<storage, read> grid: array<i32>;
// per node: (normal xy, distance, in the band), (wall speed xy, half thickness, 0)
@group(0) @binding(2) var<storage, read> solid: array<vec4<f32>>;
// two halves of wetness, then each node's face distance, then each node's side
@group(0) @binding(3) var<storage, read_write> wet: array<f32>;

// How far an off-band node counts as from a wall's face (past where the film shows).
const FAR: f32 = 4.0;

fn mass_at(x: i32, y: i32) -> f32 {
    if x < 0 || y < 0 || x >= i32(w.grid.x) || y >= i32(w.grid.y) {
        return 0.0;
    }
    return f32(grid[4u * (u32(y) * w.grid.x + u32(x))]);
}

@compute @workgroup_size(256)
fn cs_wet(@builtin(global_invocation_id) id: vec3<u32>) {
    let cells = w.grid.x * w.grid.y;
    let n = id.x;
    if n >= cells {
        return;
    }
    let to = w.grid.z * cells;
    let old = (1u - w.grid.z) * cells;
    let s = solid[2u * n];
    if s.w < 0.5 {
        wet[to + n] = 0.0;
        wet[2u * cells + n] = FAR;
        wet[3u * cells + n] = 0.0;
        return;
    }
    let half = solid[2u * n + 1u].z;
    // The liquid's side: a filled wall keeps it on its normal's side; a hollow one, the side with
    // more liquid two cells off the wall, else what the node above knew, else what it knew.
    var side = 1.0;
    if half >= 0.0 {
        let x = i32(n % w.grid.x);
        let y = i32(n / w.grid.x);
        let o = vec2<i32>(round(s.xy * 2.0));
        let ahead = mass_at(x + o.x, y + o.y);
        let behind = mass_at(x - o.x, y - o.y);
        side = wet[3u * cells + n];
        if ahead > behind {
            side = 1.0;
        } else if behind > ahead {
            side = -1.0;
        } else if side == 0.0 && n >= w.grid.x {
            side = wet[3u * cells + n - w.grid.x];
        }
    }
    var face = -FAR;
    if side != 0.0 {
        face = select(side * s.z - half, s.z, half < 0.0);
    }
    wet[2u * cells + n] = face;
    wet[3u * cells + n] = side;
    if face < -0.5 {
        wet[to + n] = 0.0;
        return;
    }
    if f32(grid[4u * n]) > w.rates.z {
        wet[to + n] = 1.0;
        return;
    }
    let was = wet[old + n] * w.rates.x;
    var v = was;
    if n >= w.grid.x {
        let above = wet[old + n - w.grid.x] * 0.97;
        v = max(was, was + (above - was) * w.rates.y);
    }
    wet[to + n] = select(0.0, v, v > 1e-4);
}

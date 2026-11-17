// Liquid part 3: the wet grid (Wet walls). Once a liquid step, after its last substep: a grid
// node in the walls band with liquid on it is wet (1); any other node keeps `keep` of what it had
// and runs `drip` of the way towards most of the node above it (world y is down), so wet runs
// slowly down the walls in streaks and dries. Two halves, read one and write the other, so no node reads what another writes in
// the same pass. See liquid_sim/wet.rs.

struct Wet {
    // cols, rows, the half written (0 or 1), 0
    grid: vec4<u32>,
    // keep, drip, mass that wets a node (fixed point), 0
    rates: vec4<f32>,
};

@group(0) @binding(0) var<uniform> w: Wet;
// per node: mass, momentum x, momentum y, volume (fixed point; the middle two are speeds by now)
@group(0) @binding(1) var<storage, read> grid: array<i32>;
// per node: (normal xy, distance, in the band), (wall speed xy, 0, 0)
@group(0) @binding(2) var<storage, read> solid: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> wet: array<f32>;

@compute @workgroup_size(256)
fn cs_wet(@builtin(global_invocation_id) id: vec3<u32>) {
    let cells = w.grid.x * w.grid.y;
    let n = id.x;
    if n >= cells {
        return;
    }
    let to = w.grid.z * cells;
    let old = (1u - w.grid.z) * cells;
    if solid[2u * n].w < 0.5 {
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

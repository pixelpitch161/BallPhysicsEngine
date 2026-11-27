// ---- moving parts (Water mode part 5) ------------------------------------------------------------
//
// A step in which parts move starts by laying them over the still field at the step's end
// (`cs_movers`, over the nodes the frame's moves can change): each node takes the still field's
// distance and Wetting, and each part's distance (its own grid's, read in its own frame, times its
// scale) where that is nearer or the node is inside it; it is then the part's (its Wetting, its
// number in the field's third section, for the face's speed). The deep water's solids by cell are
// worked out again over those nodes and two more (`cs_deep_walls_moved`).

// Per layout node: the still field's distance (cells) and then its Wetting, as `solid`'s first two
// sections hold them.
@group(0) @binding(30) var<storage, read_write> still: array<f32>;
// Every moving part's distances (cells), in its own frame, one grid after another.
@group(0) @binding(31) var<storage, read_write> shapes: array<f32>;

// The furthest a field says (cells), as the host keeps it.
const FAR_CELLS: f32 = 64.0;

// Part `m`'s distance (cells, in its own frame at scale 1) at `own` (its own frame, cells);
// `FAR_CELLS` off its grid.
fn shape_at(m: Mover, own: vec2<f32>) -> f32 {
    let g = own - m.g.xy;
    let cols = m.h.x;
    let rows = m.h.y;
    if !(g.x >= 0.0 && g.y >= 0.0 && g.x <= f32(cols) - 1.0 && g.y <= f32(rows) - 1.0) {
        return FAR_CELLS;
    }
    let lo = min(vec2<u32>(floor(g)), vec2<u32>(max(cols, 2u) - 2u, max(rows, 2u) - 2u));
    let f = g - vec2<f32>(lo);
    let at = m.h.z + lo.y * cols + lo.x;
    let top = mix(shapes[at], shapes[at + 1u], f.x);
    let bottom = mix(shapes[at + cols], shapes[at + cols + 1u], f.x);
    return mix(top, bottom, f.y);
}

@compute @workgroup_size(16, 16)
fn cs_movers(@builtin(global_invocation_id) id: vec3<u32>) {
    let x = fr.mbox.x + id.x;
    let y = fr.mbox.y + id.y;
    if x > fr.mbox.z || y > fr.mbox.w || x >= fr.grid.x || y >= fr.grid.y {
        return;
    }
    let n = fr.grid.x * fr.grid.y;
    let k = node(i32(x), i32(y));
    var d = still[k];
    var wet = still[n + k];
    var who = 0u;
    let p = vec2<f32>(f32(x), f32(y));
    for (var j = 0u; j < fr.mv.y; j = j + 1u) {
        let m = fr.movers[j];
        let pose = mover_pose(m, st.t.x);
        let scale = max(pose.w, 0.01);
        let r = p - pose.xy;
        let reach = m.g.z * scale + 1.0;
        if dot(r, r) > reach * reach {
            continue;
        }
        let c = cos(pose.z);
        let s = sin(pose.z);
        let own = shape_at(m, vec2<f32>(r.x * c + r.y * s, -r.x * s + r.y * c) / scale);
        if own >= FAR_CELLS {
            continue;
        }
        let here = own * scale;
        if here < d || here < 0.0 {
            who = j + 1u;
            wet = m.g.w;
        }
        d = min(d, here);
    }
    solid[k] = clamp(d, -FAR_CELLS, FAR_CELLS);
    solid[n + k] = wet;
    solid[2u * n + k] = f32(who);
}

// The deep water's solids by cell again where parts moved (the moved nodes and two more round
// them: a cell reads the nodes a cell past its own).
@compute @workgroup_size(16, 16)
fn cs_deep_walls_moved(@builtin(global_invocation_id) id: vec3<u32>) {
    let x0 = max(fr.mbox.x, 2u) - 2u;
    let y0 = max(fr.mbox.y, 2u) - 2u;
    let x = x0 + id.x;
    let y = y0 + id.y;
    if x > fr.mbox.z + 2u || y > fr.mbox.w + 2u || x >= fr.grid.x || y >= fr.grid.y {
        return;
    }
    walls_at(node(i32(x), i32(y)));
}

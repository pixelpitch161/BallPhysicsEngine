// ---- the solids, once they are put -----------------------------------------------------------

// How far node (x, y) is from the liquid's edge at a solid (a quarter cell off its face): below 0
// where no liquid can be. Far, off the grid.
fn node_phi(x: i32, y: i32) -> f32 {
    if !in_grid(x, y) {
        return 4.0;
    }
    return clamp(solid[node(x, y)] - GAP, -4.0, 4.0);
}

// The open share of a face whose two ends are `a` and `b` from the liquid's edge.
fn open_share(a: f32, b: f32) -> f32 {
    if a >= 0.0 && b >= 0.0 {
        return 1.0;
    }
    if a < 0.0 && b < 0.0 {
        return 0.0;
    }
    let hi = max(a, b);
    let f = hi / (hi - min(a, b));
    if f < 0.1 {
        return 0.0;
    }
    if f > 0.9 {
        return 1.0;
    }
    return f;
}

// The solids' share of a kernel a cell wide each way round `q` (a hat, as the drops' volume is
// spread): how much of it lies past the liquid's edge, from 5 x 5 samples of the solid field.
fn solid_share(q: vec2<f32>) -> f32 {
    var sum = 0.0;
    var all = 0.0;
    for (var a = -2; a <= 2; a = a + 1) {
        for (var b = -2; b <= 2; b = b + 1) {
            let o = vec2<f32>(f32(a), f32(b)) * 0.4;
            let w = (1.0 - abs(o.x)) * (1.0 - abs(o.y));
            let phi = solid_at(q + o).z - GAP;
            sum = sum + w * clamp(0.5 - phi / 0.4, 0.0, 1.0);
            all = all + w;
        }
    }
    return sum / all;
}

@compute @workgroup_size(16, 16)
fn cs_walls(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= st.grid.x || id.y >= st.grid.y {
        return;
    }
    let x = i32(id.x);
    let y = i32(id.y);
    let k = node(x, y);
    let p00 = node_phi(x, y);
    let open_u = open_share(p00, node_phi(x, y + 1));
    let open_v = open_share(p00, node_phi(x + 1, y));
    let share = solid_share(vec2<f32>(f32(x) + 0.5, f32(y) + 0.5));
    // The same round the node itself, for the hold nodes.
    let at_node = solid_share(vec2<f32>(f32(x), f32(y)));
    wallc[k] = vec4<f32>(open_u, open_v, share, at_node);
}

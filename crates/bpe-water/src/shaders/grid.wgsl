// ---- the cells: what must flow out, and the faces after the pressure ---------------------------

// The speed on the left (`c` 0) or top (`c` 2) face of cell `k` from the sums, and whether it has
// one (enough liquid on it).
fn face_speed(k: u32, c: u32) -> vec2<f32> {
    let m = f32(atomicLoad(&sums[SUMS * k + c])) / S_M;
    if m < st.a.w {
        return vec2<f32>(0.0, 0.0);
    }
    return vec2<f32>(f32(atomicLoad(&sums[SUMS * k + c + 1u])) / (S_P * m), 1.0);
}

// Whether the course's edge shuts the liquid in (`st.flags.y`).
fn edges_shut() -> bool {
    return st.flags.y == 1u;
}

// The open share of the left face of cell (x, y): its solids', and shut at the course's edges
// when they stop the liquid.
fn open_left(x: i32, y: i32) -> f32 {
    let cols = i32(st.grid.x);
    let rows = i32(st.grid.y);
    if x < 0 || y < 0 || x >= cols || y >= rows - 1 {
        return 0.0;
    }
    if edges_shut() && (x <= i32(EDGE) || x >= cols - 1 - i32(EDGE) || y < i32(EDGE) || y >= rows - 1 - i32(EDGE)) {
        return 0.0;
    }
    return wallc[node(x, y)].x;
}

fn open_top(x: i32, y: i32) -> f32 {
    let cols = i32(st.grid.x);
    let rows = i32(st.grid.y);
    if x < 0 || y < 0 || x >= cols - 1 || y >= rows {
        return 0.0;
    }
    if edges_shut() && (y <= i32(EDGE) || y >= rows - 1 - i32(EDGE) || x < i32(EDGE) || x >= cols - 1 - i32(EDGE)) {
        return 0.0;
    }
    return wallc[node(x, y)].y;
}

// The share of a cell middle's kernel (a hat a cell wide each way) that lies more than `d` away
// along one way.
fn hat_beyond(d: f32) -> f32 {
    if d >= 1.0 {
        return 0.0;
    }
    if d <= -1.0 {
        return 1.0;
    }
    if d >= 0.0 {
        return 0.5 * (1.0 - d) * (1.0 - d);
    }
    return 1.0 - 0.5 * (1.0 + d) * (1.0 + d);
}

// The course's edges' share of a cell middle's kernel, when they stop the liquid.
fn edge_share(q: vec2<f32>) -> f32 {
    if !edges_shut() {
        return 0.0;
    }
    let lo = EDGE + 0.05 - GAP;
    let hi = vec2<f32>(f32(st.grid.x) - 1.0, f32(st.grid.y) - 1.0) - lo;
    let sx = hat_beyond(q.x - lo) + hat_beyond(hi.x - q.x);
    let sy = hat_beyond(q.y - lo) + hat_beyond(hi.y - q.y);
    return 1.0 - (1.0 - min(sx, 1.0)) * (1.0 - min(sy, 1.0));
}

@compute @workgroup_size(16, 16)
fn cs_grid(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= st.grid.x || id.y >= st.grid.y {
        return;
    }
    let x = i32(id.x);
    let y = i32(id.y);
    let k = node(x, y);
    let cols = i32(st.grid.x);
    let rows = i32(st.grid.y);
    let left = face_speed(k, 0u);
    let top = face_speed(k, 2u);
    var bits = left.y + 2.0 * top.y;
    let liquid = f32(atomicLoad(&sums[SUMS * k + 4u])) / S_M;
    if x >= cols - 1 || y >= rows - 1 {
        // (The last column and row of nodes are no cells.)
        vel[k] = vec4<f32>(left.x, top.x, 0.0, bits);
        coef[k] = vec4<f32>(0.0);
        cellp[k] = vec4<f32>(0.0);
        return;
    }
    let right = face_speed(k + 1u, 0u);
    let bottom = face_speed(k + st.grid.x, 2u);
    let a_l = open_left(x, y);
    let a_r = open_left(x + 1, y);
    let a_t = open_top(x, y);
    let a_b = open_top(x, y + 1);
    let diag = a_l + a_r + a_t + a_b;
    let q = vec2<f32>(f32(x) + 0.5, f32(y) + 0.5);
    let walls = wallc[k].z;
    let rho = liquid + 1.0 - (1.0 - walls) * (1.0 - edge_share(q));
    let solved = liquid > 1e-4 && diag > 1e-3;
    var want = 0.0;
    var p = 0.0;
    if solved {
        let dt = st.time.x;
        // A face no drop reaches has no speed of its own: it is air, and the liquid moves into it
        // freely, so it takes the speed of the face across the cell (as nothing, the cell would
        // see a fast lone drop as running into still liquid, and brake it: a thin stream piled up
        // on its own gaps and fell as slow flat clusters).
        let u_l = select(select(0.0, right.x, right.y > 0.5), left.x, left.y > 0.5);
        let u_r = select(select(0.0, left.x, left.y > 0.5), right.x, right.y > 0.5);
        let v_t = select(select(0.0, bottom.x, bottom.y > 0.5), top.x, top.y > 0.5);
        let v_b = select(select(0.0, top.x, top.y > 0.5), bottom.x, bottom.y > 0.5);
        let out = a_r * u_r - a_l * u_l + a_b * v_b - a_t * v_t;
        // What must flow out: an over-full cell empties (a share of its excess a step, capped); one
        // with room may fill (a share of its room a step) before it pushes back.
        let e = rho - 1.0;
        var s = 0.0;
        if e > 0.0 {
            s = min(st.a.y * e, st.a.z) / dt;
        } else {
            s = st.b.x * e / dt;
        }
        want = s - out;
        // The pressure it had: where the sweeps start from.
        p = cellp[k].x;
        if !(p >= 0.0 && p < 1e9) {
            p = 0.0;
        }
        bits = bits + 4.0;
    }
    vel[k] = vec4<f32>(left.x, top.x, liquid, bits);
    coef[k] = vec4<f32>(a_l, a_r, a_t, a_b);
    cellp[k] = vec4<f32>(p, want, diag, select(0.0, 1.0, solved));
}

// The faces' speeds with the pressure's push, for the drops to gather.
@compute @workgroup_size(16, 16)
fn cs_proj(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= st.grid.x || id.y >= st.grid.y {
        return;
    }
    let k = id.y * st.grid.x + id.x;
    let x = i32(id.x);
    let y = i32(id.y);
    var v = vel[k];
    let bits = u32(v.w + 0.5);
    var has_u = (bits & 1u) != 0u;
    var has_v = (bits & 2u) != 0u;
    let p = cellp[k].x;
    if has_u {
        if open_left(x, y) > 0.0 {
            var other = 0.0;
            if x > 0 {
                other = cellp[k - 1u].x;
            }
            v.x = v.x - (p - other);
        } else {
            // A shut face has no speed of its own: the drops leave it out.
            has_u = false;
        }
    }
    if has_v {
        if open_top(x, y) > 0.0 {
            var other = 0.0;
            if y > 0 {
                other = cellp[k - st.grid.x].x;
            }
            v.y = v.y - (p - other);
        } else {
            has_v = false;
        }
    }
    vel[k] = vec4<f32>(v.x, v.y, v.z, f32(u32(has_u) + 2u * u32(has_v) + (bits & 4u)));
}

// The mean of the speeds (`which` 0 across, 1 down) of the faces of cells `a` to `d` that have one,
// and how many had.
fn mean_of(which: u32, a: u32, b: u32, c: u32, d: u32) -> vec2<f32> {
    var sum = 0.0;
    var n = 0.0;
    let ks = array<u32, 4>(a, b, c, d);
    for (var e = 0; e < 4; e = e + 1) {
        let g = vel[ks[e]];
        if (u32(g.w + 0.5) & (1u << which)) != 0u {
            sum = sum + select(g.y, g.x, which == 0u);
            n = n + 1.0;
        }
    }
    if n < 0.5 {
        return vec2<f32>(0.0, 0.0);
    }
    return vec2<f32>(sum / n, n);
}

// Free slip: a face a solid shuts takes the liquid's speed beside it, less its part into or out
// of the solid (so a face along a wall's normal reads nothing, and one inside a slope reads the
// slide along it). Without it the drops by a wall gathered a share of "nothing" from every shut
// face: a slope braked the liquid like a staircase.
@compute @workgroup_size(16, 16)
fn cs_slip(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= st.grid.x || id.y >= st.grid.y {
        return;
    }
    let k = id.y * st.grid.x + id.x;
    let x = i32(id.x);
    let y = i32(id.y);
    let cols = i32(st.grid.x);
    let rows = i32(st.grid.y);
    var v = vel[k];
    var bits = u32(v.w + 0.5);
    let inner = x > 0 && y > 0 && x < cols - 1 && y < rows - 1;
    let shut = edges_shut();
    if inner && (bits & 1u) == 0u && open_left(x, y) <= 0.0 {
        let own = mean_of(0u, k - 1u, k + 1u, k - st.grid.x, k + st.grid.x);
        if own.y > 0.5 {
            let other = mean_of(1u, k - 1u, k, k - 1u + st.grid.x, k + st.grid.x);
            var n = solid_at(vec2<f32>(f32(x), f32(y) + 0.5)).xy;
            if shut && (x <= i32(EDGE) || x >= cols - 1 - i32(EDGE)) {
                n = vec2<f32>(1.0, 0.0);
            } else if shut && (y < i32(EDGE) || y >= rows - 1 - i32(EDGE)) {
                n = vec2<f32>(0.0, 1.0);
            }
            let flow = vec2<f32>(own.x, other.x);
            v.x = (flow - dot(flow, n) * n).x;
            bits = bits | 1u;
        }
    }
    if inner && (bits & 2u) == 0u && open_top(x, y) <= 0.0 {
        let own = mean_of(1u, k - 1u, k + 1u, k - st.grid.x, k + st.grid.x);
        if own.y > 0.5 {
            let other = mean_of(0u, k, k + 1u, k - st.grid.x, k - st.grid.x + 1u);
            var n = solid_at(vec2<f32>(f32(x) + 0.5, f32(y))).xy;
            if shut && (y <= i32(EDGE) || y >= rows - 1 - i32(EDGE)) {
                n = vec2<f32>(0.0, 1.0);
            } else if shut && (x < i32(EDGE) || x >= cols - 1 - i32(EDGE)) {
                n = vec2<f32>(1.0, 0.0);
            }
            let flow = vec2<f32>(other.x, own.x);
            v.y = (flow - dot(flow, n) * n).y;
            bits = bits | 2u;
        }
    }
    vel_out[k] = vec4<f32>(v.x, v.y, v.z, f32(bits));
}

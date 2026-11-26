
// ---- the pressure -----------------------------------------------------------------------------

// A drop's change of its multiplier this sweep (the first sweep starts every multiplier at
// nothing).
fn lambda_at(i: u32, first: bool) {
    let xi = y_in[i].xy;
    if !alive(xi) {
        lamb[i] = vec4<f32>(0.0);
        return;
    }
    let h = fr.k.x;
    let c = cell_of(xi);
    var rho = 0.0;
    var sum = vec2<f32>(0.0);
    var sq = 0.0;
    for (var dy = -1; dy <= 1; dy = dy + 1) {
        let y = c.y + dy;
        if y < 0 || y >= i32(fr.cells.y) {
            continue;
        }
        let x0 = u32(max(c.x - 1, 0));
        let x1 = u32(min(c.x + 1, i32(fr.cells.x) - 1));
        let first_j = starts[u32(y) * fr.cells.x + x0];
        let last_j = starts[u32(y) * fr.cells.x + x1 + 1u];
        for (var j = first_j; j < last_j; j = j + 1u) {
            let r = xi - y_in[j].xy;
            let r2 = dot(r, r);
            if r2 >= h * h {
                continue;
            }
            let far = sqrt(r2);
            rho = rho + weight(far);
            if j == i {
                continue;
            }
            var g = vec2<f32>(0.0);
            if r2 > 1e-10 {
                g = slope(r, far);
            } else {
                g = any_way(i, j) * (-30.0 / (PI * h * h * h));
            }
            sum = sum + g;
            sq = sq + dot(g, g);
        }
    }
    let wall = wall_density(xi);
    rho = rho + wall.z;
    sum = sum + wall.xy;
    let rho0 = fr.k.y;
    let over = rho / rho0 - 1.0;
    let stiff = (dot(sum, sum) + sq) / (rho0 * rho0);
    var was = 0.0;
    if !first {
        was = lamb[i].y;
    }
    // (Never above nothing: the liquid pushes apart where it is over-full and never pulls.)
    var now = min(was - fr.k.w * over / (stiff + fr.k.z), 0.0);
    if !(abs(now) < 1e30) {
        now = 0.0;
    }
    lamb[i] = vec4<f32>(now - was, now, rho / rho0, 0.0);
}

// The first sweep, which also empties the bins' counts for the next step (the scatter is done
// with them).
@compute @workgroup_size(256)
fn cs_lambda_first(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let stride = groups.x * 256u;
    for (var c = gid.x; c <= fr.cells.z; c = c + stride) {
        atomicStore(&counts[c], 0u);
    }
    if gid.x < st.n.x {
        lambda_at(gid.x, true);
    }
}

@compute @workgroup_size(256)
fn cs_lambda(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x < st.n.x {
        lambda_at(gid.x, false);
    }
}

// A drop moved by its own and its neighbours' changes, kept off the solids. The last sweep also
// works out its speed: how far it went this step.
fn apply_at(i: u32, last: bool) {
    let me = y_in[i];
    let xi = me.xy;
    let was = velb[i];
    if !alive(xi) {
        y_out[i] = me;
        if last {
            velb[i] = vec4<f32>(0.0, 0.0, was.z, 0.0);
        }
        return;
    }
    let h = fr.k.x;
    let c = cell_of(xi);
    let mine = lamb[i].x;
    var shift = vec2<f32>(0.0);
    for (var dy = -1; dy <= 1; dy = dy + 1) {
        let y = c.y + dy;
        if y < 0 || y >= i32(fr.cells.y) {
            continue;
        }
        let x0 = u32(max(c.x - 1, 0));
        let x1 = u32(min(c.x + 1, i32(fr.cells.x) - 1));
        let first_j = starts[u32(y) * fr.cells.x + x0];
        let last_j = starts[u32(y) * fr.cells.x + x1 + 1u];
        for (var j = first_j; j < last_j; j = j + 1u) {
            if j == i {
                continue;
            }
            let r = xi - y_in[j].xy;
            let r2 = dot(r, r);
            if r2 >= h * h {
                continue;
            }
            var g = vec2<f32>(0.0);
            if r2 > 1e-10 {
                g = slope(r, sqrt(r2));
            } else {
                g = any_way(i, j) * (-30.0 / (PI * h * h * h));
            }
            shift = shift + (mine + lamb[j].x) * g;
        }
    }
    shift = (shift + mine * wall_density(xi).xy) / fr.k.y;
    let far = length(shift);
    if far > fr.s.z {
        shift = shift * (fr.s.z / far);
    }
    if !finite2(shift) {
        shift = vec2<f32>(0.0);
    }
    let kind = u32(was.z);
    let x = keep_off(xi + shift, vec2<f32>(0.0), stops_at_edge(kind)).x;
    y_out[i] = vec4<f32>(x, me.zw);
    if last {
        velb[i] = vec4<f32>((x - was.xy) / fr.time.x, was.z, 0.0);
    }
}

@compute @workgroup_size(256)
fn cs_apply(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x < st.n.x {
        apply_at(gid.x, false);
    }
}

@compute @workgroup_size(256)
fn cs_apply_last(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x < st.n.x {
        apply_at(gid.x, true);
    }
}


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
    // (Air is pushed only by air and the solids: the liquid makes way for it, so it rises through
    // the liquid as a bubble does, and a pocket of it keeps its room. Its fullness counts every
    // drop.)
    let air = is_air(u32(velb[i].z));
    var rho = 0.0;
    var all = 0.0;
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
            let wj = weight(far);
            all = all + wj;
            if air && j != i && !is_air(u32(velb[j].z)) {
                continue;
            }
            rho = rho + wj;
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
    let drops_only = all;
    all = all + wall.z;
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
    // (Whether air bursts is looked at for air alone: `select` would look for every drop.)
    var burst = 0.0;
    if air {
        burst = select(1.0, 2.0, bursts(xi, all / rho0, drops_only / rho0));
    }
    lamb[i] = vec4<f32>(now - was, now, all / rho0, burst);
}

// Whether an air drop at `x`, `full` full (`drops` full by the drops alone), bursts: it has open air
// beside it. Under the water's open top it is less full than `air.y`; out of the water (where no
// deep water is) a solid beside it does not count, so air thrown out of the water against a ceiling
// bursts too, and air held under a solid deep in the water stays.
fn bursts(x: vec2<f32>, full: f32, drops: f32) -> bool {
    if full < fr.air.y {
        return true;
    }
    if drops >= fr.air.y {
        return false;
    }
    let off = vec2<f32>(0.5, 0.5);
    let b = base2(x, off);
    let f = clamp(x - off - vec2<f32>(b), vec2<f32>(0.0), vec2<f32>(1.0));
    var w = 0.0;
    for (var a = 0; a < 2; a = a + 1) {
        for (var c = 0; c < 2; c = c + 1) {
            w = w + bilinear(f, a, c) * dfa[node(b.x + a, b.y + c)];
        }
    }
    return !(w > 0.0);
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
    let air = lamb[i].w > 0.5;
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
            if j == i || (air && lamb[j].w < 0.5) {
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

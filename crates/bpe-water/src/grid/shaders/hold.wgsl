// ---- the hold-together -----------------------------------------------------------------------
//
// A drop carries how stretched the liquid at it is (`e`, J - 1). Stretched (0 < e < its liquid's
// tear) it pulls: a pressure below nothing that grows with the stretch, so drops pulled further
// apart pull back harder and a thin stream evens out its own spacing instead of gathering in
// lumps. Torn (e at the tear) it is loose and pulls on nothing. The pull is felt through the hold
// nodes with a smooth 3-node kernel, mass and pull alike, so a lone drop pulls on nothing and two
// drops pull on each other equally. The pressure solve that follows stops the pull squashing the
// liquid.

@compute @workgroup_size(256)
fn cs_clear(@builtin(global_invocation_id) id: vec3<u32>) {
    let k = id.x;
    if k >= st.grid.x * st.grid.y {
        return;
    }
    for (var c = 0u; c < SUMS; c = c + 1u) {
        atomicStore(&sums[SUMS * k + c], 0);
    }
    for (var c = 0u; c < HOLD; c = c + 1u) {
        atomicStore(&hold[HOLD * k + c], 0);
    }
    // The node's four spots.
    if st.r.x > 0.0 {
        let x = i32(k % st.grid.x);
        let y = i32(k / st.grid.x);
        for (var b = 0; b < 2; b = b + 1) {
            for (var a = 0; a < 2; a = a + 1) {
                let s = spot_index(vec2<i32>(2 * x + a, 2 * y + b));
                for (var c = 0u; c < SPOT; c = c + 1u) {
                    atomicStore(&spots[s + c], 0);
                }
            }
        }
    }
}

// The pressure a drop's stretch gives (below 0: a pull), with the damping of its own spreading.
fn pull_of(p: Drop) -> f32 {
    let feel = st.feels[liquid_of(p)];
    if st.h.w < 0.5 || !(p.e > 0.0) || p.e >= feel.a.z {
        return 0.0;
    }
    let weight = feel.b.x;
    let div = p.c.x + p.c.w;
    return -feel.a.y * weight * p.e - st.h.x * sqrt(feel.a.y) * weight * div;
}

@compute @workgroup_size(256)
fn cs_hold_p2g(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= st.grid.z {
        return;
    }
    let p = drops[i];
    if !steps(p) {
        return;
    }
    // Where it is, for its neighbours' elbow room.
    if st.r.x > 0.0 {
        let sp = spot_of(p.x);
        let s = spot_index(sp.at);
        atomicAdd(&spots[s], 1);
        atomicAdd(&spots[s + 1u], sp.within.x);
        atomicAdd(&spots[s + 2u], sp.within.y);
    }
    let feel = st.feels[liquid_of(p)];
    let mass = feel.b.x * VOL;
    let base = base3(p.x);
    let fx = frac3(p.x, base);
    let w = weights3(fx);
    let dw = slopes3(fx);
    let k = st.time.x * VOL * pull_of(p);
    for (var a = 0; a < 3; a = a + 1) {
        for (var b = 0; b < 3; b = b + 1) {
            let n = node(base.x + a, base.y + b);
            let wt = w[a].x * w[b].y;
            atomicAdd(&hold[HOLD * n], fixed(wt * mass, S_M));
            atomicAdd(&hold[HOLD * n + 3u], fixed(wt * VOL, S_M));
            if k != 0.0 {
                let slope = vec2<f32>(dw[a].x * w[b].y, w[a].x * dw[b].y);
                atomicAdd(&hold[HOLD * n + 1u], fixed(k * slope.x, S_P));
                atomicAdd(&hold[HOLD * n + 2u], fixed(k * slope.y, S_P));
            }
        }
    }
}

// Each hold node's change of speed: the pull's impulse over the liquid's mass there. A solid's
// share of the node counts as liquid that cannot move, as sticky as the liquid is: so a stretched
// drop beside a solid is pulled to it.
@compute @workgroup_size(16, 16)
fn cs_hold_grid(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= st.grid.x || id.y >= st.grid.y {
        return;
    }
    let k = id.y * st.grid.x + id.x;
    let m = f32(atomicLoad(&hold[HOLD * k])) / S_M;
    let vol = f32(atomicLoad(&hold[HOLD * k + 3u])) / S_M;
    let walls = wallc[k].w;
    var dv = vec2<f32>(0.0);
    if m > 1e-6 {
        let impulse = vec2<f32>(f32(atomicLoad(&hold[HOLD * k + 1u])), f32(atomicLoad(&hold[HOLD * k + 2u]))) / S_P;
        // (The stickiness is the first liquid's: one number for the node.)
        let held = st.feels[0].a.w * walls * 8.0;
        dv = impulse / (m + held);
    }
    hold_out[k] = vec4<f32>(dv, vol + walls, vol);
}

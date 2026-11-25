
// ---- speeds, the skin, grip and drag ----------------------------------------------------------

@compute @workgroup_size(256)
fn cs_finish(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= st.n.x {
        return;
    }
    let me = y_in[i];
    let xi = me.xy;
    if !alive(xi) {
        xv[i] = vec4<f32>(xi, 0.0, 0.0);
        nf[i] = vec4<f32>(0.0);
        return;
    }
    let mv = velb[i];
    let feel = fr.feels[u32(mv.z) & 15u];
    let dt = fr.time.x;
    let h = fr.k.x;
    let rho0 = fr.k.y;
    let c = cell_of(xi);
    let vi = mv.xy;
    let li = lamb[i];
    var even = vec2<f32>(0.0);
    var lies = vec2<f32>(0.0);
    var pull = vec2<f32>(0.0);
    var bend = vec2<f32>(0.0);
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
            let other = y_in[j];
            let r = xi - other.xy;
            let r2 = dot(r, r);
            if r2 >= h * h {
                continue;
            }
            let lj = lamb[j];
            var far = sqrt(r2);
            var way = vec2<f32>(0.0);
            if r2 > 1e-10 {
                way = r / far;
            } else {
                way = -any_way(i, j);
                far = 1e-5;
            }
            even = even + (velb[j].xy - vi) * weight(far);
            // (Which way the liquid lies, by a smooth kernel: by the steep one it is mostly the
            // nearest drop's way, and the bend's pull turns the surface ragged.)
            let t = 1.0 - r2 / (h * h);
            lies = lies - way * (far * 24.0 / (PI * h * h * h * h) * t * t);
            // (Drops with little liquid round them pull harder: a surface's drops have fewer
            // neighbours to be pulled by.)
            let thin = 2.0 / max(li.z + lj.z, 0.5);
            pull = pull + thin * skin(far / h) * way;
            bend = bend + thin * (me.zw - other.zw);
        }
    }
    // (The evening of speeds is the liquid's own: its Thickness.)
    var v = vi + feel.b.z * even / rho0;
    var a = -feel.a.y * pull - feel.a.z * bend;
    let s = solid_at(xi);
    if s.z < h {
        // Grip: a pull toward a near face, none at the face itself.
        let off = fr.w.y;
        let t = clamp((s.z - off) / max(h - off, 1e-3), 0.0, 1.0);
        a = a - feel.a.w * 4.0 * t * (1.0 - t) * s.xy;
    }
    // The skin's and the grip's change of speed in a step is held to a share of the cap.
    var dv = a * dt;
    let most = 0.25 * fr.time.w;
    let big = length(dv);
    if big > most {
        dv = dv * (most / big);
    }
    v = v + dv;
    if s.z < fr.s.y {
        // Drag: speed along a near face is lost.
        let near = 1.0 - smoothstep(fr.w.y, fr.s.y, s.z);
        let along = v - dot(v, s.xy) * s.xy;
        v = v - along * (1.0 - exp(-feel.b.y * near * dt));
    }
    if !finite2(v) {
        v = vec2<f32>(0.0);
    }
    xv[i] = vec4<f32>(xi, v);
    nf[i] = vec4<f32>(lies * (h / 4.0), li.z, 0.0);
}

// The drops `st.n.x` on, `st.n.y` of them, in the grid core's layout (its `c` holds how full the
// drop's neighbourhood is, 0, and which way the liquid lies from it).
@compute @workgroup_size(256)
fn cs_export(@builtin(global_invocation_id) gid: vec3<u32>) {
    let k = gid.x;
    if k >= st.n.y {
        return;
    }
    let i = st.n.x + k;
    let p = xv[i];
    let n = nf[i];
    let d = id_cur[i];
    outd[k] = DropOut(p.xy, p.zw, vec4<f32>(n.z, 0.0, n.xy), 0.0, d.x, d.y, d.z);
}

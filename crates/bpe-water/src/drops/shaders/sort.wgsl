
// ---- falling, and sorting ---------------------------------------------------------------------

@compute @workgroup_size(256)
fn cs_predict(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= st.n.x {
        return;
    }
    if i >= st.n.y {
        // Born this step: it is already where it is at the step's end, and was a step's travel
        // back (so the speed the step finds is its own).
        let b = births[st.n.z + (i - st.n.y)];
        xv[i] = b.xv;
        nf[i] = vec4<f32>(0.0, 0.0, 1.0, 0.0);
        id_cur[i] = b.id;
        let x = b.xv.xy;
        xp[i] = vec4<f32>(x, x - b.xv.zw * fr.time.x);
        atomicAdd(&counts[bin_of(x)], 1u);
        return;
    }
    let p = xv[i];
    if !alive(p.xy) {
        xp[i] = vec4<f32>(p.xy, p.xy);
        atomicAdd(&counts[fr.cells.z], 1u);
        return;
    }
    let kind = id_cur[i].x;
    let feel = fr.feels[kind & 15u];
    let dt = fr.time.x;
    var v = p.zw + vec2<f32>(fr.time.y, fr.time.z) * feel.a.x * dt;
    v = v * exp(-feel.b.x * dt);
    let speed = length(v);
    if speed > fr.time.w {
        v = v * (fr.time.w / speed);
    }
    if !finite2(v) {
        v = vec2<f32>(0.0);
    }
    // Moved in short sub-moves, kept off the solids at each: a fast drop cannot jump a thin wall.
    let stops = stops_at_edge(kind);
    let moves = clamp(u32(ceil(length(v) * dt / max(fr.w.z, 0.05))), 1u, 24u);
    let h = dt / f32(moves);
    var x = p.xy;
    for (var m = 0u; m < moves; m = m + 1u) {
        let kept = keep_off(x + v * h, v, stops);
        x = kept.x;
        v = kept.v;
    }
    xp[i] = vec4<f32>(x, p.xy);
    atomicAdd(&counts[bin_of(x)], 1u);
}

// The running sums of the bins' counts, in blocks: within each block...
@compute @workgroup_size(256)
fn cs_sum_a(@builtin(global_invocation_id) gid: vec3<u32>) {
    let b = gid.x;
    if b >= fr.cells.w {
        return;
    }
    var run = 0u;
    for (var k = 0u; k < BLOCK; k = k + 1u) {
        let c = b * BLOCK + k;
        if c <= fr.cells.z {
            starts[c] = run;
            run = run + atomicLoad(&counts[c]);
        }
    }
    blocks[b] = run;
}

var<workgroup> part: array<u32, 256>;

// ...over the blocks (one workgroup: each thread sums a run of blocks, the runs' sums are summed
// across the workgroup, then each run's blocks get their starts)...
@compute @workgroup_size(256)
fn cs_sum_b(@builtin(local_invocation_id) lid: vec3<u32>) {
    let t = lid.x;
    let all = fr.cells.w;
    let run = (all + 255u) / 256u;
    let a = min(t * run, all);
    let b = min(a + run, all);
    var sum = 0u;
    for (var k = a; k < b; k = k + 1u) {
        sum = sum + blocks[k];
    }
    part[t] = sum;
    workgroupBarrier();
    for (var off = 1u; off < 256u; off = off * 2u) {
        var add = 0u;
        if t >= off {
            add = part[t - off];
        }
        workgroupBarrier();
        part[t] = part[t] + add;
        workgroupBarrier();
    }
    var base = 0u;
    if t > 0u {
        base = part[t - 1u];
    }
    for (var k = a; k < b; k = k + 1u) {
        let here = blocks[k];
        blocks[k] = base;
        base = base + here;
    }
}

// ...and each bin's start. The counts are emptied for the scatter.
@compute @workgroup_size(256)
fn cs_sum_c(@builtin(global_invocation_id) gid: vec3<u32>) {
    let c = gid.x;
    if c > fr.cells.z {
        return;
    }
    starts[c] = starts[c] + blocks[c / BLOCK];
    atomicStore(&counts[c], 0u);
    if c == fr.cells.z {
        starts[c + 1u] = st.n.x;
    }
}

@compute @workgroup_size(256)
fn cs_scatter(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= st.n.x {
        return;
    }
    let c = bin_of(xp[i].xy);
    let s = starts[c] + atomicAdd(&counts[c], 1u);
    idx[s] = i;
}

// Each drop to its sorted place: by bin, and within a bin by serial (with no cap on how many a bin
// holds), so every sum over neighbours adds in the same order on every run.
@compute @workgroup_size(256)
fn cs_order(@builtin(global_invocation_id) gid: vec3<u32>) {
    let s = gid.x;
    if s >= st.n.x {
        return;
    }
    let i = idx[s];
    let q = xp[i];
    let c = bin_of(q.xy);
    let a = starts[c];
    let b = starts[c + 1u];
    let me = id_cur[i];
    var rank = 0u;
    for (var t = a; t < b; t = t + 1u) {
        let j = idx[t];
        if j != i {
            let other = id_cur[j].y;
            if other < me.y || (other == me.y && j < i) {
                rank = rank + 1u;
            }
        }
    }
    let dest = a + rank;
    let n = nf[i];
    id_nxt[dest] = me;
    y_in[dest] = vec4<f32>(q.xy, n.xy);
    velb[dest] = vec4<f32>(q.zw, f32(me.x & 511u), 0.0);
}

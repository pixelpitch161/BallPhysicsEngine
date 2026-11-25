// ---- the drops: to the grid and back ----------------------------------------------------------

@compute @workgroup_size(256)
fn cs_p2g(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= st.grid.z {
        return;
    }
    let p = drops[i];
    if !steps(p) {
        return;
    }
    let feel = st.feels[liquid_of(p)];
    let mass = feel.b.x * VOL;
    let dt = st.time.x;
    var v = p.v + vec2<f32>(st.time.y, st.time.z) * feel.a.x * dt;
    // The hold-together's pull, from the hold nodes.
    if st.h.w > 0.5 {
        let base = base3(p.x);
        let fx = frac3(p.x, base);
        let w = weights3(fx);
        var dv = vec2<f32>(0.0);
        for (var a = 0; a < 3; a = a + 1) {
            for (var b = 0; b < 3; b = b + 1) {
                dv = dv + w[a].x * w[b].y * hold_out[node(base.x + a, base.y + b)].xy;
            }
        }
        v = v + dv;
    }
    // Speed across, on left faces (at whole x, y + a half).
    {
        let off = vec2<f32>(0.0, 0.5);
        let base = base2(p.x, off);
        let f = clamp(p.x - off - vec2<f32>(base), vec2<f32>(0.0), vec2<f32>(1.0));
        for (var a = 0; a < 2; a = a + 1) {
            for (var b = 0; b < 2; b = b + 1) {
                let k = node(base.x + a, base.y + b);
                let w = select(1.0 - f.x, f.x, a == 1) * select(1.0 - f.y, f.y, b == 1);
                let dpos = vec2<f32>(f32(base.x + a), f32(base.y + b) + 0.5) - p.x;
                let val = v.x + p.c.x * dpos.x + p.c.y * dpos.y;
                atomicAdd(&sums[SUMS * k], fixed(w * mass, S_M));
                atomicAdd(&sums[SUMS * k + 1u], fixed(w * mass * val, S_P));
            }
        }
    }
    // Speed down, on top faces (at x + a half, whole y).
    {
        let off = vec2<f32>(0.5, 0.0);
        let base = base2(p.x, off);
        let f = clamp(p.x - off - vec2<f32>(base), vec2<f32>(0.0), vec2<f32>(1.0));
        for (var a = 0; a < 2; a = a + 1) {
            for (var b = 0; b < 2; b = b + 1) {
                let k = node(base.x + a, base.y + b);
                let w = select(1.0 - f.x, f.x, a == 1) * select(1.0 - f.y, f.y, b == 1);
                let dpos = vec2<f32>(f32(base.x + a) + 0.5, f32(base.y + b)) - p.x;
                let val = v.y + p.c.z * dpos.x + p.c.w * dpos.y;
                atomicAdd(&sums[SUMS * k + 2u], fixed(w * mass, S_M));
                atomicAdd(&sums[SUMS * k + 3u], fixed(w * mass * val, S_P));
            }
        }
    }
    // Volume, at cell middles.
    {
        let off = vec2<f32>(0.5, 0.5);
        let base = base2(p.x, off);
        let f = clamp(p.x - off - vec2<f32>(base), vec2<f32>(0.0), vec2<f32>(1.0));
        for (var a = 0; a < 2; a = a + 1) {
            for (var b = 0; b < 2; b = b + 1) {
                let k = node(base.x + a, base.y + b);
                let w = select(1.0 - f.x, f.x, a == 1) * select(1.0 - f.y, f.y, b == 1);
                atomicAdd(&sums[SUMS * k + 4u], fixed(w * VOL, S_M));
            }
        }
    }
}

// One of a drop's two speeds gathered from the 2 x 2 faces round it: the speed and its slope
// (along x, along y). Faces with no speed are left out and the rest re-weighted, as if the missing
// ones had the mean. `had` is 0 when no face had a speed.
struct Got {
    v: f32,
    slope: vec2<f32>,
    had: f32,
};

fn gather_one(x: vec2<f32>, off: vec2<f32>, which: u32) -> Got {
    let base = base2(x, off);
    let f = clamp(x - off - vec2<f32>(base), vec2<f32>(0.0), vec2<f32>(1.0));
    var ws = array<f32, 4>(0.0, 0.0, 0.0, 0.0);
    var vs = array<f32, 4>(0.0, 0.0, 0.0, 0.0);
    var sum_w = 0.0;
    var sum_v = 0.0;
    for (var a = 0; a < 2; a = a + 1) {
        for (var b = 0; b < 2; b = b + 1) {
            let k = node(base.x + a, base.y + b);
            let g = vel_out[k];
            let bits = u32(g.w + 0.5);
            if (bits & (1u << which)) != 0u {
                let w = select(1.0 - f.x, f.x, a == 1) * select(1.0 - f.y, f.y, b == 1);
                let val = select(g.y, g.x, which == 0u);
                ws[a * 2 + b] = 1.0;
                vs[a * 2 + b] = val;
                sum_w = sum_w + w;
                sum_v = sum_v + w * val;
            }
        }
    }
    if sum_w < 1e-4 {
        return Got(0.0, vec2<f32>(0.0), 0.0);
    }
    let mean = sum_v / sum_w;
    var slope = vec2<f32>(0.0);
    for (var a = 0; a < 2; a = a + 1) {
        for (var b = 0; b < 2; b = b + 1) {
            if ws[a * 2 + b] > 0.5 {
                let wx = select(1.0 - f.x, f.x, a == 1);
                let wy = select(1.0 - f.y, f.y, b == 1);
                let dx = select(-1.0, 1.0, a == 1) * wy;
                let dy = select(-1.0, 1.0, b == 1) * wx;
                slope = slope + vec2<f32>(dx, dy) * (vs[a * 2 + b] - mean);
            }
        }
    }
    return Got(mean, slope, 1.0);
}

// Keeps a drop at `x` moving at `v` off the solids and (when it stops there) the course's edge.
struct Kept {
    x: vec2<f32>,
    v: vec2<f32>,
};

fn keep_off(xin: vec2<f32>, vin: vec2<f32>, stops: bool) -> Kept {
    var x = xin;
    var v = vin;
    let s = solid_at(x);
    if s.z < OFF {
        x = x + s.xy * min(OFF - s.z, 4.0);
        let vn = dot(v, s.xy);
        if vn < 0.0 {
            v = v - vn * s.xy;
        }
    }
    if stops {
        let lo = vec2<f32>(EDGE);
        let hi = vec2<f32>(f32(st.grid.x) - 1.0 - EDGE, f32(st.grid.y) - 1.0 - EDGE);
        if x.x < lo.x + 0.05 { x.x = lo.x + 0.05; v.x = max(v.x, 0.0); }
        if x.x > hi.x - 0.05 { x.x = hi.x - 0.05; v.x = min(v.x, 0.0); }
        if x.y < lo.y + 0.05 { x.y = lo.y + 0.05; v.y = max(v.y, 0.0); }
        if x.y > hi.y - 0.05 { x.y = hi.y - 0.05; v.y = min(v.y, 0.0); }
    }
    return Kept(x, v);
}

// How far a drop moves this step to keep its elbow room: away from the other drops of the nine
// spots round it that are nearer than `st.r.y` (each spot's drops taken together, at their middle;
// its own spot's without itself).
fn elbow_room(p: Drop) -> vec2<f32> {
    let reach = st.r.y;
    if !(st.r.x > 0.0) || !(reach > 0.0) {
        return vec2<f32>(0.0);
    }
    let sp = spot_of(p.x);
    let me = (vec2<f32>(sp.at) + vec2<f32>(sp.within) / SPOT_ONE) * 0.5;
    var push = vec2<f32>(0.0);
    for (var b = -1; b <= 1; b = b + 1) {
        for (var a = -1; a <= 1; a = a + 1) {
            let at = sp.at + vec2<i32>(a, b);
            if at.x < 0 || at.y < 0 || at.x >= i32(2u * st.grid.x) || at.y >= i32(2u * st.grid.y) {
                continue;
            }
            let s = spot_index(at);
            var n = atomicLoad(&spots[s]);
            var sum = vec2<i32>(atomicLoad(&spots[s + 1u]), atomicLoad(&spots[s + 2u]));
            if a == 0 && b == 0 {
                n = n - 1;
                sum = sum - sp.within;
            }
            if n <= 0 {
                continue;
            }
            let middle = (vec2<f32>(at) + vec2<f32>(sum) / (f32(n) * SPOT_ONE)) * 0.5;
            var off = me - middle;
            var far = length(off);
            if far >= reach {
                continue;
            }
            if far < 1e-3 {
                // On top of each other: each its own way, by its serial (scrambled, so drops born
                // one after the other part widely).
                let turn = f32((p.serial * 2654435761u) >> 22u) * 0.006135923;
                off = vec2<f32>(cos(turn), sin(turn));
                far = 0.0;
            } else {
                off = off / far;
            }
            push = push + off * (f32(min(n, 4)) * (reach - far));
        }
    }
    var shift = st.r.x * push;
    let len = length(shift);
    if len > st.r.z {
        shift = shift * (st.r.z / len);
    }
    if !finite2(shift) {
        return vec2<f32>(0.0);
    }
    return shift;
}

@compute @workgroup_size(256)
fn cs_g2p(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= st.grid.z {
        return;
    }
    var p = drops[i];
    if !steps(p) {
        return;
    }
    let feel = st.feels[liquid_of(p)];
    let dt = st.time.x;
    let gu = gather_one(p.x, vec2<f32>(0.0, 0.5), 0u);
    let gv = gather_one(p.x, vec2<f32>(0.5, 0.0), 1u);
    // With no face to go by (it is alone in a solid's margin), it falls on its own.
    let own = p.v + vec2<f32>(st.time.y, st.time.z) * feel.a.x * dt;
    var v = vec2<f32>(select(own.x, gu.v, gu.had > 0.5), select(own.y, gv.v, gv.had > 0.5));
    var c = vec4<f32>(gu.slope, gv.slope);
    // The liquid round it: the drops' own, and with the solids' share.
    var liquid = 0.0;
    var full = 0.0;
    {
        let off = vec2<f32>(0.5, 0.5);
        let base = base2(p.x, off);
        let f = clamp(p.x - off - vec2<f32>(base), vec2<f32>(0.0), vec2<f32>(1.0));
        for (var a = 0; a < 2; a = a + 1) {
            for (var b = 0; b < 2; b = b + 1) {
                let w = select(1.0 - f.x, f.x, a == 1) * select(1.0 - f.y, f.y, b == 1);
                let k = node(base.x + a, base.y + b);
                let own_vol = vel_out[k].z;
                liquid = liquid + w * own_vol;
                full = full + w * (own_vol + wallc[k].z);
            }
        }
    }
    // The hold nodes at the drop (their smooth kernel): the liquid round it with the solids'
    // share, the drops' own, and the own's slope; and its own part in those, to take out.
    var round = 0.0;
    var mine = 0.0;
    var own_slope = vec2<f32>(0.0);
    var self_own = 0.0;
    var self_slope = vec2<f32>(0.0);
    {
        let base = base3(p.x);
        let fx = frac3(p.x, base);
        let w = weights3(fx);
        let dw = slopes3(fx);
        for (var a = 0; a < 3; a = a + 1) {
            for (var b = 0; b < 3; b = b + 1) {
                let h = hold_out[node(base.x + a, base.y + b)];
                let wt = w[a].x * w[b].y;
                let g = vec2<f32>(dw[a].x * w[b].y, w[a].x * dw[b].y);
                round = round + wt * h.z;
                mine = mine + wt * h.w;
                own_slope = own_slope + g * h.w;
                self_own = self_own + VOL * wt * wt;
                // (The nodes' values held still, as `own_slope` reads them: once, not the twice of a
                // drop's own density sloping as it moves.)
                self_slope = self_slope + VOL * wt * g;
            }
        }
    }
    // How stretched it is: moved by the flow's own spreading; nothing is stretched inside full
    // liquid; a torn drop joins again where the liquid is dense enough.
    var e = p.e;
    if !(abs(e) < 1e30) {
        e = 0.0;
    }
    let tear = feel.a.z;
    if e >= tear {
        if full >= st.h.z {
            e = 0.0;
        }
    } else {
        e = e + (1.0 + e) * dt * (c.x + c.w);
        // Inside full liquid it settles fast; everywhere it eases off slowly (a pull is how
        // the liquid answers being pulled apart, not something it keeps).
        let rate = st.h2.x + st.h.y * smoothstep(st.h2.y - 0.15, st.h2.y, round);
        e = e * exp(-rate * dt);
        e = clamp(e, -0.5, tear);
    }
    // Thin liquid on its way evens out its own spacing: a drop moves a little, along the way it
    // travels only, from where the other drops round it are many to where they are few (nothing
    // is stirred and nothing spreads sideways). A stream that
    // left a shape in bunches is one even thread a little further on. Not inside a body of liquid
    // (the pressure spaces that), and not at rest.
    var even = vec2<f32>(0.0);
    let pace = length(v);
    if st.h2.z > 0.0 && pace > 1e-3 {
        let way = v / pace;
        let others = max(mine - self_own, 0.0);
        let slope = dot(own_slope - self_slope, way);
        let thin = 1.0 - smoothstep(0.35, 0.5, mine);
        let moving = smoothstep(12.0, 36.0, pace);
        let far = clamp(-st.h2.z * dt * slope / (others + 0.05) * thin * moving, -st.h2.w, st.h2.w);
        if abs(far) < 1e9 {
            // Moved along its own flight, as if a moment earlier or later on it: its place and its
            // speed both, so it stays on the path it was flying (moved in place only, each drop
            // of a curving thread went on along a path beside its own and the thread frayed).
            let fall = vec2<f32>(st.time.y, st.time.z) * feel.a.x;
            let moment = far / pace;
            even = v * moment + 0.5 * fall * moment * moment;
            v = v + fall * moment;
        }
    }
    // Elbow room: drops nearer each other than `st.r.y` move apart (their places only, never
    // their speeds: liquid that was squeezed takes its room back without being thrown).
    let apart = elbow_room(p) * smoothstep(0.33 * st.r.w, st.r.w, length(v));
    v = v * exp(-feel.b.y * dt);
    let speed = length(v);
    if speed > st.time.w {
        v = v * (st.time.w / speed);
        c = vec4<f32>(0.0);
    }
    if !finite2(v) {
        v = vec2<f32>(0.0);
        c = vec4<f32>(0.0);
    }
    // Moved in short sub-moves, kept off the solids at each: a fast drop cannot jump a thin wall.
    let stops = (p.kind & STOPS) != 0u;
    let moves = clamp(u32(ceil(length(v) * dt / max(st.b.y, 0.05))), 1u, 24u);
    let h = dt / f32(moves);
    var x = keep_off(p.x + even + apart, v, stops).x;
    for (var m = 0u; m < moves; m = m + 1u) {
        let kept = keep_off(x + v * h, v, stops);
        x = kept.x;
        v = kept.v;
    }
    drops[i].x = x;
    drops[i].v = v;
    drops[i].c = c;
    drops[i].e = e;
}

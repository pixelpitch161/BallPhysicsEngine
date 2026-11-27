
// ---- speeds, the skin, grip and drag ----------------------------------------------------------

// A drop as a step leaves it: its place and speed, and its kind.
struct Finished {
    p: vec4<f32>,
    kind: u32,
};

// Works out sorted drop `i`'s speed at the step's end (the evening of speeds, the skin, grip and
// drag), writes which way the liquid lies from it and how full it is, and returns it.
fn finish_at(i: u32) -> Finished {
    let me = y_in[i];
    let xi = me.xy;
    let mv = velb[i];
    if !alive(xi) {
        nf[i] = vec4<f32>(0.0);
        return Finished(vec4<f32>(xi, 0.0, 0.0), u32(mv.z));
    }
    let kind = u32(mv.z);
    // (Air has no skin of its own, and the liquid's skin leaves it out: to the skin a pocket's wall
    // is a surface, so a bubble keeps round. The evening of speeds and the closing loss act between
    // every pair: a bubble's drag.)
    let air = is_air(kind);
    let feel = fr.feels[kind & 15u];
    let dt = fr.time.x;
    let h = fr.k.x;
    let rho0 = fr.k.y;
    let c = cell_of(xi);
    let vi = mv.xy;
    let li = lamb[i];
    var even = vec2<f32>(0.0);
    var meet = vec2<f32>(0.0);
    var lies = vec2<f32>(0.0);
    var pull = vec2<f32>(0.0);
    var bend = vec2<f32>(0.0);
    // (Air under the water's open top: liquid over it that is not full, a film or the surface.)
    var film = false;
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
            let vj = velb[j];
            // (Air and liquid even out and close on each other only by a share: a bubble's drag.)
            let wj = weight(far) * select(1.0, fr.air.w, air != is_air(u32(vj.z)));
            let rel = vj.xy - vi;
            even = even + rel * wj;
            // (Drops with little liquid round them pull harder: a surface's drops have fewer
            // neighbours to be pulled by. The pair's own number, so the two give and take alike.)
            let thin = 2.0 / max(li.z + lj.z, 0.5);
            // (Drops that close on each other: `way` points from the other to this one. Each of
            // the pair loses the same share of the closing speed, by the pair's own weight, so
            // momentum is kept.)
            let closing = dot(rel, way);
            if closing > 0.0 {
                meet = meet + closing * way * (wj * thin);
            }
            if air || is_air(u32(vj.z)) {
                // (The other over this air drop, liquid, with room beside it: it is a film or the
                // water's top, and the bubble bursts through it, as a real one's film drains and
                // breaks.)
                film = film || (air && !is_air(u32(vj.z)) && r.y > 0.5 * far && lj.z < fr.air.y);
                continue;
            }
            // (Which way the liquid lies, by a smooth kernel: by the steep one it is mostly the
            // nearest drop's way, and the bend's pull turns the surface ragged.)
            let t = 1.0 - r2 / (h * h);
            lies = lies - way * (far * 24.0 / (PI * h * h * h * h) * t * t);
            pull = pull + thin * skin(far / h) * way;
            bend = bend + thin * (me.zw - other.zw);
        }
    }
    // (The evening of speeds is the liquid's own: its Thickness. Drops that meet do not bounce
    // apart: a share of the speed at which they close is taken away, over the neighbours' weight
    // a drop deep in the liquid has, the same for every drop.)
    if air && (li.w > 1.5 || film) {
        // Air with open air beside it, or under the water's open top, bursts: it leaves the course
        // (and is let go with the drops that left).
        nf[i] = vec4<f32>(0.0);
        return Finished(vec4<f32>(-1e6, -1e6, 0.0, 0.0), kind);
    }
    let s = solid_at(xi);
    // Wetting: a wetted face is liquid to the liquid's skin, as much as it is wet. It pulls the
    // drops near it as the liquid does (adhesion), the liquid lies toward it, so a puddle's foot is
    // no surface for the bend to hold up, and it counts in the bend as resting liquid with no
    // surface of its own: else the bend, which the face would not match, holds a soaked floor's
    // edge up (part 3, item 0).
    var wet = 0.0;
    if s.z < h && !air {
        wet = wet_at(xi);
        if wet > 0.0 {
            let t = wet_table(s.z / h);
            let thin_w = 2.0 / max(li.z + 1.0, 0.5);
            pull = pull + (wet * thin_w * t.x) * s.xy;
            lies = lies - (wet * t.y) * s.xy;
            // (The lattice's drops within reach: the disc's part past the face, over a spacing
            // squared, a quarter of a cell. Each adds the pair's bend with a neighbour whose own
            // surface way is nothing.)
            let q = clamp(s.z, 0.0, h);
            let seg = h * h * acos(q / h) - q * sqrt(max(h * h - q * q, 0.0));
            bend = bend + (fr.deep2.w * wet * thin_w * 4.0 * seg) * me.zw;
        }
    }
    var v = vi + feel.b.z * even / rho0;
    v = v + fr.deep2.y * meet / max(rho0 - weight(0.0), 1e-6);
    var a = vec2<f32>(0.0);
    if !air {
        a = -feel.a.y * pull - feel.a.z * bend;
    }
    if s.z < h && !air {
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
        // Drag: speed along a near face is lost (as the face moves: the speed along it beside its own).
        let near = 1.0 - smoothstep(fr.w.y, fr.s.y, s.z);
        let rel = v - solid_speed(xi);
        let along = rel - dot(rel, s.xy) * s.xy;
        // (A waxy face keeps only a share of the drag every liquid has.)
        let drag = feel.b.y - fr.s.w * (1.0 - wet) * (1.0 - fr.w.x);
        v = v - along * (1.0 - exp(-max(drag, 0.0) * near * dt));
    }
    if !finite2(v) {
        v = vec2<f32>(0.0);
    }
    nf[i] = vec4<f32>(lies * (h / 4.0), li.z, 0.0);
    return Finished(vec4<f32>(xi, v), kind);
}

// The last step of a frame: its drops kept.
@compute @workgroup_size(256)
fn cs_finish(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= st.n.x {
        return;
    }
    xv[i] = finish_at(i).p;
}

// A step's end joined with the next step's fall (its `st`: the step's drops are the first
// `st.n.y`; the next step's births come in by `cs_born`). Within a frame the places and speeds
// stay in the step's own buffers: only the frame's last step keeps them (`cs_finish`).
@compute @workgroup_size(256)
fn cs_finish_predict(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= st.n.y {
        return;
    }
    let f = finish_at(i);
    fall(i, f.p, f.kind);
}

// The drops `st.n.x` on, `st.n.y` of them, in `Drop`'s layout (its `c` holds how full the
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

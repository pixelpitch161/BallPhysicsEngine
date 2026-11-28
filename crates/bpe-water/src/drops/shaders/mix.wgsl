// ---- colour mixing (Water mode part 4.2) ----------------------------------------------------------
//
// A drop's mix, its id's 4th word: the share of one other liquid it carries, the dye in it (bits 0 to
// 3 that liquid, bits 16 to 31 its share of 65535; 0 unmixed). Its motion stays its own liquid's.
// Once a frame, after its last step, `fr.mixf.y` times over: each liquid drop's holding of every
// liquid moves toward its liquid neighbours' (`cs_mix`, into `mixb`), and is kept (`cs_mix_keep`,
// into the sorted ids). A diffusion: a pair's share a pass is the product of the two liquids' roots
// (`Feel.c.z`, from their Mix speeds, so it is the same px² a second at every detail) times the
// spiky kernel's weight for the Laplacian, times how much the two don't keep apart, times when they
// mix (Mixes when).

// Per drop, sorted: its mix after this pass.
@group(0) @binding(30) var<storage, read_write> mixb: array<u32>;

fn mix_share(w: u32) -> f32 {
    return f32(w >> 16u) / 65535.0;
}

fn mix_other(w: u32) -> u32 {
    return w & 15u;
}

// How much two drops' colours mix by drop `kind`'s liquid's Mixes when, at `rel` and `closing`, how
// fast they move against each other and close on each other (cells a second): Always all of it,
// Only when stirred by how fast they move against each other (none below a quarter of the stir
// speed: a resting pool's drops jiggle against each other at up to about 35 px a second, at 6 and 3
// px cells alike, and that is no stir), Only on hits by how fast they close.
fn mix_gate(kind: u32, rel: f32, closing: f32) -> f32 {
    let when = fr.feels[kind & 15u].c.w;
    if when > 1.5 {
        return smoothstep(0.5 * fr.mixs.y, fr.mixs.y, closing);
    }
    if when > 0.5 {
        return smoothstep(0.25 * fr.mixs.x, fr.mixs.x, rel);
    }
    return 1.0;
}

@compute @workgroup_size(256)
fn cs_mix(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= st.n.x {
        return;
    }
    let me = id_nxt[i];
    mixb[i] = me.w;
    let p = xv[i];
    let own = me.x & 15u;
    let ri = fr.feels[own].c.z;
    // (Air, a drop that left and a liquid that doesn't mix keep their word.)
    if !alive(p.xy) || is_air(me.x) || !(ri > 0.0) {
        return;
    }
    let h = fr.k.x;
    let c = cell_of(p.xy);
    let lap = 2.0 / fr.k.y * 30.0 / (PI * h * h * h);
    // How much of each liquid the neighbours hold, each by its share a pass, and those shares' sum.
    var held: array<f32, 16>;
    var total = 0.0;
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
            let q = xv[j];
            let r = p.xy - q.xy;
            let r2 = dot(r, r);
            if !(r2 < h * h) {
                continue;
            }
            let o = id_nxt[j];
            let rj = fr.feels[o.x & 15u].c.z;
            if is_air(o.x) || !(rj > 0.0) {
                continue;
            }
            let far = sqrt(r2);
            let t = 1.0 - far / h;
            let rel = q.zw - p.zw;
            // (`r` points from the other to this one: they close when the other moves this way.)
            let closing = select(0.0, dot(rel, r / far), far > 1e-6);
            let speed = length(rel);
            let gate = min(mix_gate(me.x, speed, closing), mix_gate(o.x, speed, closing));
            let share = gate * (1.0 - keep_of(me.x, o.x)) * ri * rj * lap * t * t / max(far, 0.05 * h);
            if !(share > 0.0) {
                continue;
            }
            let sj = mix_share(o.w);
            held[o.x & 15u] = held[o.x & 15u] + share * (1.0 - sj);
            if sj > 0.0 {
                held[mix_other(o.w)] = held[mix_other(o.w)] + share * sj;
            }
            total = total + share;
        }
    }
    if !(total > 0.0) {
        return;
    }
    // (A crowded spot takes at most 0.9 of its neighbours' mean a pass, so it stays calm.)
    let scale = select(1.0, 0.9 / total, total > 0.9);
    let si = mix_share(me.w);
    let oi = mix_other(me.w);
    var own_now = 0.0;
    var best = 0u;
    var most = 0.0;
    for (var l = 0u; l < 16u; l = l + 1u) {
        var mine = 0.0;
        if l == own {
            mine = 1.0 - si;
        } else if si > 0.0 && l == oi {
            mine = si;
        }
        let now = mine * (1.0 - total * scale) + held[l] * scale;
        if l == own {
            own_now = now;
        } else if now > most {
            most = now;
            best = l;
        }
    }
    // (What isn't its own liquid is carried as the one it holds most of: with three liquids round
    // it, the weaker's dye goes to the stronger, own against other kept.)
    let share = u32(round(clamp(1.0 - own_now, 0.0, 1.0) * 65535.0));
    mixb[i] = select(best | (share << 16u), 0u, share == 0u || !(most > 0.0));
}

// The pass's mixes kept, for the next pass to read (and the next frame to carry).
@compute @workgroup_size(256)
fn cs_mix_keep(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= st.n.x {
        return;
    }
    id_nxt[i].w = mixb[i];
}

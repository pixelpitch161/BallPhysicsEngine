
// ---- thick liquids (Water mode part 4.3b) -------------------------------------------------------
//
// A liquid thicker than water and oil (its `Feel.d.x` > 0) takes, once a frame after its last
// step's Finish (as the colours mix), Peer, Cornelis and Teschner's prescribed velocity gradient
// (2015, "An Implicit Viscosity Formulation for SPH Fluids"). Each of its drops works out the
// gradient of its speeds from its own liquid's neighbours, read exactly for a straight-line field
// even at a surface (`cs_thick_grad`), and keeps only its turn, letting go of its shear and of its
// spreading or squeezing. (Peer, Cornelis and Teschner keep the spreading too, a liquid's own being
// next to none; but a stream falling from a tap draws out, its rows parting as they speed up, and
// that kept as a spreading fanned the stream out sideways into spray.) Then stages (`cs_thick_stage`, each followed by `cs_thick_tick`) move
// each drop's speed toward the one its neighbours' speeds and the kept gradients give it. Inside
// the liquid a stage is the Laplacian's smoothing (a straight-line field, a turn included, is
// kept); at a surface it keeps a turn and lets go of shear, as a free surface does, so a falling
// thread can buckle and fold and a blob keeps its spin. A solid face within reach counts as liquid
// at rest behind it, so a thick liquid clings to what it touches. (A thick liquid has no plain evening of
// speeds in its Finish: that pulls a surface drop toward its neighbours' speeds and wears a turn
// away.)
//
// The smoothing a frame wants grows with the Thickness and shrinks with the cells (honey on fine
// cells wants thousands of plain sweeps), so the sweeps are the stages of a Runge-Kutta-Legendre
// step (Meyer, Balsara and Aslam's RKL1, "super time stepping"): `s` stages give as much smoothing
// as (s^2 + s) / 2 plain ones and stay calm, each a plain sweep with its liquid's share `q`
// (`Feel.d.x`) times `mu_j = (2j - 1) / j`, plus `nu_j = (1 - j) / j` of the stage before last.
//
// The sorted drops are as the frame's last Finish left them (`xv`, `id_nxt` their kinds). Each
// drop's kept turn is in `lamb`'s y, and the stages' speeds rotate
// through three places: `xv`'s z and w, `xp`'s x and y, `xp`'s z and w (stage j writes place j % 3;
// the stages are a multiple of three, so the last lands back in `xv`). `blocks[0]` is the stage's
// number. All three are free once the step's Finish has run.

// The share of a plain sweep a stage takes for a drop of `kind` (0: it is not thick).
fn thick_share(kind: u32) -> f32 {
    if is_air(kind) {
        return 0.0;
    }
    return fr.feels[kind & 15u].d.x;
}

// Whether neighbour kind `o` is of liquid `own` (not air).
fn thick_same(own: u32, o: u32) -> bool {
    return !is_air(o) && (o & 15u) == (own & 15u);
}

// What the kept turn `turn` (radians a second) moves a point `x` off by.
fn thick_off(turn: f32, x: vec2<f32>) -> vec2<f32> {
    return turn * vec2<f32>(x.y, -x.x);
}

@compute @workgroup_size(256)
fn cs_thick_grad(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    // (The first stage's number.)
    if i == 0u {
        blocks[0] = 1u;
    }
    if i >= st.n.x {
        return;
    }
    lamb[i] = vec4<f32>(0.0);
    let p = xv[i];
    let own = id_nxt[i].x;
    if !alive(p.xy) || !(thick_share(own) > 0.0) {
        return;
    }
    let h = fr.k.x;
    let c = cell_of(p.xy);
    let slope = -30.0 / (PI * h * h * h);
    // A = sum (v_j - v_i) (x) grad W_ij and M = sum (x_j - x_i) (x) grad W_ij, as columns: a
    // straight-line field's gradient is A M^-1.
    var a0 = vec2<f32>(0.0);
    var a1 = vec2<f32>(0.0);
    var m0 = vec2<f32>(0.0);
    var m1 = vec2<f32>(0.0);
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
            if j == i || !thick_same(own, id_nxt[j].x) {
                continue;
            }
            let q = xv[j];
            let r = p.xy - q.xy;
            let r2 = dot(r, r);
            if !(r2 < h * h) || !(r2 > 1e-12) || !alive(q.xy) {
                continue;
            }
            let far = sqrt(r2);
            let t = 1.0 - far / h;
            let g = (slope * t * t / far) * r;
            let dv = q.zw - p.zw;
            let dx = -r;
            a0 = a0 + dv * g.x;
            a1 = a1 + dv * g.y;
            m0 = m0 + dx * g.x;
            m1 = m1 + dx * g.y;
        }
    }
    // (A thread one drop wide reads no gradient across it: M's smaller eigenvalue is held up to a
    // tenth of its trace. M is symmetric, each pair's gradient lying along the pair, and anywhere
    // with neighbours all round, a surface too, its smaller eigenvalue is well over that, so a
    // straight-line field's gradient is read exactly: a share of the trace added everywhere would
    // read every turn short, and the sweeps would wear a blob's spin away at its surface.)
    let tr = m0.x + m1.y;
    if !(tr > 1e-9) {
        return;
    }
    let off_diag = 0.5 * (m1.x + m0.y);
    let half_gap = 0.5 * (m0.x - m1.y);
    let least = 0.5 * tr - sqrt(half_gap * half_gap + off_diag * off_diag);
    let reg = max(0.1 * tr - least, 0.0);
    let ma = m0.x + reg;
    let mb = off_diag;
    let mc = off_diag;
    let md = m1.y + reg;
    let det = ma * md - mb * mc;
    if !(abs(det) > 1e-12) {
        return;
    }
    // M^-1's columns.
    let i0 = vec2<f32>(md, -mc) / det;
    let i1 = vec2<f32>(-mb, ma) / det;
    // G = A M^-1: its columns are the speeds' change along x and along y.
    let g0 = a0 * i0.x + a1 * i0.y;
    let g1 = a0 * i1.x + a1 * i1.y;
    // Kept: the turn; the shear and the spreading are let go.
    let turn = 0.5 * (g1.x - g0.y);
    if finite2(vec2<f32>(turn, 0.0)) {
        lamb[i] = vec4<f32>(0.0, turn, 0.0, 0.0);
    }
}

// Drop `i`'s speed in place `at` (0 `xv`'s, 1 and 2 `xp`'s).
fn thick_at(i: u32, at: u32) -> vec2<f32> {
    if at == 0u {
        return xv[i].zw;
    }
    if at == 1u {
        return xp[i].xy;
    }
    return xp[i].zw;
}

fn thick_put(i: u32, at: u32, v: vec2<f32>) {
    if at == 0u {
        xv[i].z = v.x;
        xv[i].w = v.y;
    } else if at == 1u {
        xp[i].x = v.x;
        xp[i].y = v.y;
    } else {
        xp[i].z = v.x;
        xp[i].w = v.y;
    }
}

// A stage: each thick drop's speed in the last stage's place moved its liquid's share of the way to
// its neighbours' and its own there, weighted by the kernel, each moved on by the pair's kept
// gradient; then RKL1's mix of that and the stage before last.
@compute @workgroup_size(256)
fn cs_thick_stage(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= st.n.x {
        return;
    }
    let stage = max(blocks[0], 1u);
    let last = (stage + 2u) % 3u;
    let before = (stage + 1u) % 3u;
    let p = xv[i];
    let own = id_nxt[i].x;
    let share = thick_share(own);
    // (A drop that is not thick keeps its speed in `xv` and is never read by the stages.)
    if !alive(p.xy) || !(share > 0.0) {
        return;
    }
    let vi = thick_at(i, last);
    let h = fr.k.x;
    let c = cell_of(p.xy);
    let gi = lamb[i].y;
    var sum = weight(0.0) * vi;
    var total = weight(0.0);
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
            if j == i || !thick_same(own, id_nxt[j].x) {
                continue;
            }
            // (Only the place: a stage that lands in `xv` writes its speeds.)
            let q = xv[j].xy;
            let r = p.xy - q;
            let r2 = dot(r, r);
            if !(r2 < h * h) || !alive(q) {
                continue;
            }
            let w = weight(sqrt(r2));
            sum = sum + w * (thick_at(j, last) + thick_off(0.5 * (gi + lamb[j].y), r));
            total = total + w;
        }
    }
    // (A solid face within reach holds the drop still, as the liquid would that lay behind it: its
    // share of the neighbourhood, at rest. So a thick liquid clings where it touches a solid, and
    // sags off a ledge instead of sliding off it.)
    let s = solid_at(p.xy);
    if s.z < h {
        total = total + wall_table(max(s.z, 0.0) / h).x * fr.k.y;
    }
    let swept = vi + share * (sum / total - vi);
    var v = swept;
    if stage > 1u {
        let k = f32(stage);
        v = ((2.0 * k - 1.0) / k) * swept + ((1.0 - k) / k) * thick_at(i, before);
    }
    if !finite2(v) {
        v = vi;
    }
    thick_put(i, stage % 3u, v);
}

// The next stage's number.
@compute @workgroup_size(1)
fn cs_thick_tick() {
    blocks[0] = blocks[0] + 1u;
}

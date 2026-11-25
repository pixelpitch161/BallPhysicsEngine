// ---- the pressure: red-black sweeps on a ladder of coarser grids -------------------------------

// Whether a cell of `level` is solved, from its `cellp` entry: level 0 keeps a flag in w; a
// coarser level keeps its floor there (below) and is solved when its middle is above 0.
fn is_solved(c: vec4<f32>, level: u32) -> bool {
    if level == 0u {
        return c.w > 0.5;
    }
    return c.z > 0.0;
}

// The least a cell's answer may be: no pressure on level 0; on a coarser level, as far down as
// the fullest-drained of its four can still go (so a correction never asks any cell under it for
// a pressure below 0). Without this a pool in free fall, whose cells all lack "less than
// nothing", handed the coarse levels a big negative sum, and their answer wiped out the pressure
// of everything.
fn floor_of(c: vec4<f32>, level: u32) -> f32 {
    if level == 0u {
        return 0.0;
    }
    return c.w;
}

// One colour of a red-black sweep of one level (`tick.n.x`: twice the level, plus the colour).
// Level 0 is the pressure itself, which is never below 0; a coarser level holds the correction to
// the level under it, which may be.
@compute @workgroup_size(16, 16)
fn cs_sor(@builtin(global_invocation_id) id: vec3<u32>) {
    let level = tick.n.x >> 1u;
    let lv = st.lv[level];
    if id.x >= lv.y || id.y >= lv.z {
        return;
    }
    if ((id.x + id.y) & 1u) != (tick.n.x & 1u) {
        return;
    }
    let k = lv.x + id.y * lv.y + id.x;
    let c = cellp[k];
    if !is_solved(c, level) {
        return;
    }
    let a = coef[k];
    var sum = c.y;
    if id.x > 0u {
        sum = sum + a.x * cellp[k - 1u].x;
    }
    if id.x + 1u < lv.y {
        sum = sum + a.y * cellp[k + 1u].x;
    }
    if id.y > 0u {
        sum = sum + a.z * cellp[k - lv.y].x;
    }
    if id.y + 1u < lv.z {
        sum = sum + a.w * cellp[k + lv.y].x;
    }
    var p = c.x + st.a.x * (sum / c.z - c.x);
    let least = floor_of(c, level);
    if !(p > least) {
        p = least;
    }
    cellp[k].x = p;
}

// Makes level `tick.n.x >> 1` (1 or more) from the level under it: each of its cells is four of
// that level's (2 x 2), its faces the sums of theirs that cross to a neighbouring four, what it
// must make up the sum of what theirs still lack. Its own answer starts at 0.
@compute @workgroup_size(16, 16)
fn cs_down(@builtin(global_invocation_id) id: vec3<u32>) {
    let level = tick.n.x >> 1u;
    let lv = st.lv[level];
    if id.x >= lv.y || id.y >= lv.z {
        return;
    }
    let f = st.lv[level - 1u];
    var faces = vec4<f32>(0.0);
    var diag = 0.0;
    var lacks = 0.0;
    var any = false;
    var least = -1e30;
    for (var b = 0u; b < 2u; b = b + 1u) {
        for (var a = 0u; a < 2u; a = a + 1u) {
            let x = 2u * id.x + a;
            let y = 2u * id.y + b;
            if x >= f.y || y >= f.z {
                continue;
            }
            let k = f.x + y * f.y + x;
            let c = cellp[k];
            if !is_solved(c, level - 1u) {
                continue;
            }
            any = true;
            let under = floor_of(c, level - 1u);
            least = max(least, under - c.x);
            let o = coef[k];
            // Its four neighbours' answers, and whether each is solved.
            var n = vec4<f32>(0.0);
            var on = vec4<bool>(false);
            if x > 0u {
                let m = cellp[k - 1u];
                n.x = m.x;
                on.x = is_solved(m, level - 1u);
            }
            if x + 1u < f.y {
                let m = cellp[k + 1u];
                n.y = m.x;
                on.y = is_solved(m, level - 1u);
            }
            if y > 0u {
                let m = cellp[k - f.y];
                n.z = m.x;
                on.z = is_solved(m, level - 1u);
            }
            if y + 1u < f.z {
                let m = cellp[k + f.y];
                n.w = m.x;
                on.w = is_solved(m, level - 1u);
            }
            var r = c.y + dot(o, n) - c.z * c.x;
            // A cell held at its floor that would rather have less lacks nothing.
            if c.x <= under && r < 0.0 {
                r = 0.0;
            }
            lacks = lacks + r;
            diag = diag + c.z;
            // Left and right: the face inside the four (taken off the middle once from each
            // side), or the one that crosses to the next four.
            if a == 1u {
                if on.x {
                    diag = diag - o.x;
                }
                if on.y {
                    faces.y = faces.y + o.y;
                }
            } else {
                if on.y {
                    diag = diag - o.y;
                }
                if on.x {
                    faces.x = faces.x + o.x;
                }
            }
            if b == 1u {
                if on.z {
                    diag = diag - o.z;
                }
                if on.w {
                    faces.w = faces.w + o.w;
                }
            } else {
                if on.w {
                    diag = diag - o.w;
                }
                if on.z {
                    faces.z = faces.z + o.z;
                }
            }
        }
    }
    let k = lv.x + id.y * lv.y + id.x;
    // What is left of the middle once the faces to the neighbouring fours are taken off is the
    // four's way out to the air. The faces are halved (the fours' middles are twice as far apart
    // as the cells' were); the way out is not, but its middle is a quarter of itself further
    // from the air than the cells' middles were. (Halved like the faces, every level put the
    // surface further off than it is, and a deep pool's pressure swung instead of settling.)
    let air = max(diag - (faces.x + faces.y + faces.z + faces.w), 0.0);
    faces = 0.5 * faces;
    diag = faces.x + faces.y + faces.z + faces.w + air / (1.0 + 0.25 * air);
    let solved = any && diag > 1e-4;
    coef[k] = select(vec4<f32>(0.0), faces, solved);
    cellp[k] = vec4<f32>(0.0, select(0.0, lacks, solved), select(0.0, diag, solved), min(least, 0.0));
}

// Adds level `(tick.n.x >> 1) + 1`'s answer to level `tick.n.x >> 1`: each cell takes its four's.
@compute @workgroup_size(16, 16)
fn cs_up(@builtin(global_invocation_id) id: vec3<u32>) {
    let level = tick.n.x >> 1u;
    let lv = st.lv[level];
    if id.x >= lv.y || id.y >= lv.z {
        return;
    }
    let k = lv.x + id.y * lv.y + id.x;
    let c = cellp[k];
    if !is_solved(c, level) {
        return;
    }
    let up = st.lv[level + 1u];
    let e = cellp[up.x + (id.y >> 1u) * up.y + (id.x >> 1u)].x;
    var p = c.x + st.b.z * e;
    let least = floor_of(c, level);
    if !(p > least) {
        p = least;
    }
    cellp[k].x = p;
}

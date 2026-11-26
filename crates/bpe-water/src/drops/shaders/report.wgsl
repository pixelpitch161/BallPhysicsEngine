
// ---- what the host reads back, two frames later ---------------------------------------------

// The fastest drop's speed in pixels a second (the bits of a float above nothing compare as whole
// numbers do, so the largest is the same however the card schedules the threads), how many drops
// are in the course after the frame's last sort and the air born after it, how many cells were
// deep, how many spots of trapped air were found, and each pour's fullness at its spout.
struct Report {
    fastest: atomic<u32>,
    live: u32,
    // the cells deep at the frame's last step (deep.wgsl)
    deep: u32,
    // the spots of trapped air found at the frame's last step (air.wgsl), born or not
    air: u32,
    spout: array<f32, 64>,
};

@group(0) @binding(18) var<storage, read_write> report: Report;

// How full a drop born at `q` would be (1 is full), the solids counted by their share. (Counting
// the new drop itself: the point between drops of a full pool has less than a drop there has.)
fn fullness_at(q: vec2<f32>) -> f32 {
    let h = fr.k.x;
    let c = cell_of(q);
    var rho = weight(0.0);
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
            let r = q - xv[j].xy;
            let r2 = dot(r, r);
            if r2 < h * h {
                rho = rho + weight(sqrt(r2));
            }
        }
    }
    return (rho + wall_density(q).z) / fr.k.y;
}

@compute @workgroup_size(256)
fn cs_report(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i == 0u {
        let found = airb[air_blocks()];
        report.live = starts[fr.cells.z] + min(found, fr.dgrid.w);
        report.deep = atomicLoad(&deep_count[0]);
        report.air = found;
    }
    if i < fr.grid.z {
        let both = fr.spouts[i / 2u];
        let at = select(both.xy, both.zw, (i & 1u) == 1u);
        if finite2(at) {
            // (A spout whose room lies outside the course is judged at the course's edge.)
            let q = clamp(at, vec2<f32>(0.0), vec2<f32>(fr.grid.xy) - vec2<f32>(1.0));
            report.spout[i] = fullness_at(q);
        }
    }
    if i >= st.n.x {
        return;
    }
    let p = xv[i];
    if !alive(p.xy) {
        return;
    }
    let speed = length(p.zw) * fr.s.x;
    if speed > 0.0 && speed < 1e30 {
        atomicMax(&report.fastest, bitcast<u32>(speed));
    }
}

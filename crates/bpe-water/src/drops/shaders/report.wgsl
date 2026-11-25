
// ---- what the host reads back, two frames later ---------------------------------------------

// The fastest drop's speed in pixels a second (the bits of a float above nothing compare as whole
// numbers do, so the largest is the same however the card schedules the threads), how many drops
// are in the course after the frame's last sort, and each pour's fullness at its spout.
struct Report {
    fastest: atomic<u32>,
    live: u32,
    spare: vec2<u32>,
    spout: array<f32, 64>,
};

@group(0) @binding(18) var<storage, read_write> report: Report;

@compute @workgroup_size(256)
fn cs_report(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i == 0u {
        report.live = starts[fr.cells.z];
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

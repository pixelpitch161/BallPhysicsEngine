// What the sound reads of each liquid (liquid part 5): per liquid, in u32 atomics (so the order
// drops add in can't change the sum), how many drops, their speed, their row, and their colour
// quantised to 0..63 a channel with its square. Every sum fits 2^20 drops. See liquid_sim/stats.rs.

struct Drop {
    x: vec2<f32>,
    v: vec2<f32>,
    c: vec4<f32>,
    colour: vec4<f32>,
    j: f32,
    kind: u32,
    serial: u32,
    kick: f32,
    mat: vec2<f32>,
    spare: vec2<f32>,
    f: vec4<f32>,
};

const WORDS: u32 = 12u;

@group(0) @binding(0) var<storage, read> drops: array<Drop>;
@group(0) @binding(1) var<storage, read> counts: array<u32>;
@group(0) @binding(2) var<storage, read_write> stats: array<atomic<u32>>;

fn finite(x: f32) -> bool {
    return abs(x) < 1e30;
}

@compute @workgroup_size(256)
fn cs_stats(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= counts[1] {
        return;
    }
    let d = drops[i];
    if !(finite(d.x.x) && finite(d.x.y) && finite(d.v.x) && finite(d.v.y)) {
        return;
    }
    // A stain (liquid part 4: liquid number 16 and up) is no liquid's drop.
    if (d.kind & 255u) >= 16u {
        return;
    }
    let base = (d.kind & 15u) * WORDS;
    atomicAdd(&stats[base], 1u);
    atomicAdd(&stats[base + 1u], u32(clamp(length(d.v) * 4.0, 0.0, 4095.0)));
    atomicAdd(&stats[base + 2u], u32(clamp(d.x.y, 0.0, 4094.0)));
    for (var k = 0u; k < 3u; k = k + 1u) {
        var c = d.colour[k];
        if !finite(c) {
            c = 0.0;
        }
        let q = u32(round(clamp(c, 0.0, 1.0) * 63.0));
        atomicAdd(&stats[base + 3u + k], q);
        atomicAdd(&stats[base + 6u + k], q * q);
    }
}

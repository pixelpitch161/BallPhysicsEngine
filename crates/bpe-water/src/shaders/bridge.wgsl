// The bridge to today's liquid drawing (docs/notes/water.md): each of the water's drops written
// as one of today's (bpe-gfx's liquid_sim.wgsl `Drop`, 28 numbers), so the drops pass and the
// surface pass draw Water mode's liquid unchanged. `cs_bridge` reads the grid core's drops,
// `cs_bridge_drops` the all-drops engine's (stored by field).

struct Drop {
    x: vec2<f32>,
    v: vec2<f32>,
    c: vec4<f32>,
    e: f32,
    kind: u32,
    serial: u32,
    colour: u32,
};

struct Bridge {
    // columns and rows of nodes, drops, 0
    grid: vec4<u32>,
};

const DROP_FLOATS: u32 = 28u;

@group(0) @binding(0) var<uniform> br: Bridge;
@group(0) @binding(1) var<storage, read> drops: array<Drop>;
// per cell: ..., the solids' share of its middle's kernel, ...
@group(0) @binding(2) var<storage, read> wallc: array<vec4<f32>>;
// per cell: ..., the drops' volume at its middle, ...
@group(0) @binding(3) var<storage, read> vel_out: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> out: array<f32>;
// The all-drops engine's drops: place and speed; which way the liquid lies, how full, 0; kind,
// serial, colour, 0.
@group(0) @binding(5) var<storage, read> xv: array<vec4<f32>>;
@group(0) @binding(6) var<storage, read> nf: array<vec4<f32>>;
@group(0) @binding(7) var<storage, read> ids: array<vec4<u32>>;

// Writes drop `i` in today's layout: `c` as the grid core keeps it (how full its neighbourhood is,
// 0, which way the liquid lies), `full` the liquid round it (1 is full).
fn put(i: u32, x: vec2<f32>, v: vec2<f32>, c: vec4<f32>, colour: u32, full_in: f32, kind: u32, serial: u32, e: f32) {
    let at = i * DROP_FLOATS;
    var full = full_in;
    if !(full > 0.0) {
        full = 0.0;
    }
    out[at] = x.x;
    out[at + 1u] = x.y;
    out[at + 2u] = v.x;
    out[at + 3u] = v.y;
    out[at + 4u] = c.x;
    out[at + 5u] = c.y;
    out[at + 6u] = c.z;
    out[at + 7u] = c.w;
    out[at + 8u] = f32(colour & 255u) / 255.0;
    out[at + 9u] = f32((colour >> 8u) & 255u) / 255.0;
    out[at + 10u] = f32((colour >> 16u) & 255u) / 255.0;
    out[at + 11u] = f32((colour >> 24u) & 255u) / 255.0;
    // 1 / the liquid round it.
    out[at + 12u] = 1.0 / clamp(full, 0.05, 1.5);
    out[at + 13u] = bitcast<f32>(kind);
    out[at + 14u] = bitcast<f32>(serial);
    // kick, mat, spare
    out[at + 15u] = 0.0;
    out[at + 16u] = 0.0;
    out[at + 17u] = 0.0;
    out[at + 18u] = 0.0;
    out[at + 19u] = 0.0;
    // its elastic stretch: none
    out[at + 20u] = 1.0;
    out[at + 21u] = 0.0;
    out[at + 22u] = 0.0;
    out[at + 23u] = 1.0;
    out[at + 24u] = e;
    out[at + 25u] = 0.0;
    out[at + 26u] = 0.0;
    out[at + 27u] = 0.0;
}

@compute @workgroup_size(256)
fn cs_bridge(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= br.grid.z {
        return;
    }
    let p = drops[i];
    // The drops' own liquid round it (1 is full), between the four cell middles. Without the
    // solids' share, as today's core keeps it: the picture draws a drop with little liquid round
    // it as a droplet, and a film on a wall is that too.
    let lo = clamp(vec2<i32>(floor(p.x - 0.5)), vec2<i32>(0), vec2<i32>(i32(br.grid.x) - 2, i32(br.grid.y) - 2));
    let f = clamp(p.x - 0.5 - vec2<f32>(lo), vec2<f32>(0.0), vec2<f32>(1.0));
    var full = 0.0;
    for (var a = 0; a < 2; a = a + 1) {
        for (var b = 0; b < 2; b = b + 1) {
            let k = u32(lo.y + b) * br.grid.x + u32(lo.x + a);
            let w = select(1.0 - f.x, f.x, a == 1) * select(1.0 - f.y, f.y, b == 1);
            full = full + w * vel_out[k].z;
        }
    }
    put(i, p.x, p.v, p.c, p.colour, full, p.kind, p.serial, p.e);
}

@compute @workgroup_size(256)
fn cs_bridge_drops(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= br.grid.z {
        return;
    }
    let p = xv[i];
    let n = nf[i];
    let k = ids[i];
    put(i, p.xy, p.zw, vec4<f32>(n.z, 0.0, n.xy), k.z, n.z, k.x, k.y, 0.0);
}

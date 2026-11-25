// The bridge to today's liquid drawing (docs/notes/water.md): each of the water's drops written
// as one of today's (bpe-gfx's liquid_sim.wgsl `Drop`, 28 numbers), so the drops pass and the
// surface pass draw the new core's liquid unchanged.

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
    // columns and rows of nodes, drops, 1 when a drop holds its own fullness (the all-drops trial)
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

@compute @workgroup_size(256)
fn cs_bridge(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= br.grid.z {
        return;
    }
    let p = drops[i];
    let at = i * DROP_FLOATS;
    // The drops' own liquid round it (1 is full), between the four cell middles. Without the
    // solids' share, as today's core keeps it: the picture draws a drop with little liquid round
    // it as a droplet, and a film on a wall is that too.
    let lo = clamp(vec2<i32>(floor(p.x - 0.5)), vec2<i32>(0), vec2<i32>(i32(br.grid.x) - 2, i32(br.grid.y) - 2));
    let f = clamp(p.x - 0.5 - vec2<f32>(lo), vec2<f32>(0.0), vec2<f32>(1.0));
    var full = 0.0;
    var c = p.c;
    if br.grid.w == 1u {
        full = p.c.x;
        c = vec4<f32>(0.0);
    } else {
    for (var a = 0; a < 2; a = a + 1) {
            for (var b = 0; b < 2; b = b + 1) {
                let k = u32(lo.y + b) * br.grid.x + u32(lo.x + a);
                let w = select(1.0 - f.x, f.x, a == 1) * select(1.0 - f.y, f.y, b == 1);
                full = full + w * vel_out[k].z;
            }
        }
    }
    if !(full > 0.0) {
        full = 0.0;
    }
    out[at] = p.x.x;
    out[at + 1u] = p.x.y;
    out[at + 2u] = p.v.x;
    out[at + 3u] = p.v.y;
    out[at + 4u] = c.x;
    out[at + 5u] = c.y;
    out[at + 6u] = c.z;
    out[at + 7u] = c.w;
    out[at + 8u] = f32(p.colour & 255u) / 255.0;
    out[at + 9u] = f32((p.colour >> 8u) & 255u) / 255.0;
    out[at + 10u] = f32((p.colour >> 16u) & 255u) / 255.0;
    out[at + 11u] = f32((p.colour >> 24u) & 255u) / 255.0;
    // 1 / the liquid round it.
    out[at + 12u] = 1.0 / clamp(full, 0.05, 1.5);
    out[at + 13u] = bitcast<f32>(p.kind);
    out[at + 14u] = bitcast<f32>(p.serial);
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
    out[at + 24u] = p.e;
    out[at + 25u] = 0.0;
    out[at + 26u] = 0.0;
    out[at + 27u] = 0.0;
}

// Wet marks (Water mode part 6b; docs/superpowers/plans/water-mode/part6-light-and-looks.md,
// decision 37): each of a layout's nodes holds the water frame liquid first touched it (u32 max:
// never). `cs_wet` marks the four nodes round each liquid drop with the frame its body is at, by
// `atomicMin` of whole numbers, so the field is the same whatever order the drops are in.

struct WetU {
    // the layout's nodes across and down, the drops, the frame to mark
    size: vec4<u32>,
};

@group(0) @binding(0) var<uniform> wu: WetU;
// The drops: place and speed (grid units); kind, serial, colour, 0.
@group(0) @binding(1) var<storage, read> xv: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> ids: array<vec4<u32>>;
@group(0) @binding(3) var<storage, read_write> wet: array<atomic<u32>>;

// The kind bit of trapped air (drop.rs `AIR`): air marks nothing.
const AIR: u32 = 128u;

@compute @workgroup_size(64)
fn cs_wet(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let i = id.x + id.y * groups.x * 64u;
    if i >= wu.size.z || (ids[i].x & AIR) != 0u {
        return;
    }
    let g = xv[i].xy;
    // (A drop that is not a number, or past the layout, marks nothing: by its bits, as a compiler
    // may take a NaN to compare as anything.)
    let e = bitcast<vec2<u32>>(g) & vec2<u32>(0x7f800000u);
    if any(e == vec2<u32>(0x7f800000u)) {
        return;
    }
    let cols = wu.size.x;
    let rows = wu.size.y;
    if cols < 2u || rows < 2u || any(g < vec2<f32>(0.0)) || g.x > f32(cols - 1u) || g.y > f32(rows - 1u) {
        return;
    }
    let x = min(u32(floor(g.x)), cols - 2u);
    let y = min(u32(floor(g.y)), rows - 2u);
    let k = y * cols + x;
    let frame = wu.size.w;
    atomicMin(&wet[k], frame);
    atomicMin(&wet[k + 1u], frame);
    atomicMin(&wet[k + cols], frame);
    atomicMin(&wet[k + cols + 1u], frame);
}

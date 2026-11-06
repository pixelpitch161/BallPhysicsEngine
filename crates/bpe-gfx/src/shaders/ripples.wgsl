// The Ripples field (gradients part 2): for each texel of a picture half the video's size, the
// two nearest Ripples sources of different owners among everything, among the balls and among
// the shapes, found by a jump flood. Pasted after ripple_common.wgsl.
//
// The sources (`RippleSource` in frame.rs) sit in one storage buffer after a head of two vec4s
// (see `ripple_head_*`); field texels hold a source's index + 1, 0 for none:
// A = (every 1, every 2, balls 1, balls 2), B = (shapes 1, shapes 2).

@group(0) @binding(0) var<storage, read> sources: array<vec4<f32>>;
// x: this pass's jump in texels.
@group(0) @binding(1) var<uniform> jump: vec4<u32>;
@group(0) @binding(2) var from_a: texture_2d<u32>;
@group(0) @binding(3) var from_b: texture_2d<u32>;

// Source `i` (1-based).
fn source_at(i: u32) -> RippleSource {
    let k = 2u + (i - 1u) * 3u;
    return RippleSource(sources[k], sources[k + 1u], sources[k + 2u]);
}

struct SeedOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) @interpolate(flat) index: u32,
};

struct FieldOut {
    @location(0) a: vec4<u32>,
    @location(1) b: vec2<u32>,
};

// A quad round source `instance` (its bounding box and a texel and a half more), in the field.
@vertex
fn vs_seed(@builtin(vertex_index) v: u32, @builtin(instance_index) instance: u32) -> SeedOut {
    let s = source_at(instance + 1u);
    let pad = s.b.x + 1.5 * ripple_texel_world(sources[0], sources[1]);
    let lo = min(s.a.xy, s.a.zw) - vec2<f32>(pad);
    let hi = max(s.a.xy, s.a.zw) + vec2<f32>(pad);
    let corner = vec2<f32>(f32(v & 1u), f32((v >> 1u) & 1u));
    let world = mix(lo, hi, corner);
    let screen = ripple_screen(world, sources[0]);
    var out: SeedOut;
    out.pos = vec4<f32>(2.0 * screen.x / sources[1].x, -2.0 * screen.y / sources[1].y, 0.0, 1.0);
    out.index = instance + 1u;
    return out;
}

// The texels inside the source, or within a texel of it, seed it (the pipeline's write masks
// say which of the lists it goes in).
@fragment
fn fs_seed(in: SeedOut) -> FieldOut {
    let world = ripple_world_of(in.pos.xy, sources[0], sources[1]);
    let s = source_at(in.index);
    if ripple_distance(s, world) > 0.75 * ripple_texel_world(sources[0], sources[1]) {
        discard;
    }
    return FieldOut(vec4<u32>(in.index, 0u, in.index, 0u), vec2<u32>(in.index, 0u));
}

struct Best {
    i1: u32,
    i2: u32,
    d1: f32,
    d2: f32,
    o1: f32,
    o2: f32,
};

const RIPPLE_FAR: f32 = 3.0e38;

// Puts source `i` (owner `o`, `d` away) into `b` when it is one of the two nearest owners.
fn take(b: ptr<function, Best>, i: u32, d: f32, o: f32) {
    if i == (*b).i1 || i == (*b).i2 {
        return;
    }
    if (*b).i1 != 0u && o == (*b).o1 {
        if d < (*b).d1 {
            (*b).i1 = i;
            (*b).d1 = d;
        }
        return;
    }
    if (*b).i2 != 0u && o == (*b).o2 {
        if d < (*b).d2 {
            (*b).i2 = i;
            (*b).d2 = d;
            if (*b).d2 < (*b).d1 {
                let was = *b;
                (*b).i1 = was.i2;
                (*b).d1 = was.d2;
                (*b).o1 = was.o2;
                (*b).i2 = was.i1;
                (*b).d2 = was.d1;
                (*b).o2 = was.o1;
            }
        }
        return;
    }
    if d < (*b).d1 {
        (*b).i2 = (*b).i1;
        (*b).d2 = (*b).d1;
        (*b).o2 = (*b).o1;
        (*b).i1 = i;
        (*b).d1 = d;
        (*b).o1 = o;
    } else if d < (*b).d2 {
        (*b).i2 = i;
        (*b).d2 = d;
        (*b).o2 = o;
    }
}

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

// One jump: each texel looks at itself and its eight neighbours `jump.x` texels away and keeps
// the nearest two owners of each list.
@fragment
fn fs_jump(@builtin(position) pos: vec4<f32>) -> FieldOut {
    let size = vec2<i32>(textureDimensions(from_a));
    let at = vec2<i32>(pos.xy);
    let world = ripple_world_of(pos.xy, sources[0], sources[1]);
    let none = Best(0u, 0u, RIPPLE_FAR, RIPPLE_FAR, -1.0, -1.0);
    var every = none;
    var balls = none;
    var shapes = none;
    let step = i32(jump.x);
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let q = at + vec2<i32>(dx, dy) * step;
            if any(q < vec2<i32>(0)) || any(q >= size) {
                continue;
            }
            let a = textureLoad(from_a, q, 0);
            let b = textureLoad(from_b, q, 0);
            let found = array<u32, 6>(a.x, a.y, a.z, a.w, b.x, b.y);
            for (var k = 0; k < 6; k++) {
                let i = found[k];
                if i == 0u {
                    continue;
                }
                let s = source_at(i);
                let d = ripple_distance(s, world);
                take(&every, i, d, s.b.y);
                if s.b.z < 0.5 {
                    take(&balls, i, d, s.b.y);
                } else {
                    take(&shapes, i, d, s.b.y);
                }
            }
        }
    }
    return FieldOut(vec4<u32>(every.i1, every.i2, balls.i1, balls.i2), vec2<u32>(shapes.i1, shapes.i2));
}

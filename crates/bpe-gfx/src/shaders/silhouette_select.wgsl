// Things masks' silhouettes (colour grading part 5, renderer/silhouettes.rs): which things this
// silhouette chooses. Pasted after a shader's own code with SIL_GROUP replaced by its group.

struct SilSelect {
    // The first code in sil_codes, how many there are, 1 for every thing, 0.
    range: vec4<u32>,
};

@group(SIL_GROUP) @binding(0) var<uniform> sil_select: SilSelect;
// The chosen things' codes (thing_code, whole numbers), each choice's sorted.
@group(SIL_GROUP) @binding(1) var<storage, read> sil_codes: array<u32>;

// Whether the thing with `code` is chosen.
fn sil_picked(code: u32) -> bool {
    if sil_select.range.z != 0u {
        return true;
    }
    var lo = sil_select.range.x;
    var hi = lo + sil_select.range.y;
    while lo < hi {
        let mid = (lo + hi) / 2u;
        let c = sil_codes[mid];
        if c == code {
            return true;
        }
        if c < code {
            lo = mid + 1u;
        } else {
            hi = mid;
        }
    }
    return false;
}

// What a thing's body (code 0: not a thing's body, left out) covering `cover` here adds, laid over
// what is under it: a chosen thing its coverage in red, any thing its coverage in alpha.
fn sil_out(code: u32, cover: f32) -> vec4<f32> {
    let a = clamp(cover, 0.0, 1.0);
    if code == 0u || a <= 0.0 {
        discard;
    }
    return vec4<f32>(select(0.0, a, sil_picked(code)), 0.0, 0.0, a);
}


// ---- a kept moment packed (part 8) ---------------------------------------------------------------

// Each drop in 12 bytes, for the final's take: its cell (the layout's, `floor` of its place) and
// its kind's low byte; its place in the cell (x, y: 1/256 of a cell each) and its speed (x, y: a
// signed byte each on a log scale, pixels a second); its colour mix (mix.wgsl). A drop not in the
// course is all ones.
@group(0) @binding(29) var<storage, read_write> packed: array<u32>;

// A speed (pixels a second) as a signed byte: round(15 ln(1 + |v| / 4)), at most 127 (about 19,000
// px/s), its sign kept (take/frame.rs `speed_byte` matches it).
fn speed_byte(v: f32) -> u32 {
    if !(abs(v) < 1e30) {
        return 0u;
    }
    let m = i32(min(round(15.0 * log(1.0 + abs(v) / 4.0)), 127.0));
    return u32(select(m, -m, v < 0.0)) & 255u;
}

// The first `st.n.x` drops, the kinds as the step sorted them (binding 5).
@compute @workgroup_size(256)
fn cs_pack(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= st.n.x {
        return;
    }
    let p = xv[i];
    if !alive(p.xy) {
        packed[3u * i] = 0xFFFFFFFFu;
        packed[3u * i + 1u] = 0xFFFFFFFFu;
        packed[3u * i + 2u] = 0xFFFFFFFFu;
        return;
    }
    let most = vec2<f32>(fr.grid.xy) - vec2<f32>(2.0);
    let c = clamp(floor(p.xy), vec2<f32>(0.0), most);
    let q = vec2<u32>(clamp((p.xy - c) * 256.0, vec2<f32>(0.0), vec2<f32>(255.0)));
    let cell = u32(c.x) + u32(c.y) * (fr.grid.x - 1u);
    let v = p.zw * fr.s.x;
    let id = id_nxt[i];
    let kind = id.x & 255u;
    packed[3u * i] = (cell & 0xFFFFFFu) | (kind << 24u);
    packed[3u * i + 1u] = q.x | (q.y << 8u) | (speed_byte(v.x) << 16u) | (speed_byte(v.y) << 24u);
    packed[3u * i + 2u] = select(id.w, 0u, (kind & AIR) != 0u);
}

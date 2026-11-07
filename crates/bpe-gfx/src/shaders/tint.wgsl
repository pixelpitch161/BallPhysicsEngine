// Glass and light part 1: the tint picture (light pass 8). Every thing that blocks light writes
// how deep it is, per channel, as optical depth (-ln of the share it lets through), with alpha 0:
// the pipelines blend premultiplied "over", so depths add and what passes multiplies, red glass
// over blue glass letting through only what both do. lighting.wgsl's `fs_field` reads it into the
// blockers' field.

const TINT_PASS: f32 = 8.0;
// How much deeper the colour is where a thing is thickest at Deeper when thick 1 (the colour's
// depth times 1 + THICK x thickness): lighting.wgsl's THICK_DEPTH.
const THICK: f32 = 2.0;

// Whether this is the tint pass.
fn tint_pass() -> bool {
    return g.light.x > 7.5;
}

// A colour's hue and saturation at full brightness (its brightest channel 1): how a see-through
// thing of that colour tints light, whatever its brightness. Black tints nothing.
fn tint_chroma(c: vec3<f32>) -> vec3<f32> {
    let most = max(c.r, max(c.g, c.b));
    if most < 1e-4 {
        return vec3<f32>(1.0);
    }
    return clamp(c / most, vec3<f32>(0.0), vec3<f32>(1.0));
}

// What a pixel writes to the tint picture: `colour` the thing's colour there (straight), `presence`
// how much of it is there (its opacity, its antialiased edge divided out), `blocks` how much it
// blocks light (1 less its See-through), `tints` its Tints light and `thick` Deeper when thick times
// how thick it is there (0 to 1). Nothing where it doesn't block light.
fn tint_out(colour: vec3<f32>, presence: f32, blocks: f32, tints: f32, thick: f32) -> vec4<f32> {
    let p = clamp(presence, 0.0, 1.0);
    let b = clamp(blocks, 0.0, 1.0);
    if b <= 0.0 || p <= 0.0 {
        return vec4<f32>(0.0);
    }
    let share = max(1.0 - b * p, 1e-6);
    let through = max(mix(vec3<f32>(1.0), tint_chroma(max(colour, vec3<f32>(0.0))), clamp(tints * p, 0.0, 1.0)), vec3<f32>(1.0 / 1024.0));
    let depth = -log(through) * (1.0 + THICK * max(thick, 0.0));
    return vec4<f32>(vec3<f32>(-log(share)) + depth, 0.0);
}

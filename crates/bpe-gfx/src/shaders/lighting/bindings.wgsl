// Lighting (Flat and Stage), run only when the scene has a Lighting card (see lighting.rs).
//
// The things were drawn three times before this runs: the picture as always, the lit colour
// (every layer's colour scaled by its Lit) and the surface (R: blocks light, G: height in world
// units, both times cover, A: cover). Here:
// 1. `fs_seed` marks, at the field's size, which texels block light;
// 2. `fs_jump` runs the jump flood, keeping the nearest blocking texel (xy) and the nearest open
//    one (zw) of every texel;
// 3. `fs_field` turns them into a signed distance in world units (negative inside a blocker);
// 4. `fs_direct` adds up the light from Light things at each pixel, each light's shadow marched
//    through the field, on the surface and in the air (for haze);
// 5. with things that give off light (or Bounce light), the light field: `fs_gi_scene` gathers
//    what gives light (and what lit things bounce) and what is opaque, a second jump flood
//    (`fs_seed_gi`, `fs_gi_dist`) how far each texel is from something opaque, `fs_cascade` the
//    radiance cascades from the farthest in, and `fs_gi_resolve` the light at each texel (its
//    mean, and its mean times the cosine and sine of where it comes from);
// 6. with Light blooms, `fs_glow_source` (the lamps' bulbs and what gives off light) and
//    `fs_glow_down` make the bloom's levels;
// 7. `fs_mix` mixes it in: colour + (light - 1) x lit colour, then the haze and the bloom; a
//    surface that shines, and anything on a Stage (part 2a), is shaded there by `surfaced`;
// 8. a Glossy or Mirror floor on a Stage mirrors that picture (`fs_floor`).
//
// Part 4 adds, each only when used: `vs_stir`/`fs_stir` (balls stirring moving haze), `fs_extras`
// (the moving haze's density and the remembered light, glow in the dark), gobos, flicker (on the
// CPU), Lights up and ultraviolet light in `lamps()`, and in the mix the haze's density, smoke
// that catches beams, underwater light, UV glow, glow in the dark and laser beams.

struct Lighting {
    // 2 / output width, 2 / output height in screen units, world units per output pixel, time.
    view: vec4<f32>,
    // The camera: centre xy, zoom, turn.
    camera: vec4<f32>,
    // Ambient light (rgb, already times its strength), w what is shown (0 the lit picture, 1 the
    // light alone, 2 the light as a heat map: the editor's views).
    ambient: vec4<f32>,
    // Shadow style (0 soft, 1 hard, 2 toon, 3 off), softness, length, contact shading.
    shadows: vec4<f32>,
    // Contact reach, toon steps, march steps, light count.
    more: vec4<f32>,
    // The field's size in texels, world units per field texel, jump step (fs_jump).
    field: vec4<f32>,
    // Lighting part 3: this sample's index, how many, 1 when baking, light bounces.
    sample: vec4<f32>,
    // The bake's path-traced bounce: Bounce light, 1 on a Stage (whose balls mark the surface
    // with a quarter, kept out of the blockers' field), 1 when light through see-through things
    // takes their colour, 1 when the tint picture was drawn (glass and light part 1: the field
    // pass then reads it in `picture`'s place).
    trace: vec4<f32>,
    // Glass and light part 3: 1 when glass bends lamp light (the photon pass runs); part 4b: y how
    // many entries `dichroic` has; two spare.
    bent: vec4<f32>,
    // Review S19, the lights each tile of the picture lists (`lights_near`): tiles across and down per
    // unit of uv (the pictures' size over the tile's), how many tiles across and down (0 across: no
    // lists, every pixel takes every light).
    tiles: vec4<f32>,
    // Where the tiles' heads start in `links` (two numbers a tile: where its list starts in `links`
    // and how long it is); three spare.
    tile_list: vec4<f32>,
    // Which surface indices are glass that bends lamp light: bit i of word i / 32 (2048 bits).
    bends: array<vec4<u32>, 16>,
    // Glass and light part 4b: dichroic surfaces, [index, Dichroic, shift, hue] (for the shadows).
    dichroic: array<vec4<f32>, 16>,
};

struct LightGpu {
    // Place xy (world), height above the picture, kind (0 point, 1 spot, 2 sun, 3 area, 4 strip).
    p0: vec4<f32>,
    // Colour times strength (linear rgb), w unused.
    p1: vec4<f32>,
    // Reach, falloff (0 real, 1 soft, 2 linear, 3 none), size, shadow strength.
    p2: vec4<f32>,
    // Direction xy (where the light points), cos of the cone's inner and outer edge.
    p3: vec4<f32>,
    // Width, length (area), first segment, segment count (strip).
    p4: vec4<f32>,
    // Shadow colour (rgb), casts shadows (1 or 0).
    p5: vec4<f32>,
    // How far short of the lamp shadows stop (a ridden ball's radius), the sun's angular size
    // (tan), area shape (0 rectangle, 1 disc), the light's shadow softness.
    p6: vec4<f32>,
    // How much its light shows in the haze, 1 when ultraviolet, an ultraviolet light's visible
    // share, a laser's glow.
    p7: vec4<f32>,
    // Gobo: kind, size, sharpness, depth.
    p8: vec4<f32>,
    // The gobo's turn (cos, sin), its drift (xy, world units).
    p9: vec4<f32>,
    // Lights up: mode (0 everything, 1 only, 2 all but), first link, how many, spare.
    p10: vec4<f32>,
    // A laser's width, its hit glow, 1 when its beam ends on something, spare.
    p11: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Lighting;
@group(0) @binding(1) var picture: texture_2d<f32>;
// The lit share of the picture; in the depth of field pass, which never reads that, its quarter-size
// copy of the picture it blurs (`fs_dof_small`).
@group(0) @binding(2) var lit_picture: texture_2d<f32>;
@group(0) @binding(3) var surface: texture_2d<f32>;
@group(0) @binding(4) var field: texture_2d<f32>;
@group(0) @binding(5) var samp: sampler;
@group(0) @binding(6) var<storage, read> lights: array<LightGpu>;
@group(0) @binding(7) var<storage, read> segments: array<vec4<f32>>;
// The jump flood's input (fs_jump, fs_field).
@group(0) @binding(8) var seeds: texture_2d<f32>;
// The link numbers the lights' Lights up name (part 4).
@group(0) @binding(9) var<storage, read> links: array<f32>;

struct FsIn {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> FsIn {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: FsIn;
    out.pos = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(uv.x, 1.0 - uv.y);
    return out;
}


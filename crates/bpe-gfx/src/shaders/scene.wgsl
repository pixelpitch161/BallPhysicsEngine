// Scene passes, all drawn into the HDR target in linear light.
// World coordinates are video pixels, origin at the frame centre, y down.

struct Globals {
    // x: 2 / video width, y: 2 / video height, z: world units per output pixel, w: time
    view: vec4<f32>,
    // The background paint: atlas row, start, scroll, repeat.
    bg_paint: vec4<f32>,
    // The background place: direction, span, centre xy.
    bg_place: vec4<f32>,
    // x: bloom strength, y: exposure
    post: vec4<f32>,
    // centre xy, zoom, turn
    camera: vec4<f32>,
    // x: background on screen, y: how its place runs, z: grain, w: 1 to mix the two colours below
    bg: vec4<f32>,
    bg_from: vec4<f32>,
    bg_to: vec4<f32>,
    grade: vec4<f32>,
    grade_more: vec4<f32>,
    // Lighting (see the lighting shader, `shaders/lighting/`; light_passes.wgsl names the passes): x 0 draws the
    // picture as always, 1 each thing's lit colour,
    // 2 its surface, 3 the light it gives off, 4 the light the background gives off alone; y how
    // much the background takes light.
    light: vec4<f32>,
    // The background's own light (mode 4): its own colour times its strength, then what its
    // colours are multiplied by.
    light_emit: array<vec4<f32>, 2>,
    // The background's form (with bg.y 3): its own numbers, then its code.
    bg_form: vec4<f32>,
    bg_shape: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;

// What a thing drawn without layers writes, premultiplied `c` in hand: in the light passes its
// colour times `lit` (mode 1), or how much it blocks light, its height (flat: 0) and its cover
// (mode 2), or the light `emit` (straight) it gives off where it covers (mode 3); `c` itself
// otherwise.
fn light_flat(c: vec4<f32>, lit: f32, blocks: f32, emit: vec3<f32>, extra: vec4<f32>) -> vec4<f32> {
    let m = light_pass(g.light.x);
    // Light pass 8, the tint picture (tint.wgsl): its colour's depth where it blocks light, its
    // antialiased edge divided out (as `hard_block`).
    if m >= PASS_TINT {
        if extra.y > 0.5 || c.a <= 1e-5 {
            return vec4<f32>(0.0);
        }
        let p = select(c.a, c.a / AA_COVER, AA_COVER >= HARD_FROM);
        return tint_out(c.rgb / c.a, min(p, 1.0), blocks, 1.0, 0.0);
    }
    // Light passes 5 (what glows under UV) and 6 (what catches beams, link numbers).
    if m >= PASS_UV {
        return light_extra(c, extra);
    }
    // Shown only under UV light: nothing in the picture or the other light passes.
    if extra.y > 0.5 {
        return vec4<f32>(0.0);
    }
    if m <= PASS_PICTURE {
        return c;
    }
    if m <= PASS_LIT {
        return vec4<f32>(c.rgb * lit, c.a);
    }
    if m >= PASS_EMISSION {
        // The light it gives off, where it covers (light covering nothing by how bright it is).
        let k = select(c.a, max(c.r, max(c.g, c.b)), c.a <= 0.0);
        return vec4<f32>(emit * k, c.a);
    }
    // The surface: only what blocks covers it, so a spark or a trail passing over a ball leaves
    // the ball's blocking and height under it as they are. R is how much it blocks by how
    // see-through it is, its antialiased edge taken whole (`hard_block`).
    let a = clamp(blocks, 0.0, 1.0) * clamp(c.a, 0.0, 1.0);
    return vec4<f32>(hard_block(a, AA_COVER), 0.0, 0.0, a);
}

// How much of the pixel a thing drawn without layers covers by its antialiased edge alone (not
// by how see-through it is), when it says (a trail does; -1 when unknown): what the surface
// pass's `hard_block` divides out.
var<private> AA_COVER: f32 = -1.0;

// The least antialiased cover at which the surface pass (lighting's mode 2) takes a blocker
// whole: the lighting shader's BLOCK_LEAST, below which a pixel doesn't block.
const HARD_FROM: f32 = 0.1;

// The surface picture's R in Flat (the lighting shader): how much the things over the pixel block,
// by how see-through they are (`blocks` premultiplied by `cover`, the antialiased edge alone,
// which is divided out where it is at least HARD_FROM), so a pixel half covered by a solid line
// reads 1 and one under a half see-through wall 0.5. Below HARD_FROM it stays cover-weighted, so
// which pixels block is as before. A Stage keeps cover-weighted R throughout.
fn hard_block(blocks: f32, cover: f32) -> f32 {
    if g.light.z > 0.5 || cover < HARD_FROM {
        return blocks;
    }
    return blocks / cover;
}

// Light pass 5: what glows under UV light, its colours times its UV glow (black, still covering
// what is behind, when it doesn't glow). Pass 6: how much it catches beams and its thing's link
// number, both times its cover.
fn light_extra(c: vec4<f32>, extra: vec4<f32>) -> vec4<f32> {
    if light_pass(g.light.x) <= PASS_UV {
        return vec4<f32>(c.rgb * max(extra.x, 0.0), c.a);
    }
    // Shown only under UV light: it neither catches beams nor takes a link number.
    if extra.y > 0.5 {
        return vec4<f32>(0.0);
    }
    let a = clamp(c.a, 0.0, 1.0);
    return hard_tags(vec4<f32>(tag_catch(extra.z) * a, extra.w * a, tag_rain(extra.z) * a, a));
}

// A trail's or particle's `extra.z` (glass and light part 4b, frame.rs's `pack_tags`): Catches
// beams, and how much it makes rainbows, packed as a negative whole number when it does.
fn tag_catch(z: f32) -> f32 {
    if z >= 0.0 {
        return z;
    }
    return floor(-z / 256.0) / 64.0;
}

fn tag_rain(z: f32) -> f32 {
    if z >= 0.0 {
        return 0.0;
    }
    let v = -z;
    return (v - 256.0 * floor(v / 256.0)) / 255.0;
}

// The tags pass's output (`t` premultiplied by cover): what catches beams adds up softly, while
// the link number is written whole where the thing covers at least half the pixel and not at all
// elsewhere, so two things' numbers never mix at an edge. Other passes pass `t` through.
fn hard_tags(t: vec4<f32>) -> vec4<f32> {
    if light_pass(g.light.x) != PASS_TAGS {
        return t;
    }
    // B: how much it makes rainbows (part 4b), soft like the beams.
    if t.a < 0.5 {
        return vec4<f32>(t.r, 0.0, t.b, 0.0);
    }
    return vec4<f32>(t.r, t.g / t.a, t.b, 1.0);
}

// The camera (centre xy, zoom, turn) turns a world point into screen units; with no camera
// (centre 0, zoom 1, turn 0) the point is unchanged, exactly.
fn to_screen(p: vec2<f32>) -> vec2<f32> {
    let c = g.camera;
    var d = p - c.xy;
    if c.w != 0.0 {
        let s = sin(-c.w);
        let k = cos(-c.w);
        d = vec2<f32>(d.x * k - d.y * s, d.x * s + d.y * k);
    }
    return d * c.z;
}

fn to_clip(p: vec2<f32>) -> vec4<f32> {
    let q = to_screen(p);
    return vec4<f32>(q.x * g.view.x, -q.y * g.view.y, 0.0, 1.0);
}

fn quad_corner(i: u32) -> vec2<f32> {
    return vec2<f32>(f32(i & 1u), f32((i >> 1u) & 1u)) * 2.0 - 1.0;
}

// ---- Background gradient (fullscreen) ----

struct BgOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec2<f32>,
};

@vertex
fn vs_background(@builtin(vertex_index) i: u32) -> BgOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    let ndc = uv * 2.0 - 1.0;
    var out: BgOut;
    out.pos = vec4<f32>(ndc, 0.0, 1.0);
    out.world = vec2<f32>(ndc.x / g.view.x, -ndc.y / g.view.y);
    return out;
}

// The world point under a screen point: `to_screen` backwards.
fn screen_to_world(q: vec2<f32>) -> vec2<f32> {
    let c = g.camera;
    var d = q / c.z;
    if c.w != 0.0 {
        let s = sin(c.w);
        let k = cos(c.w);
        d = vec2<f32>(d.x * k - d.y * s, d.x * s + d.y * k);
    }
    return d + c.xy;
}

@fragment
fn fs_background(in: BgOut) -> @location(0) vec4<f32> {
    let c = background_colour(in);
    if light_pass(g.light.x) == PASS_BACKGROUND {
        return vec4<f32>(g.light_emit[0].rgb + g.light_emit[1].rgb * c.rgb, 1.0);
    }
    if light_pass(g.light.x) == PASS_EMISSION {
        // The things' light: the background covers nothing there (its own is drawn alone).
        return vec4<f32>(0.0);
    }
    // In the tags pass the background carries the scene's link number (Globals.light.w).
    return light_flat(c, g.light.y, 0.0, vec3<f32>(0.0), vec4<f32>(0.0, 0.0, 0.0, g.light.w));
}

fn background_colour(in: BgOut) -> vec4<f32> {
    // For the gradient grain (paint.wgsl).
    paint_px = in.pos.xy;
    if g.bg_paint.x < 0.0 {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    // `in.world` is in screen units here; the place is measured there, or in the world.
    var along = 0.0;
    if g.bg.y > 0.5 {
        var at = in.world;
        if g.bg.x == 0.0 {
            at = screen_to_world(in.world);
        }
        let d = at - g.bg_place.zw;
        let span = max(g.bg_place.y, 0.001);
        if g.bg.y > 2.5 {
            // Any other form: the place in spans from the centre, the form turning it.
            let p = PaintGpu(
                g.bg_paint.x, g.bg_paint.y, g.bg_shape.x, g.bg.z, g.bg_place.x, 1.0, g.bg_paint.z, g.bg_paint.w,
                PAINT_NO_VARY, g.bg_form, vec4<f32>(0.0),
            );
            // Ripples look the field up in the world, wherever the place is measured.
            paint_world = screen_to_world(in.world);
            return vec4<f32>(paint_colour(p, d / span, g.view.w, vec4<f32>(0.0, 0.0, 0.0, 1.0)).rgb, 1.0);
        }
        if g.bg.y < 1.5 {
            let a = radians(g.bg_place.x);
            along = dot(d, vec2<f32>(cos(a), sin(a))) / span + 0.5;
        } else {
            along = length(d) / span;
        }
    }
    if g.bg.w > 0.5 {
        // A plain blend of two colours, mixed here without the atlas's half floats.
        let t = paint_wrap(along + g.bg_paint.y + g.view.w * g.bg_paint.z, g.bg_paint.w);
        return vec4<f32>(paint_grain(vec4<f32>(mix(g.bg_from.rgb, g.bg_to.rgb, t), 1.0), g.bg.z).rgb, 1.0);
    }
    let p = PaintGpu(
        g.bg_paint.x, g.bg_paint.y, 0.0, g.bg.z, 0.0, 1.0, g.bg_paint.z, g.bg_paint.w, PAINT_NO_VARY, vec4<f32>(0.0),
        vec4<f32>(0.0),
    );
    return vec4<f32>(paint_along(p, along, g.view.w).rgb, 1.0);
}

// ---- Shared helpers ----

const PI: f32 = 3.14159265;
const TAU: f32 = 6.28318531;

// How much a pixel hides what is behind it. A negative colour alpha is an additive colour: it
// adds its light and hides nothing.
fn covering(cover: f32, color_alpha: f32) -> f32 {
    return select(cover, 0.0, color_alpha < 0.0);
}

fn rotate(p: vec2<f32>, angle: f32) -> vec2<f32> {
    let c = cos(angle);
    let s = sin(angle);
    return vec2<f32>(c * p.x - s * p.y, s * p.x + c * p.y);
}

fn segment_distance(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let ab = b - a;
    let t = clamp(dot(p - a, ab) / max(dot(ab, ab), 1e-8), 0.0, 1.0);
    return length(p - a - ab * t);
}

// ---- Balls ----

// Shiny sphere lighting for a point `p` on a unit disc, lit from the upper left.
fn sphere_light(p: vec2<f32>, color: vec3<f32>) -> vec3<f32> {
    let r = length(p);
    let n = vec3<f32>(p, sqrt(max(1.0 - r * r, 0.0)));
    // Light from the upper left (y is down in world space).
    let l = normalize(vec3<f32>(-0.45, -0.55, 0.7));
    let diffuse = 0.45 + 0.8 * max(dot(n, l), 0.0);
    let h = normalize(l + vec3<f32>(0.0, 0.0, 1.0));
    let spec = pow(max(dot(n, h), 0.0), 24.0) * 0.7;
    return color * diffuse + vec3<f32>(spec);
}

// ---- Flashes (instanced additive glows, strength in the colour) ----

struct FlashIn {
    @location(0) center: vec2<f32>,
    @location(1) size: f32,
    @location(2) color: vec4<f32>,
};

struct FlashOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_flash(@builtin(vertex_index) i: u32, f: FlashIn) -> FlashOut {
    let corner = quad_corner(i);
    var out: FlashOut;
    out.pos = to_clip(f.center + corner * f.size);
    out.local = corner;
    out.color = f.color;
    return out;
}

// What `fs_flash` and `fs_flash_each` draw.
fn shade_flash(in: FlashOut) -> vec4<f32> {
    let r2 = dot(in.local, in.local);
    let v = exp(-r2 * 5.0) * (1.0 - smoothstep(0.8, 1.0, sqrt(r2)));
    return vec4<f32>(in.color.rgb * v, 0.0);
}

@fragment
fn fs_flash(in: FlashOut) -> @location(0) vec4<f32> {
    return light_flat(shade_flash(in), 0.0, 0.0, vec3<f32>(0.0), vec4<f32>(0.0));
}

// In a layer whose things blend with each other (see each.wgsl).
@fragment
fn fs_flash_each(in: FlashOut) -> @location(0) vec4<f32> {
    return each_out(shade_flash(in), g.post.w);
}

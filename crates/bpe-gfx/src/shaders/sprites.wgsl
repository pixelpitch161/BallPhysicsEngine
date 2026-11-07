// Particles and the round bits of trails (dots, smoke puffs), one quad per instance. The CPU
// works out where each one is and how it looks (see `Particle` in frame.rs); this only draws it.

struct Globals {
    // x: 2 / video width, y: 2 / video height, z: world units per output pixel, w: time
    view: vec4<f32>,
    bg_paint: vec4<f32>,
    bg_place: vec4<f32>,
    post: vec4<f32>,
    // centre xy, zoom, turn
    camera: vec4<f32>,
    bg: vec4<f32>,
    bg_from: vec4<f32>,
    bg_to: vec4<f32>,
    grade: vec4<f32>,
    grade_more: vec4<f32>,
    // Lighting (see lighting.wgsl): x 0 draws the picture as always, 1 each thing's lit colour,
    // 2 its surface; y how much the background takes light.
    light: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;

// As scene.wgsl's: a sprite in the light passes (see lighting.wgsl).
fn light_flat(c: vec4<f32>, lit: f32, blocks: f32, emit: vec3<f32>, extra: vec4<f32>) -> vec4<f32> {
    let m = g.light.x;
    // Light passes 5 (what glows under UV) and 6 (what catches beams, link numbers).
    if m > 4.5 {
        return light_extra(c, extra);
    }
    // Shown only under UV light: nothing in the picture or the other light passes.
    if extra.y > 0.5 {
        return vec4<f32>(0.0);
    }
    if m < 0.5 {
        return c;
    }
    if m < 1.5 {
        return vec4<f32>(c.rgb * lit, c.a);
    }
    if m > 2.5 {
        // The light it gives off, where it covers (light covering nothing by how bright it is).
        let k = select(c.a, max(c.r, max(c.g, c.b)), c.a <= 0.0);
        return vec4<f32>(emit * k, c.a);
    }
    // The surface: only what blocks covers it, so a spark or a trail passing over a ball leaves
    // the ball's blocking and height under it as they are.
    let a = clamp(blocks, 0.0, 1.0) * clamp(c.a, 0.0, 1.0);
    return vec4<f32>(a, 0.0, 0.0, a);
}

// Light pass 5: what glows under UV light, its colours times its UV glow (black, still covering
// what is behind, when it doesn't glow). Pass 6: how much it catches beams and its thing's link
// number, both times its cover.
fn light_extra(c: vec4<f32>, extra: vec4<f32>) -> vec4<f32> {
    if g.light.x < 5.5 {
        return vec4<f32>(c.rgb * max(extra.x, 0.0), c.a);
    }
    if extra.y > 0.5 {
        return vec4<f32>(0.0);
    }
    let a = clamp(c.a, 0.0, 1.0);
    // As scene.wgsl's hard_tags: beams soft, the link number whole where it covers half or more.
    if a < 0.5 {
        return vec4<f32>(max(extra.z, 0.0) * a, 0.0, 0.0, 0.0);
    }
    return vec4<f32>(max(extra.z, 0.0) * a, extra.w, 0.0, 1.0);
}

const SPARKS: u32 = 0u;
const SMOKE: u32 = 1u;
const CONFETTI: u32 = 2u;
const STARS: u32 = 3u;
const SHARDS: u32 = 4u;
const RINGS: u32 = 5u;
const DOTS: u32 = 6u;
const HEARTS: u32 = 7u;
const BUBBLES: u32 = 8u;
const STREAKS: u32 = 9u;
// Particles part 5's shapes: long ones reach `style.x` along their turn, as streaks do.
const LEAF: u32 = 10u;
const PETAL: u32 = 11u;
const FLAKE: u32 = 12u;
const NEEDLE: u32 = 13u;
const BIRD: u32 = 14u;
const FISH: u32 = 15u;
const DIAMOND: u32 = 16u;

struct SpriteIn {
    // centre xy (world pixels), turn (radians), half size (pixels)
    @location(0) place: vec4<f32>,
    // linear light; a negative alpha adds |alpha| of the light and covers nothing
    @location(1) color: vec4<f32>,
    // x: half length along the turn, y: shade, z: softness 0..1, w: band 0..1
    @location(2) style: vec4<f32>,
    @location(3) shape: vec4<u32>,
    // Its layer's Lit and how much it blocks light, read in the light passes.
    @location(4) light: vec4<f32>,
    // The light it gives off (straight, linear), read in the emission pass.
    @location(5) emit: vec4<f32>,
    // UV glow, only under UV, catches beams, link number (lighting part 4).
    @location(6) extra: vec4<f32>,
};

struct SpriteOut {
    @builtin(position) pos: vec4<f32>,
    // Where in the sprite, in pixels from its centre along its turn.
    @location(0) at: vec2<f32>,
    @location(1) color: vec4<f32>,
    // half length, half size, softness, band
    @location(2) size: vec4<f32>,
    @location(3) @interpolate(flat) kind: u32,
    @location(4) shade: f32,
    // A bird's flap or a fish's swing, -1 to 1 (`shape.y`, 0 to 65535).
    @location(8) @interpolate(flat) pose: f32,
    @location(5) @interpolate(flat) light: vec2<f32>,
    @location(6) @interpolate(flat) emit: vec3<f32>,
    @location(7) @interpolate(flat) extra: vec4<f32>,
};

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

fn rotate(p: vec2<f32>, angle: f32) -> vec2<f32> {
    let c = cos(angle);
    let s = sin(angle);
    return vec2<f32>(c * p.x - s * p.y, s * p.x + c * p.y);
}

@vertex
fn vs_sprite(@builtin(vertex_index) vi: u32, v: SpriteIn) -> SpriteOut {
    var out: SpriteOut;
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u)) * 2.0 - 1.0;
    let px = g.view.z;
    let size = max(v.place.w, 0.0);
    var reach = size;
    if v.shape.x == SPARKS || v.shape.x == STREAKS || v.shape.x == CONFETTI || v.shape.x == SHARDS || v.shape.x >= LEAF {
        reach = max(v.style.x, size);
    }
    // Room for the soft edge and a pixel either side.
    var margin = 2.0 * px + v.style.z * size;
    if v.shape.x == RINGS {
        margin = margin + max(1.2, 0.75 * px) + 2.0;
    }
    var across = size;
    if v.shape.x == BIRD {
        // Its wing tips rise and fall by as much as half its span.
        across = max(size, reach * 0.7);
    }
    let half = vec2<f32>(reach, across) + vec2<f32>(margin);
    let at = corner * half;
    out.pos = to_clip(v.place.xy + rotate(at, v.place.z));
    out.at = at;
    out.color = v.color;
    out.size = vec4<f32>(reach, size, clamp(v.style.z, 0.0, 1.0), clamp(v.style.w, 0.0, 1.0));
    out.kind = v.shape.x;
    out.light = v.light.xy;
    out.emit = v.emit.rgb;
    out.extra = v.extra;
    out.shade = v.style.y;
    out.pose = f32(v.shape.y) / 65535.0 * 2.0 - 1.0;
    return out;
}

// The distance from `p` to the edge of a heart of size `s` (pixels), negative inside.
fn heart(p0: vec2<f32>, s: f32) -> f32 {
    // A heart one unit tall, its point down (y down on screen).
    var p = vec2<f32>(abs(p0.x), -p0.y) / max(s, 1e-4) * 0.9 + vec2<f32>(0.0, 0.55);
    var d: f32;
    if p.y + p.x > 1.0 {
        d = length(p - vec2<f32>(0.25, 0.75)) - 0.3535534;
    } else {
        let a = dot(p - vec2<f32>(0.0, 1.0), p - vec2<f32>(0.0, 1.0));
        let m = 0.5 * max(p.x + p.y, 0.0);
        let b = dot(p - vec2<f32>(m, m), p - vec2<f32>(m, m));
        d = sqrt(min(a, b)) * sign(p.x - p.y);
    }
    return d * s / 0.9;
}

// The distance from `p` to the segment from `a` to `b`.
fn segment(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-8), 0.0, 1.0);
    return length(pa - ba * h);
}

// A shape whose half width along x (-reach to reach) is `w`: the distance to its edge, roughly.
fn widthwise(p: vec2<f32>, reach: f32, w: f32, slope: f32) -> f32 {
    let r = max(reach, 1e-4);
    if abs(p.x) >= r {
        return length(vec2<f32>(abs(p.x) - r, p.y));
    }
    return (abs(p.y) - w) / sqrt(1.0 + slope * slope);
}

// The distance to the edge of a triangle a, b, c (negative inside).
fn triangle(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>, c: vec2<f32>) -> f32 {
    let e0 = b - a;
    let e1 = c - b;
    let e2 = a - c;
    let v0 = p - a;
    let v1 = p - b;
    let v2 = p - c;
    let pq0 = v0 - e0 * clamp(dot(v0, e0) / dot(e0, e0), 0.0, 1.0);
    let pq1 = v1 - e1 * clamp(dot(v1, e1) / dot(e1, e1), 0.0, 1.0);
    let pq2 = v2 - e2 * clamp(dot(v2, e2) / dot(e2, e2), 0.0, 1.0);
    let s = sign(e0.x * e2.y - e0.y * e2.x);
    let d = min(
        min(vec2<f32>(dot(pq0, pq0), s * (v0.x * e0.y - v0.y * e0.x)), vec2<f32>(dot(pq1, pq1), s * (v1.x * e1.y - v1.y * e1.x))),
        vec2<f32>(dot(pq2, pq2), s * (v2.x * e2.y - v2.y * e2.x))
    );
    return -sqrt(d.x) * sign(d.y);
}

// The distance to the edge of one of part 5's shapes, in pixels, negative inside.
fn more_shape(kind: u32, p: vec2<f32>, reach: f32, size: f32, pose: f32) -> f32 {
    let r = max(reach, 1e-4);
    let lx = clamp(p.x / r, -1.0, 1.0);
    switch kind {
        case LEAF: {
            // Pointed at both ends: its width a parabola along it.
            let w = size * (1.0 - lx * lx);
            return widthwise(p, r, w, 2.0 * size * lx / r);
        }
        case PETAL: {
            // Narrow at its base (behind), round at the front with a notch in it.
            let w = size * sqrt(max(1.0 - lx * lx, 0.0)) * (0.35 + 0.65 * (lx * 0.5 + 0.5));
            let body = widthwise(p, r, w, size / r);
            let notch = size * 0.3 - length(p - vec2<f32>(r, 0.0));
            return max(body, notch);
        }
        case FLAKE: {
            // Six arms with a pair of branches on each, folded into one twelfth of a turn.
            var a = atan2(p.y, p.x);
            let sector = 1.0471976;
            a = abs(a - sector * round(a / sector));
            let q = length(p) * vec2<f32>(cos(a), sin(a));
            let thick = max(size * 0.11, 0.6);
            let arm = segment(q, vec2<f32>(0.0), vec2<f32>(size, 0.0));
            let b0 = vec2<f32>(size * 0.55, 0.0);
            let branch = segment(q, b0, b0 + vec2<f32>(0.5, 0.866) * size * 0.32);
            return min(arm, branch) - thick;
        }
        case NEEDLE: {
            let w = max(size * 0.35, 0.5) * (1.0 - abs(lx));
            return widthwise(p, r, w, size * 0.35 / r);
        }
        case BIRD: {
            // An upright "m": two bent wings from the body, their tips rising and falling.
            let q = vec2<f32>(abs(p.x), p.y);
            let elbow = vec2<f32>(r * 0.48, -r * (0.18 - 0.1 * pose));
            let tip = vec2<f32>(r, -r * (0.05 + 0.45 * pose));
            let t = size * 0.22 + 0.4;
            let wing = min(segment(q, vec2<f32>(0.0, 0.0), elbow), segment(q, elbow, tip)) - t * (1.0 - 0.5 * q.x / r);
            let body = length(p / vec2<f32>(1.0, 1.6)) - t * 1.4;
            return min(wing, body);
        }
        case FISH: {
            // A body along it, its tail at the back swinging with the pose.
            let body = (length(p / vec2<f32>(r * 0.72, size)) - 1.0) * min(r * 0.72, size);
            let root = vec2<f32>(-r * 0.55, 0.0);
            let swing = pose * 0.45;
            let back = vec2<f32>(-r, 0.0) - root;
            let c = cos(swing);
            let s = sin(swing);
            let turned = vec2<f32>(back.x * c - back.y * s, back.x * s + back.y * c);
            let side = vec2<f32>(-turned.y, turned.x) / max(length(turned), 1e-4) * size * 0.85;
            let tail = triangle(p, root, root + turned + side, root + turned - side);
            return min(body, tail);
        }
        case DIAMOND: {
            // A rhombus reaching `reach` along it and `size` across.
            let q = abs(p) / vec2<f32>(r, max(size, 1e-4));
            return (q.x + q.y - 1.0) * min(r, size) / sqrt(2.0);
        }
        default: {
            return length(p) - size;
        }
    }
}

// The distance from the sprite's edge, in pixels, negative inside.
fn edge_distance(in: SpriteOut) -> f32 {
    let p = in.at;
    let reach = in.size.x;
    let size = in.size.y;
    switch in.kind {
        case SPARKS, STREAKS: {
            let q = vec2<f32>(max(abs(p.x) - max(reach - size, 0.0), 0.0), p.y);
            return length(q) - size;
        }
        case CONFETTI: {
            let d = abs(p) - vec2<f32>(reach, size);
            return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0);
        }
        case SHARDS: {
            let l = p / vec2<f32>(max(reach, 1e-4), max(size, 1e-4));
            return -min(1.0 - abs(l.x), (1.0 - l.x) * 0.5 - abs(l.y)) * size;
        }
        case RINGS: {
            return abs(length(p) - size) - max(1.2, 0.75 * g.view.z);
        }
        case HEARTS: {
            return heart(p, size);
        }
        case LEAF, PETAL, FLAKE, NEEDLE, BIRD, FISH, DIAMOND: {
            return more_shape(in.kind, p, reach, size, in.pose);
        }
        default: {
            return length(p) - size;
        }
    }
}

// How much of the sprite covers this pixel, 0 to 1.
fn coverage(in: SpriteOut) -> f32 {
    let px = g.view.z;
    let size = max(in.size.y, 1e-4);
    let l = in.at / vec2<f32>(max(in.size.x, 1e-4), size);
    let d = edge_distance(in);
    let soft = in.size.z;
    let band = in.size.w;
    // Smaller than a pixel: dim instead of flickering.
    let tiny = clamp(size / px, 0.0, 1.0);
    var cover: f32;
    if soft > 0.0 {
        // A soft edge reaching `soft` of the size in from the edge and out past it.
        let w = max(px, soft * size * 2.0);
        cover = smoothstep(0.0, 1.0, 0.5 - d / w);
    } else {
        switch in.kind {
            case SPARKS: {
                let across = clamp((1.0 - abs(l.y)) * size / px, 0.0, 1.0);
                let along = clamp((1.0 - abs(l.x)) * 4.0, 0.0, 1.0);
                // Brightest at the front, fading towards the tail.
                cover = across * along * (0.25 + 0.75 * (l.x * 0.5 + 0.5)) * tiny;
            }
            case SMOKE: {
                cover = exp(-3.5 * dot(l, l));
            }
            case STARS: {
                let x = abs(l.x);
                let y = abs(l.y);
                let rays = exp(-7.0 * x) * exp(-90.0 * y * y) + exp(-7.0 * y) * exp(-90.0 * x * x);
                cover = min(rays + exp(-18.0 * dot(l, l)), 1.0);
            }
            case BUBBLES: {
                let r = length(l);
                let rim = clamp((0.15 * size - abs(d + 0.075 * size)) / px + 0.5, 0.0, 1.0);
                let fill = 0.15 * clamp(0.5 - d / px, 0.0, 1.0);
                let shine = exp(-40.0 * dot(l - vec2<f32>(-0.35, -0.35), l - vec2<f32>(-0.35, -0.35)));
                cover = min(max(rim, fill) + shine * step(r, 1.0), 1.0) * tiny;
            }
            default: {
                cover = clamp(0.5 - d / px, 0.0, 1.0) * tiny;
            }
        }
    }
    if band > 0.0 {
        // Only the outer band shows: an outline round the sprite's edge.
        cover = cover * clamp(0.5 + (d + band * size) / px, 0.0, 1.0);
    }
    return cover;
}

@fragment
fn fs_sprite(in: SpriteOut) -> @location(0) vec4<f32> {
    return light_flat(sprite_colour(in), in.light.x, in.light.y, in.emit, in.extra);
}

// The sprite's premultiplied colour and coverage here.
fn sprite_colour(in: SpriteOut) -> vec4<f32> {
    let cover = coverage(in);
    let a = cover * abs(in.color.a);
    if a < 1e-4 {
        discard;
    }
    var rgb = in.color.rgb * in.shade;
    if in.kind == LEAF {
        // A darker vein down its middle.
        let l = in.at.y / max(in.size.y, 1e-4);
        rgb = rgb * (1.0 - 0.3 * exp(-60.0 * l * l));
    }
    if in.color.a < 0.0 {
        // Added light, brighter than the ball colour so it blooms, covering nothing.
        return vec4<f32>(rgb * a * 2.0, 0.0);
    }
    return vec4<f32>(rgb * a, a);
}

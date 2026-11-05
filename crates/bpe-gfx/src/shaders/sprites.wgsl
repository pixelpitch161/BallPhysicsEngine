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
};

@group(0) @binding(0) var<uniform> g: Globals;

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

struct SpriteIn {
    // centre xy (world pixels), turn (radians), half size (pixels)
    @location(0) place: vec4<f32>,
    // linear light; a negative alpha adds |alpha| of the light and covers nothing
    @location(1) color: vec4<f32>,
    // x: half length along the turn, y: shade, z: softness 0..1, w: band 0..1
    @location(2) style: vec4<f32>,
    @location(3) shape: vec4<u32>,
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
    if v.shape.x == SPARKS || v.shape.x == STREAKS || v.shape.x == CONFETTI || v.shape.x == SHARDS {
        reach = max(v.style.x, size);
    }
    // Room for the soft edge and a pixel either side.
    var margin = 2.0 * px + v.style.z * size;
    if v.shape.x == RINGS {
        margin = margin + max(1.2, 0.75 * px) + 2.0;
    }
    let half = vec2<f32>(reach, size) + vec2<f32>(margin);
    let at = corner * half;
    out.pos = to_clip(v.place.xy + rotate(at, v.place.z));
    out.at = at;
    out.color = v.color;
    out.size = vec4<f32>(reach, size, clamp(v.style.z, 0.0, 1.0), clamp(v.style.w, 0.0, 1.0));
    out.kind = v.shape.x;
    out.shade = v.style.y;
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
    return sprite_colour(in);
}

// The sprite's premultiplied colour and coverage here.
fn sprite_colour(in: SpriteOut) -> vec4<f32> {
    let cover = coverage(in);
    let a = cover * abs(in.color.a);
    if a < 1e-4 {
        discard;
    }
    let rgb = in.color.rgb * in.shade;
    if in.color.a < 0.0 {
        // Added light, brighter than the ball colour so it blooms, covering nothing.
        return vec4<f32>(rgb * a * 2.0, 0.0);
    }
    return vec4<f32>(rgb * a, a);
}

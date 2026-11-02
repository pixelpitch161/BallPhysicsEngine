// Stamping marks into a canvas's two textures: one instanced quad per mark, in the order the
// marks were made. The paint texture takes each mark premultiplied over what is there; the aux
// texture's r is replaced by the mark's time wherever the mark covers at least half a texel. Shapes are signed
// distances in canvas pixels with a soft edge one texel wide.
//
// One pass draws the marks of a few world steps, and its depth says when in the pass each texel
// was last touched: a stroke is a ball's disc moving from its start to its end over its step, so
// where two balls paint the same texels in the same pass, the one that left a texel later stays
// on top, whichever was drawn first. Other shapes land at their step's end.

struct Canvas {
    // Width and height in canvas pixels, then the texture's width and height in texels.
    size: vec4<f32>,
};

@group(0) @binding(0) var<uniform> canvas: Canvas;

struct MarkIn {
    // Where the mark starts and ends, in canvas pixels from the canvas's middle.
    @location(0) ends: vec4<f32>,
    // Radius, turn in radians, variant, shape (0 stroke, 1 splat, 2 ring, 3 star, 4 crack,
    // 5 burst, 6 dots, 7 smear). A stroke's variant: bit 0 square tips, bit 1 no start cap (its
    // turn is then the previous piece's direction), bit 2 no end cap.
    @location(1) look: vec4<f32>,
    // Linear light, straight alpha.
    @location(2) colour: vec4<f32>,
    // x: when the mark was made, in seconds; y: its world step counted from the pass's first;
    // z: how many world steps the pass's marks span; w: the radius at the mark's end.
    @location(3) clock: vec4<f32>,
    // The colour at the mark's end, straight alpha.
    @location(4) colour_to: vec4<f32>,
    // x, y: the part of its step a stroke was made over, from 0 to 1.
    @location(5) within: vec4<f32>,
};

struct StampOut {
    @builtin(position) pos: vec4<f32>,
    // Canvas pixels from the canvas's middle.
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) ends: vec4<f32>,
    @location(2) @interpolate(flat) look: vec4<f32>,
    @location(3) @interpolate(flat) colour: vec4<f32>,
    @location(4) @interpolate(flat) clock: vec4<f32>,
    @location(5) @interpolate(flat) colour_to: vec4<f32>,
    @location(6) @interpolate(flat) within: vec4<f32>,
};

const TAU: f32 = 6.283185307;

// Canvas pixels per texel, the longer of the two sides.
fn texel_size() -> f32 {
    return max(canvas.size.x / max(canvas.size.z, 1.0), canvas.size.y / max(canvas.size.w, 1.0));
}

@vertex
fn vs_stamp(@builtin(vertex_index) i: u32, m: MarkIn) -> StampOut {
    let corner = vec2<f32>(f32(i & 1u), f32((i >> 1u) & 1u)) * 2.0 - 1.0;
    let start = m.ends.xy;
    let to = m.ends.zw;
    // A stroke or smear may be wider at either end.
    let r = max(m.look.x, m.clock.w);
    let margin = 2.0 * texel_size();
    var mid = start;
    var along = vec2<f32>(1.0, 0.0);
    var half = vec2<f32>(r + margin);
    if m.look.w < 0.5 || m.look.w > 6.5 {
        // A stroke's box runs along it, past both ends by the radius.
        let d = to - start;
        let span = length(d);
        if span > 0.0 {
            along = d / span;
        }
        mid = (start + to) * 0.5;
        half = vec2<f32>(span * 0.5 + r + margin, r + margin);
    }
    let across = vec2<f32>(-along.y, along.x);
    let local = mid + along * corner.x * half.x + across * corner.y * half.y;
    var out: StampOut;
    // The top of the canvas (negative y) is the top of the texture.
    out.pos = vec4<f32>(local.x / (canvas.size.x * 0.5), -local.y / (canvas.size.y * 0.5), 0.0, 1.0);
    out.local = local;
    out.ends = m.ends;
    out.look = m.look;
    out.colour = m.colour;
    out.clock = m.clock;
    out.colour_to = m.colour_to;
    out.within = m.within;
    return out;
}

// A well-mixed number from 0 to 1 for a variant, an index and a salt (PCG).
fn hash(variant: u32, k: u32, salt: u32) -> f32 {
    var x = variant * 747796405u + k * 2891336453u + salt * 277803737u + 1u;
    x = ((x >> ((x >> 28u) + 4u)) ^ x) * 277803737u;
    x = (x >> 22u) ^ x;
    return f32(x) / 4294967295.0;
}

fn segment(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-8), 0.0, 1.0);
    return length(pa - ba * h);
}

fn box_distance(p: vec2<f32>, half: vec2<f32>) -> f32 {
    let d = abs(p) - half;
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0);
}

// A capsule from `a` to `b` whose radius runs from `r0` to `r1`; a square tip squares its ends off.
fn stroke(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>, r0: f32, r1: f32, square: bool) -> f32 {
    let d = b - a;
    let span = length(d);
    let t = along_part(p, a, b);
    let r = mix(r0, r1, t);
    if !square {
        return segment(p, a, b) - r;
    }
    var along = vec2<f32>(1.0, 0.0);
    if span > 0.0 {
        along = d / span;
    }
    let q = p - a;
    let u = dot(q, along);
    return max(abs(dot(q, vec2<f32>(-along.y, along.x))) - r, max(-u - r0, u - span - r1));
}

// How far along `a`..`b` the nearest point to `p` is, from 0 to 1.
fn along_part(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let ba = b - a;
    return clamp(dot(p - a, ba) / max(dot(ba, ba), 1e-8), 0.0, 1.0);
}

// A blob of 7 lobes, their size and phase perturbed by the variant.
fn splat(q: vec2<f32>, r: f32, v: u32) -> f32 {
    let a = atan2(q.y, q.x);
    let edge = 0.74 + 0.18 * cos(7.0 * a + hash(v, 0u, 1u) * TAU) + 0.08 * cos(3.0 * a + hash(v, 0u, 2u) * TAU);
    return (length(q) - r * edge) * 0.8;
}

fn ring(q: vec2<f32>, r: f32) -> f32 {
    return abs(length(q) - 0.85 * r) - 0.15 * r;
}

// A five-pointed star whose points reach `r` (after Inigo Quilez's sdStar5).
fn star(q: vec2<f32>, r: f32) -> f32 {
    let k1 = vec2<f32>(0.809016994, -0.587785252);
    let k2 = vec2<f32>(-k1.x, k1.y);
    var p = vec2<f32>(abs(q.x), -q.y);
    p -= 2.0 * max(dot(k1, p), 0.0) * k1;
    p -= 2.0 * max(dot(k2, p), 0.0) * k2;
    p.x = abs(p.x);
    p.y -= r;
    let ba = 0.45 * vec2<f32>(-k1.y, k1.x) - vec2<f32>(0.0, 1.0);
    let h = clamp(dot(p, ba) / dot(ba, ba), 0.0, r);
    return length(p - ba * h) * sign(p.y * ba.x - p.x * ba.y);
}

// Five jagged rays, their turns, lengths and zigzags from the variant.
fn crack(q: vec2<f32>, r: f32, v: u32) -> f32 {
    var d = length(q) - 0.12 * r;
    for (var k = 0u; k < 5u; k++) {
        let a = TAU * f32(k) / 5.0 + (hash(v, k, 3u) - 0.5) * 0.8;
        let reach = r * (0.6 + 0.4 * hash(v, k, 4u));
        let along = vec2<f32>(cos(a), sin(a));
        let across = vec2<f32>(-along.y, along.x);
        let p1 = along * reach / 3.0 + across * (hash(v, k, 5u) - 0.5) * 0.25 * reach;
        let p2 = along * reach * 2.0 / 3.0 + across * (hash(v, k, 6u) - 0.5) * 0.25 * reach;
        let p3 = along * reach;
        let w = 0.05 * r;
        d = min(d, segment(q, vec2<f32>(0.0), p1) - w);
        d = min(d, segment(q, p1, p2) - w * 0.7);
        d = min(d, segment(q, p2, p3) - w * 0.4);
    }
    return d;
}

// Twelve thin rays out from near the middle.
fn burst(q: vec2<f32>, r: f32) -> f32 {
    let sector = TAU / 12.0;
    let a = atan2(q.y, q.x);
    let folded = a - sector * round(a / sector);
    let p = length(q) * vec2<f32>(cos(folded), sin(folded));
    return segment(p, vec2<f32>(0.3 * r, 0.0), vec2<f32>(r, 0.0)) - 0.04 * r;
}

// Six discs placed and sized by the variant, all inside the radius.
fn dots(q: vec2<f32>, r: f32, v: u32) -> f32 {
    var d = 1e9;
    for (var k = 0u; k < 6u; k++) {
        let a = TAU * hash(v, k, 7u);
        let centre = vec2<f32>(cos(a), sin(a)) * r * (0.15 + 0.6 * hash(v, k, 8u));
        d = min(d, length(q - centre) - r * (0.12 + 0.13 * hash(v, k, 9u)));
    }
    return d;
}

// When in its step (0 its start, 1 its end) a disc of radius `r` moving from `a` to `b` last
// touches `p`: the later of the two times its edge crosses `p`, or its nearest pass for a point
// just beyond its edge. Not clamped: below 0 or above 1 means before or after the move.
fn last_touch(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let d = b - a;
    let dd = dot(d, d);
    if dd < 1e-8 {
        return 1.0;
    }
    let e = p - a;
    let along = dot(e, d) / dd;
    let off = max(dot(e, e) - along * along * dd, 0.0);
    let half = sqrt(max(r * r - off, 0.0) / dd);
    return along + half;
}

struct StampTargets {
    @location(0) paint: vec4<f32>,
    @location(1) aux: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

@fragment
fn fs_stamp(in: StampOut) -> StampTargets {
    let start = in.ends.xy;
    let r = in.look.x;
    let variant = u32(round(in.look.z));
    let shape = u32(round(in.look.w));
    let r_to = in.clock.w;
    let to = in.ends.zw;
    let ribbon = shape == 0u || shape == 7u;
    // A piece of a longer stroke leaves out the caps that the pieces beside it cover. Two joined
    // pieces are cut apart by one plane across the first of them, so they meet without a gap
    // or an overlap however the ribbon bends. The cut is hard: every texel belongs to one
    // piece, so translucent paint never shows a seam or a bead.
    if shape == 0u && (variant & 6u) != 0u {
        let d = to - start;
        let span = length(d);
        if (variant & 2u) != 0u {
            // The previous piece ends where this one starts, and its direction is the turn.
            let before = vec2<f32>(cos(in.look.y), sin(in.look.y));
            if dot(in.local - start, before) < 0.0 {
                discard;
            }
        }
        if (variant & 4u) != 0u && span > 0.0 && dot(in.local - start, d / span) >= span {
            discard;
        }
    }
    // Stamps are drawn in their own turned frame.
    let c = cos(in.look.y);
    let s = sin(in.look.y);
    let o = in.local - start;
    let q = vec2<f32>(c * o.x + s * o.y, -s * o.x + c * o.y);
    var d = 0.0;
    switch shape {
        case 0u: { d = stroke(in.local, start, to, r, r_to, (variant & 1u) != 0u); }
        case 1u: { d = splat(q, r, variant); }
        case 2u: { d = ring(q, r); }
        case 3u: { d = star(q, r); }
        case 4u: { d = crack(q, r, variant); }
        case 5u: { d = burst(q, r); }
        case 6u: { d = dots(q, r, variant); }
        default: { d = stroke(in.local, start, to, r, r_to, false); }
    }
    let cover = clamp(0.5 - d / texel_size(), 0.0, 1.0);
    if cover <= 0.0 {
        discard;
    }
    // A stroke or smear runs from its colour to its end colour.
    var colour = in.colour;
    if ribbon {
        colour = mix(in.colour, in.colour_to, along_part(in.local, start, to));
    }
    let amount = clamp(colour.a, 0.0, 1.0) * cover;
    var out: StampTargets;
    out.paint = vec4<f32>(max(colour.rgb, vec3<f32>(0.0)) * amount, amount);
    // Only the mark's solid part re-dates the texel: its soft edge leaves older paint's time be.
    out.aux = vec4<f32>(in.clock.x, 0.0, 0.0, select(0.0, 1.0, cover >= 0.5));
    var touch = 1.0;
    if shape == 0u {
        touch = last_touch(in.local, start, to, 0.5 * (r + r_to));
    }
    // A piece of a longer move scales its own touch to its share of the move, so the order of
    // two balls' paint runs on smoothly across the pieces' joints.
    out.depth = (in.clock.y + clamp(mix(in.within.x, in.within.y, touch), 0.0, 1.0)) / max(in.clock.z, 1.0);
    return out;
}

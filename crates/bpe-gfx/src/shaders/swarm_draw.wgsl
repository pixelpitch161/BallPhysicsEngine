// Drawing a Huge swarm (particles part 8), pasted after sprites.wgsl and each.wgsl: each
// instance is one particle of the swarm's buffers, worked out as the exact engine's drawing does
// (bpe-kit `things/particles/draw.rs`: `seen`, `sprite`; `around_subs.rs`:
// `particle_with_extras`) and handed to sprites.wgsl's own `sprite_out`, so it looks the same in
// every pass. A particle the layer doesn't draw gets an empty quad.

struct SwarmLook {
    // drawn (0 sprites, 1 streaks, 2 more shapes, 3 glitter), sprite code, role (SW_ROLE_*),
    // flags (SW_*)
    k0: vec4<u32>,
    // the particle step, Only (0 all, 1 younger, 2 older, 3 a share, 4 the rest), size over life
    // (0 even, 1 grow, 2 shrink, 3 pop), fade (0 in, 1 out, 2 both, 3 none)
    k1: vec4<u32>,
    // colour follow (SW_FOLLOW_*), repeat (0 clamp, 1 repeat, 2 mirror), angle (0 own turn,
    // 1 the way it goes, 2 a bird's lean, 3 pixels), 0
    k2: vec4<u32>,
    // dt, the drawing's time, the particle step's time, opacity
    t0: vec4<f32>,
    // Only's age, Only's share, twinkle, twinkle speed
    t1: vec4<f32>,
    // size ×, blend amount, softness, the thing's code (for Things masks' silhouettes)
    t2: vec4<f32>,
    // sprites: length = size × x + speed × y; streaks: stretch, least, width; shapes: length,
    // pixel grid, turn by (radians), flap speed; glitter: shimmer, light from (radians),
    // sharpness, base
    d0: vec4<f32>,
    // glitter: star, 0, 0, 0
    d1: vec4<f32>,
    // glitter: the flash colour (linear)
    d2: vec4<f32>,
    // paint: span, way xy (Place's angle), scroll
    p0: vec4<f32>,
    // paint: centre xy, 0, 0
    p1: vec4<f32>,
    // the camera: centre xy, turn, zoom
    cam: vec4<f32>,
    // shadow: colour rgb, opacity; glow: strength × opacity, grows by, own colours, 0;
    // outline: width, sits (0 outside, 1 centre, 2 inside), opacity, own colours
    s0: vec4<f32>,
    // shadow: offset xy, softness (pixels), 0
    s1: vec4<f32>,
    // the colour for follows the card can't work out on the GPU
    c0: vec4<f32>,
    ramp: array<vec4<f32>, 256>,
    sub_ramp: array<vec4<f32>, 64>,
};

struct SwMain {
    place: vec2<f32>,
    velocity: vec2<f32>,
    born: u32,
    life: f32,
    seed: u32,
    turn: f32,
};

struct SwMore {
    home: vec2<f32>,
    size: f32,
    spin: f32,
    flip: f32,
    ball: u32,
    spot: u32,
    spare: u32,
};

const SW_ROLE_OWN: u32 = 0u;
const SW_ROLE_STAR: u32 = 1u;
const SW_ROLE_SHADOW: u32 = 2u;
const SW_ROLE_GLOW: u32 = 3u;
const SW_ROLE_OUTLINE: u32 = 4u;

const SW_FLUTTERS: u32 = 1u;
const SW_BALL_COLOURS: u32 = 2u;
const SW_ADDS: u32 = 4u;
const SW_SCREEN: u32 = 8u;
const SW_POSE: u32 = 16u;
const SW_LOOPED: u32 = 32u;

const SW_FOLLOW_FIXED: u32 = 0u;
const SW_FOLLOW_LIFE: u32 = 1u;
const SW_FOLLOW_SPEED: u32 = 2u;
const SW_FOLLOW_ORDER: u32 = 3u;
const SW_FOLLOW_PLACE: u32 = 4u;
const SW_FOLLOW_SCREEN: u32 = 5u;
const SW_FOLLOW_START: u32 = 6u;
const SW_FOLLOW_TIME: u32 = 7u;

const SW_TAU: f32 = 6.2831853;

@group(1) @binding(0) var<uniform> look: SwarmLook;
@group(1) @binding(1) var<storage, read> sw_main: array<SwMain>;
@group(1) @binding(2) var<storage, read> sw_more: array<SwMore>;
// The parent's ball colours by row (linear light).
@group(1) @binding(3) var<storage, read> sw_balls: array<vec4<f32>>;

fn sw_unit(seed: u32, k: u32) -> f32 {
    var x = 0x9e3779b9u;
    for (var i = 0u; i < 2u; i = i + 1u) {
        let p = select(k, seed, i == 0u);
        x = x ^ (p * 0x85ebca6bu);
        x = ((x << 13u) | (x >> 19u)) * 0xc2b2ae35u;
        x = x ^ (x >> 16u);
    }
    x = x ^ (x >> 15u);
    x = x * 0x2c1b3c6du;
    x = x ^ (x >> 12u);
    return f32(x >> 8u) / 16777216.0;
}

fn sw_signed(seed: u32, k: u32) -> f32 {
    return sw_unit(seed, k) * 2.0 - 1.0;
}

fn sw_finite(x: f32) -> f32 {
    return select(0.0, x, x == x && abs(x) < 3.0e38);
}

fn sw_wrap(x0: f32, repeat: u32) -> f32 {
    let x = sw_finite(x0);
    var t = x;
    if repeat == 1u {
        t = fract(x);
    } else if repeat == 2u {
        t = 1.0 - abs(fract(x / 2.0) * 2.0 - 1.0);
    }
    return clamp(t, 0.0, 1.0);
}

fn sw_ramp(t: f32) -> vec4<f32> {
    let x = clamp(t, 0.0, 1.0) * 255.0;
    let i = u32(floor(x));
    let j = min(i + 1u, 255u);
    return mix(look.ramp[i], look.ramp[j], x - f32(i));
}

fn sw_sub_ramp(t: f32) -> vec4<f32> {
    let x = clamp(t, 0.0, 1.0) * 63.0;
    let i = u32(floor(x));
    let j = min(i + 1u, 63u);
    return mix(look.sub_ramp[i], look.sub_ramp[j], x - f32(i));
}

fn sw_to_screen(p: vec2<f32>) -> vec2<f32> {
    let d = p - look.cam.xy;
    let s = sin(-look.cam.z);
    let k = cos(-look.cam.z);
    return vec2<f32>(d.x * k - d.y * s, d.x * s + d.y * k) * look.cam.w;
}

// The particle's own colour, as `paint::offset` places it on the layer's Colours.
fn sw_own_colour(m: SwMain, o: SwMore, t: f32) -> vec4<f32> {
    let follow = look.k2.x;
    if follow == SW_FOLLOW_FIXED {
        return look.c0;
    }
    let span = max(sw_finite(look.p0.x), 1e-3);
    var raw = 0.0;
    var scrolls = true;
    switch follow {
        case 1u: {
            raw = t;
        }
        case 2u: {
            raw = length(m.velocity) / span;
        }
        case 3u: {
            let index = floor(sw_unit(m.seed, 41u) * 1024.0);
            raw = index / select(1023.0, 1024.0, (look.k0.w & SW_LOOPED) != 0u);
        }
        case 4u, 5u, 6u: {
            var at = m.place;
            if follow == SW_FOLLOW_START {
                at = o.home;
            } else if follow == SW_FOLLOW_SCREEN && (look.k0.w & SW_SCREEN) == 0u {
                at = sw_to_screen(m.place);
            }
            raw = dot(at - look.p1.xy, look.p0.yz) / span + 0.5;
        }
        case 7u: {
            scrolls = false;
            raw = look.t0.y * select(look.p0.w, 0.25, look.p0.w == 0.0);
        }
        default: {}
    }
    var x = sw_finite(raw);
    if scrolls {
        x = x + sw_finite(look.t0.y) * sw_finite(look.p0.w);
    }
    return sw_ramp(sw_wrap(x, look.k2.y));
}

fn sw_size_curve(t0: f32) -> f32 {
    let t = clamp(t0, 0.0, 1.0);
    switch look.k1.z {
        case 1u: {
            return 0.3 + 0.7 * t;
        }
        case 2u: {
            return 1.0 - 0.8 * t;
        }
        case 3u: {
            if t >= 0.9 {
                return -1.0;
            }
            return 1.0 + 0.5 * t / 0.9;
        }
        default: {
            return 1.0;
        }
    }
}

fn sw_fade(t0: f32) -> f32 {
    let t = clamp(t0, 0.0, 1.0);
    let fade_in = min(t / 0.15, 1.0);
    let fade_out = 1.0 - t;
    switch look.k1.w {
        case 0u: {
            return fade_in;
        }
        case 1u: {
            return fade_out;
        }
        case 2u: {
            return fade_in * fade_out;
        }
        default: {
            return 1.0;
        }
    }
}

// The way it points: its own turn or the way it goes.
fn sw_pointing(m: SwMain, by: f32) -> f32 {
    var base = m.turn;
    if look.k2.z == 1u && length(m.velocity) > 1e-3 {
        base = atan2(m.velocity.y, m.velocity.x);
    }
    return base + by;
}

fn sw_none() -> SpriteOut {
    var out: SpriteOut;
    out.pos = vec4<f32>(-4.0, -4.0, 0.0, 1.0);
    return out;
}

@vertex
fn vs_swarm(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> SpriteOut {
    let m = sw_main[ii];
    let o = sw_more[ii];
    let now = look.k1.x;
    let dt = look.t0.x;
    let age = f32(now - min(m.born, now)) * dt;
    // Which particles the layer draws.
    switch look.k1.y {
        case 1u: {
            if !(age < look.t1.x) {
                return sw_none();
            }
        }
        case 2u: {
            if !(age >= look.t1.x) {
                return sw_none();
            }
        }
        case 3u: {
            if !(sw_unit(m.seed, 61u) < look.t1.y) {
                return sw_none();
            }
        }
        case 4u: {
            if !(sw_unit(m.seed, 61u) >= look.t1.y) {
                return sw_none();
            }
        }
        default: {}
    }
    let t = clamp(age / max(m.life, 1e-6), 0.0, 1.0);
    let grow = sw_size_curve(t);
    if grow < 0.0 {
        return sw_none();
    }
    let twinkle = clamp(look.t1.z, 0.0, 1.0);
    let flicker = 1.0 - twinkle * (0.5 + 0.5 * sin(look.t1.w * SW_TAU * age + sw_unit(m.seed, 67u) * SW_TAU));
    let alpha = max(look.t0.w * sw_fade(t) * flicker, 0.0);
    if alpha <= 0.0 {
        return sw_none();
    }
    var base: vec4<f32>;
    if (look.k0.w & SW_BALL_COLOURS) != 0u && o.ball > 0u && o.ball - 1u < arrayLength(&sw_balls) {
        base = sw_balls[o.ball - 1u];
    } else {
        base = sw_own_colour(m, o, t);
    }
    let size = o.size * max(look.t2.x, 0.0) * grow;
    if size <= 0.0 {
        return sw_none();
    }
    // The layer's shade (`draw::shade`).
    let amount = look.t2.y;
    let adds = (look.k0.w & SW_ADDS) != 0u;
    var colour: vec4<f32>;
    if !adds && base.a > 0.0 {
        colour = vec4<f32>(base.rgb, min(base.a * alpha * amount, 1.0));
    } else {
        colour = vec4<f32>(base.rgb, -(alpha * amount));
    }
    // Its own sprite (`draw::sprite`).
    let speed = length(m.velocity);
    var v: SpriteIn;
    v.place = vec4<f32>(m.place, m.turn, size);
    v.color = colour;
    v.style = vec4<f32>(size, 1.0, 0.0, 0.0);
    // A Things mask's silhouette (colour grading part 5) takes the body's particles only.
    var code = 0u;
    if look.k0.z == SW_ROLE_OWN {
        code = u32(max(look.t2.w, 0.0));
    }
    v.shape = vec4<u32>(look.k0.y, 0u, code, 0u);
    v.light = vec4<f32>(0.0);
    v.emit = vec4<f32>(0.0);
    v.extra = vec4<f32>(0.0);
    var flutters = (look.k0.w & SW_FLUTTERS) != 0u;
    var flash = 0.0;
    switch look.k0.x {
        case 0u: {
            v.style.x = size * look.d0.x + speed * look.d0.y;
            v.style.z = clamp(look.t2.z, 0.0, 1.0);
        }
        case 1u: {
            let half_width = max(size * look.d0.z, 0.0);
            let len = max(max(speed * look.d0.x, look.d0.y), 0.0);
            var angle = m.turn;
            if speed > 1e-3 {
                angle = atan2(m.velocity.y, m.velocity.x);
            }
            v.place = vec4<f32>(m.place, angle, half_width);
            v.style.x = max(len * 0.5, half_width);
            flutters = false;
        }
        case 2u: {
            v.style.z = clamp(look.t2.z, 0.0, 1.0);
            if look.k2.z == 3u {
                // Pixels: on the grid, square to the picture.
                var at = m.place;
                if look.d0.y > 0.0 {
                    at = round(m.place / look.d0.y) * look.d0.y;
                }
                v.place = vec4<f32>(at, 0.0, size);
                v.style.x = size;
                flutters = false;
            } else {
                var angle = sw_pointing(m, look.d0.z);
                if look.k2.z == 2u {
                    angle = 0.5 * (m.velocity.x / max(speed, 1.0)) + look.d0.z;
                }
                v.place.z = angle;
                v.style.x = size * look.d0.x;
                if (look.k0.w & SW_POSE) != 0u {
                    let lived = max(look.t0.z - f32(m.born) * dt, 0.0);
                    let pose = sin(lived * look.d0.w * SW_TAU + sw_unit(m.seed, 73u) * SW_TAU);
                    v.shape.y = u32(clamp(pose * 0.5 + 0.5, 0.0, 1.0) * 65535.0);
                }
            }
        }
        case 3u: {
            let facet = m.turn + sw_unit(m.seed, 71u) * SW_TAU + look.d0.x * look.t0.y * SW_TAU * sw_signed(m.seed, 72u);
            let facing = max(cos(facet - look.d0.y), 0.0);
            flash = pow(facing, max(look.d0.z, 1.0));
            let bright = clamp(look.d0.w, 0.0, 1.0) + flash;
            v.color = vec4<f32>(colour.rgb * bright, colour.a);
            v.place.w = size * 0.75;
            v.style.x = size;
            flutters = false;
        }
        default: {}
    }
    if flutters {
        let face = cos(o.flip);
        v.place.w = size * max(abs(face), 0.12);
        v.style.y = 0.7 + 0.3 * face;
    }
    // What the run draws of it (`particle_with_extras`).
    let own_size = v.place.w;
    let own_adds = v.color.a < 0.0;
    switch look.k0.z {
        case 1u: {
            // Glitter's star.
            if !(flash > 0.02 && look.d1.x > 0.0) {
                return sw_none();
            }
            v.shape.x = 3u;
            v.place.w = size * look.d1.x * flash;
            v.style = vec4<f32>(v.place.w, 1.0, 0.0, 0.0);
            v.color = vec4<f32>(look.d2.rgb, -abs(colour.a) * flash);
        }
        case 2u: {
            v.color = vec4<f32>(look.s0.rgb, clamp(look.s0.a, 0.0, 1.0) * alpha);
            v.place.x = v.place.x + look.s1.x;
            v.place.y = v.place.y + look.s1.y;
            v.style.z = max(v.style.z, min(max(look.s1.z, 0.0) / max(own_size, 1e-3), 1.0));
        }
        case 3u: {
            var c = base;
            if look.s0.z > 0.5 {
                c = sw_sub_ramp(t);
            }
            let k = look.s0.x * alpha;
            let grow_by = max(look.s0.y, 0.0);
            v.color = vec4<f32>(c.rgb * k, -1.0);
            v.place.w = own_size + grow_by;
            v.style.x = v.style.x + grow_by;
            v.style.z = 1.0;
        }
        case 4u: {
            let w = max(look.s0.x, 0.0);
            var reach = own_size + w;
            if look.s0.y > 0.5 && look.s0.y < 1.5 {
                reach = own_size + w * 0.5;
            } else if look.s0.y >= 1.5 {
                reach = own_size;
            }
            var band = 0.0;
            if reach > 0.0 {
                band = clamp(w / reach, 0.0, 1.0);
            }
            var c = vec4<f32>(base.rgb * 0.7, base.a);
            if look.s0.w > 0.5 {
                c = sw_sub_ramp(t);
            }
            let a = min(clamp(look.s0.z, 0.0, 1.0) * alpha, 1.0);
            if own_adds {
                v.color = vec4<f32>(c.rgb * a, -1.0);
            } else {
                v.color = vec4<f32>(c.rgb, a);
            }
            v.style.x = v.style.x + reach - own_size;
            v.place.w = reach;
            v.style.w = band;
            v.style.z = 0.0;
        }
        default: {}
    }
    return sprite_out(vi, v);
}

@fragment
fn fs_swarm_each(in: SpriteOut) -> @location(0) vec4<f32> {
    return each_out(sprite_colour(in), g.post.w);
}

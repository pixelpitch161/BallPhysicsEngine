// Putting a video together: one clip's finished picture cropped to the video (cover fit), or two
// clips' pictures mixed by a transition. Line for line the maths of `compose::reference`.

struct Params {
    look: u32,
    direction: u32,
    progress: f32,
    softness: f32,
    colour: vec4<f32>,
    old_crop: vec4<f32>,
    new_crop: vec4<f32>,
    size: vec4<f32>,
}

@group(0) @binding(0) var old_picture: texture_2d<f32>;
@group(0) @binding(1) var new_picture: texture_2d<f32>;
@group(0) @binding(2) var pictures: sampler;
@group(0) @binding(3) var<uniform> params: Params;

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

const BLACK: vec4<f32> = vec4<f32>(0.0, 0.0, 0.0, 1.0);

fn inside(q: vec2<f32>) -> bool {
    return q.x >= 0.0 && q.x <= 1.0 && q.y >= 0.0 && q.y <= 1.0;
}

fn old_at(q: vec2<f32>) -> vec4<f32> {
    let c = textureSampleLevel(old_picture, pictures, params.old_crop.xy + q * params.old_crop.zw, 0.0);
    return select(BLACK, c, inside(q));
}

fn new_at(q: vec2<f32>) -> vec4<f32> {
    let c = textureSampleLevel(new_picture, pictures, params.new_crop.xy + q * params.new_crop.zw, 0.0);
    return select(BLACK, c, inside(q));
}

// The side a wipe or push comes from, as a step across the screen (y down).
fn side(direction: u32) -> vec2<f32> {
    switch direction {
        case 0u: { return vec2<f32>(0.0, 1.0); }
        case 1u: { return vec2<f32>(0.0, -1.0); }
        case 2u: { return vec2<f32>(1.0, 0.0); }
        default: { return vec2<f32>(-1.0, 0.0); }
    }
}

fn compose(uv: vec2<f32>) -> vec4<f32> {
    let p = params.progress;
    let e = smoothstep(0.0, 1.0, p);
    let c = params.colour;
    switch params.look {
        case 1u: { return mix(old_at(uv), new_at(uv), e); }
        case 2u: {
            if e < 0.5 {
                return mix(old_at(uv), c, 2.0 * e);
            }
            return mix(c, new_at(uv), 2.0 * e - 1.0);
        }
        case 3u: {
            var f = 1.0 - smoothstep(0.5, 1.0, p);
            if p < 0.5 {
                f = smoothstep(0.2, 0.5, p);
            }
            return mix(old_at(uv), new_at(uv), e) + c * f;
        }
        case 4u: {
            let s = side(params.direction);
            let along = uv.x * abs(s.x) + uv.y * abs(s.y);
            var d = along;
            if s.x + s.y > 0.0 {
                d = 1.0 - along;
            }
            let soft = params.softness;
            var w = 0.0;
            if soft <= 0.0 {
                w = select(0.0, 1.0, d < p);
            } else {
                w = clamp((p * (1.0 + soft) - d) / soft, 0.0, 1.0);
            }
            return mix(old_at(uv), new_at(uv), w);
        }
        case 5u: {
            let s = side(params.direction);
            let nq = uv - s * (1.0 - e);
            let oq = uv + s * e;
            return select(old_at(oq), new_at(nq), inside(nq));
        }
        case 6u: {
            let k = 1.0 + 2.0 * e;
            let m = 0.6 + 0.4 * e;
            let oq = 0.5 + (uv - 0.5) / k;
            let nq = 0.5 + (uv - 0.5) / m;
            return old_at(oq) * (1.0 - e) + new_at(nq) * e;
        }
        default: { return new_at(uv); }
    }
}

@fragment
fn fs_compose(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = pos.xy / params.size.xy;
    let c = compose(uv);
    return vec4<f32>(clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}

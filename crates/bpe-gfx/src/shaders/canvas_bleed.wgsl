// The bleed pass: wet paint spreads a little into its wet neighbours and into empty texels beside
// thick wet paint. A Bleed mark covers a rectangle of the canvas (its ends are the corners, in
// canvas pixels from the middle). The pass reads copies of the three textures made just before it
// (scratch) and writes the canvas's own, so every texel reads the same old picture whichever
// texel is drawn first. Texels are loaded exactly, never filtered.

struct Canvas {
    // Width and height in canvas pixels, then the texture's width and height in texels.
    size: vec4<f32>,
};

@group(0) @binding(0) var<uniform> canvas: Canvas;
@group(0) @binding(1) var old_paint: texture_2d<f32>;
@group(0) @binding(2) var old_aux: texture_2d<f32>;
@group(0) @binding(3) var old_body: texture_2d<f32>;

struct MarkIn {
    @location(0) ends: vec4<f32>,
    // x: the dry time in seconds.
    @location(1) look: vec4<f32>,
    @location(2) colour: vec4<f32>,
    // x: when the mark was made, in seconds; w: the bleed setting, 0 to 1.
    @location(3) clock: vec4<f32>,
    @location(4) colour_to: vec4<f32>,
    @location(5) within: vec4<f32>,
};

struct BleedOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) @interpolate(flat) look: vec4<f32>,
    @location(1) @interpolate(flat) clock: vec4<f32>,
};

@vertex
fn vs_bleed(@builtin(vertex_index) i: u32, m: MarkIn) -> BleedOut {
    let corner = vec2<f32>(f32(i & 1u), f32((i >> 1u) & 1u));
    let local = mix(m.ends.xy, m.ends.zw, corner);
    var out: BleedOut;
    out.pos = vec4<f32>(local.x / (canvas.size.x * 0.5), -local.y / (canvas.size.y * 0.5), 0.0, 1.0);
    out.look = m.look;
    out.clock = m.clock;
    return out;
}

struct BleedTargets {
    @location(0) paint: vec4<f32>,
    @location(1) aux: vec4<f32>,
    @location(2) body: vec4<f32>,
};

fn clamped(t: vec2<i32>) -> vec2<i32> {
    let dims = vec2<i32>(textureDimensions(old_paint));
    return clamp(t, vec2<i32>(0), dims - vec2<i32>(1));
}

fn load_paint(t: vec2<i32>) -> vec4<f32> {
    return textureLoad(old_paint, clamped(t), 0);
}

fn load_aux(t: vec2<i32>) -> vec4<f32> {
    return textureLoad(old_aux, clamped(t), 0);
}

fn load_body(t: vec2<i32>) -> vec4<f32> {
    return textureLoad(old_body, clamped(t), 0);
}

// Paint that is still wet: some, and painted less than the dry time ago.
fn is_wet(p: vec4<f32>, a: vec4<f32>, now: f32, dry: f32) -> bool {
    return dry > 0.0 && p.a > 1.0 / 255.0 && now - a.r < dry;
}

@fragment
fn fs_bleed(in: BleedOut) -> BleedTargets {
    let t = vec2<i32>(floor(in.pos.xy));
    let bleed = clamp(in.clock.w, 0.0, 1.0);
    let w = bleed * 0.25;
    let o = i32(round(1.0 + 2.0 * bleed));
    let now = in.clock.x;
    let dry = in.look.x;
    let p = load_paint(t);
    let a = load_aux(t);
    let b = load_body(t);
    var out: BleedTargets;
    out.paint = p;
    out.aux = a;
    out.body = b;
    let offs = array<vec2<i32>, 4>(vec2<i32>(-o, 0), vec2<i32>(o, 0), vec2<i32>(0, -o), vec2<i32>(0, o));
    if is_wet(p, a, now, dry) {
        var sum = vec3<f32>(0.0);
        var n = 0.0;
        for (var i = 0; i < 4; i++) {
            let q = t + offs[i];
            let pq = load_paint(q);
            let aq = load_aux(q);
            if is_wet(pq, aq, now, dry) {
                sum += pq.rgb / pq.a;
                n += 1.0;
            }
        }
        if n > 0.0 {
            let rgb = mix(p.rgb / p.a, sum / n, w);
            out.paint = vec4<f32>(rgb * p.a, p.a);
        }
        return out;
    }
    // Empty or dry: gain from wet neighbours that hold at least 48 ink.
    var g = 0.0;
    var colour = vec3<f32>(0.0);
    var newest = -1e9;
    for (var i = 0; i < 4; i++) {
        let q = t + offs[i];
        let pq = load_paint(q);
        let aq = load_aux(q);
        let bq = load_body(q).r;
        if is_wet(pq, aq, now, dry) && bq >= 48.0 / 255.0 {
            g += bq * w;
            colour += (pq.rgb / pq.a) * bq;
            newest = max(newest, aq.r);
        }
    }
    if g > 0.0 {
        let alpha = min(1.0, g * 4.0);
        // The body-weighted average of the wet neighbours' colours.
        let rgb = colour / (g / w);
        out.paint = vec4<f32>(rgb * alpha, alpha) + p * (1.0 - alpha);
        out.body = vec4<f32>(b.r + g, 0.0, 0.0, 0.0);
        if alpha >= 0.5 {
            out.aux = vec4<f32>(newest, a.g, a.b, a.a);
        }
    }
    return out;
}

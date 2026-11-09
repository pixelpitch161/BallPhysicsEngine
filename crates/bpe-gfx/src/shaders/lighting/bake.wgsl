// ---------- lighting part 3: the bake ----------
//
// A baked frame is drawn several times and averaged (Renderer::render_samples); each sample's
// light passes get its index in `u.sample`, and these random numbers (only used when baking, so
// the live picture never changes) differ from sample to sample: blurred reflections, rainbows,
// the path-traced bounce light (`fs_trace`, Stage) and more caustic photons.

var<private> RNG: u32;

fn baking() -> bool {
    return u.sample.z > 0.5;
}

// PCG hash (Jarzynski and Olano).
fn pcg(v: u32) -> u32 {
    let state = v * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

// Starts this pixel's random numbers for this sample (`salt` tells passes apart).
fn seed_rng(ip: vec2<i32>, salt: u32) {
    RNG = pcg(u32(ip.x) * 1973u + u32(ip.y) * 9277u + u32(u.sample.x) * 26699u + salt * 7919u);
}

// A random number, 0 to just under 1.
fn rand() -> f32 {
    RNG = pcg(RNG);
    return f32(RNG >> 8u) / 16777216.0;
}

// A wavelength's colour for `w` from -1 (red) to 1 (violet); it averages to white over w.
fn spectral(w: f32) -> vec3<f32> {
    let r = max(1.0 - abs(w + 0.6) / 0.6, 0.0);
    let g = max(1.0 - abs(w) / 0.6, 0.0);
    let b = max(1.0 - abs(w - 0.6) / 0.6, 0.0);
    return vec3<f32>(r * 3.5294, g * 3.3333, b * 3.5294);
}

// A direction round unit `n`, more often near it (cosine-weighted).
fn cosine_dir(n: vec3<f32>) -> vec3<f32> {
    let u1 = rand();
    let phi = 6.2831853 * rand();
    let r = sqrt(u1);
    let up = select(vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(1.0, 0.0, 0.0), abs(n.z) > 0.9);
    let t = normalize(cross(up, n));
    let b = cross(n, t);
    return normalize(t * (r * cos(phi)) + b * (r * sin(phi)) + n * sqrt(max(1.0 - u1, 0.0)));
}

// What a ray meets: 0 nothing, 1 a ball, 2 the wall, 3 the floor, 4 a shape's front; where, its
// normal, its colour, the ball (-1 none) and its surface.
struct Hit {
    what: i32,
    q: vec3<f32>,
    n: vec3<f32>,
    c: vec3<f32>,
    ball: i32,
    sf: SurfaceGpu,
};

// The first thing a ray from `o` along unit `d` meets (ball `skip` left out), as trace_scene finds
// it.
fn scene_hit(o: vec3<f32>, d: vec3<f32>, skip: i32) -> Hit {
    var h: Hit;
    h.what = 0;
    h.ball = -1;
    let ball = hit_ball(o, d, skip);
    var t = ball.x;
    var what = select(0, 1, ball.y >= 0.0);
    if in_stage() {
        if d.z < -1e-5 {
            let tw = (-st.room.y - o.z) / d.z;
            if tw > 1e-3 && tw < t {
                t = tw;
                what = 2;
            }
        }
        if st.floor.x > 0.5 && d.y > 1e-5 {
            let tf = (floor_y() - o.y) / d.y;
            if tf > 1e-3 && tf < t && (o.z + d.z * tf) >= -st.room.y {
                t = tf;
                what = 3;
            }
        }
        if d.z < -1e-5 && o.z > st.room.z {
            let ts = (st.room.z - o.z) / d.z;
            if ts > 1e-3 && ts < t && field_on_slab(o + d * ts) < 0.0 {
                t = ts;
                what = 4;
            }
        }
    }
    if what == 0 {
        return h;
    }
    let q = o + d * t;
    h.what = what;
    h.q = q;
    if what == 1 {
        let at = u32(ball.y);
        let sp = spheres[at];
        h.n = normalize(q - vec3<f32>(sp.a.xy, 0.0));
        let index = u32(max(sp.c.x, 0.0));
        h.sf = surfaces[select(0u, index, index < u32(st.more.z))];
        h.c = sp.b.rgb;
        if h.n.z > 0.2 {
            h.c = picture_at(vec3<f32>(q.xy, 0.0));
        }
        h.ball = i32(at);
        return h;
    }
    h.n = vec3<f32>(0.0, 0.0, 1.0);
    h.sf = plane_surface(st.floor.w);
    if what == 3 {
        h.n = vec3<f32>(0.0, -1.0, 0.0);
        h.sf = plane_surface(st.floor.z);
    } else if what == 4 {
        let uv = world_to_uv(vec2<f32>(q.x, q.y + q.z * st.room.w));
        let size = vec2<f32>(textureDimensions(surface));
        h.sf = surface_of(textureLoad(surface, vec2<i32>(clamp(uv, vec2<f32>(0.0), vec2<f32>(0.999)) * size), 0));
    }
    h.c = picture_at(q);
    return h;
}

// Light `l` toward 3D point `q`: the unit direction to the lamp, how far (1e5 for the sun) and
// how much of it arrives (falloff, cone, an ultraviolet light's visible share); lasers none.
struct Toward {
    dir: vec3<f32>,
    dist: f32,
    amount: f32,
};

fn light_toward(l: LightGpu, q: vec3<f32>) -> Toward {
    var out: Toward;
    out.amount = 0.0;
    out.dir = vec3<f32>(0.0, 0.0, 1.0);
    out.dist = 1e5;
    let kind = l.p0.w;
    if kind > 4.5 {
        return out;
    }
    if kind > 1.5 && kind < 2.5 {
        let el_sin = clamp(l.p0.z, -1.0, 1.0);
        out.dir = normalize(vec3<f32>(-l.p3.xy * sqrt(max(1.0 - el_sin * el_sin, 0.0)), el_sin));
        out.amount = 1.0;
    } else {
        let d = source_point(l, q.xy) - q.xy;
        let to_lamp = vec3<f32>(d, l.p0.z - q.z);
        let len3 = length(to_lamp);
        out.amount = falloff(len3, l.p2.x, l.p2.y);
        if kind > 0.5 && kind < 1.5 {
            let dist = length(d);
            let cosine = select(dot(-d / max(dist, 1e-3), l.p3.xy), 1.0, dist <= 1e-3);
            let e = clamp((cosine - l.p3.w) / max(l.p3.z - l.p3.w, 1e-6), 0.0, 1.0);
            out.amount *= e * e * (3.0 - 2.0 * e);
        }
        out.dir = select(vec3<f32>(0.0, 0.0, 1.0), to_lamp / len3, len3 > 1e-6);
        out.dist = len3;
    }
    if l.p7.y > 0.5 {
        out.amount *= max(l.p7.z, 0.0);
    }
    return out;
}

// The lamps' light falling on 3D point `q` facing `n` (no ambient: that is light from everywhere
// already), each shadowed by the balls when it casts shadows.
fn lamps_on(q: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    let near = lights_near(q.xy);
    for (var k = 0u; k < near.count; k++) {
        let i = light_in(near, k);
        let l = lights[i];
        let to = light_toward(l, q);
        let ndl = dot(n, to.dir);
        if to.amount == 0.0 || ndl <= 0.0 {
            continue;
        }
        var vis = vec3<f32>(1.0);
        if l.p5.w > 0.5 {
            let ang = max(l.p2.z / max(to.dist, 1.0), 0.01);
            vis = sphere_shadow(q + n * 0.5, to.dir, to.dist, ang, l.p6.x);
        }
        sum += l.p1.rgb * to.amount * ndl * vis;
    }
    return sum;
}

// How many rays a pixel the bake's bounce light takes each sample.
const TRACE_RAYS: i32 = 2;

// The bake's bounce light in Stage (part 3): from the point each pixel shows (a ball's surface, a
// shape's front, the wall or floor), cosine-weighted rays walk up to Light bounces hits, each hit
// lit by the lamps (with the balls' shadows) and carrying its colour on. The light arriving,
// times Bounce light; the mix adds it as it adds the background's own light.
@fragment
fn fs_trace(in: FsIn) -> @location(0) vec4<f32> {
    let ip = vec2<i32>(in.pos.xy);
    seed_rng(ip, 17u);
    let p = uv_to_world(in.uv);
    let s = textureLoad(surface, ip, 0);
    var q: vec3<f32>;
    var n: vec3<f32>;
    var skip = -1;
    if s.a > 0.5 {
        let sf = surface_of(s);
        let px = u.view.z;
        let h = height_at(ip);
        let hx = height_at(ip + vec2<i32>(1, 0)) - height_at(ip - vec2<i32>(1, 0));
        let hy = height_at(ip + vec2<i32>(0, 1)) - height_at(ip - vec2<i32>(0, 1));
        var slope = vec2<f32>(hx, hy) / (2.0 * px);
        if u.camera.w != 0.0 {
            slope = rotate_by(slope, u.camera.w);
        }
        n = normalize(vec3<f32>(-slope, 1.0));
        q = thing_point(p, h, sf.b.w > 1.5);
        if sf.b.w > 0.5 && sf.b.w < 1.5 {
            skip = ball_at(p);
        }
    } else {
        let spot = stage_spot(p);
        q = spot.p;
        n = spot.n;
    }
    let bounces = clamp(i32(u.sample.w), 1, 16);
    var sum = vec3<f32>(0.0);
    for (var r = 0; r < TRACE_RAYS; r++) {
        var ro = q + n * 0.75;
        var nn = n;
        var carried = vec3<f32>(1.0);
        var sk = skip;
        for (var b = 0; b < 16; b++) {
            if b >= bounces {
                break;
            }
            let hit = scene_hit(ro, cosine_dir(nn), sk);
            if hit.what == 0 {
                break;
            }
            let c = clamp(hit.c, vec3<f32>(0.0), vec3<f32>(0.95));
            let metal = clamp(hit.sf.a.y, 0.0, 1.0);
            sum += carried * c * (lamps_on(hit.q, hit.n) * (1.0 - metal) + vec3<f32>(max(hit.sf.e.z, 0.0)));
            carried *= c * (1.0 - metal);
            ro = hit.q + hit.n * 0.75;
            nn = hit.n;
            sk = hit.ball;
        }
    }
    let out = sum / f32(TRACE_RAYS) * u.trace.x;
    return vec4<f32>(clamp(out, vec3<f32>(-6.0e4), vec3<f32>(6.0e4)), 0.0);
}

// The bake's noise clean-up (part 3): one à-trous pass over `picture` (the traced light), 5 x 5
// taps `u.field.w` texels apart, each weighed down where the surface picture changes (cover,
// surface, height) or the light itself jumps; a step of 0 copies.
fn denoised(pos: vec2<f32>) -> vec4<f32> {
    let ip = vec2<i32>(pos);
    let step = i32(u.field.w);
    let c0 = textureLoad(picture, ip, 0);
    if step <= 0 {
        return vec4<f32>(c0.rgb, 0.0);
    }
    let s0 = textureLoad(surface, ip, 0);
    let code0 = surface_code(s0);
    let l0 = luma(c0.rgb);
    let size = vec2<i32>(textureDimensions(picture));
    let kernel = array<f32, 3>(0.375, 0.25, 0.0625);
    let height_scale = u.view.z * 3.0 * f32(step) + 1.0;
    var sum = vec3<f32>(0.0);
    var weights = 0.0;
    for (var dy = -2; dy <= 2; dy++) {
        for (var dx = -2; dx <= 2; dx++) {
            let at = clamp(ip + vec2<i32>(dx, dy) * step, vec2<i32>(0), size - 1);
            let c = textureLoad(picture, at, 0).rgb;
            let sq = textureLoad(surface, at, 0);
            var w = kernel[abs(dx)] * kernel[abs(dy)];
            w *= exp(-abs(sq.a - s0.a) * 8.0);
            w *= select(0.1, 1.0, abs(surface_code(sq) - code0) < 0.5);
            w *= exp(-abs(sq.g - s0.g) / height_scale);
            w *= exp(-abs(luma(c) - l0) / (0.5 * abs(l0) + 0.05));
            sum += c * w;
            weights += w;
        }
    }
    return vec4<f32>(sum / max(weights, 1e-6), 0.0);
}

@fragment
fn fs_denoise(in: FsIn) -> @location(0) vec4<f32> {
    return denoised(in.pos.xy);
}

// The last clean-up pass (or the copy), added into the background-light picture.
@fragment
fn fs_trace_add(in: FsIn) -> @location(0) vec4<f32> {
    return denoised(in.pos.xy);
}


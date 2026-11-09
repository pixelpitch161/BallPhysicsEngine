// ---------- caustics (part 3): light focused through glass balls ----------

struct CausticU {
    // Photons a (light, glass ball) pair, glass balls, lights, Mirror light (part 4a).
    counts: vec4<f32>,
    // World units per output pixel, this sample's turn of the photons, 1 when baking, Glass
    // caustics.
    more: vec4<f32>,
};

struct CausticGlass {
    // Centre xy, radius, see-through (a mirror: its Facet gaps).
    a: vec4<f32>,
    // The colour it lets through (normalised, linear rgb), bends light (a mirror: what it gives
    // back, its turn).
    b: vec4<f32>,
    // Rainbow split, frost, 1 for a mirror ball (part 4a), a mirror's Facets.
    c: vec4<f32>,
};

@group(3) @binding(0) var<uniform> cu: CausticU;
@group(3) @binding(1) var<storage, read> glass: array<CausticGlass>;

struct CausticOut {
    @builtin(position) pos: vec4<f32>,
    // Where in the photon's soft disc (-1 to 1 across).
    @location(0) local: vec2<f32>,
    // Its light, already over the disc's area.
    @location(1) power: vec3<f32>,
};

// Photon `k`: from its light toward a point of its glass ball's disc (a Fibonacci spiral), bent
// in and out of the sphere (its own wavelength with Rainbow split), landing on the wall, the floor
// (Stage) or the plane under the ball (Flat); drawn as a small soft disc where that point shows.
@vertex
fn vs_caustic(@builtin(vertex_index) vi: u32, @builtin(instance_index) k: u32) -> CausticOut {
    var out: CausticOut;
    out.pos = vec4<f32>(4.0, 4.0, 0.5, 1.0);
    out.local = vec2<f32>(0.0);
    out.power = vec3<f32>(0.0);
    let per = max(u32(cu.counts.x), 1u);
    let balls = max(u32(cu.counts.y), 1u);
    let pair = k / per;
    let j = k % per;
    let li = pair / balls;
    let gi = pair % balls;
    if li >= u32(cu.counts.z) {
        return out;
    }
    let l = lights[li];
    let g = glass[gi];
    // Part 4a: a mirror throws only the light of lamps that cast shadows (as the 2D photons).
    if g.c.z > 0.5 && l.p5.w < 0.5 {
        return out;
    }
    let c = vec3<f32>(g.a.xy, 0.0);
    let r = g.a.z;
    let to = light_toward(l, c);
    if to.amount == 0.0 || r <= 0.0 {
        return out;
    }
    let sun = to.dist >= 1e5 - 1.0;
    if !sun && to.dist <= r * 1.02 {
        return out;
    }
    let dir = -to.dir;
    let up = select(vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(1.0, 0.0, 0.0), abs(dir.z) > 0.9);
    let ax = normalize(cross(dir, up));
    let ay = cross(dir, ax);
    let ang = f32(j) * 2.39996323 + cu.more.y;
    let rad = r * 0.999 * sqrt((f32(j) + 0.5) / f32(per));
    let aim_at = c + (ax * cos(ang) + ay * sin(ang)) * rad;
    var o = c - dir * (4.0 * r) + (aim_at - c);
    var rd = dir;
    if !sun {
        o = c + to.dir * to.dist;
        rd = normalize(aim_at - o);
    }
    let oc = o - c;
    let b = dot(oc, rd);
    let disc = b * b - (dot(oc, oc) - r * r);
    if disc < 0.0 {
        return out;
    }
    let p1 = o + rd * (-b - sqrt(disc));
    let n1 = (p1 - c) / r;
    let mirror = g.c.z > 0.5;
    // Its wavelength: -1 red to 1 violet, red bending least.
    RNG = pcg(k * 7919u + u32(cu.more.y * 1000.0));
    let w = select(0.0, rand() * 2.0 - 1.0, g.c.x > 0.0 && !mirror);
    var p2 = p1;
    var d3 = vec3<f32>(0.0);
    if mirror {
        // A mirror ball (part 4a): off the tile it meets (a gap throws nothing).
        var n = n1;
        if g.c.w >= 3.0 {
            let tile = facet_normal(n1, g.c.w, g.b.w);
            if g.a.w > 0.0 && tile.w > 1.0 - g.a.w {
                return out;
            }
            n = normalize(tile.xyz);
        }
        d3 = reflect(rd, n);
        if dot(d3, n1) <= 0.0 {
            return out;
        }
    } else {
        let eta = max(g.b.w * (1.0 + max(g.c.x, 0.0) * 0.04 * w), 1.0001);
        let d2 = refract(rd, n1, 1.0 / eta);
        if dot(d2, d2) < 1e-6 {
            return out;
        }
        p2 = p1 + d2 * (-2.0 * dot(p1 - c, d2));
        let n2 = (p2 - c) / r;
        d3 = refract(d2, -n2, eta);
        if dot(d3, d3) < 1e-6 {
            return out;
        }
    }
    // Where it lands.
    var t = 1e30;
    var on_floor = false;
    if in_stage() {
        if d3.z < -1e-5 {
            let tw = (-st.room.y - p2.z) / d3.z;
            if tw > 0.0 {
                t = tw;
            }
        }
        if st.floor.x > 0.5 && d3.y > 1e-5 && st.room.w > LEAST_K {
            let tf = (floor_y() - p2.y) / d3.y;
            if tf > 0.0 && tf < t && (p2.z + d3.z * tf) >= -st.room.y {
                t = tf;
                on_floor = true;
            }
        }
    } else if d3.z < -1e-5 {
        t = (-r - p2.z) / d3.z;
    }
    if t >= 1e29 {
        return out;
    }
    let q = p2 + d3 * t;
    let px = cu.more.x;
    let radius = max(2.0 * r * sqrt(3.14159265 / f32(per)), 1.5 * px) * (1.0 + 4.0 * max(g.c.y, 0.0));
    var power = l.p1.rgb * to.amount * (3.14159265 * r * r / f32(per));
    if mirror {
        power *= g.b.rgb * cu.counts.w;
    } else {
        power *= clamp(g.a.w, 0.0, 1.0) * 0.92 * g.b.rgb * cu.more.w;
        if g.c.x > 0.0 {
            power *= spectral(w);
        }
    }
    power *= 3.0 / (3.14159265 * radius * radius);
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u)) * 2.0 - vec2<f32>(1.0);
    let squash = select(1.0, st.room.w, on_floor);
    let at = vec2<f32>(q.x, q.y + q.z * st.room.w) + vec2<f32>(corner.x, corner.y * squash) * radius;
    let uv = world_to_uv(at);
    out.pos = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.5, 1.0);
    out.local = corner;
    out.power = power;
    return out;
}

@fragment
fn fs_caustic(in: CausticOut) -> @location(0) vec4<f32> {
    let d = dot(in.local, in.local);
    if d >= 1.0 {
        discard;
    }
    let s = textureLoad(surface, vec2<i32>(in.pos.xy), 0);
    // Nothing in front: in Stage anything covering the wall, in Flat a ball (it has height).
    var open = 1.0 - clamp(s.g * 0.5, 0.0, 1.0);
    if in_stage() {
        open = 1.0 - clamp(s.a, 0.0, 1.0);
    }
    let k = (1.0 - d) * (1.0 - d);
    return vec4<f32>(in.power * k * open, 0.0);
}

// ---------- light beams' bands (glass and light part 2) ----------
//
// A Light beam is drawn as ribbons: each the stretch between one ray's segment `a` and the next
// ray's segment `b`, added over the mixed picture. Brighter where the band narrows (a lens
// focusing it), dimmer where it spreads (a prism's fan), soft at the band's two edges.

struct RibbonGpu {
    // One ray's segment, xy to zw (world).
    a: vec4<f32>,
    // The next ray's segment, xy to zw (world).
    b: vec4<f32>,
    // Colour times strength and glow (linear), the band's edge softness (0 to 1).
    c: vec4<f32>,
    // Where across the band each ray is (0 to 1), how far apart they were at the lamp, spare.
    d: vec4<f32>,
};

@group(3) @binding(2) var<storage, read> ribbons: array<RibbonGpu>;

// How much brighter a ribbon gets where its rays come together, at most.
const RIBBON_MOST: f32 = 8.0;

struct RibbonOut {
    @builtin(position) pos: vec4<f32>,
    // Where across the band, 0 to 1.
    @location(0) across: f32,
    @location(1) colour: vec3<f32>,
    @location(2) @interpolate(flat) soft: f32,
};

@vertex
fn vs_ribbon(@builtin(vertex_index) vi: u32, @builtin(instance_index) k: u32) -> RibbonOut {
    let r = ribbons[k];
    // A strip of four: a's start, b's start, a's end, b's end.
    let second = (vi & 1u) == 1u;
    let far = vi >= 2u;
    let p = select(select(r.a.xy, r.a.zw, far), select(r.b.xy, r.b.zw, far), second);
    // The band's width at this end, measured across the way it runs (not along a wall it meets
    // at a slant).
    let way = (r.a.zw - r.a.xy) + (r.b.zw - r.b.xy);
    let along = select(vec2<f32>(1.0, 0.0), way / max(length(way), 1e-6), dot(way, way) > 1e-12);
    let gap = select(r.b.xy - r.a.xy, r.b.zw - r.a.zw, far);
    let width = abs(gap.x * along.y - gap.y * along.x);
    let bright = clamp(r.d.z / max(width, 1e-3), 0.0, RIBBON_MOST);
    let uv = world_to_uv(p);
    var out: RibbonOut;
    out.pos = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.5, 1.0);
    out.across = select(r.d.x, r.d.y, second);
    out.colour = max(r.c.rgb, vec3<f32>(0.0)) * bright;
    out.soft = clamp(r.c.w, 0.0, 1.0) * 0.5;
    return out;
}

@fragment
fn fs_ribbon(in: RibbonOut) -> @location(0) vec4<f32> {
    // The editor's views of the light alone show no beams (as `fs_mix` adds no laser lines there).
    if u.ambient.w > 0.5 {
        discard;
    }
    var edge = 1.0;
    if in.soft > 1e-4 {
        edge = smoothstep(0.0, in.soft, in.across) * smoothstep(0.0, in.soft, 1.0 - in.across);
    }
    return vec4<f32>(min(in.colour * edge, vec3<f32>(6.0e4)), 0.0);
}

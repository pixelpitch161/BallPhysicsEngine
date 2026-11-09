// ---------- depth of field (part 2b, Stage) ----------

// How deep (z, toward the camera) the stage is at pixel `ip`: a ball's surface, a shape's front,
// or the wall or floor it shows.
fn depth_at(ip: vec2<i32>) -> f32 {
    let size = vec2<i32>(textureDimensions(surface));
    let at = clamp(ip, vec2<i32>(0), size - 1);
    let uv = (vec2<f32>(at) + 0.5) / vec2<f32>(size);
    let s = textureLoad(surface, at, 0);
    var z = stage_spot(uv_to_world(uv)).p.z;
    if s.a > 0.02 {
        var zt = s.g / s.a;
        if surface_of(s).b.w > 1.5 {
            zt += st.room.z;
        }
        z = mix(z, zt, smoothstep(0.02, 0.98, s.a));
    }
    return z;
}

// How wide the blur is at depth `z`, in output pixels.
fn blur_at(z: f32) -> f32 {
    return min(abs(z - st.dof.y) * st.dof.z / max(u.view.z, 1e-6), DOF_MOST);
}

@fragment
fn fs_dof(in: FsIn) -> @location(0) vec4<f32> {
    let ip = vec2<i32>(in.pos.xy);
    let c = textureLoad(picture, ip, 0);
    let own = blur_at(depth_at(ip));
    if own < 0.5 {
        return c;
    }
    let size = vec2<i32>(textureDimensions(picture));
    let turn = 6.2831853 * fract(52.9829189 * fract(dot(in.pos.xy, vec2<f32>(0.06711056, 0.00583715))));
    var sum = c.rgb;
    var weights = 1.0;
    // The bake takes more taps: smoother bokeh.
    let taps = select(DOF_TAPS, 4 * DOF_TAPS, baking());
    // A wide blur reads each tap from the small copy, where it is the mean of 4 x 4 pixels: 24
    // single pixels spread over a wide disc show as speckle. A narrow one reads single pixels.
    let soft = smoothstep(6.0, 14.0, own);
    let small = DOF_SMALL * vec2<f32>(textureDimensions(lit_picture));
    for (var k = 0; k < taps; k++) {
        let a = f32(k) * 2.39996 + turn;
        let r = own * sqrt((f32(k) + 0.5) / f32(taps));
        let place = in.pos.xy + vec2<f32>(cos(a), sin(a)) * r;
        let at = clamp(vec2<i32>(floor(place)), vec2<i32>(0), size - 1);
        var tc = textureLoad(picture, at, 0).rgb;
        if soft > 0.0 {
            // Only where everything the copy averaged there blurs as far as this tap: next to
            // something sharp it would bring that thing's colour in.
            // The least of the four texels the filtered read blends (a filtered alpha would be their
            // mean, and let half of a sharp texel's colour through).
            let uv = place / small;
            let copy = textureSampleLevel(lit_picture, samp, uv, 0.0).rgb;
            let near = textureGather(3, lit_picture, samp, uv);
            let least = min(min(near.x, near.y), min(near.z, near.w));
            tc = mix(tc, copy, soft * clamp(least - r + 1.0, 0.0, 1.0));
        }
        // A tap counts only where its own blur reaches this pixel, so a sharp thing isn't smeared
        // by a blurred one behind it; bright taps count more (round bokeh).
        let reach = clamp(blur_at(depth_at(at)) - r + 1.0, 0.0, 1.0);
        let bright = 1.0 + max(dot(tc, vec3<f32>(0.2126, 0.7152, 0.0722)) - 1.0, 0.0);
        let wt = reach * bright;
        sum += tc * wt;
        weights += wt;
    }
    return vec4<f32>(sum / weights, c.a);
}

// Depth of field's small copy: each texel the mean of the DOF_SMALL x DOF_SMALL pixels under it,
// from four filtered reads each halfway between two pixels' centres both ways; in alpha, the
// least blur among those pixels (what reach the mean as a whole has).
@fragment
fn fs_dof_small(in: FsIn) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(picture));
    let corner = floor(in.pos.xy) * DOF_SMALL;
    var sum = vec3<f32>(0.0);
    for (var k = 0; k < 4; k++) {
        let at = corner + vec2<f32>(f32(k % 2) * 2.0 + 1.0, f32(k / 2) * 2.0 + 1.0);
        sum += textureSampleLevel(picture, samp, at / size, 0.0).rgb;
    }
    var least = DOF_MOST;
    let n = i32(DOF_SMALL);
    for (var k = 0; k < n * n; k++) {
        least = min(least, blur_at(depth_at(vec2<i32>(corner) + vec2<i32>(k % n, k / n))));
    }
    return vec4<f32>(sum * 0.25, least);
}

fn srgb_decode(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

// How bright light `l` is as a heat colour (linear): the stops are style::HEAT's, in sRGB.
fn heat(l: f32) -> vec3<f32> {
    if l < 0.0 {
        // Negative light: violet, deeper the darker.
        return srgb_decode(mix(vec3<f32>(0.0), vec3<f32>(0.541, 0.302, 1.0), clamp(-l, 0.0, 1.0)));
    }
    var at = array<f32, 7>(0.0, 0.25, 0.5, 1.0, 1.5, 2.0, 3.0);
    var colour = array<vec3<f32>, 7>(
        vec3<f32>(0.0, 0.0, 0.0),
        vec3<f32>(0.102, 0.165, 0.620),
        vec3<f32>(0.098, 0.706, 0.902),
        vec3<f32>(0.235, 0.812, 0.306),
        vec3<f32>(0.949, 0.824, 0.294),
        vec3<f32>(1.0, 0.231, 0.184),
        vec3<f32>(1.0, 1.0, 1.0),
    );
    var out = colour[6];
    for (var k = 0; k < 6; k++) {
        if l < at[k + 1] {
            out = mix(colour[k], colour[k + 1], (l - at[k]) / (at[k + 1] - at[k]));
            break;
        }
    }
    return srgb_decode(out);
}

// ---------- the glossy floor (Stage, a floor that isn't Matte) ----------

// The lit picture round world point `q`, blurred over `radius`: a Vogel disc of FLOOR_TAPS
// samples turned by a per-pixel noise `turn`, so a wide blur is soft grain, not copies. With
// `things` false the things on the picture are left out (the wall mirrors only the wall). Its
// alpha is how much was found.
fn lit_near(q: vec2<f32>, radius: f32, turn: f32, things: bool) -> vec4<f32> {
    var sum = vec3<f32>(0.0);
    var weight = 0.0;
    for (var k = 0; k < FLOOR_TAPS; k++) {
        let a = f32(k) * 2.39996 + turn;
        let o = vec2<f32>(cos(a), sin(a)) * radius * sqrt((f32(k) + 0.5) / f32(FLOOR_TAPS));
        let uv = world_to_uv(q + o);
        let inside = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
        let off = length(uv - inside);
        var wt = clamp(1.0 - off * 20.0, 0.0, 1.0);
        if !things {
            wt *= 1.0 - clamp(textureSampleLevel(surface, samp, inside, 0.0).a, 0.0, 1.0);
        }
        sum += textureSampleLevel(picture, samp, inside, 0.0).rgb * wt;
        weight += wt;
    }
    return vec4<f32>(sum / max(weight, 1e-4), clamp(weight / f32(FLOOR_TAPS) * 2.0, 0.0, 1.0));
}

@fragment
fn fs_floor(in: FsIn) -> @location(0) vec4<f32> {
    let ip = vec2<i32>(in.pos.xy);
    let lit = textureLoad(picture, ip, 0);
    let s = textureLoad(surface, ip, 0);
    let p = uv_to_world(in.uv);
    let back = st.room.y;
    let f = floor_y();
    let seam = f - back * st.room.w;
    let open = 1.0 - clamp(s.a, 0.0, 1.0);
    if st.floor.x < 0.5 || p.y < seam || open <= 0.0 {
        return lit;
    }
    let k = max(st.room.w, LEAST_K);
    let mirror = st.floor.z > 1.5;
    let blur = select(max(st.more.x, 0.0), 0.0, mirror);
    // In front of the picture plane the floor mirrors what stands on it; behind it, the wall.
    var refl = vec4<f32>(0.0);
    // Interleaved gradient noise (Jimenez): a different turn of the blur's taps at each pixel.
    let turn = 6.2831853 * fract(52.9829189 * fract(dot(in.pos.xy, vec2<f32>(0.06711056, 0.00583715))));
    let wall_q = vec2<f32>(p.x, 2.0 * seam - p.y);
    let wall = lit_near(wall_q, blur * (p.y - seam) * 0.5, turn, false);
    if p.y >= f {
        let q = vec2<f32>(p.x, 2.0 * f - p.y);
        let near = lit_near(q, blur * (p.y - f) * 0.5, turn, true);
        let held = textureSampleLevel(surface, samp, clamp(world_to_uv(q), vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).a;
        refl = mix(wall, near, clamp(held, 0.0, 1.0));
    } else {
        refl = wall;
    }
    // Fresnel at the floor's one viewing angle.
    let cosine = k / sqrt(1.0 + k * k);
    var amount = 0.04 + 0.96 * pow(1.0 - cosine, 5.0);
    if mirror {
        amount = 0.9;
    }
    amount *= max(st.more.y, 0.0) * open * refl.a;
    return vec4<f32>(mix(lit.rgb, refl.rgb, clamp(amount, 0.0, 1.0)), lit.a);
}


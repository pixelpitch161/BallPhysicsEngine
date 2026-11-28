// ---- forces (Water mode part 4b, drops/forces.rs) -------------------------------------------

const TAU: f32 = 6.28318531;
// A spike's tooth is (1 - |x|) cubed, x the turn from its middle over its half width, a share
// SPIKE_WIDTH of the turn between two spikes; its point is rounded off within SPIKE_ROUND of the
// middle. A magnet's depth is held to MOST_DEPTH, and to MOST_STEEP times a tooth's half width
// (radians), so narrow teeth are no steeper than wide ones (forces.rs).
const SPIKE_WIDTH: f32 = 0.5;
const SPIKE_ROUND: f32 = 0.1;
const MOST_DEPTH: f32 = 0.9;
const MOST_STEEP: f32 = 3.9;
// The teeth start and are whole at these shares of the reach.
const SPIKE_FROM: f32 = 0.2;
const SPIKE_WHOLE: f32 = 0.45;
// A whirlpool's core and where it starts fading out, as shares of its reach.
const SPIN_CORE: f32 = 0.3;
const SPIN_FADE: f32 = 0.7;
// Fullness: at and above the first a drop meets no air, at and below the second it is all in the air.
const OPEN_FULL: f32 = 0.75;
const OPEN_LONE: f32 = 0.35;

// A magnet's pull on a drop `d` (cells) from its middle: down the slope of -A R Psi(u / s), u = r / R,
// Psi = (1 - v^2)^3, its reach's share s = 1 - depth c(u) (1 - tooth) (forces.rs).
fn magnet_push(f: Force, d: vec2<f32>) -> vec2<f32> {
    let reach = f.a.z;
    let r = length(d);
    if !(reach > 0.0) || r >= reach || r < 1e-6 * reach {
        return vec2<f32>(0.0);
    }
    let u = r / reach;
    var share = 1.0;
    var along = 0.0;
    var round_slope = 0.0;
    let n = min(f.c.z, 64u);
    let spread = clamp(f.b.w, 0.0, TAU);
    let depth = min(clamp(f.b.y, 0.0, MOST_DEPTH), MOST_STEEP * SPIKE_WIDTH * spread / f32(max(n, 1u)));
    if n > 0u && depth > 0.0 && spread > 0.0 {
        let t0 = clamp((u - SPIKE_FROM) / (SPIKE_WHOLE - SPIKE_FROM), 0.0, 1.0);
        let c = t0 * t0 * (3.0 - 2.0 * t0);
        var dc = 0.0;
        if u > SPIKE_FROM && u < SPIKE_WHOLE {
            dc = 6.0 * t0 * (1.0 - t0) / (SPIKE_WHOLE - SPIKE_FROM);
        }
        var th = atan2(d.y, d.x) - f.b.z;
        th = th - TAU * floor((th + PI) / TAU);
        // (Past the spread, the valleys' reach.)
        var x = 1.0;
        let span = spread / f32(n);
        if abs(th) <= spread * 0.5 {
            let t = (th + spread * 0.5) / span;
            let k = min(floor(t), f32(n) - 1.0);
            x = clamp((t - k - 0.5) / SPIKE_WIDTH, -1.0, 1.0);
        }
        let w = SPIKE_WIDTH * span;
        // (|x| rounded off within SPIKE_ROUND of the middle, so the pull doesn't flip across it.)
        let soft = sqrt(x * x + SPIKE_ROUND * SPIKE_ROUND);
        let whole = sqrt(1.0 + SPIKE_ROUND * SPIKE_ROUND) - SPIKE_ROUND;
        let tt = max(1.0 - (soft - SPIKE_ROUND) / whole, 0.0);
        let tooth = tt * tt * tt;
        share = 1.0 - depth * c * (1.0 - tooth);
        along = -depth * dc * (1.0 - tooth);
        round_slope = depth * c * (-3.0 * tt * tt * x / (soft * whole * w));
    }
    let v = u / share;
    if v >= 1.0 {
        return vec2<f32>(0.0);
    }
    let q = 1.0 - v * v;
    let dpsi = -6.0 * v * q * q;
    let out = d / r;
    let round = vec2<f32>(-out.y, out.x);
    let shares = share * share;
    // (Its size and its way apart, so a huge pull holds at the most push instead of overflowing.)
    let k = f.b.x * dpsi;
    let g = (1.0 / share - u * along / shares) * out - (round_slope / shares) * round;
    let size = length(g);
    // (The card sends only magnets with finite numbers, so `k` is a number, if maybe a huge one.)
    if !(size > 0.0 && size < 1e30) {
        return vec2<f32>(0.0);
    }
    if abs(k) * size > f.a.w {
        return g * (sign(k) * f.a.w / size);
    }
    return g * k;
}

// A whirlpool draws the speed `v` of a drop `d` from its middle round it toward its spin.
fn spin(f: Force, d: vec2<f32>, v: vec2<f32>, dt: f32) -> vec2<f32> {
    let reach = f.a.z;
    let r = length(d);
    if !(reach > 0.0) || r >= reach || r < 1e-6 * reach {
        return v;
    }
    let x = r / (SPIN_CORE * reach);
    let want = f.b.x * 2.0 * x / (1.0 + x * x);
    let fade = 1.0 - smoothstep(SPIN_FADE, 1.0, r / reach);
    let round = vec2<f32>(-d.y, d.x) / r;
    let keep = exp(-max(f.b.y, 0.0) * fade * dt);
    return v + (want - dot(v, round)) * (1.0 - keep) * round;
}

// Wind draws the speed `v` of a drop whose neighbourhood is `full` toward its own, by how much of
// the drop meets the air.
fn blow(f: Force, v: vec2<f32>, full: f32, dt: f32) -> vec2<f32> {
    let open = 1.0 - smoothstep(OPEN_LONE, OPEN_FULL, full);
    let keep = exp(-max(f.b.z, 0.0) * open * dt);
    return f.b.xy + (v - f.b.xy) * keep;
}

// Whether grid point `q` lies in the reach of a magnet that moves any of `liquids` (bit k: liquid k).
// There that water is the drops' own, not the deep part's (deep.wgsl): the deep water rests no fuller
// than the drops just above it, and a magnet presses them fuller than that, so its pressure stirred the
// mound. (Other liquids keep the deep part: a deep tank of water with a magnet for the ferrofluid on its
// floor sank 10 px and stirred at 20 px/s when the whole reach was the drops' own.)
fn in_magnet(q: vec2<f32>, liquids: u32) -> bool {
    let n = min(fr.forces_n.x, 16u);
    for (var k = 0u; k < n; k = k + 1u) {
        let f = fr.forces[k];
        if f.c.x == 1u && (f.c.y & liquids) != 0u && length(q - f.a.xy) < f.a.z {
            return true;
        }
    }
    return false;
}

// Drop `i` at `x` moving at `vin`, of `kind` (liquid), after the step's forces: the magnets' pulls
// first, then each whirlpool and wind in the list's order.
fn forced(i: u32, x: vec2<f32>, vin: vec2<f32>, kind: u32, dt: f32) -> vec2<f32> {
    let me = 1u << (kind & 15u);
    let n = min(fr.forces_n.x, 16u);
    var a = vec2<f32>(0.0);
    for (var k = 0u; k < n; k = k + 1u) {
        let f = fr.forces[k];
        if f.c.x == 1u && (f.c.y & me) != 0u {
            a = a + magnet_push(f, x - f.a.xy);
        }
    }
    var v = vin + a * dt;
    for (var k = 0u; k < n; k = k + 1u) {
        let f = fr.forces[k];
        if (f.c.y & me) == 0u {
            continue;
        }
        if f.c.x == 2u {
            v = spin(f, x - f.a.xy, v, dt);
        } else if f.c.x == 3u {
            v = blow(f, v, nf[i].z, dt);
        }
    }
    if !finite2(v) {
        return vin;
    }
    return v;
}

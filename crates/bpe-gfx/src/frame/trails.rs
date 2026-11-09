//! Trails and streaks: strips of vertices with round outer corners.

use super::*;

/// One corner of a trail strip.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct TrailVertex {
    pub position: [f32; 2],
    /// Signed distance from the centre line, and the half width at this point.
    pub edge: [f32; 2],
    /// Linear light; alpha -1 is additive (the light itself fades along the streak).
    pub color: [f32; 4],
    /// How soft its edges are and how much of it shows (see [`StreakStyle`]).
    pub style: [f32; 2],
    /// Its layer's Lit and how much it blocks light (read only when the scene is lit).
    pub light: [f32; 2],
    /// The light it gives off, straight linear rgb at full cover (read only in the emission
    /// pass); w spare.
    pub emit: [f32; 4],
    /// Lighting part 4, as [`LayerGpu::more`]: UV glow, only under UV, catches beams, link number.
    pub extra: [f32; 4],
}

/// How a streak's edges look: its own, a soft glow, or only a band along its edge.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StreakStyle {
    /// 0 keeps a crisp one-pixel edge; 1 is soft from the centre line out to twice the width.
    pub softness: f32,
    /// 0 fills it; otherwise only the outer `band` share of its half width shows (an outline).
    pub band: f32,
}

/// Appends a strip through `points` (newest first) as a triangle list, two triangles per
/// segment, `half_widths[i]` wide and coloured `colors[i]` at `points[i]` (a point past the end
/// of either takes the last one), drawn as `style` says. Nothing fades or narrows here: the
/// caller has done that. `pixel` is a pixel in world units, the anti-aliasing margin each edge
/// keeps. Fewer than two points draw nothing.
pub fn push_trail(
    out: &mut Vec<TrailVertex>,
    points: &[Vec2],
    half_widths: &[f32],
    colors: &[[f32; 4]],
    style: StreakStyle,
    pixel: f32,
) {
    let (Some(&w), Some(&c)) = (half_widths.last(), colors.last()) else { return };
    strip(
        out,
        points,
        |i| half_widths.get(i).copied().unwrap_or(w),
        |i| colors.get(i).copied().unwrap_or(c),
        style,
        pixel,
    );
}

/// Appends a streak through `points` (newest first) as a triangle list: two triangles per
/// segment. The width shrinks from `half_width` at the head to `half_width * (1 - taper)` at the
/// tail, and the colour fades out along it. `pixel` is a pixel in world units, the anti-aliasing
/// margin each edge keeps. Fewer than two points draw nothing.
pub fn push_streak(
    out: &mut Vec<TrailVertex>,
    points: &[Vec2],
    half_width: f32,
    taper: f32,
    color: [f32; 4],
    pixel: f32,
) {
    streak(out, points, half_width, taper, |_| color, pixel);
}

/// [`push_streak`] with a colour for each point, `colors[i]` at `points[i]`; each fades out along
/// the streak as a single colour would. A point past the end of `colors` takes the last one.
pub fn push_streak_colours(
    out: &mut Vec<TrailVertex>,
    points: &[Vec2],
    half_width: f32,
    taper: f32,
    colors: &[[f32; 4]],
    pixel: f32,
) {
    let Some(&last) = colors.last() else { return };
    streak(out, points, half_width, taper, |i| colors.get(i).copied().unwrap_or(last), pixel);
}

pub(super) fn streak(
    out: &mut Vec<TrailVertex>,
    points: &[Vec2],
    half_width: f32,
    taper: f32,
    color_at: impl Fn(usize) -> [f32; 4],
    pixel: f32,
) {
    let n = points.len();
    if n < 2 || half_width <= 0.0 {
        return;
    }
    let taper = taper.clamp(0.0, 1.0);
    let along = |i: usize| i as f32 / (n - 1) as f32;
    let width_at = |i: usize| half_width * (1.0 - along(i) * taper);
    let faded = |i: usize| {
        let fade = (1.0 - along(i)) * (1.0 - along(i));
        let color = color_at(i);
        if color[3] < 0.0 {
            [color[0] * fade, color[1] * fade, color[2] * fade, -1.0]
        } else {
            [color[0], color[1], color[2], color[3] * fade]
        }
    };
    strip(out, points, width_at, faded, StreakStyle::default(), pixel);
}

/// The strip under [`push_trail`] and [`push_streak`]: each point `width_at` wide and `color_at`.
pub(super) fn strip(
    out: &mut Vec<TrailVertex>,
    points: &[Vec2],
    width_at: impl Fn(usize) -> f32,
    color_at: impl Fn(usize) -> [f32; 4],
    style: StreakStyle,
    pixel: f32,
) {
    let n = points.len();
    if n < 2 {
        return;
    }
    let (softness, band) = (style.softness.clamp(0.0, 1.0), style.band.clamp(0.0, 1.0));
    // Each segment's direction, worked out once: every segment's corners need its neighbours' too.
    let directions: Vec<Option<Vec2>> = points.windows(2).map(|w| (w[1] - w[0]).try_normalize()).collect();
    let direction = |i: usize| directions[i];
    // At a gentle bend the two segments share a side vector so the strip has no gaps. At a sharp
    // bend (a bounce) each segment keeps its own, which avoids a folded spike at the corner.
    let side_at = |i: usize, own: Vec2| {
        let neighbour = if i == 0 || i + 1 >= n {
            None
        } else if own == direction(i).unwrap_or(own) {
            direction(i - 1)
        } else {
            direction(i)
        };
        match neighbour {
            Some(other) if other.dot(own) > 0.5 => (other + own).normalize().perp(),
            _ => own.perp(),
        }
    };
    let vertex = |i: usize, side: Vec2, sign: f32| {
        let width = width_at(i).max(0.0);
        // A soft edge reaches out past the width.
        let reach = width * (1.0 + softness) + pixel;
        TrailVertex {
            position: (points[i] + side * reach * sign).into(),
            edge: [reach * sign, width],
            color: color_at(i),
            style: [softness, band],
            light: [0.0; 2],
            emit: [0.0; 4],
            extra: [0.0; 4],
        }
    };
    // Sharp corners (bounces) get a join: both segments meet at the inner mitre point (so the
    // strip doesn't cover the inside of the corner twice) and the outside is filled round (so it
    // has no notch). Each join triangle's edge values are true distances from one segment's
    // centre line, so the shader's antialiasing and soft edges carry on across it.
    let corners: Vec<Option<Corner>> = (0..n)
        .map(|i| {
            if i == 0 || i + 1 >= n {
                return None;
            }
            let (d1, d2) = (direction(i - 1)?, direction(i)?);
            if d1.dot(d2) > 0.5 {
                return None;
            }
            let (n1, n2) = (d1.perp(), d2.perp());
            let s = if d2.dot(n1) > 0.0 { 1.0 } else { -1.0 };
            let reach = width_at(i).max(0.0) * (1.0 + softness) + pixel;
            let denom = 1.0 + n1.dot(n2);
            let m = (n1 + n2) * (s * reach / denom.max(1e-6));
            let (before, after) = (points[i].distance(points[i - 1]), points[i + 1].distance(points[i]));
            let mitre =
                (denom > 1e-3 && -m.dot(d1) <= before * 0.999 && m.dot(d2) <= after * 0.999).then_some(points[i] + m);
            Some(Corner { inner: s, mitre, n1, n2, reach })
        })
        .collect();
    let joined = |i: usize, v: TrailVertex, sign: f32| match corners[i] {
        Some(Corner { inner, mitre: Some(m), .. }) if sign == inner => TrailVertex { position: m.into(), ..v },
        _ => v,
    };
    for i in 0..n - 1 {
        let Some(own) = direction(i) else { continue };
        let (s0, s1) = (side_at(i, own), side_at(i + 1, own));
        let (a, b) = (joined(i, vertex(i, s0, 1.0), 1.0), joined(i, vertex(i, s0, -1.0), -1.0));
        let (c, d) = (joined(i + 1, vertex(i + 1, s1, 1.0), 1.0), joined(i + 1, vertex(i + 1, s1, -1.0), -1.0));
        out.extend_from_slice(&[a, b, c, b, d, c]);
    }
    for (i, corner) in corners.iter().enumerate() {
        let Some(k) = corner else { continue };
        let p = points[i];
        let base = vertex(i, k.n1, 1.0);
        let at = |position: Vec2, x: f32| TrailVertex { position: position.into(), edge: [x, base.edge[1]], ..base };
        let (u0, u1) = (-k.n1 * k.inner, -k.n2 * k.inner);
        let middle = at(p, 0.0);
        if let Some(m) = k.mitre {
            // The two triangles between each segment's own outer corner, the middle and the mitre.
            out.extend_from_slice(&[at(p + u0 * k.reach, -k.reach), middle, at(m, k.reach)]);
            out.extend_from_slice(&[at(p + u1 * k.reach, -k.reach), middle, at(m, k.reach)]);
        }
        // A fan round the outside, always turning the inside's way (on an exact reversal the turn's
        // sign is the sign of zero), at most 30 degrees a step (within 0.6 px of round at 20 px
        // wide). The same count whatever the width, so a trail's sub-layers match it vertex for
        // vertex, and few, so a bouncy crowd keeps within the trails' budget.
        let turn = k.inner * u0.angle_to(u1).abs();
        let steps = ((turn.abs() / 0.53).ceil() as usize).clamp(1, 6);
        let mut last = u0;
        for s in 1..=steps {
            let next = Vec2::from_angle(turn * s as f32 / steps as f32).rotate(u0);
            out.extend_from_slice(&[middle, at(p + last * k.reach, k.reach), at(p + next * k.reach, k.reach)]);
            last = next;
        }
    }
}

/// A sharp corner of a strip (see [`strip`]): which side is inside (+1 or -1 times the side
/// vectors), where the two inner edges meet (when that falls within both segments), the two
/// segments' side vectors and how far the strip reaches out there.
#[derive(Clone, Copy)]
pub(super) struct Corner {
    pub(super) inner: f32,
    pub(super) mitre: Option<Vec2>,
    pub(super) n1: Vec2,
    pub(super) n2: Vec2,
    pub(super) reach: f32,
}

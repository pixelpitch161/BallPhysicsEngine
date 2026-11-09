//! The shared list of gradient shapes: how a gradient that fills an area (inside a ball, over a
//! shape, across the background) lays its colours out. One list for every such site; its numbers
//! are the gradient's companion settings (`"<k>~form"`, `"<k>~sides"`, ... in
//! [`crate::gradient_extras`]), copied onto the gradient as its draw-time [`Layout`] once a frame.
//!
//! A form turns a point `q` into a place along the gradient. `q` is in the site's own units
//! (ball units inside a ball, -1 to 1 across it, y down; shape units in a shape) after the
//! site's centre, turn and size: `q = rotate(u - centre, -turn) * k`. The same maths runs on the
//! GPU in `bpe-gfx/src/shaders/paint.wgsl`; the noise is the same hash on both.

use glam::Vec2;

use crate::dice::{pcg32, unit24_closed};
use crate::{Choice, Gradient, Mix};

/// How a gradient lays its colours out over an area.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum GradientForm {
    /// Straight across, at the turn.
    #[default]
    Straight,
    /// Outward from the centre (across an outline's ring, out from a glow's edge).
    Round,
    /// Round the centre, from the turn.
    Sweep,
    /// Straight bands, back and forth.
    Stripes,
    /// Round bands, back and forth.
    Rings,
    /// Once round the outline, clockwise from the top.
    AlongTheEdge,
    /// Outward in a diamond.
    Diamond,
    /// Outward in a polygon or a star.
    Polygon,
    /// Round and out at once, in arms.
    Spiral,
    /// A sweep folded back on itself.
    Kaleido,
    /// Round, from a bright point moved off the middle.
    OffCentre,
    /// Straight, bent by waves.
    Waves,
    /// Straight, bent by noise.
    Marble,
    /// Each stop a dot placed in the area; colours blend by how near they are.
    FreePoints,
    /// Rings out from every ball and shape.
    Ripples,
    /// Noise that drifts over time.
    Turbulence,
}

impl GradientForm {
    /// Its pattern number in paint.wgsl.
    pub fn code(self) -> f32 {
        match self {
            Self::Straight => 1.0,
            Self::Round => 2.0,
            Self::Sweep => 3.0,
            Self::AlongTheEdge => 4.0,
            Self::Stripes => 5.0,
            Self::Rings => 6.0,
            Self::Diamond => 13.0,
            Self::Polygon => 14.0,
            Self::Spiral => 15.0,
            Self::Kaleido => 16.0,
            Self::OffCentre => 17.0,
            Self::Waves => 18.0,
            Self::Marble => 19.0,
            Self::FreePoints => 20.0,
            Self::Ripples => 21.0,
            Self::Turbulence => 22.0,
        }
    }

    /// Every form, in the order the editor lists them.
    pub const ALL: [GradientForm; 16] = [
        Self::Straight,
        Self::Round,
        Self::Sweep,
        Self::Stripes,
        Self::Rings,
        Self::AlongTheEdge,
        Self::Diamond,
        Self::Polygon,
        Self::Spiral,
        Self::Kaleido,
        Self::OffCentre,
        Self::Waves,
        Self::Marble,
        Self::FreePoints,
        Self::Ripples,
        Self::Turbulence,
    ];

    /// The words its chip shows.
    pub fn label(self) -> &'static str {
        match self {
            Self::Straight => "Straight",
            Self::Round => "Round",
            Self::Sweep => "Sweep",
            Self::Stripes => "Stripes",
            Self::Rings => "Rings",
            Self::AlongTheEdge => "Along the edge",
            Self::Diamond => "Diamond",
            Self::Polygon => "Polygon / star",
            Self::Spiral => "Spiral",
            Self::Kaleido => "Kaleido sweep",
            Self::OffCentre => "Off-centre round",
            Self::Waves => "Waves",
            Self::Marble => "Marble",
            Self::FreePoints => "Free points",
            Self::Ripples => "Ripples from things",
            Self::Turbulence => "Turbulence",
        }
    }

    /// Whether, on Clamp, it runs back and forth instead of stopping at the last colour (it never
    /// reaches an end of its own).
    pub fn mirrors_on_clamp(self) -> bool {
        matches!(self, Self::Stripes | Self::Rings | Self::AlongTheEdge | Self::Ripples)
    }
}

/// What happens past the end of the gradient.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum Repeat {
    #[default]
    Clamp,
    Repeat,
    Mirror,
}

impl Repeat {
    /// Its number in paint.wgsl.
    pub fn code(self) -> f32 {
        match self {
            Self::Clamp => 0.0,
            Self::Repeat => 1.0,
            Self::Mirror => 2.0,
        }
    }

    /// `x` brought into 0 to 1 the way it says (as `paint_wrap` in paint.wgsl).
    pub fn wrap(self, x: f32) -> f32 {
        let t = match self {
            Self::Clamp => x,
            Self::Repeat => x - x.floor(),
            Self::Mirror => 1.0 - ((x * 0.5 - (x * 0.5).floor()) * 2.0 - 1.0).abs(),
        };
        if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 }
    }
}

/// What Ripples measure from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum RippleFrom {
    #[default]
    Everything,
    Balls,
    Shapes,
}

impl RippleFrom {
    pub fn code(self) -> f32 {
        match self {
            Self::Everything => 0.0,
            Self::Balls => 1.0,
            Self::Shapes => 2.0,
        }
    }
}

/// Whether a gradient setting fills an area, and so offers the Shape tab: `Fills` sites take
/// their place from the companions Turn, Centre, Size and Beyond the last colour; `Placed`
/// sites have placing settings of their own and take only the form and its numbers. The form
/// is the site's default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Area {
    #[default]
    No,
    Fills(GradientForm),
    Placed(GradientForm),
}

impl Area {
    pub fn is_area(self) -> bool {
        self != Self::No
    }

    pub fn fills(self) -> bool {
        matches!(self, Self::Fills(_))
    }

    /// The form a gradient of this site has when none is stored.
    pub fn default_form(self) -> GradientForm {
        match self {
            Self::No => GradientForm::Straight,
            Self::Fills(f) | Self::Placed(f) => f,
        }
    }
}

/// The most Free points: stops past this many are left out.
pub const MOST_POINTS: usize = 16;

/// The companions' defaults (see [`crate::gradient_extras::COMPANIONS`]).
pub mod defaults {
    pub const TURN: f32 = 90.0;
    pub const SIZE: f32 = 1.0;
    pub const SIDES: f32 = 5.0;
    pub const STAR: f32 = 0.0;
    pub const ARMS: f32 = 1.0;
    pub const TWIST: f32 = 1.0;
    pub const MIRRORS: f32 = 6.0;
    pub const FOCUS: [f32; 2] = [-0.35, -0.35];
    pub const STRETCH: f32 = 1.0;
    pub const STRETCH_TURN: f32 = 0.0;
    pub const COUNT: f32 = 3.0;
    pub const HEIGHT: f32 = 0.15;
    pub const FLOW: f32 = 0.0;
    pub const DETAIL: f32 = 3.0;
    pub const BEND: f32 = 0.5;
    pub const SEED: f32 = 0.0;
    pub const SOFTNESS: f32 = 2.0;
    pub const SPACING: f32 = 60.0;
    pub const SPEED: f32 = 0.2;
}

/// A gradient's shape companions as they stand at a moment: set on the gradient by
/// [`crate::gradient_extras::apply_moves`] just before drawing; never saved. The placing ones are
/// `None` when not stored, so each site falls back to its own.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub form: Option<GradientForm>,
    pub turn: Option<f32>,
    pub centre: Option<[f32; 2]>,
    pub size: Option<f32>,
    pub repeat: Option<Repeat>,
    pub sides: f32,
    pub star: f32,
    pub arms: f32,
    pub twist: f32,
    pub mirrors: f32,
    pub focus: [f32; 2],
    pub stretch: f32,
    pub stretch_turn: f32,
    pub count: f32,
    pub height: f32,
    pub flow: f32,
    pub detail: f32,
    pub bend: f32,
    pub seed: f32,
    pub softness: f32,
    pub points: Vec<[f32; 2]>,
    pub spacing: f32,
    pub from: RippleFrom,
    pub speed: f32,
}

impl Default for Layout {
    fn default() -> Self {
        use defaults::*;
        Self {
            form: None,
            turn: None,
            centre: None,
            size: None,
            repeat: None,
            sides: SIDES,
            star: STAR,
            arms: ARMS,
            twist: TWIST,
            mirrors: MIRRORS,
            focus: FOCUS,
            stretch: STRETCH,
            stretch_turn: STRETCH_TURN,
            count: COUNT,
            height: HEIGHT,
            flow: FLOW,
            detail: DETAIL,
            bend: BEND,
            seed: SEED,
            softness: SOFTNESS,
            points: Vec::new(),
            spacing: SPACING,
            from: RippleFrom::Everything,
            speed: SPEED,
        }
    }
}

impl Layout {
    /// The stored form, else `default` (the site's).
    pub fn form_or(&self, default: GradientForm) -> GradientForm {
        self.form.unwrap_or(default)
    }

    /// The form's own numbers as paint.wgsl reads them (`PaintGpu::form`), the free points' row
    /// left to the caller (`[1]` for Free points).
    pub fn numbers(&self, form: GradientForm) -> [f32; 4] {
        let f = finite;
        match form {
            GradientForm::Polygon => [f(self.sides, 5.0), f(self.star, 0.0), 0.0, 0.0],
            GradientForm::Spiral => [f(self.arms, 1.0), f(self.twist, 1.0), 0.0, 0.0],
            GradientForm::Kaleido => [f(self.mirrors, 6.0), 0.0, 0.0, 0.0],
            GradientForm::OffCentre => {
                [f(self.focus[0], 0.0), f(self.focus[1], 0.0), f(self.stretch, 1.0), f(self.stretch_turn, 0.0)]
            }
            GradientForm::Waves => [f(self.count, 3.0), f(self.height, 0.0), f(self.flow, 0.0), 0.0],
            GradientForm::Marble => [f(self.detail, 3.0), f(self.bend, 0.0), f(self.seed, 0.0), 0.0],
            GradientForm::FreePoints => [f(self.softness, 2.0), -1.0, 0.0, 0.0],
            GradientForm::Ripples => [f(self.spacing, 60.0), f(self.flow, 0.0), self.from.code(), 0.0],
            GradientForm::Turbulence => [f(self.detail, 3.0), f(self.speed, 0.0), f(self.seed, 0.0), 0.0],
            _ => [0.0; 4],
        }
    }
}

fn finite(x: f32, or: f32) -> f32 {
    if x.is_finite() { x } else { or }
}

/// `q` turned by `degrees`, from +x towards +y.
pub fn turned(q: Vec2, degrees: f32) -> Vec2 {
    Vec2::from_angle(finite(degrees, 0.0).to_radians()).rotate(q)
}

/// The seed setting as the hash takes it.
fn seed_bits(seed: f32) -> u32 {
    let s = finite(seed, 0.0).round_ties_even().clamp(-1e9, 1e9) as i64;
    s as u32
}

/// A value in -0.5 to 0.5 at lattice point (x, y) for `seed`.
fn lattice(x: i32, y: i32, seed: u32) -> f32 {
    let h = pcg32(seed ^ pcg32((x as u32) ^ pcg32(y as u32 ^ 0x9e37_79b9)));
    unit24_closed(h) - 0.5
}

/// Smooth value noise at `q`, -0.5 to 0.5.
pub fn noise(q: Vec2, seed: u32) -> f32 {
    let q = Vec2::new(finite(q.x, 0.0).clamp(-1e6, 1e6), finite(q.y, 0.0).clamp(-1e6, 1e6));
    let i = q.floor();
    let f = q - i;
    let s = f * f * (Vec2::splat(3.0) - 2.0 * f);
    let (x, y) = (i.x as i32, i.y as i32);
    let a = lattice(x, y, seed);
    let b = lattice(x.wrapping_add(1), y, seed);
    let c = lattice(x, y.wrapping_add(1), seed);
    let d = lattice(x.wrapping_add(1), y.wrapping_add(1), seed);
    let top = a + (b - a) * s.x;
    let bottom = c + (d - c) * s.x;
    top + (bottom - top) * s.y
}

/// Octaves of noise, each twice as fine and half as strong, kept to -0.5 to 0.5.
pub fn fbm(q: Vec2, octaves: f32, seed: u32) -> f32 {
    let n = finite(octaves, 3.0).round_ties_even().clamp(1.0, 8.0) as u32;
    let (mut sum, mut amp, mut total, mut at) = (0.0, 1.0, 0.0, q);
    for i in 0..n {
        sum += amp * noise(at, seed.wrapping_add(i.wrapping_mul(0x68e3_1da4)));
        total += amp;
        amp *= 0.5;
        at *= 2.0;
    }
    sum / total
}

/// How far along a polygon or star of `sides` corners (pointing up) `q` is: 1 on its outline.
fn polygon(q: Vec2, sides: f32, star: f32) -> f32 {
    let n = finite(sides, 5.0).round_ties_even().clamp(3.0, 4096.0);
    let step = std::f32::consts::PI / n;
    let inner = (step.cos() * (1.0 - finite(star, 0.0))).max(0.02);
    let theta = q.y.atan2(q.x) + std::f32::consts::FRAC_PI_2;
    let k = (theta / step).floor();
    let corner = |k: f32| {
        let a = -std::f32::consts::FRAC_PI_2 + k * step;
        let r = if (k.rem_euclid(2.0)) < 0.5 { 1.0 } else { inner };
        Vec2::from_angle(a) * r
    };
    let (v0, v1) = (corner(k), corner(k + 1.0));
    let e = v1 - v0;
    let mut normal = Vec2::new(e.y, -e.x).normalize_or_zero();
    if normal.dot(v0) < 0.0 {
        normal = -normal;
    }
    let h = normal.dot(v0).max(1e-6);
    q.dot(normal) / h
}

/// The focal radial: 0 at `focus`, 1 on the unit circle, after squashing `q` by `stretch` along
/// `stretch_turn` degrees.
fn off_centre(q: Vec2, focus: [f32; 2], stretch: f32, stretch_turn: f32) -> f32 {
    let s = finite(stretch, 1.0);
    let s = if s.abs() < 1e-3 { 1e-3f32.copysign(s) } else { s };
    let mut p = turned(q, -stretch_turn);
    p.x /= s;
    let p = turned(p, stretch_turn);
    let mut f = Vec2::new(finite(focus[0], 0.0), finite(focus[1], 0.0));
    if f.length() > 0.99 {
        f = f.normalize() * 0.99;
    }
    let d = p - f;
    let len = d.length();
    if len < 1e-6 {
        return 0.0;
    }
    let dir = d / len;
    let b = f.dot(dir);
    let c = f.length_squared() - 1.0;
    let reach = -b + (b * b - c).max(0.0).sqrt();
    len / reach.max(1e-6)
}

/// Where along the gradient `q` is for `form` with `layout`'s numbers, `k` the site's scale (what
/// Sweep multiplies its turn by), `time` seconds in. Before the site's start, scroll and repeat.
/// Free points have no place along (0); Ripples need the frame's things and fall back to Round.
pub fn along(form: GradientForm, layout: &Layout, q: Vec2, k: f32, time: f32) -> f32 {
    let q = Vec2::new(finite(q.x, 0.0), finite(q.y, 0.0));
    let time = finite(time, 0.0);
    let turns = q.y.atan2(q.x) / std::f32::consts::TAU;
    let fract = |x: f32| x - x.floor();
    let out = match form {
        GradientForm::Straight => q.x * 0.5 + 0.5,
        GradientForm::Round | GradientForm::Rings | GradientForm::Ripples => q.length(),
        GradientForm::Sweep => fract(turns) * finite(k, 1.0),
        GradientForm::Stripes => q.x * 0.5,
        // From the top at the default Turn (90°), like the shapes.
        GradientForm::AlongTheEdge => fract(turns + 0.5),
        GradientForm::Diamond => q.x.abs() + q.y.abs(),
        GradientForm::Polygon => polygon(q, layout.sides, layout.star),
        GradientForm::Spiral => {
            fract(turns * finite(layout.arms, 1.0).round_ties_even().max(1.0) + q.length() * finite(layout.twist, 0.0))
        }
        GradientForm::Kaleido => {
            let m = finite(layout.mirrors, 1.0).round_ties_even().max(1.0);
            1.0 - (2.0 * fract(turns * m) - 1.0).abs()
        }
        GradientForm::OffCentre => off_centre(q, layout.focus, layout.stretch, layout.stretch_turn),
        GradientForm::Waves => {
            let wave = (std::f32::consts::TAU
                * (finite(layout.count, 0.0) * (q.y * 0.5 + 0.5) + finite(layout.flow, 0.0) * time))
                .sin();
            q.x * 0.5 + 0.5 + finite(layout.height, 0.0) * 0.5 * wave
        }
        GradientForm::Marble => {
            q.x * 0.5 + 0.5 + finite(layout.bend, 0.0) * fbm(q * 1.5, layout.detail, seed_bits(layout.seed))
        }
        GradientForm::Turbulence => {
            let t = time * finite(layout.speed, 0.0);
            let seed = seed_bits(layout.seed);
            let p = q * 1.5;
            let w = Vec2::new(
                fbm(p + Vec2::new(0.0, t), layout.detail, seed),
                fbm(p + Vec2::new(5.2, 1.3 - t), layout.detail, seed.wrapping_add(1)),
            );
            fbm(p + 1.5 * w, layout.detail, seed.wrapping_add(2)) * 1.6 + 0.5
        }
        GradientForm::FreePoints => 0.0,
    };
    if out.is_finite() { out } else { 0.0 }
}

/// The linear-light colour `g` laid out as `form` by `layout` gives at `q` in an area's own
/// space (-1 to 1 across it), `time` seconds in, as a site that takes its place from the
/// layout (Turn, Centre, Size and Beyond the last colour) paints it; Ripples, which need the
/// frame's things, as rings from the middle. What the editor's Shape previews show.
pub fn area_colour(g: &Gradient, form: GradientForm, layout: &Layout, q: Vec2, time: f32) -> [f32; 4] {
    let turn = layout.turn.filter(|t| t.is_finite()).unwrap_or(defaults::TURN);
    let centre = layout.centre.filter(|c| c[0].is_finite() && c[1].is_finite()).unwrap_or([0.0; 2]);
    let size = layout.size.filter(|s| s.is_finite() && *s > 1e-6).unwrap_or(defaults::SIZE);
    let local = turned(q - Vec2::from(centre), -turn) / size;
    if form == GradientForm::FreePoints {
        return free_colour(g, layout, local);
    }
    let mut repeat = layout.repeat.unwrap_or_default();
    if form.mirrors_on_clamp() && repeat == Repeat::Clamp {
        repeat = Repeat::Mirror;
    }
    g.sample(repeat.wrap(along(form, layout, local, 1.0 / size, time)))
}

/// Stored Free points `points` after the gradient's stops went from `old` to `new`: a stop added
/// gets a point of its own where the defaults would put it, a removed stop's point goes, so each
/// stop keeps its point. `None` when nothing is stored or the stops are as many as before.
pub fn points_after(old: &Gradient, new: &Gradient, points: &[[f32; 2]]) -> Option<Vec<[f32; 2]>> {
    let (was, now) = (old.stops.len(), new.stops.len());
    if points.is_empty() || was == now {
        return None;
    }
    // Every stored point, one per old stop (missing ones where the defaults put them).
    let mut out: Vec<[f32; 2]> =
        (0..was).map(|i| points.get(i).copied().unwrap_or_else(|| default_point(i, was.min(MOST_POINTS)))).collect();
    let same = |a: &crate::Stop, b: &crate::Stop| a.at == b.at && a.color == b.color;
    if now > was {
        for (i, s) in new.stops.iter().enumerate() {
            if old.stops.iter().all(|o| !same(o, s)) {
                let at = i.min(out.len());
                out.insert(at, default_point(i, now.min(MOST_POINTS)));
                if out.len() == now {
                    break;
                }
            }
        }
        while out.len() < now {
            out.push(default_point(out.len(), now.min(MOST_POINTS)));
        }
    } else {
        let gone: Vec<usize> =
            (0..was).filter(|&i| new.stops.iter().all(|s| !same(s, &old.stops[i]))).take(was - now).collect();
        for &i in gone.iter().rev() {
            out.remove(i);
        }
        out.truncate(now);
    }
    out.truncate(MOST_POINTS);
    Some(out)
}

/// Where Free point `i` of `n` sits when none is stored: round a circle from the top.
pub fn default_point(i: usize, n: usize) -> [f32; 2] {
    let a = (-90.0 + 360.0 * i as f32 / n.max(1) as f32).to_radians();
    [0.6 * a.cos(), 0.6 * a.sin()]
}

/// Each Free point and its stop's colour (linear light): at most [`MOST_POINTS`], one per stop.
pub fn free_points(g: &Gradient, layout: &Layout) -> Vec<([f32; 2], [f32; 4])> {
    let n = g.stops.len().min(MOST_POINTS);
    (0..n)
        .map(|i| {
            let p = layout.points.get(i).copied().filter(|p| p[0].is_finite() && p[1].is_finite());
            (p.unwrap_or_else(|| default_point(i, n)), g.stops[i].color.to_linear())
        })
        .collect()
}

/// Whether Free points blend in OKLab (the gradient mixes evenly or round the wheel) rather
/// than in linear light.
pub fn free_in_oklab(mix: Mix) -> bool {
    matches!(mix, Mix::Even | Mix::RoundTheWheel | Mix::LongWayRound)
}

/// The linear-light colour Free points give at `q`: each stop's colour weighted by
/// `1 / (distance^softness + 1e-6)`.
pub fn free_colour(g: &Gradient, layout: &Layout, q: Vec2) -> [f32; 4] {
    let points = free_points(g, layout);
    if points.len() <= 1 {
        return g.first().to_linear();
    }
    let q = Vec2::new(finite(q.x, 0.0), finite(q.y, 0.0));
    let power = finite(layout.softness, 2.0).clamp(0.1, 64.0);
    let oklab = free_in_oklab(g.mix);
    let (mut sum, mut total) = ([0.0f32; 4], 0.0f32);
    let nearest = points
        .iter()
        .min_by(|a, b| (q - Vec2::from(a.0)).length_squared().total_cmp(&(q - Vec2::from(b.0)).length_squared()));
    let nearest = nearest.map_or(g.first().to_linear(), |p| p.1);
    for &(p, c) in &points {
        let d = (q - Vec2::from(p)).length();
        let w = 1.0 / (d.powf(power) + 1e-6);
        let rgb = [c[0], c[1], c[2]];
        let v = if oklab { crate::oklab::linear_to_oklab(rgb) } else { rgb };
        for k in 0..3 {
            sum[k] += w * v[k];
        }
        sum[3] += w * c[3];
        total += w;
    }
    if !(total.is_finite() && total > 0.0) {
        return nearest;
    }
    let mean: [f32; 4] = std::array::from_fn(|k| sum[k] / total);
    if !mean.iter().all(|v| v.is_finite()) {
        return nearest;
    }
    let rgb = [mean[0], mean[1], mean[2]];
    let rgb = if oklab { crate::oklab::oklab_to_linear(rgb).map(|x| x.max(0.0)) } else { rgb };
    [rgb[0], rgb[1], rgb[2], mean[3]]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_follow_their_stops_when_stops_come_and_go() {
        use crate::Color;
        let three = Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 255, 0), Color::rgb(0, 0, 255)]);
        let points = [[0.1, 0.1], [0.2, 0.2], [0.3, 0.3]];
        let mut two = three.clone();
        two.stops.remove(1);
        assert_eq!(points_after(&three, &two, &points), Some(vec![[0.1, 0.1], [0.3, 0.3]]), "stop 1's point goes");
        let back = points_after(&two, &three, &[[0.1, 0.1], [0.3, 0.3]]).unwrap();
        assert_eq!((back.len(), back[0], back[2]), (3, [0.1, 0.1], [0.3, 0.3]), "a new stop gets a point between");
        assert_eq!(points_after(&three, &two, &[]), None, "nothing stored, nothing to keep");
        assert_eq!(points_after(&three, &three, &points), None);
    }
    use crate::Color;

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    fn at(form: GradientForm, layout: &Layout, x: f32, y: f32) -> f32 {
        along(form, layout, Vec2::new(x, y), 1.0, 0.0)
    }

    #[test]
    fn straight_runs_across_and_round_out() {
        let l = Layout::default();
        assert!(near(at(GradientForm::Straight, &l, -1.0, 0.0), 0.0));
        assert!(near(at(GradientForm::Straight, &l, 0.0, 0.3), 0.5));
        assert!(near(at(GradientForm::Straight, &l, 1.0, 0.0), 1.0));
        assert!(near(at(GradientForm::Round, &l, 0.6, 0.8), 1.0));
        assert!(near(at(GradientForm::Diamond, &l, 1.0, 0.0), 1.0));
        assert!(near(at(GradientForm::Diamond, &l, 0.5, -0.5), 1.0));
    }

    #[test]
    fn along_the_edge_starts_at_the_top_and_goes_clockwise() {
        // At the default Turn, as a site sees it (y down).
        let l = Layout::default();
        let seen = |x: f32, y: f32| {
            let q = turned(Vec2::new(x, y), -defaults::TURN);
            at(GradientForm::AlongTheEdge, &l, q.x, q.y)
        };
        assert!(near(seen(0.0, -1.0), 0.0) || near(seen(0.0, -1.0), 1.0));
        assert!(near(seen(1.0, 0.0), 0.25));
        assert!(near(seen(0.0, 1.0), 0.5));
    }

    #[test]
    fn a_polygon_is_one_on_its_outline() {
        for n in [3usize, 5, 6] {
            for star in [0.0f32, 0.5] {
                let l = Layout { sides: n as f32, star, ..Layout::default() };
                let step = std::f32::consts::PI / n as f32;
                let inner = (step.cos() * (1.0 - star)).max(0.02);
                for k in 0..2 * n {
                    let a = -std::f32::consts::FRAC_PI_2 + k as f32 * step;
                    let r = if k % 2 == 0 { 1.0 } else { inner };
                    let v0 = Vec2::from_angle(a) * r;
                    let a1 = a + step;
                    let r1 = if k % 2 == 0 { inner } else { 1.0 };
                    let v1 = Vec2::from_angle(a1) * r1;
                    // The corner itself and the middle of the edge after it.
                    for p in [v0 * 0.9999, (v0 + v1) * 0.5] {
                        let got = at(GradientForm::Polygon, &l, p.x, p.y);
                        assert!((got - 1.0).abs() < 2e-3, "{n} sides, star {star}, corner {k}: {got}");
                    }
                }
                assert!(near(at(GradientForm::Polygon, &l, 0.0, 0.0), 0.0));
            }
        }
    }

    #[test]
    fn spiral_and_kaleido_have_no_seam() {
        let l = Layout { arms: 3.0, twist: 0.7, mirrors: 4.0, ..Layout::default() };
        for r in [0.2f32, 0.5, 0.9] {
            let above = at(GradientForm::Spiral, &l, -r, -1e-5);
            let below = at(GradientForm::Spiral, &l, -r, 1e-5);
            let gap = (above - below).abs();
            assert!(gap < 1e-3 || (1.0 - gap) < 1e-3, "spiral seam {above} {below}");
            let a = at(GradientForm::Kaleido, &l, -r, -1e-5);
            let b = at(GradientForm::Kaleido, &l, -r, 1e-5);
            assert!((a - b).abs() < 1e-3, "kaleido seam {a} {b}");
        }
        // Mirrored: the same either side of a mirror line.
        let a = at(GradientForm::Kaleido, &l, 0.5, 0.2);
        let b = at(GradientForm::Kaleido, &l, 0.5, -0.2);
        assert!(near(a, b));
    }

    #[test]
    fn off_centre_runs_from_the_focus_to_the_circle() {
        let l = Layout { focus: [0.3, -0.4], ..Layout::default() };
        assert!(near(at(GradientForm::OffCentre, &l, 0.3, -0.4), 0.0));
        for a in [0.0f32, 1.0, 2.5, 4.0] {
            let p = Vec2::from_angle(a);
            assert!(near(at(GradientForm::OffCentre, &l, p.x, p.y), 1.0), "angle {a}");
        }
        let round = Layout { focus: [0.0, 0.0], ..Layout::default() };
        assert!(near(at(GradientForm::OffCentre, &round, 0.3, 0.4), 0.5));
    }

    #[test]
    fn flat_waves_are_straight() {
        let l = Layout { height: 0.0, ..Layout::default() };
        for (x, y) in [(0.2, 0.7), (-0.9, -0.1)] {
            assert!(near(at(GradientForm::Waves, &l, x, y), at(GradientForm::Straight, &l, x, y)));
        }
        let still = Layout { bend: 0.0, ..Layout::default() };
        assert!(near(at(GradientForm::Marble, &still, 0.3, 0.1), 0.65));
    }

    #[test]
    fn noise_stays_in_range_and_repeats() {
        for i in 0..400 {
            let q = Vec2::new(i as f32 * 0.37 - 50.0, (i * 7 % 23) as f32 * 0.91 - 9.0);
            let n = noise(q, 7);
            assert!((-0.5..=0.5).contains(&n));
            assert_eq!(n, noise(q, 7));
            let f = fbm(q, 5.0, 3);
            assert!((-0.5..=0.5).contains(&f));
        }
        assert_ne!(noise(Vec2::new(0.5, 0.5), 1), noise(Vec2::new(0.5, 0.5), 2));
    }

    #[test]
    fn turbulence_drifts_with_time_and_speed() {
        let l = Layout { form: Some(GradientForm::Turbulence), speed: 0.5, ..Layout::default() };
        let q = Vec2::new(0.2, -0.3);
        let a = along(GradientForm::Turbulence, &l, q, 1.0, 0.0);
        let b = along(GradientForm::Turbulence, &l, q, 1.0, 1.0);
        assert_ne!(a, b);
        assert_eq!(a, along(GradientForm::Turbulence, &l, q, 1.0, 0.0), "pure in time");
        let still = Layout { speed: 0.0, ..l };
        assert_eq!(
            along(GradientForm::Turbulence, &still, q, 1.0, 0.0),
            along(GradientForm::Turbulence, &still, q, 1.0, 9.0)
        );
    }

    #[test]
    fn free_points_give_each_stop_its_colour_where_it_sits() {
        let red = Color::rgb(255, 0, 0);
        let blue = Color::rgb(0, 0, 255);
        let g = Gradient::even(&[red, blue]);
        let l = Layout { points: vec![[-0.5, 0.0], [0.5, 0.0]], ..Layout::default() };
        let c = free_colour(&g, &l, Vec2::new(-0.5, 0.0));
        assert!((c[0] - 1.0).abs() < 1e-3 && c[2] < 1e-3);
        let mid = free_colour(&g, &l, Vec2::new(0.0, 0.3));
        assert!((mid[0] - mid[2]).abs() < 1e-4, "half way is an even mix");
        // Missing points sit round a circle; past 16 stops are left out.
        let many = Gradient::even(&[red; 17]);
        assert_eq!(free_points(&many, &Layout::default()).len(), MOST_POINTS);
        assert_eq!(free_points(&g, &Layout::default())[1].0, default_point(1, 2));
        let one = Gradient::solid(red);
        assert_eq!(free_colour(&one, &l, Vec2::ZERO), red.to_linear());
    }

    #[test]
    fn nothing_gives_nan() {
        let odd = [0.0f32, -1.0, 1e-9, 1e9, f32::NAN, f32::INFINITY];
        let g = Gradient::even(&[Color::WHITE, Color::rgb(0, 0, 0), Color::rgb(9, 99, 200)]);
        for &x in &odd {
            let l = Layout {
                sides: x,
                star: x,
                arms: x,
                twist: x,
                mirrors: x,
                focus: [x, x],
                stretch: x,
                stretch_turn: x,
                count: x,
                height: x,
                flow: x,
                detail: x,
                bend: x,
                seed: x,
                softness: x,
                speed: x,
                ..Layout::default()
            };
            for form in GradientForm::ALL {
                for q in [Vec2::ZERO, Vec2::new(0.3, -0.7), Vec2::new(x, 0.5), Vec2::new(1e7, -1e7)] {
                    for k in [1.0, x] {
                        let a = along(form, &l, q, k, x);
                        assert!(a.is_finite(), "{form:?} at {q} with {x}");
                    }
                    let c = free_colour(&g, &l, q);
                    assert!(c.iter().all(|v| v.is_finite()), "free points at {q} with {x}");
                }
            }
        }
    }

    #[test]
    fn repeat_wraps_like_the_shader() {
        assert_eq!(Repeat::Clamp.wrap(1.4), 1.0);
        assert!(near(Repeat::Repeat.wrap(1.25), 0.25));
        assert!(near(Repeat::Mirror.wrap(1.25), 0.75));
        assert_eq!(Repeat::Mirror.wrap(f32::NAN), 0.0);
    }
}

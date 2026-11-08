//! A laser's beam, traced through the world on the CPU (lighting part 4): from the lamp along
//! its direction it meets shapes (each side of an outline a band of the wall's thickness) and
//! balls (circles), and at each one bounces like off a mirror, stops, passes through, or, for a
//! surface that answers [`Meets::Bend`], is bent going in and coming out (Snell's law, total
//! internal reflection). Glass answers `Bend` (part 2b: `push_light`'s `meets` asks
//! `surface::glass_of`). The path's corners are what the renderer draws and
//! lights from, as a strip.

use bpe_core::ThingId;
use bpe_gfx::dichroic::Dichroic;
use bpe_runtime::World;
use glam::Vec2;

use super::draw::outline_points;

/// What a beam does where it meets a thing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Meets {
    /// It bounces off like off a mirror.
    Bounce,
    /// It ends there.
    Stop,
    /// It goes straight on.
    Pass,
    /// It goes in, bent as glass of this index of refraction bends light, and out again; with a
    /// Rainbow split above 0 each colour bends by its own index (glass and light part 2), and
    /// dichroic glass lets one hue through and throws the opposite one back (part 4b).
    Bend { index: f32, rainbow: f32, dichroic: Dichroic },
}

impl Meets {
    /// Glass of `index` and Rainbow split `rainbow` that isn't dichroic.
    pub fn bend(index: f32, rainbow: f32) -> Self {
        Meets::Bend { index, rainbow, dichroic: Dichroic::default() }
    }
}

/// What a beam met at a corner of its path: a thing, for a ball its row, for a wall which side of
/// its outline, and what the beam did there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Met {
    pub thing: ThingId,
    pub row: Option<u32>,
    pub side: u32,
    pub how: Turned,
}

/// What a beam did at a corner of its path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Turned {
    /// It ended there (stopped, or out of bounces).
    Ended,
    /// It bounced off, or was all reflected inside glass.
    Reflected,
    /// It went into glass, bent.
    BentIn,
    /// It came out of glass, bent.
    BentOut,
}

/// What a beam met at a corner of its path. `None` at the lamp and where it ran out of length.
pub type Corner = Option<Met>;

/// The beam's path: its corners from the lamp on, what it met at each, and whether it ended on
/// something (rather than running out of length).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Path {
    pub points: Vec<Vec2>,
    pub corners: Vec<Corner>,
    pub ended_on_hit: bool,
}

/// A wall as the tracer sees it: its thing, the sides of its outline, half its band's thickness,
/// and whether it is filled (a solid: light inside it leaves only through its outer surface).
type Wall = (ThingId, Vec<(Vec2, Vec2)>, f32, bool);

/// The things a beam can meet, as the tracer sees them at one moment.
#[derive(Clone, Debug, Default)]
pub struct Scenery {
    /// Each wall.
    walls: Vec<Wall>,
    /// Each live ball: its thing, its row, its middle and radius.
    balls: Vec<(ThingId, u32, Vec2, f32)>,
}

/// The world's walls and live balls.
pub fn scenery(world: &World) -> Scenery {
    let mut walls = Vec::new();
    for (&id, wall) in &world.walls {
        let points = outline_points(&wall.outline);
        let n = points.len();
        if n < 2 {
            continue;
        }
        let sides = (0..n).map(|k| (points[k], points[(k + 1) % n])).collect();
        // A filled shape is its outline (as balls and the picture see it), with no band round it.
        let half = if wall.solid { 0.0 } else { 0.5 * wall.outline.thickness().max(0.0) };
        walls.push((id, sides, half, wall.solid));
    }
    let mut balls = Vec::new();
    for (&id, bodies) in &world.bodies {
        for row in 0..bodies.len() {
            if bodies.alive[row] {
                balls.push((id, row as u32, bodies.position[row], bodies.radius[row]));
            }
        }
    }
    Scenery { walls, balls }
}

/// Where a ray meets something: how far along, the surface's normal facing the ray, what it is.
#[derive(Clone, Copy, Debug)]
struct Hit {
    t: f32,
    normal: Vec2,
    thing: ThingId,
    /// The ball's row, for a ball.
    row: Option<u32>,
    /// Which side of a wall's outline.
    side: u32,
}

/// Rays shorter than this from where they start see nothing (so a bounce doesn't meet the
/// surface it leaves).
const EPS: f32 = 1e-3;

/// The ray `from` + t `dir` (t > `EPS`) against a circle: where it goes in (`leaving` false) or
/// comes out (true), and the outward normal there.
fn circle(from: Vec2, dir: Vec2, centre: Vec2, r: f32, leaving: bool) -> Option<(f32, Vec2)> {
    let m = from - centre;
    let b = m.dot(dir);
    let c = m.length_squared() - r * r;
    let disc = b * b - c;
    if disc < 0.0 || r <= 0.0 {
        return None;
    }
    let root = disc.sqrt();
    let t = if leaving { -b + root } else { -b - root };
    (t > EPS).then(|| (t, (from + dir * t - centre) / r))
}

/// The ray against a band of half thickness `r` round side `a`-`b` (a capsule): where it goes in
/// (`leaving` false) or comes out, and the outward normal there. On a filled wall (`fill`: the
/// side's normal away from the fill, and all the wall's sides) only the band's outer surface
/// counts: the fill is glass too, so light inside it leaves only where the whole shape ends.
fn band(
    from: Vec2,
    dir: Vec2,
    a: Vec2,
    b: Vec2,
    r: f32,
    leaving: bool,
    fill: Option<(Vec2, &[(Vec2, Vec2)])>,
) -> Option<(f32, Vec2)> {
    let outward = fill.map(|(out, _)| out);
    let along = b - a;
    let len = along.length();
    if len <= 1e-6 {
        return if outward.is_some() { None } else { circle(from, dir, a, r.max(1e-3), leaving) };
    }
    let u = along / len;
    let n = Vec2::new(-u.y, u.x);
    let mut best: Option<(f32, Vec2)> = None;
    let mut keep = |cand: Option<(f32, Vec2)>| {
        if let Some((t, normal)) = cand
            && best.is_none_or(|(bt, _)| t < bt)
        {
            best = Some((t, normal));
        }
    };
    // The two long sides, offset by the band's half thickness.
    let sides = match outward {
        Some(out) => [out, out],
        None => [n, -n],
    };
    for side in sides {
        let o = a + side * r;
        let facing = dir.dot(side);
        // Going in, the ray runs against the side's normal; coming out, with it.
        if (leaving && facing <= 1e-9) || (!leaving && facing >= -1e-9) {
            continue;
        }
        let t = (o - from).dot(side) / facing;
        if t <= EPS {
            continue;
        }
        let s = (from + dir * t - a).dot(u);
        if (0.0..=len).contains(&s) {
            keep(Some((t, side)));
        }
    }
    // Round ends (a thin line has none: its one line faces whichever way the ray comes from).
    if r > 0.0 {
        for corner in [a, b] {
            let cand = circle(from, dir, corner, r, leaving);
            // Leaving a filled shape, a corner's round end counts only where it is the shape's edge.
            if let (Some((_, sides)), true, Some((t, _))) = (fill, leaving, cand)
                && in_polygon(from + dir * t, sides)
            {
                continue;
            }
            keep(cand);
        }
    }
    best
}

/// Whether `p` is inside the closed outline made of `sides` (even-odd).
fn in_polygon(p: Vec2, sides: &[(Vec2, Vec2)]) -> bool {
    let mut inside = false;
    for &(a, b) in sides {
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

/// +1 when `sides` run anticlockwise in x-right, y-up terms (their left normals point in), -1
/// when clockwise: a filled wall's outward normal of side `u` is `turn` times `(u.y, -u.x)`.
fn turn_of(sides: &[(Vec2, Vec2)]) -> f32 {
    let area: f32 = sides.iter().map(|(a, b)| a.x * b.y - b.x * a.y).sum();
    if area >= 0.0 { 1.0 } else { -1.0 }
}

impl Scenery {
    /// Every thing it holds, walls first (balls of a group once a ball).
    pub fn things(&self) -> impl Iterator<Item = ThingId> + '_ {
        self.walls.iter().map(|w| w.0).chain(self.balls.iter().map(|b| b.0))
    }

    /// The nearest thing the ray meets within `most` (`inside`: the thing it is inside, whose
    /// surface it can only leave), leaving out `skip` (a ball the laser rides).
    fn nearest(
        &self,
        from: Vec2,
        dir: Vec2,
        most: f32,
        skip: Option<(ThingId, u32)>,
        inside: Option<(ThingId, Option<u32>)>,
    ) -> Option<Hit> {
        let mut best: Option<Hit> = None;
        let mut keep = |t: f32, normal: Vec2, thing: ThingId, row: Option<u32>, side: u32| {
            if t <= most && best.is_none_or(|b| t < b.t) {
                best = Some(Hit { t, normal, thing, row, side });
            }
        };
        for (thing, sides, r, solid) in &self.walls {
            let leaving = inside.is_some_and(|(id, row)| id == *thing && row.is_none());
            if inside.is_some() && !leaving {
                continue;
            }
            let turn = if *solid && sides.len() >= 3 { Some(turn_of(sides)) } else { None };
            for (k_side, &(a, b)) in sides.iter().enumerate() {
                let fill = turn.map(|k| {
                    let u = (b - a).normalize_or_zero();
                    (Vec2::new(u.y, -u.x) * k, sides.as_slice())
                });
                if let Some((t, n)) = band(from, dir, a, b, *r, leaving, fill) {
                    keep(t, if leaving { -n } else { n }, *thing, None, k_side as u32);
                }
            }
        }
        for &(thing, row, centre, r) in &self.balls {
            if skip == Some((thing, row)) {
                continue;
            }
            let leaving = inside == Some((thing, Some(row)));
            if inside.is_some() && !leaving {
                continue;
            }
            if let Some((t, n)) = circle(from, dir, centre, r, leaving) {
                keep(t, if leaving { -n } else { n }, thing, Some(row), 0);
            }
        }
        best
    }
}

/// `dir` reflected off a surface of normal `n`.
fn reflect(dir: Vec2, n: Vec2) -> Vec2 {
    dir - 2.0 * dir.dot(n) * n
}

/// `dir` bent through a surface of normal `n` (facing it) from a medium of index `from` into one
/// of index `into`, or `None` when it is all reflected (total internal reflection).
pub fn refract(dir: Vec2, n: Vec2, from: f32, into: f32) -> Option<Vec2> {
    let eta = from / into.max(1e-6);
    let cosi = -dir.dot(n);
    let k = 1.0 - eta * eta * (1.0 - cosi * cosi);
    (k >= 0.0).then(|| (dir * eta + n * (eta * cosi - k.sqrt())).normalize_or(dir))
}

/// The most times a beam bounces, whatever its Bounces says: each bounce is a strip light every
/// pixel of the light pass looks at, so a runaway count would stall the picture.
pub const MOST_BOUNCES: u32 = 256;

/// The most segments one light's beam is drawn with, its colours together (glass and light part
/// 2): past it each colour loses its farthest segments.
pub const MOST_BEAM_SEGMENTS: usize = 4096;

/// The most colours a beam splits into: each is a light every lit pixel looks at.
pub const MOST_COLOURS: u32 = 64;

/// How much Rainbow split 1 spreads the index of refraction from red (700 nm) to violet (400 nm):
/// on Bends light 1.5 the two leave a 60° prism 4° apart.
pub const DISPERSION: f32 = 0.0462;

/// The index of refraction glass of `index` and Rainbow split `rainbow` has for wavelength `w`
/// (-1 red, 0 the middle, 1 violet): red bends least. At `w` 0 it is `index` exactly.
pub fn index_at(index: f32, rainbow: f32, w: f32) -> f32 {
    if rainbow == 0.0 || w == 0.0 {
        return index;
    }
    (index + rainbow * DISPERSION * 0.5 * w).max(1.0)
}

/// A wavelength's colour for `w` from -1 (red) to 1 (violet): `lighting.wgsl::spectral`'s three
/// triangles (before its scale, which the shares divide out).
pub fn spectral(w: f32) -> [f32; 3] {
    let hat = |c: f32| (1.0 - (w - c).abs() / 0.6).max(0.0);
    [hat(-0.6), hat(0.0), hat(0.6)]
}

/// `n` colours across the spectrum, red first: each one's place `w` (-1 red to 1 violet) and its
/// share of white light per channel (the shares add to exactly 1 in each channel).
pub fn spectrum(n: u32) -> Vec<(f32, [f32; 3])> {
    let n = n.max(1);
    if n == 1 {
        return vec![(0.0, [1.0; 3])];
    }
    let places: Vec<f32> = (0..n).map(|k| -1.0 + (2 * k + 1) as f32 / n as f32).collect();
    let raw: Vec<[f32; 3]> = places.iter().map(|&w| spectral(w)).collect();
    let mut sum = [0.0f32; 3];
    for r in &raw {
        for c in 0..3 {
            sum[c] += r[c];
        }
    }
    places
        .into_iter()
        .zip(raw)
        .map(|(w, r)| (w, [0, 1, 2].map(|c| if sum[c] > 1e-9 { r[c] / sum[c] } else { 1.0 / n as f32 })))
        .collect()
}

/// A beam on its way: where it is, which way it goes, how much length and how many bounces it has
/// used, the glass it is inside (thing, ball row, index) and its path so far.
#[derive(Clone, Debug)]
struct Walk {
    at: Vec2,
    dir: Vec2,
    left: f32,
    turns: u32,
    inside: Option<(ThingId, Option<u32>, f32)>,
    points: Vec<Vec2>,
    corners: Vec<Corner>,
}

impl Walk {
    fn path(self, ended_on_hit: bool) -> Path {
        Path { points: self.points, corners: self.corners, ended_on_hit }
    }
}

/// Where a white beam meets glass that splits it: the walk at that point (before it bends in),
/// the glass's normal there, what it is, and its index, Rainbow split and dichroic numbers.
#[derive(Clone, Debug)]
struct Pending {
    walk: Walk,
    normal: Vec2,
    met: Met,
    index: f32,
    rainbow: f32,
    dichroic: Dichroic,
}

impl Pending {
    /// The cosine the beam meets the glass at.
    fn cos(&self) -> f32 {
        self.walk.dir.dot(self.normal).abs()
    }

    /// The walk of what dichroic glass throws back (part 4b), reflected off it.
    fn reflected(&self) -> Walk {
        let mut walk = Walk { points: vec![self.walk.at], corners: vec![None], ..self.walk.clone() };
        walk.dir = reflect(walk.dir, self.normal);
        walk.corners[0] = Some(Met { how: Turned::Reflected, ..self.met });
        walk
    }

    /// The walk of colour `w` from the split, bent into the glass with its own index.
    fn bent(&self, w: f32) -> Walk {
        let mut walk = Walk { points: vec![self.walk.at], corners: vec![None], ..self.walk.clone() };
        let n = index_at(self.index, self.rainbow, w);
        let how = match refract(walk.dir, self.normal, 1.0, n) {
            Some(into) => {
                walk.inside = Some((self.met.thing, self.met.row, n));
                walk.dir = into;
                Turned::BentIn
            }
            None => {
                walk.dir = reflect(walk.dir, self.normal);
                Turned::Reflected
            }
        };
        walk.corners[0] = Some(Met { how, ..self.met });
        walk
    }
}

/// Walks a beam of wavelength `w` on from `walk` until it ends, runs out of bounces, length or
/// `most_segments`, or (with `split`) meets glass that splits it, which it hands back unbent: glass
/// with Rainbow split when `by_colour`, and dichroic glass (part 4b).
#[allow(clippy::too_many_arguments)]
fn walk_on(
    scenery: &Scenery,
    mut walk: Walk,
    bounces: u32,
    skip: Option<(ThingId, u32)>,
    meets: &dyn Fn(ThingId, bool) -> Meets,
    w: f32,
    split: bool,
    by_colour: bool,
    most_segments: usize,
) -> (Path, Option<Pending>) {
    // A beam passing through things can meet very many: a limit keeps it from looping forever.
    for _ in 0..(4 * bounces as usize + 4096) {
        if walk.points.len() > most_segments {
            return (walk.path(false), None);
        }
        let Some(hit) = scenery.nearest(walk.at, walk.dir, walk.left, skip, walk.inside.map(|(t, r, _)| (t, r))) else {
            let end = walk.at + walk.dir * walk.left;
            walk.points.push(end);
            walk.corners.push(None);
            return (walk.path(false), None);
        };
        let point = walk.at + walk.dir * hit.t;
        walk.left -= hit.t;
        let answer = meets(hit.thing, hit.row.is_some());
        if answer == Meets::Pass && walk.inside.is_none() {
            walk.at = point;
            continue;
        }
        walk.points.push(point);
        let met = Met { thing: hit.thing, row: hit.row, side: hit.side, how: Turned::Ended };
        walk.corners.push(Some(met));
        if answer == Meets::Stop || walk.turns >= bounces {
            return (walk.path(true), None);
        }
        walk.turns += 1;
        walk.at = point;
        let (dir, how) = match (answer, walk.inside) {
            (_, Some((_, _, index))) => match refract(walk.dir, hit.normal, index, 1.0) {
                Some(out) => {
                    walk.inside = None;
                    (out, Turned::BentOut)
                }
                None => (reflect(walk.dir, hit.normal), Turned::Reflected),
            },
            (Meets::Bend { index, rainbow, dichroic }, None) => {
                if split && ((by_colour && rainbow > 0.0) || dichroic.on()) {
                    // The white path ends here (as Ended); each colour's starts here, bent its own way.
                    let rainbow = if by_colour { rainbow } else { 0.0 };
                    let pending = Pending { walk: walk.clone(), normal: hit.normal, met, index, rainbow, dichroic };
                    return (walk.path(true), Some(pending));
                }
                let n = index_at(index, rainbow, w);
                match refract(walk.dir, hit.normal, 1.0, n) {
                    Some(into) => {
                        walk.inside = Some((hit.thing, hit.row, n));
                        (into, Turned::BentIn)
                    }
                    None => (reflect(walk.dir, hit.normal), Turned::Reflected),
                }
            }
            _ => (reflect(walk.dir, hit.normal), Turned::Reflected),
        };
        walk.dir = dir;
        if let Some(Some(last)) = walk.corners.last_mut() {
            last.how = how;
        }
    }
    (walk.path(false), None)
}

/// The start of a beam from `from` along `dir`, or `None` (a path that goes nowhere) when `dir` has
/// no length.
fn start(from: Vec2, dir: Vec2, length: f32) -> Result<Walk, Path> {
    let Some(dir) = dir.try_normalize() else {
        return Err(Path { points: vec![from, from], corners: vec![None, None], ended_on_hit: false });
    };
    Ok(Walk { at: from, dir, left: length.max(0.0), turns: 0, inside: None, points: vec![from], corners: vec![None] })
}

/// The beam from `from` along `dir` for `length` in all, bouncing at most `bounces` times
/// (bends count as bounces, and never more than [`MOST_BOUNCES`]), each thing answered by
/// `meets(thing, is a ball)`. It never splits: glass bends it with its plain index.
pub fn trace(
    scenery: &Scenery,
    from: Vec2,
    dir: Vec2,
    length: f32,
    bounces: u32,
    skip: Option<(ThingId, u32)>,
    meets: &dyn Fn(ThingId, bool) -> Meets,
) -> Path {
    trace_wavelength(scenery, from, dir, length, bounces, skip, meets, 0.0)
}

/// [`trace`] for light of wavelength `w` (-1 red to 1 violet): glass with Rainbow split bends it
/// by [`index_at`].
#[allow(clippy::too_many_arguments)]
pub fn trace_wavelength(
    scenery: &Scenery,
    from: Vec2,
    dir: Vec2,
    length: f32,
    bounces: u32,
    skip: Option<(ThingId, u32)>,
    meets: &dyn Fn(ThingId, bool) -> Meets,
    w: f32,
) -> Path {
    match start(from, dir, length) {
        Ok(walk) => walk_on(scenery, walk, bounces.min(MOST_BOUNCES), skip, meets, w, false, false, usize::MAX).0,
        Err(path) => path,
    }
}

/// One colour of a split beam: its place on the spectrum, its share of the light per channel and
/// its path from where the beam split; `reflected` for what dichroic glass threw back (part 4b).
#[derive(Clone, Debug, PartialEq)]
pub struct Colour {
    pub w: f32,
    pub share: [f32; 3],
    pub path: Path,
    pub reflected: bool,
}

/// A beam that may have split into colours: the white path from the lamp (to where it split, when
/// it did) and each colour's path from there, red first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Split {
    pub white: Path,
    pub colours: Vec<Colour>,
}

/// [`trace`], splitting into `colours` colours at the first glass with Rainbow split it bends into
/// (glass and light part 2): from there each colour goes its own way. Dichroic glass (part 4b)
/// splits it too: what goes in (each colour, or the one white path) takes its hue through, and one
/// more path, last, is the opposite hue thrown back. With one colour and no dichroic glass, or no
/// such glass, it is [`trace`]'s path exactly. All the paths together have at most
/// `most_segments` segments (each path an even share of what the white path leaves).
#[allow(clippy::too_many_arguments)]
pub fn trace_split(
    scenery: &Scenery,
    from: Vec2,
    dir: Vec2,
    length: f32,
    bounces: u32,
    skip: Option<(ThingId, u32)>,
    meets: &dyn Fn(ThingId, bool) -> Meets,
    colours: u32,
    most_segments: usize,
) -> Split {
    let colours = colours.clamp(1, MOST_COLOURS);
    let bounces = bounces.min(MOST_BOUNCES);
    let walk = match start(from, dir, length) {
        Ok(walk) => walk,
        Err(white) => return Split { white, colours: Vec::new() },
    };
    let most = most_segments.max(1);
    let (white, pending) = walk_on(scenery, walk, bounces, skip, meets, 0.0, true, colours > 1, most);
    let Some(pending) = pending else { return Split { white, colours: Vec::new() } };
    let used = white.points.len().saturating_sub(1);
    let spread = if pending.rainbow > 0.0 { spectrum(colours) } else { spectrum(1) };
    let dichroic = pending.dichroic.on();
    let paths = spread.len() + usize::from(dichroic);
    let per = (most.saturating_sub(used) / paths).max(1);
    let through = if dichroic { pending.dichroic.through(pending.cos()) } else { [1.0; 3] };
    let mut out: Vec<Colour> = spread
        .into_iter()
        .map(|(w, share)| {
            let (path, _) = walk_on(scenery, pending.bent(w), bounces, skip, meets, w, false, false, per);
            let share = [0, 1, 2].map(|c| share[c] * through[c]);
            Colour { w, share, path, reflected: false }
        })
        .collect();
    if dichroic {
        let (path, _) = walk_on(scenery, pending.reflected(), bounces, skip, meets, 0.0, false, false, per);
        out.push(Colour { w: 0.0, share: pending.dichroic.bounced(pending.cos()), path, reflected: true });
    }
    Split { white, colours: out }
}

/// How bright a beam's band shows for its Beam glow: a wide band would drown the picture at a
/// laser line's brightness.
pub const BEAM_SHOW: f32 = 0.35;

/// The most ribbons one light's band is drawn with.
pub const MOST_RIBBONS: usize = 16384;

/// The most segments all of one beam's rays across are traced with, together.
pub const MOST_RIBBON_SEGMENTS: usize = 32768;

/// A Light beam's band as ribbons (glass and light part 2): between each two neighbouring rays,
/// segment by segment while they meet the same things, a quad. Over the white parts a ribbon is
/// `colour` (already times strength and glow); where both rays split at the same glass, each
/// colour's two paths make ribbons of `colour` times that colour's share. `across` is each ray's
/// place across the band (0 to 1), `gap` the space between two rays at the lamp. At most `most`.
pub fn ribbons(
    rays: &[Split],
    across: &[f32],
    gap: f32,
    colour: [f32; 3],
    softness: f32,
    most: usize,
) -> Vec<bpe_gfx::RibbonGpu> {
    let mut out = Vec::new();
    let soft = if softness.is_finite() { softness.clamp(0.0, 1.0) } else { 0.0 };
    // Each pair of rays an even share, so a band too busy for them all keeps its whole width.
    let pairs = rays.len().saturating_sub(1).max(1);
    let share = (most / pairs).max(1);
    for j in 0..rays.len().saturating_sub(1) {
        let most = (out.len() + share).min(most);
        let (a, b) = (&rays[j], &rays[j + 1]);
        let d = [across[j], across[j + 1], gap, 0.0];
        let whole = pair(&a.white, &b.white, [colour[0], colour[1], colour[2], soft], d, most, &mut out);
        let split_together = whole
            && !a.colours.is_empty()
            && a.colours.len() == b.colours.len()
            && a.white.corners.last() == b.white.corners.last();
        if !split_together {
            continue;
        }
        for (ca, cb) in a.colours.iter().zip(&b.colours) {
            let c = [colour[0] * ca.share[0], colour[1] * ca.share[1], colour[2] * ca.share[2]];
            if c.iter().all(|v| v.abs() < 1e-6) {
                continue;
            }
            pair(&ca.path, &cb.path, [c[0], c[1], c[2], soft], d, most, &mut out);
        }
    }
    out
}

/// The ribbons between paths `a` and `b`, segment by segment while both have one and each
/// segment's start met the same thing. Where the two segments end on different things (one ray
/// meets a shape the other misses) both are cut to the shorter one's length and the ribbon ends
/// there. True when it walked both to their ends (the same count, ending on the same thing).
fn pair(a: &Path, b: &Path, c: [f32; 4], d: [f32; 4], most: usize, out: &mut Vec<bpe_gfx::RibbonGpu>) -> bool {
    let (na, nb) = (a.points.len().saturating_sub(1), b.points.len().saturating_sub(1));
    for m in 0..na.min(nb) {
        if m > 0 && a.corners.get(m) != b.corners.get(m) {
            return false;
        }
        if out.len() >= most {
            return false;
        }
        let (a0, mut a1, b0, mut b1) = (a.points[m], a.points[m + 1], b.points[m], b.points[m + 1]);
        let parting = a.corners.get(m + 1) != b.corners.get(m + 1);
        if parting {
            let (la, lb) = (a0.distance(a1), b0.distance(b1));
            let t = la.min(lb);
            if la > t {
                a1 = a0 + (a1 - a0) * (t / la);
            }
            if lb > t {
                b1 = b0 + (b1 - b0) * (t / lb);
            }
        }
        out.push(bpe_gfx::RibbonGpu { a: [a0.x, a0.y, a1.x, a1.y], b: [b0.x, b0.y, b1.x, b1.y], c, d });
        if parting {
            return false;
        }
    }
    na == nb
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall_scenery(a: Vec2, b: Vec2, half: f32) -> Scenery {
        Scenery { walls: vec![(ThingId(7), vec![(a, b)], half, false)], balls: Vec::new() }
    }

    #[test]
    fn a_beam_bounces_off_a_wall_like_a_mirror() {
        let s = wall_scenery(Vec2::new(-100.0, 100.0), Vec2::new(100.0, 100.0), 5.0);
        let dir = Vec2::new(1.0, 1.0).normalize();
        let path = trace(&s, Vec2::ZERO, dir, 1000.0, 4, None, &|_, _| Meets::Bounce);
        assert_eq!(path.points.len(), 3);
        // It meets the band's near side (y = 95) and comes back up at the same angle.
        assert!((path.points[1] - Vec2::new(95.0, 95.0)).length() < 1e-3, "{:?}", path.points);
        let out = (path.points[2] - path.points[1]).normalize();
        assert!((out - Vec2::new(1.0, -1.0).normalize()).length() < 1e-4, "{out:?}");
        assert!(!path.ended_on_hit);
        let total: f32 = path.points.windows(2).map(|w| w[0].distance(w[1])).sum();
        assert!((total - 1000.0).abs() < 1e-2, "it keeps to its length: {total}");
    }

    #[test]
    fn a_beam_stops_passes_or_runs_out_of_bounces() {
        let s = wall_scenery(Vec2::new(-100.0, 100.0), Vec2::new(100.0, 100.0), 0.0);
        let down = Vec2::new(0.0, 1.0);
        let stop = trace(&s, Vec2::ZERO, down, 1000.0, 4, None, &|_, _| Meets::Stop);
        assert_eq!(stop.points, vec![Vec2::ZERO, Vec2::new(0.0, 100.0)]);
        assert!(stop.ended_on_hit);
        let pass = trace(&s, Vec2::ZERO, down, 1000.0, 4, None, &|_, _| Meets::Pass);
        assert_eq!(pass.points, vec![Vec2::ZERO, Vec2::new(0.0, 1000.0)]);
        let none = trace(&s, Vec2::ZERO, down, 1000.0, 0, None, &|_, _| Meets::Bounce);
        assert!(none.ended_on_hit && none.points.len() == 2, "no bounces left: it ends there");
    }

    #[test]
    fn a_beam_bounces_off_a_ball_and_skips_the_one_it_rides() {
        let s = Scenery { walls: Vec::new(), balls: vec![(ThingId(3), 0, Vec2::new(0.0, 100.0), 20.0)] };
        let path = trace(&s, Vec2::ZERO, Vec2::new(0.0, 1.0), 500.0, 4, None, &|_, _| Meets::Bounce);
        assert!((path.points[1] - Vec2::new(0.0, 80.0)).length() < 1e-3);
        assert!((path.points[2] - Vec2::new(0.0, -340.0)).length() < 1e-2, "straight back: {:?}", path.points);
        let ridden = trace(&s, Vec2::new(0.0, 100.0), Vec2::new(0.0, 1.0), 500.0, 4, Some((ThingId(3), 0)), &|_, _| {
            Meets::Bounce
        });
        assert_eq!(ridden.points.len(), 2, "the ball it rides doesn't stop it");
    }

    #[test]
    fn glass_bends_a_beam_in_and_out() {
        // Through a glass ball off its middle: bent in, bent out, carrying on beyond.
        let s = Scenery { walls: Vec::new(), balls: vec![(ThingId(3), 0, Vec2::new(0.0, 200.0), 50.0)] };
        let path = trace(&s, Vec2::new(25.0, 0.0), Vec2::new(0.0, 1.0), 1000.0, 8, None, &|_, _| Meets::bend(1.5, 0.0));
        assert_eq!(path.points.len(), 4, "{:?}", path.points);
        let out = (path.points[3] - path.points[2]).normalize();
        assert!(out.x < -0.1, "a glass ball focuses light toward its axis: {out:?}");
        // Head on, it goes straight through.
        let straight = trace(&s, Vec2::ZERO, Vec2::new(0.0, 1.0), 1000.0, 8, None, &|_, _| Meets::bend(1.5, 0.0));
        assert!((straight.points.last().unwrap().x).abs() < 1e-3);
    }

    #[test]
    fn steep_light_inside_glass_is_all_reflected() {
        let n = Vec2::new(0.0, -1.0);
        let dir = Vec2::new(0.9, 0.1).normalize();
        assert!(refract(dir, n, 1.5, 1.0).is_none(), "past the critical angle");
        assert!(refract(Vec2::new(0.0, 1.0), n, 1.5, 1.0).is_some());
    }
    /// An equilateral glass prism, 200 px a side, apex up, filled (`half` its band).
    fn prism(half: f32) -> Scenery {
        let (a, b, c) = (Vec2::new(-100.0, 86.6), Vec2::new(100.0, 86.6), Vec2::new(0.0, -86.6));
        Scenery { walls: vec![(ThingId(9), vec![(a, c), (c, b), (b, a)], half, true)], balls: Vec::new() }
    }

    fn glass(rainbow: f32) -> impl Fn(ThingId, bool) -> Meets {
        move |_, _| Meets::bend(1.5, rainbow)
    }

    /// The way the last segment of `p` goes, in degrees clockwise from the right.
    fn way_out(p: &Path) -> f32 {
        let n = p.points.len();
        let d = p.points[n - 1] - p.points[n - 2];
        d.y.atan2(d.x).to_degrees()
    }

    /// From the left at the least-deviation incidence for index 1.5, aimed at the left face's middle.
    fn least_deviation() -> (Vec2, Vec2) {
        let dir = Vec2::from_angle((30.0f32 - 0.75f32.asin().to_degrees()).to_radians());
        (Vec2::new(-50.0, 0.0) - dir * 300.0, dir)
    }

    #[test]
    fn rainbow_split_off_or_one_colour_is_the_plain_path() {
        let s = prism(2.0);
        let (from, dir) = least_deviation();
        let plain = trace(&s, from, dir, 2000.0, 8, None, &glass(1.0));
        for (rainbow, colours) in [(0.0, 12), (1.0, 1)] {
            let split = trace_split(&s, from, dir, 2000.0, 8, None, &glass(rainbow), colours, MOST_BEAM_SEGMENTS);
            assert!(split.colours.is_empty(), "{rainbow} {colours}");
            if rainbow == 0.0 {
                assert_eq!(split.white, trace(&s, from, dir, 2000.0, 8, None, &glass(0.0)));
            } else {
                assert_eq!(split.white, plain);
            }
        }
        assert_eq!(plain.points.len(), 4, "in, through, out and on: {:?}", plain.points);
        assert!((way_out(&plain) - 18.59).abs() < 0.05, "least deviation leaves as it came: {}", way_out(&plain));
    }

    #[test]
    fn a_prism_splits_white_light_red_least_bent() {
        let s = prism(0.0);
        let (from, dir) = least_deviation();
        let split = trace_split(&s, from, dir, 2000.0, 8, None, &glass(1.0), 12, MOST_BEAM_SEGMENTS);
        assert_eq!(split.white.points.len(), 2, "white to the prism's face");
        assert!(split.white.ended_on_hit);
        assert_eq!(split.colours.len(), 12);
        let ways: Vec<f32> = split.colours.iter().map(|c| way_out(&c.path)).collect();
        for (c, w) in split.colours.iter().zip(&ways) {
            assert_eq!(c.path.points[0], split.white.points[1], "each colour starts where the white ends");
            assert_eq!(c.path.points.len(), 3, "through and out: {:?}", c.path.points);
            assert!(w.abs() < 30.0, "{w}");
        }
        assert!(ways.windows(2).all(|p| p[1] > p[0]), "red bends least, violet most: {ways:?}");
        let red = trace_wavelength(&s, from, dir, 2000.0, 8, None, &glass(1.0), -1.0);
        let violet = trace_wavelength(&s, from, dir, 2000.0, 8, None, &glass(1.0), 1.0);
        let spread = way_out(&violet) - way_out(&red);
        assert!((spread - 4.0).abs() < 0.25, "Rainbow split 1 spreads the spectrum 4°: {spread}");
    }

    #[test]
    fn each_colour_can_be_all_reflected_inside() {
        // Straight in at 30° to the left face: red (index about 1.42) leaves through the right face,
        // violet (about 1.58) meets it past its critical angle and turns back inside.
        let s = prism(0.0);
        let split = trace_split(&s, Vec2::new(-300.0, 0.0), Vec2::X, 2000.0, 8, None, &glass(4.0), 12, 4096);
        let out_right = Vec2::new(0.866, -0.5);
        let leaves = |c: &Colour| {
            let p = &c.path.points;
            (p[2] - p[1]).normalize().dot(out_right) > 0.0
        };
        assert!(leaves(&split.colours[0]), "{:?}", split.colours[0].path.points);
        assert!(!leaves(&split.colours[11]), "{:?}", split.colours[11].path.points);
    }

    #[test]
    fn the_segment_cap_holds_between_two_mirrors() {
        let s = Scenery {
            walls: vec![
                (ThingId(1), vec![(Vec2::new(-300.0, -400.0), Vec2::new(-300.0, 400.0))], 0.0, false),
                (ThingId(2), vec![(Vec2::new(300.0, -400.0), Vec2::new(300.0, 400.0))], 0.0, false),
                // A glass pane between them, met straight on: every colour goes back and forth for ever.
                (ThingId(3), vec![(Vec2::new(0.0, -400.0), Vec2::new(0.0, 400.0))], 10.0, false),
            ],
            balls: Vec::new(),
        };
        let meets = |thing: ThingId, _: bool| {
            if thing == ThingId(3) { Meets::bend(1.5, 1.0) } else { Meets::Bounce }
        };
        let split = trace_split(&s, Vec2::new(-250.0, 5.0), Vec2::X, 1e9, 256, None, &meets, 32, MOST_BEAM_SEGMENTS);
        let counts: Vec<usize> = split.colours.iter().map(|c| c.path.points.len() - 1).collect();
        let total = split.white.points.len() - 1 + counts.iter().sum::<usize>();
        assert_eq!(counts.len(), 32);
        assert!(total <= MOST_BEAM_SEGMENTS, "{total}");
        let (lo, hi) = (counts.iter().min().unwrap(), counts.iter().max().unwrap());
        assert!(*lo >= 1 && hi - lo <= 1, "{counts:?}");
        assert!(*lo > 100, "each colour keeps its share: {counts:?}");
    }

    #[test]
    fn the_spectrum_adds_to_white() {
        for n in 1..=MOST_COLOURS {
            let colours = spectrum(n);
            assert_eq!(colours.len(), n as usize);
            for c in 0..3 {
                let sum: f32 = colours.iter().map(|(_, s)| s[c]).sum();
                assert!((sum - 1.0).abs() < 1e-5, "{n}: channel {c} adds to {sum}");
            }
            if n > 1 {
                assert!(colours[0].1[0] > colours[0].1[2], "red first");
                assert!(colours[n as usize - 1].1[2] > colours[n as usize - 1].1[0], "violet last");
            }
        }
    }

    /// `n` rays across a band `width` wide, heading right from x 0.
    fn band_of(s: &Scenery, n: u32, width: f32, meets: &dyn Fn(ThingId, bool) -> Meets) -> (Vec<Split>, Vec<f32>) {
        let across: Vec<f32> = (0..n).map(|j| j as f32 / (n - 1) as f32).collect();
        let rays = across
            .iter()
            .map(|u| trace_split(s, Vec2::new(0.0, (u - 0.5) * width), Vec2::X, 1000.0, 8, None, meets, 12, 4096))
            .collect();
        (rays, across)
    }

    #[test]
    fn a_beam_band_is_rays_across_wide() {
        let s = Scenery::default();
        let (rays, across) = band_of(&s, 5, 160.0, &|_, _| Meets::Bounce);
        let r = ribbons(&rays, &across, 40.0, [1.0; 3], 0.3, MOST_RIBBONS);
        assert_eq!(r.len(), 4);
        assert_eq!([r[0].a[1], r[3].b[1]], [-80.0, 80.0]);
        assert!(r.iter().all(|q| q.a[2] == 1000.0 && q.c == [1.0, 1.0, 1.0, 0.3] && q.d[2] == 40.0));
    }

    #[test]
    fn a_beam_ribbon_ends_where_its_rays_part() {
        let s = Scenery {
            walls: vec![(ThingId(4), vec![(Vec2::new(200.0, -100.0), Vec2::new(200.0, -20.0))], 0.0, false)],
            balls: Vec::new(),
        };
        let (rays, across) = band_of(&s, 5, 160.0, &|_, _| Meets::Stop);
        let r = ribbons(&rays, &across, 40.0, [1.0; 3], 0.3, MOST_RIBBONS);
        assert_eq!(r.len(), 4);
        for q in &r {
            let far = q.a[2].max(q.b[2]);
            if q.d[1] <= 0.5 {
                assert!(far <= 200.0 + 1e-3, "cut at the wall: {q:?}");
            } else {
                assert_eq!(far, 1000.0);
            }
        }
    }

    #[test]
    fn a_beam_through_a_prism_makes_coloured_ribbons() {
        let s = prism(0.0);
        let (from, dir) = least_deviation();
        let side = dir.perp();
        let across: Vec<f32> = (0..6).map(|j| j as f32 / 5.0).collect();
        let rays: Vec<Split> = across
            .iter()
            .map(|u| trace_split(&s, from + side * ((u - 0.5) * 40.0), dir, 2000.0, 8, None, &glass(1.0), 12, 4096))
            .collect();
        assert!(rays.iter().all(|r| r.colours.len() == 12));
        let r = ribbons(&rays, &across, 8.0, [2.0; 3], 0.0, MOST_RIBBONS);
        assert!(r.len() > 5 * 12, "{}", r.len());
        let shares = spectrum(12);
        assert!(r.iter().any(|q| q.c[0] == 2.0 * shares[0].1[0] && q.c[2] == 0.0), "a red ribbon");
        assert!(r.iter().any(|q| q.c[2] == 2.0 * shares[11].1[2] && q.c[0] == 0.0), "a violet ribbon");
    }

    #[test]
    fn a_band_split_by_a_prisms_corner_parts_there() {
        // Down onto the apex: the two left rays go into the left face, the two right ones the right.
        let s = prism(0.0);
        let across: Vec<f32> = (0..4).map(|j| j as f32 / 3.0).collect();
        let rays: Vec<Split> = across
            .iter()
            .map(|u| {
                let from = Vec2::new((u - 0.5) * 30.0, -300.0);
                trace_split(&s, from, Vec2::Y, 1000.0, 8, None, &glass(0.0), 1, 4096)
            })
            .collect();
        let r = ribbons(&rays, &across, 10.0, [1.0; 3], 0.0, MOST_RIBBONS);
        let middle: Vec<_> = r.iter().filter(|q| q.d[0] == across[1]).collect();
        assert_eq!(middle.len(), 1, "the middle pair stops at the faces: {middle:?}");
        assert!(r.iter().filter(|q| q.d[0] == across[0]).count() > 1, "each side's pair goes on through");
    }

    #[test]
    fn dichroic_glass_splits_a_beam_into_the_hue_through_and_the_opposite_thrown_back() {
        // A glass ball head on: the beam goes in, and dichroic glass throws its opposite hue back.
        let s = Scenery { walls: Vec::new(), balls: vec![(ThingId(3), 0, Vec2::new(0.0, 200.0), 50.0)] };
        let d = Dichroic { amount: 1.0, shift: 0.0, hue: 0.5 };
        let meets = move |_, _| Meets::Bend { index: 1.5, rainbow: 0.0, dichroic: d };
        let split = trace_split(&s, Vec2::new(10.0, 0.0), Vec2::new(0.0, 1.0), 1000.0, 8, None, &meets, 1, 4096);
        assert_eq!(split.white.points.len(), 2, "white to the glass: {:?}", split.white.points);
        assert_eq!(split.colours.len(), 2, "what goes through, then what is thrown back");
        let (through, back) = (&split.colours[0], &split.colours[1]);
        assert!(!through.reflected && back.reflected);
        let cos = 0.2f32.mul_add(-0.2, 1.0).sqrt();
        let near = |a: [f32; 3], b: [f32; 3]| (0..3).all(|c| (a[c] - b[c]).abs() < 1e-4);
        assert!(near(through.share, d.through(cos)), "{:?}", through.share);
        assert!(near(back.share, d.bounced(cos)), "{:?}", back.share);
        for c in 0..3 {
            assert!((through.share[c] + back.share[c] - 1.0).abs() < 1e-5, "nothing lost");
        }
        // Hue 0.5 (cyan) goes through; red bounces off.
        assert!(through.share[0] < 0.01 && through.share[1] > 0.99 && through.share[2] > 0.99);
        assert!(back.share[0] > 0.99 && back.share[1] < 0.01);
        assert_eq!(back.path.corners[0].map(|m| m.how), Some(Turned::Reflected));
        assert!(back.path.points[1].y < back.path.points[0].y, "thrown back up");
        // Dichroic 0 is the old path exactly.
        let plain = trace_split(
            &s,
            Vec2::new(10.0, 0.0),
            Vec2::new(0.0, 1.0),
            1000.0,
            8,
            None,
            &|_, _| Meets::bend(1.5, 0.0),
            1,
            4096,
        );
        assert!(plain.colours.is_empty());
        assert_eq!(
            plain.white,
            trace(&s, Vec2::new(10.0, 0.0), Vec2::new(0.0, 1.0), 1000.0, 8, None, &|_, _| { Meets::bend(1.5, 0.0) })
        );
    }

    #[test]
    fn a_dichroic_prism_splits_into_colours_and_one_thrown_back() {
        let s = Scenery { walls: Vec::new(), balls: vec![(ThingId(3), 0, Vec2::new(0.0, 200.0), 50.0)] };
        let d = Dichroic { amount: 0.5, shift: 0.2, hue: 0.1 };
        let meets = move |_, _| Meets::Bend { index: 1.5, rainbow: 1.0, dichroic: d };
        let split = trace_split(&s, Vec2::new(20.0, 0.0), Vec2::new(0.0, 1.0), 1000.0, 8, None, &meets, 6, 4096);
        assert_eq!(split.colours.len(), 7);
        assert!(split.colours[..6].iter().all(|c| !c.reflected) && split.colours[6].reflected);
        // Every channel: the colours' shares and the thrown back add to 1.
        for c in 0..3 {
            let sum: f32 = split.colours.iter().map(|k| k.share[c]).sum();
            assert!((sum - 1.0).abs() < 1e-4, "channel {c}: {sum}");
        }
        let segments: usize = split.colours.iter().map(|c| c.path.points.len() - 1).sum();
        assert!(segments + split.white.points.len() - 1 <= 4096);
    }
}

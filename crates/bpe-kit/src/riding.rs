//! One way to ride a parent (workstream W14). A light, a Particles thing or a Text inside a Balls,
//! Emitter or Shape thing (lights and particles also inside a Text) rides it: one shared Rides on
//! choice says on what, and this module says where that is now. Lights and texts take every spot
//! ([`Ridden::spots`]); particles are born at one random spot at a time ([`Ridden::pick`]).

use std::cell::OnceCell;
use std::rc::Rc;

use bpe_core::{Choice, Params, Scene, ThingId};
use bpe_runtime::{Bodies, CameraView, Wall, World};
use glam::Vec2;

use crate::outline;
use crate::things::text::{TextAt, TextNow, TextSettings, text_now};

/// What part of its parent a light, a Particles thing or a Text rides.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum RidesOn {
    /// Every ball.
    #[default]
    EachBall,
    /// The middle of the balls, of the shape or of the letters.
    TheMiddle,
    /// One ball, by its number (from 1).
    BallNumber,
    /// Each ball's rim, the shape's outline or the letters' outlines.
    TheEdge,
    /// Inside each ball, the shape or the letters (particles).
    Inside,
    /// Every letter of a Text showing now.
    EachLetter,
    /// Where a Text's Draw on is tracing now (nothing while there is none).
    ThePen,
}

/// The kinds of thing a Text can ride.
pub const RIDDEN_BY_TEXT: [&str; 3] = ["balls", "emitter", "shape"];
/// The kinds of thing a light or a Particles thing can ride.
pub const RIDDEN: [&str; 4] = ["balls", "emitter", "shape", "text"];

/// Whether a thing of kind `rider` inside a thing of kind `parent` rides it.
pub fn can_ride(rider: &str, parent: &str) -> bool {
    if rider == crate::things::text::KIND { RIDDEN_BY_TEXT.contains(&parent) } else { RIDDEN.contains(&parent) }
}

/// The Rides on choices a thing of kind `rider` inside a thing of kind `parent` is offered: what
/// makes sense there. Inside is for particles only (a lamp or a label is one point). Anything else
/// (no parent it rides) is offered every choice.
pub fn offered(rider: &str, parent: &str) -> &'static [RidesOn] {
    use RidesOn::*;
    let particles = rider == crate::things::particles::KIND;
    if !can_ride(rider, parent) {
        return &[EachBall, TheMiddle, BallNumber, TheEdge, Inside, EachLetter, ThePen];
    }
    match (parent, particles) {
        ("balls" | "emitter", true) => &[EachBall, TheMiddle, BallNumber, TheEdge, Inside],
        ("balls" | "emitter", false) => &[EachBall, TheMiddle, BallNumber, TheEdge],
        ("shape", true) => &[TheMiddle, TheEdge, Inside],
        ("shape", false) => &[TheMiddle, TheEdge],
        (_, true) => &[TheMiddle, EachLetter, TheEdge, Inside, ThePen],
        (_, false) => &[TheMiddle, EachLetter, TheEdge, ThePen],
    }
}

/// Whether thing `thing` of `scene` is offered Rides on choice `choice` (its saved name), for the
/// editor's dropdown: the parent is its tree parent, past any rigs for a light.
pub fn offers(scene: &Scene, thing: ThingId, choice: &str) -> bool {
    let Some(t) = scene.things.get(&thing) else { return true };
    let parent = if t.kind == crate::things::light::LIGHT {
        crate::things::rig::fixtures::ridden(scene, thing)
    } else {
        t.parent.filter(|p| *p != ThingId::ROOT)
    };
    let Some(parent) = parent.and_then(|p| scene.things.get(&p)) else { return true };
    RidesOn::from_name(choice).is_none_or(|c| offered(&t.kind, &parent.kind).contains(&c))
}

/// Makes thing `id`'s saved Rides on one that its parent offers (The middle when it isn't), so a
/// thing just added inside a shape or a Text says what it does. Things that don't ride are left.
pub fn fit_choice(scene: &mut Scene, id: ThingId) {
    use crate::things::{light::LIGHT, particles, text};
    let Some(t) = scene.things.get(&id) else { return };
    if ![LIGHT, particles::KIND, text::KIND].contains(&t.kind.as_str()) {
        return;
    }
    let now = match t.params.get("rides_on") {
        Some(bpe_core::Value::Choice(c)) => c.clone(),
        _ => RidesOn::default().name().to_owned(),
    };
    if !offers(scene, id, &now)
        && let Some(t) = scene.things.get_mut(&id)
    {
        t.params.set("rides_on", bpe_core::Value::Choice(RidesOn::TheMiddle.name().into()));
    }
}

/// How a thing rides: the choice, the ball it names (from 1) and its share round the edge.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ride {
    pub on: RidesOn,
    pub ball_number: u32,
    /// From 0 (the top) round to 1, clockwise: where on the edge a light or a Text sits.
    pub edge_at: f32,
}

/// One place on what a thing rides.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Spot {
    pub at: Vec2,
    /// How fast what it rides is moving there.
    pub velocity: Vec2,
    /// The way out of what it rides (away from its middle), when known.
    pub out: Option<Vec2>,
    /// The ball it rides (thing, body row).
    pub ball: Option<(ThingId, usize)>,
    /// That ball's radius (0 for anything else).
    pub radius: f32,
}

/// A box: its lowest and highest corners.
type Box2 = (Vec2, Vec2);

/// A Text being ridden, as it shows now, with its places in the world.
pub struct Letters {
    pub now: TextNow,
    /// Whether the Text is stuck to the screen (its places go through the camera).
    pub screen: bool,
    pub camera: CameraView,
    shown: OnceCell<Vec<(Vec2, Vec<Vec<Vec2>>)>>,
    /// Every shown contour together, their box and their middle, worked out once (a particle
    /// step asks for them at every birth).
    all: OnceCell<(Vec<Vec<Vec2>>, Option<Box2>)>,
    middle: OnceCell<Option<Vec2>>,
}

impl Letters {
    /// The Text at `at` as it shows now: `None` when it has no letters there.
    pub fn of(at: &TextAt) -> Option<Letters> {
        let now = text_now(at)?;
        let screen = at.tree.things.get(&at.id).is_none_or(|t| t.screen.unwrap_or(true));
        Some(Letters {
            now,
            screen,
            camera: at.world.camera,
            shown: OnceCell::new(),
            all: OnceCell::new(),
            middle: OnceCell::new(),
        })
    }

    /// A point of the Text's own frame in the world.
    pub fn world_of(&self, p: Vec2) -> Vec2 {
        let p = self.now.to_world(p);
        if self.screen { self.camera.to_world(p) } else { p }
    }

    /// Each letter showing now: its middle and its contours, in the world.
    pub fn shown(&self) -> &[(Vec2, Vec<Vec<Vec2>>)] {
        self.shown.get_or_init(|| {
            (0..self.now.layout.letters.len())
                .filter_map(|k| {
                    let m = self.now.shown.get(k).copied().flatten()?;
                    let contours: Vec<Vec<Vec2>> = self
                        .now
                        .contours(k)
                        .into_iter()
                        .filter(|c| c.len() >= 2)
                        .map(|c| c.into_iter().map(|p| self.world_of(p)).collect())
                        .collect();
                    Some((self.world_of(self.now.middle_moved(k, m)), contours))
                })
                .collect()
        })
    }

    /// The middle of its letters in the world (of every letter as placed, shown or not).
    pub fn middle(&self) -> Option<Vec2> {
        *self.middle.get_or_init(|| self.now.bounds().map(|(lo, hi)| self.world_of(0.5 * (lo + hi))))
    }

    /// Every shown letter's contours together, in the world, and their box.
    pub fn outlines(&self) -> (&[Vec<Vec2>], Option<(Vec2, Vec2)>) {
        let (all, bounds) = self.all.get_or_init(|| {
            let all: Vec<Vec<Vec2>> = self.shown().iter().flat_map(|(_, c)| c.iter().cloned()).collect();
            let bounds = outline::bounds(all.iter().flatten());
            (all, bounds)
        });
        (all, *bounds)
    }

    /// How fast the Text moves, in the world.
    pub fn velocity(&self) -> Vec2 {
        let v = self.now.pose.velocity;
        if self.screen { self.camera.to_world(v) - self.camera.to_world(Vec2::ZERO) } else { v }
    }

    /// Where its Draw on pen is now, in the world.
    pub fn pen(&self) -> Option<Vec2> {
        crate::things::text::moves::pen_of(&self.now).map(|p| self.world_of(p))
    }
}

/// A Text in `world` as its sim has it (settings and Look card as they are now, the saved scene's
/// fonts), for riders that only see the world: particles, the camera and the editor's handles. Its
/// letter moves that follow hits or sound don't move here.
pub fn letters_in_world(world: &World, id: ThingId) -> Option<Letters> {
    let tree = world.tree.as_ref()?;
    let state = world.states.get(&id).and_then(|s| s.as_any().downcast_ref::<crate::things::text::TextState>());
    let saved;
    let params: &Params = match state {
        Some(st) => &st.params,
        None => {
            saved = tree.things.get(&id)?.params.clone();
            &saved
        }
    };
    let look = state.and_then(|st| st.look.as_deref());
    Letters::of(&TextAt { look, ..TextAt::new(tree, params, world, id, world.time()) })
}

/// What a riding thing's parent is now.
pub enum Ridden<'a> {
    /// Nothing the world moves (no parent, the scene, a folder): the rider keeps its own place.
    Nothing,
    Balls {
        id: ThingId,
        bodies: &'a Bodies,
    },
    Shape {
        wall: &'a Wall,
        velocity: Vec2,
    },
    Text(Rc<Letters>),
}

/// Dice for a random spot: which ball or letter (any number), and numbers from 0 to 1 by use.
pub struct Dice<'a> {
    pub pick: usize,
    pub unit: &'a dyn Fn(u32) -> f32,
}

impl<'a> Ridden<'a> {
    /// What `parent` is in `world`; `text` reads a Text parent (once).
    pub fn of(world: &'a World, parent: Option<ThingId>, text: impl FnOnce(ThingId) -> Option<Rc<Letters>>) -> Self {
        let Some(parent) = parent.filter(|&p| p != ThingId::ROOT) else { return Ridden::Nothing };
        if let Some(bodies) = world.bodies.get(&parent) {
            return Ridden::Balls { id: parent, bodies };
        }
        if world.kind_of(parent) == Some(crate::things::text::KIND) {
            return text(parent).map_or(Ridden::Nothing, Ridden::Text);
        }
        if let Some(wall) = world.walls.get(&parent) {
            let pose = world.poses.get(&parent).copied().unwrap_or_default();
            let velocity = if wall.velocity != Vec2::ZERO { wall.velocity } else { pose.velocity };
            return Ridden::Shape { wall, velocity };
        }
        Ridden::Nothing
    }

    /// The alive balls' rows.
    fn alive(bodies: &Bodies) -> impl Iterator<Item = usize> + '_ {
        (0..bodies.len()).filter(|&r| bodies.alive[r])
    }

    fn ball(id: ThingId, b: &Bodies, row: usize, at: Vec2, out: Option<Vec2>) -> Spot {
        Spot { at, velocity: b.velocity[row], out, ball: Some((id, row)), radius: b.radius[row] }
    }

    /// The middle of the alive balls (`None` with none), moving at their mean speed.
    fn balls_middle(b: &Bodies) -> Option<Spot> {
        let n = Self::alive(b).count();
        if n == 0 {
            return None;
        }
        let (sum, vel) =
            Self::alive(b).fold((Vec2::ZERO, Vec2::ZERO), |(p, v), r| (p + b.position[r], v + b.velocity[r]));
        Some(Spot { at: sum / n as f32, velocity: vel / n as f32, ..Spot::default() })
    }

    /// The row of ball number `n` (from 1), when it is alive.
    fn numbered(b: &Bodies, n: u32) -> Option<usize> {
        b.row_of(n.saturating_sub(1)).filter(|&r| b.alive[r])
    }

    /// The middle of a Text's letters, moving with it.
    fn text_middle(t: &Letters) -> Spot {
        let at = t.middle().unwrap_or_else(|| t.world_of(Vec2::ZERO));
        Spot { at, velocity: t.velocity(), ..Spot::default() }
    }

    /// Every spot `ride` puts a lamp or a label on: `None` when the parent is nothing the world
    /// moves (the rider keeps its own place); empty when there is nowhere now (no balls left, no pen).
    pub fn spots(&self, ride: &Ride) -> Option<Vec<Spot>> {
        Some(match self {
            Ridden::Nothing => return None,
            Ridden::Balls { id, bodies: b } => match ride.on {
                RidesOn::EachBall => Self::alive(b).map(|r| Self::ball(*id, b, r, b.position[r], None)).collect(),
                RidesOn::BallNumber => Self::numbered(b, ride.ball_number)
                    .map(|r| Self::ball(*id, b, r, b.position[r], None))
                    .into_iter()
                    .collect(),
                RidesOn::TheEdge => {
                    let out = rim(ride.edge_at);
                    Self::alive(b)
                        .map(|r| Self::ball(*id, b, r, b.position[r] + out * b.radius[r], Some(out)))
                        .collect()
                }
                _ => Self::balls_middle(b).into_iter().collect(),
            },
            Ridden::Shape { wall, velocity } => {
                let at = match ride.on {
                    RidesOn::TheEdge => outline::round_one(&outline::points(&wall.outline), ride.edge_at),
                    _ => None,
                };
                let (at, out) = at.map_or((outline::centre(&wall.outline), None), |(at, out)| (at, Some(out)));
                vec![Spot { at, velocity: *velocity, out, ..Spot::default() }]
            }
            Ridden::Text(t) => {
                let velocity = t.velocity();
                let here = |at: Vec2, out: Option<Vec2>| Spot { at, velocity, out, ..Spot::default() };
                match ride.on {
                    RidesOn::EachLetter => t.shown().iter().map(|(m, _)| here(*m, None)).collect(),
                    RidesOn::TheEdge => {
                        let all: Vec<&[Vec2]> = t.outlines().0.iter().map(Vec::as_slice).collect();
                        match outline::round(&all, ride.edge_at) {
                            Some((at, out)) => vec![here(at, Some(out))],
                            None => vec![Self::text_middle(t)],
                        }
                    }
                    RidesOn::ThePen => t.pen().map(|p| here(p, None)).into_iter().collect(),
                    _ => vec![Self::text_middle(t)],
                }
            }
        })
    }

    /// One random spot `ride` lets a particle be born at, from `dice`: `None` when there is nowhere
    /// now (no balls left, no letters, no pen) and for nothing the world moves.
    pub fn pick(&self, ride: &Ride, dice: &Dice) -> Option<Spot> {
        let (u1, u2) = ((dice.unit)(1), (dice.unit)(2));
        match self {
            Ridden::Nothing => None,
            Ridden::Balls { id, bodies: b } => {
                let rows: Vec<usize> = Self::alive(b).collect();
                if rows.is_empty() {
                    return None;
                }
                let pick = rows[dice.pick % rows.len()];
                let on = |row: usize, at: Vec2, out: Option<Vec2>| Self::ball(*id, b, row, at, out);
                Some(match ride.on {
                    RidesOn::EachBall => on(pick, b.position[pick], None),
                    RidesOn::BallNumber => {
                        let row = Self::numbered(b, ride.ball_number)?;
                        on(row, b.position[row], None)
                    }
                    RidesOn::TheEdge => {
                        let out = Vec2::from_angle(u1 * std::f32::consts::TAU);
                        on(pick, b.position[pick] + out * b.radius[pick], Some(out))
                    }
                    RidesOn::Inside => {
                        let out = Vec2::from_angle(u1 * std::f32::consts::TAU);
                        on(pick, b.position[pick] + out * b.radius[pick] * u2.sqrt(), Some(out))
                    }
                    _ => {
                        let n = rows.len() as f32;
                        let at = rows.iter().map(|&r| b.position[r]).sum::<Vec2>() / n;
                        let velocity = rows.iter().map(|&r| b.velocity[r]).sum::<Vec2>() / n;
                        Spot { at, velocity, ..Spot::default() }
                    }
                })
            }
            Ridden::Shape { wall, velocity } => {
                let centre = outline::centre(&wall.outline);
                let points = outline::points(&wall.outline);
                let place = |at: Vec2, out: Option<Vec2>| Spot { at, velocity: *velocity, out, ..Spot::default() };
                Some(match ride.on {
                    RidesOn::TheEdge => match outline::round_one(&points, u1) {
                        Some((at, out)) => place(at, Some(out)),
                        None => place(centre, None),
                    },
                    RidesOn::Inside => {
                        let at = outline::bounds(&points)
                            .and_then(|(lo, hi)| tried_inside(lo, hi, dice, 8, |p| outline::inside(&points, p)))
                            .unwrap_or(centre);
                        place(at, (at - centre).try_normalize())
                    }
                    _ => place(centre, None),
                })
            }
            Ridden::Text(t) => {
                let velocity = t.velocity();
                let middle = Self::text_middle(t);
                let out = |at: Vec2| (at - middle.at).try_normalize();
                let here = |at: Vec2, out: Option<Vec2>| Spot { at, velocity, out, ..Spot::default() };
                let shown = t.shown();
                let letter = || (!shown.is_empty()).then(|| shown[dice.pick % shown.len()].0);
                match ride.on {
                    RidesOn::EachLetter => letter().map(|at| here(at, out(at))),
                    RidesOn::TheEdge => {
                        let all: Vec<&[Vec2]> = t.outlines().0.iter().map(Vec::as_slice).collect();
                        outline::round(&all, u1).map(|(at, way)| here(at, Some(way)))
                    }
                    RidesOn::Inside => {
                        let (all, bounds) = t.outlines();
                        let found =
                            bounds.and_then(|(lo, hi)| tried_inside(lo, hi, dice, 16, |p| outline::inside_any(all, p)));
                        found.or_else(letter).map(|at| here(at, out(at)))
                    }
                    RidesOn::ThePen => t.pen().map(|p| here(p, None)),
                    _ => Some(middle),
                }
            }
        }
    }

    /// How many places it rides one by one now (balls by Each ball, The edge or Inside; letters
    /// by Each letter), for a Steady rate a ball or a letter; 1 otherwise.
    pub fn count(&self, ride: &Ride) -> usize {
        match (self, ride.on) {
            (Ridden::Balls { bodies, .. }, RidesOn::EachBall | RidesOn::TheEdge | RidesOn::Inside) => {
                bodies.alive.iter().filter(|a| **a).count()
            }
            (Ridden::Text(t), RidesOn::EachLetter) => t.shown().len(),
            _ => 1,
        }
    }
}

/// The way out to share `share` round a ball's rim, from its top clockwise (as an outline's points
/// go).
pub fn rim(share: f32) -> Vec2 {
    Vec2::from_angle(-std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * share)
}

/// A random point in the box `lo`..`hi` that `inside` takes, trying `tries` times (dice 20, 21,
/// 22, …).
fn tried_inside(lo: Vec2, hi: Vec2, dice: &Dice, tries: u32, inside: impl Fn(Vec2) -> bool) -> Option<Vec2> {
    (0..tries).find_map(|k| {
        let p = lo + (hi - lo) * Vec2::new((dice.unit)(20 + 2 * k), (dice.unit)(21 + 2 * k));
        inside(p).then_some(p)
    })
}

/// What `TextSettings` and other riders read as their ride.
impl From<&TextSettings> for Ride {
    fn from(s: &TextSettings) -> Ride {
        Ride { on: s.rides_on, ball_number: s.ball_number, edge_at: s.edge_at }
    }
}

#[cfg(test)]
#[path = "riding_tests.rs"]
mod tests;

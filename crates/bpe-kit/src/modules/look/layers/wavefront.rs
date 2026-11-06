//! The Wavefront layer (`layer.wavefront`): a line through each round's live balls, in their order
//! round the round (a Balls group's balls are one round), closed when the round goes all the way
//! round. It is drawn as a trail strip, so the around sub-layers (Glow, Outline, Shadow and the
//! colour ones) work on it as on a trail.

use bpe_core::{Blend, Choice, Color, Gradient, Settings};
use bpe_gfx::Frame;
use glam::Vec2;

use super::around::AroundLayer;
use super::around_subs::{HitInfo, Pieces};
use super::trail::draw::{StripLook, strip_budget, strip_with_subs};
use super::trail::kinds::Path;
use super::trail::{AroundPlace, TrailFade, TrailLayer, TrailSource};

/// The layer kind.
pub const WAVEFRONT: &str = "layer.wavefront";

/// Whether the line closes back on its first ball.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Closed {
    /// When the round goes all the way round (an emitter firing All round, a Ring group).
    #[default]
    Auto,
    Yes,
    No,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct WavefrontLayer {
    /// Under the balls or over them.
    #[setting(label = "Where", group = "Line", simple)]
    pub place: AroundPlace,
    /// How wide the line is.
    #[setting(label = "Width", group = "Line", range(0.5, 40), unit = "px", simple, keyframe, pulse)]
    pub width: f32,
    /// Whether the line closes back on its first ball.
    #[setting(label = "Closed", group = "Line")]
    pub closed: Closed,
    /// A gap between two neighbours longer than this breaks the line there, so it doesn't cross
    /// the picture when they fly apart; 0 never breaks it.
    #[setting(label = "Break when apart", group = "Line", range(0, 4000), span(0, 1000), unit = "px")]
    pub break_apart: f32,
    /// The balls' own colours, or its own.
    #[setting(label = "Colour from", group = "Colour", simple)]
    pub source: TrailSource,
    /// Its own colours, along the line from the round's first ball to its last.
    #[setting(label = "Colours", group = "Colour", when = "source=own")]
    pub colour: Gradient,
    /// How soft its edges are.
    #[setting(label = "Softness", group = "Colour", range(0, 1))]
    pub softness: f32,
    #[setting(label = "Opacity", group = "Colour", range(0, 1), keyframe, pulse)]
    pub opacity: f32,
    /// How it mixes with what is under it.
    #[setting(label = "Blend", group = "Blending")]
    pub blend: Blend,
    #[setting(label = "Blend amount", range(0, 1), group = "Blending", keyframe)]
    pub blend_amount: f32,
}

impl Default for WavefrontLayer {
    fn default() -> Self {
        Self {
            place: AroundPlace::Behind,
            width: 2.0,
            closed: Closed::Auto,
            break_apart: 200.0,
            source: TrailSource::Ball,
            colour: Gradient::even(&[Color::WHITE, Color::WHITE]),
            softness: 0.5,
            opacity: 1.0,
            blend: Blend::Normal,
            blend_amount: 1.0,
        }
    }
}

/// What the wavefront reads about its thing's balls.
pub struct Fronts<'a> {
    pub position: &'a [Vec2],
    /// Body row `i` is live and wears the layer.
    pub shown: &'a dyn Fn(usize) -> bool,
    /// The round of row `i` (0 for a Balls group's).
    pub round: &'a dyn Fn(usize) -> u32,
    /// Row `i`'s place round its round, what the line goes through it in.
    pub order: &'a dyn Fn(usize) -> u32,
    /// Row `i`'s colour as seen (its fade in its alpha), linear light; alpha < 0 adds light.
    pub colour: &'a dyn Fn(usize) -> [f32; 4],
    /// Row `i`'s fade, 0 to 1.
    pub fade: &'a dyn Fn(usize) -> f32,
    /// Whether a round goes all the way round, for Closed Auto.
    pub all_round: bool,
    /// The size the sub-layers' reaches are shares of (the balls' Size).
    pub radius: f32,
}

/// One unbroken run of the line: rows in order, and whether it closes.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub rows: Vec<usize>,
    pub closed: bool,
}

/// The runs of line `w` draws through `f`'s balls: each round's shown rows in order, broken
/// where neighbours are further apart than Break when apart.
pub fn runs(w: &WavefrontLayer, f: &Fronts) -> Vec<Run> {
    let mut rows: Vec<usize> = (0..f.position.len()).filter(|&i| (f.shown)(i)).collect();
    rows.sort_by_key(|&i| ((f.round)(i), (f.order)(i), i));
    let close = match w.closed {
        Closed::Auto => f.all_round,
        Closed::Yes => true,
        Closed::No => false,
    };
    let apart = |a: usize, b: usize| w.break_apart > 0.0 && f.position[a].distance(f.position[b]) > w.break_apart;
    let mut out = Vec::new();
    let mut start = 0;
    while start < rows.len() {
        let round = (f.round)(rows[start]);
        let end = start + rows[start..].iter().take_while(|&&i| (f.round)(i) == round).count();
        let one = &rows[start..end];
        start = end;
        // Split where neighbours are far apart.
        let mut pieces: Vec<Vec<usize>> = vec![vec![one[0]]];
        for pair in one.windows(2) {
            if apart(pair[0], pair[1]) {
                pieces.push(Vec::new());
            }
            pieces.last_mut().expect("one at least").push(pair[1]);
        }
        let whole = pieces.len() == 1;
        if close && one.len() > 2 {
            let (first, last) = (one[0], one[one.len() - 1]);
            if whole && !apart(last, first) {
                out.push(Run { rows: pieces.pop().expect("one"), closed: true });
                continue;
            }
            if !whole && !apart(last, first) {
                // The last piece runs on into the first.
                let tail = pieces.pop().expect("two at least");
                let head = std::mem::take(&mut pieces[0]);
                pieces[0] = tail.into_iter().chain(head).collect();
            }
        }
        out.extend(pieces.into_iter().filter(|p| p.len() > 1).map(|rows| Run { rows, closed: false }));
    }
    out
}

/// `c` at `m` of its strength: light-adding colours (or an Add blend's) by their light, the rest
/// by their cover.
fn faded(c: [f32; 4], m: f32, adds: bool) -> [f32; 4] {
    if adds || c[3] < 0.0 { [c[0] * m, c[1] * m, c[2] * m, c[3]] } else { [c[0], c[1], c[2], c[3] * m] }
}

/// Draws the wavefront `w` (the around layer `layer`) through `f`'s balls into `frame`.
pub fn draw(layer: &AroundLayer, w: &WavefrontLayer, f: &Fronts, frame: &mut Frame) {
    if w.opacity <= 0.0 || w.width <= 0.0 {
        return;
    }
    // A trail standing in for the line's look: its own colours as they are, blended its way.
    let trail = TrailLayer {
        brightness: 1.0,
        fade: TrailFade::None,
        blend: w.blend,
        blend_amount: w.blend_amount,
        ..TrailLayer::default()
    };
    let look = StripLook { trail: &trail, subs: &layer.subs, hit: HitInfo::default(), radius: f.radius.max(1.0) };
    let adds = w.blend == Blend::Add;
    let mut pieces = Pieces::default();
    let mut points_left = strip_budget(&layer.subs);
    for run in runs(w, f) {
        let mut rows = run.rows;
        if run.closed {
            rows.push(rows[0]);
        }
        if rows.len() > points_left {
            break;
        }
        points_left -= rows.len();
        let n = rows.len();
        let points: Vec<Vec2> = rows.iter().map(|&i| f.position[i]).collect();
        let mut arc = Vec::with_capacity(n);
        let mut along = 0.0;
        for k in 0..n {
            if k > 0 {
                along += points[k].distance(points[k - 1]);
            }
            arc.push(along);
        }
        let path = Path { points, ago: vec![0.0; n], arc, corner: vec![false; n] };
        let widths = vec![w.width * 0.5; n];
        let bases: Vec<[f32; 4]> = rows
            .iter()
            .enumerate()
            .map(|(k, &i)| match w.source {
                TrailSource::Ball => faded((f.colour)(i), w.opacity, adds),
                TrailSource::Own => {
                    let t = if n <= 1 { 0.0 } else { k as f32 / (n - 1) as f32 };
                    faded(w.colour.sample(t), w.opacity * (f.fade)(i), adds)
                }
            })
            .collect();
        strip_with_subs(&look, &path, &widths, &bases, w.softness.clamp(0.0, 1.0), &mut pieces);
    }
    pieces.give_light(layer.emit);
    pieces.into_frame(frame, w.place == AroundPlace::InFront);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fronts<'a>(
        position: &'a [Vec2],
        round: &'a dyn Fn(usize) -> u32,
        order: &'a dyn Fn(usize) -> u32,
        all_round: bool,
    ) -> Fronts<'a> {
        Fronts {
            position,
            shown: &|_| true,
            round,
            order,
            colour: &|_| [1.0, 1.0, 1.0, 1.0],
            fade: &|_| 1.0,
            all_round,
            radius: 10.0,
        }
    }

    #[test]
    fn a_wavefront_draws_one_closed_strip_per_round_in_order() {
        // Two rounds of four round a circle, listed out of order.
        let ring = |k: u32, r: f32| Vec2::from_angle(std::f32::consts::TAU * k as f32 / 4.0) * r;
        let position = [
            ring(2, 50.0),
            ring(0, 50.0),
            ring(3, 50.0),
            ring(1, 50.0),
            ring(0, 90.0),
            ring(1, 90.0),
            ring(2, 90.0),
            ring(3, 90.0),
        ];
        let order_of = [2, 0, 3, 1, 0, 1, 2, 3];
        let round = |i: usize| if i < 4 { 1 } else { 2 };
        let order = |i: usize| order_of[i];
        let f = fronts(&position, &round, &order, true);
        let w = WavefrontLayer::default();
        let r = runs(&w, &f);
        assert_eq!(r, vec![Run { rows: vec![1, 3, 0, 2], closed: true }, Run { rows: vec![4, 5, 6, 7], closed: true }]);
        let layer = AroundLayer {
            id: bpe_core::LayerId(1),
            kind: super::super::around::AroundKind::Wavefront(Box::new(w.clone())),
            subs: Vec::new(),
            only: Vec::new(),
            light: [0.0; 4],
            emit: None,
        };
        let mut frame = Frame::default();
        draw(&layer, &w, &f, &mut frame);
        assert!(!frame.trails.is_empty());
        assert!(frame.trails_front.is_empty(), "behind the balls by default");
        let open = WavefrontLayer { closed: Closed::No, ..w };
        assert!(runs(&open, &f).iter().all(|r| !r.closed));
    }

    #[test]
    fn a_gap_wider_than_break_apart_breaks_it() {
        let position = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(500.0, 0.0), Vec2::new(510.0, 0.0)];
        let round = |_| 1;
        let order = |i: usize| i as u32;
        let f = fronts(&position, &round, &order, false);
        let w = WavefrontLayer::default();
        let r = runs(&w, &f);
        assert_eq!(r, vec![Run { rows: vec![0, 1], closed: false }, Run { rows: vec![2, 3], closed: false }]);
        let never = WavefrontLayer { break_apart: 0.0, ..w };
        assert_eq!(runs(&never, &f), vec![Run { rows: vec![0, 1, 2, 3], closed: false }]);
    }

    #[test]
    fn a_closed_line_broken_once_runs_on_from_its_last_ball_into_its_first() {
        let position = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(500.0, 0.0), Vec2::new(-10.0, 0.0)];
        let round = |_| 1;
        let order = |i: usize| i as u32;
        let f = fronts(&position, &round, &order, true);
        let r = runs(&WavefrontLayer::default(), &f);
        assert_eq!(r, vec![Run { rows: vec![3, 0, 1], closed: false }]);
    }
}

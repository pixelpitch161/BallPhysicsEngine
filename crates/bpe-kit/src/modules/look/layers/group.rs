//! Group layers: a look spread across every ball of a group. The group gradient gives each ball
//! a colour from where it sits in the group (or its number, speed, hits…) and mixes it with the
//! ball's own colours.

use bpe_core::{Blend, Choice, Color, Gradient, Settings};
use glam::Vec2;

pub use crate::things::balls::rows::RoundSpan;
use bpe_core::dice::Rng;

/// What spreads the gradient's colours across the group.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Spread {
    /// Where each ball starts in the group, along the gradient's angle.
    #[default]
    Place,
    /// The balls' numbers, first to last (round to one before the first on a looped gradient).
    Order,
    /// The balls' numbers out to the end at the middle ball and back again, so a ring's ends meet.
    ThereAndBack,
    /// How far each ball starts from the middle of the group.
    CentreOut,
    /// A colour picked at random for each ball.
    Random,
    /// How fast each ball goes.
    Speed,
    /// How many times each ball has hit something.
    Hits,
    /// Every ball round the gradient together, over time.
    Time,
    /// The emitter round each ball was fired in, first to last (every ten round and round when
    /// the rounds don't end). A Balls group's balls are all the first.
    Round,
    /// The direction each ball was fired or launched in, once round the gradient from the
    /// gradient's angle.
    LaunchAngle,
}

/// How the colours bunch along the gradient.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Easing {
    #[default]
    Even,
    EaseIn,
    EaseOut,
    EaseBoth,
}

/// How the group's colour mixes with each ball's own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum GroupMix {
    /// The group's colour instead of the ball's.
    #[default]
    Replace,
    /// The group's hue and saturation, the ball's lightness.
    Tint,
    Multiply,
    Screen,
    Overlay,
    /// Not at all: the layer does nothing.
    Off,
}

impl GroupMix {
    /// The blend that mixes it, `None` for Off.
    pub fn blend(self) -> Option<Blend> {
        match self {
            Self::Replace => Some(Blend::Normal),
            Self::Tint => Some(Blend::Colour),
            Self::Multiply => Some(Blend::Multiply),
            Self::Screen => Some(Blend::Screen),
            Self::Overlay => Some(Blend::Overlay),
            Self::Off => None,
        }
    }

    /// Its number on a ball instance (`range.w` in layers.wgsl): one more than its blend's
    /// shader code, 0 for none.
    pub fn code(self) -> u32 {
        self.blend().map_or(0, |b| b.shader_code() + 1)
    }
}

/// Which of the ball's colours the group's colour changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Changes {
    /// The Body's colour (and patterns that are a shade of it).
    #[default]
    Body,
    /// The whole finished ball, every layer.
    EveryLayer,
    /// The layers picked from a list.
    Choose,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct GroupGradient {
    /// The colours spread across the group.
    #[setting(label = "Colours", keyframe)]
    pub colours: Gradient,
    /// Which way the colours run across the group, for Place, and where Launch angle starts.
    #[setting(label = "Angle", when = "spread=place|launch_angle", span(-180, 180), unit = "°", keyframe)]
    pub angle: f32,
    /// How much of the group the gradient spans, 1 being the whole group.
    #[setting(label = "Stretch", when = "spread!=order|there_and_back|random|launch_angle", range(0.1, 4), keyframe)]
    pub stretch: f32,
    /// How the colours bunch: evenly, towards the start, towards the end, or towards both.
    #[setting(label = "Easing")]
    pub ease: Easing,
    /// How strongly they bunch.
    #[setting(label = "Easing strength", when = "ease!=even", range(1, 6))]
    pub ease_strength: f32,
    /// What spreads the colours: where each ball is, its number, speed, hits, or time.
    #[setting(label = "Spread by")]
    pub spread: Spread,
    /// How the group's colour mixes with each ball's own.
    #[setting(label = "With ball colours", group = "Mixing")]
    pub mix: GroupMix,
    /// 0 leaves the balls as they are, 1 mixes in all of it.
    #[setting(label = "Amount", group = "Mixing", when = "mix!=off", range(0, 1), keyframe, pulse)]
    pub amount: f32,
    /// Which of the ball's colours it changes.
    #[setting(label = "Colours it changes", group = "Mixing", when = "mix!=off")]
    pub changes: Changes,
    /// The layers it changes, by number, for Choose.
    #[setting(label = "Layers", group = "Mixing", when = "mix!=off and changes=choose")]
    pub chosen: String,
}

impl Default for GroupGradient {
    fn default() -> Self {
        Self {
            colours: Gradient::even(&[Color::rgb(0x3e, 0x9b, 0xff), Color::rgb(0xff, 0x3e, 0xc8)]),
            angle: 0.0,
            stretch: 1.0,
            ease: Easing::Even,
            ease_strength: 2.0,
            spread: Spread::Place,
            mix: GroupMix::Replace,
            amount: 0.8,
            changes: Changes::Body,
            chosen: String::new(),
        }
    }
}

impl GroupGradient {
    /// The layer ids named in `chosen` ("3,5"); words that aren't numbers are skipped.
    pub fn chosen_ids(&self) -> Vec<u32> {
        self.chosen.split([',', ' ']).filter_map(|w| w.trim().parse().ok()).collect()
    }
}

/// What the group gradient reads about one ball.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BallInGroup {
    /// Its place in the group's shape, and how many places the group has.
    pub number: u32,
    pub count: u32,
    /// Where it started, from the thing (the group's own places, unturned by the thing).
    pub offset: Vec2,
    pub speed: f32,
    pub hits: u32,
    pub time: f32,
    /// The emitter round it was fired in, from 1 (0 for a Balls group's), and how many rounds
    /// the gradient spreads over.
    pub round: u32,
    pub rounds: RoundSpan,
    /// The direction it was fired or launched in, radians clockwise from +x.
    pub launch: f32,
}

/// What the gradient reads about the whole group: its places' middle and how far they spread.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GroupShapeInfo {
    pub middle: Vec2,
    /// The places' smallest and largest along the gradient's angle, from the middle.
    pub along: (f32, f32),
    /// The furthest place from the middle.
    pub furthest: f32,
    pub seed: i32,
}

impl GroupShapeInfo {
    /// The group of `offsets` (the balls' places), for a gradient at `angle` degrees.
    pub fn of(offsets: &[Vec2], angle: f32, seed: i32) -> Self {
        let n = offsets.len().max(1) as f32;
        let middle = offsets.iter().copied().sum::<Vec2>() / n;
        let dir = Vec2::from_angle(angle.to_radians());
        let (mut lo, mut hi, mut far) = (0.0f32, 0.0f32, 0.0f32);
        for p in offsets {
            let d = (*p - middle).dot(dir);
            lo = lo.min(d);
            hi = hi.max(d);
            far = far.max(p.distance(middle));
        }
        Self { middle, along: (lo, hi), furthest: far, seed }
    }
}

/// How fast a ball reaches the end of the gradient for Speed, in pixels per second (times Stretch).
pub const FULL_SPEED: f32 = 1500.0;
/// How many hits reach the end of the gradient for Hits (times Stretch).
pub const FULL_HITS: f32 = 10.0;
/// How many times a second Time runs through the gradient (over Stretch).
pub const TIME_SPEED: f32 = 0.25;
/// How many rounds of an endless emitter one run through the gradient takes (times Stretch).
pub const ENDLESS_ROUNDS: f32 = 10.0;

/// Where along the gradient (0 to 1) ball `b` of group `g` sits, before easing.
pub fn group_t(gr: &GroupGradient, g: &GroupShapeInfo, b: &BallInGroup) -> f32 {
    let span = gr.stretch.clamp(0.1, 4.0);
    let t = match gr.spread {
        Spread::Place => {
            let d = (b.offset - g.middle).dot(Vec2::from_angle(gr.angle.to_radians()));
            let (lo, hi) = g.along;
            let extent = (hi - lo).max(1e-3) * span;
            (d - (lo + hi) * 0.5) / extent + 0.5
        }
        Spread::Order => crate::paint::eval::order_place(b.number as usize, b.count as usize, gr.colours.looped),
        Spread::ThereAndBack => crate::paint::eval::there_and_back(b.number as usize, b.count as usize),
        Spread::CentreOut => b.offset.distance(g.middle) / (g.furthest.max(1e-3) * span),
        Spread::Random => Rng::new(g.seed as u32, b.number ^ (7 << 24)).unit(),
        Spread::Speed => b.speed / (FULL_SPEED * span),
        Spread::Hits => b.hits as f32 / (FULL_HITS * span),
        Spread::Time => {
            let x = b.time * TIME_SPEED / span;
            x - x.floor()
        }
        Spread::Round => {
            let from_first = b.round.saturating_sub(1) as f32;
            match b.rounds {
                RoundSpan::Finite(n) if n <= 1 => 0.0,
                // A looped gradient goes round: the last round one step before the first again.
                RoundSpan::Finite(n) if gr.colours.looped => from_first / (n as f32 * span),
                RoundSpan::Finite(n) => from_first / ((n - 1) as f32 * span),
                RoundSpan::Endless => {
                    let x = from_first / (ENDLESS_ROUNDS * span);
                    x - x.floor()
                }
            }
        }
        Spread::LaunchAngle => {
            let x = (b.launch.to_degrees() - gr.angle) / 360.0;
            x - x.floor()
        }
    };
    if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 }
}

/// `t` (0 to 1) eased the way `ease` says, `strength` from 1 (barely) to 6.
pub fn eased(t: f32, ease: Easing, strength: f32) -> f32 {
    let k = strength.clamp(1.0, 6.0);
    let t = t.clamp(0.0, 1.0);
    match ease {
        Easing::Even => t,
        Easing::EaseIn => t.powf(k),
        Easing::EaseOut => 1.0 - (1.0 - t).powf(k),
        Easing::EaseBoth => {
            let (a, b) = (t.powf(k), (1.0 - t).powf(k));
            if a + b > 0.0 { a / (a + b) } else { t }
        }
    }
}

/// The group gradient's colour for ball `b` (linear light, straight).
pub fn group_colour(gr: &GroupGradient, g: &GroupShapeInfo, b: &BallInGroup) -> [f32; 4] {
    gr.colours.sample(eased(group_t(gr, g, b), gr.ease, gr.ease_strength))
}

/// `ball` (linear, straight) with the group's colour `group` mixed in by `mix` at `amount`, as the
/// GPU mixes it (W3C blend maths, as blend_maths.wgsl).
pub fn mixed(ball: [f32; 4], group: [f32; 4], mix: GroupMix, amount: f32) -> [f32; 4] {
    let Some(blend) = mix.blend() else { return ball };
    let b = Vec3::new(ball[0], ball[1], ball[2]);
    let s = Vec3::new(group[0], group[1], group[2]);
    let r = blended(blend, b, s);
    let a = amount.clamp(0.0, 1.0);
    let out = b * (1.0 - a) + r * a;
    [out.x, out.y, out.z, ball[3]]
}

use glam::Vec3;

fn lum(c: Vec3) -> f32 {
    c.dot(Vec3::new(0.3, 0.59, 0.11))
}

fn clip_colour(c: Vec3) -> Vec3 {
    let l = lum(c);
    let (n, x) = (c.min_element(), c.max_element());
    let mut c = c;
    if n < 0.0 {
        c = Vec3::splat(l) + (c - l) * l / (l - n).max(1e-6);
    }
    if x > 1.0 {
        c = Vec3::splat(l) + (c - l) * (1.0 - l) / (x - l).max(1e-6);
    }
    c
}

fn set_lum(c: Vec3, l: f32) -> Vec3 {
    clip_colour(c + (l - lum(c)))
}

fn overlay_one(d: f32, s: f32) -> f32 {
    if d <= 0.5 { 2.0 * s * d } else { 1.0 - (1.0 - s) * (2.0 - 2.0 * d) }
}

/// The blends a group gradient uses, on the CPU (the same maths as `blended` in blend_maths.wgsl).
fn blended(blend: Blend, d: Vec3, s: Vec3) -> Vec3 {
    let (dc, sc) = (d.clamp(Vec3::ZERO, Vec3::ONE), s.clamp(Vec3::ZERO, Vec3::ONE));
    match blend {
        Blend::Screen => Vec3::ONE - (Vec3::ONE - dc) * (Vec3::ONE - sc),
        Blend::Multiply => d * s,
        Blend::Overlay => Vec3::new(overlay_one(dc.x, sc.x), overlay_one(dc.y, sc.y), overlay_one(dc.z, sc.z)),
        Blend::Colour => set_lum(sc, lum(dc)),
        _ => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ball(number: u32, count: u32) -> BallInGroup {
        BallInGroup { number, count, ..BallInGroup::default() }
    }

    #[test]
    fn order_runs_from_the_first_ball_to_the_last() {
        let gr = GroupGradient { spread: Spread::Order, ..GroupGradient::default() };
        let g = GroupShapeInfo::default();
        assert_eq!(group_t(&gr, &g, &ball(0, 10)), 0.0);
        assert_eq!(group_t(&gr, &g, &ball(9, 10)), 1.0);
        assert_eq!(group_t(&gr, &g, &ball(0, 1)), 0.0, "one ball takes the first colour");
    }

    #[test]
    fn round_runs_from_the_first_round_to_the_last_and_comes_round_when_endless() {
        let gr = GroupGradient { spread: Spread::Round, ..GroupGradient::default() };
        let g = GroupShapeInfo::default();
        let at = |round, rounds| group_t(&gr, &g, &BallInGroup { round, rounds, ..BallInGroup::default() });
        assert_eq!(at(1, RoundSpan::Finite(5)), 0.0);
        assert_eq!(at(3, RoundSpan::Finite(5)), 0.5);
        assert_eq!(at(5, RoundSpan::Finite(5)), 1.0);
        assert_eq!(at(1, RoundSpan::Finite(1)), 0.0, "one round takes the first colour");
        assert_eq!(at(0, RoundSpan::Finite(1)), 0.0, "a Balls group's balls are the first round");
        assert!((at(6, RoundSpan::Endless) - 0.5).abs() < 1e-6);
        assert_eq!(at(11, RoundSpan::Endless), 0.0, "every ten rounds it starts again");
    }

    #[test]
    fn a_looped_gradient_closes_the_ring_for_order_and_rounds() {
        let mut gr = GroupGradient { spread: Spread::Order, ..GroupGradient::default() };
        gr.colours.looped = true;
        let g = GroupShapeInfo::default();
        assert_eq!(group_t(&gr, &g, &ball(9, 10)), 0.9);
        gr.spread = Spread::Round;
        let at = |gr: &GroupGradient, round| {
            group_t(gr, &g, &BallInGroup { round, rounds: RoundSpan::Finite(4), ..BallInGroup::default() })
        };
        assert_eq!(at(&gr, 4), 0.75);
        gr.spread = Spread::ThereAndBack;
        let there: Vec<f32> = (0..4).map(|n| group_t(&gr, &g, &ball(n, 4))).collect();
        assert_eq!(there, vec![0.0, 0.5, 1.0, 0.5]);
    }

    #[test]
    fn launch_angle_goes_once_round_from_the_gradients_angle() {
        let gr = GroupGradient { spread: Spread::LaunchAngle, angle: 90.0, ..GroupGradient::default() };
        let g = GroupShapeInfo::default();
        let at = |deg: f32| group_t(&gr, &g, &BallInGroup { launch: deg.to_radians(), ..BallInGroup::default() });
        assert!(at(90.0).abs() < 1e-5);
        assert!((at(270.0) - 0.5).abs() < 1e-5);
        assert!((at(0.0) - 0.75).abs() < 1e-5);
        assert!((at(-90.0) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn place_runs_across_the_group_along_the_angle() {
        let places = [Vec2::new(-50.0, 0.0), Vec2::new(0.0, 30.0), Vec2::new(50.0, 0.0)];
        let gr = GroupGradient::default();
        let g = GroupShapeInfo::of(&places, gr.angle, 1);
        let t = |p: Vec2| group_t(&gr, &g, &BallInGroup { offset: p, ..BallInGroup::default() });
        assert!(t(places[0]).abs() < 1e-5 && (t(places[2]) - 1.0).abs() < 1e-5 && (t(places[1]) - 0.5).abs() < 1e-5);
        let down = GroupGradient { angle: 90.0, ..GroupGradient::default() };
        let g = GroupShapeInfo::of(&places, down.angle, 1);
        let t = |p: Vec2| group_t(&down, &g, &BallInGroup { offset: p, ..BallInGroup::default() });
        assert!(t(places[1]) > t(places[0]) && (t(places[0]) - t(places[2])).abs() < 1e-5, "top to bottom");
    }

    #[test]
    fn centre_out_is_nothing_in_the_middle() {
        let places = [Vec2::new(-10.0, 0.0), Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(0.0, 20.0)];
        let gr = GroupGradient { spread: Spread::CentreOut, ..GroupGradient::default() };
        let g = GroupShapeInfo::of(&places, 0.0, 1);
        let middle = BallInGroup { offset: g.middle, ..BallInGroup::default() };
        assert!(group_t(&gr, &g, &middle).abs() < 1e-5);
        let far = BallInGroup { offset: Vec2::new(0.0, 20.0), ..BallInGroup::default() };
        assert!((group_t(&gr, &g, &far) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn random_is_the_same_for_one_seed() {
        let gr = GroupGradient { spread: Spread::Random, ..GroupGradient::default() };
        let g = GroupShapeInfo { seed: 4, ..GroupShapeInfo::default() };
        let ts: Vec<f32> = (0..20).map(|n| group_t(&gr, &g, &ball(n, 20))).collect();
        assert_eq!(ts, (0..20).map(|n| group_t(&gr, &g, &ball(n, 20))).collect::<Vec<_>>());
        assert!(ts.iter().any(|t| *t < 0.3) && ts.iter().any(|t| *t > 0.7));
        let other = GroupShapeInfo { seed: 5, ..g };
        assert_ne!(ts[0], group_t(&gr, &other, &ball(0, 20)));
    }

    #[test]
    fn ease_in_bunches_colours_towards_the_start() {
        assert!(eased(0.5, Easing::EaseIn, 2.0) < 0.3);
        assert!(eased(0.5, Easing::EaseOut, 2.0) > 0.7);
        assert!((eased(0.5, Easing::EaseBoth, 3.0) - 0.5).abs() < 1e-6);
        assert!(eased(0.2, Easing::EaseBoth, 3.0) < 0.2);
        assert_eq!(eased(0.37, Easing::Even, 5.0), 0.37);
    }

    #[test]
    fn replace_gives_the_gradient_colour_and_multiply_on_white_does_too() {
        let blue = [0.1, 0.2, 0.9, 1.0];
        assert_eq!(mixed([1.0; 4], blue, GroupMix::Replace, 1.0), blue);
        let m = mixed([1.0; 4], blue, GroupMix::Multiply, 1.0);
        assert!(m.iter().zip(blue).all(|(a, b)| (a - b).abs() < 1e-6));
        let red = [0.8, 0.1, 0.1, 1.0];
        assert_eq!(mixed(red, blue, GroupMix::Replace, 0.0), red, "amount 0 leaves the ball");
        assert_eq!(mixed(red, blue, GroupMix::Off, 1.0), red);
        let screen = mixed([0.0, 0.0, 0.0, 1.0], blue, GroupMix::Screen, 1.0);
        assert!(screen.iter().zip(blue).all(|(a, b)| (a - b).abs() < 1e-6), "screen on black is the colour");
        let tint = mixed([0.5, 0.5, 0.5, 1.0], blue, GroupMix::Tint, 1.0);
        assert!(tint[2] > tint[0], "tint takes the hue");
    }

    #[test]
    fn mix_codes_are_one_past_the_blend_codes() {
        assert_eq!(GroupMix::Off.code(), 0);
        assert_eq!(GroupMix::Replace.code(), 1);
        assert_eq!(GroupMix::Multiply.code(), Blend::Multiply.shader_code() + 1);
    }

    #[test]
    fn chosen_reads_layer_numbers() {
        let gr = GroupGradient { chosen: "3, 5,x 7".into(), ..GroupGradient::default() };
        assert_eq!(gr.chosen_ids(), [3, 5, 7]);
    }
}

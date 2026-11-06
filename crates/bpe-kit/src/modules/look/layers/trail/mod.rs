//! The Trail layer (`layer.trail`): something left behind each ball along where it has been, in
//! the Look's "Trails and particles" list. Six kinds: a Streak, a Ribbon that twists, Ghosts
//! (fading copies of the whole ball), Dots, a thin Line (that can stay for good) and Smoke.
//! `draw.rs` turns one into what the frame draws; `kinds.rs` holds the per-kind maths.

pub mod draw;
pub mod kinds;
pub mod line;

use bpe_core::{Blend, Choice, Color, Settings};

use crate::paint::{Follow, Paint};

/// The layer kind.
pub const TRAIL: &str = "layer.trail";

/// What a trail is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum TrailKind {
    /// A fading streak, narrowing towards its tail.
    #[default]
    Streak,
    /// A flat band that twists as the ball turns, darker on its back.
    Ribbon,
    /// Fading copies of the whole ball along its path.
    Ghosts,
    /// Dots dropped along the path.
    Dots,
    /// A thin line of even width, which can stay for good.
    Line,
    /// Soft puffs that grow and rise.
    Smoke,
}

/// Where a trail or particles draw against their ball.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum AroundPlace {
    /// Under the balls.
    #[default]
    Behind,
    /// Over the balls.
    InFront,
}

/// How a trail's length is measured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum LengthBy {
    /// How far back in time it reaches.
    #[default]
    Time,
    /// How far back along the path it reaches.
    Distance,
}

/// How something changes from one end to the other: evenly, slowly at first (In), quickly at
/// first (Out), or slowly at both ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Curve {
    #[default]
    Even,
    In,
    Out,
    Both,
}

impl Curve {
    /// How far along, 0 to 1, at `t` of the way (0 to 1): 0 at 0 and 1 at 1 for every curve.
    pub fn at(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Even => t,
            Self::In => t * t,
            Self::Out => 1.0 - (1.0 - t) * (1.0 - t),
            Self::Both => t * t * (3.0 - 2.0 * t),
        }
    }
}

/// How a trail fades from the ball to its tail: along a curve, or not at all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum TrailFade {
    #[default]
    Even,
    In,
    Out,
    Both,
    None,
}

impl TrailFade {
    /// How much shows at `t` of the way from the ball (0) to the tail (1): 1 at the ball.
    pub fn at(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::None => 1.0,
            // Fading out quickly at first: today's streak.
            Self::Out => (1.0 - t) * (1.0 - t),
            Self::Even => 1.0 - t,
            Self::In => 1.0 - t * t,
            Self::Both => 1.0 - Curve::Both.at(t),
        }
    }
}

/// Where a trail takes its colour from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum TrailSource {
    /// The ball's own colour.
    #[default]
    Ball,
    /// The trail's own Colours.
    Own,
}

/// How a Ribbon twists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Twist {
    /// With the ball's turning (its Turning reaction).
    #[default]
    FollowsTurning,
    /// At its own speed.
    Spins,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct TrailLayer {
    /// What it is.
    #[setting(label = "Kind", group = "Shape", simple)]
    pub kind: TrailKind,
    /// Under the balls or over them.
    #[setting(label = "Where", group = "Shape", simple)]
    pub place: AroundPlace,
    /// Whether its length is how far back in time or along the path it reaches.
    #[setting(label = "Length by", group = "Shape", when = "kind!=ghosts")]
    pub length_by: LengthBy,
    /// How far back in time it reaches.
    #[setting(
        label = "Length",
        group = "Shape",
        when = "kind!=ghosts and length_by=time",
        range(0.05, 2),
        unit = "s",
        simple,
        keyframe
    )]
    pub seconds: f32,
    /// How far back along the path it reaches.
    #[setting(
        label = "Length",
        group = "Shape",
        when = "kind!=ghosts and length_by=distance",
        range(10, 4000),
        unit = "px",
        keyframe
    )]
    pub pixels: f32,
    /// The whole path since the start stays.
    #[setting(label = "Stays forever", group = "Shape", when = "kind=line")]
    pub forever: bool,
    /// Longer when the ball is fast: the length grows by this much for every 1000 px/s.
    #[setting(label = "Longer when fast", group = "Shape", when = "kind!=ghosts", range(0, 2), keyframe)]
    pub faster: f32,
    /// Slower balls show none of it.
    #[setting(label = "Shows above speed", group = "Shape", range(0, 3000), unit = "px/s", keyframe)]
    pub least_speed: f32,
    /// How wide it is at the ball, in ball radii.
    #[setting(
        label = "Width",
        group = "Shape",
        when = "kind=streak|ribbon|smoke",
        range(0.05, 2),
        simple,
        keyframe,
        pulse
    )]
    pub width: f32,
    /// How thick the line is.
    #[setting(label = "Thickness", group = "Shape", when = "kind=line", range(0.5, 20), unit = "px", keyframe)]
    pub thickness: f32,
    /// How wide it is at its tail, as a share of its width at the ball: 0 ends in a point.
    #[setting(label = "Width at the tail", group = "Shape", when = "kind=streak|ribbon", range(0, 1), keyframe)]
    pub tail_width: f32,
    /// How the width changes from the ball to the tail.
    #[setting(label = "Width curve", group = "Shape", when = "kind=streak|ribbon")]
    pub width_curve: Curve,
    /// How many copies of the ball.
    #[setting(label = "How many", group = "Shape", when = "kind=ghosts", range(1, 30), keyframe)]
    pub ghosts: i32,
    /// How far apart in time the copies are.
    #[setting(label = "Every", group = "Shape", when = "kind=ghosts", range(0.02, 0.5), unit = "s", keyframe)]
    pub ghost_every: f32,
    /// How far apart along the path the dots are.
    #[setting(label = "Every", group = "Shape", when = "kind=dots", range(2, 200), unit = "px", keyframe)]
    pub dot_every: f32,
    /// How big each dot is, in ball radii.
    #[setting(label = "Dot size", group = "Shape", when = "kind=dots", range(0.05, 1), keyframe, pulse)]
    pub dot_size: f32,
    /// How many puffs a second it leaves.
    #[setting(label = "Puffs per second", group = "Shape", when = "kind=smoke", range(5, 120))]
    pub puffs: f32,
    /// How much each puff grows as it ages, in its own size per second.
    #[setting(label = "Spreads", group = "Shape", when = "kind=smoke", range(0, 4), keyframe)]
    pub spreads: f32,
    /// How fast the puffs rise.
    #[setting(label = "Rises", group = "Shape", when = "kind=smoke", range(0, 400), unit = "px/s", keyframe)]
    pub rises: f32,
    /// How soft the puffs' edges are.
    #[setting(label = "Softness", group = "Shape", when = "kind=smoke", range(0, 1), keyframe)]
    pub softness: f32,
    /// A Ribbon twists with the ball's turning, or spins at its own speed.
    #[setting(label = "Twist", group = "Shape", when = "kind=ribbon")]
    pub twist: Twist,
    /// How fast a spinning Ribbon turns.
    #[setting(
        label = "Turns per second",
        group = "Shape",
        when = "kind=ribbon and twist=spins",
        range(0, 10),
        keyframe
    )]
    pub twist_speed: f32,
    /// The ball's own colour, or the Colours below.
    #[setting(label = "Colour from", group = "Colours", when = "kind!=ghosts", simple)]
    pub source: TrailSource,
    /// Its own colours: over its length from the ball to the tail, or by where, how fast or when
    /// each part of it was left.
    #[setting(
        paint,
        label = "Colours",
        group = "Colours",
        prefix = "",
        when = "kind!=ghosts and source=own",
        follows = "life|place|speed|time"
    )]
    pub colour: Paint,
    /// How bright it is at the ball (for Ghosts, how see-through the nearest copy is).
    #[setting(label = "Brightness", group = "Light", range(0, 2), simple, keyframe, pulse)]
    pub brightness: f32,
    /// How it fades from the ball to its tail.
    #[setting(label = "Fade", group = "Light")]
    pub fade: TrailFade,
    /// How far it waves from side to side.
    #[setting(
        label = "Wobble",
        group = "Motion",
        when = "kind=streak|ribbon|line|dots",
        range(0, 100),
        unit = "px",
        keyframe,
        pulse
    )]
    pub wobble_amount: f32,
    /// How many waves along it.
    #[setting(
        label = "Waves along it",
        group = "Motion",
        when = "kind=streak|ribbon|line|dots",
        range(0.5, 20),
        keyframe
    )]
    pub wobble_waves: f32,
    /// How fast the waves run along it.
    #[setting(
        label = "Wave speed",
        group = "Motion",
        when = "kind=streak|ribbon|line|dots",
        range(0, 10),
        unit = "per s",
        keyframe
    )]
    pub wobble_speed: f32,
    /// Starts again from the ball every time it bounces.
    #[setting(label = "Restart at each bounce", group = "Motion")]
    pub restart: bool,
    /// How it mixes with what is under it: Add glows and adds its light, Normal paints over
    /// it, the others mix with it. With the others, everything with the same Blend and amount
    /// is mixed in once, as one picture, so where trails overlap they do not stack.
    #[setting(label = "Blend", group = "Blending", when = "kind!=ghosts")]
    pub blend: Blend,
    /// How much of the blend shows: 0 is as if it weren't there, 1 is the whole blend.
    #[setting(label = "Blend amount", range(0, 1), group = "Blending", when = "kind!=ghosts", keyframe)]
    pub blend_amount: f32,
}

impl Default for TrailLayer {
    fn default() -> Self {
        Self {
            kind: TrailKind::Streak,
            place: AroundPlace::Behind,
            length_by: LengthBy::Time,
            seconds: 0.5,
            pixels: 300.0,
            forever: false,
            faster: 0.0,
            least_speed: 0.0,
            width: 0.5,
            thickness: 2.0,
            tail_width: 0.0,
            width_curve: Curve::Even,
            ghosts: 6,
            ghost_every: 0.06,
            dot_every: 20.0,
            dot_size: 0.3,
            puffs: 40.0,
            spreads: 1.5,
            rises: 40.0,
            softness: 0.7,
            twist: Twist::FollowsTurning,
            twist_speed: 1.0,
            source: TrailSource::Ball,
            colour: Paint { follow: Follow::Life, ..Paint::solid(Color::WHITE) },
            brightness: 0.5,
            fade: TrailFade::Even,
            wobble_amount: 0.0,
            wobble_waves: 3.0,
            wobble_speed: 1.0,
            restart: false,
            blend: Blend::Add,
            blend_amount: 1.0,
        }
    }
}

/// How fast a ball goes at which "Longer when fast" adds its whole amount, in px/s.
pub const FAST: f32 = 1000.0;
/// How fast a ball is taken to go when a By distance trail asks how much history it needs.
pub const HISTORY_SPEED: f32 = 300.0;

impl TrailLayer {
    /// How many seconds of the ball's past this trail can reach back at most, for the history
    /// the world keeps: its Length (a By distance length at [`HISTORY_SPEED`], at least 2 s),
    /// longer for Longer when fast (up to 3000 px/s), and the copies' reach for Ghosts.
    pub fn reach(&self) -> f32 {
        let base = match (self.kind, self.length_by) {
            (TrailKind::Ghosts, _) => self.ghosts.max(1) as f32 * self.ghost_every.max(0.0),
            (_, LengthBy::Time) => self.seconds.max(0.0),
            (_, LengthBy::Distance) => (self.pixels.max(0.0) / HISTORY_SPEED).max(2.0),
        };
        let fast = if self.kind == TrailKind::Ghosts { 1.0 } else { 1.0 + self.faster.max(0.0) * 3.0 };
        base * fast
    }

    /// Whether it keeps the whole path since the start.
    pub fn stays(&self) -> bool {
        self.kind == TrailKind::Line && self.forever
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_curve_runs_from_nothing_to_all() {
        for c in [Curve::Even, Curve::In, Curve::Out, Curve::Both] {
            assert_eq!(c.at(0.0), 0.0, "{c:?}");
            assert_eq!(c.at(1.0), 1.0, "{c:?}");
            assert!(c.at(0.25) < c.at(0.75), "{c:?} rises");
        }
        assert!(Curve::In.at(0.5) < 0.5 && Curve::Out.at(0.5) > 0.5);
        assert_eq!(Curve::Both.at(0.5), 0.5);
        for f in [TrailFade::Even, TrailFade::In, TrailFade::Out, TrailFade::Both] {
            assert_eq!((f.at(0.0), f.at(1.0)), (1.0, 0.0), "{f:?}");
        }
        assert_eq!(TrailFade::None.at(1.0), 1.0);
        assert_eq!(TrailFade::Out.at(0.5), 0.25, "today's streak fade");
    }

    #[test]
    fn a_trail_reaches_back_as_far_as_its_length_says() {
        let t = TrailLayer::default();
        assert_eq!(t.reach(), 0.5);
        assert_eq!(TrailLayer { faster: 1.0, ..t.clone() }.reach(), 2.0);
        assert_eq!(TrailLayer { length_by: LengthBy::Distance, pixels: 300.0, ..t.clone() }.reach(), 2.0);
        assert_eq!(TrailLayer { length_by: LengthBy::Distance, pixels: 1200.0, ..t.clone() }.reach(), 4.0);
        let ghosts = TrailLayer { kind: TrailKind::Ghosts, ghosts: 10, ghost_every: 0.1, faster: 2.0, ..t.clone() };
        assert!((ghosts.reach() - 1.0).abs() < 1e-6);
        assert!(!t.stays() && TrailLayer { kind: TrailKind::Line, forever: true, ..t }.stays());
    }
}

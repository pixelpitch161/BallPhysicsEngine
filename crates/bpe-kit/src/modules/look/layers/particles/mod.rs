//! The Particles layer (`layer.particles`, and `shape.particles` on shapes): bits let off by a
//! ball (or a shape) on hits, while it moves, when it pops or steadily, in the Look's "Trails and
//! particles" list. `emit.rs` finds when and where they were let off, `sim.rs` moves each one
//! from its birth to the frame's moment (the same every time, so scrubbing and export agree) and
//! `draw.rs` turns them into what the frame draws.

pub mod draw;
pub mod emit;
pub mod sim;

use bpe_core::{Blend, Choice, Color, Settings};

use super::trail::AroundPlace;
use crate::paint::{Follow, Paint};

/// The layer kind on balls.
pub const PARTICLES: &str = "layer.particles";

/// When particles are let off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Emit {
    /// Every time the ball hits something.
    #[default]
    OnHit,
    /// All the time the ball moves.
    WhileMoving,
    /// While the ball goes faster than the least speed.
    OnlyWhenFast,
    /// When the ball pops.
    WhenItPops,
    /// All the time, at a steady rate.
    Steady,
}

/// What the particles are made of.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum MadeOf {
    #[default]
    Sparks,
    Smoke,
    Confetti,
    Stars,
    Shards,
    Rings,
    Dots,
    Hearts,
    Bubbles,
    Streaks,
    /// Small copies of the ball, wearing its whole look.
    MiniBalls,
}

impl MadeOf {
    /// The shape's number in particles.wgsl (Mini balls draw as balls, not particles).
    pub fn code(self) -> u32 {
        match self {
            Self::Sparks => 0,
            Self::Smoke => 1,
            Self::Confetti => 2,
            Self::Stars => 3,
            Self::Shards => 4,
            Self::Rings => 5,
            Self::Dots => 6,
            Self::Hearts => 7,
            Self::Bubbles => 8,
            Self::Streaks => 9,
            Self::MiniBalls => 10,
        }
    }
}

/// The way particles fly when they are let off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Direction {
    /// Away from what was hit (on a shape, away from its edge).
    #[default]
    OffTheBounce,
    /// Any way.
    AllRound,
    /// The way the ball goes.
    AlongThePath,
    /// Back the way the ball came.
    BehindTheBall,
    /// Upwards.
    Up,
}

/// How a particle's size changes over its life.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum SizeOverLife {
    Even,
    Grow,
    #[default]
    Shrink,
    /// Grows, then bursts.
    Pop,
}

impl SizeOverLife {
    /// The share of its size a particle has at `t` of its life (0 to 1), or `None` once a Pop has
    /// burst (the last tenth of its life).
    pub fn at(self, t: f32) -> Option<f32> {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Even => Some(1.0),
            Self::Grow => Some(0.3 + 0.7 * t),
            Self::Shrink => Some(1.0 - 0.8 * t),
            Self::Pop if t >= 0.9 => None,
            Self::Pop => Some(1.0 + 0.5 * t / 0.9),
        }
    }
}

/// How a particle fades over its life.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ParticleFade {
    /// Fades in as it starts.
    In,
    /// Fades out as it ends.
    #[default]
    Out,
    /// Both.
    Both,
    None,
}

impl ParticleFade {
    /// How much of the particle shows at `t` of its life (0 to 1).
    pub fn at(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        let fade_in = (t / 0.15).min(1.0);
        let fade_out = 1.0 - t;
        match self {
            Self::In => fade_in,
            Self::Out => fade_out,
            Self::Both => fade_in * fade_out,
            Self::None => 1.0,
        }
    }
}

/// Where particles take their colour from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ParticleSource {
    /// The colour of the ball that let them off.
    #[default]
    Ball,
    /// Their own Colours.
    Own,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ParticlesLayer {
    /// When they are let off.
    #[setting(label = "When", group = "Particles", simple)]
    pub emit: Emit,
    /// How many each hit (or pop) lets off.
    #[setting(
        label = "Count per hit",
        group = "Particles",
        when = "emit=on_hit|when_it_pops",
        range(1, 200),
        simple,
        keyframe
    )]
    pub count: i32,
    /// How many are let off each second.
    #[setting(
        label = "Rate",
        group = "Particles",
        when = "emit=while_moving|only_when_fast|steady",
        range(1, 500),
        unit = "per s",
        keyframe
    )]
    pub rate: f32,
    /// Hits (or, Only when fast, balls) slower than this let off nothing.
    #[setting(
        label = "Least speed",
        group = "Particles",
        when = "emit=on_hit|only_when_fast",
        range(0, 2000),
        unit = "px/s",
        keyframe
    )]
    pub least_speed: f32,
    /// Harder hits let off more and faster particles.
    #[setting(label = "Follow impact", group = "Particles", when = "emit=on_hit")]
    pub follow_impact: bool,
    /// What they are made of.
    #[setting(label = "Made of", group = "Particles", simple)]
    pub made_of: MadeOf,
    /// Under the balls or over them.
    #[setting(label = "Where", group = "Particles")]
    pub place: AroundPlace,
    /// The way they fly.
    #[setting(label = "Direction", group = "Each particle")]
    pub direction: Direction,
    /// How wide they fan out round that way.
    #[setting(label = "Spread", group = "Each particle", range(0, 360), unit = "°", keyframe)]
    pub spread: f32,
    /// How fast they fly.
    #[setting(label = "Speed", group = "Each particle", range(0, 2000), unit = "px/s", simple, keyframe, pulse)]
    pub speed: f32,
    /// How much of the ball's own speed they take with them.
    #[setting(label = "Takes the ball's speed", group = "Each particle", range(0, 1), keyframe)]
    pub inherit: f32,
    /// How big each one is; for rings, how big the ring grows.
    #[setting(label = "Size", group = "Each particle", range(0.5, 200), unit = "px", simple, keyframe, pulse)]
    pub size: f32,
    /// How each one's size changes over its life.
    #[setting(label = "Size over life", group = "Each particle")]
    pub size_curve: SizeOverLife,
    /// How fast they turn.
    #[setting(label = "Spin", group = "Each particle", range(0, 1440), unit = "°/s", keyframe)]
    pub spin: f32,
    /// How much the spin differs from one to the next.
    #[setting(label = "Spin spread", group = "Each particle", range(0, 1), keyframe)]
    pub spin_spread: f32,
    /// How long each one lasts.
    #[setting(label = "Life", group = "Each particle", range(0.05, 3), unit = "s", simple, keyframe)]
    pub life: f32,
    /// How each one fades over its life.
    #[setting(label = "Fade", group = "Each particle")]
    pub fade: ParticleFade,
    /// The colour of the ball that let them off, or the Colours below.
    #[setting(label = "Colour from", group = "Colours", when = "made_of!=mini_balls")]
    pub source: ParticleSource,
    /// Their own colours: over each one's life, a stop picked for each, or by where, how fast or
    /// when it was let off.
    #[setting(
        paint,
        label = "Colours",
        group = "Colours",
        prefix = "",
        when = "made_of!=mini_balls and source=own",
        follows = "life|order|place|speed|time"
    )]
    pub colour: Paint,
    /// How much they twinkle.
    #[setting(label = "Twinkle", group = "Colours", range(0, 1), keyframe)]
    pub twinkle: f32,
    /// How fast they twinkle.
    #[setting(label = "Twinkle speed", group = "Colours", range(0, 30), unit = "per s", keyframe)]
    pub twinkle_speed: f32,
    /// How fast they fall; negative makes them rise.
    #[setting(label = "Gravity", group = "Motion", range(-2000, 2000), unit = "px/s²", keyframe)]
    pub gravity: f32,
    /// How quickly they slow down.
    #[setting(label = "Drag", group = "Motion", range(0, 10), keyframe)]
    pub drag: f32,
    /// How strongly they swirl about.
    #[setting(label = "Swirl", group = "Motion", range(0, 2000), unit = "px/s²", keyframe)]
    pub swirl: f32,
    /// How big the swirls are.
    #[setting(label = "Swirl size", group = "Motion", when = "swirl", range(20, 1000), unit = "px", keyframe)]
    pub swirl_size: f32,
    /// How strongly they are pulled back to the ball that let them off.
    #[setting(label = "Pull back to the ball", group = "Motion", range(0, 50), unit = "per s", keyframe)]
    pub pull: f32,
    /// They bounce off shapes instead of flying through them.
    #[setting(label = "Bounce off shapes", group = "Motion")]
    pub bounce: bool,
    /// How much of their speed they keep on a bounce.
    #[setting(label = "Bounciness", group = "Motion", when = "bounce", range(0, 1), keyframe)]
    pub bounciness: f32,
    /// How they mix with what is under them: Add glows and adds their light, Normal paints over
    /// it, the others mix with it. With the others, everything with the same Blend and amount is
    /// mixed in once, as one picture, so where particles overlap they do not stack.
    #[setting(label = "Blend", group = "Blending", when = "made_of!=mini_balls")]
    pub blend: Blend,
    /// How much of the blend shows: 0 is as if they weren't there, 1 is the whole blend.
    #[setting(label = "Blend amount", range(0, 1), group = "Blending", when = "made_of!=mini_balls", keyframe)]
    pub blend_amount: f32,
}

impl Default for ParticlesLayer {
    fn default() -> Self {
        Self {
            emit: Emit::OnHit,
            count: 12,
            rate: 60.0,
            least_speed: 0.0,
            follow_impact: true,
            made_of: MadeOf::Sparks,
            place: AroundPlace::Behind,
            direction: Direction::OffTheBounce,
            spread: 140.0,
            speed: 380.0,
            inherit: 0.0,
            size: 2.5,
            size_curve: SizeOverLife::Shrink,
            spin: 0.0,
            spin_spread: 0.5,
            life: 0.6,
            fade: ParticleFade::Out,
            source: ParticleSource::Ball,
            colour: Paint { follow: Follow::Life, ..Paint::solid(Color::WHITE) },
            twinkle: 0.0,
            twinkle_speed: 8.0,
            gravity: 300.0,
            drag: 2.0,
            swirl: 0.0,
            swirl_size: 200.0,
            pull: 0.0,
            bounce: false,
            bounciness: 0.5,
            blend: Blend::Add,
            blend_amount: 1.0,
        }
    }
}

impl ParticlesLayer {
    /// How many seconds of the ball's past these particles read: where the ball was when each
    /// still-living one was let off (while moving, only when fast), or to pull them back to it.
    /// 0 when they need none.
    pub fn reach(&self) -> f32 {
        let moving = matches!(self.emit, Emit::WhileMoving | Emit::OnlyWhenFast);
        if moving || self.pull > 0.0 { self.life.max(0.0) } else { 0.0 }
    }
}

/// The layer kind on shapes.
pub const SHAPE_PARTICLES: &str = "shape.particles";

/// When a shape lets particles off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ShapeEmit {
    /// Every time a ball hits the shape.
    #[default]
    OnHit,
    /// All the time, from random places along its edge.
    Steady,
}

/// What a shape's particles are made of: a ball's, but no Mini balls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ShapeMadeOf {
    #[default]
    Sparks,
    Smoke,
    Confetti,
    Stars,
    Shards,
    Rings,
    Dots,
    Hearts,
    Bubbles,
    Streaks,
}

impl ShapeMadeOf {
    /// The same thing as a ball's.
    pub fn as_ball(self) -> MadeOf {
        MadeOf::from_name(self.name()).unwrap_or_default()
    }
}

/// The way a shape's particles fly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ShapeDirection {
    /// Away from the shape's edge, on the side the ball hit (or, steady, either side).
    #[default]
    OffTheEdge,
    /// Any way.
    AllRound,
    /// Upwards.
    Up,
}

/// A shape's Particles: a ball's, let off when a ball hits the shape or steadily along its edge.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ShapeParticlesLayer {
    /// When they are let off.
    #[setting(label = "When", group = "Particles", simple)]
    pub emit: ShapeEmit,
    /// How many each hit lets off.
    #[setting(label = "Count per hit", group = "Particles", when = "emit=on_hit", range(1, 200), simple, keyframe)]
    pub count: i32,
    /// How many are let off each second.
    #[setting(label = "Rate", group = "Particles", when = "emit=steady", range(1, 500), unit = "per s", keyframe)]
    pub rate: f32,
    /// Hits slower than this let off nothing.
    #[setting(
        label = "Least speed",
        group = "Particles",
        when = "emit=on_hit",
        range(0, 2000),
        unit = "px/s",
        keyframe
    )]
    pub least_speed: f32,
    /// Harder hits let off more and faster particles.
    #[setting(label = "Follow impact", group = "Particles", when = "emit=on_hit")]
    pub follow_impact: bool,
    /// What they are made of.
    #[setting(label = "Made of", group = "Particles", simple)]
    pub made_of: ShapeMadeOf,
    /// Under the balls or over them.
    #[setting(label = "Where", group = "Particles")]
    pub place: AroundPlace,
    /// The way they fly.
    #[setting(label = "Direction", group = "Each particle")]
    pub direction: ShapeDirection,
    #[setting(label = "Spread", group = "Each particle", range(0, 360), unit = "°", keyframe)]
    pub spread: f32,
    #[setting(label = "Speed", group = "Each particle", range(0, 2000), unit = "px/s", simple, keyframe, pulse)]
    pub speed: f32,
    #[setting(label = "Size", group = "Each particle", range(0.5, 200), unit = "px", simple, keyframe, pulse)]
    pub size: f32,
    #[setting(label = "Size over life", group = "Each particle")]
    pub size_curve: SizeOverLife,
    #[setting(label = "Spin", group = "Each particle", range(0, 1440), unit = "°/s", keyframe)]
    pub spin: f32,
    #[setting(label = "Spin spread", group = "Each particle", range(0, 1), keyframe)]
    pub spin_spread: f32,
    #[setting(label = "Life", group = "Each particle", range(0.05, 3), unit = "s", simple, keyframe)]
    pub life: f32,
    #[setting(label = "Fade", group = "Each particle")]
    pub fade: ParticleFade,
    /// The colour of the ball that hit, or the Colours below.
    #[setting(label = "Colour from", group = "Colours")]
    pub source: ParticleSource,
    #[setting(
        paint,
        label = "Colours",
        group = "Colours",
        prefix = "",
        when = "source=own",
        follows = "life|order|place|speed|time"
    )]
    pub colour: Paint,
    #[setting(label = "Twinkle", group = "Colours", range(0, 1), keyframe)]
    pub twinkle: f32,
    #[setting(label = "Twinkle speed", group = "Colours", range(0, 30), unit = "per s", keyframe)]
    pub twinkle_speed: f32,
    #[setting(label = "Gravity", group = "Motion", range(-2000, 2000), unit = "px/s²", keyframe)]
    pub gravity: f32,
    #[setting(label = "Drag", group = "Motion", range(0, 10), keyframe)]
    pub drag: f32,
    #[setting(label = "Swirl", group = "Motion", range(0, 2000), unit = "px/s²", keyframe)]
    pub swirl: f32,
    #[setting(label = "Swirl size", group = "Motion", when = "swirl", range(20, 1000), unit = "px", keyframe)]
    pub swirl_size: f32,
    #[setting(label = "Bounce off shapes", group = "Motion")]
    pub bounce: bool,
    #[setting(label = "Bounciness", group = "Motion", when = "bounce", range(0, 1), keyframe)]
    pub bounciness: f32,
    #[setting(label = "Blend", group = "Blending")]
    pub blend: Blend,
    #[setting(label = "Blend amount", range(0, 1), group = "Blending", keyframe)]
    pub blend_amount: f32,
}

impl Default for ShapeParticlesLayer {
    fn default() -> Self {
        let b = ParticlesLayer::default();
        Self {
            emit: ShapeEmit::OnHit,
            count: b.count,
            rate: b.rate,
            least_speed: b.least_speed,
            follow_impact: b.follow_impact,
            made_of: ShapeMadeOf::Sparks,
            place: b.place,
            direction: ShapeDirection::OffTheEdge,
            spread: b.spread,
            speed: b.speed,
            size: b.size,
            size_curve: b.size_curve,
            spin: b.spin,
            spin_spread: b.spin_spread,
            life: b.life,
            fade: b.fade,
            source: b.source,
            colour: b.colour,
            twinkle: b.twinkle,
            twinkle_speed: b.twinkle_speed,
            gravity: b.gravity,
            drag: b.drag,
            swirl: b.swirl,
            swirl_size: b.swirl_size,
            bounce: b.bounce,
            bounciness: b.bounciness,
            blend: b.blend,
            blend_amount: b.blend_amount,
        }
    }
}

impl ShapeParticlesLayer {
    /// The same particles as a ball's layer, for the shared emission and simulation: Steady and
    /// On hit as they are, Off the edge as Off the bounce, nothing taken from a ball's speed and
    /// no pull back.
    pub fn as_ball(&self) -> ParticlesLayer {
        ParticlesLayer {
            emit: match self.emit {
                ShapeEmit::OnHit => Emit::OnHit,
                ShapeEmit::Steady => Emit::Steady,
            },
            count: self.count,
            rate: self.rate,
            least_speed: self.least_speed,
            follow_impact: self.follow_impact,
            made_of: self.made_of.as_ball(),
            place: self.place,
            direction: match self.direction {
                ShapeDirection::OffTheEdge => Direction::OffTheBounce,
                ShapeDirection::AllRound => Direction::AllRound,
                ShapeDirection::Up => Direction::Up,
            },
            spread: self.spread,
            speed: self.speed,
            inherit: 0.0,
            size: self.size,
            size_curve: self.size_curve,
            spin: self.spin,
            spin_spread: self.spin_spread,
            life: self.life,
            fade: self.fade,
            source: self.source,
            colour: self.colour.clone(),
            twinkle: self.twinkle,
            twinkle_speed: self.twinkle_speed,
            gravity: self.gravity,
            drag: self.drag,
            swirl: self.swirl,
            swirl_size: self.swirl_size,
            pull: 0.0,
            bounce: self.bounce,
            bounciness: self.bounciness,
            blend: self.blend,
            blend_amount: self.blend_amount,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_fades_over_life_start_and_end_where_they_say() {
        assert_eq!(SizeOverLife::Grow.at(0.0), Some(0.3));
        assert!((SizeOverLife::Shrink.at(1.0).unwrap() - 0.2).abs() < 1e-6);
        assert_eq!(SizeOverLife::Pop.at(0.95), None, "burst in the last tenth");
        assert!(SizeOverLife::Pop.at(0.85).unwrap() > 1.4);
        assert_eq!(ParticleFade::Out.at(1.0), 0.0);
        assert_eq!(ParticleFade::In.at(0.0), 0.0);
        assert_eq!(ParticleFade::None.at(0.5), 1.0);
    }

    #[test]
    fn a_shape_s_made_of_names_a_ball_s_without_mini_balls() {
        for &n in ShapeMadeOf::NAMES {
            assert_eq!(ShapeMadeOf::from_name(n).unwrap().as_ball().name(), n);
        }
        assert!(!ShapeMadeOf::NAMES.contains(&"mini_balls"));
        assert_eq!(MadeOf::MiniBalls.code(), 10);
    }
}

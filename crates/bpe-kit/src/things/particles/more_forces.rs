//! Part 3's force layers (particles spec 3.3): Pull to, Push away from, Spring home, Flock,
//! Magnetic field, Flow field, Spirograph, Pulse on the beat and Ripple from hits. Each reads
//! only its particle and what the step gathered beforehand ([`Ready`]), so chunks on any number
//! of threads give the same particles.

use std::f32::consts::{PI, TAU};

use bpe_core::{Choice, Settings, Target};
use glam::Vec2;

use super::BeatEvery;
use super::beats::Beats;
use super::births::{signed, unit};
use super::forces::{OnlyFor, force_settings};
use super::ready::{ForceReady, Ready};
use super::state::{DEAD, Speck};
use crate::modules::look::layers::particles::sim::noise;

pub const PULL: &str = "force.pull";
pub const PUSH: &str = "force.push";
pub const SPRING: &str = "force.spring";
pub const FLOCK: &str = "force.flock";
pub const MAGNETIC: &str = "force.magnetic";
pub const FLOW: &str = "force.flow";
pub const SPIROGRAPH: &str = "force.spirograph";
pub const PULSE: &str = "force.pulse";
pub const RIPPLE: &str = "force.ripple";

/// What a pull or push acts from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Toward {
    /// The thing's own place (the middle of the screen for a whole-screen source).
    #[default]
    ItsPlace,
    /// A point you set.
    APoint,
    /// The ball each particle came from (else the middle of its parent).
    ItsBall,
    /// One ball of its parent, by its number.
    BallNumber,
    /// The middle of the balls or the shape it sits in.
    TheMiddleOfItsParent,
    /// Every ball near the particle.
    BallsNearby,
}

/// What a spirograph turns round.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Around {
    #[default]
    ItsPlace,
    APoint,
    ItsBall,
    BallNumber,
    TheMiddleOfItsParent,
}

impl Around {
    /// The same choice as a [`Toward`].
    pub fn toward(self) -> Toward {
        match self {
            Around::ItsPlace => Toward::ItsPlace,
            Around::APoint => Toward::APoint,
            Around::ItsBall => Toward::ItsBall,
            Around::BallNumber => Toward::BallNumber,
            Around::TheMiddleOfItsParent => Toward::TheMiddleOfItsParent,
        }
    }
}

/// How a pull weakens with distance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Falloff {
    /// The same at every distance.
    Even,
    /// Half as strong twice as far (1/d).
    #[default]
    WeakerFurther,
    /// A quarter as strong twice as far (1/d², like gravity).
    MuchWeakerFurther,
}

/// What a magnetic field comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum FieldFrom {
    /// Every ball is a magnet.
    #[default]
    EachBall,
    /// One magnet at the thing's place.
    ItsPlace,
}

/// Which way a ball's magnet points.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum FieldPoints {
    #[default]
    AlongItsMotion,
    /// The Angle you set.
    AWay,
}

/// The shape of a flow field.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Pattern {
    /// A smooth, slowly changing current.
    #[default]
    Noise,
    /// Round and in towards the thing's place.
    Spiral,
    /// One way, swaying side to side in bands.
    Waves,
    /// Round the thing's place, fastest at Size.
    Vortex,
}

/// Which way a beat kicks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum PulseWay {
    /// Away from the thing's place.
    #[default]
    Outwards,
    Up,
    /// The Angle you set.
    AWay,
    /// Each particle its own way.
    AllRound,
}

force_settings!(
    /// Pulls particles towards a place, a ball or the balls near them.
    PullForce {
        #[setting(label = "Toward", simple)]
        toward: Toward = Toward::ItsPlace,
        #[setting(label = "Point", when = "toward=a_point", unit = "px", keyframe)]
        point: Vec2 = Vec2::ZERO,
        #[setting(label = "Ball number", when = "toward=ball_number", range(1, 1e9), span(1, 100))]
        ball_number: u32 = 1,
        /// How hard it pulls at At pixels away.
        #[setting(label = "Strength", span(-3000, 3000), unit = "px/s²", simple, keyframe, pulse)]
        strength: f32 = 600.0,
        #[setting(label = "Falloff")]
        falloff: Falloff = Falloff::WeakerFurther,
        /// The distance at which it pulls with Strength (with balls nearby: at the ball's edge).
        #[setting(
            label = "At",
            when = "falloff!=even and toward!=balls_nearby",
            range(1, 1e9),
            span(1, 1000),
            unit = "px",
            keyframe
        )]
        at: f32 = 100.0,
        /// How far it reaches: 0 everywhere.
        #[setting(label = "Reach", when = "toward!=balls_nearby", range(0, 1e9), span(0, 2000), unit = "px", keyframe)]
        reach: f32 = 0.0,
        /// How far each ball reaches, in its radii.
        #[setting(label = "Reach", when = "toward=balls_nearby", range(0, 1e9), span(1, 20), unit = "×", keyframe)]
        ball_reach: f32 = 4.0,
        /// A sideways pull as a share of the pull: particles go round instead of straight in.
        #[setting(label = "Orbit", span(-2, 2), keyframe)]
        orbit: f32 = 0.0,
        /// Particles closer than this are swallowed.
        #[setting(label = "Swallow radius", range(0, 1e9), span(0, 200), unit = "px", keyframe)]
        swallow: f32 = 0.0,
    }
);

force_settings!(
    /// Pushes particles away from a place, a ball or the balls near them.
    PushForce {
        #[setting(label = "Away from", simple)]
        toward: Toward = Toward::BallsNearby,
        #[setting(label = "Point", when = "toward=a_point", unit = "px", keyframe)]
        point: Vec2 = Vec2::ZERO,
        #[setting(label = "Ball number", when = "toward=ball_number", range(1, 1e9), span(1, 100))]
        ball_number: u32 = 1,
        #[setting(label = "Strength", span(-3000, 3000), unit = "px/s²", simple, keyframe, pulse)]
        strength: f32 = 1500.0,
        #[setting(label = "Falloff")]
        falloff: Falloff = Falloff::WeakerFurther,
        #[setting(
            label = "At",
            when = "falloff!=even and toward!=balls_nearby",
            range(1, 1e9),
            span(1, 1000),
            unit = "px",
            keyframe
        )]
        at: f32 = 100.0,
        #[setting(label = "Reach", when = "toward!=balls_nearby", range(0, 1e9), span(0, 2000), unit = "px", keyframe)]
        reach: f32 = 0.0,
        #[setting(label = "Reach", when = "toward=balls_nearby", range(0, 1e9), span(1, 20), unit = "×", keyframe)]
        ball_reach: f32 = 4.0,
        #[setting(label = "Orbit", span(-2, 2), keyframe)]
        orbit: f32 = 0.0,
    }
);

force_settings!(
    /// Pulls each particle back to its home: its place in a word or grid, else where it was born.
    SpringForce {
        #[setting(label = "Stiffness", range(0, 1e9), span(0, 400), unit = "per s²", simple, keyframe, pulse)]
        stiffness: f32 = 30.0,
        #[setting(label = "Damping", range(0, 1e9), span(0, 30), unit = "per s", keyframe)]
        damping: f32 = 4.0,
    }
);

force_settings!(
    /// Particles fly together like birds or fish: apart when crowded, the same way, and close.
    FlockForce {
        /// How far a particle sees its neighbours.
        #[setting(label = "Reach", range(1, 1e9), span(10, 500), unit = "px", keyframe)]
        reach: f32 = 120.0,
        /// Neighbours closer than this share of Reach are pushed apart.
        #[setting(label = "Keep apart", range(0, 1), span(0, 1), keyframe)]
        apart: f32 = 0.3,
        #[setting(label = "Separation", range(0, 1e9), span(0, 50), unit = "per s²", simple, keyframe)]
        separation: f32 = 6.0,
        #[setting(label = "Alignment", range(0, 1e9), span(0, 10), unit = "per s", simple, keyframe)]
        alignment: f32 = 1.6,
        #[setting(label = "Cohesion", range(0, 1e9), span(0, 10), unit = "per s²", simple, keyframe)]
        cohesion: f32 = 1.2,
        #[setting(label = "Least speed", range(0, 1e9), span(0, 2000), unit = "px/s", keyframe)]
        least: f32 = 200.0,
        #[setting(label = "Most speed", range(0, 1e9), span(0, 2000), unit = "px/s", keyframe)]
        most: f32 = 440.0,
    }
);

force_settings!(
    /// Particles line up with a magnet's field like iron filings, and are drawn towards it.
    MagneticForce {
        #[setting(label = "From", simple)]
        from: FieldFrom = FieldFrom::EachBall,
        #[setting(label = "Points", when = "from=each_ball")]
        points: FieldPoints = FieldPoints::AlongItsMotion,
        /// The way the magnet points, in degrees: −90 up.
        #[setting(label = "Angle", when = "from=its_place or points=a_way", span(-180, 180), unit = "°", keyframe)]
        angle: f32 = -90.0,
        /// How far the field is full strength, in the ball's radii.
        #[setting(label = "Field size", when = "from=each_ball", range(0, 1e9), span(0.5, 20), unit = "×", keyframe)]
        size: f32 = 3.0,
        #[setting(label = "Field size", when = "from=its_place", range(0, 1e9), span(1, 2000), unit = "px", keyframe)]
        size_px: f32 = 100.0,
        /// How fast particles turn to the field.
        #[setting(label = "Line up", range(0, 1e9), span(0, 40), unit = "per s", simple, keyframe)]
        line_up: f32 = 10.0,
        /// How hard the field draws particles in.
        #[setting(label = "Pull", span(-2000, 2000), unit = "px/s²", keyframe, pulse)]
        pull: f32 = 60.0,
    }
);

force_settings!(
    /// Particles are carried along a current: noise, a spiral, waves or a vortex.
    FlowForce {
        #[setting(label = "Pattern", simple)]
        pattern: Pattern = Pattern::Noise,
        #[setting(label = "Speed", span(-2000, 2000), unit = "px/s", simple, keyframe, pulse)]
        speed: f32 = 160.0,
        /// How big its swirls, bands or eye are.
        #[setting(label = "Size", range(1, 1e9), span(10, 2000), unit = "px", keyframe)]
        size: f32 = 300.0,
        /// How quickly particles take on the current's speed.
        #[setting(label = "Grip", range(0, 1e9), span(0, 10), unit = "per s", keyframe)]
        grip: f32 = 2.0,
        /// How fast the pattern changes.
        #[setting(label = "Changes", when = "pattern=noise|waves", span(-2, 2), unit = "per s", keyframe)]
        changes: f32 = 0.05,
        #[setting(label = "Angle", when = "pattern=waves", span(-180, 180), unit = "°", keyframe)]
        angle: f32 = 0.0,
        /// How much a spiral turns inwards.
        #[setting(label = "Pull in", when = "pattern=spiral", span(-90, 90), unit = "°", keyframe)]
        pull_in: f32 = 15.0,
        /// How far waves sway either way.
        #[setting(label = "Sway", when = "pattern=waves", span(0, 90), unit = "°", keyframe)]
        sway: f32 = 30.0,
    }
);

force_settings!(
    /// Particles loop round on spirograph curves.
    SpirographForce {
        #[setting(label = "Round", simple)]
        round: Around = Around::ItsPlace,
        #[setting(label = "Point", when = "round=a_point", unit = "px", keyframe)]
        point: Vec2 = Vec2::ZERO,
        #[setting(label = "Ball number", when = "round=ball_number", range(1, 1e9), span(1, 100))]
        ball_number: u32 = 1,
        #[setting(label = "Ring radius", span(0, 1000), unit = "px", keyframe)]
        ring: f32 = 240.0,
        #[setting(label = "Wheel radius", span(1, 500), unit = "px", keyframe)]
        wheel: f32 = 90.0,
        #[setting(label = "Pen distance", span(0, 500), unit = "px", keyframe)]
        pen: f32 = 110.0,
        #[setting(label = "Speed", span(-3, 3), unit = "turns per s", simple, keyframe, pulse)]
        speed: f32 = 0.4,
        /// How much each particle's curve differs from the others'.
        #[setting(label = "Pens differ", span(0, 1), keyframe)]
        differ: f32 = 0.4,
    }
);

force_settings!(
    /// A kick on every beat of the scene's tempo.
    PulseForce {
        #[setting(label = "Every")]
        every: BeatEvery = BeatEvery::Beat,
        #[setting(label = "Bars", when = "every=bars", range(1, 1e6), span(1, 16), unit = "bars")]
        bars: u32 = 2,
        #[setting(label = "Offset", range(0, 1e6), span(0, 16), unit = "beats")]
        offset: f32 = 0.0,
        /// How much speed each beat gives.
        #[setting(label = "Strength", span(-2000, 2000), unit = "px/s", simple, keyframe, pulse)]
        strength: f32 = 300.0,
        /// How long each kick lasts: 0 all at once.
        #[setting(label = "Decay", range(0, 1e9), span(0, 2), unit = "s", keyframe)]
        decay: f32 = 0.08,
        #[setting(label = "Way")]
        way: PulseWay = PulseWay::Outwards,
        #[setting(label = "Angle", when = "way=a_way", span(-180, 180), unit = "°", keyframe)]
        angle: f32 = -90.0,
    }
);

force_settings!(
    /// Every hit sends out a ring that pushes particles as it passes.
    RippleForce {
        #[setting(label = "Hits from")]
        who_hits: Target = Target::Anything,
        #[setting(label = "Least speed", span(0, 2000), unit = "px/s", keyframe)]
        least_speed: f32 = 0.0,
        /// Harder hits push harder.
        #[setting(label = "Follow impact")]
        follow_impact: bool = true,
        /// How fast the ring spreads.
        #[setting(label = "Speed", range(1, 1e9), span(50, 3000), unit = "px/s", keyframe)]
        speed: f32 = 900.0,
        #[setting(label = "Width", range(1, 1e9), span(5, 400), unit = "px", keyframe)]
        width: f32 = 60.0,
        #[setting(label = "Strength", span(-20000, 20000), unit = "px/s²", simple, keyframe, pulse)]
        strength: f32 = 4000.0,
        #[setting(label = "Lasts", range(0.01, 1e9), span(0.1, 5), unit = "s", keyframe)]
        lasts: f32 = 1.4,
    }
);

/// A pull or push as the maths reads it.
#[derive(Clone, Copy, Debug)]
pub struct Pulling {
    pub toward: Toward,
    pub strength: f32,
    pub falloff: Falloff,
    pub at: f32,
    pub reach: f32,
    pub ball_reach: f32,
    pub orbit: f32,
    pub swallow: f32,
    /// 1 pulls, −1 pushes.
    pub sign: f32,
}

impl From<&PullForce> for Pulling {
    fn from(f: &PullForce) -> Self {
        Pulling {
            toward: f.toward,
            strength: f.strength,
            falloff: f.falloff,
            at: f.at,
            reach: f.reach,
            ball_reach: f.ball_reach,
            orbit: f.orbit,
            swallow: f.swallow,
            sign: 1.0,
        }
    }
}

impl From<&PushForce> for Pulling {
    fn from(f: &PushForce) -> Self {
        Pulling {
            toward: f.toward,
            strength: f.strength,
            falloff: f.falloff,
            at: f.at,
            reach: f.reach,
            ball_reach: f.ball_reach,
            orbit: f.orbit,
            swallow: 0.0,
            sign: -1.0,
        }
    }
}

/// A number that is finite, else 0.
fn fin(x: f32) -> f32 {
    if x.is_finite() { x } else { 0.0 }
}

/// The nearest distance a pull is worked out at, so nothing divides by nothing.
const SOFT: f32 = 4.0;
/// The most a falloff multiplies Strength by.
const MOST_FACTOR: f32 = 100.0;

/// What a fixed-centre force acts from for particle `s`: the step's centre, else (Its ball)
/// the particle's own ball, else its parent's middle, else the thing's middle. With velocity.
pub fn centre_for(s: &Speck, toward: Toward, ready: &Ready, fr: &ForceReady) -> Option<(Vec2, Vec2)> {
    match (toward, fr) {
        (Toward::ItsBall, _) => {
            let own = (s.ball as usize).checked_sub(1).and_then(|r| ready.parent_balls.get(r).copied().flatten());
            Some(match own {
                Some(b) => (b.place, b.velocity),
                None => ready.parent_middle.unwrap_or((ready.middle, Vec2::ZERO)),
            })
        }
        (_, ForceReady::Centre(c)) => *c,
        _ => None,
    }
}

/// Pulls (or pushes) `s` by `p` towards `centre`; `at` is the distance Strength is felt at and
/// `reach` how far it reaches (0 everywhere).
fn pull_from(s: &mut Speck, p: &Pulling, centre: Vec2, at: f32, reach: f32, amount: f32, dt: f32) {
    let off = centre - s.place;
    let d = off.length();
    if p.sign > 0.0 && p.swallow > 0.0 && d < p.swallow {
        s.flags |= DEAD;
        return;
    }
    if (reach > 0.0 && d > reach) || d < 1e-6 || !d.is_finite() {
        return;
    }
    let inward = off / d;
    let soft = d.max(SOFT).max(fin(p.swallow));
    let at = fin(at).max(1e-3);
    let factor = match p.falloff {
        Falloff::Even => 1.0,
        Falloff::WeakerFurther => at / soft,
        Falloff::MuchWeakerFurther => (at / soft).powi(2),
    }
    .min(MOST_FACTOR);
    let a = fin(p.strength) * factor * amount;
    s.velocity += (inward * p.sign + inward.perp() * fin(p.orbit)) * a * dt;
}

/// Pull to and Push away from.
pub fn pull(s: &mut Speck, p: &Pulling, amount: f32, dt: f32, ready: &Ready, fr: &ForceReady) {
    if p.toward == Toward::BallsNearby {
        let Some(grid) = &ready.balls else { return };
        // Reach 0 reaches nothing here (0 "everywhere" is for a single centre): the grid only
        // lists balls near the particle, and only when some force asks for one.
        if fin(p.ball_reach) <= 0.0 {
            return;
        }
        for &i in grid.near(s.place) {
            let b = grid.balls[i as usize];
            pull_from(s, p, b.place, b.radius.max(1.0), fin(p.ball_reach).max(0.0) * b.radius, amount, dt);
            if s.flags & DEAD != 0 {
                return;
            }
        }
        return;
    }
    if let Some((centre, _)) = centre_for(s, p.toward, ready, fr) {
        pull_from(s, p, centre, p.at, fin(p.reach).max(0.0), amount, dt);
    }
}

/// Spring home.
pub fn spring(s: &mut Speck, f: &SpringForce, amount: f32, dt: f32, ready: &Ready) {
    let home = ready.homes.home_of(s);
    s.velocity += (home - s.place) * fin(f.stiffness).max(0.0) * amount * dt;
    s.velocity *= (-fin(f.damping).max(0.0) * amount * dt).exp();
}

/// Flock: particle `index` of the step's snapshot.
pub fn flock(s: &mut Speck, index: usize, f: &FlockForce, amount: f32, dt: f32, ready: &Ready) {
    let Some(mates) = &ready.mates else { return };
    let reach = fin(f.reach).max(1.0);
    let apart = reach * fin(f.apart).clamp(0.0, 1.0);
    let (mut n, mut middle, mut heading, mut push) = (0u32, Vec2::ZERO, Vec2::ZERO, Vec2::ZERO);
    mates.near(index, s.place, reach, |place, velocity| {
        let off = place - s.place;
        n += 1;
        middle += off;
        heading += velocity;
        if off.length_squared() < apart * apart {
            push -= off;
        }
    });
    if n > 0 {
        let n = n as f32;
        let a = push * fin(f.separation) + middle / n * fin(f.cohesion) + (heading / n - s.velocity) * fin(f.alignment);
        s.velocity += a * amount * dt;
    }
    // Held between Least and Most speed.
    let (least, most) = (fin(f.least).max(0.0), fin(f.most).max(0.0));
    let speed = s.velocity.length();
    let want = speed.clamp(least.min(most), most.max(least));
    let blend = amount.clamp(0.0, 1.0);
    if speed > 1e-6 {
        s.velocity *= 1.0 + (want / speed - 1.0) * blend;
    } else if want > 0.0 {
        s.velocity = Vec2::from_angle(unit(s.seed, 75) * TAU) * want * blend;
    }
    if s.velocity.length_squared() > 1e-12 {
        s.turn = s.velocity.y.atan2(s.velocity.x);
    }
}

/// The field of a magnet at `centre` pointing `m`, full strength within `size`, at `p`: its way
/// (times its strength, at most 1) and the pull towards the magnet (times its strength).
fn dipole(p: Vec2, centre: Vec2, m: Vec2, size: f32) -> Option<(Vec2, Vec2)> {
    let r = p - centre;
    let d = r.length();
    if !(d > 1e-6 && d < 4.0 * size && size > 0.0) {
        return None;
    }
    let n = r / d;
    let strength = (size / d).powi(3).min(1.0);
    let b = (n * 3.0 * m.dot(n) - m).try_normalize()?;
    Some((b * strength, -n * strength))
}

/// Magnetic field.
pub fn magnetic(s: &mut Speck, f: &MagneticForce, amount: f32, dt: f32, ready: &Ready) {
    let fixed = Vec2::from_angle(fin(f.angle).to_radians());
    let (mut field, mut toward) = (Vec2::ZERO, Vec2::ZERO);
    match f.from {
        FieldFrom::EachBall => {
            let Some(grid) = &ready.balls else { return };
            for &i in grid.near(s.place) {
                let b = grid.balls[i as usize];
                let m = match f.points {
                    FieldPoints::AlongItsMotion => b.velocity.try_normalize().unwrap_or(Vec2::NEG_Y),
                    FieldPoints::AWay => fixed,
                };
                if let Some((bf, pull)) = dipole(s.place, b.place, m, fin(f.size).max(0.0) * b.radius) {
                    field += bf;
                    toward += pull;
                }
            }
        }
        FieldFrom::ItsPlace => {
            if let Some((bf, pull)) = dipole(s.place, ready.middle, fixed, fin(f.size_px).max(0.0)) {
                field = bf;
                toward = pull;
            }
        }
    }
    let strength = field.length().min(1.0);
    if strength > 1e-4 {
        let want = field.y.atan2(field.x);
        // A needle: either end may point along the field.
        let mut gap = (want - s.turn + PI).rem_euclid(TAU) - PI;
        if gap.abs() > PI * 0.5 {
            gap -= PI * gap.signum();
        }
        let rate = (fin(f.line_up).max(0.0) * (strength + 0.06) * amount * dt).min(1.0);
        s.turn += gap * rate;
        s.turn = (s.turn + PI).rem_euclid(TAU) - PI;
    }
    s.velocity += toward * fin(f.pull) * amount * dt;
}

/// Flow field.
pub fn flow(s: &mut Speck, f: &FlowForce, amount: f32, dt: f32, ready: &Ready) {
    let t = ready.time as f32;
    let size = fin(f.size).max(1.0);
    let way = match f.pattern {
        Pattern::Noise => {
            let a = noise(s.place / size + Vec2::new(0.0, t * fin(f.changes))) * TAU * 2.0;
            Vec2::from_angle(a)
        }
        Pattern::Waves => {
            let base = fin(f.angle).to_radians();
            let across = s.place.dot(Vec2::from_angle(base).perp());
            let sway = fin(f.sway).to_radians() * (TAU * (across / size - t * fin(f.changes))).sin();
            Vec2::from_angle(base + sway)
        }
        Pattern::Spiral | Pattern::Vortex => {
            let off = s.place - ready.middle;
            let d = off.length();
            if d < 1e-6 {
                return;
            }
            let round = off.perp() / d;
            if f.pattern == Pattern::Spiral {
                let a = fin(f.pull_in).to_radians();
                round * a.cos() - off / d * a.sin()
            } else {
                round * if d < size { d / size } else { size / d }
            }
        }
    };
    let take = 1.0 - (-fin(f.grip).max(0.0) * amount * dt).exp();
    s.velocity += (way * fin(f.speed) - s.velocity) * take;
}

/// Where particle `s`'s spirograph pen is `age` seconds into its life, round the middle.
pub fn pen_at(s: &Speck, f: &SpirographForce, age: f32) -> Vec2 {
    let differ = fin(f.differ);
    let big = fin(f.ring) * (1.0 + differ * signed(s.seed, 70));
    let small = (fin(f.wheel) * (1.0 + differ * signed(s.seed, 71))).abs().max(1.0);
    let pen = fin(f.pen) * (1.0 + differ * signed(s.seed, 72));
    let theta = TAU * (fin(f.speed) * age + unit(s.seed, 73));
    let k = (big - small) / small;
    Vec2::new(
        (big - small) * theta.cos() + pen * (k * theta).cos(),
        (big - small) * theta.sin() - pen * (k * theta).sin(),
    )
}

/// Spirograph.
pub fn spirograph(s: &mut Speck, f: &SpirographForce, age: f32, amount: f32, dt: f32, ready: &Ready, fr: &ForceReady) {
    let Some((centre, moving)) = centre_for(s, f.round.toward(), ready, fr) else { return };
    let target = centre + moving * dt + pen_at(s, f, age + dt);
    let follow = (target - s.place) / dt;
    if follow.is_finite() {
        s.velocity = s.velocity.lerp(follow, amount.clamp(0.0, 1.0));
    }
}

/// How much of a beat's kick falls in the `dt` before `since` seconds after it.
fn kick_share(since: f64, dt: f64, decay: f64) -> f64 {
    let f = |x: f64| {
        if x < 0.0 {
            0.0
        } else if decay <= 0.0 {
            1.0
        } else {
            1.0 - (-x / decay).exp()
        }
    };
    f(since) - f(since - dt)
}

/// How much of every beat's kick so far falls in the `dt` before now: the beats within `dt` one
/// by one, the older ones' tails summed as a geometric series (each is `e^(−x/d)·(e^(dt/d) − 1)`
/// at `x` seconds after its beat), so a long Decay still gives each kick its whole Strength.
pub fn pulse_share(b: &Beats, dt: f64, decay: f64) -> f64 {
    let mut share = 0.0;
    let mut m = 0;
    while m < b.count {
        let x = b.since + m as f64 * b.every;
        if x >= dt {
            break;
        }
        share += kick_share(x, dt, decay);
        m += 1;
    }
    if m < b.count && decay > 0.0 && b.every > 0.0 {
        let x = b.since + m as f64 * b.every;
        let r = (-b.every / decay).exp();
        let left = (b.count - m) as f64;
        let series = if r < 1.0 { (1.0 - r.powf(left)) / (1.0 - r) } else { left };
        share += ((-(x - dt) / decay).exp() - (-x / decay).exp()) * series;
    }
    share
}

/// Pulse on the beat.
pub fn pulse(s: &mut Speck, f: &PulseForce, amount: f32, dt: f32, ready: &Ready, fr: &ForceReady) {
    let ForceReady::Beats(Some(beats)) = fr else { return };
    let share = pulse_share(beats, dt as f64, fin(f.decay).max(0.0) as f64);
    if share == 0.0 {
        return;
    }
    let way = match f.way {
        PulseWay::Outwards => (s.place - ready.middle).try_normalize().unwrap_or(Vec2::NEG_Y),
        PulseWay::Up => Vec2::NEG_Y,
        PulseWay::AWay => Vec2::from_angle(fin(f.angle).to_radians()),
        PulseWay::AllRound => Vec2::from_angle(unit(s.seed, 74) * TAU),
    };
    s.velocity += way * fin(f.strength) * amount * share as f32;
}

/// Ripple from hits.
pub fn ripple(s: &mut Speck, f: &RippleForce, amount: f32, dt: f32, fr: &ForceReady) {
    let ForceReady::Rings(rings) = fr else { return };
    let (speed, width, lasts) = (fin(f.speed).max(1.0), fin(f.width).max(1.0), fin(f.lasts).max(0.01));
    for ring in rings {
        let off = s.place - ring.centre;
        let d = off.length();
        let x = (d - speed * ring.age).abs() / width;
        if x >= 1.0 || d < 1e-6 {
            continue;
        }
        let fade = (1.0 - ring.age / lasts).max(0.0);
        let bump = (1.0 - x * x).powi(2);
        s.velocity += off / d * fin(f.strength) * ring.impact * fade * bump * amount * dt;
    }
}

/// Whether `only` (with its age and share) lets a force act on `s` at `age`.
pub fn acts_on(only: OnlyFor, at: f32, share: f32, s: &Speck, age: f32) -> bool {
    match only {
        OnlyFor::All => true,
        OnlyFor::YoungerThan => age < at,
        OnlyFor::OlderThan => age >= at,
        // The same split as a Look layer's Only: A share.
        OnlyFor::AShare => unit(s.seed, 61) < share,
        OnlyFor::TheRest => unit(s.seed, 61) >= share,
    }
}

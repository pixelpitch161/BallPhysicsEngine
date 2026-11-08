//! A Particles thing's Motion card (`motion.particles`): a list of force layers, applied in list
//! order every particle step. Each has an Amount (how much of it acts) and an Only for (every
//! particle, the young ones, the old ones, a share of them or the rest). Part 1's forces: Gravity,
//! Wind, Drag, Swirl, Wander, Rise, Sway and flutter; part 2's: Follow the air (and Rise heating
//! the air, Wind's gust waves); part 3's are in `more_forces.rs`.

use bpe_core::{Choice, LayerSection, ModuleKind, ResolvedSlot, SettingDef, Settings, SlotId};
use glam::Vec2;

use super::more_forces::{self as more, FlockForce, FlowForce, MagneticForce, PullForce, PulseForce, PushForce};
use super::more_forces::{RippleForce, SpirographForce, SpringForce};
use super::ready::Ready;
use super::state::Speck;
use crate::air::gust_factor;
use crate::modules::look::layers::Kind;
use crate::modules::look::layers::particles::emit::hash;
use crate::modules::look::layers::particles::sim::{curl, noise};

/// The card kind.
pub const CARD: &str = "motion.particles";
pub const GRAVITY: &str = "force.gravity";
pub const WIND: &str = "force.wind";
pub const DRAG: &str = "force.drag";
pub const SWIRL: &str = "force.swirl";
pub const WANDER: &str = "force.wander";
pub const RISE: &str = "force.rise";
pub const SWAY: &str = "force.sway";
pub const AIR: &str = "force.air";

/// Which particles a force acts on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum OnlyFor {
    #[default]
    All,
    /// Particles younger than Age.
    YoungerThan,
    /// Particles older than Age.
    OlderThan,
    /// A share of the particles (the same ones a Look layer's Only: A share picks).
    AShare,
    /// The other particles.
    TheRest,
}

macro_rules! force_settings {
    ($(#[$meta:meta])* $name:ident { $($(#[$fmeta:meta])* $field:ident : $ty:ty = $default:expr),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Settings)]
        pub struct $name {
            /// How much of the force acts: 0 none, 1 all of it (more pushes harder).
            #[setting(label = "Amount", span(0, 1), simple, keyframe, pulse)]
            pub amount: f32,
            #[setting(label = "Only for")]
            pub only: OnlyFor,
            #[setting(label = "Age", when = "only=younger_than|older_than", range(0, 1e9), span(0, 10), unit = "s", keyframe)]
            pub age: f32,
            /// How many of the particles it acts on (A share), or doesn't (The rest).
            #[setting(label = "Share", when = "only=a_share|the_rest", range(0, 1), span(0, 1), keyframe)]
            pub share: f32,
            $($(#[$fmeta])* pub $field: $ty,)*
        }

        impl Default for $name {
            fn default() -> Self {
                Self { amount: 1.0, only: OnlyFor::All, age: 1.0, share: 0.5, $($field: $default,)* }
            }
        }
    };
}
pub(crate) use force_settings;

force_settings!(
    /// A steady pull one way.
    GravityForce {
        #[setting(label = "Strength", when = "!scene", span(-2000, 2000), unit = "px/s²", simple, keyframe, pulse)]
        strength: f32 = 400.0,
        /// The way it pulls, in degrees: 90 is down.
        #[setting(label = "Direction", when = "!scene", span(-180, 180), unit = "°", keyframe)]
        direction: f32 = 90.0,
        /// Pull as the scene's gravity does.
        #[setting(label = "Scene's gravity")]
        scene: bool = false,
    }
);

force_settings!(
    /// Air moving one way: particles are carried towards its speed, by Grip.
    WindForce {
        #[setting(label = "Speed", span(0, 2000), unit = "px/s", simple, keyframe, pulse)]
        speed: f32 = 200.0,
        #[setting(label = "Direction", span(-180, 180), unit = "°", keyframe)]
        direction: f32 = 0.0,
        /// How quickly particles take on the wind's speed.
        #[setting(label = "Grip", range(0, 1e9), span(0, 10), unit = "per s", keyframe)]
        grip: f32 = 2.0,
        /// How much stronger and weaker it blows by turns.
        #[setting(label = "Gusts", span(0, 1), keyframe)]
        gusts: f32 = 0.0,
        #[setting(label = "Gust every", when = "gusts", range(0.01, 1e9), span(0.1, 10), unit = "s", keyframe)]
        gust_every: f32 = 2.0,
        /// The distance between gusts as they travel across the picture: 0 gusts everywhere at
        /// once.
        #[setting(label = "Gust waves", when = "gusts", range(0, 1e9), span(0, 2000), unit = "px", keyframe)]
        waves: f32 = 0.0,
    }
);

force_settings!(
    /// Slows particles down.
    DragForce {
        #[setting(label = "Drag", range(0, 1e9), span(0, 10), unit = "per s", simple, keyframe, pulse)]
        drag: f32 = 1.0,
    }
);

force_settings!(
    /// Eddies: particles are turned round in swirls.
    SwirlForce {
        #[setting(label = "Strength", span(0, 2000), unit = "px/s²", simple, keyframe, pulse)]
        strength: f32 = 300.0,
        #[setting(label = "Size", range(1, 1e9), span(10, 1000), unit = "px", keyframe)]
        size: f32 = 150.0,
        /// How fast the swirls drift upwards.
        #[setting(label = "Drift", span(-500, 500), unit = "px/s", keyframe)]
        drift: f32 = 30.0,
    }
);

force_settings!(
    /// Each particle wanders its own way, turning now and then.
    WanderForce {
        #[setting(label = "Strength", span(0, 2000), unit = "px/s²", simple, keyframe, pulse)]
        strength: f32 = 200.0,
        #[setting(label = "Turns", range(0, 1e9), span(0, 10), unit = "per s", keyframe)]
        turns: f32 = 1.0,
    }
);

force_settings!(
    /// Heat: particles rise, less as they cool.
    RiseForce {
        #[setting(label = "Strength", span(-2000, 2000), unit = "px/s²", simple, keyframe, pulse)]
        strength: f32 = 300.0,
        /// How long they take to cool to a third: 0 never cools.
        #[setting(label = "Cools over", range(0, 1e9), span(0, 10), unit = "s", keyframe)]
        cools: f32 = 0.0,
        /// How much each rising particle warms the air round it (a unit of heat a second), so the
        /// air rises too and carries particles that follow it.
        #[setting(label = "Heats the air", range(0, 1e9), span(0, 1), unit = "per s", keyframe)]
        heats: f32 = 0.0,
    }
);

force_settings!(
    /// Rocking side to side as they go, and flipping over (falling leaves, confetti).
    SwayForce {
        #[setting(label = "Sway", span(0, 200), unit = "px", simple, keyframe, pulse)]
        sway: f32 = 20.0,
        #[setting(label = "Sway speed", range(0, 1e9), span(0, 10), unit = "per s", keyframe)]
        sway_speed: f32 = 1.0,
        /// How often they flip over.
        #[setting(label = "Flutter", range(0, 1e9), span(0, 20), unit = "per s", keyframe)]
        flutter: f32 = 0.0,
        /// How far each flip carries them sideways.
        #[setting(label = "Flutter drift", when = "flutter", span(0, 300), unit = "px/s", keyframe)]
        flutter_drift: f32 = 30.0,
    }
);

force_settings!(
    /// Carried by the air: the scene's wind, stirred by the balls and lifted by heat (the Air
    /// card).
    FollowAirForce {
        /// How quickly particles take on the air's speed.
        #[setting(label = "Grip", range(0, 1e9), span(0, 20), unit = "per s", simple, keyframe, pulse)]
        grip: f32 = 4.0,
    }
);

impl RiseForce {
    /// How much of its strength is left `age` seconds into a particle's life.
    pub fn cool(&self, age: f32) -> f32 {
        if self.cools > 0.0 { (-age / self.cools).exp() } else { 1.0 }
    }
}

/// One force as the step uses it.
#[derive(Clone, Debug, PartialEq)]
pub enum Force {
    Gravity(GravityForce),
    Wind(WindForce),
    Drag(DragForce),
    Swirl(SwirlForce),
    Wander(WanderForce),
    Rise(RiseForce),
    Sway(SwayForce),
    Pull(PullForce),
    Push(PushForce),
    Spring(SpringForce),
    Flock(FlockForce),
    Magnetic(MagneticForce),
    Flow(FlowForce),
    Spirograph(SpirographForce),
    Pulse(PulseForce),
    Ripple(RippleForce),
    Air(FollowAirForce),
}

impl Force {
    /// The force of layer kind `kind` with settings `p`; `None` for a kind that isn't one.
    pub fn read(kind: &str, p: &bpe_core::Params) -> Option<Force> {
        Some(match kind {
            GRAVITY => Force::Gravity(GravityForce::from_params(p)),
            WIND => Force::Wind(WindForce::from_params(p)),
            DRAG => Force::Drag(DragForce::from_params(p)),
            SWIRL => Force::Swirl(SwirlForce::from_params(p)),
            WANDER => Force::Wander(WanderForce::from_params(p)),
            RISE => Force::Rise(RiseForce::from_params(p)),
            SWAY => Force::Sway(SwayForce::from_params(p)),
            more::PULL => Force::Pull(PullForce::from_params(p)),
            more::PUSH => Force::Push(PushForce::from_params(p)),
            more::SPRING => Force::Spring(SpringForce::from_params(p)),
            more::FLOCK => Force::Flock(FlockForce::from_params(p)),
            more::MAGNETIC => Force::Magnetic(MagneticForce::from_params(p)),
            more::FLOW => Force::Flow(FlowForce::from_params(p)),
            more::SPIROGRAPH => Force::Spirograph(SpirographForce::from_params(p)),
            more::PULSE => Force::Pulse(PulseForce::from_params(p)),
            more::RIPPLE => Force::Ripple(RippleForce::from_params(p)),
            AIR => Force::Air(FollowAirForce::from_params(p)),
            _ => return None,
        })
    }

    /// Amount, Only for, Age and Share.
    pub fn common(&self) -> (f32, OnlyFor, f32, f32) {
        match self {
            Force::Gravity(f) => (f.amount, f.only, f.age, f.share),
            Force::Wind(f) => (f.amount, f.only, f.age, f.share),
            Force::Drag(f) => (f.amount, f.only, f.age, f.share),
            Force::Swirl(f) => (f.amount, f.only, f.age, f.share),
            Force::Wander(f) => (f.amount, f.only, f.age, f.share),
            Force::Rise(f) => (f.amount, f.only, f.age, f.share),
            Force::Sway(f) => (f.amount, f.only, f.age, f.share),
            Force::Pull(f) => (f.amount, f.only, f.age, f.share),
            Force::Push(f) => (f.amount, f.only, f.age, f.share),
            Force::Spring(f) => (f.amount, f.only, f.age, f.share),
            Force::Flock(f) => (f.amount, f.only, f.age, f.share),
            Force::Magnetic(f) => (f.amount, f.only, f.age, f.share),
            Force::Flow(f) => (f.amount, f.only, f.age, f.share),
            Force::Spirograph(f) => (f.amount, f.only, f.age, f.share),
            Force::Pulse(f) => (f.amount, f.only, f.age, f.share),
            Force::Ripple(f) => (f.amount, f.only, f.age, f.share),
            Force::Air(f) => (f.amount, f.only, f.age, f.share),
        }
    }

    /// Whether it is one of part 3's forces, which read what the step gathered ([`Ready`]).
    pub fn is_more(&self) -> bool {
        !matches!(
            self,
            Force::Gravity(_)
                | Force::Wind(_)
                | Force::Drag(_)
                | Force::Swirl(_)
                | Force::Wander(_)
                | Force::Rise(_)
                | Force::Sway(_)
                | Force::Air(_)
        )
    }
}

/// The Motion card as the step uses it: its shown forces, in list order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Forces {
    pub list: Vec<Force>,
}

/// What a force reads besides the particle.
#[derive(Clone, Copy, Debug)]
pub struct ForceCx<'a> {
    /// Seconds since the start.
    pub time: f32,
    pub dt: f32,
    /// The scene's gravity.
    pub gravity: Vec2,
    /// The air, when the world has one awake (Follow the air reads it).
    pub air: Option<&'a crate::air::Air>,
}

/// A number that is finite, else 0.
fn fin(x: f32) -> f32 {
    if x.is_finite() { x } else { 0.0 }
}

impl Forces {
    /// The card's shown force layers, in list order.
    pub fn read(card: Option<&ResolvedSlot>) -> Forces {
        let Some(card) = card else { return Forces::default() };
        let list = card.layers.iter().filter_map(|l| Force::read(&l.kind, &card.layer_params(l.id))).collect();
        Forces { list }
    }

    /// Changes `s`'s velocity (and its flip and turn) by every force, `age` seconds into its life.
    /// `index` is its place in the step's particles and `ready` what the step gathered.
    pub fn apply(&self, s: &mut Speck, index: usize, age: f32, cx: &ForceCx<'_>, ready: &Ready) {
        let dt = cx.dt;
        for (k, f) in self.list.iter().enumerate() {
            if s.flags & super::state::DEAD != 0 {
                return;
            }
            let (amount, only, at, share) = f.common();
            let amount = fin(amount);
            if amount == 0.0 || !more::acts_on(only, at, share, s, age) {
                continue;
            }
            match f {
                Force::Gravity(g) => {
                    let pull = if g.scene {
                        cx.gravity
                    } else {
                        Vec2::from_angle(fin(g.direction).to_radians()) * fin(g.strength)
                    };
                    s.velocity += pull * amount * dt;
                }
                Force::Wind(w) => {
                    let way = Vec2::from_angle(fin(w.direction).to_radians());
                    let gust = gust_factor(fin(w.gusts), w.gust_every, fin(w.waves), cx.time, s.place, way);
                    let wind = way * fin(w.speed) * gust;
                    let take = 1.0 - (-fin(w.grip).max(0.0) * amount * dt).exp();
                    s.velocity += (wind - s.velocity) * take;
                }
                Force::Drag(d) => s.velocity *= (-fin(d.drag).max(0.0) * amount * dt).exp(),
                Force::Swirl(w) => {
                    let size = w.size.max(1.0);
                    let at = (s.place + Vec2::new(0.0, fin(w.drift) * cx.time)) / size;
                    s.velocity += curl(at) * fin(w.strength) * amount * dt;
                }
                Force::Wander(w) => {
                    let phase = age * fin(w.turns).max(0.0);
                    let k = (s.seed & 0xffff) as f32 * 0.013;
                    let a = noise(Vec2::new(phase, k)) * std::f32::consts::TAU * 1.5;
                    s.velocity += Vec2::from_angle(a) * fin(w.strength) * amount * dt;
                }
                Force::Rise(r) => {
                    s.velocity.y -= fin(r.strength) * r.cool(age) * amount * dt;
                }
                Force::Air(a) => {
                    if let Some(air) = cx.air {
                        let take = 1.0 - (-fin(a.grip).max(0.0) * amount * dt).exp();
                        s.velocity += (air.at(s.place) - s.velocity) * take;
                    }
                }
                Force::Sway(w) => {
                    // A sideways sway of Sway pixels either way: its velocity's change this step.
                    let omega = std::f32::consts::TAU * fin(w.sway_speed).max(0.0);
                    let phase0 = bpe_core::dice::unit24(hash(&[s.seed, 0x5a7])) * std::f32::consts::TAU;
                    let v = |t: f32| fin(w.sway) * omega * (omega * t + phase0).cos();
                    s.velocity.x += (v(age) - v((age - dt).max(0.0))) * amount;
                    if w.flutter > 0.0 {
                        s.flip += std::f32::consts::TAU * fin(w.flutter) * amount * dt;
                        s.velocity.x += fin(w.flutter_drift) * s.flip.sin() * amount * dt * 4.0;
                    }
                }
                Force::Pull(p) => more::pull(s, &p.into(), amount, dt, ready, ready.of(k)),
                Force::Push(p) => more::pull(s, &p.into(), amount, dt, ready, ready.of(k)),
                Force::Spring(p) => more::spring(s, p, amount, dt, ready),
                Force::Flock(p) => more::flock(s, index, p, amount, dt, ready),
                Force::Magnetic(p) => more::magnetic(s, p, amount, dt, ready),
                Force::Flow(p) => more::flow(s, p, amount, dt, ready),
                Force::Spirograph(p) => more::spirograph(s, p, age, amount, dt, ready, ready.of(k)),
                Force::Pulse(p) => more::pulse(s, p, amount, dt, ready, ready.of(k)),
                Force::Ripple(p) => more::ripple(s, p, amount, dt, ready.of(k)),
            }
        }
    }

    /// Whether any force reads the air.
    pub fn follows_air(&self) -> bool {
        self.list.iter().any(|f| matches!(f, Force::Air(_)))
    }

    /// Whether any force warms the air.
    pub fn heats_air(&self) -> bool {
        self.list.iter().any(|f| matches!(f, Force::Rise(r) if r.heats != 0.0))
    }

    /// The heat particle `s`, `age` seconds old, puts into the air over `dt` (the Rise forces that
    /// heat and act on it).
    pub fn heat_of(&self, s: &Speck, age: f32, dt: f32) -> f32 {
        let mut heat = 0.0;
        for f in &self.list {
            let Force::Rise(r) = f else { continue };
            let (amount, only, at, share) = f.common();
            if r.heats == 0.0 || !more::acts_on(only, at, share, s, age) {
                continue;
            }
            heat += fin(r.heats).max(0.0) * r.cool(age) * fin(amount).max(0.0) * dt;
        }
        heat
    }

    /// Whether any force flips particles over (the draw squashes them by it).
    pub fn flips(&self) -> bool {
        self.list.iter().any(|f| matches!(f, Force::Sway(s) if s.flutter > 0.0))
    }
}

/// The Motion card of a Particles thing.
pub struct ForcesCard;

impl ModuleKind for ForcesCard {
    fn id(&self) -> &'static str {
        CARD
    }
    fn name(&self) -> &'static str {
        "Forces"
    }
    fn help(&self) -> &'static str {
        "What moves the particles: a list of forces, each applied in turn from the top."
    }
    fn slot(&self) -> SlotId {
        SlotId::Motion
    }
    fn settings(&self) -> &'static [SettingDef] {
        &[]
    }
    fn fits(&self, thing_kind: &str) -> bool {
        thing_kind == super::KIND
    }
}

/// The force layer kinds, in the add menu's order.
pub fn kinds() -> Vec<Kind> {
    let force = |id, name, help, settings| Kind {
        id,
        name,
        help,
        section: LayerSection::Force,
        repeats: true,
        base: false,
        card: CARD,
        takes_subs: false,
        settings,
        dynamic: crate::modules::look::layers::no_more,
    };
    vec![
        force(GRAVITY, "Gravity", "A steady pull one way: down, or as the scene's gravity pulls.", GravityForce::defs),
        force(WIND, "Wind", "Air moving one way that carries the particles along, with gusts.", WindForce::defs),
        force(DRAG, "Drag", "Slows the particles down, as air does.", DragForce::defs),
        force(SWIRL, "Swirl", "Eddies that turn the particles round as they go.", SwirlForce::defs),
        force(WANDER, "Wander", "Each particle wanders off its own way, turning now and then.", WanderForce::defs),
        force(RISE, "Rise", "Heat: the particles rise, less and less as they cool.", RiseForce::defs),
        force(SWAY, "Sway and flutter", "Rocking side to side, and flipping over as they fall.", SwayForce::defs),
        force(
            AIR,
            "Follow the air",
            "Carried by the air: the scene's wind, stirred up by the balls and lifted by heat (the Air card).",
            FollowAirForce::defs,
        ),
        force(
            more::PULL,
            "Pull to",
            "Pulls the particles towards a place, a ball or the balls near them.",
            PullForce::defs,
        ),
        force(
            more::PUSH,
            "Push away from",
            "Pushes the particles away from a place or a ball: flee, comet tails.",
            PushForce::defs,
        ),
        force(
            more::SPRING,
            "Spring home",
            "Springs each particle back to its home: its place in a word or grid, or where it was born.",
            SpringForce::defs,
        ),
        force(more::FLOCK, "Flock", "The particles fly together like birds or fish.", FlockForce::defs),
        force(
            more::MAGNETIC,
            "Magnetic field",
            "The particles line up with the balls' magnetic field, like iron filings.",
            MagneticForce::defs,
        ),
        force(
            more::FLOW,
            "Flow field",
            "Carries the particles along a current: noise, a spiral, waves or a vortex.",
            FlowForce::defs,
        ),
        force(more::SPIROGRAPH, "Spirograph", "The particles loop round on spirograph curves.", SpirographForce::defs),
        force(more::PULSE, "Pulse on the beat", "A kick on every beat or bar of the scene's tempo.", PulseForce::defs),
        force(
            more::RIPPLE,
            "Ripple from hits",
            "Every hit sends out a ring that pushes the particles as it passes.",
            RippleForce::defs,
        ),
    ]
}

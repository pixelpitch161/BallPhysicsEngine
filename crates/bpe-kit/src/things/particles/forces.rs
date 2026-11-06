//! A Particles thing's Motion card (`motion.particles`): a list of force layers, applied in list
//! order every particle step. Each has an Amount (how much of it acts) and an Only for (every
//! particle, the young ones or the old ones). Part 1's forces: Gravity, Wind, Drag, Swirl, Wander,
//! Rise, Sway and flutter.

use bpe_core::{Choice, LayerSection, ModuleKind, ResolvedSlot, SettingDef, Settings, SlotId};
use glam::Vec2;

use super::state::Speck;
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

/// Which particles a force acts on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum OnlyFor {
    #[default]
    All,
    /// Particles younger than Age.
    YoungerThan,
    /// Particles older than Age.
    OlderThan,
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
            #[setting(label = "Age", when = "only!=all", range(0, 1e9), span(0, 10), unit = "s", keyframe)]
            pub age: f32,
            $($(#[$fmeta])* pub $field: $ty,)*
        }

        impl Default for $name {
            fn default() -> Self {
                Self { amount: 1.0, only: OnlyFor::All, age: 1.0, $($field: $default,)* }
            }
        }
    };
}

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
            _ => return None,
        })
    }

    fn common(&self) -> (f32, OnlyFor, f32) {
        match self {
            Force::Gravity(f) => (f.amount, f.only, f.age),
            Force::Wind(f) => (f.amount, f.only, f.age),
            Force::Drag(f) => (f.amount, f.only, f.age),
            Force::Swirl(f) => (f.amount, f.only, f.age),
            Force::Wander(f) => (f.amount, f.only, f.age),
            Force::Rise(f) => (f.amount, f.only, f.age),
            Force::Sway(f) => (f.amount, f.only, f.age),
        }
    }
}

/// The Motion card as the step uses it: its shown forces, in list order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Forces {
    pub list: Vec<Force>,
}

/// What a force reads besides the particle.
#[derive(Clone, Copy, Debug)]
pub struct ForceCx {
    /// Seconds since the start.
    pub time: f32,
    pub dt: f32,
    /// The scene's gravity.
    pub gravity: Vec2,
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

    /// Changes `s`'s velocity (and its flip) by every force, `age` seconds into its life.
    pub fn apply(&self, s: &mut Speck, age: f32, cx: &ForceCx) {
        let dt = cx.dt;
        for f in &self.list {
            let (amount, only, at) = f.common();
            let amount = fin(amount);
            if amount == 0.0 {
                continue;
            }
            let applies = match only {
                OnlyFor::All => true,
                OnlyFor::YoungerThan => age < at,
                OnlyFor::OlderThan => age >= at,
            };
            if !applies {
                continue;
            }
            match f {
                Force::Gravity(g) => {
                    let pull =
                        if g.scene { cx.gravity } else { Vec2::from_angle(fin(g.direction).to_radians()) * fin(g.strength) };
                    s.velocity += pull * amount * dt;
                }
                Force::Wind(w) => {
                    let gust = if w.gusts != 0.0 {
                        let phase = cx.time / w.gust_every.max(0.01);
                        1.0 + fin(w.gusts) * noise(Vec2::new(phase, 3.7))
                    } else {
                        1.0
                    };
                    let wind = Vec2::from_angle(fin(w.direction).to_radians()) * fin(w.speed) * gust;
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
                    let cool = if r.cools > 0.0 { (-age / r.cools).exp() } else { 1.0 };
                    s.velocity.y -= fin(r.strength) * cool * amount * dt;
                }
                Force::Sway(w) => {
                    // A sideways sway of Sway pixels either way: its velocity's change this step.
                    let omega = std::f32::consts::TAU * fin(w.sway_speed).max(0.0);
                    let phase0 = (hash(&[s.seed, 0x5a7]) >> 8) as f32 / (1u32 << 24) as f32 * std::f32::consts::TAU;
                    let v = |t: f32| fin(w.sway) * omega * (omega * t + phase0).cos();
                    s.velocity.x += (v(age) - v((age - dt).max(0.0))) * amount;
                    if w.flutter > 0.0 {
                        s.flip += std::f32::consts::TAU * fin(w.flutter) * amount * dt;
                        s.velocity.x += fin(w.flutter_drift) * s.flip.sin() * amount * dt * 4.0;
                    }
                }
            }
        }
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
    ]
}

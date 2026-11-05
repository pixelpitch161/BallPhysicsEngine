//! The Emitter: fires balls it designs itself (its own Look and Physics, like a Balls group),
//! round after round, catches balls that reach it, or both, with a marker that flashes and
//! ripples. `fire.rs` holds the firing maths, `sim.rs` the simulation, `draw.rs` the drawing and
//! `foci.rs` what it can sit on.

pub mod aim;
pub mod aim_cache;
pub mod draw;
pub mod fire;
pub mod foci;
pub mod sim;
pub mod test_fire;
#[cfg(test)]
pub(crate) mod test_scenes;

use bpe_core::{Choice, Color, SettingDef, Settings, SlotId, Target, ThingKind, VaryUse};

use crate::things::balls::BallsSettings;
use crate::things::balls::rows::RoundSpan;

/// The kind id.
pub const KIND: &str = "emitter";

/// The Emit tab's headings.
pub const FIRE: &str = "Fire";
pub const ROUNDS: &str = "Rounds";
pub const AIM: &str = "Aim";
/// The Catch tab's.
pub const CATCH: &str = "Catch";
pub const SEND_BACK: &str = "Send back out";
/// The Marker tab's.
pub const MARKER: &str = "Marker";
pub const FLASH: &str = "Flash";
pub const RIPPLE: &str = "Ripple";
/// Group mode's Variety fold.
pub const VARIETY: &str = "Variety";
/// The emitter's own size, shown beside the Look card's Ball / Group switch.
pub const SIZE: &str = "Size";

pub const EMIT_HEADINGS: [&str; 3] = [FIRE, ROUNDS, AIM];
pub const CATCH_HEADINGS: [&str; 2] = [CATCH, SEND_BACK];
pub const MARKER_HEADINGS: [&str; 3] = [MARKER, FLASH, RIPPLE];

pub struct EmitterKind;

/// Where the balls start from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum StartFrom {
    /// The emitter's middle.
    #[default]
    Point,
    /// A ring round it, each ball on the ring at its own direction.
    Ring,
    /// A line across the aim direction.
    Line,
}

/// Which ways a round's balls go.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Directions {
    /// Evenly all the way round, from the aim direction.
    #[default]
    AllRound,
    /// Evenly across a fan round the aim direction.
    Fan,
    /// All the aim way.
    OneWay,
    /// Each a way picked at random.
    Random,
}

/// The order a round's balls go in, when there is a gap between them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum FireOrder {
    /// Round from the aim direction (or along the fan or line).
    #[default]
    Around,
    Random,
    /// From the middle of the fan or line outwards, one side then the other.
    MiddleOut,
}

/// What starts a round.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum FireWhen {
    /// One round, after the start.
    #[default]
    AtStart,
    /// A round every so often.
    Every,
    /// A round each time a wave of balls lands on an emitter.
    WhenAWaveLands,
}

/// What happens to earlier rounds' balls when a new round fires.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Earlier {
    #[default]
    Keep,
    FadeOut,
    Remove,
}

/// How the balls are aimed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Aim {
    /// They go the ways Directions says, at Speed.
    #[default]
    Off,
    /// Each ball goes the way (and as fast) that takes it to the target at the time.
    StraightIn,
    /// Each ball keeps its own direction; how fast it goes and when it leaves are worked out so
    /// it reaches the target at the time.
    KeepMyDirections,
    /// Each ball keeps its own direction; how fast it goes and when it leaves are found by
    /// test-firing a lone copy of it, so it reaches the target at the time after bouncing off
    /// whatever is in the way.
    ThroughBounces,
}

/// What happens to an aimed ball that can't reach the target in time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum CantReach {
    /// It isn't fired.
    #[default]
    Skip,
    /// It goes all the same: as late as it must, or unaimed.
    FireAnyway,
}

/// Which focus of a shape the emitter sits on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum SitsFocus {
    #[default]
    Focus1,
    Focus2,
}

/// What happens to a ball that reaches the emitter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Catches {
    /// It isn't caught.
    #[default]
    Nothing,
    /// It is taken out.
    Remove,
    /// It is sent back out.
    SendBackOut,
    /// It flies on, but still counts as arriving.
    PassThrough,
}

/// Which balls are caught.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum CatchFrom {
    #[default]
    AnyBalls,
    ItsOwn,
    Chosen,
}

/// Which way a caught ball is sent back out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ReturnWay {
    /// Back the way it came.
    #[default]
    SameWayBack,
    /// Fired again from here, the way this emitter fires.
    OutMyWay,
}

/// What the emitter shows of itself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Marker {
    None,
    #[default]
    RingWithDot,
    Dot,
    Ring,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct EmitterSettings {
    /// Off, it fires nothing (it can still catch).
    #[setting(label = "Emits", group = "Fire")]
    pub emits: bool,
    /// How big each ball is. Shown beside the Look card's Ball / Group switch.
    #[setting(label = "Size", group = "Size", range(0.5, 200), unit = "px", simple, keyframe)]
    pub radius: f32,
    /// The shape whose focus the emitter sits on (a thing id), 0 for none: it stays where it is
    /// put.
    #[setting(label = "Sits on", group = "Fire")]
    pub sits_on: i32,
    /// Which of the shape's two foci.
    #[setting(label = "Focus", group = "Fire", when = "sits_on")]
    pub sits_focus: SitsFocus,
    /// Where the balls start from.
    #[setting(label = "Start from", group = "Fire")]
    pub start_from: StartFrom,
    /// How far from the middle the ring is.
    #[setting(label = "Radius", group = "Fire", when = "start_from=ring", range(1, 4000), unit = "px")]
    pub start_radius: f32,
    /// How long the line is.
    #[setting(label = "Length", group = "Fire", when = "start_from=line", range(1, 4000), unit = "px")]
    pub start_length: f32,
    /// Which ways the balls go.
    #[setting(label = "Directions", group = "Fire", when = "aim!=straight_in or start_from=ring", simple)]
    pub directions: Directions,
    /// The way the balls go, the middle of a fan, or where All round starts; clockwise from
    /// right, -90 is up.
    #[setting(
        label = "Aim direction",
        group = "Fire",
        when = "aim!=straight_in or start_from!=point",
        range(-180, 180),
        unit = "°",
        keyframe
    )]
    pub aim_angle: f32,
    /// How wide the fan opens.
    #[setting(
        label = "Fan angle",
        group = "Fire",
        when = "directions=fan and aim!=straight_in or directions=fan and start_from=ring",
        range(0, 360),
        unit = "°",
        keyframe
    )]
    pub fan_angle: f32,
    /// How fast the balls leave.
    #[setting(
        label = "Speed",
        group = "Fire",
        when = "aim!=straight_in",
        range(0, 20000),
        span(50, 2000),
        unit = "px/s",
        simple,
        keyframe
    )]
    pub speed: f32,
    /// How much each ball's speed differs, at random.
    #[setting(label = "Speed spread", group = "Fire", when = "aim=off", range(0, 1))]
    pub vary_speed: f32,
    /// How many balls the first round has.
    #[setting(label = "How many", group = "Fire", range(1, 20000), simple, keyframe)]
    pub count: u32,
    /// The time between one ball of a round and the next.
    #[setting(
        label = "Gap",
        group = "Fire",
        when = "aim=off or aim=straight_in",
        range(0, 30),
        span(0, 1),
        unit = "s",
        keyframe
    )]
    pub gap: f32,
    /// Which balls of a round go first, when there is a gap.
    #[setting(label = "Order", group = "Fire", when = "gap and aim=off or gap and aim=straight_in")]
    pub order: FireOrder,
    /// What starts a round.
    #[setting(label = "Fire when", group = "Rounds", simple)]
    pub fire_when: FireWhen,
    /// When the first round fires.
    #[setting(label = "After", group = "Rounds", range(0, 600), span(0, 10), unit = "s")]
    pub start_delay: f32,
    /// The time from one round to the next.
    #[setting(
        label = "Every",
        group = "Rounds",
        when = "fire_when=every",
        range(0.05, 600),
        span(0.05, 20),
        unit = "s",
        keyframe
    )]
    pub every: f32,
    /// The emitters whose waves landing fire a round: this one, or others.
    #[setting(label = "On", group = "Rounds", when = "fire_when=when_a_wave_lands")]
    pub wave_from: Target,
    /// Also fire the first round at the start, before any wave has landed.
    #[setting(label = "Also at start", group = "Rounds", when = "fire_when=when_a_wave_lands")]
    pub wave_first: bool,
    /// How many rounds it fires.
    #[setting(label = "Rounds", group = "Rounds", when = "fire_when!=at_start and !endless", range(1, 1000))]
    pub rounds: u32,
    /// Rounds without end (Most balls still holds).
    #[setting(label = "Endless", group = "Rounds", when = "fire_when!=at_start")]
    pub endless: bool,
    /// Each round has this many times the balls of the one before...
    #[setting(label = "Each round ×", group = "Rounds", when = "fire_when!=at_start", range(0.1, 100))]
    pub grow_times: f32,
    /// ...and this many more.
    #[setting(label = "Each round +", group = "Rounds", when = "fire_when!=at_start", range(-10000, 10000))]
    pub grow_plus: i32,
    /// What happens to earlier rounds' balls when a new round fires.
    #[setting(label = "Earlier rounds", group = "Rounds", when = "fire_when!=at_start")]
    pub earlier: Earlier,
    /// How long an earlier round takes to fade out before it is taken away.
    #[setting(
        label = "Fade time",
        group = "Rounds",
        when = "earlier=fade_out and fire_when!=at_start",
        range(0.05, 30),
        span(0.05, 5),
        unit = "s"
    )]
    pub fade_time: f32,
    /// The most balls of this emitter there may be at once: a round is cut short there.
    #[setting(label = "Most balls", group = "Rounds", range(1, 100000))]
    pub most_balls: u32,
    /// How the balls are aimed.
    #[setting(label = "Aim", group = "Aim")]
    pub aim: Aim,
    /// What the balls are aimed at (a thing id): 0 for the other focus of the shape it sits on.
    #[setting(label = "Target", group = "Aim", when = "aim!=off")]
    pub aim_target: i32,
    /// How long after a round fires its balls reach the target.
    #[setting(
        label = "Arrive after",
        group = "Aim",
        when = "aim!=off",
        range(0.1, 60),
        span(0.1, 10),
        unit = "s",
        keyframe
    )]
    pub arrive_after: f32,
    /// What happens to a ball that can't reach the target in time.
    #[setting(label = "Can't reach", group = "Aim", when = "aim!=off")]
    pub cant_reach: CantReach,
    /// How many times a ball aimed through bounces bounces at least before it gets there (the
    /// fewest that work win).
    #[setting(label = "Bounces at least", group = "Aim", when = "aim=through_bounces", range(0, 20))]
    pub aim_bounces: u32,
    /// What happens to a ball that reaches it.
    #[setting(label = "Catches", group = "Catch", simple)]
    pub catches: Catches,
    /// How far from its middle a ball's centre is caught.
    #[setting(
        label = "Catch size",
        group = "Catch",
        when = "catches!=nothing",
        range(1, 2000),
        span(1, 300),
        unit = "px",
        keyframe
    )]
    pub catch_size: f32,
    /// Which balls it catches.
    #[setting(label = "From", group = "Catch", when = "catches!=nothing")]
    pub catch_from: CatchFrom,
    /// The things whose balls it catches, for Chosen.
    #[setting(label = "Things", group = "Catch", when = "catches!=nothing and catch_from=chosen")]
    pub catch_things: Target,
    /// A caught ball's trail stays behind and runs out into the catch point over its own length,
    /// as if the ball had stopped there.
    #[setting(label = "Leave trails", group = "Catch", when = "catches=remove")]
    pub leave_trails: bool,
    /// Which way a caught ball goes back out.
    #[setting(label = "Which way", group = "Send back out", when = "catches=send_back_out")]
    pub return_way: ReturnWay,
    /// How long a caught ball waits before it goes back out.
    #[setting(
        label = "Delay",
        group = "Send back out",
        when = "catches=send_back_out",
        range(0, 30),
        span(0, 2),
        unit = "s"
    )]
    pub return_delay: f32,
    /// What the emitter shows of itself.
    #[setting(label = "Marker", group = "Marker")]
    pub marker: Marker,
    /// How big the marker is.
    #[setting(label = "Size", group = "Marker", when = "marker!=none", range(1, 200), unit = "px", keyframe)]
    pub marker_size: f32,
    #[setting(label = "Colour", group = "Marker")]
    pub marker_colour: Color,
    /// A soft light round the marker.
    #[setting(label = "Glow", group = "Marker", when = "marker!=none", range(0, 1), keyframe)]
    pub marker_glow: f32,
    /// How bright the flash is when it fires a round.
    #[setting(label = "When it fires", group = "Flash", range(0, 1))]
    pub flash_fire: f32,
    /// How bright the flash is when a wave of balls lands on it.
    #[setting(label = "When a wave lands", group = "Flash", range(0, 1))]
    pub flash_land: f32,
    /// How long a flash takes to fade.
    #[setting(label = "Fade", group = "Flash", range(0.05, 10), span(0.05, 2), unit = "s")]
    pub flash_fade: f32,
    /// Rings that spread out when a wave lands.
    #[setting(label = "Ripple", group = "Ripple")]
    pub ripple: bool,
    #[setting(label = "Rings", group = "Ripple", when = "ripple", range(1, 10))]
    pub ripple_rings: u32,
    /// How far the rings spread.
    #[setting(label = "How far", group = "Ripple", when = "ripple", range(10, 4000), span(10, 1000), unit = "px")]
    pub ripple_far: f32,
    #[setting(label = "Fade", group = "Ripple", when = "ripple", range(0.05, 10), span(0.05, 4), unit = "s")]
    pub ripple_fade: f32,
    /// How much each ball's size differs, at random.
    #[setting(label = "Size spread", group = "Variety", range(0, 1))]
    pub vary_size: f32,
    /// How far round the colour wheel each ball's colour may turn, at random.
    #[setting(label = "Colour spread", group = "Variety", range(0, 1))]
    pub vary_colour: f32,
    /// How far each ball's look may be turned, at random.
    #[setting(label = "Turn spread", group = "Variety", range(0, 180), unit = "°")]
    pub vary_turn: f32,
    /// Picks each ball's variety and the random directions; another number, another pick.
    #[setting(label = "Seed", group = "Variety")]
    pub seed: i32,
}

impl Default for EmitterSettings {
    fn default() -> Self {
        Self {
            emits: true,
            radius: 6.0,
            sits_on: 0,
            sits_focus: SitsFocus::Focus1,
            start_from: StartFrom::Point,
            start_radius: 60.0,
            start_length: 300.0,
            directions: Directions::AllRound,
            aim_angle: -90.0,
            fan_angle: 90.0,
            speed: 600.0,
            vary_speed: 0.0,
            count: 10,
            gap: 0.0,
            order: FireOrder::Around,
            fire_when: FireWhen::AtStart,
            start_delay: 0.0,
            every: 4.0,
            wave_from: Target::Me,
            wave_first: true,
            rounds: 1,
            endless: false,
            grow_times: 1.0,
            grow_plus: 0,
            earlier: Earlier::Keep,
            fade_time: 1.0,
            most_balls: 20000,
            aim: Aim::Off,
            aim_target: 0,
            arrive_after: 2.0,
            cant_reach: CantReach::Skip,
            aim_bounces: 1,
            catches: Catches::Nothing,
            catch_size: 40.0,
            catch_from: CatchFrom::AnyBalls,
            catch_things: Target::Anything,
            leave_trails: false,
            return_way: ReturnWay::SameWayBack,
            return_delay: 0.0,
            marker: Marker::RingWithDot,
            marker_size: 12.0,
            marker_colour: Color::rgb(0xff, 0xc4, 0x6b),
            marker_glow: 0.4,
            flash_fire: 0.5,
            flash_land: 0.9,
            flash_fade: 0.4,
            ripple: true,
            ripple_rings: 3,
            ripple_far: 300.0,
            ripple_fade: 1.2,
            vary_size: 0.0,
            vary_colour: 0.0,
            vary_turn: 0.0,
            seed: 1,
        }
    }
}

impl EmitterSettings {
    /// How many rounds it fires, `None` for endless. At start fires one.
    pub fn round_limit(&self) -> Option<u32> {
        match self.fire_when {
            FireWhen::AtStart => Some(1),
            _ if self.endless => None,
            _ => Some(self.rounds.max(1)),
        }
    }

    /// What the group gradient's Round spread reaches over.
    pub fn round_span(&self) -> RoundSpan {
        self.round_limit().map_or(RoundSpan::Endless, RoundSpan::Finite)
    }

    /// How many steps a fading round takes, at `rate` steps a second; 0 when earlier rounds
    /// don't fade.
    pub fn fade_steps(&self, rate: u32) -> u64 {
        if self.earlier != Earlier::FadeOut || self.fire_when == FireWhen::AtStart {
            return 0;
        }
        ((self.fade_time.max(0.0) as f64 * rate as f64).round() as u64).max(1)
    }

    /// The look-only variety and size, read as a Balls group's (what the shared ball drawing
    /// reads).
    pub fn variety(&self) -> BallsSettings {
        BallsSettings {
            radius: self.radius,
            group: true,
            seed: self.seed,
            vary_size: self.vary_size,
            vary_speed: self.vary_speed,
            vary_colour: self.vary_colour,
            vary_turn: self.vary_turn,
            ..BallsSettings::default()
        }
    }
}

const SLOTS: &[SlotId] = &[SlotId::Look, SlotId::Motion, SlotId::Bounce];

impl ThingKind for EmitterKind {
    fn id(&self) -> &'static str {
        KIND
    }
    fn family(&self) -> &'static str {
        "Bouncers"
    }
    fn name(&self) -> &'static str {
        "Emitter"
    }
    fn icon(&self) -> &'static str {
        "emitter"
    }
    fn help(&self) -> &'static str {
        "Fires balls it designs itself, catches balls that reach it, or both."
    }
    fn settings(&self) -> &'static [SettingDef] {
        EmitterSettings::defs()
    }
    fn slots(&self) -> &'static [SlotId] {
        SLOTS
    }
    fn default_slots(&self) -> &'static [SlotId] {
        SLOTS
    }
    /// Size may vary across the balls as they go; Speed is read once, as each ball is fired, so
    /// it can't follow their speed or hits.
    fn vary(&self, key: &str) -> Option<VaryUse> {
        match key {
            "radius" => Some(VaryUse::Live),
            "speed" => Some(VaryUse::AtLaunch),
            _ => None,
        }
    }
}

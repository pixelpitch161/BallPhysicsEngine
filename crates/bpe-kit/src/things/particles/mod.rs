//! Particles: a thing that makes its own particles (particles spec, part 1): rain over the whole
//! picture, sparks from a point, fire riding a ball, specks thrown off where balls hit. Its own
//! settings are the **Source** (where and when particles are born, and what each starts as); its
//! cards are **Motion** (force layers, `forces.rs`), **Meets things** (balls, shapes and the
//! picture's edges, `meets.rs`) and **Look** (draw layers, `look.rs`).
//!
//! The particles are worked out exactly on the CPU, stepped with the physics at 120 Hz
//! (`sim.rs`, `step.rs`), their state in the world so checkpoints and seeks replay them. In part 1
//! a Particles thing is decoration only: it reads the balls and walls and changes nothing anything
//! else reads, so it is stepped beside the physics ([`bpe_runtime::ThingSim::in_main`]).

pub mod air;
#[cfg(test)]
mod air_tests;
pub mod beats;
pub mod births;
pub mod draw;
pub mod feeds;
pub mod forces;
pub mod liquid;
pub mod look;
pub mod looks;
mod looks_motion;
pub mod marks;
pub mod meets;
pub mod modifiers;
pub mod moments;
pub mod more_forces;
pub mod piles;
pub mod ready;
pub mod rules;
pub mod sim;
pub mod spots;
pub mod state;
pub mod step;
pub mod swarm;
#[cfg(test)]
mod swarm_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_motion;
#[cfg(test)]
mod tests_part4;
#[cfg(test)]
mod tests_part6;
#[cfg(test)]
mod tests_part7;
#[cfg(test)]
mod tests_real_light;
pub mod trail;
pub mod two_way;
pub mod wants;
pub mod web;

use bpe_core::{Choice, SettingDef, Settings, SlotId, Target, ThingKind};
use glam::Vec2;

/// The thing kind's id.
pub const KIND: &str = "particles";

/// The thing kinds a Particles thing inside can ride.
pub const RIDES: [&str; 3] = ["balls", "emitter", "shape"];

/// Where particles are born.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum BornFrom {
    /// Over everything the camera sees, or along one of its edges.
    #[default]
    WholeScreen,
    /// Inside a rectangle, circle or ring round the thing's place.
    AnArea,
    /// At the thing's place.
    APoint,
    /// Along a line through the thing's place.
    ALine,
    /// On what the thing sits in: its balls or its shape.
    RidingItsParent,
    /// In the letters of a word or the filled parts of a picture: each particle's home is its
    /// place in it (particles part 3).
    AWordOrPicture,
    /// On the points of a grid: each particle's home is its point (particles part 3).
    AGrid,
    /// From the particles of the Particles thing it sits in: where they die, land, hit or pop.
    ItsParentsParticles,
}

/// Which of its parent's particles' moments a fed Particles thing is born from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum FedWhen {
    /// Where a particle's life runs out.
    #[default]
    Die,
    /// Where one comes to rest on a shape or the floor.
    Land,
    HitABall,
    HitAShape,
    /// Where one hits a ball or a shape.
    HitAnything,
    /// Where one pops on a ball.
    Pop,
    /// Where one going up stops rising (a rocket at the top of its climb).
    StopRising,
    /// All the time, from every one of them: Rate a particle a second.
    EveryStep,
}

/// What a word or picture source is made of.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum WordFrom {
    #[default]
    Text,
    /// A PNG picture.
    Picture,
    /// A Text thing's letters, in its font and on its path (Text part 4).
    ATextThing,
}

/// How heavy a word's letters are (the IBM Plex Sans weights the editor ships).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum Weight {
    Regular,
    Medium,
    #[default]
    SemiBold,
}

/// Which parts of a picture particles fill.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum Uses {
    /// Where it isn't see-through.
    #[default]
    SolidParts,
    /// Where it is light.
    BrightParts,
    /// Where it is dark.
    DarkParts,
}

/// What a grid covers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum GridCovers {
    /// Everything the camera sees, and Margin round it.
    #[default]
    TheScreen,
    /// Width × Height round the thing's place.
    ARectangle,
}

/// How a grid's points are spaced.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum GridBy {
    /// Spacing pixels apart.
    #[default]
    Spacing,
    /// Columns × Rows points across what it covers.
    ColumnsAndRows,
}

/// How often something happens on the beat.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum BeatEvery {
    /// Every beat of the scene's tempo.
    #[default]
    Beat,
    /// At the start of every bar.
    Bar,
    /// Every few bars.
    Bars,
}

/// Which part of the screen Whole screen births come from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Edge {
    #[default]
    Everywhere,
    Top,
    Bottom,
    Left,
    Right,
}

/// The shape of an area source.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum AreaShape {
    #[default]
    Rectangle,
    Circle,
    Ring,
}

/// What part of its parent a riding source is born on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum RidesOn {
    /// Every ball.
    #[default]
    EachBall,
    /// The middle of the balls, or of the shape.
    TheMiddle,
    /// One ball, by its number.
    BallNumber,
    /// Each ball's rim, or the shape's outline.
    TheEdge,
    /// Inside each ball, or inside the shape.
    Inside,
}

/// When particles are born.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Emit {
    /// A steady stream, Rate a second (a ball, when it rides each ball).
    #[default]
    Steady,
    /// Count at once, Every so often.
    Bursts,
    /// Count each time a ball hits something.
    OnHits,
    /// Count at all times: born at the start, and each one that dies is replaced.
    KeepACrowd,
    /// Count at the start, and no more.
    OnceAtTheStart,
    /// Count on every beat, bar or few bars of the scene's tempo (particles part 3).
    OnTheBeat,
}

/// The way a new particle flies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Heading {
    /// The Angle you set.
    #[default]
    AWay,
    /// Any way.
    AllRound,
    /// Away from what a ball hit (On hits), or away from the middle of what it rides.
    OffTheBounce,
    /// The way the ball it rides is going.
    AlongThePath,
    /// Back the way the ball it rides came from.
    Behind,
    /// Sideways round what they come from: round the ball they ride, the middle of an area or a
    /// word, the hit (particles part 3).
    RoundTheMiddle,
}

/// A Particles thing's own settings: its Source.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ParticlesSettings {
    #[setting(label = "From", simple, group = "Where")]
    pub from: BornFrom,
    #[setting(label = "Edge", when = "from=whole_screen", group = "Where")]
    pub edge: Edge,
    /// How far past the screen's edge particles are born (and live on before they go).
    #[setting(
        label = "Margin",
        when = "from=whole_screen or from=a_grid and grid_covers=the_screen",
        span(0, 400),
        unit = "px",
        group = "Where"
    )]
    pub margin: f32,
    #[setting(label = "Area shape", when = "from=an_area", group = "Where")]
    pub area: AreaShape,
    #[setting(
        label = "Width",
        when = "from=an_area and area=rectangle or from=a_grid and grid_covers=a_rectangle",
        range(0, 1e9),
        span(0, 2000),
        unit = "px",
        keyframe,
        group = "Where"
    )]
    pub width: f32,
    #[setting(
        label = "Height",
        when = "from=an_area and area=rectangle or from=a_grid and grid_covers=a_rectangle",
        range(0, 1e9),
        span(0, 2000),
        unit = "px",
        keyframe,
        group = "Where"
    )]
    pub height: f32,
    #[setting(
        label = "Radius",
        when = "from=an_area and area!=rectangle",
        range(0, 1e9),
        span(0, 1000),
        unit = "px",
        keyframe,
        group = "Where"
    )]
    pub radius: f32,
    #[setting(
        label = "Inner radius",
        when = "from=an_area and area=ring",
        range(0, 1e9),
        span(0, 1000),
        unit = "px",
        keyframe,
        group = "Where"
    )]
    pub inner: f32,
    #[setting(
        label = "Length",
        when = "from=a_line",
        range(0, 1e9),
        span(0, 2000),
        unit = "px",
        keyframe,
        group = "Where"
    )]
    pub length: f32,
    /// The way the line (or the area, word or grid) turns, in degrees.
    #[setting(
        label = "Turn",
        when = "from=a_line|an_area|a_word_or_picture|a_grid",
        span(-180, 180),
        unit = "°",
        keyframe,
        group = "Where"
    )]
    pub line_turn: f32,
    #[setting(label = "Rides on", when = "from=riding_its_parent", group = "Where")]
    pub rides_on: RidesOn,
    #[setting(
        label = "Ball number",
        when = "from=riding_its_parent and rides_on=ball_number",
        range(1, 1e9),
        span(1, 100),
        group = "Where"
    )]
    pub ball_number: u32,
    #[setting(label = "Made of", when = "from=a_word_or_picture", group = "Where")]
    pub word_from: WordFrom,
    #[setting(label = "Text", when = "from=a_word_or_picture and word_from=text", group = "Where")]
    pub text: String,
    /// The Text whose letters the particles fill (its first, when this names several).
    #[setting(label = "Text thing", when = "from=a_word_or_picture and word_from=a_text_thing", group = "Where")]
    pub text_thing: Target,
    /// How tall the capitals are.
    #[setting(
        label = "Letter height",
        when = "from=a_word_or_picture and word_from=text",
        range(1, 1e9),
        span(20, 1000),
        unit = "px",
        group = "Where"
    )]
    pub letter_height: f32,
    #[setting(label = "Weight", when = "from=a_word_or_picture and word_from=text", group = "Where")]
    pub weight: Weight,
    /// A PNG picture: particles fill its solid, bright or dark parts.
    #[setting(
        label = "Picture",
        when = "from=a_word_or_picture and word_from=picture",
        unit = "picture",
        group = "Where"
    )]
    pub picture: String,
    #[setting(
        label = "Picture width",
        when = "from=a_word_or_picture and word_from=picture",
        range(1, 1e9),
        span(20, 2000),
        unit = "px",
        group = "Where"
    )]
    pub picture_width: f32,
    #[setting(label = "Uses", when = "from=a_word_or_picture and word_from=picture", group = "Where")]
    pub uses: Uses,
    #[setting(label = "Covers", when = "from=a_grid", group = "Where")]
    pub grid_covers: GridCovers,
    #[setting(label = "Grid by", when = "from=a_grid", group = "Where")]
    pub grid_by: GridBy,
    #[setting(
        label = "Columns",
        when = "from=a_grid and grid_by=columns_and_rows",
        range(1, 1e6),
        span(1, 200),
        group = "Where"
    )]
    pub columns: u32,
    #[setting(
        label = "Rows",
        when = "from=a_grid and grid_by=columns_and_rows",
        range(1, 1e6),
        span(1, 200),
        group = "Where"
    )]
    pub rows: u32,
    /// How far apart the places are: one particle every Spacing pixels.
    #[setting(
        label = "Spacing",
        when = "from=a_word_or_picture or from=a_grid and grid_by=spacing",
        range(1, 1e9),
        span(2, 100),
        unit = "px",
        group = "Where"
    )]
    pub spacing: f32,
    /// How far each grid point is moved off its place, as a share of a cell.
    #[setting(label = "Jitter", when = "from=a_grid", span(0, 1), group = "Where")]
    pub jitter: f32,
    /// Every other row shifted by half a cell (a honeycomb).
    #[setting(label = "Stagger rows", when = "from=a_grid", group = "Where")]
    pub stagger: bool,
    /// How fast the homes turn round the middle.
    #[setting(
        label = "Homes turn",
        when = "from=a_word_or_picture|a_grid",
        span(-90, 90),
        unit = "°/s",
        keyframe,
        group = "Where"
    )]
    pub homes_turn: f32,

    #[setting(label = "Emit", when = "from!=its_parents_particles", simple, group = "When")]
    pub emit: Emit,
    /// Which of the parent's particles' moments this thing's particles are born at.
    #[setting(label = "When they", when = "from=its_parents_particles", group = "When")]
    pub fed_when: FedWhen,
    /// Particles a second (a ball, when they ride each ball).
    #[setting(
        label = "Rate",
        when = "from!=its_parents_particles and emit=steady or from=its_parents_particles and fed_when=every_step",
        range(0, 1e9),
        span(0, 2000),
        unit = "per s",
        simple,
        keyframe,
        pulse,
        group = "When"
    )]
    pub rate: f32,
    #[setting(
        label = "Every",
        when = "from!=its_parents_particles and emit=bursts",
        range(0.001, 1e9),
        span(0.05, 10),
        unit = "s",
        keyframe,
        group = "When"
    )]
    pub every: f32,
    /// How many each burst, hit, beat or crowd has (a word or grid's crowd fills every place), or
    /// each moment of its parent's particles lets off.
    #[setting(
        label = "Count",
        when = "from!=its_parents_particles and emit=bursts|on_hits|on_the_beat or from!=a_word_or_picture|a_grid|its_parents_particles and emit=keep_a_crowd|once_at_the_start or from=its_parents_particles and fed_when!=every_step",
        range(0, 1e9),
        span(1, 2000),
        keyframe,
        pulse,
        group = "When"
    )]
    pub count: u32,
    #[setting(label = "Every", when = "from!=its_parents_particles and emit=on_the_beat", group = "When")]
    pub beat_every: BeatEvery,
    #[setting(
        label = "Bars",
        when = "from!=its_parents_particles and emit=on_the_beat and beat_every=bars",
        range(1, 1e6),
        span(1, 16),
        unit = "bars",
        group = "When"
    )]
    pub bars: u32,
    /// Starts this many beats in.
    #[setting(
        label = "Offset",
        when = "from!=its_parents_particles and emit=on_the_beat",
        range(0, 1e6),
        span(0, 16),
        unit = "beats",
        group = "When"
    )]
    pub beat_offset: f32,
    /// Whose balls' hits let particles off.
    #[setting(label = "Who hits", when = "from!=its_parents_particles and emit=on_hits", group = "When")]
    pub who_hits: Target,
    #[setting(
        label = "Least speed",
        when = "from!=its_parents_particles and emit=on_hits",
        span(0, 2000),
        unit = "px/s",
        keyframe,
        group = "When"
    )]
    pub least_speed: f32,
    /// Harder hits let off more, faster particles.
    #[setting(label = "Follow impact", when = "from!=its_parents_particles and emit=on_hits", group = "When")]
    pub follow_impact: bool,

    #[setting(
        label = "Life",
        range(0.001, 1e9),
        span(0.05, 20),
        unit = "s",
        simple,
        keyframe,
        group = "Each particle"
    )]
    pub life: f32,
    #[setting(label = "Life spread", span(0, 1), keyframe, group = "Each particle")]
    pub life_spread: f32,
    #[setting(label = "Speed", span(0, 2000), unit = "px/s", simple, keyframe, pulse, group = "Each particle")]
    pub speed: f32,
    #[setting(label = "Speed spread", span(0, 1), keyframe, group = "Each particle")]
    pub speed_spread: f32,
    #[setting(label = "Direction", group = "Each particle")]
    pub heading: Heading,
    /// The way they fly, in degrees: 0 to the right, 90 down, −90 up.
    #[setting(label = "Angle", when = "heading=a_way", span(-180, 180), unit = "°", keyframe, group = "Each particle")]
    pub angle: f32,
    /// How wide they fan out round their way.
    #[setting(
        label = "Spread",
        when = "heading!=all_round",
        span(0, 360),
        unit = "°",
        keyframe,
        group = "Each particle"
    )]
    pub spread: f32,
    /// How much of the speed of the ball they come from they take with them.
    #[setting(label = "Takes the parent's speed", span(0, 1), keyframe, group = "Each particle")]
    pub inherit: f32,
    /// Each one's half size.
    #[setting(
        label = "Size",
        range(0, 1e9),
        span(0.5, 60),
        unit = "px",
        simple,
        keyframe,
        pulse,
        group = "Each particle"
    )]
    pub size: f32,
    #[setting(label = "Size spread", span(0, 1), keyframe, group = "Each particle")]
    pub size_spread: f32,
    #[setting(label = "Turn", span(-180, 180), unit = "°", keyframe, group = "Each particle")]
    pub turn: f32,
    /// How much of a whole turn each one's start turn is spread by.
    #[setting(label = "Turn spread", span(0, 1), keyframe, group = "Each particle")]
    pub turn_spread: f32,
    #[setting(label = "Spin", span(-1440, 1440), unit = "°/s", keyframe, group = "Each particle")]
    pub spin: f32,
    #[setting(label = "Spin spread", span(0, 1), keyframe, group = "Each particle")]
    pub spin_spread: f32,
    /// Half of them spin the other way.
    #[setting(label = "Spin both ways", group = "Each particle")]
    pub spin_both: bool,
    /// Another seed gives other dice: other places, speeds and sizes.
    #[setting(label = "Seed", span(0, 1000), group = "Each particle")]
    pub seed: u32,

    /// When this many are alive, new ones aren't born until some die (at most 1 000 000, or
    /// 4 000 000 in a Huge swarm).
    #[setting(label = "Most particles", range(1, 4000000), span(1, 200000), group = "Limits")]
    pub most: u32,
}

impl Default for ParticlesSettings {
    fn default() -> Self {
        Self {
            from: BornFrom::WholeScreen,
            edge: Edge::Everywhere,
            margin: 40.0,
            area: AreaShape::Rectangle,
            width: 400.0,
            height: 300.0,
            radius: 200.0,
            inner: 100.0,
            length: 400.0,
            line_turn: 0.0,
            rides_on: RidesOn::EachBall,
            ball_number: 1,
            word_from: WordFrom::Text,
            text_thing: Target::Anything,
            text: "BOUNCE".into(),
            letter_height: 240.0,
            weight: Weight::SemiBold,
            picture: String::new(),
            picture_width: 600.0,
            uses: Uses::SolidParts,
            grid_covers: GridCovers::TheScreen,
            grid_by: GridBy::Spacing,
            columns: 24,
            rows: 40,
            spacing: 12.0,
            jitter: 0.0,
            stagger: false,
            homes_turn: 0.0,
            emit: Emit::Steady,
            fed_when: FedWhen::Die,
            rate: 60.0,
            every: 1.0,
            count: 30,
            beat_every: BeatEvery::Beat,
            bars: 2,
            beat_offset: 0.0,
            who_hits: Target::Anything,
            least_speed: 0.0,
            follow_impact: true,
            life: 2.0,
            life_spread: 0.3,
            speed: 100.0,
            speed_spread: 0.3,
            heading: Heading::AWay,
            angle: -90.0,
            spread: 30.0,
            inherit: 0.0,
            size: 3.0,
            size_spread: 0.3,
            turn: 0.0,
            turn_spread: 1.0,
            spin: 0.0,
            spin_spread: 0.5,
            spin_both: true,
            seed: 0,
            most: 20_000,
        }
    }
}

/// The hard cap on Most particles in the exact engine.
pub const MOST_EVER: u32 = 1_000_000;
/// The hard cap on Most particles in a Huge swarm (particles part 8).
pub const MOST_SWARM: u32 = bpe_gfx::swarm::MOST_SWARM;

impl ParticlesSettings {
    /// The way `angle` (degrees) points.
    pub fn way(angle: f32) -> Vec2 {
        Vec2::from_angle(angle.to_radians())
    }
}

pub struct ParticlesKind;

impl ThingKind for ParticlesKind {
    fn id(&self) -> &'static str {
        KIND
    }
    fn name(&self) -> &'static str {
        "Particles"
    }
    fn icon(&self) -> &'static str {
        "particles"
    }
    fn help(&self) -> &'static str {
        "Makes its own particles: rain, sparks, smoke, specks. Inside a group of balls or a shape it can ride on it."
    }
    fn settings(&self) -> &'static [SettingDef] {
        ParticlesSettings::defs()
    }
    /// Its Sound card (particles part 7) is turned on from the Sound tab, like a shape's.
    fn slots(&self) -> &'static [SlotId] {
        &[SlotId::Motion, SlotId::Bounce, SlotId::Look, SlotId::Sound]
    }
    fn default_slots(&self) -> &'static [SlotId] {
        &[SlotId::Motion, SlotId::Bounce, SlotId::Look]
    }
    fn family(&self) -> &'static str {
        "Particles"
    }
    fn slot_label(&self, slot: SlotId) -> &'static str {
        match slot {
            SlotId::Bounce => "Meets things",
            other => other.name(),
        }
    }
    fn can_stick_to_screen(&self) -> bool {
        false
    }
}

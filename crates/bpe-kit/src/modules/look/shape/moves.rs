//! A Text's letter moves (Text part 4): layers in the Reaction section of its `look.shape` card,
//! offered only on a Text ([`MoveKind::fits`]). Each moves, turns, stretches, fades or traces every
//! letter that wears it, worked out at the moment drawn (`things::text::moves`). The drawing alone
//! reads them: a walled Text's letters stay where they are laid out for the balls.

use bpe_core::{Choice, LayerKind, LayerSection, SettingDef, Settings, Target};

use super::CARD;
use crate::things::light::FlickerStyle;

pub const WAVE: &str = "text.move.wave";
pub const BEAT: &str = "text.move.beat";
pub const HIT: &str = "text.move.hit";
pub const FLICKER: &str = "text.move.flicker";
pub const SOUND: &str = "text.move.sound";
pub const SHAKE: &str = "text.move.shake";
pub const SPIN: &str = "text.move.spin";
pub const SCATTER: &str = "text.move.scatter";
pub const DRAW_ON: &str = "text.move.draw";

/// Every move kind, in the add menu's order.
pub const MOVES: [&str; 9] = [WAVE, BEAT, HIT, FLICKER, SOUND, SHAKE, SPIN, SCATTER, DRAW_ON];

/// Wave: the letters ride up and down a wave running along them.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct WaveMove {
    /// How much of the move shows (0 none, 1 all).
    #[setting(label = "Amount", range(0, 1), simple, keyframe, pulse)]
    pub amount: f32,
    /// How far each letter rises and falls.
    #[setting(label = "Height", span(0, 200), unit = "px", simple, keyframe)]
    pub height: f32,
    /// How many letters one wave spans.
    #[setting(label = "Letters a wave", span(1, 40), range(0.1, 10000), keyframe)]
    pub letters: f32,
    /// How many waves a second run along the letters.
    #[setting(label = "Speed", span(-4, 4), unit = "per s", keyframe)]
    pub speed: f32,
    /// How far each letter leans with the slope of the wave.
    #[setting(label = "Tilt", span(0, 45), unit = "°", keyframe)]
    pub tilt: f32,
}

impl Default for WaveMove {
    fn default() -> Self {
        Self { amount: 1.0, height: 30.0, letters: 8.0, speed: 0.5, tilt: 0.0 }
    }
}

/// Jump on the beat: every letter hops on the scene's beats.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct BeatMove {
    /// How much of the move shows (0 none, 1 all).
    #[setting(label = "Amount", range(0, 1), simple, keyframe, pulse)]
    pub amount: f32,
    /// How high each hop goes.
    #[setting(label = "Height", span(0, 200), unit = "px", simple, keyframe)]
    pub height: f32,
    /// How many beats from one hop to the next.
    #[setting(label = "Every", span(0.25, 8), range(0.01, 10000), unit = "beats")]
    pub every: f32,
    /// How long a hop takes.
    #[setting(label = "Takes", span(0.05, 1), range(0.01, 60), unit = "s")]
    pub takes: f32,
    /// How much later each letter hops than the one before.
    #[setting(label = "Each letter later", span(0, 0.2), range(0, 10), unit = "s")]
    pub later: f32,
    /// How much a letter stretches up as it leaves and squashes as it lands.
    #[setting(label = "Squash", range(0, 1), keyframe)]
    pub squash: f32,
}

impl Default for BeatMove {
    fn default() -> Self {
        Self { amount: 1.0, height: 40.0, every: 1.0, takes: 0.3, later: 0.04, squash: 0.3 }
    }
}

/// Which letters a hit makes jump.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum HitLetters {
    /// The letter the ball hit (a Text in the world that balls bounce off); others say the
    /// thing's hits make every letter jump.
    #[default]
    TheHitLetter,
    EveryLetter,
}

/// Jump on hit: letters hop when something is hit.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct HitMove {
    /// How much of the move shows (0 none, 1 all).
    #[setting(label = "Amount", range(0, 1), simple, keyframe, pulse)]
    pub amount: f32,
    /// Whose hits: Me is the Text's own letters (or, riding balls, each copy's ball).
    #[setting(label = "Hits from", simple)]
    pub hits_of: Target,
    /// How high each hop goes.
    #[setting(label = "Height", span(0, 200), unit = "px", simple, keyframe)]
    pub height: f32,
    /// How long a hop takes.
    #[setting(label = "Takes", span(0.05, 1), range(0.01, 60), unit = "s")]
    pub takes: f32,
    /// The letter hit alone, or every letter.
    #[setting(label = "Which letters")]
    pub which: HitLetters,
    /// How much later each letter hops than the one before, when every letter hops.
    #[setting(label = "Each letter later", when = "which=every_letter", span(0, 0.2), range(0, 10), unit = "s")]
    pub later: f32,
}

impl Default for HitMove {
    fn default() -> Self {
        Self {
            amount: 1.0,
            hits_of: Target::Me,
            height: 50.0,
            takes: 0.35,
            which: HitLetters::TheHitLetter,
            later: 0.03,
        }
    }
}

/// Flicker: letters dim and come back like a buzzing neon tube.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct FlickerMove {
    /// How deep the flicker goes (0 none, 1 right out).
    #[setting(label = "Amount", range(0, 1), simple, keyframe, pulse)]
    pub amount: f32,
    /// The way it flickers.
    #[setting(label = "Style", simple)]
    pub style: FlickerStyle,
    /// How fast.
    #[setting(label = "Speed", span(0.1, 30), range(0, 1000), unit = "per s", keyframe)]
    pub speed: f32,
    /// Another number gives another flicker.
    #[setting(label = "Seed", span(0, 100), range(0, 1e9))]
    pub seed: u32,
    /// Every letter flickers together, rather than each its own way.
    #[setting(label = "Together")]
    pub together: bool,
}

impl Default for FlickerMove {
    fn default() -> Self {
        Self { amount: 0.5, style: FlickerStyle::Faulty, speed: 8.0, seed: 0, together: false }
    }
}

/// Follow sound: letters jump and stretch with how loud a sound is.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct SoundMove {
    /// How much of the move shows (0 none, 1 all).
    #[setting(label = "Amount", range(0, 1), simple, keyframe, pulse)]
    pub amount: f32,
    /// How loud: right-click it and pick Follow sound, and each letter reads the sound at its own
    /// moment.
    #[setting(label = "Level", range(0, 1), simple, keyframe, pulse)]
    pub level: f32,
    /// How high a letter jumps at full level.
    #[setting(label = "Jump", span(0, 300), unit = "px", keyframe)]
    pub jump: f32,
    /// How much taller a letter grows at full level.
    #[setting(label = "Stretch", span(0, 2), keyframe)]
    pub stretch: f32,
    /// How much bigger a letter grows at full level.
    #[setting(label = "Grow", span(0, 1), keyframe)]
    pub grow: f32,
    /// How much later each letter hears the sound than the one before.
    #[setting(label = "Each letter later", span(0, 0.3), range(0, 10), unit = "s")]
    pub later: f32,
    /// How much each letter's own share differs from the others'.
    #[setting(label = "Each its own", range(0, 1), keyframe)]
    pub own: f32,
    /// Another number gives each letter another share.
    #[setting(label = "Seed", when = "own", span(0, 100), range(0, 1e9))]
    pub seed: u32,
}

impl Default for SoundMove {
    fn default() -> Self {
        Self { amount: 1.0, level: 0.0, jump: 60.0, stretch: 0.5, grow: 0.1, later: 0.0, own: 0.3, seed: 0 }
    }
}

/// Shake: letters tremble on their own.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ShakeMove {
    /// How much of the move shows (0 none, 1 all).
    #[setting(label = "Amount", range(0, 1), simple, keyframe, pulse)]
    pub amount: f32,
    /// How far each letter strays.
    #[setting(label = "Distance", span(0, 40), unit = "px", simple, keyframe)]
    pub distance: f32,
    /// How far each letter turns.
    #[setting(label = "Turn", span(0, 45), unit = "°", keyframe)]
    pub turn: f32,
    /// How fast it trembles.
    #[setting(label = "Speed", span(0.1, 40), range(0, 1000), unit = "per s", keyframe)]
    pub speed: f32,
    /// Another number gives another shake.
    #[setting(label = "Seed", span(0, 100), range(0, 1e9))]
    pub seed: u32,
}

impl Default for ShakeMove {
    fn default() -> Self {
        Self { amount: 1.0, distance: 6.0, turn: 4.0, speed: 12.0, seed: 0 }
    }
}

/// Spin each letter: every letter turns about its own middle.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct SpinMove {
    /// How much of the move shows (0 none, 1 all).
    #[setting(label = "Amount", range(0, 1), simple, keyframe, pulse)]
    pub amount: f32,
    /// How fast each letter turns, clockwise.
    #[setting(label = "Speed", span(-720, 720), unit = "°/s", simple, keyframe)]
    pub speed: f32,
    /// How much further round each letter is than the one before.
    #[setting(label = "Each letter later", span(0, 90), unit = "°", keyframe)]
    pub later: f32,
    /// 0 turns right round; more rocks each letter back and forth this far, once a turn's time.
    #[setting(label = "Swing", span(0, 90), unit = "°", keyframe)]
    pub swing: f32,
}

impl Default for SpinMove {
    fn default() -> Self {
        Self { amount: 1.0, speed: 90.0, later: 0.0, swing: 0.0 }
    }
}

/// Scatter and come back: the letters fly apart and fly home again.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ScatterMove {
    /// How much of the move shows (0 none, 1 all).
    #[setting(label = "Amount", range(0, 1), simple, keyframe, pulse)]
    pub amount: f32,
    /// When they fly apart.
    #[setting(label = "Starts at", span(0, 30), range(0, 100000), unit = "s")]
    pub start: f32,
    /// How far each letter flies.
    #[setting(label = "Distance", span(0, 1500), unit = "px", simple, keyframe)]
    pub distance: f32,
    /// How long flying out takes.
    #[setting(label = "Out takes", span(0.05, 3), range(0.01, 600), unit = "s")]
    pub out: f32,
    /// How long they stay apart.
    #[setting(label = "Stays out", span(0, 5), range(0, 100000), unit = "s")]
    pub stay: f32,
    /// How long flying back takes.
    #[setting(label = "Back takes", span(0.05, 3), range(0.01, 600), unit = "s")]
    pub back: f32,
    /// How far each letter turns on the way out.
    #[setting(label = "Turn", span(0, 720), unit = "°", keyframe)]
    pub turn: f32,
    /// How much they fade while apart.
    #[setting(label = "Fade", range(0, 1), keyframe)]
    pub fade: f32,
    /// Seconds from one scatter to the next (0: once).
    #[setting(label = "Every", span(0, 20), range(0, 100000), unit = "s")]
    pub every: f32,
    /// Another number sends the letters other ways.
    #[setting(label = "Seed", span(0, 100), range(0, 1e9))]
    pub seed: u32,
}

impl Default for ScatterMove {
    fn default() -> Self {
        Self {
            amount: 1.0,
            start: 1.0,
            distance: 400.0,
            out: 0.6,
            stay: 0.4,
            back: 0.8,
            turn: 180.0,
            fade: 0.0,
            every: 0.0,
            seed: 0,
        }
    }
}

/// The order Draw on traces the letters in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum DrawOrder {
    /// One pen, letter after letter, at an even speed.
    #[default]
    OneAfterAnother,
    /// Every letter at once.
    AllTogether,
}

/// What a letter shows once it is traced.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum DrawThen {
    /// The traced line stays.
    #[default]
    LinesOnly,
    /// The line stays and the letter fills in under it.
    LinesAndFill,
    /// The letter fills in as the line fades.
    FillOnly,
}

/// Draw on: a pen traces each letter's outline in, then the lines stay or the letter fills.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct DrawOnMove {
    /// How much of the move shows: 1 traces from nothing, less starts part traced.
    #[setting(label = "Amount", range(0, 1), keyframe, pulse)]
    pub amount: f32,
    /// When the pen sets off.
    #[setting(label = "Starts at", span(0, 30), range(0, 100000), unit = "s", simple)]
    pub start: f32,
    /// How long tracing every letter takes.
    #[setting(label = "Takes", span(0.1, 20), range(0.01, 100000), unit = "s", simple)]
    pub takes: f32,
    /// One pen letter after letter, or every letter at once.
    #[setting(label = "Order")]
    pub order: DrawOrder,
    /// How wide the pen's line is.
    #[setting(label = "Pen width", span(1, 40), range(0.1, 1000), unit = "px", keyframe)]
    pub width: f32,
    /// What a traced letter shows.
    #[setting(label = "Then")]
    pub then: DrawThen,
    /// How long filling in takes.
    #[setting(label = "Fill takes", when = "then!=lines_only", span(0.05, 3), range(0, 600), unit = "s")]
    pub fill: f32,
}

impl Default for DrawOnMove {
    fn default() -> Self {
        Self {
            amount: 1.0,
            start: 0.0,
            takes: 3.0,
            order: DrawOrder::OneAfterAnother,
            width: 6.0,
            then: DrawThen::LinesOnly,
            fill: 0.5,
        }
    }
}

/// A letter move's kind (and an experimental effect's, Text part 5: `effects::kinds`).
pub struct MoveKind {
    pub id: &'static str,
    pub name: &'static str,
    pub help: &'static str,
    pub settings: fn() -> &'static [SettingDef],
    /// A shape can wear it too, as one letter (W12).
    pub shapes: bool,
}

impl LayerKind for MoveKind {
    fn id(&self) -> &'static str {
        self.id
    }
    fn name(&self) -> &'static str {
        self.name
    }
    fn help(&self) -> &'static str {
        self.help
    }
    fn settings(&self) -> &'static [SettingDef] {
        (self.settings)()
    }
    fn section(&self) -> LayerSection {
        LayerSection::Reaction
    }
    fn repeats(&self) -> bool {
        true
    }
    fn card(&self) -> &'static str {
        CARD
    }
    fn takes_subs(&self) -> bool {
        false
    }
    fn per_ball(&self) -> bool {
        false
    }
    /// A Text's letters move; a shape wears the moves that make sense for one piece.
    fn fits(&self, thing_kind: &str) -> bool {
        thing_kind == crate::things::text::KIND || (self.shapes && thing_kind == crate::things::shape::KIND)
    }
}

/// Every move kind, in the add menu's order.
pub fn kinds() -> Vec<MoveKind> {
    let k = |id, name, help, settings| MoveKind { id, name, help, settings, shapes: true };
    vec![
        k(WAVE, "Wave", "The letters (or the shape) ride up and down a wave running along them.", WaveMove::defs),
        k(
            BEAT,
            "Jump on the beat",
            "Every letter (or the shape) hops on the scene's beats, one after another.",
            BeatMove::defs,
        ),
        k(
            HIT,
            "Jump on hit",
            "Letters (or the shape) hop when a ball hits them, or when a thing is hit.",
            HitMove::defs,
        ),
        k(FLICKER, "Flicker", "Letters (or the shape) dim and come back like a buzzing neon tube.", FlickerMove::defs),
        k(
            SOUND,
            "Follow sound",
            "Letters (or the shape) jump and stretch with how loud a sound is: right-click Level to pick what it \
             follows.",
            SoundMove::defs,
        ),
        k(SHAKE, "Shake", "Each letter (or the shape) trembles on its own.", ShakeMove::defs),
        k(SPIN, "Spin each letter", "Every letter (or the shape) turns about its own middle.", SpinMove::defs),
        k(
            SCATTER,
            "Scatter and come back",
            "The letters (or the shape) fly apart, then fly home again.",
            ScatterMove::defs,
        ),
        MoveKind {
            id: DRAW_ON,
            name: "Draw on",
            help: "A pen traces each letter's outline in; then the lines stay or the letter fills.",
            settings: DrawOnMove::defs,
            shapes: false,
        },
    ]
}

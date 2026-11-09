//! The Text thing (Text part 1, spec `docs/superpowers/specs/2026-11-07-text-thing-design.md`):
//! words in any of the built-in fonts or a loaded one, laid out straight, round a circle, on a wave
//! or along a shape's edge, each letter drawn as a shape wearing the Text's `look.shape` card. A
//! new Text sits on the screen. Set in the world, its letters are a wall balls bounce off or stay
//! inside, each letter a part rules, health and sound tell apart (part 3).

pub mod draw;
pub mod fx;
#[cfg(test)]
mod fx_tests;
pub mod looks;
pub mod moves;
pub mod shape;
pub mod show;
pub mod sim;
pub mod walls;

pub use draw::{TextAt, TextDraw, TextNow, text_copies, text_now};
pub use sim::{TextSim, TextState, letter_char};

use bpe_core::{Choice, SettingDef, Settings, SlotId, Target, ThingKind};

pub use crate::riding::RidesOn;
use crate::text::{Align, Case, LayoutSpec};

/// The kind id.
pub const KIND: &str = "text";

/// The path a Text's letters follow.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum TextPath {
    /// Lines as laid out.
    #[default]
    Straight,
    /// Round a circle about the text's middle.
    RoundACircle,
    /// Up and down a wave.
    AlongAWave,
    /// Along another shape's edge.
    AlongAShape,
}

/// How the letters show (Text part 2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ShowBy {
    /// The whole text at once.
    #[default]
    AllAtOnce,
    LetterByLetter,
    WordByWord,
    LineByLine,
}

/// How letters come and go.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum TextMove {
    /// Straight in or out.
    #[default]
    None,
    /// Grow from nothing with a little overshoot.
    Pop,
    Fade,
    /// Up from below, fading in.
    Slide,
    /// Down from above with a bounce.
    Drop,
    /// At once, like a typewriter.
    Typewriter,
}

/// What times the letters showing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum TimedBy {
    #[default]
    Seconds,
    /// One on each beat (or every few) of the scene's tempo.
    Beats,
    /// One on each hit of a thing.
    Hits,
}

/// What balls do at the letters (Text part 3). Only a Text in the world that rides nothing is a
/// wall.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum TextBalls {
    /// Balls go through (the letters are still a zone rules see, and lasers meet them).
    #[default]
    PassThrough,
    /// Balls bounce off the solid letters.
    BounceOff,
    /// Each letter's outline is a band of Thickness: balls inside a letter stay inside it.
    BounceInside,
}

/// When the letters are drawn from distance tiles (quick for crowds of small letters).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum FastLetters {
    /// For a Text riding every ball or showing standings.
    #[default]
    Automatic,
    Always,
    Never,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct TextSettings {
    /// What it says: any number of lines. Values in braces ({hits}, {time} …) are read as the video plays.
    #[setting(label = "Words", group = "Words", unit = "lines", simple, keyframe, hold)]
    pub words: String,
    /// The font it is set in.
    #[setting(label = "Font", group = "Words", unit = "font", simple)]
    pub font: String,
    /// How heavy the letters are, for fonts that come in weights.
    #[setting(label = "Weight", group = "Words", range(100, 1000), keyframe)]
    pub weight: f32,
    /// How far the letters lean.
    #[setting(label = "Slant", group = "Words", range(-30, 30), unit = "°", keyframe)]
    pub slant: f32,
    /// The height of a capital letter.
    #[setting(label = "Size", group = "Words", span(10, 1000), range(1, 20000), unit = "px", simple, keyframe)]
    pub size: f32,
    /// Room added after every letter.
    #[setting(label = "Letter spacing", group = "Words", span(-50, 200), unit = "px", keyframe)]
    pub letter_spacing: f32,
    /// How far apart the lines are, in the font's own line heights.
    #[setting(label = "Line spacing", group = "Words", range(0.3, 5), keyframe)]
    pub line_spacing: f32,
    /// Where each line sits across the block.
    #[setting(label = "Align", group = "Words")]
    pub align: Align,
    /// Lines longer than this break between words (0: no wrapping).
    #[setting(label = "Wrap width", group = "Words", span(0, 2000), range(0, 20000), unit = "px", keyframe)]
    pub wrap: f32,
    /// As typed, or every letter changed to capitals or lower case.
    #[setting(label = "Case", group = "Words")]
    pub case: Case,
    /// How fast the whole text turns, clockwise.
    #[setting(label = "Spin", group = "Words", range(-360, 360), unit = "°/s", keyframe)]
    pub spin: f32,
    /// Draw the letters from distance tiles: much quicker for crowds of small letters, but a glow or
    /// outline can reach only a sixth of a letter's size past it.
    #[setting(label = "Fast letters", group = "Words")]
    pub fast: FastLetters,
    /// What the letters follow.
    #[setting(label = "Path", group = "Path", simple)]
    pub path: TextPath,
    /// The circle's radius.
    #[setting(
        label = "Radius",
        group = "Path",
        when = "path=round_a_circle",
        span(20, 2000),
        range(1, 20000),
        unit = "px",
        keyframe
    )]
    pub radius: f32,
    /// Where round the circle the middle of the text sits (0 at the top, clockwise).
    #[setting(label = "Starts at", group = "Path", when = "path=round_a_circle", span(-180, 180), unit = "°", keyframe)]
    pub start_angle: f32,
    /// Read from inside the circle: the tops of the letters point to its middle.
    #[setting(label = "Reads from inside", group = "Path", when = "path=round_a_circle")]
    pub inside: bool,
    /// How high the wave goes.
    #[setting(label = "Height", group = "Path", when = "path=along_a_wave", span(0, 500), unit = "px", keyframe)]
    pub wave_height: f32,
    /// How long one wave is.
    #[setting(
        label = "Wavelength",
        group = "Path",
        when = "path=along_a_wave",
        span(20, 3000),
        range(1, 20000),
        unit = "px",
        keyframe
    )]
    pub wavelength: f32,
    /// How far along the wave is moved, in waves.
    #[setting(label = "Wave shift", group = "Path", when = "path=along_a_wave", span(0, 1), keyframe)]
    pub wave_shift: f32,
    /// How many waves a second move past.
    #[setting(label = "Wave speed", group = "Path", when = "path=along_a_wave", span(-5, 5), unit = "per s", keyframe)]
    pub wave_speed: f32,
    /// The letters turn with the wave's slope.
    #[setting(label = "Letters follow the slope", group = "Path", when = "path=along_a_wave")]
    pub follow_slope: bool,
    /// The shape whose edge the letters run along (the first shape it names).
    #[setting(label = "Shape", group = "Path", when = "path=along_a_shape")]
    pub along: Target,
    /// Where round the shape's edge the middle of the text sits, from its first corner.
    #[setting(label = "Starts at", group = "Path", when = "path=along_a_shape", range(0, 1), keyframe)]
    pub along_start: f32,
    /// How far off the edge the letters sit (negative: inside).
    #[setting(label = "Gap", group = "Path", when = "path=along_a_shape", span(-200, 200), unit = "px", keyframe)]
    pub gap: f32,
    /// Whether the text shows whole, or a letter, word or line at a time.
    #[setting(label = "Show", group = "Show", simple)]
    pub show: ShowBy,
    /// How each letter, word or line comes in.
    #[setting(label = "Appear", group = "Show")]
    pub appear: TextMove,
    /// How each goes again (after Stays for).
    #[setting(label = "Disappear", group = "Show")]
    pub disappear: TextMove,
    /// How long coming in or going takes.
    #[setting(
        label = "Takes",
        group = "Show",
        when = "appear!=none|typewriter or disappear!=none|typewriter",
        span(0.05, 2),
        range(0.01, 60),
        unit = "s"
    )]
    pub move_time: f32,
    /// What sets the moment each one shows.
    #[setting(label = "Timed by", group = "Show")]
    pub timed_by: TimedBy,
    /// When the first shows.
    #[setting(label = "Starts at", group = "Show", span(0, 30), range(0, 100000), unit = "s")]
    pub show_start: f32,
    /// Seconds from one to the next.
    #[setting(
        label = "Each takes",
        group = "Show",
        when = "timed_by=seconds and show!=all_at_once",
        span(0.02, 2),
        range(0, 10000),
        unit = "s"
    )]
    pub each: f32,
    /// Beats from one to the next.
    #[setting(
        label = "Every",
        group = "Show",
        when = "timed_by=beats and show!=all_at_once",
        span(0.25, 16),
        range(0.01, 10000),
        unit = "beats"
    )]
    pub every: f32,
    /// The thing whose hits show the next one (a hit with it on either side).
    #[setting(label = "Hits from", group = "Show", when = "timed_by=hits")]
    pub hits_of: Target,
    /// How long each stays before it goes (0: it stays).
    #[setting(label = "Stays for", group = "Show", span(0, 10), range(0, 100000), unit = "s")]
    pub stays: f32,
    /// How long a standings list takes to move its lines to their new places.
    #[setting(label = "Re-sort time", group = "Values", range(0.1, 1), unit = "s")]
    pub resort: f32,
    /// Inside a group of balls, an emitter or a shape: what it rides on. Its Position is then how far
    /// from it.
    #[setting(label = "Rides on", group = "Riding")]
    pub rides_on: RidesOn,
    /// The ball it rides, by its number in the group.
    #[setting(label = "Ball number", group = "Riding", when = "rides_on=ball_number", range(1, 1e9), span(1, 100))]
    pub ball_number: u32,
    /// Where round the edge it sits (each ball's rim, the shape's outline), from 0 (the top) to 1
    /// (all the way round, clockwise).
    #[setting(label = "Round the edge", group = "Riding", when = "rides_on=the_edge", span(0, 1), keyframe)]
    pub edge_at: f32,
    /// What balls do at the letters. Only for a Text in the world (Stuck to the screen off) that rides nothing.
    #[setting(label = "Balls", group = "Balls", simple)]
    pub balls: TextBalls,
    /// How thick each letter's outline is: what the balls inside bounce off.
    #[setting(
        label = "Thickness",
        group = "Balls",
        when = "balls=bounce_inside",
        span(1, 60),
        range(0.5, 2000),
        unit = "px",
        keyframe
    )]
    pub thickness: f32,
}

impl Default for TextSettings {
    fn default() -> Self {
        Self {
            words: "TEXT".to_string(),
            font: crate::text::DEFAULT_FONT.to_string(),
            weight: 400.0,
            slant: 0.0,
            size: 160.0,
            letter_spacing: 0.0,
            line_spacing: 1.0,
            align: Align::Centre,
            wrap: 0.0,
            case: Case::AsTyped,
            spin: 0.0,
            path: TextPath::Straight,
            radius: 300.0,
            start_angle: 0.0,
            inside: false,
            wave_height: 60.0,
            wavelength: 600.0,
            wave_shift: 0.0,
            wave_speed: 0.0,
            follow_slope: true,
            along: Target::Anything,
            along_start: 0.0,
            gap: 20.0,
            fast: FastLetters::Automatic,
            show: ShowBy::AllAtOnce,
            appear: TextMove::None,
            disappear: TextMove::None,
            move_time: 0.35,
            timed_by: TimedBy::Seconds,
            show_start: 0.0,
            each: 0.15,
            every: 1.0,
            hits_of: Target::Anything,
            stays: 0.0,
            resort: 0.4,
            rides_on: RidesOn::EachBall,
            edge_at: 0.0,
            ball_number: 1,
            balls: TextBalls::PassThrough,
            thickness: 10.0,
        }
    }
}

impl TextSettings {
    /// What the layout reads.
    pub fn layout_spec(&self) -> LayoutSpec<'_> {
        self.layout_spec_of(&self.words)
    }

    /// What the layout reads, with `words` in place of its own (its values read, a rule's words).
    pub fn layout_spec_of<'a>(&'a self, words: &'a str) -> LayoutSpec<'a> {
        LayoutSpec {
            words,
            size: self.size,
            weight: self.weight,
            slant: self.slant,
            letter_spacing: self.letter_spacing,
            line_spacing: self.line_spacing,
            align: self.align,
            wrap: self.wrap,
            case: self.case,
        }
    }
}

pub struct TextKind;

const SLOTS: &[SlotId] = &[SlotId::Look, SlotId::Bounce, SlotId::Sound, SlotId::Health];
const DEFAULT_SLOTS: &[SlotId] = &[SlotId::Look];

impl ThingKind for TextKind {
    fn id(&self) -> &'static str {
        KIND
    }
    fn family(&self) -> &'static str {
        "Text"
    }
    fn offers(&self, scene: &bpe_core::Scene, thing: bpe_core::ThingId, key: &str, choice: &str) -> bool {
        key != "rides_on" || crate::riding::offers(scene, thing, choice)
    }
    fn name(&self) -> &'static str {
        "Text"
    }
    fn icon(&self) -> &'static str {
        "text"
    }
    fn help(&self) -> &'static str {
        "Words on the screen or in the world, in any font, with the Look layers shapes have."
    }
    fn settings(&self) -> &'static [SettingDef] {
        TextSettings::defs()
    }
    fn slots(&self) -> &'static [SlotId] {
        SLOTS
    }
    fn default_slots(&self) -> &'static [SlotId] {
        DEFAULT_SLOTS
    }
    fn sticks_to_screen(&self) -> bool {
        true
    }
    /// Its layers and letter moves can be worn by only some letters.
    fn layers_by_number(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Scene, ThingId, resolve};

    #[test]
    fn the_settings_are_in_their_groups_and_defaults_match_the_spec() {
        let words = [
            "words",
            "font",
            "weight",
            "slant",
            "size",
            "letter_spacing",
            "line_spacing",
            "align",
            "wrap",
            "case",
            "spin",
            "fast",
        ];
        let path = [
            "path",
            "radius",
            "start_angle",
            "inside",
            "wave_height",
            "wavelength",
            "wave_shift",
            "wave_speed",
            "follow_slope",
            "along",
            "along_start",
            "gap",
        ];
        let show =
            ["show", "appear", "disappear", "move_time", "timed_by", "show_start", "each", "every", "hits_of", "stays"];
        let defs = TextSettings::defs();
        let keys: Vec<&str> = defs.iter().map(|d| d.key).collect();
        let mut want_keys: Vec<&str> = words[..11].to_vec();
        want_keys.push("fast");
        want_keys.extend(path);
        want_keys.extend(show);
        want_keys.extend(["resort", "rides_on", "ball_number", "edge_at", "balls", "thickness"]);
        assert_eq!(keys, want_keys);
        for d in defs {
            let want = if words.contains(&d.key) {
                "Words"
            } else if path.contains(&d.key) {
                "Path"
            } else if show.contains(&d.key) {
                "Show"
            } else if d.key == "resort" {
                "Values"
            } else if matches!(d.key, "balls" | "thickness") {
                "Balls"
            } else {
                "Riding"
            };
            assert_eq!(d.group, want, "{}", d.key);
        }
        let words_def = defs.iter().find(|d| d.key == "words").unwrap();
        assert!(words_def.flags.keyframe && words_def.flags.hold, "Words keys hold");
        let d = TextSettings::default();
        assert_eq!((d.words.as_str(), d.font.as_str(), d.size, d.weight), ("TEXT", "Anton", 160.0, 400.0));
        assert_eq!((d.align, d.case, d.path), (Align::Centre, Case::AsTyped, TextPath::Straight));
        assert_eq!((d.radius, d.wave_height, d.wavelength, d.gap, d.follow_slope), (300.0, 60.0, 600.0, 20.0, true));
        let p = TextSettings::from_params(&bpe_core::Params::default());
        assert_eq!(p, d, "the defaults are what an empty thing reads");
        let unit = |k: &str| defs.iter().find(|d| d.key == k).unwrap().unit;
        assert_eq!((unit("words"), unit("font")), ("lines", "font"));
    }

    #[test]
    fn a_new_text_starts_with_a_shape_look_card_and_on_the_screen() {
        let reg = crate::registry();
        let mut scene = Scene::empty(&reg);
        let id = crate::add_thing(&mut scene, &reg, KIND, ThingId::ROOT).unwrap();
        let t = &scene.things[&id];
        assert_eq!(t.slots.len(), 1);
        assert_eq!(t.slots[&SlotId::Look].kind, crate::modules::look::shape::CARD);
        let r = resolve(&scene, &reg);
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        assert!(r.things[&id].screen);
    }
}

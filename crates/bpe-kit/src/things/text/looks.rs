//! The starting looks a Text can be added with (Text part 1: the spec's #1, #12, #13, #14, #18,
//! #24; part 2: the live ones, Hit counter, Countdown timer, Lyrics on the beat, Labels on balls
//! and Race leaderboard; part 3: the real-letter ones, Balls inside a word, Bounce off letters, Pour in and fill,
//! Letters break apart and Letters that play notes, each with its balls beside it; part 4: Word made of balls, Particle
//! word, Laser writing and Text follows sound, and Neon sign's flicker; part 5: the experimental
//! Glitch, Melting text, Kaleidoscope words and Echo stack). A look only fills settings when
//! it is picked from the Add thing menu; nothing here makes a scene of its own.

use bpe_core::{Color, EditError, Gradient, Registry, Scene, Settings, SlotId, ThingId, Value, layered_key};
use glam::Vec2;

use super::{KIND, ShowBy, TextBalls, TextMove, TextPath, TextSettings, TimedBy};
use crate::modules::look::shape as look;
use crate::text::{Align, PathAt, arrange, fonts, layout};
use crate::things::balls::{BallsSettings, GroupShape, Launch, Release};

/// A Text's starting look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    Empty,
    HookTitle,
    SpinningRing,
    NeonSign,
    GlassLetters,
    ShadowLettering,
    FlowingGradient,
    HitCounter,
    CountdownTimer,
    LyricsOnTheBeat,
    LabelsOnBalls,
    RaceLeaderboard,
    BallsInsideAWord,
    BounceOffLetters,
    PourInAndFill,
    LettersBreakApart,
    LettersThatPlayNotes,
    WordMadeOfBalls,
    ParticleWord,
    LaserWriting,
    TextFollowsSound,
    Glitch,
    MeltingText,
    KaleidoscopeWords,
    EchoStack,
}

/// The looks the Add thing menu lists under Looks, in order.
pub const LOOKS: [Look; 24] = [
    Look::HookTitle,
    Look::SpinningRing,
    Look::NeonSign,
    Look::GlassLetters,
    Look::ShadowLettering,
    Look::FlowingGradient,
    Look::HitCounter,
    Look::CountdownTimer,
    Look::LyricsOnTheBeat,
    Look::LabelsOnBalls,
    Look::RaceLeaderboard,
    Look::BallsInsideAWord,
    Look::BounceOffLetters,
    Look::PourInAndFill,
    Look::LettersBreakApart,
    Look::LettersThatPlayNotes,
    Look::WordMadeOfBalls,
    Look::ParticleWord,
    Look::LaserWriting,
    Look::TextFollowsSound,
    Look::Glitch,
    Look::MeltingText,
    Look::KaleidoscopeWords,
    Look::EchoStack,
];

impl Look {
    pub fn name(self) -> &'static str {
        match self {
            Look::Empty => "Empty text",
            Look::HookTitle => "Hook title",
            Look::SpinningRing => "Spinning word ring",
            Look::NeonSign => "Neon sign",
            Look::GlassLetters => "Glass letters",
            Look::ShadowLettering => "Shadow lettering",
            Look::FlowingGradient => "Flowing gradient",
            Look::HitCounter => "Hit counter",
            Look::CountdownTimer => "Countdown timer",
            Look::LyricsOnTheBeat => "Lyrics on the beat",
            Look::LabelsOnBalls => "Labels on balls",
            Look::RaceLeaderboard => "Race leaderboard",
            Look::BallsInsideAWord => "Balls inside a word",
            Look::BounceOffLetters => "Bounce off letters",
            Look::PourInAndFill => "Pour in and fill",
            Look::LettersBreakApart => "Letters break apart",
            Look::LettersThatPlayNotes => "Letters that play notes",
            Look::WordMadeOfBalls => "Word made of balls",
            Look::ParticleWord => "Particle word",
            Look::LaserWriting => "Laser writing",
            Look::TextFollowsSound => "Text follows sound",
            Look::Glitch => "Glitch",
            Look::MeltingText => "Melting text",
            Look::KaleidoscopeWords => "Kaleidoscope words",
            Look::EchoStack => "Echo stack",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            Look::Empty => "Plain white words on the screen, ready to type into.",
            Look::HookTitle => "A bold white title with a black outline and a drop shadow, at the top of the screen.",
            Look::SpinningRing => "Words round a circle that slowly turns, in the world.",
            Look::NeonSign => "Glowing pink tube letters that light up what is around them.",
            Look::GlassLetters => "Clear glass letters that bend what is behind them.",
            Look::ShadowLettering => "Warm letters lit by a lamp, throwing long shadows.",
            Look::FlowingGradient => "Rainbow colours flowing through the letters.",
            Look::HitCounter => "A big number counting every hit, at the top of the screen.",
            Look::CountdownTimer => "A clock counting down from 30 seconds.",
            Look::LyricsOnTheBeat => "Lines that pop in one at a time on the beat, then fade.",
            Look::LabelsOnBalls => "Each ball's number riding on it.",
            Look::RaceLeaderboard => "The five balls furthest down, their order changing as they race.",
            Look::BallsInsideAWord => "Hollow letters with a ball bouncing round inside each one.",
            Look::BounceOffLetters => "Solid letters that a row of balls falls onto and bounces off.",
            Look::PourInAndFill => "Hollow letters slowly filling up with small balls.",
            Look::LettersBreakApart => "Each letter takes three hits, then shatters into pieces.",
            Look::LettersThatPlayNotes => "Balls hit the letters of FACADE, and each letter plays its own note.",
            Look::WordMadeOfBalls => {
                "The word HI built of floating balls; a big ball drops on it and knocks them loose."
            }
            Look::ParticleWord => "Specks holding the word GONE; gusts of wind blow it away and it forms again.",
            Look::LaserWriting => "A red laser writes LASER, its glowing pen tracing each letter in turn.",
            Look::TextFollowsSound => "BASS letters that jump and stretch with how loud Bus 1 is, one after another.",
            Look::Glitch => {
                "Colour-split letters torn into bands now and then, glitching harder every time a ball hits."
            }
            Look::MeltingText => "Letters that slowly drip and stretch down the screen, then start again.",
            Look::KaleidoscopeWords => "A spinning word mirrored eight ways round the middle of the screen.",
            Look::EchoStack => "Two dozen coloured copies stacked toward a point that wanders round.",
        }
    }

    /// Whether the look needs the scene lit.
    fn lit(self) -> bool {
        matches!(self, Look::NeonSign | Look::GlassLetters | Look::ShadowLettering | Look::LaserWriting)
    }

    /// Whether the look adds a group of balls beside its Text (the real-letter looks).
    pub fn adds_balls(self) -> bool {
        matches!(
            self,
            Look::BallsInsideAWord
                | Look::BounceOffLetters
                | Look::PourInAndFill
                | Look::LettersBreakApart
                | Look::LettersThatPlayNotes
        )
    }
}

/// The balls a real-letter look adds beside its Text: their settings and where the thing sits.
struct BallsBeside {
    settings: BallsSettings,
    position: Vec2,
    /// Whether they fall under the scene's gravity (a new ball floats).
    falls: bool,
}

/// Text settings `s` laid out on a straight line with the scene's fonts: each letter's inner
/// point (where a ball fits best inside its stroke, and how far that is from the edge) and its
/// middle, in the Text's own frame.
fn letters_of(scene: &Scene, s: &TextSettings) -> (Vec<(Vec2, f32)>, Vec<Vec2>) {
    let found = fonts::find(&scene.fonts, &s.font, s.weight);
    let laid = layout(found.font(), &s.layout_spec());
    let placed = arrange(&laid, &PathAt::Straight);
    (super::walls::inner_points(&laid, &placed), super::walls::middles(&laid, &placed))
}

/// The widest a real-letter look's words are laid out: inside the new scene's ring, with room.
const FIT_WIDTH: f32 = 900.0;

/// `s` with its Size made smaller, when need be, so its words are at most [`FIT_WIDTH`] wide.
fn fitted(scene: &Scene, mut s: TextSettings) -> TextSettings {
    let found = fonts::find(&scene.fonts, &s.font, s.weight);
    let laid = layout(found.font(), &s.layout_spec());
    let width = laid.max.x - laid.min.x;
    if width.is_finite() && width > FIT_WIDTH {
        s.size = (s.size * FIT_WIDTH / width).floor();
    }
    s
}

/// A group of balls starting at `places` (from the group's middle at 0), each `radius`.
fn custom_balls(places: Vec<[f32; 2]>, radius: f32) -> BallsSettings {
    BallsSettings {
        radius,
        group: true,
        shape: GroupShape::Custom,
        count: places.len() as u32,
        custom: places,
        ..BallsSettings::default()
    }
}

/// A Text with the `look` filled in, under `parent`; a lit look gives the scene its Lighting card
/// when it has none, and Shadow lettering and Glass letters add a lamp beside it.
pub fn add_text(scene: &mut Scene, reg: &Registry, look: Look, parent: ThingId) -> Result<ThingId, EditError> {
    add_text_saying(scene, reg, look, parent, None)
}

/// [`add_text`] with the look's own words replaced by `words`, before its letters are laid out
/// (so a real-letter look's size fits them and its balls sit in their letters).
pub fn add_text_saying(
    scene: &mut Scene,
    reg: &Registry,
    look: Look,
    parent: ThingId,
    words: Option<&str>,
) -> Result<ThingId, EditError> {
    let say = |own: &str| -> String { words.unwrap_or(own).into() };
    let mut s = TextSettings::default();
    let mut screen = true;
    let mut position = Vec2::ZERO;
    let solid = |c: Color| Value::Gradient(Gradient::solid(c));
    // Layers to add, each with its settings and its sub-layers'.
    type Sub = (&'static str, Vec<(&'static str, Value)>);
    type Layer = (&'static str, Vec<(&'static str, Value)>, Vec<Sub>);
    let mut layers: Vec<Layer> = Vec::new();
    let mut card: Vec<(&'static str, Value)> = Vec::new();
    let mut parent = parent;
    let mut balls: Option<BallsBeside> = None;
    let mut health: Vec<(&'static str, Value)> = Vec::new();
    let mut sound: Vec<(&'static str, Value)> = Vec::new();
    // Letter moves and experimental effects to add, each with its settings.
    let mut moves: Vec<(&'static str, Vec<(&'static str, Value)>)> = Vec::new();
    // Bright letters that flash white when hit.
    let flashing = |c: Color| -> Layer {
        let flash = vec![("colour", Value::Color(Color::WHITE))];
        (look::PAINT, vec![("colours", Value::Gradient(Gradient::solid(c)))], vec![(look::SUB_FLASH, flash)])
    };
    // A black outline round white letters, readable over anything.
    let outlined = |width: f64| -> Layer {
        (
            look::OUTLINE,
            vec![
                ("width", Value::Number(width)),
                ("sits", Value::Choice("outside".into())),
                ("colour_from", Value::Choice("own".into())),
                ("colour", solid(Color::rgb(0, 0, 0))),
            ],
            vec![],
        )
    };
    match look {
        Look::Empty => {}
        Look::HookTitle => {
            s.words = say("WAIT FOR\nTHE LAST\nONE…");
            s.size = 120.0;
            s.line_spacing = 0.95;
            // Each word pops in after the one before.
            s.show = ShowBy::WordByWord;
            s.appear = TextMove::Pop;
            s.each = 0.12;
            position = Vec2::new(0.0, -620.0);
            let shadow = vec![
                ("angle", Value::Number(90.0)),
                ("distance", Value::Number(8.0)),
                ("softness", Value::Number(6.0)),
                ("colour", Value::Color(Color::rgb(0, 0, 0))),
                ("opacity", Value::Number(0.6)),
            ];
            layers.push((look::PAINT, vec![("colours", solid(Color::WHITE))], vec![(look::SUB_SHADOW, shadow)]));
            layers.push((
                look::OUTLINE,
                vec![
                    ("width", Value::Number(6.0)),
                    ("sits", Value::Choice("outside".into())),
                    ("colour_from", Value::Choice("own".into())),
                    ("colour", solid(Color::rgb(0, 0, 0))),
                ],
                vec![],
            ));
        }
        Look::SpinningRing => {
            s.words = say("ROUND AND ROUND • ROUND AND ROUND • ");
            s.font = "Bebas Neue".into();
            s.size = 70.0;
            s.letter_spacing = 6.0;
            s.path = TextPath::RoundACircle;
            s.radius = 330.0;
            s.spin = 30.0;
            screen = false;
            layers.push((look::PAINT, vec![("colours", solid(Color::rgb(0xff, 0xd8, 0x4a)))], vec![]));
        }
        Look::NeonSign => {
            s.words = say("NEON");
            s.font = "Monoton".into();
            s.size = 200.0;
            screen = false;
            let pink = Color::rgb(0xff, 0x3d, 0xf0);
            // The dark tube behind the light.
            layers.push((look::PAINT, vec![("colours", solid(Color::rgb(0x2a, 0x0a, 0x28)))], vec![]));
            layers.push((
                look::OUTLINE,
                vec![
                    ("width", Value::Number(6.0)),
                    ("sits", Value::Choice("centre".into())),
                    ("colour_from", Value::Choice("own".into())),
                    ("colour", solid(pink)),
                ],
                vec![(look::SUB_LIGHT, vec![("strength", Value::Number(2.0))])],
            ));
            layers.push((
                look::GLOW,
                vec![("colour_from", Value::Choice("own".into())), ("colour", solid(pink))],
                vec![],
            ));
            // A tube now and then buzzing out.
            moves.push((
                look::moves::FLICKER,
                vec![("amount", Value::Number(0.25)), ("style", Value::Choice("faulty".into()))],
            ));
        }
        Look::GlassLetters => {
            s.words = say("GLASS");
            s.font = "Bowlby One".into();
            s.size = 220.0;
            screen = false;
            s = fitted(scene, s);
            use crate::modules::look::surface::{SurfaceKind, preset_values};
            use bpe_core::Choice;
            card.push(("surface", Value::Choice(SurfaceKind::Glass.name().into())));
            card.extend(preset_values(SurfaceKind::Glass));
        }
        Look::ShadowLettering => {
            s.words = say("SUN");
            s.size = 260.0;
            screen = false;
            let mut g = Gradient::even(&[Color::rgb(0xff, 0xe7, 0xb0), Color::rgb(0xc4, 0x7f, 0x34)]);
            g.mix = bpe_core::Mix::Even;
            layers.push((
                look::PAINT,
                vec![
                    ("colours", Value::Gradient(g)),
                    ("colours~form", Value::Choice("straight".into())),
                    ("colours~turn", Value::Number(90.0)),
                ],
                vec![],
            ));
        }
        Look::FlowingGradient => {
            s.words = say("FLOW");
            s.size = 220.0;
            let rainbow = crate::paint::palettes::built_in()
                .into_iter()
                .find(|p| p.name == "Rainbow")
                .map(|p| p.colours)
                .unwrap_or_else(|| Gradient::solid(Color::WHITE));
            layers.push((
                look::PAINT,
                vec![
                    ("colours", Value::Gradient(rainbow)),
                    ("colours~form", Value::Choice("straight".into())),
                    ("colours~cycle", Value::Number(0.3)),
                    ("colours~wave", Value::Number(1.0)),
                ],
                vec![],
            ));
        }
        Look::HitCounter => {
            s.words = say("{hits}");
            s.size = 260.0;
            position = Vec2::new(0.0, -600.0);
            layers.push((look::PAINT, vec![("colours", solid(Color::WHITE))], vec![]));
            layers.push(outlined(10.0));
        }
        Look::CountdownTimer => {
            s.words = say("{countdown 30|m:ss}");
            s.font = "Space Mono".into();
            s.size = 150.0;
            position = Vec2::new(0.0, -620.0);
            layers.push((look::PAINT, vec![("colours", solid(Color::WHITE))], vec![]));
            layers.push(outlined(6.0));
        }
        Look::LyricsOnTheBeat => {
            s.words = say("HERE IT COMES\nWATCH IT FALL\nONE MORE TIME\nDROP");
            s.size = 110.0;
            s.show = ShowBy::LineByLine;
            s.timed_by = TimedBy::Beats;
            s.every = 4.0;
            s.appear = TextMove::Pop;
            s.disappear = TextMove::Fade;
            s.stays = 1.8;
            position = Vec2::new(0.0, -450.0);
            layers.push((look::PAINT, vec![("colours", solid(Color::WHITE))], vec![]));
            layers.push(outlined(7.0));
        }
        Look::LabelsOnBalls => {
            s.words = say("{ball number}");
            s.size = 36.0;
            screen = false;
            // Inside the picked group of balls, else the first there is.
            let rides =
                |id: &ThingId| scene.things.get(id).is_some_and(|t| matches!(t.kind.as_str(), "balls" | "emitter"));
            if !rides(&parent)
                && let Some(first) = scene.order.iter().find(|id| rides(id))
            {
                parent = *first;
            }
            layers.push((look::PAINT, vec![("colours", solid(Color::WHITE))], vec![]));
            layers.push(outlined(4.0));
        }
        Look::RaceLeaderboard => {
            s.words = say("{standings down 5}");
            s.font = "Space Mono".into();
            s.size = 42.0;
            s.align = Align::Left;
            s.line_spacing = 1.3;
            // The block is centred on its place: its left edge about 40 px in from the screen's.
            position = Vec2::new(-200.0, -760.0);
            layers.push((look::PAINT, vec![("colours", solid(Color::WHITE))], vec![]));
            layers.push(outlined(4.0));
        }
        Look::BallsInsideAWord => {
            s.words = say("BALLS");
            s.font = "Bowlby One".into();
            s.size = 360.0;
            s.balls = TextBalls::BounceInside;
            s.thickness = 12.0;
            screen = false;
            s = fitted(scene, s);
            let mut g = Gradient::even(&[Color::rgb(0x3d, 0xe8, 0xff), Color::rgb(0xff, 0x4f, 0xd8)]);
            g.mix = bpe_core::Mix::Even;
            layers.push((look::PAINT, vec![("colours", Value::Gradient(g))], vec![]));
            layers.push((look::GLOW, vec![], vec![]));
            // A ball in each letter, where its stroke is widest, set off a random way.
            let (inner, _) = letters_of(scene, &s);
            let fits = inner.iter().map(|(_, d)| *d).fold(f32::INFINITY, f32::min);
            let radius = (0.5 * fits).clamp(4.0, 16.0);
            let mut b = custom_balls(inner.iter().map(|(p, _)| p.to_array()).collect(), radius);
            b.velocity = Vec2::new(260.0, 0.0);
            b.launch = Launch::Random;
            balls = Some(BallsBeside { settings: b, position: Vec2::ZERO, falls: false });
        }
        Look::BounceOffLetters => {
            s.words = say("BOUNCE");
            s.font = "Archivo Black".into();
            s.size = 300.0;
            s.balls = TextBalls::BounceOff;
            screen = false;
            s = fitted(scene, s);
            position = Vec2::new(0.0, 150.0);
            layers.push(flashing(Color::rgb(0xff, 0xd8, 0x4a)));
            let mut b =
                BallsSettings { radius: 18.0, group: true, shape: GroupShape::Line, count: 9, ..Default::default() };
            b.space = 50.0;
            b.velocity = Vec2::new(60.0, 0.0);
            balls = Some(BallsBeside { settings: b, position: Vec2::new(0.0, -250.0), falls: true });
        }
        Look::PourInAndFill => {
            s.words = say("FILL");
            s.font = "Bowlby One".into();
            s.size = 420.0;
            s.balls = TextBalls::BounceInside;
            s.thickness = 8.0;
            screen = false;
            s = fitted(scene, s);
            layers.push((look::PAINT, vec![("colours", solid(Color::WHITE))], vec![]));
            // 60 small balls, let go one by one from each letter's widest place in turn.
            let (inner, _) = letters_of(scene, &s);
            let places: Vec<[f32; 2]> =
                (0..60).filter_map(|k| inner.get(k % inner.len().max(1))).map(|(p, _)| p.to_array()).collect();
            let mut b = custom_balls(places, 7.0);
            b.release = Release::OneByOne;
            b.release_gap = 0.15;
            b.velocity = Vec2::new(150.0, 0.0);
            b.launch = Launch::Random;
            balls = Some(BallsBeside { settings: b, position: Vec2::ZERO, falls: true });
        }
        Look::LettersBreakApart => {
            s.words = say("SMASH");
            s.font = "Bungee".into();
            s.size = 280.0;
            s.balls = TextBalls::BounceOff;
            screen = false;
            s = fitted(scene, s);
            position = Vec2::new(0.0, 150.0);
            layers.push(flashing(Color::rgb(0xff, 0x5a, 0x4a)));
            health = vec![
                ("start", Value::Number(3.0)),
                ("at_zero", Value::Choice("break".into())),
                ("look", Value::Choice("shatter".into())),
            ];
            let mut b =
                BallsSettings { radius: 22.0, group: true, shape: GroupShape::Line, count: 7, ..Default::default() };
            b.space = 60.0;
            b.velocity = Vec2::new(120.0, 0.0);
            balls = Some(BallsBeside { settings: b, position: Vec2::new(0.0, -250.0), falls: true });
        }
        Look::LettersThatPlayNotes => {
            s.words = say("FACADE");
            s.font = "Fredoka".into();
            s.weight = 600.0;
            s.size = 260.0;
            s.balls = TextBalls::BounceOff;
            screen = false;
            s = fitted(scene, s);
            position = Vec2::new(0.0, 150.0);
            layers.push(flashing(Color::rgb(0x7c, 0xf5, 0xa8)));
            sound = vec![("notes_plays", Value::Choice("spell_notes".into()))];
            // A ball over each letter, let go one after another so the word plays in order.
            // Over each letter's widest stroke (an A's middle is its point).
            let (inner, _) = letters_of(scene, &s);
            let places = inner.iter().map(|(p, _)| [p.x, 0.0]).collect();
            let mut b = custom_balls(places, 16.0);
            b.release = Release::OneByOne;
            b.release_gap = 0.5;
            b.velocity = Vec2::ZERO;
            balls = Some(BallsBeside { settings: b, position: Vec2::new(0.0, -250.0), falls: true });
        }
        Look::WordMadeOfBalls => {
            s.words = say("HI");
            s.font = "Bowlby One".into();
            s.size = 300.0;
            screen = false;
            position = Vec2::new(0.0, 150.0);
            // The Text only spells the balls (its Look card goes below); they fill its letters.
            let b = BallsSettings {
                radius: 7.0,
                group: true,
                shape: GroupShape::Word,
                made_of: crate::things::balls::WordFrom::ATextThing,
                space: 2.0,
                velocity: Vec2::ZERO,
                ..Default::default()
            };
            balls = Some(BallsBeside { settings: b, position, falls: false });
        }
        Look::ParticleWord => {
            s.words = say("GONE");
            s.font = "Space Grotesk".into();
            s.size = 160.0;
            screen = false;
        }
        Look::LaserWriting => {
            s.words = say("LASER");
            s.font = "Orbitron".into();
            s.size = 200.0;
            screen = false;
            s = fitted(scene, s);
            let red = Color::rgb(0xff, 0x2a, 0x2a);
            layers.push((look::PAINT, vec![("colours", solid(red))], vec![]));
            layers.push((
                look::GLOW,
                vec![("colour_from", Value::Choice("own".into())), ("colour", solid(red))],
                vec![(look::SUB_LIGHT, vec![("strength", Value::Number(1.5))])],
            ));
            moves.push((
                look::moves::DRAW_ON,
                vec![
                    ("start", Value::Number(0.5)),
                    ("takes", Value::Number(4.0)),
                    ("width", Value::Number(6.0)),
                    ("then", Value::Choice("lines_only".into())),
                ],
            ));
        }
        Look::TextFollowsSound => {
            s.words = say("BASS");
            s.size = 260.0;
            layers.push((look::PAINT, vec![("colours", solid(Color::rgb(0x8a, 0xf0, 0xff)))], vec![]));
            layers.push(outlined(6.0));
            moves.push((
                look::moves::SOUND,
                vec![("later", Value::Number(0.05)), ("own", Value::Number(0.3)), ("jump", Value::Number(80.0))],
            ));
        }
        Look::Glitch => {
            s.words = say("GLITCH");
            s.font = "Bungee".into();
            s.size = 150.0;
            layers.push((look::PAINT, vec![("colours", solid(Color::WHITE))], vec![]));
            use look::effects::{GLITCH, SPLIT};
            moves.push((SPLIT, vec![("distance", Value::Number(4.0)), ("harder", Value::Number(0.8))]));
            moves.push((
                GLITCH,
                vec![
                    ("amount", Value::Number(0.12)),
                    ("band", Value::Number(14.0)),
                    ("tear", Value::Number(30.0)),
                    ("tear_split", Value::Number(8.0)),
                    ("harder", Value::Number(0.8)),
                ],
            ));
        }
        Look::MeltingText => {
            s.words = say("MELT");
            s.font = "Bowlby One".into();
            s.size = 220.0;
            let mut g = Gradient::even(&[Color::rgb(0xff, 0xcf, 0x4a), Color::rgb(0xff, 0x4f, 0x8b)]);
            g.mix = bpe_core::Mix::Even;
            layers.push((
                look::PAINT,
                vec![
                    ("colours", Value::Gradient(g)),
                    ("colours~form", Value::Choice("straight".into())),
                    ("colours~turn", Value::Number(90.0)),
                ],
                vec![],
            ));
            moves.push((
                look::effects::MELT,
                vec![
                    ("drip", Value::Number(260.0)),
                    ("speed", Value::Number(0.2)),
                    ("start", Value::Number(1.0)),
                    ("every", Value::Number(9.0)),
                    ("width", Value::Number(10.0)),
                ],
            ));
        }
        Look::KaleidoscopeWords => {
            s.words = say("WOW");
            s.font = "Bungee".into();
            s.size = 80.0;
            s.spin = 70.0;
            position = Vec2::new(150.0, 0.0);
            let rainbow = crate::paint::palettes::built_in()
                .into_iter()
                .find(|p| p.name == "Rainbow")
                .map(|p| p.colours)
                .unwrap_or_else(|| Gradient::solid(Color::WHITE));
            layers.push((
                look::PAINT,
                vec![
                    ("colours", Value::Gradient(rainbow)),
                    ("colours~form", Value::Choice("straight".into())),
                    ("colours~cycle", Value::Number(0.2)),
                ],
                vec![],
            ));
            moves.push((
                look::effects::MIRROR,
                vec![
                    ("ways", Value::Int(8)),
                    ("round", Value::Choice("the_middle_of_the_picture".into())),
                    ("mirrored", Value::Bool(true)),
                    ("wedges", Value::Bool(true)),
                    ("spin", Value::Number(17.0)),
                ],
            ));
        }
        Look::EchoStack => {
            s.words = say("DEEP");
            s.size = 200.0;
            layers.push((look::PAINT, vec![("colours", solid(Color::WHITE))], vec![]));
            layers.push(outlined(5.0));
            moves.push((
                look::effects::ECHO,
                vec![
                    ("copies", Value::Int(24)),
                    ("toward", Value::Vec2([0.0, 0.0])),
                    ("wander", Value::Number(300.0)),
                    ("wander_speed", Value::Number(0.12)),
                    ("reach", Value::Number(0.8)),
                    ("last_size", Value::Number(0.45)),
                    ("hue", Value::Number(12.0)),
                    ("hue_spin", Value::Number(40.0)),
                    ("colourise", Value::Number(1.0)),
                    ("last_opacity", Value::Number(0.9)),
                ],
            ));
        }
    }
    let id = crate::add_thing(scene, reg, KIND, parent)?;
    // An Empty text keeps the name a new Text gets; a look is named after itself.
    let name = (look != Look::Empty).then(|| {
        if let Some(t) = scene.things.get_mut(&id) {
            t.name.clear();
        }
        scene.free_name(look.name())
    });
    if let Some(t) = scene.things.get_mut(&id) {
        t.params = s.to_params();
        if let Some(name) = name {
            t.name = name;
        }
        t.screen = Some(screen);
        t.transform.position = position;
        // Labels ride on their balls: drawn over them, not hidden under them.
        if look == Look::LabelsOnBalls {
            t.z_order = 1;
        }
    }
    crate::riding::fit_choice(scene, id);
    for (kind, params, subs) in layers {
        let layer = scene.add_layer(reg, id, SlotId::Look, kind)?;
        put(scene, id, layer, &params);
        for (sub_kind, sub_params) in subs {
            let sub = scene.add_sub_layer(reg, id, SlotId::Look, layer, sub_kind)?;
            put(scene, id, sub, &sub_params);
        }
    }
    if let Some(c) = scene.card_mut(id, SlotId::Look) {
        for (k, v) in card {
            c.params.set(k, v);
        }
    }
    for (kind, params) in moves {
        let layer = scene.add_layer(reg, id, SlotId::Look, kind)?;
        put(scene, id, layer, &params);
        // Its Level follows how loud Bus 1 is.
        if look == Look::TextFollowsSound && kind == look::moves::SOUND {
            let address = bpe_core::Address { thing: id, slot: Some(SlotId::Look), key: layered_key(layer, "level") };
            let pulse = bpe_core::Pulse {
                source: bpe_core::PulseSource::Sound { shape: ThingId::ROOT, source: bpe_core::SoundSource::Bus(1) },
                amount: Value::Number(1.0),
                range: Some((0.0, 1.0)),
                ..Default::default()
            };
            scene.pulses.insert(address, pulse);
        }
    }
    if look == Look::WordMadeOfBalls
        && let Some(t) = scene.things.get_mut(&id)
    {
        t.slots.remove(&SlotId::Look);
    }
    if look.lit() {
        let lit = scene.things.get(&ThingId::ROOT).is_some_and(|r| r.slots.contains_key(&SlotId::Lighting));
        if !lit {
            scene.add_slot(reg, ThingId::ROOT, SlotId::Lighting, crate::modules::lighting::LIGHTING)?;
        }
    }
    if !health.is_empty() {
        crate::fill_slot(scene, reg, id, SlotId::Health)?;
        if let Some(c) = scene.card_mut(id, SlotId::Health) {
            for (k, v) in health {
                c.params.set(k, v);
            }
        }
    }
    if !sound.is_empty() {
        let mut card = crate::sound::starter_voice(reg);
        for (k, v) in sound {
            card.params.set(k, v);
        }
        if let Some(t) = scene.things.get_mut(&id) {
            t.slots.insert(SlotId::Sound, card);
        }
    }
    let mut word_balls = None;
    if let Some(b) = balls {
        // Beside the Text.
        let ball = crate::add_thing(scene, reg, "balls", parent)?;
        let name = scene.free_name("Balls");
        let mut settings = b.settings;
        settings.word_text = bpe_core::Target::Thing(id);
        if let Some(t) = scene.things.get_mut(&ball) {
            t.params = settings.to_params();
            t.transform.position = b.position;
            t.name = name;
        }
        word_balls = Some(ball);
        if b.falls
            && let Some(m) = scene.card_mut(ball, SlotId::Motion)
        {
            m.params.set("gravity_scale", Value::Number(1.0));
        }
    }
    match look {
        // A big ball dropped on the word.
        Look::WordMadeOfBalls => {
            let big = crate::add_thing(scene, reg, "balls", parent)?;
            let name = scene.free_name("Big ball");
            if let Some(t) = scene.things.get_mut(&big) {
                t.params =
                    BallsSettings { radius: 40.0, velocity: Vec2::new(0.0, 200.0), ..Default::default() }.to_params();
                t.transform.position = Vec2::new(-120.0, -500.0);
                t.name = name;
            }
            if let Some(m) = scene.card_mut(big, SlotId::Motion) {
                m.params.set("gravity_scale", Value::Number(1.0));
            }
            // The word's balls and the big one bump into each other.
            for b in word_balls.into_iter().chain([big]) {
                if let Some(c) = scene.card_mut(b, SlotId::Bounce) {
                    c.params.set("hit_balls", Value::Bool(true));
                }
            }
        }
        // Specks holding the word, sprung home, blown away by gusts.
        Look::ParticleWord => {
            use crate::things::particles::{self, looks::Look as Specks};
            let specks = particles::looks::add_particles(scene, reg, Specks::FormAWord, parent)?;
            let name = scene.free_name("Particle word");
            if let Some(t) = scene.things.get_mut(&specks) {
                t.params.set("word_from", Value::Choice("a_text_thing".into()));
                t.params.set("text_thing", bpe_core::SettingValue::to_value(&bpe_core::Target::Thing(id)));
                t.name = name;
            }
            let wind = scene.add_layer(reg, specks, SlotId::Motion, particles::forces::WIND)?;
            if let Some(card) = scene.card_mut(specks, SlotId::Motion) {
                for (k, v) in
                    [("speed", 700.0), ("direction", 0.0), ("gusts", 1.0), ("gust_every", 5.0), ("waves", 600.0)]
                {
                    card.params.set(&layered_key(wind, k), Value::Number(v));
                }
            }
            // The Text only spells the specks.
            if let Some(t) = scene.things.get_mut(&id) {
                t.slots.remove(&SlotId::Look);
            }
        }
        // A red laser from above aimed at the pen, and a hot point riding it.
        Look::LaserWriting => {
            use crate::things::light::{LightType, add_light};
            let laser = add_light(scene, reg, LightType::Laser, id)?;
            if let Some(l) = scene.things.get_mut(&laser) {
                l.params.set("position", Value::Vec2([0.0, -700.0]));
                l.params.set("points", Value::Choice("at_the_pen".into()));
                l.params.set("colour", Value::Color(Color::rgb(0xff, 0x30, 0x30)));
            }
            let tip = add_light(scene, reg, LightType::Point, id)?;
            if let Some(l) = scene.things.get_mut(&tip) {
                l.params.set("rides_on", Value::Choice("the_pen".into()));
                l.params.set("position", Value::Vec2([0.0, 0.0]));
                l.params.set("colour", Value::Color(Color::rgb(0xff, 0x80, 0x60)));
            }
        }
        _ => {}
    }
    // A lamp up and to the left: Shadow lettering's casts the long shadow; Glass letters' gives the
    // glass its highlights and coloured shadow (without one it looks flat grey).
    if look == Look::ShadowLettering || look == Look::GlassLetters {
        use crate::things::light::{LightType, add_light};
        let lamp = add_light(scene, reg, LightType::Point, parent)?;
        // The glass's lamp is close, so its highlights are big and bright.
        let (colour, off) = if look == Look::GlassLetters {
            (Color::WHITE, Vec2::new(-220.0, -360.0))
        } else {
            (Color::rgb(0xff, 0xe8, 0xc8), Vec2::new(-300.0, -700.0))
        };
        if let Some(l) = scene.things.get_mut(&lamp) {
            l.params.set("position", Value::Vec2([position.x + off.x, position.y + off.y]));
            l.params.set("colour", Value::Color(colour));
        }
    }
    Ok(id)
}

/// Writes `params` as layer `layer`'s settings on the Text's Look card.
fn put(scene: &mut Scene, id: ThingId, layer: bpe_core::LayerId, params: &[(&str, Value)]) {
    if let Some(card) = scene.card_mut(id, SlotId::Look) {
        for (k, v) in params {
            card.params.set(&layered_key(layer, k), v.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::resolve;
    use bpe_gfx::{Frame, build_frame};
    use bpe_runtime::{Log, World};

    fn added(look: Look) -> (Scene, ThingId) {
        let reg = crate::registry();
        let mut scene = Scene::empty(&reg);
        let id = add_text(&mut scene, &reg, look, ThingId::ROOT).unwrap();
        (scene, id)
    }

    #[test]
    fn every_look_adds_a_text_that_resolves_cleanly() {
        let kit = crate::kit();
        let reg = &kit.registry;
        for look in std::iter::once(Look::Empty).chain(LOOKS) {
            // Beside a group of balls (the leaderboard and the labels read them), a moment in.
            let mut scene = Scene::empty(reg);
            let balls = crate::add_thing(&mut scene, reg, "balls", ThingId::ROOT).unwrap();
            let mut b = crate::BallsSettings::from_params(&scene.things[&balls].params);
            b.ring(4, 200.0);
            scene.things.get_mut(&balls).unwrap().params = b.to_params();
            add_text(&mut scene, reg, look, ThingId::ROOT).unwrap();
            let resolved = resolve(&scene, reg);
            assert!(resolved.problems.is_empty(), "{look:?}: {:?}", resolved.problems);
            let mut world = World::build(&scene, &resolved, &kit.sims);
            let mut log = Log::default();
            world.advance_to(1.0, &kit.sims, &mut log);
            let mut frame = Frame::default();
            build_frame(&scene, &resolved, &world, &log, 1.0, &kit.draws, &mut frame);
            let paths = frame.paths.len()
                + frame.overlay.as_ref().map_or(0, |o| o.paths.len())
                + frame.layers.iter().map(|l| l.frame.paths.len()).sum::<usize>();
            // The word-made-of looks' Texts only spell their balls or specks.
            if matches!(look, Look::WordMadeOfBalls | Look::ParticleWord) {
                assert!(scene.things.values().all(|t| t.kind != KIND || !t.slots.contains_key(&SlotId::Look)));
                continue;
            }
            assert!(paths > 0, "{look:?} draws");
        }
    }

    #[test]
    fn the_part_5_looks_copy_and_reshape_their_letters() {
        let kit = crate::kit();
        let reg = &kit.registry;
        for (look, words, font, at) in [
            (Look::Glitch, "GLITCH", "Bungee", 0.5),
            (Look::MeltingText, "MELT", "Bowlby One", 4.0),
            (Look::KaleidoscopeWords, "WOW", "Bungee", 1.0),
            (Look::EchoStack, "DEEP", "Anton", 1.0),
        ] {
            let (scene, id) = added(look);
            let s = TextSettings::from_params(&scene.things[&id].params);
            assert_eq!((s.words.as_str(), s.font.as_str()), (words, font), "{look:?}");
            assert_eq!(scene.things[&id].screen, Some(true), "{look:?}");
            let resolved = resolve(&scene, reg);
            assert!(resolved.problems.is_empty(), "{look:?}: {:?}", resolved.problems);
            let fx = super::super::fx::Fx::of(&resolved.things[&id].slots[&SlotId::Look]);
            assert!(!fx.is_empty(), "{look:?} has its effects");
            let mut world = World::build(&scene, &resolved, &kit.sims);
            let mut log = Log::default();
            world.advance_to(at, &kit.sims, &mut log);
            let mut frame = Frame::default();
            build_frame(&scene, &resolved, &world, &log, at, &kit.draws, &mut frame);
            let drawn = frame.overlay.as_ref().map_or(0, |o| o.paths.len());
            let letters = words.chars().count();
            if look == Look::MeltingText {
                assert_eq!(drawn, letters, "melting reshapes the letters, no copies");
                let o = frame.overlay.as_ref().unwrap();
                assert!(o.paths.iter().all(|p| p.b[3] > 40.0), "cut finer to drip");
            } else if look != Look::Glitch {
                assert!(drawn >= letters * 4, "{look:?}: {drawn} instances for {letters} letters");
            } else {
                // Colour split: three a letter at least.
                assert!(drawn >= letters * 3, "{look:?}: {drawn}");
            }
        }
    }

    #[test]
    fn labels_draw_over_their_balls() {
        let reg = crate::registry();
        let mut scene = Scene::empty(&reg);
        let balls = crate::add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
        let id = add_text(&mut scene, &reg, Look::LabelsOnBalls, ThingId::ROOT).unwrap();
        let (labels, under) = (scene.things[&id].z_order, scene.things[&balls].z_order);
        assert!(labels > under, "labels {labels} over balls {under}");
    }

    #[test]
    fn every_look_s_words_fit_on_the_screen() {
        // The screen is 1080 wide, its middle at 0; words laid out round their place.
        for look in LOOKS {
            let (scene, id) = added(look);
            let Some(t) = scene.things.get(&id) else { continue };
            if t.kind != KIND || !t.slots.contains_key(&SlotId::Look) {
                continue;
            }
            let s = TextSettings::from_params(&t.params);
            // Live words ({countdown …}) are laid out as they read when drawn.
            if s.words.trim().is_empty() || s.words.contains('{') || s.path != TextPath::Straight {
                continue;
            }
            let found = fonts::find(&scene.fonts, &s.font, s.weight);
            let laid = layout(found.font(), &s.layout_spec());
            let (left, right) = (t.transform.position.x + laid.min.x, t.transform.position.x + laid.max.x);
            assert!(left >= -540.0 && right <= 540.0, "{look:?} runs off the screen: {left} to {right}");
        }
    }

    #[test]
    fn the_lit_looks_light_the_scene() {
        for look in LOOKS {
            let (scene, _) = added(look);
            let lit = scene.things[&ThingId::ROOT].slots.contains_key(&SlotId::Lighting);
            assert_eq!(lit, look.lit(), "{look:?}");
            let lights = scene.things.values().filter(|t| t.kind == crate::things::light::LIGHT).count();
            let want = match look {
                Look::ShadowLettering | Look::GlassLetters => 1,
                Look::LaserWriting => 2,
                _ => 0,
            };
            assert_eq!(lights, want, "{look:?}");
        }
    }

    #[test]
    fn each_look_sets_its_words_and_font() {
        let table = [
            (Look::HookTitle, "WAIT FOR\nTHE LAST\nONE…", "Anton", true),
            (Look::SpinningRing, "ROUND AND ROUND • ROUND AND ROUND • ", "Bebas Neue", false),
            (Look::NeonSign, "NEON", "Monoton", false),
            (Look::GlassLetters, "GLASS", "Bowlby One", false),
            (Look::ShadowLettering, "SUN", "Anton", false),
            (Look::FlowingGradient, "FLOW", "Anton", true),
            (Look::HitCounter, "{hits}", "Anton", true),
            (Look::CountdownTimer, "{countdown 30|m:ss}", "Space Mono", true),
            (Look::LyricsOnTheBeat, "HERE IT COMES\nWATCH IT FALL\nONE MORE TIME\nDROP", "Anton", true),
            (Look::LabelsOnBalls, "{ball number}", "Anton", false),
            (Look::RaceLeaderboard, "{standings down 5}", "Space Mono", true),
            (Look::BallsInsideAWord, "BALLS", "Bowlby One", false),
            (Look::BounceOffLetters, "BOUNCE", "Archivo Black", false),
            (Look::PourInAndFill, "FILL", "Bowlby One", false),
            (Look::LettersBreakApart, "SMASH", "Bungee", false),
            (Look::LettersThatPlayNotes, "FACADE", "Fredoka", false),
        ];
        for (look, words, font, screen) in table {
            let (scene, id) = added(look);
            let s = TextSettings::from_params(&scene.things[&id].params);
            assert_eq!((s.words.as_str(), s.font.as_str()), (words, font), "{look:?}");
            assert_eq!(scene.things[&id].screen, Some(screen), "{look:?}");
            assert_eq!(scene.things[&id].name, look.name(), "{look:?}");
        }
        let (scene, id) = added(Look::SpinningRing);
        let s = TextSettings::from_params(&scene.things[&id].params);
        assert_eq!((s.path, s.radius, s.spin), (TextPath::RoundACircle, 330.0, 30.0));
        let (scene, id) = added(Look::GlassLetters);
        assert_eq!(scene.card(id, SlotId::Look).unwrap().params.get("surface"), Some(&Value::Choice("glass".into())));
    }

    #[test]
    fn the_live_looks_show_and_ride() {
        let (scene, id) = added(Look::HookTitle);
        let s = TextSettings::from_params(&scene.things[&id].params);
        assert_eq!((s.show, s.appear, s.each), (ShowBy::WordByWord, TextMove::Pop, 0.12));
        let (scene, id) = added(Look::LyricsOnTheBeat);
        let s = TextSettings::from_params(&scene.things[&id].params);
        assert_eq!((s.show, s.timed_by, s.every, s.stays), (ShowBy::LineByLine, TimedBy::Beats, 4.0, 1.8));
        // Labels go inside the first group of balls when none is picked.
        let reg = crate::registry();
        let mut scene = Scene::empty(&reg);
        let balls = crate::add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
        let id = add_text(&mut scene, &reg, Look::LabelsOnBalls, ThingId::ROOT).unwrap();
        assert_eq!(scene.things[&id].parent, Some(balls));
        // With no balls, under what was picked.
        let (scene, id) = added(Look::LabelsOnBalls);
        assert_eq!(scene.things[&id].parent, Some(ThingId::ROOT));
    }

    /// The new scene (its ring and ball) with `look` added, run for `seconds`: the world, its log
    /// and the Text's and its balls' ids.
    fn run_look(look: Look, seconds: f64) -> (World, Log, ThingId, ThingId) {
        let kit = crate::kit();
        let reg = &kit.registry;
        let mut scene = crate::new_scene(reg);
        // The scene's own ball out of the way.
        let first = scene.order.iter().copied().find(|id| scene.things[id].kind == "balls").unwrap();
        scene.things.get_mut(&first).unwrap().enabled = false;
        let id = add_text(&mut scene, reg, look, ThingId::ROOT).unwrap();
        let balls = scene.order.iter().copied().rfind(|b| scene.things[b].kind == "balls").unwrap();
        assert_ne!(balls, first, "{look:?} adds its balls");
        let resolved = resolve(&scene, reg);
        assert!(resolved.problems.is_empty(), "{look:?}: {:?}", resolved.problems);
        let mut world = World::build(&scene, &resolved, &kit.sims);
        let mut log = Log::default();
        world.advance_to(seconds, &kit.sims, &mut log);
        (world, log, id, balls)
    }

    fn letter_hits(log: &Log, id: ThingId) -> Vec<u32> {
        log.events()
            .iter()
            .filter(|e| e.kind == bpe_runtime::EventKind::Hit)
            .filter_map(|e| e.b.filter(|b| b.thing == id).map(|b| b.item))
            .collect()
    }

    #[test]
    fn the_real_letter_looks_play_out() {
        for look in LOOKS {
            assert_eq!(
                look.adds_balls(),
                matches!(
                    look,
                    Look::BallsInsideAWord
                        | Look::BounceOffLetters
                        | Look::PourInAndFill
                        | Look::LettersBreakApart
                        | Look::LettersThatPlayNotes
                )
            );
        }
        // A ball in each letter of BALLS, still inside it after a while, hitting the walls.
        let (w, log, id, balls) = run_look(Look::BallsInsideAWord, 4.0);
        let bpe_runtime::Outline::Letters { letters, center, .. } = &w.walls[&id].outline else { panic!() };
        let b = &w.bodies[&balls];
        assert_eq!(b.len(), 5);
        for row in 0..5 {
            assert!(letters.list[row].inside(b.position[row] - *center), "ball {row} left its letter");
        }
        let hits = letter_hits(&log, id);
        assert!((0..5).all(|k| hits.contains(&k)), "{hits:?}");
        // Balls fall onto BOUNCE and bounce off it, many letters hit.
        let (w, log, id, balls) = run_look(Look::BounceOffLetters, 3.0);
        let mut hit = letter_hits(&log, id);
        hit.sort();
        hit.dedup();
        assert!(hit.len() >= 4, "{hit:?}");
        assert!(w.bodies[&balls].position.iter().all(|p| p.y < 150.0), "on top, not through");
        // FILL fills up: 60 balls let go one by one, all inside the letters.
        let (w, _, id, balls) = run_look(Look::PourInAndFill, 10.0);
        let bpe_runtime::Outline::Letters { letters, center, .. } = &w.walls[&id].outline else { panic!() };
        let b = &w.bodies[&balls];
        assert_eq!(b.len(), 60);
        let inside = b.position.iter().filter(|p| letters.inside(**p - *center)).count();
        assert!(inside >= 57, "{inside} of 60 inside");
        // SMASH's letters break after three hits each.
        let (w, log, id, _) = run_look(Look::LettersBreakApart, 8.0);
        let broken = w.parts[&id].alive.iter().filter(|a| !**a).count();
        assert!(broken >= 1, "{:?}", letter_hits(&log, id));
        let health = w.rules().unwrap().health();
        for (k, alive) in w.parts[&id].alive.iter().enumerate() {
            if *alive {
                assert!(health.of(id, Some(k as u32)).unwrap().now > 0.0);
            }
        }
        // FACADE: each ball falls onto its own letter, in order.
        let (_, log, id, _) = run_look(Look::LettersThatPlayNotes, 4.0);
        let mut first = Vec::new();
        for k in letter_hits(&log, id) {
            if !first.contains(&k) {
                first.push(k);
            }
        }
        assert_eq!(first, [0, 1, 2, 3, 4, 5], "letter by letter");
    }

    #[test]
    fn the_part_4_looks_play_out() {
        let kit = crate::kit();
        let reg = &kit.registry;
        // HI built of balls inside its letters; the big ball knocks some loose.
        let mut scene = Scene::empty(reg);
        let id = add_text(&mut scene, reg, Look::WordMadeOfBalls, ThingId::ROOT).unwrap();
        let word = scene.order.iter().copied().find(|b| scene.things[b].kind == "balls").unwrap();
        let resolved = resolve(&scene, reg);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let mut world = World::build(&scene, &resolved, &kit.sims);
        let letters = super::super::shape::letters_of(&scene, id);
        let at = scene.things[&word].transform.position;
        let start = world.bodies[&word].position.clone();
        assert!(start.len() > 50, "{} balls", start.len());
        assert!(start.iter().all(|p| crate::things::balls::shapes::winding(&letters, *p - at) != 0));
        let mut log = Log::default();
        world.advance_to(5.0, &kit.sims, &mut log);
        let moved = world.bodies[&word].position.iter().zip(&start).filter(|(p, q)| p.distance(**q) > 5.0).count();
        let big = scene.order.iter().copied().rfind(|b| scene.things[b].kind == "balls").unwrap();
        assert!(moved > 0, "the big ball knocks some loose: {:?}", world.bodies[&big].position);
        // GONE in specks: the Particles thing reads the Text's letters.
        let mut scene = Scene::empty(reg);
        let id = add_text(&mut scene, reg, Look::ParticleWord, ThingId::ROOT).unwrap();
        let specks = scene.order.iter().copied().find(|b| scene.things[b].kind == "particles").unwrap();
        let s = crate::things::particles::ParticlesSettings::from_params(&scene.things[&specks].params);
        assert_eq!(s.text_thing, bpe_core::Target::Thing(id));
        let resolved = resolve(&scene, reg);
        let world = World::build(&scene, &resolved, &kit.sims);
        let st = world.states[&specks].as_any().downcast_ref::<crate::things::particles::state::ParticlesState>();
        assert!(st.unwrap().spots.len() > 100, "the word's places");
        // LASER: the pen is on the word at 2 s.
        let mut scene = Scene::empty(reg);
        let id = add_text(&mut scene, reg, Look::LaserWriting, ThingId::ROOT).unwrap();
        let resolved = resolve(&scene, reg);
        let world = World::build(&scene, &resolved, &kit.sims);
        let r = &resolved.things[&id];
        let at = super::super::draw::TextAt {
            look: r.slots.get(&SlotId::Look),
            ..super::super::draw::TextAt::new(&scene, &r.params, &world, id, 2.0)
        };
        let pen = super::super::moves::pen(&at).expect("a pen at 2 s");
        assert!(pen.x.abs() < 600.0 && pen.y.abs() < 200.0, "{pen}");
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 2.0, &kit.draws, &mut frame);
        let lit = frame.light.as_ref().expect("lit");
        let ends = lit.segments.iter().any(|s| Vec2::new(s[2], s[3]).distance(pen) < 2.0);
        assert!(ends, "{:?} ends at {pen}", lit.segments);
        // BASS follows Bus 1.
        let mut scene = Scene::empty(reg);
        let id = add_text(&mut scene, reg, Look::TextFollowsSound, ThingId::ROOT).unwrap();
        let pulse = scene.pulses.iter().find(|(a, _)| a.thing == id).expect("a pulse").1;
        assert_eq!(
            pulse.source,
            bpe_core::PulseSource::Sound { shape: ThingId::ROOT, source: bpe_core::SoundSource::Bus(1) }
        );
    }

    #[test]
    fn the_notes_look_spells_its_word() {
        let (scene, id) = added(Look::LettersThatPlayNotes);
        let card = scene.card(id, SlotId::Sound).expect("a Sound card");
        assert_eq!(card.params.get("notes_plays"), Some(&Value::Choice("spell_notes".into())));
        let (scene, id) = added(Look::LettersBreakApart);
        let card = scene.card(id, SlotId::Health).expect("a Health card");
        assert_eq!(card.params.get("look"), Some(&Value::Choice("shatter".into())));
    }
}

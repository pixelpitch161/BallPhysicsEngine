//! Text: a caption over the picture that can show the time, a countdown or the counts.

pub mod draw;
pub mod fill;

pub use draw::TextDraw;
pub use fill::{Values, fill};

use bpe_core::{Choice, Color, SettingDef, Settings, SlotId, ThingKind};
use bpe_gfx::text::{FontFace, TextAlign};

pub struct TextKind;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Font {
    #[default]
    Anton,
    VarelaRound,
    SpaceMono,
}

impl Font {
    pub fn face(self) -> FontFace {
        match self {
            Font::Anton => FontFace::Anton,
            Font::VarelaRound => FontFace::VarelaRound,
            Font::SpaceMono => FontFace::SpaceMono,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Align {
    #[default]
    Center,
    Left,
    Right,
}

impl Align {
    pub fn text_align(self) -> TextAlign {
        match self {
            Align::Center => TextAlign::Center,
            Align::Left => TextAlign::Left,
            Align::Right => TextAlign::Right,
        }
    }
}

/// How the text comes and goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Appear {
    None,
    #[default]
    Fade,
    Pop,
    Rise,
    Typewriter,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct TextSettings {
    /// What it says. {time}, {seconds}, {countdown}, {hits} and {balls} fill in as the video
    /// plays.
    #[setting(label = "Text", simple)]
    pub text: String,
    #[setting(label = "Font", simple, random)]
    pub font: Font,
    /// The height of a capital letter.
    #[setting(label = "Size", range(8, 600), unit = "px", simple, keyframe, random(24, 200), pulse)]
    pub size: f32,
    /// When it first shows.
    #[setting(label = "Shows from", range(0, 600), unit = "s", simple)]
    pub show_from: f32,
    /// When it goes; 0 keeps it to the end.
    #[setting(label = "Shows until", range(0, 600), unit = "s", simple)]
    pub show_to: f32,
    /// How it comes and goes.
    #[setting(label = "Appears", simple, random)]
    pub appear: Appear,
    /// How long coming and going take.
    #[setting(label = "Appear time", range(0.05, 3), unit = "s")]
    pub appear_seconds: f32,
    #[setting(label = "Align")]
    pub align: Align,
    #[setting(label = "Outline", range(0, 20), unit = "px", keyframe)]
    pub outline: f32,
    #[setting(label = "Outline colour", keyframe)]
    pub outline_color: Color,
    /// The text shrinks to fit this much of the video's width; 0 lets it be any width.
    #[setting(label = "Fits in", range(0, 1))]
    pub max_width: f32,
    /// How much the text jumps when something hits.
    #[setting(label = "Jump on hits", range(0, 1))]
    pub jump: f32,
}

impl Default for TextSettings {
    fn default() -> Self {
        Self {
            text: "Hello".into(),
            font: Font::Anton,
            size: 96.0,
            show_from: 0.0,
            show_to: 0.0,
            appear: Appear::Fade,
            appear_seconds: 0.4,
            align: Align::Center,
            outline: 4.0,
            outline_color: Color::rgb(0, 0, 0),
            max_width: 0.9,
            jump: 0.0,
        }
    }
}

const SLOTS: &[SlotId] = &[SlotId::Look, SlotId::Shader];
const DEFAULT_SLOTS: &[SlotId] = &[SlotId::Look];

impl ThingKind for TextKind {
    fn id(&self) -> &'static str {
        "text"
    }
    fn name(&self) -> &'static str {
        "Text"
    }
    fn icon(&self) -> &'static str {
        "text"
    }
    fn help(&self) -> &'static str {
        "A caption. {time}, {seconds}, {countdown}, {hits} and {balls} fill in as the video plays."
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
}

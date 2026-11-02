//! Meter: a bar, ring or ranking on the screen of a canvas's live numbers: each team's share in
//! a colour war, or how much of a cover is scraped away.

pub mod draw;

pub use draw::MeterDraw;

use bpe_core::{Choice, Color, SettingDef, Settings, SlotId, Target, ThingKind};

use super::text::Font;

pub struct MeterKind;

/// Which numbers the meter shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum MeterReads {
    /// Every team's share of the canvas.
    #[default]
    Shares,
    Revealed,
    Covered,
    Wet,
}

/// How the meter looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum MeterLook {
    #[default]
    SplitBar,
    Ring,
    Ranking,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct MeterSettings {
    /// The canvas it reads: the first one this names.
    #[setting(label = "Canvas", simple)]
    pub canvas: Target,
    #[setting(label = "Reads", simple)]
    pub reads: MeterReads,
    #[setting(label = "Look", simple)]
    pub look: MeterLook,
    #[setting(label = "Width", when = "look!=ring", range(100, 1080), unit = "px", keyframe)]
    pub width: f32,
    #[setting(label = "Thickness", range(4, 200), unit = "px", keyframe)]
    pub thickness: f32,
    #[setting(label = "Ring size", when = "look=ring", range(40, 540), unit = "px", keyframe)]
    pub ring_size: f32,
    /// How calmly it moves: 0 follows every change, 1 averages over half a second.
    #[setting(label = "Smooth", range(0, 1))]
    pub smooth: f32,
    #[setting(label = "Show percent")]
    pub show_percent: bool,
    #[setting(label = "Show names", when = "reads=shares")]
    pub show_names: bool,
    #[setting(label = "Font")]
    pub font: Font,
    #[setting(label = "Text size", range(8, 200), unit = "px", keyframe)]
    pub text_size: f32,
    #[setting(label = "Text colour")]
    pub text_colour: Color,
    #[setting(label = "Empty colour")]
    pub empty_colour: Color,
    #[setting(label = "Fill colour", when = "reads!=shares")]
    pub fill_colour: Color,
    #[setting(label = "Rows", when = "look=ranking", range(1, 8))]
    pub max_rows: u32,
}

impl Default for MeterSettings {
    fn default() -> Self {
        Self {
            canvas: Target::Anything,
            reads: MeterReads::Shares,
            look: MeterLook::SplitBar,
            width: 800.0,
            thickness: 56.0,
            ring_size: 160.0,
            smooth: 0.5,
            show_percent: true,
            show_names: true,
            font: Font::Anton,
            text_size: 36.0,
            text_colour: Color::WHITE,
            empty_colour: Color { a: 38, ..Color::WHITE },
            fill_colour: Color::rgb(0xff, 0xd2, 0x3e),
            max_rows: 4,
        }
    }
}

impl ThingKind for MeterKind {
    fn id(&self) -> &'static str {
        "meter"
    }
    fn family(&self) -> &'static str {
        "On screen"
    }
    fn name(&self) -> &'static str {
        "Meter"
    }
    fn icon(&self) -> &'static str {
        "text"
    }
    fn help(&self) -> &'static str {
        "A bar, ring or ranking of a canvas's numbers: each team's share in a colour war, or how much is revealed."
    }
    fn settings(&self) -> &'static [SettingDef] {
        MeterSettings::defs()
    }
    fn slots(&self) -> &'static [SlotId] {
        &[]
    }
    fn default_slots(&self) -> &'static [SlotId] {
        &[]
    }
    fn sticks_to_screen(&self) -> bool {
        true
    }
}

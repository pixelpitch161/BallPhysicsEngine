//! The Text thing (Text part 1, spec `docs/superpowers/specs/2026-11-07-text-thing-design.md`):
//! words in any of the built-in fonts or a loaded one, laid out straight, round a circle, on a wave
//! or along a shape's edge, each letter drawn as a shape wearing the Text's `look.shape` card. A
//! new Text sits on the screen; it has no wall yet (part 3).

pub mod draw;
pub mod looks;
pub mod sim;

pub use draw::{TextDraw, TextNow, text_now};
pub use sim::{TextSim, TextState};

use bpe_core::{Choice, SettingDef, Settings, SlotId, Target, ThingKind};

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

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct TextSettings {
    /// What it says: any number of lines.
    #[setting(label = "Words", group = "Words", unit = "lines", simple)]
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
    #[setting(label = "Spin", group = "Words", range(-360, 360), unit = "deg/s", keyframe)]
    pub spin: f32,
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
        }
    }
}

impl TextSettings {
    /// What the layout reads.
    pub fn layout_spec(&self) -> LayoutSpec<'_> {
        LayoutSpec {
            words: &self.words,
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

const SLOTS: &[SlotId] = &[SlotId::Look];

impl ThingKind for TextKind {
    fn id(&self) -> &'static str {
        KIND
    }
    fn family(&self) -> &'static str {
        "Text"
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
        SLOTS
    }
    fn sticks_to_screen(&self) -> bool {
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
        let defs = TextSettings::defs();
        let keys: Vec<&str> = defs.iter().map(|d| d.key).collect();
        assert_eq!(keys, words.iter().chain(&path).copied().collect::<Vec<_>>());
        for d in defs {
            let want = if words.contains(&d.key) { "Words" } else { "Path" };
            assert_eq!(d.group, want, "{}", d.key);
        }
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

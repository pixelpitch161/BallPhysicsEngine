//! The Point Colour panel: up to eight picked colours, each moving the colours near it (Hue,
//! Saturation, Luminance shifts) as far out as its Range. The picks are fixed slots (`pick1_` ..
//! `pick8_`) so every number keeps its address for keys and pulses; a slot is used or not.

use crate::sound::voice::{full_params, prefixed_defs};
use bpe_core::{Color, GroupOpts, Params, SettingDef, SettingGroup, Settings};

/// How many colours the panel can pick.
pub const PICKS: usize = 8;

/// One pick's settings.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct PointPick {
    /// Whether this slot holds a picked colour.
    #[setting(label = "Used")]
    pub on: bool,
    /// The picked colour: colours near it are the ones moved.
    #[setting(label = "Colour")]
    pub colour: Color,
    /// Turns the colours near the pick's hue round the colour wheel.
    #[setting(label = "Hue", range(-100, 100), keyframe, pulse)]
    pub hue: f32,
    /// Less (down to grey) or more colour in the colours near the pick.
    #[setting(label = "Saturation", range(-100, 100), keyframe, pulse)]
    pub saturation: f32,
    /// Darker or brighter colours near the pick.
    #[setting(label = "Luminance", range(-100, 100), keyframe, pulse)]
    pub luminance: f32,
    /// How far from the picked colour a colour may be and still be moved.
    #[setting(label = "Range", range(0, 100), keyframe, pulse)]
    pub range: f32,
}

impl Default for PointPick {
    fn default() -> Self {
        Self { on: false, colour: Color::rgb(255, 0, 0), hue: 0.0, saturation: 0.0, luminance: 0.0, range: 50.0 }
    }
}

impl SettingGroup for PointPick {
    fn group_defs(o: &GroupOpts, default: &Self) -> Vec<SettingDef> {
        prefixed_defs(o, default)
    }

    fn group_read(o: &GroupOpts, default: &Self, p: &Params) -> Self {
        let mut own = full_params(default);
        for (k, v) in p.iter() {
            if let Some(plain) = k.strip_prefix(o.prefix) {
                own.set(plain, v.clone());
            }
        }
        Self::from_params(&own)
    }

    fn group_write(&self, o: &GroupOpts, default: &Self, p: &mut Params) {
        let (mine, base) = (full_params(self), full_params(default));
        for (k, v) in mine.iter() {
            if base.get(k) != Some(v) {
                p.set(&format!("{}{k}", o.prefix), v.clone());
            }
        }
    }
}

/// The Point Colour panel's settings.
#[derive(Clone, Debug, Default, PartialEq, Settings)]
pub struct PointColourPanel {
    #[setting(paint, prefix = "pick1_", label = "Colour 1", group = "Colour 1")]
    pub pick1: PointPick,
    #[setting(paint, prefix = "pick2_", label = "Colour 2", group = "Colour 2")]
    pub pick2: PointPick,
    #[setting(paint, prefix = "pick3_", label = "Colour 3", group = "Colour 3")]
    pub pick3: PointPick,
    #[setting(paint, prefix = "pick4_", label = "Colour 4", group = "Colour 4")]
    pub pick4: PointPick,
    #[setting(paint, prefix = "pick5_", label = "Colour 5", group = "Colour 5")]
    pub pick5: PointPick,
    #[setting(paint, prefix = "pick6_", label = "Colour 6", group = "Colour 6")]
    pub pick6: PointPick,
    #[setting(paint, prefix = "pick7_", label = "Colour 7", group = "Colour 7")]
    pub pick7: PointPick,
    #[setting(paint, prefix = "pick8_", label = "Colour 8", group = "Colour 8")]
    pub pick8: PointPick,
}

/// Slot `slot`'s (0..8) key prefix: `"pick1_"` for the first.
pub fn pick_prefix(slot: usize) -> String {
    format!("pick{}_", slot + 1)
}

impl PointColourPanel {
    /// The eight slots in order.
    pub fn picks(&self) -> [&PointPick; PICKS] {
        [&self.pick1, &self.pick2, &self.pick3, &self.pick4, &self.pick5, &self.pick6, &self.pick7, &self.pick8]
    }

    /// The first slot not in use.
    pub fn free_slot(&self) -> Option<usize> {
        self.picks().iter().position(|p| !p.on)
    }

    /// The panel as the frame carries it: the used picks, in slot order.
    pub fn gpu(&self) -> bpe_gfx::GradePointColour {
        let n = |v: f32| if v.is_finite() { v } else { 0.0 };
        let linear = |c: Color| [c.r, c.g, c.b].map(|v| bpe_gfx::grade::maths::untone(v as f64 / 255.0) as f32);
        let picks = self
            .picks()
            .iter()
            .filter(|p| p.on)
            .map(|p| bpe_gfx::GradePick {
                colour: linear(p.colour),
                hue: n(p.hue),
                saturation: n(p.saturation),
                luminance: n(p.luminance),
                range: if p.range.is_finite() { p.range } else { 50.0 },
            })
            .collect();
        bpe_gfx::GradePointColour { picks }
    }
}

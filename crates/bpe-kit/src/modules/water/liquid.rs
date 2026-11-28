//! A Water scene's Liquid cards: **Feel**, **Mixing** and **Heat** (its **Look** is in [`super::look`]).
//! Settings today's Liquid thing also has keep its names, ranges and spans.

use bpe_core::{Choice, Settings};

/// How a Liquid flows.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct FeelCard {
    /// 0 runs like water; higher is syrup, then honey, then paste.
    #[setting(label = "Thickness", range(0, 1e9), span(0, 1), simple, keyframe, group = "How it flows")]
    pub thickness: f32,
    /// How strongly it pulls itself into drops and smooth edges.
    #[setting(label = "Surface pull", range(0, 1e9), span(0, 1), keyframe, group = "How it flows")]
    pub surface_pull: f32,
    /// How heavy it is next to other liquids: a heavier one sinks under a lighter one.
    #[setting(label = "Weight", range(0.1, 10), span(0.1, 10), simple, keyframe, group = "How it flows")]
    pub weight: f32,
    /// How much it clings to the course instead of running off.
    #[setting(label = "Stickiness", range(0, 1e9), span(0, 1), keyframe, group = "How it flows")]
    pub stickiness: f32,
}

impl Default for FeelCard {
    fn default() -> Self {
        Self { thickness: 0.02, surface_pull: 0.5, weight: 1.0, stickiness: 0.0 }
    }
}

/// When a Liquid's colours mix with its neighbours'.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum MixesWhen {
    #[default]
    Always,
    OnlyWhenStirred,
    OnlyOnHits,
}

/// A Liquid's Mixing card: how it meets other liquids.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct MixingCard {
    /// How much it keeps apart from other liquids: 1 is oil and water (round drops, a sharp line, no
    /// colours running across).
    #[setting(label = "Keeps apart", range(0, 1), span(0, 1), simple, group = "Meeting other liquids")]
    pub keeps_apart: f32,
    /// How fast colours run together where liquids meet: 0 never, 1 fastest.
    #[setting(label = "Mix speed", range(0, 1), span(0, 1), simple, group = "Meeting other liquids")]
    pub mix_speed: f32,
    #[setting(label = "Mixes when", group = "Meeting other liquids")]
    pub mixes_when: MixesWhen,
}

impl Default for MixingCard {
    fn default() -> Self {
        Self { keeps_apart: 0.0, mix_speed: 0.5, mixes_when: MixesWhen::Always }
    }
}

/// A Liquid's Heat card.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct HeatCard {
    /// How hot new liquid is (0 cold, 1 hot).
    #[setting(label = "Temperature", range(0, 1), span(0, 1), simple, keyframe, group = "Heat")]
    pub temperature: f32,
    /// How fast it loses heat at its surface.
    #[setting(label = "Cools", range(0, 1e9), span(0, 2), unit = "/s", keyframe, group = "Heat")]
    pub cools: f32,
    /// How fast it takes heat from Heaters.
    #[setting(label = "Warms", range(0, 1e9), span(0, 2), unit = "/s", keyframe, group = "Heat")]
    pub warms: f32,
    /// How much thicker it gets as it cools (lava).
    #[setting(label = "Cold thickens it", range(0, 1e9), span(0, 1), keyframe, group = "Heat")]
    pub cold_thickens: f32,
    /// It turns solid when it is cold enough, and melts again when warm.
    #[setting(label = "Freezes", group = "Heat")]
    pub freezes: bool,
    #[setting(label = "Freezes below", when = "freezes", range(0, 1), span(0, 1), keyframe, group = "Heat")]
    pub freezes_below: f32,
    #[setting(label = "Melts above", when = "freezes", range(0, 1), span(0, 1), keyframe, group = "Heat")]
    pub melts_above: f32,
    /// How fast it dries up at its surface (0: never).
    #[setting(label = "Dries up", range(0, 1e9), span(0, 1), unit = "/s", keyframe, group = "Heat")]
    pub dries: f32,
    /// Dried liquid leaves a mark where it was.
    #[setting(label = "Stains", when = "dries", group = "Heat")]
    pub stains: bool,
    #[setting(label = "Stain strength", when = "dries and stains", range(0, 1), span(0, 1), keyframe, group = "Heat")]
    pub stain_strength: f32,
}

impl Default for HeatCard {
    fn default() -> Self {
        let old = crate::things::liquid::LiquidSettings::default();
        Self {
            temperature: old.temperature,
            cools: old.cools,
            warms: old.warms,
            cold_thickens: old.cold_thickens,
            freezes: old.freezes,
            freezes_below: old.freezes_below,
            melts_above: old.melts_above,
            dries: old.dries,
            stains: old.stains,
            stain_strength: old.stain_strength,
        }
    }
}

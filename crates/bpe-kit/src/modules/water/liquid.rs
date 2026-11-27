//! A Water scene's Liquid cards: **Feel**, **Mixing** and **Heat** (its **Look** is in [`super::look`]).
//! Settings today's Liquid thing also has keep its names, ranges and spans.

use bpe_core::{Choice, Color, Settings};

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
    #[setting(label = "Weight", range(0.05, 1e9), span(0.1, 10), simple, keyframe, group = "How it flows")]
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

/// A Liquid's Mixing card.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct MixingCard {
    /// How fast colours run together where liquids meet: 0 never, 1 at once.
    #[setting(label = "Mix speed", range(0, 1), span(0, 1), simple, group = "Colour mixing")]
    pub mix_speed: f32,
    #[setting(label = "Mixes when", group = "Colour mixing")]
    pub mixes_when: MixesWhen,
    /// How fast its colours drift to Fades to (0: never).
    #[setting(label = "Fades", range(0, 1e9), span(0, 1), unit = "/s", group = "Colour mixing")]
    pub fades: f32,
    #[setting(label = "Fades to", when = "fades", group = "Colour mixing")]
    pub fades_to: Color,
    /// Keep the last Seconds of the liquid so it can play backwards.
    #[setting(label = "Remember for Un-mix", group = "Un-mix")]
    pub remember: bool,
    #[setting(label = "Seconds", when = "remember", range(0.1, 60), span(1, 20), unit = "s", group = "Un-mix")]
    pub unmix_seconds: f32,
    /// While on, the liquid plays its remembered seconds backwards (colours un-mix too).
    #[setting(label = "Un-mixing", when = "remember", group = "Un-mix")]
    pub unmixing: bool,
}

impl Default for MixingCard {
    fn default() -> Self {
        Self {
            mix_speed: 0.5,
            mixes_when: MixesWhen::Always,
            fades: 0.0,
            fades_to: Color::WHITE,
            remember: false,
            unmix_seconds: 5.0,
            unmixing: false,
        }
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

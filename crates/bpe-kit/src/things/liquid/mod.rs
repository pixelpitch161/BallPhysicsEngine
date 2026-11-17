//! Liquid: a thing whose drops are a real liquid (liquid spec, part 1): it fills a shape or the
//! scene, pours from spouts, is pushed aside by the balls, carries its colour in every drop and
//! mixes with itself and with other liquids. The drops move on the graphics card
//! (`bpe_gfx::liquid_sim`); every Liquid thing of a scene shares one body of liquid there, held by
//! the first one's state, so liquids poured together meet.
//!
//! Its own settings are what the liquid is like (**Liquid**: thickness, weight, springiness...,
//! Detail and the Area); its cards are **Fill** (the Motion slot: fill at the start, Level, and a
//! list of Pours), **Meets** (the Bounce slot: balls push it, and a list of rules for other liquids)
//! and **Look**. A liquid that pushes balls back (part 2: the Meets card's Pushes balls) steps with
//! the physics, and every other Liquid thing with it; one that doesn't is decoration stepped beside
//! the physics.

pub mod births;
pub mod cards;
pub mod draw;
pub mod feel;
pub mod forces;
pub mod rules;
pub mod sim;
pub mod state;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_balls;
#[cfg(test)]
mod tests_part4;
pub mod walls;

use std::cell::Cell;

use bpe_core::{Choice, SettingDef, Settings, SlotId, ThingKind};

pub use cards::{
    CURRENT, ChangesBall, ColourFrom, CurrentKind, CurrentLayer, DRAIN, DrainLayer, FILL, FillSettings, FillWith,
    Floats, Forces, HEAT, HeatLayer, LOOK, LookSettings, MAGNET, MEETS, MagnetAt, MagnetLayer, Meeting, MeetsSettings,
    MixStyle, MixesWhen, OTHER, OtherLiquid, POUR, PourLayer, RuleStyle,
};
pub use feel::taken_colours;
pub use state::{LiquidState, hash_of, inputs_hash, state_of};

/// The thing kind's id.
pub const KIND: &str = "liquid";

/// How many drops make the liquid: each step finer halves the space between them (about).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum Detail {
    Rough,
    #[default]
    Normal,
    Fine,
    VeryFine,
    Extreme,
}

impl Detail {
    /// The grid's cell, in pixels (two drops a cell each way).
    pub fn cell_px(self) -> f32 {
        match self {
            Detail::Rough => 12.0,
            Detail::Normal => 8.0,
            Detail::Fine => 6.0,
            Detail::VeryFine => 4.0,
            Detail::Extreme => 3.0,
        }
    }

    /// How many drops fill 100 × 100 pixels.
    pub fn drops_per_100px(self) -> u32 {
        (4.0 * (100.0 / self.cell_px()).powi(2)).round() as u32
    }
}

/// How many liquid steps a second.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum Steps {
    #[default]
    Steps240,
    Steps120,
}

impl Steps {
    pub fn per_second(self) -> u32 {
        match self {
            Steps::Steps240 => 240,
            Steps::Steps120 => 120,
        }
    }
}

/// Where the liquid can be: the rectangle of the world its drops live in, fixed at the start.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum AreaFrom {
    /// What the camera sees at the start, grown to hold every Liquid thing's shape.
    #[default]
    ShotAtTheStart,
    /// The Liquid things' shapes alone (what the camera sees when none is in a shape).
    FitTheShapes,
    /// As Shot at the start, with Margin more on every side.
    Custom,
}

/// A Liquid thing's own settings: what the liquid is like.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct LiquidSettings {
    /// How thick it is: water at 0, honey near 1, tar past it.
    #[setting(label = "Thickness", range(0, 1e9), span(0, 1), simple, keyframe, pulse, group = "How it flows")]
    pub thickness: f32,
    /// How heavy it is (water is 1): a lighter liquid sits on top of a heavier one.
    #[setting(label = "Weight", range(0.05, 1e9), span(0.1, 10), simple, keyframe, pulse, group = "How it flows")]
    pub weight: f32,
    /// How much it pulls itself together into beads and round drops (low spreads into sheets).
    #[setting(label = "Surface pull", range(0, 1e9), span(0, 1), keyframe, pulse, group = "How it flows")]
    pub surface_pull: f32,
    /// How firm it is: low is bouncy with small waves, high stiff and still.
    #[setting(label = "Springiness", range(0, 1), span(0, 1), keyframe, pulse, group = "How it flows")]
    pub springiness: f32,
    /// How easily it breaks into droplets when it moves fast.
    #[setting(label = "Spray", range(0, 1), span(0, 1), keyframe, pulse, group = "How it flows")]
    pub spray: f32,
    /// How fast sloshing dies down.
    #[setting(label = "Settles", range(0, 1e9), span(0, 1), keyframe, pulse, group = "How it flows")]
    pub settles: f32,
    /// How hard the scene's gravity pulls it (below 0 it falls upwards).
    #[setting(label = "Gravity", span(-2, 2), unit = "×", keyframe, pulse, group = "How it flows")]
    pub gravity: f32,
    /// How much it holds together in strands and springs back, like slime or jelly.
    #[setting(label = "Stretch", range(0, 1e9), span(0, 1), keyframe, pulse, group = "How it flows")]
    pub stretch: f32,
    /// How far a stretchy liquid pulls (times its length) before it snaps.
    #[setting(
        label = "Tears at",
        when = "stretch",
        range(1, 1e9),
        span(1, 5),
        unit = "×",
        keyframe,
        pulse,
        group = "How it flows"
    )]
    pub tears_at: f32,
    /// How hot new liquid is (0 cold, 1 hot).
    #[setting(label = "Temperature", range(0, 1), span(0, 1), keyframe, pulse, group = "Heat")]
    pub temperature: f32,
    /// How fast it loses heat at its surface.
    #[setting(label = "Cools", range(0, 1e9), span(0, 2), unit = "/s", keyframe, pulse, group = "Heat")]
    pub cools: f32,
    /// How fast it takes heat from Heat places (on the Fill card) and hot balls.
    #[setting(label = "Warms", range(0, 1e9), span(0, 2), unit = "/s", keyframe, pulse, group = "Heat")]
    pub warms: f32,
    /// How much thicker it gets as it cools (lava).
    #[setting(label = "Cold thickens it", range(0, 1e9), span(0, 1), keyframe, pulse, group = "Heat")]
    pub cold_thickens: f32,
    /// It turns solid when it is cold enough, and melts again when warm.
    #[setting(label = "Freezes", group = "Heat")]
    pub freezes: bool,
    #[setting(label = "Freezes below", when = "freezes", range(0, 1), span(0, 1), keyframe, group = "Heat")]
    pub freezes_below: f32,
    #[setting(label = "Melts above", when = "freezes", range(0, 1), span(0, 1), keyframe, group = "Heat")]
    pub melts_above: f32,
    /// How fast it dries up at its surface (0: never).
    #[setting(label = "Dries up", range(0, 1e9), span(0, 1), unit = "/s", keyframe, pulse, group = "Heat")]
    pub dries: f32,
    /// Dried drops leave a mark where they were.
    #[setting(label = "Stains", when = "dries", group = "Heat")]
    pub stains: bool,
    #[setting(label = "Stain strength", when = "dries and stains", range(0, 1), span(0, 1), keyframe, group = "Heat")]
    pub stain_strength: f32,
    /// Keep the last Seconds of the liquid so it can play backwards (the first Liquid thing's is
    /// used for all; it takes video memory).
    #[setting(label = "Remember for Un-mix", group = "Un-mix")]
    pub remember: bool,
    #[setting(label = "Seconds", when = "remember", range(0.1, 60), span(1, 20), unit = "s", group = "Un-mix")]
    pub unmix_seconds: f32,
    /// While on, the whole liquid plays its remembered seconds backwards (colours un-mix too).
    #[setting(label = "Un-mixing", when = "remember", keyframe, group = "Un-mix")]
    pub unmixing: bool,
    /// How many drops make the liquid in the editor (the first Liquid thing's is used for all).
    #[setting(label = "Detail", simple, group = "Detail")]
    pub detail: Detail,
    /// How many drops make it in the exported video.
    #[setting(label = "Export detail", group = "Detail")]
    pub export_detail: Detail,
    /// Export at Export detail even while a liquid pushes balls: they will take different paths
    /// from the preview (off, such an export uses the preview's Detail, so balls go as tuned).
    #[setting(label = "Finer export", group = "Detail")]
    pub finer_export: bool,
    /// How often the liquid moves on (the first Liquid thing's is used for all).
    #[setting(label = "Steps a second", group = "Detail")]
    pub steps: Steps,
    /// Where the liquid can be, fixed at the start (the first Liquid thing's is used for all).
    #[setting(label = "Area", group = "Area")]
    pub area: AreaFrom,
    #[setting(label = "Margin", when = "area=custom", range(0, 1e9), span(0, 2000), unit = "px", group = "Area")]
    pub margin: f32,
}

impl Default for LiquidSettings {
    fn default() -> Self {
        LiquidSettings {
            thickness: 0.05,
            weight: 1.0,
            surface_pull: 0.3,
            springiness: 0.5,
            spray: 0.3,
            settles: 0.2,
            gravity: 1.0,
            stretch: 0.0,
            tears_at: 2.0,
            temperature: 0.5,
            cools: 0.0,
            warms: 1.0,
            cold_thickens: 0.0,
            freezes: false,
            freezes_below: 0.2,
            melts_above: 0.3,
            dries: 0.0,
            stains: false,
            stain_strength: 0.5,
            remember: false,
            unmix_seconds: 5.0,
            unmixing: false,
            detail: Detail::Normal,
            export_detail: Detail::Fine,
            finer_export: false,
            steps: Steps::Steps240,
            area: AreaFrom::ShotAtTheStart,
            margin: 100.0,
        }
    }
}

/// `x` when it is a number, else `or`.
pub(crate) fn num(x: f32, or: f32) -> f32 {
    if x.is_finite() { x } else { or }
}

impl LiquidSettings {
    /// The settings in `p`, any number that isn't one read as its default.
    pub fn read(p: &bpe_core::Params) -> LiquidSettings {
        let s = LiquidSettings::from_params(p);
        let d = LiquidSettings::default();
        LiquidSettings {
            thickness: num(s.thickness, d.thickness).max(0.0),
            weight: num(s.weight, d.weight).max(0.05),
            surface_pull: num(s.surface_pull, d.surface_pull).max(0.0),
            springiness: num(s.springiness, d.springiness).clamp(0.0, 1.0),
            spray: num(s.spray, d.spray).clamp(0.0, 1.0),
            settles: num(s.settles, d.settles).max(0.0),
            gravity: num(s.gravity, d.gravity),
            stretch: num(s.stretch, 0.0).max(0.0),
            tears_at: num(s.tears_at, d.tears_at).max(1.0),
            temperature: num(s.temperature, d.temperature).clamp(0.0, 1.0),
            cools: num(s.cools, 0.0).max(0.0),
            warms: num(s.warms, d.warms).max(0.0),
            cold_thickens: num(s.cold_thickens, 0.0).max(0.0),
            freezes_below: num(s.freezes_below, d.freezes_below).clamp(0.0, 1.0),
            melts_above: num(s.melts_above, d.melts_above).clamp(0.0, 1.0),
            dries: num(s.dries, 0.0).max(0.0),
            stain_strength: num(s.stain_strength, d.stain_strength).clamp(0.0, 1.0),
            unmix_seconds: num(s.unmix_seconds, d.unmix_seconds).clamp(0.1, 60.0),
            margin: num(s.margin, d.margin).max(0.0),
            ..s
        }
    }
}

thread_local! {
    /// Whether worlds built on this thread are for an export (they use Export detail).
    static EXPORTING: Cell<bool> = const { Cell::new(false) };
}

/// Worlds built on this thread until the guard is dropped are for an export: their Liquid things
/// use Export detail.
pub fn exporting_for_now() -> ExportGuard {
    let before = EXPORTING.with(|e| e.replace(true));
    ExportGuard { before }
}

/// What [`exporting_for_now`] gives.
pub struct ExportGuard {
    before: bool,
}

impl Drop for ExportGuard {
    fn drop(&mut self) {
        EXPORTING.with(|e| e.set(self.before));
    }
}

/// Whether worlds built on this thread are for an export.
pub fn exporting() -> bool {
    EXPORTING.with(|e| e.get())
}

pub struct LiquidKind;

impl ThingKind for LiquidKind {
    fn id(&self) -> &'static str {
        KIND
    }
    fn name(&self) -> &'static str {
        "Liquid"
    }
    fn icon(&self) -> &'static str {
        "drop"
    }
    fn help(&self) -> &'static str {
        "A real liquid: it fills a shape or the scene, pours, splashes and mixes its colours. Inside a shape it fills \
         the shape."
    }
    fn settings(&self) -> &'static [SettingDef] {
        LiquidSettings::defs()
    }
    fn slots(&self) -> &'static [SlotId] {
        &[SlotId::Motion, SlotId::Bounce, SlotId::Look]
    }
    fn default_slots(&self) -> &'static [SlotId] {
        &[SlotId::Motion, SlotId::Bounce, SlotId::Look]
    }
    fn family(&self) -> &'static str {
        "Liquids"
    }
    fn slot_label(&self, slot: SlotId) -> &'static str {
        match slot {
            SlotId::Motion => "Fill",
            SlotId::Bounce => "Meets",
            other => other.name(),
        }
    }
    fn can_stick_to_screen(&self) -> bool {
        false
    }
}

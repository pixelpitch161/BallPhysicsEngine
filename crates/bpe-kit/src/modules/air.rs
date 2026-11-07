//! The scene's **Air** card (particles part 2): the wind the air settles to, how fast it calms
//! down, how fine its grid is, how balls stir it, how heat lifts it and whether shapes block it.
//! Only Particles things that follow the air (or heat it) read it ([`crate::air`]); the balls never
//! do, so editing it restarts only the particle playback.

use bpe_core::{Choice, ModuleKind, ResolvedThing, SettingDef, Settings, SlotId};

use crate::air::AirSpec;

/// The card kind.
pub const AIR: &str = "air";

/// How fine the air's grid is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Resolution {
    /// 32 px cells: quick, broad currents.
    Coarse,
    /// 16 px cells.
    #[default]
    Normal,
    /// 8 px cells: small eddies, slower.
    Fine,
}

impl Resolution {
    /// A cell's side, px.
    pub fn cell(self) -> f32 {
        match self {
            Resolution::Coarse => 32.0,
            Resolution::Normal => 16.0,
            Resolution::Fine => 8.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct AirSettings {
    /// The wind the air settles back to.
    #[setting(label = "Wind", span(0, 1000), unit = "px/s", simple, keyframe, pulse, group = "Wind")]
    pub wind_speed: f32,
    /// The way it blows, in degrees: 0 to the right, 90 down.
    #[setting(label = "Wind direction", span(-180, 180), unit = "°", keyframe, group = "Wind")]
    pub wind_direction: f32,
    /// How much stronger and weaker it blows by turns.
    #[setting(label = "Gusts", span(0, 1), keyframe, group = "Wind")]
    pub gusts: f32,
    #[setting(
        label = "Gust every",
        when = "gusts",
        range(0.01, 1e9),
        span(0.1, 10),
        unit = "s",
        keyframe,
        group = "Wind"
    )]
    pub gust_every: f32,
    /// The distance between gusts as they travel across: 0 gusts everywhere at once.
    #[setting(
        label = "Gust waves",
        when = "gusts",
        range(0, 1e9),
        span(0, 2000),
        unit = "px",
        keyframe,
        group = "Wind"
    )]
    pub waves: f32,

    /// How quickly stirred air settles back to the wind.
    #[setting(label = "Calms down", range(0, 1e9), span(0, 10), unit = "per s", simple, keyframe, group = "Air")]
    pub calm: f32,
    /// How much the air's eddies keep turning instead of smoothing out.
    #[setting(label = "Keeps its swirls", span(0, 1), keyframe, group = "Air")]
    pub swirls: f32,
    #[setting(label = "Resolution", group = "Air")]
    pub resolution: Resolution,
    /// How far past the picture's edge the air is worked out.
    #[setting(label = "Margin", range(0, 1e9), span(0, 800), unit = "px", group = "Air")]
    pub margin: f32,

    /// How strongly moving balls take the air along with them.
    #[setting(label = "Balls stir", span(0, 1), simple, keyframe, pulse, group = "Balls and shapes")]
    pub stir: f32,
    /// How far round a ball it stirs the air, in its radii.
    #[setting(
        label = "Stir reach",
        when = "stir",
        range(0, 1e9),
        span(1, 6),
        unit = "×",
        keyframe,
        group = "Balls and shapes"
    )]
    pub stir_reach: f32,
    /// Shapes are solid: the air flows round them.
    #[setting(label = "Shapes block the air", group = "Balls and shapes")]
    pub shapes_block: bool,

    /// How hard warm air rises (heat comes from Rise forces that heat the air).
    #[setting(label = "Heat rises", span(0, 500), unit = "px/s²", keyframe, group = "Heat")]
    pub rises: f32,
    #[setting(label = "Heat fades", range(0, 1e9), span(0, 10), unit = "per s", keyframe, group = "Heat")]
    pub fades: f32,
}

impl Default for AirSettings {
    fn default() -> Self {
        let s = AirSpec::default();
        AirSettings {
            wind_speed: s.wind_speed,
            wind_direction: s.wind_direction,
            gusts: s.gusts,
            gust_every: s.gust_every,
            waves: s.waves,
            calm: s.calm,
            swirls: s.swirls,
            resolution: Resolution::Normal,
            margin: s.margin,
            stir: s.stir,
            stir_reach: s.stir_reach,
            shapes_block: s.shapes_block,
            rises: s.rises,
            fades: s.fades,
        }
    }
}

impl AirSettings {
    /// The settings as the air's solver uses them.
    pub fn spec(&self) -> AirSpec {
        AirSpec {
            wind_speed: self.wind_speed,
            wind_direction: self.wind_direction,
            gusts: self.gusts,
            gust_every: self.gust_every,
            waves: self.waves,
            calm: self.calm,
            cell: self.resolution.cell(),
            stir: self.stir,
            stir_reach: self.stir_reach,
            rises: self.rises,
            fades: self.fades,
            swirls: self.swirls,
            shapes_block: self.shapes_block,
            margin: self.margin,
        }
    }
}

/// The scene's Air card as the solver uses it, when it has one.
pub fn spec_of(root: &ResolvedThing) -> Option<AirSpec> {
    let card = root.slots.get(&SlotId::Air)?;
    (card.kind == AIR).then(|| AirSettings::from_params(&card.params).spec())
}

pub struct AirKind;

impl ModuleKind for AirKind {
    fn id(&self) -> &'static str {
        AIR
    }
    fn name(&self) -> &'static str {
        "Air"
    }
    fn help(&self) -> &'static str {
        "The air particles can follow: a wind it settles to, stirred by the balls, lifted by heat and blocked by shapes."
    }
    fn slot(&self) -> SlotId {
        SlotId::Air
    }
    fn settings(&self) -> &'static [SettingDef] {
        AirSettings::defs()
    }
    fn fits(&self, thing_kind: &str) -> bool {
        thing_kind == "scene"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_starts_as_the_solver_does() {
        let mut spec = AirSpec::default();
        assert_eq!(AirSettings::default().spec(), spec);
        let fine = AirSettings { resolution: Resolution::Fine, ..AirSettings::default() };
        spec.cell = 8.0;
        assert_eq!(fine.spec(), spec);
    }
}

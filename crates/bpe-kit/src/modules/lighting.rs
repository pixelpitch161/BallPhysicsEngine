//! The scene's Lighting card (`scene.lighting`, lighting spec part 1a): with it the scene is lit
//! by its Light things, with soft shadows; without it the picture is exactly as it always was.

use bpe_core::{Choice, Color, KindId, LightingScene, ModuleKind, SettingDef, Settings, SlotId, Target};

use super::light_scenes::{SceneOrder, ScenesPlay};
use bpe_gfx::LightingGpu;

/// The card kind's id.
pub const LIGHTING: &str = "scene.lighting";

pub struct LightingKind;

/// How the scene is laid out for light.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Depth {
    /// The flat picture, lit from lamps hanging above it.
    #[default]
    Flat2D,
}

/// How shadows look.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ShadowStyle {
    /// Soft edges that grow softer away from what casts them.
    #[default]
    Soft,
    /// Crisp edges.
    Hard,
    /// Crisp edges and the light in flat bands, like a cartoon.
    Toon,
    /// No shadows at all.
    Off,
}

impl ShadowStyle {
    pub fn code(self) -> f32 {
        match self {
            Self::Soft => 0.0,
            Self::Hard => 1.0,
            Self::Toon => 2.0,
            Self::Off => 3.0,
        }
    }
}

/// How hard the lighting works: higher looks better and costs more.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Quality {
    Draft,
    #[default]
    Good,
    High,
    Ultra,
}

impl Quality {
    pub fn code(self) -> f32 {
        match self {
            Self::Draft => 0.0,
            Self::Good => 1.0,
            Self::High => 2.0,
            Self::Ultra => 3.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct LightingSettings {
    /// How the scene is laid out for light.
    #[setting(label = "Depth", group = "Room")]
    pub depth: Depth,
    /// The light everywhere, even where no lamp reaches.
    #[setting(label = "Ambient colour", keyframe, group = "Room")]
    pub ambient_colour: Color,
    /// Whether the light everywhere is on (the Lights tab's M on Ambient).
    #[setting(label = "Ambient on", group = "Room")]
    pub ambient_on: bool,
    /// How bright the light everywhere is: 0 is darkness but for the lamps, 1 is as unlit.
    #[setting(label = "Ambient", when = "ambient_on", span(0, 1), simple, keyframe, group = "Room")]
    pub ambient: f32,
    /// How much the background takes the light: 0 leaves it as it is.
    #[setting(label = "Background takes light", span(0, 1), keyframe, group = "Room")]
    pub background_lit: f32,
    /// How much light lit things that block light bounce onto what is round them.
    #[setting(label = "Bounce light", span(0, 2), keyframe, group = "Room")]
    pub bounce: f32,
    /// How far the light of glowing things (and bounced light) reaches.
    #[setting(label = "Glow reach", range(0, 1e9), span(0, 4000), unit = "px", keyframe, group = "Room")]
    pub glow_reach: f32,
    /// How much the lamps and glowing things spill into a soft bloom (the lamps show as bulbs).
    #[setting(label = "Light blooms", span(0, 2), keyframe, group = "Room")]
    pub light_blooms: f32,
    /// How shadows look.
    #[setting(label = "Style", group = "Shadows")]
    pub shadow_style: ShadowStyle,
    /// How soft every shadow is, on top of each lamp's size.
    #[setting(label = "Softness", when = "shadow_style=soft", span(0, 4), unit = "×", keyframe, group = "Shadows")]
    pub shadow_softness: f32,
    /// How far behind what casts it a shadow reaches.
    #[setting(label = "Length", when = "shadow_style!=off", span(0, 4000), unit = "px", keyframe, group = "Shadows")]
    pub shadow_length: f32,
    /// How much light dims close to things that block it.
    #[setting(label = "Contact shading", span(0, 1), keyframe, group = "Shadows")]
    pub contact: f32,
    /// How far from a blocker contact shading reaches.
    #[setting(
        label = "Contact reach",
        when = "contact",
        range(0, 1e9),
        span(0, 200),
        unit = "px",
        keyframe,
        group = "Shadows"
    )]
    pub contact_reach: f32,
    /// How many bands of light a toon picture has.
    #[setting(label = "Toon steps", when = "shadow_style=toon", range(1, 1e9), span(1, 8), group = "Shadows")]
    pub toon_steps: u32,
    /// How much the air shows the light: beams from the lamps, halos round glowing things.
    #[setting(label = "Haze", span(0, 2), keyframe, group = "Air")]
    pub haze: f32,
    /// The colour the haze takes the light in.
    #[setting(label = "Haze colour", when = "haze", keyframe, group = "Air")]
    pub haze_colour: Color,
    /// How much the haze veils what is behind it.
    #[setting(label = "Haze dims", when = "haze", span(0, 1), keyframe, group = "Air")]
    pub haze_dims: f32,
    /// How hard the lighting works, while editing and in the video alike.
    #[setting(label = "Quality", group = "Quality")]
    pub quality: Quality,
    /// What shows the lighting scenes: Off leaves every light as it is.
    #[setting(label = "Scenes play", group = "Scenes")]
    pub scenes_play: ScenesPlay,
    /// The scene showing: 0 is the lights' own settings. Its keys on the timeline switch scenes.
    #[setting(
        label = "Show scene",
        when = "scenes_play=on_the_timeline",
        range(0, 1e9),
        span(0, 8),
        keyframe,
        hold,
        group = "Scenes"
    )]
    pub show_scene: i32,
    /// How many bars each scene shows for.
    #[setting(
        label = "Every",
        when = "scenes_play=in_turn",
        range(0.001, 1e9),
        span(0.25, 16),
        unit = "bars",
        keyframe,
        group = "Scenes"
    )]
    pub scene_every: f32,
    /// Whose hits move to the next scene.
    #[setting(label = "Heard from", when = "scenes_play=on_hits", group = "Scenes")]
    pub scene_heard: Target,
    /// Hits closer together than this count once.
    #[setting(
        label = "Least gap",
        when = "scenes_play=on_hits",
        range(0, 1e9),
        span(0, 2),
        unit = "s",
        keyframe,
        group = "Scenes"
    )]
    pub scene_gap: f32,
    /// The order scenes come in.
    #[setting(label = "Order", when = "scenes_play=in_turn or scenes_play=on_hits", group = "Scenes")]
    pub scene_order: SceneOrder,
    /// Picks another random run of scenes.
    #[setting(label = "Seed", when = "scene_order=random", span(0, 100), group = "Scenes")]
    pub scene_seed: u32,
    /// How long one scene takes to fade into the next: 0 switches at once.
    #[setting(
        label = "Crossfade",
        when = "scenes_play!=off",
        range(0, 1e9),
        span(0, 4),
        unit = "s",
        keyframe,
        group = "Scenes"
    )]
    pub scene_fade: f32,
    /// The lighting scenes: each the settings every light had when it was saved.
    #[setting(label = "Scenes", group = "Scenes")]
    pub scenes: Vec<LightingScene>,
}

impl Default for LightingSettings {
    fn default() -> Self {
        Self {
            depth: Depth::Flat2D,
            ambient_colour: Color::WHITE,
            ambient_on: true,
            ambient: 0.25,
            background_lit: 1.0,
            bounce: 0.0,
            glow_reach: 2000.0,
            light_blooms: 0.0,
            haze: 0.0,
            haze_colour: Color::WHITE,
            haze_dims: 0.0,
            shadow_style: ShadowStyle::Soft,
            shadow_softness: 1.0,
            shadow_length: 2400.0,
            contact: 0.0,
            contact_reach: 40.0,
            toon_steps: 3,
            quality: Quality::Good,
            scenes_play: ScenesPlay::Off,
            show_scene: 1,
            scene_every: 4.0,
            scene_heard: Target::Kind(KindId::from("balls")),
            scene_gap: 0.25,
            scene_order: SceneOrder::InOrder,
            scene_seed: 1,
            scene_fade: 0.5,
            scenes: Vec::new(),
        }
    }
}

impl LightingSettings {
    /// The card as the renderer reads it (the background's own light is the scene settings',
    /// filled in by the scene's draw).
    pub fn gpu(&self) -> LightingGpu {
        let a = self.ambient_colour.to_linear();
        let ambient = if self.ambient_on { self.ambient } else { 0.0 };
        let h = self.haze_colour.to_linear();
        let finite = |v: f32| if v.is_finite() { v } else { 0.0 };
        let haze = finite(self.haze);
        LightingGpu {
            ambient: [a[0] * ambient, a[1] * ambient, a[2] * ambient, self.background_lit],
            shadows: [self.shadow_style.code(), self.shadow_softness, self.shadow_length, self.contact],
            more: [self.contact_reach, self.toon_steps.max(1) as f32, self.quality.code(), 0.0],
            glow: [finite(self.bounce), finite(self.glow_reach).max(0.0), finite(self.light_blooms), 0.0],
            air: [h[0] * haze, h[1] * haze, h[2] * haze, if haze != 0.0 { finite(self.haze_dims) } else { 0.0 }],
            background: [[0.0; 4]; 2],
        }
    }
}

impl ModuleKind for LightingKind {
    fn id(&self) -> &'static str {
        LIGHTING
    }
    fn name(&self) -> &'static str {
        "Lighting"
    }
    fn help(&self) -> &'static str {
        "Lights the scene with its Light things: ambient light, shadows and how hard the lighting works."
    }
    fn slot(&self) -> SlotId {
        SlotId::Lighting
    }
    fn settings(&self) -> &'static [SettingDef] {
        LightingSettings::defs()
    }
    fn fits(&self, thing_kind: &str) -> bool {
        thing_kind == "scene"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ambient_off_sends_no_ambient_and_keeps_its_strength() {
        let on = LightingSettings { ambient: 0.4, ..LightingSettings::default() };
        assert!((on.gpu().ambient[1] - 0.4).abs() < 1e-6);
        let off = LightingSettings { ambient_on: false, ..on.clone() };
        assert_eq!(&off.gpu().ambient[..3], &[0.0; 3]);
        assert_eq!(off.ambient, 0.4, "the strength is kept for when it is on again");
    }
}

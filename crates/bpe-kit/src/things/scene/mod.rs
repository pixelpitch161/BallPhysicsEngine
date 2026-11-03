//! Scene settings: the root of every scene. Its settings are the video, the world and the
//! picture, and its effects slot acts on the whole scene; nothing under it inherits it.

pub mod draw;
pub mod sim;

pub use sim::SceneSim;

use crate::paint::Paint;
use bpe_core::{Color, Gradient, SettingDef, Settings, SlotId, ThingKind};
use glam::Vec2;

pub struct SceneKind;

/// The root's inspector cards, in order. Every scene setting's `group` is one of these.
pub const ROOT_CARDS: [&str; 4] = ["Video", "World", "Background", "Picture"];

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct SceneSettings {
    /// The video's width in pixels.
    #[setting(label = "Width", range(2, 7680), unit = "px", group = "Video")]
    pub width: u32,
    /// The video's height in pixels.
    #[setting(label = "Height", range(2, 7680), unit = "px", group = "Video")]
    pub height: u32,
    /// Frames per second.
    #[setting(label = "Frame rate", range(1, 240), unit = "fps", group = "Video")]
    pub fps: u32,
    /// How long the video runs.
    #[setting(label = "Length", range(1, 600), unit = "s", simple, group = "Video")]
    pub duration: f32,
    /// How hard everything is pulled, and which way. Down is positive y.
    #[setting(label = "Gravity", unit = "px/s²", simple, keyframe, group = "World")]
    pub gravity: Vec2,
    /// How many times a second the physics moves everything.
    #[setting(label = "Physics rate", range(60, 2000), unit = "per s", group = "World")]
    pub steps_per_second: u32,
    /// Where the scene's random numbers start. Change it for a different clip from the same
    /// settings.
    #[setting(label = "Seed", range(0, 1000000), group = "World", random)]
    pub seed: u32,
    /// The colours behind everything, at any angle or spreading from a centre.
    #[setting(
        paint,
        label = "Colours",
        prefix = "background_",
        follows = "place|time",
        screen_only,
        simple,
        no_vary,
        group = "Background"
    )]
    pub background: Paint,
    /// Keep the background still on the screen while the camera moves; off paints it across
    /// the world instead.
    #[setting(label = "Background stays on screen", group = "Background")]
    pub background_on_screen: bool,
    /// How much bright things bleed light around them.
    #[setting(label = "Bloom", range(0, 1), simple, keyframe, random, pulse, group = "Picture")]
    pub bloom: f32,
    /// How bright the whole picture is.
    #[setting(label = "Exposure", range(0.1, 4), keyframe, pulse, group = "Picture")]
    pub exposure: f32,
    /// Makes the finished picture lighter or darker.
    #[setting(label = "Brightness", range(-1, 1), group = "Picture", keyframe, pulse)]
    pub grade_brightness: f32,
    /// Spreads the colours apart from the middle grey, or pulls them together.
    #[setting(label = "Contrast", range(-1, 1), group = "Picture", keyframe, pulse)]
    pub grade_contrast: f32,
    /// More colourful, or towards grey at -1.
    #[setting(label = "Saturation", range(-1, 1), group = "Picture", keyframe, pulse)]
    pub grade_saturation: f32,
    /// Warms the picture towards red, or cools it towards blue.
    #[setting(label = "Warm / cool", range(-1, 1), group = "Picture", keyframe, pulse)]
    pub grade_warmth: f32,
    /// Tints the picture towards magenta, or towards green.
    #[setting(label = "Tint", range(-1, 1), group = "Picture", keyframe, pulse)]
    pub grade_tint: f32,
    /// Darkens the corners of the picture.
    #[setting(label = "Vignette", range(0, 1), group = "Picture", keyframe, pulse)]
    pub grade_vignette: f32,
    /// How far out from the middle the vignette starts.
    #[setting(label = "Vignette size", range(0, 1), group = "Picture", keyframe, pulse)]
    pub grade_vignette_size: f32,
    /// Recolours the picture by its brightness: dark parts take the first colours, light parts
    /// the last.
    #[setting(label = "Gradient map", group = "Picture", keyframe)]
    pub grade_map: Gradient,
    /// How much of the gradient map's colour replaces the picture's own.
    #[setting(label = "Gradient map amount", range(0, 1), group = "Picture", keyframe, pulse)]
    pub grade_map_amount: f32,
}

impl Default for SceneSettings {
    fn default() -> Self {
        Self {
            duration: 20.0,
            gravity: Vec2::new(0.0, 500.0),
            background: Paint {
                colours: Gradient::even(&[Color::rgb(0x02, 0x02, 0x2a), Color::rgb(0x01, 0x00, 0x06)]),
                ..Paint::default()
            },
            bloom: 0.15,
            width: 1080,
            height: 1920,
            fps: 60,
            steps_per_second: 240,
            exposure: 1.0,
            grade_brightness: 0.0,
            grade_contrast: 0.0,
            grade_saturation: 0.0,
            grade_warmth: 0.0,
            grade_tint: 0.0,
            grade_vignette: 0.0,
            grade_vignette_size: 0.5,
            grade_map: Gradient::even(&[Color::rgb(0, 0, 0), Color::WHITE]),
            grade_map_amount: 0.0,
            seed: 1,
            background_on_screen: true,
        }
    }
}

impl SceneSettings {
    /// The colour grade these settings run over the finished picture.
    pub fn grade(&self) -> bpe_gfx::Grade {
        bpe_gfx::Grade {
            brightness: self.grade_brightness,
            contrast: self.grade_contrast,
            saturation: self.grade_saturation,
            warmth: self.grade_warmth,
            tint: self.grade_tint,
            vignette: self.grade_vignette,
            vignette_size: self.grade_vignette_size,
            map: self.grade_map.clone(),
            map_amount: self.grade_map_amount,
        }
    }
}

impl ThingKind for SceneKind {
    fn id(&self) -> &'static str {
        "scene"
    }
    fn name(&self) -> &'static str {
        "Scene settings"
    }
    fn icon(&self) -> &'static str {
        "scene"
    }
    fn help(&self) -> &'static str {
        "The video, the world and the picture, plus effects for the whole scene."
    }
    fn settings(&self) -> &'static [SettingDef] {
        SceneSettings::defs()
    }
    fn slots(&self) -> &'static [SlotId] {
        &[SlotId::Shader]
    }
    fn default_slots(&self) -> &'static [SlotId] {
        &[]
    }
    fn passes_down(&self) -> bool {
        false
    }
    fn slot_label(&self, slot: SlotId) -> &'static str {
        match slot {
            SlotId::Shader => "Effects",
            other => other.name(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels_in(card: &str) -> Vec<&'static str> {
        SceneSettings::defs().iter().filter(|d| d.group == card).map(|d| d.label).collect()
    }

    #[test]
    fn every_root_setting_is_on_a_card() {
        for d in SceneSettings::defs() {
            assert!(ROOT_CARDS.contains(&d.group), "{} has group {:?}", d.key, d.group);
        }
    }

    #[test]
    fn root_cards_hold_the_spec_rows() {
        assert_eq!(labels_in("Video"), ["Width", "Height", "Frame rate", "Length"]);
        assert_eq!(labels_in("World"), ["Gravity", "Physics rate", "Seed"]);
        assert_eq!(
            labels_in("Background"),
            [
                "Colours",
                "Colours follow",
                "Gradient",
                "Direction",
                "Centre",
                "Colours end at",
                "Beyond the last colour",
                "Scroll",
                "Take hit colour",
                "Step time",
                "Background stays on screen"
            ]
        );
        assert_eq!(
            labels_in("Picture"),
            [
                "Bloom",
                "Exposure",
                "Brightness",
                "Contrast",
                "Saturation",
                "Warm / cool",
                "Tint",
                "Vignette",
                "Vignette size",
                "Gradient map",
                "Gradient map amount"
            ]
        );
    }

    #[test]
    fn scene_settings_keep_their_effects_slot_to_themselves() {
        let k = SceneKind;
        assert_eq!(k.name(), "Scene settings");
        assert_eq!(k.slots(), &[SlotId::Shader]);
        assert!(k.default_slots().is_empty() && !k.passes_down());
        assert_eq!(k.slot_label(SlotId::Shader), "Effects");
        assert!(k.accepts(SlotId::Shader, "shader.echo"));
    }

    #[test]
    fn the_background_is_one_paint_that_follows_place_or_time_on_the_screen() {
        let defs = SceneSettings::defs();
        let keys: Vec<_> = defs.iter().map(|d| d.key).collect();
        for gone in ["background_top", "background_bottom", "background_space"] {
            assert!(!keys.contains(&gone), "{gone}");
        }
        assert_eq!(keys.iter().filter(|k| **k == "background_on_screen").count(), 1);
        for key in ["background_colours", "background_follow", "background_angle", "background_span"] {
            assert!(keys.contains(&key), "{key}");
        }
        let follow = defs.iter().find(|d| d.key == "background_follow").unwrap();
        assert_eq!(follow.ty, bpe_core::ValueType::Choice(&["place", "time"]));
        let d = SceneSettings::default().background;
        assert_eq!((d.angle, d.span, d.centre), (90.0, 1920.0, Vec2::ZERO));
        assert_eq!(d.colours.stops.len(), 2);
    }

    #[test]
    fn the_grade_is_nine_keyframed_settings_on_the_picture_card() {
        let defs = SceneSettings::defs();
        let keys = [
            ("grade_brightness", "Brightness"),
            ("grade_contrast", "Contrast"),
            ("grade_saturation", "Saturation"),
            ("grade_warmth", "Warm / cool"),
            ("grade_tint", "Tint"),
            ("grade_vignette", "Vignette"),
            ("grade_vignette_size", "Vignette size"),
            ("grade_map", "Gradient map"),
            ("grade_map_amount", "Gradient map amount"),
        ];
        for (key, label) in keys {
            let d = defs.iter().find(|d| d.key == key).unwrap_or_else(|| panic!("{key} missing"));
            assert_eq!((d.label, d.group), (label, "Picture"), "{key}");
            assert!(d.flags.keyframe, "{key}");
            assert_eq!(d.flags.pulse, key != "grade_map", "{key}");
        }
        assert_eq!(defs.iter().filter(|d| d.group == "Picture" && d.key.starts_with("grade_")).count(), 9);
        let map = defs.iter().find(|d| d.key == "grade_map").unwrap();
        assert_eq!(map.ty, bpe_core::ValueType::Gradient);
        let range = |key: &str| defs.iter().find(|d| d.key == key).unwrap().range;
        for key in ["grade_brightness", "grade_contrast", "grade_saturation", "grade_warmth", "grade_tint"] {
            assert_eq!(range(key), Some((-1.0, 1.0)), "{key}");
        }
        for key in ["grade_vignette", "grade_vignette_size", "grade_map_amount"] {
            assert_eq!(range(key), Some((0.0, 1.0)), "{key}");
        }
        let d = SceneSettings::default();
        assert_eq!(d.grade(), bpe_gfx::Grade::default());
    }
}

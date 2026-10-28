//! How one group of balls looks: shape, fill pattern, outline, glow, trail and motion effects.
//!
//! A spawner without a style draws its balls the classic way, as small shiny spheres that
//! follow the scene's Look settings (the reference reel's look). A spawner with a style draws
//! its balls with these settings instead, and ignores the scene's ball glow and trail settings.
//! Styles only change the picture: physics always treats a ball as a circle of its radius.

use crate::Color;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BallStyle {
    pub shape: BallShape,
    /// How rounded a shape's corners are, 0 (sharp) to 1 (very round). Ignored for circles.
    pub roundness: f32,
    pub fill: BallFill,
    /// The pattern's or gradient's other colour. The ball's own colour is the first.
    pub second_color: Color,
    /// How many stripes, rings, dots or slices fit across the ball.
    pub pattern_size: f32,
    /// Starting angle of the shape and pattern, in degrees.
    pub turn: f32,
    /// Degrees per second the shape and pattern turn.
    pub spin: f32,
    /// Turns the shape to point the way the ball is going.
    pub face_motion: bool,
    /// Outline width in pixels, drawn just inside the edge. 0 means none.
    pub outline: f32,
    pub outline_color: Color,
    /// Brightness of the soft halo around the ball. 0 means none.
    pub glow: f32,
    /// How much the halo reaches out, as a fraction of the ball's radius.
    pub glow_size: f32,
    /// 0 is invisible, 1 is solid.
    pub opacity: f32,
    /// How much fast balls stretch along their path, 0 to 1.
    pub stretch: f32,
    /// Full turns around the colour wheel per second, so the ball cycles through the rainbow.
    pub rainbow: f32,
    /// How far back the ball's streak reaches, in seconds. 0 means no trail.
    pub trail_seconds: f32,
    /// Streak width at the ball, as a fraction of the ball's diameter.
    pub trail_width: f32,
    /// Streak brightness at the ball, 0 to 1.
    pub trail_opacity: f32,
    pub trail_color: TrailColor,
}

/// The outline a ball is drawn with. Physics still treats every ball as a circle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BallShape {
    #[default]
    Circle,
    Square,
    Triangle,
    Diamond,
    Hexagon,
    Star,
    Heart,
    /// A circle with a hole in the middle.
    Donut,
}

/// What fills a ball's shape.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BallFill {
    /// One flat colour.
    #[default]
    Solid,
    /// Lit like a small glossy sphere, the reference reel's ball.
    Shiny,
    /// The ball's colour in the middle, fading to the second colour at the edge.
    Radial,
    /// The ball's colour at the top, fading to the second colour at the bottom.
    Linear,
    Stripes,
    Rings,
    Dots,
    Checker,
    /// Two halves, like a pool ball or a pokéball.
    Halves,
    /// Slices around the middle, like a beach ball.
    Slices,
    Swirl,
    /// A smiley face in the second colour.
    Face,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrailColor {
    /// The ball's own colour.
    #[default]
    Ball,
    Second,
    Outline,
}

impl Default for BallStyle {
    /// A plain ball in one solid colour: no outline, glow or trail.
    fn default() -> Self {
        Self {
            shape: BallShape::Circle,
            roundness: 0.15,
            fill: BallFill::Solid,
            second_color: Color::WHITE,
            pattern_size: 4.0,
            turn: 0.0,
            spin: 0.0,
            face_motion: false,
            outline: 0.0,
            outline_color: Color::WHITE,
            glow: 0.0,
            glow_size: 0.8,
            opacity: 1.0,
            stretch: 0.0,
            rainbow: 0.0,
            trail_seconds: 0.0,
            trail_width: 0.5,
            trail_opacity: 0.5,
            trail_color: TrailColor::Ball,
        }
    }
}

/// A named starting point for a ball's look, picked from a list in the editor.
pub struct StylePreset {
    pub name: &'static str,
    pub style: BallStyle,
}

impl BallStyle {
    /// Ready-made looks to start from. The first is the plain default.
    pub fn presets() -> Vec<StylePreset> {
        let base = BallStyle::default();
        let preset = |name, style| StylePreset { name, style };
        vec![
            preset("Plain", base.clone()),
            preset("Shiny marble", BallStyle { fill: BallFill::Shiny, glow: 0.12, ..base.clone() }),
            preset(
                "Neon ring",
                BallStyle {
                    fill: BallFill::Radial,
                    second_color: Color::rgb(0x08, 0x08, 0x10),
                    outline: 3.0,
                    outline_color: Color::rgb(0xff, 0xff, 0xff),
                    glow: 0.35,
                    glow_size: 0.5,
                    ..base.clone()
                },
            ),
            preset(
                "Comet",
                BallStyle {
                    glow: 0.5,
                    stretch: 0.4,
                    trail_seconds: 0.6,
                    trail_width: 0.8,
                    trail_opacity: 0.7,
                    ..base.clone()
                },
            ),
            preset(
                "Beach ball",
                BallStyle {
                    fill: BallFill::Slices,
                    second_color: Color::WHITE,
                    pattern_size: 6.0,
                    spin: 90.0,
                    outline: 1.5,
                    outline_color: Color::rgb(0x20, 0x20, 0x20),
                    ..base.clone()
                },
            ),
            preset(
                "Pool ball",
                BallStyle {
                    fill: BallFill::Halves,
                    second_color: Color::WHITE,
                    outline: 1.0,
                    outline_color: Color::rgb(0x10, 0x10, 0x10),
                    ..base.clone()
                },
            ),
            preset(
                "Smiley",
                BallStyle {
                    fill: BallFill::Face,
                    second_color: Color::rgb(0x1a, 0x12, 0x10),
                    outline: 2.0,
                    outline_color: Color::rgb(0x1a, 0x12, 0x10),
                    ..base.clone()
                },
            ),
            preset(
                "Spinning star",
                BallStyle {
                    shape: BallShape::Star,
                    fill: BallFill::Radial,
                    second_color: Color::rgb(0xff, 0xf0, 0xa0),
                    spin: 120.0,
                    glow: 0.4,
                    ..base.clone()
                },
            ),
            preset(
                "Arrow",
                BallStyle {
                    shape: BallShape::Triangle,
                    face_motion: true,
                    stretch: 0.3,
                    trail_seconds: 0.25,
                    ..base.clone()
                },
            ),
            preset(
                "Candy stripes",
                BallStyle {
                    fill: BallFill::Stripes,
                    second_color: Color::WHITE,
                    pattern_size: 5.0,
                    turn: 45.0,
                    face_motion: true,
                    ..base.clone()
                },
            ),
            preset(
                "Rainbow jelly",
                BallStyle {
                    fill: BallFill::Radial,
                    second_color: Color::rgb(0xff, 0xff, 0xff),
                    rainbow: 0.25,
                    opacity: 0.8,
                    glow: 0.3,
                    stretch: 0.5,
                    ..base.clone()
                },
            ),
            preset("Hollow ring", BallStyle { shape: BallShape::Donut, glow: 0.3, trail_seconds: 0.3, ..base.clone() }),
            preset(
                "Square block",
                BallStyle {
                    shape: BallShape::Square,
                    roundness: 0.25,
                    fill: BallFill::Linear,
                    second_color: Color::rgb(0x20, 0x10, 0x40),
                    outline: 2.0,
                    outline_color: Color::WHITE,
                    ..base
                },
            ),
        ]
    }

    /// The look closest to how a ball with no style is drawn, using the scene's Look settings, so
    /// switching a ball to its own look starts from what was on screen.
    pub fn like_classic(look: &crate::Look) -> Self {
        Self {
            fill: BallFill::Shiny,
            glow: look.ball_glow,
            trail_seconds: look.trail_seconds,
            trail_width: look.trail_width,
            trail_opacity: look.trail_opacity,
            ..Self::default()
        }
    }

    /// Copies every setting that differs between `before` and `after` into `self`, so an edit
    /// made to one ball's look can be made to other balls' looks too without overwriting the
    /// settings they don't share.
    pub fn copy_changes(&mut self, before: &BallStyle, after: &BallStyle) {
        macro_rules! fields {
            ($($field:ident),*) => {
                $(if before.$field != after.$field {
                    self.$field = after.$field.clone();
                })*
            };
        }
        // Keep this list in step with the struct; `every_field_is_copied` checks it.
        fields!(
            shape,
            roundness,
            fill,
            second_color,
            pattern_size,
            turn,
            spin,
            face_motion,
            outline,
            outline_color,
            glow,
            glow_size,
            opacity,
            stretch,
            rainbow,
            trail_seconds,
            trail_width,
            trail_opacity,
            trail_color
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_field_is_copied() {
        let before = BallStyle::default();
        let after = BallStyle {
            shape: BallShape::Star,
            roundness: 0.9,
            fill: BallFill::Swirl,
            second_color: Color::rgb(1, 2, 3),
            pattern_size: 9.0,
            turn: 12.0,
            spin: 30.0,
            face_motion: true,
            outline: 4.0,
            outline_color: Color::rgb(4, 5, 6),
            glow: 1.5,
            glow_size: 2.0,
            opacity: 0.5,
            stretch: 0.7,
            rainbow: 1.0,
            trail_seconds: 0.4,
            trail_width: 0.2,
            trail_opacity: 0.9,
            trail_color: TrailColor::Outline,
        };
        let mut other = BallStyle::default();
        other.copy_changes(&before, &after);
        assert_eq!(other, after);
    }

    #[test]
    fn only_changed_settings_are_copied() {
        let before = BallStyle::default();
        let after = BallStyle { glow: 0.7, ..before.clone() };
        let mut other = BallStyle { fill: BallFill::Dots, second_color: Color::rgb(9, 9, 9), ..before.clone() };
        other.copy_changes(&before, &after);
        assert_eq!(other, BallStyle { glow: 0.7, fill: BallFill::Dots, second_color: Color::rgb(9, 9, 9), ..before });
    }

    #[test]
    fn styles_survive_a_round_trip_and_old_files_have_none() {
        let mut scene = crate::Scene::default();
        let text = scene.to_ron().unwrap();
        assert!(!text.contains("style"), "scenes without styles must save exactly as before");
        let loaded = crate::Scene::from_ron(&text).unwrap();
        assert!(loaded.spawners.iter().all(|s| s.style().is_none()));
        for preset in BallStyle::presets() {
            *scene.spawners[0].style_mut() = Some(preset.style.clone());
            let back = crate::Scene::from_ron(&scene.to_ron().unwrap()).unwrap();
            assert_eq!(back.spawners[0].style(), Some(&preset.style), "{}", preset.name);
        }
    }
}

//! The HSL / Colour panel: Lightroom's eight colour bands, each with its own Hue, Saturation and
//! Luminance. A colour between two bands is moved by both (`bpe_gfx::grade::maths::band_weights`).

use bpe_core::Settings;

/// The bands' keys, in Lightroom's order.
pub const BANDS: [&str; 8] = ["red", "orange", "yellow", "green", "aqua", "blue", "purple", "magenta"];

/// The HSL / Colour panel's settings: `<band>_hue`, `<band>_saturation`, `<band>_luminance`.
#[derive(Clone, Debug, Default, PartialEq, Settings)]
pub struct HslPanel {
    /// The red colours' hue: up turns them towards orange, down towards magenta.
    #[setting(label = "Red", range(-100, 100), keyframe, pulse, group = "Hue")]
    pub red_hue: f32,
    /// The orange colours' hue: up turns them towards yellow, down towards red.
    #[setting(label = "Orange", range(-100, 100), keyframe, pulse, group = "Hue")]
    pub orange_hue: f32,
    /// The yellow colours' hue: up turns them towards green, down towards orange.
    #[setting(label = "Yellow", range(-100, 100), keyframe, pulse, group = "Hue")]
    pub yellow_hue: f32,
    /// The green colours' hue: up turns them towards aqua, down towards yellow.
    #[setting(label = "Green", range(-100, 100), keyframe, pulse, group = "Hue")]
    pub green_hue: f32,
    /// The aqua colours' hue: up turns them towards blue, down towards green.
    #[setting(label = "Aqua", range(-100, 100), keyframe, pulse, group = "Hue")]
    pub aqua_hue: f32,
    /// The blue colours' hue: up turns them towards purple, down towards aqua.
    #[setting(label = "Blue", range(-100, 100), keyframe, pulse, group = "Hue")]
    pub blue_hue: f32,
    /// The purple colours' hue: up turns them towards magenta, down towards blue.
    #[setting(label = "Purple", range(-100, 100), keyframe, pulse, group = "Hue")]
    pub purple_hue: f32,
    /// The magenta colours' hue: up turns them towards red, down towards purple.
    #[setting(label = "Magenta", range(-100, 100), keyframe, pulse, group = "Hue")]
    pub magenta_hue: f32,
    /// How colourful the red colours are: −100 greys them, +100 doubles their colour.
    #[setting(label = "Red", range(-100, 100), keyframe, pulse, group = "Saturation")]
    pub red_saturation: f32,
    /// How colourful the orange colours are: −100 greys them, +100 doubles their colour.
    #[setting(label = "Orange", range(-100, 100), keyframe, pulse, group = "Saturation")]
    pub orange_saturation: f32,
    /// How colourful the yellow colours are: −100 greys them, +100 doubles their colour.
    #[setting(label = "Yellow", range(-100, 100), keyframe, pulse, group = "Saturation")]
    pub yellow_saturation: f32,
    /// How colourful the green colours are: −100 greys them, +100 doubles their colour.
    #[setting(label = "Green", range(-100, 100), keyframe, pulse, group = "Saturation")]
    pub green_saturation: f32,
    /// How colourful the aqua colours are: −100 greys them, +100 doubles their colour.
    #[setting(label = "Aqua", range(-100, 100), keyframe, pulse, group = "Saturation")]
    pub aqua_saturation: f32,
    /// How colourful the blue colours are: −100 greys them, +100 doubles their colour.
    #[setting(label = "Blue", range(-100, 100), keyframe, pulse, group = "Saturation")]
    pub blue_saturation: f32,
    /// How colourful the purple colours are: −100 greys them, +100 doubles their colour.
    #[setting(label = "Purple", range(-100, 100), keyframe, pulse, group = "Saturation")]
    pub purple_saturation: f32,
    /// How colourful the magenta colours are: −100 greys them, +100 doubles their colour.
    #[setting(label = "Magenta", range(-100, 100), keyframe, pulse, group = "Saturation")]
    pub magenta_saturation: f32,
    /// How bright the red colours are. Greys and dull colours move little or not at all.
    #[setting(label = "Red", range(-100, 100), keyframe, pulse, group = "Luminance")]
    pub red_luminance: f32,
    /// How bright the orange colours are. Greys and dull colours move little or not at all.
    #[setting(label = "Orange", range(-100, 100), keyframe, pulse, group = "Luminance")]
    pub orange_luminance: f32,
    /// How bright the yellow colours are. Greys and dull colours move little or not at all.
    #[setting(label = "Yellow", range(-100, 100), keyframe, pulse, group = "Luminance")]
    pub yellow_luminance: f32,
    /// How bright the green colours are. Greys and dull colours move little or not at all.
    #[setting(label = "Green", range(-100, 100), keyframe, pulse, group = "Luminance")]
    pub green_luminance: f32,
    /// How bright the aqua colours are. Greys and dull colours move little or not at all.
    #[setting(label = "Aqua", range(-100, 100), keyframe, pulse, group = "Luminance")]
    pub aqua_luminance: f32,
    /// How bright the blue colours are. Greys and dull colours move little or not at all.
    #[setting(label = "Blue", range(-100, 100), keyframe, pulse, group = "Luminance")]
    pub blue_luminance: f32,
    /// How bright the purple colours are. Greys and dull colours move little or not at all.
    #[setting(label = "Purple", range(-100, 100), keyframe, pulse, group = "Luminance")]
    pub purple_luminance: f32,
    /// How bright the magenta colours are. Greys and dull colours move little or not at all.
    #[setting(label = "Magenta", range(-100, 100), keyframe, pulse, group = "Luminance")]
    pub magenta_luminance: f32,
}

impl HslPanel {
    /// The panel as the frame carries it.
    pub fn gpu(&self) -> bpe_gfx::GradeHsl {
        let n = |v: f32| if v.is_finite() { v } else { 0.0 };
        let hue = [
            self.red_hue,
            self.orange_hue,
            self.yellow_hue,
            self.green_hue,
            self.aqua_hue,
            self.blue_hue,
            self.purple_hue,
            self.magenta_hue,
        ]
        .map(n);
        let saturation = [
            self.red_saturation,
            self.orange_saturation,
            self.yellow_saturation,
            self.green_saturation,
            self.aqua_saturation,
            self.blue_saturation,
            self.purple_saturation,
            self.magenta_saturation,
        ]
        .map(n);
        let luminance = [
            self.red_luminance,
            self.orange_luminance,
            self.yellow_luminance,
            self.green_luminance,
            self.aqua_luminance,
            self.blue_luminance,
            self.purple_luminance,
            self.magenta_luminance,
        ]
        .map(n);
        bpe_gfx::GradeHsl { hue, saturation, luminance }
    }
}

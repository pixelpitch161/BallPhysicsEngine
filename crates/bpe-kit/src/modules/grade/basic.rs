//! The Basic panel: white balance, tone and presence, as Lightroom Classic names them, with its
//! slider ranges. Texture, Clarity and Dehaze look at the pixels around each pixel (area steps).

use bpe_core::Settings;

/// The Basic panel's settings.
#[derive(Clone, Debug, Default, PartialEq, Settings)]
pub struct BasicPanel {
    /// Warmer (towards yellow) or cooler (towards blue) light: the picture's white moved along the
    /// colour of daylight and lamps. A grey stays as bright.
    #[setting(label = "Temp", range(-100, 100), keyframe, pulse, group = "White balance")]
    pub temp: f32,
    /// Towards magenta or towards green.
    #[setting(label = "Tint", range(-100, 100), keyframe, pulse, group = "White balance")]
    pub tint: f32,
    /// Brighter or darker, in stops: +1 is twice the light.
    #[setting(label = "Exposure", range(-5, 5), keyframe, pulse, group = "Tone")]
    pub exposure: f32,
    /// Darks darker and lights lighter around the middle grey, or the other way.
    #[setting(label = "Contrast", range(-100, 100), keyframe, pulse, group = "Tone")]
    pub contrast: f32,
    /// The bright parts darker or lighter. Down brings glows brighter than white back into range.
    #[setting(label = "Highlights", range(-100, 100), keyframe, pulse, group = "Tone")]
    pub highlights: f32,
    /// The dark parts lighter or darker.
    #[setting(label = "Shadows", range(-100, 100), keyframe, pulse, group = "Tone")]
    pub shadows: f32,
    /// Where white is: up clips more to white, down pulls white (and what is past it) down.
    #[setting(label = "Whites", range(-100, 100), keyframe, pulse, group = "Tone")]
    pub whites: f32,
    /// Where black is: down crushes more to black, up lifts black to a grey.
    #[setting(label = "Blacks", range(-100, 100), keyframe, pulse, group = "Tone")]
    pub blacks: f32,
    /// Medium-sized detail (surfaces, patterns) stronger, or smoothed away.
    #[setting(label = "Texture", range(-100, 100), keyframe, pulse, group = "Presence")]
    pub texture: f32,
    /// More or less contrast between neighbouring parts, mostly in the midtones; down softens.
    #[setting(label = "Clarity", range(-100, 100), keyframe, pulse, group = "Presence")]
    pub clarity: f32,
    /// Takes a veil of haze or glow off the picture, or lays a fog over it.
    #[setting(label = "Dehaze", range(-100, 100), keyframe, pulse, group = "Presence")]
    pub dehaze: f32,
    /// More or less colour, mostly in dull colours, sparing skin.
    #[setting(label = "Vibrance", range(-100, 100), keyframe, pulse, group = "Presence")]
    pub vibrance: f32,
    /// More or less colour in every colour alike: −100 is black and white.
    #[setting(label = "Saturation", range(-100, 100), keyframe, pulse, group = "Presence")]
    pub saturation: f32,
}

impl BasicPanel {
    /// The panel as the frame carries it.
    pub fn gpu(&self) -> bpe_gfx::GradeBasic {
        let n = |v: f32| if v.is_finite() { v } else { 0.0 };
        bpe_gfx::GradeBasic {
            temp: n(self.temp),
            tint: n(self.tint),
            exposure: n(self.exposure),
            contrast: n(self.contrast),
            highlights: n(self.highlights),
            shadows: n(self.shadows),
            whites: n(self.whites),
            blacks: n(self.blacks),
            texture: n(self.texture),
            clarity: n(self.clarity),
            dehaze: n(self.dehaze),
            vibrance: n(self.vibrance),
            saturation: n(self.saturation),
        }
    }
}

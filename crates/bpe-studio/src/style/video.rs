//! The video's own colours on screen (the design system's one exception: "the video owns
//! colour"): the grade's colour wheels and the colour strips under its colour sliders.

use eframe::egui::Color32;

/// A linear-light colour of the video as the screen shows it: clamped to 0..1, sRGB-encoded.
pub fn video(c: [f64; 3]) -> Color32 {
    let byte = |v: f64| {
        let v = if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
        let e = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
        (e * 255.0).round() as u8
    };
    Color32::from_rgb(byte(c[0]), byte(c[1]), byte(c[2])) // scene colour
}

/// A colour of the video already encoded for the screen (0..1 each, clamped): the Vectorscope's
/// colours.
pub fn scope_hue(c: [f64; 3]) -> Color32 {
    let byte = |v: f64| (if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 } * 255.0).round() as u8;
    Color32::from_rgb(byte(c[0]), byte(c[1]), byte(c[2])) // scene colour
}

/// A scene colour (sRGB bytes) on screen.
pub fn video_bytes(c: bpe_core::Color) -> Color32 {
    Color32::from_rgb(c.r, c.g, c.b) // scene colour
}

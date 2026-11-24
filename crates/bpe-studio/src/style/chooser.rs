//! The New scene chooser's little pictures. They show the scenes themselves, so they are in
//! scene colours (the design system's one exception: "the video owns colour").

use eframe::egui::Color32;

/// A Balls scene's dark ground.
pub const BALLS_GROUND: Color32 = Color32::from_rgb(0x10, 0x12, 0x1c); // scene colour
/// The arena's ring.
pub const BALLS_RING: Color32 = Color32::from_rgb(0xdc, 0xe2, 0xf2); // scene colour
/// The balls, front to back.
pub const BALLS: [Color32; 3] = [
    Color32::from_rgb(0xff, 0x4f, 0x9a), // scene colour
    Color32::from_rgb(0x4f, 0xc3, 0xff), // scene colour
    Color32::from_rgb(0xff, 0xd2, 0x4a), // scene colour
];
/// What is behind a Water scene's course.
pub const WATER_BACK: Color32 = Color32::from_rgb(0x12, 0x16, 0x1d); // scene colour

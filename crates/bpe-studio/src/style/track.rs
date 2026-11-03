//! Track colours: the one hue each thing carries through the tree, the preview, the timeline and
//! the inspector.

use super::tokens::{PANEL, TEXT};
use bpe_core::{Scene, ThingId, TrackColour};
use eframe::egui::Color32;

/// The colour on screen for `c`.
pub fn track_color(c: TrackColour) -> Color32 {
    match c {
        TrackColour::Pink => Color32::from_rgb(0xff, 0x6f, 0xa8),
        TrackColour::Cyan => Color32::from_rgb(0x4f, 0xc3, 0xf0),
        TrackColour::Lime => Color32::from_rgb(0xa8, 0xd8, 0x4a),
        TrackColour::Orange => Color32::from_rgb(0xff, 0xaa, 0x45),
        TrackColour::Violet => Color32::from_rgb(0xa9, 0x8b, 0xff),
        TrackColour::Yellow => Color32::from_rgb(0xf2, 0xd2, 0x4b),
        TrackColour::Red => Color32::from_rgb(0xff, 0x70, 0x70),
        TrackColour::Teal => Color32::from_rgb(0x3f, 0xd6, 0xb5),
    }
}

/// `color` mixed 22% over the panel grey: the fill of a timeline clip.
pub fn mix_over_panel(color: Color32) -> Color32 {
    let mix = |p: u8, c: u8| (p as f32 + (c as f32 - p as f32) * 0.22).round() as u8;
    Color32::from_rgb(mix(PANEL.r(), color.r()), mix(PANEL.g(), color.g()), mix(PANEL.b(), color.b()))
}

/// The colour that marks `id` in the editor: its track colour, or `TEXT` for a thing without one
/// (the scene settings) or one that is gone.
pub fn colour_of(scene: &Scene, id: ThingId) -> Color32 {
    scene.things.get(&id).and_then(|t| t.colour).map_or(TEXT, track_color)
}

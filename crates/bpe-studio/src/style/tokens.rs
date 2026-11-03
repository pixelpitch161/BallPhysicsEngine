//! The Ball Studio Design System's tokens: every grey, size and corner the editor uses. Named
//! like the tokens in the Design System page, so the two can be compared line by line.

use eframe::egui::Color32;

/// Window background, status bar and the wells of text and value fields.
pub const GROUND: Color32 = Color32::from_rgb(0x1e, 0x1e, 0x1e);
/// Toolbar, tree, inspector and timeline panels; the base clips are tinted over.
pub const PANEL: Color32 = Color32::from_rgb(0x28, 0x28, 0x28);
/// The neutral surround behind the 9:16 preview.
pub const STAGE: Color32 = Color32::from_rgb(0x2b, 0x2b, 0x2b);
/// Dropdowns, small buttons and menus that sit above a panel.
pub const RAISED: Color32 = Color32::from_rgb(0x33, 0x33, 0x33);
/// The selected tree row's fill.
pub const SELECTED: Color32 = Color32::from_rgb(0x38, 0x38, 0x38);
/// Hover fill for buttons and menu items.
pub const HOVER: Color32 = Color32::from_rgb(0x3a, 0x3a, 0x3a);
/// Dividers between panels, sections and timeline groups.
pub const LINE_SOFT: Color32 = Color32::from_rgb(0x38, 0x38, 0x38);
/// Control outlines, field borders and slider rails.
pub const LINE: Color32 = Color32::from_rgb(0x44, 0x44, 0x44);
/// Disabled text, the eye icon, a switch that is off. Never for text that must be read.
pub const FAINT: Color32 = Color32::from_rgb(0x77, 0x77, 0x77);
/// Setting labels, hints, lane names and counts.
pub const MUTED: Color32 = Color32::from_rgb(0xa0, 0xa0, 0xa0);
/// Values, names and body text; also the fill of on and primary buttons.
pub const TEXT: Color32 = Color32::from_rgb(0xe2, 0xe2, 0xe2);
/// The playhead, the selected row's name, keyframe diamonds and the slider knob.
pub const TEXT_STRONG: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
/// Text on a `TEXT` fill.
pub const ON_LIGHT: Color32 = Color32::from_rgb(0x1e, 0x1e, 0x1e);
/// The part of a slider's rail left of its knob.
pub const SLIDER_FILL: Color32 = Color32::from_rgb(0xbd, 0xbd, 0xbd);
/// A drag handle in the preview that isn't selected: white, mostly see-through.
pub const FAINT_MARK: Color32 = Color32::from_rgba_premultiplied(0x66, 0x66, 0x66, 0x73);
/// The outline that keeps a handle readable over a bright video.
pub const OUTLINE: Color32 = Color32::from_rgb(0x00, 0x00, 0x00);
/// The world grid over the preview: the faint lines and the middle ones.
pub const GRID_LINE: Color32 = Color32::from_rgba_premultiplied(0x1c, 0x1c, 0x1c, 0x1c);
pub const GRID_MIDDLE: Color32 = Color32::from_rgba_premultiplied(0x50, 0x50, 0x50, 0x50);
/// The camera's path in the world view.
pub const PATH_LINE: Color32 = Color32::from_rgba_premultiplied(0x3c, 0x3c, 0x3c, 0x3c);
/// The dark pill behind a word drawn over the preview.
pub const PILL: Color32 = Color32::from_rgba_premultiplied(0x00, 0x00, 0x00, 0xbe);
/// A picture drawn as it is (no tint).
pub const UNTINTED: Color32 = Color32::WHITE;

/// Buttons, fields and setting rows.
pub const CONTROL_H: f32 = 26.0;
pub const TREE_ROW_H: f32 = 30.0;
pub const LANE_H: f32 = 23.0;
/// A timeline group's header.
pub const GROUP_H: f32 = 22.0;
pub const TOOLBAR_H: f32 = 40.0;
pub const STATUS_H: f32 = 24.0;
pub const TREE_W: f32 = 230.0;
pub const INSPECTOR_W: f32 = 370.0;
/// The inspector's label column.
pub const LABEL_W: f32 = 108.0;
/// The timeline's lane name column.
pub const LANE_NAME_W: f32 = 120.0;
/// The default corner.
pub const RADIUS: u8 = 2;
/// Tree rows only.
pub const RADIUS_ROW: u8 = 8;
